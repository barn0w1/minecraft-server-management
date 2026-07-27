use std::{
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

use thiserror::Error;
use tokio::{fs, process::Command, time::timeout};
use uuid::Uuid;

use crate::{
    config::RemoteAgentConfig,
    domain::{ComputeInstanceId, UnixTimestampMillis},
};

const MAX_CSR_BYTES: usize = 32 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 64 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 8192;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const SERVER_CERTIFICATE_MINIMUM_REMAINING_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct SignedAgentCertificate {
    pub certificate_chain_pem: String,
    pub leaf_der: Vec<u8>,
    pub expires_at: UnixTimestampMillis,
}

#[derive(Debug, Clone)]
pub struct AgentCertificateAuthority {
    server_certificate: PathBuf,
    server_private_key: PathBuf,
    ca_certificate: PathBuf,
    ca_private_key: PathBuf,
    work_directory: PathBuf,
    openssl_binary: PathBuf,
    trust_domain: String,
    tls_server_name: String,
    validity: Duration,
}

impl AgentCertificateAuthority {
    #[must_use]
    pub fn new(config: &RemoteAgentConfig) -> Self {
        Self {
            server_certificate: config.tls_certificate.clone(),
            server_private_key: config.tls_private_key.clone(),
            ca_certificate: config.client_ca_certificate.clone(),
            ca_private_key: config.client_ca_private_key.clone(),
            work_directory: config.certificate_work_directory.clone(),
            openssl_binary: config.openssl_binary.clone(),
            trust_domain: config.trust_domain.clone(),
            tls_server_name: config.tls_server_name.clone(),
            validity: config.certificate_validity,
        }
    }

    pub async fn preflight(&self) -> Result<(), AgentCertificateError> {
        self.verify_key_pair(
            &self.server_certificate,
            &self.server_private_key,
            "remote TLS server certificate",
        )
        .await?;
        self.verify_key_pair(
            &self.ca_certificate,
            &self.ca_private_key,
            "agent client CA certificate",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("x509")
                .arg("-checkend")
                .arg(SERVER_CERTIFICATE_MINIMUM_REMAINING_SECONDS.to_string())
                .arg("-noout")
                .arg("-in")
                .arg(&self.server_certificate),
            "check remote TLS server certificate lifetime",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("x509")
                .arg("-checkhost")
                .arg(&self.tls_server_name)
                .arg("-noout")
                .arg("-in")
                .arg(&self.server_certificate),
            "check remote TLS server certificate name",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("x509")
                .arg("-checkend")
                .arg(self.validity.as_secs().to_string())
                .arg("-noout")
                .arg("-in")
                .arg(&self.ca_certificate),
            "check agent client CA certificate lifetime",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("verify")
                .arg("-CAfile")
                .arg(&self.ca_certificate)
                .arg(&self.ca_certificate),
            "verify agent client CA certificate",
        )
        .await?;
        Ok(())
    }

    async fn verify_key_pair(
        &self,
        certificate: &Path,
        private_key: &Path,
        description: &'static str,
    ) -> Result<(), AgentCertificateError> {
        let certificate_public_key = self
            .run_stdout(
                Command::new(&self.openssl_binary)
                    .arg("x509")
                    .arg("-in")
                    .arg(certificate)
                    .arg("-pubkey")
                    .arg("-noout"),
                description,
            )
            .await?;
        let private_public_key = self
            .run_stdout(
                Command::new(&self.openssl_binary)
                    .arg("pkey")
                    .arg("-in")
                    .arg(private_key)
                    .arg("-pubout"),
                description,
            )
            .await?;
        if certificate_public_key != private_public_key {
            return Err(AgentCertificateError::KeyMismatch(description));
        }
        Ok(())
    }

    pub async fn sign(
        &self,
        compute_instance_id: ComputeInstanceId,
        certificate_signing_request_pem: &str,
        issued_at: UnixTimestampMillis,
    ) -> Result<SignedAgentCertificate, AgentCertificateError> {
        validate_csr(certificate_signing_request_pem)?;
        ensure_private_directory(&self.work_directory).await?;
        let request_directory = self
            .work_directory
            .join(format!("{}-{}", compute_instance_id, Uuid::new_v4()));
        ensure_private_directory(&request_directory).await?;
        let result = self
            .sign_in_directory(
                compute_instance_id,
                certificate_signing_request_pem,
                issued_at,
                &request_directory,
            )
            .await;
        if let Err(error) = fs::remove_dir_all(&request_directory).await
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(path = %request_directory.display(), %error, "failed to remove temporary agent certificate directory");
        }
        result
    }

    async fn sign_in_directory(
        &self,
        compute_instance_id: ComputeInstanceId,
        certificate_signing_request_pem: &str,
        issued_at: UnixTimestampMillis,
        directory: &Path,
    ) -> Result<SignedAgentCertificate, AgentCertificateError> {
        let csr_path = directory.join("request.pem");
        let extension_path = directory.join("extensions.cnf");
        let leaf_path = directory.join("certificate.pem");
        let der_path = directory.join("certificate.der");
        write_private(&csr_path, certificate_signing_request_pem.as_bytes()).await?;
        let identity = format!(
            "spiffe://{}/mcserver/compute/{}",
            self.trust_domain, compute_instance_id
        );
        let extensions = format!(
            "[agent_cert]\n\
             basicConstraints=critical,CA:FALSE\n\
             keyUsage=critical,digitalSignature\n\
             extendedKeyUsage=critical,clientAuth\n\
             subjectAltName=URI:{identity}\n\
             subjectKeyIdentifier=hash\n\
             authorityKeyIdentifier=keyid,issuer\n"
        );
        write_private(&extension_path, extensions.as_bytes()).await?;

        self.run(
            Command::new(&self.openssl_binary)
                .arg("req")
                .arg("-in")
                .arg(&csr_path)
                .arg("-noout")
                .arg("-verify"),
            "verify agent certificate signing request",
        )
        .await?;

        let validity_days = self.validity.as_secs().div_ceil(24 * 60 * 60).max(1);
        let serial = format!("0x{}", compute_instance_id.as_uuid().simple());
        let subject = format!("/CN={compute_instance_id}");
        self.run(
            Command::new(&self.openssl_binary)
                .arg("x509")
                .arg("-req")
                .arg("-in")
                .arg(&csr_path)
                .arg("-CA")
                .arg(&self.ca_certificate)
                .arg("-CAkey")
                .arg(&self.ca_private_key)
                .arg("-set_serial")
                .arg(serial)
                .arg("-days")
                .arg(validity_days.to_string())
                .arg("-sha256")
                .arg("-copy_extensions")
                .arg("none")
                .arg("-subj")
                .arg(subject)
                .arg("-extfile")
                .arg(&extension_path)
                .arg("-extensions")
                .arg("agent_cert")
                .arg("-out")
                .arg(&leaf_path),
            "sign agent client certificate",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("verify")
                .arg("-purpose")
                .arg("sslclient")
                .arg("-CAfile")
                .arg(&self.ca_certificate)
                .arg(&leaf_path),
            "verify signed agent client certificate",
        )
        .await?;
        self.run(
            Command::new(&self.openssl_binary)
                .arg("x509")
                .arg("-in")
                .arg(&leaf_path)
                .arg("-outform")
                .arg("DER")
                .arg("-out")
                .arg(&der_path),
            "encode agent client certificate",
        )
        .await?;

        let leaf_pem = read_bounded(&leaf_path, MAX_CERTIFICATE_BYTES).await?;
        let ca_pem = read_bounded(&self.ca_certificate, MAX_CERTIFICATE_BYTES).await?;
        let leaf_der = read_bounded(&der_path, MAX_CERTIFICATE_BYTES).await?;
        let mut certificate_chain_pem = String::from_utf8(leaf_pem)?;
        if !certificate_chain_pem.ends_with('\n') {
            certificate_chain_pem.push('\n');
        }
        let ca_pem = String::from_utf8(ca_pem)?;
        certificate_chain_pem.push_str(&ca_pem);
        if certificate_chain_pem.len() > MAX_CERTIFICATE_BYTES {
            return Err(AgentCertificateError::CertificateTooLarge);
        }
        let validity_ms = i64::try_from(self.validity.as_millis())
            .map_err(|_| AgentCertificateError::TimestampOutOfRange)?;
        let expires_at_ms = issued_at
            .as_millis()
            .checked_add(validity_ms)
            .ok_or(AgentCertificateError::TimestampOutOfRange)?;
        let expires_at = UnixTimestampMillis::from_millis(expires_at_ms)
            .map_err(|_| AgentCertificateError::TimestampOutOfRange)?;
        Ok(SignedAgentCertificate {
            certificate_chain_pem,
            leaf_der,
            expires_at,
        })
    }

    async fn run(&self, command: &mut Command, description: &'static str) -> Result<Output, AgentCertificateError> {
        let output = timeout(COMMAND_TIMEOUT, command.output())
            .await
            .map_err(|_| AgentCertificateError::CommandTimeout(description))??;
        if output.status.success() {
            Ok(output)
        } else {
            Err(AgentCertificateError::CommandFailed {
                description,
                status: output.status.code(),
                stderr: bounded_diagnostic(&output.stderr),
            })
        }
    }

    async fn run_stdout(
        &self,
        command: &mut Command,
        description: &'static str,
    ) -> Result<Vec<u8>, AgentCertificateError> {
        Ok(self.run(command, description).await?.stdout)
    }
}

fn validate_csr(value: &str) -> Result<(), AgentCertificateError> {
    if value.len() > MAX_CSR_BYTES {
        return Err(AgentCertificateError::CsrTooLarge);
    }
    if value.contains('\0')
        || !value.contains("-----BEGIN CERTIFICATE REQUEST-----")
        || !value.contains("-----END CERTIFICATE REQUEST-----")
    {
        return Err(AgentCertificateError::InvalidCsr);
    }
    Ok(())
}

async fn ensure_private_directory(path: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(path).await?;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

async fn write_private(path: &Path, value: &[u8]) -> Result<(), io::Error> {
    fs::write(path, value).await?;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

async fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, AgentCertificateError> {
    let metadata = fs::metadata(path).await?;
    if metadata.len() > maximum as u64 {
        return Err(AgentCertificateError::FileTooLarge {
            path: path.to_path_buf(),
            maximum,
        });
    }
    Ok(fs::read(path).await?)
}

fn bounded_diagnostic(input: &[u8]) -> String {
    let truncated = input.len() > MAX_DIAGNOSTIC_BYTES;
    let input = &input[..input.len().min(MAX_DIAGNOSTIC_BYTES)];
    let mut value = String::from_utf8_lossy(input).trim().to_owned();
    if truncated {
        value.push_str(" …[truncated]");
    }
    value
}

#[derive(Debug, Error)]
pub enum AgentCertificateError {
    #[error("agent certificate I/O failed")]
    Io(#[from] io::Error),
    #[error("agent certificate command {0} timed out")]
    CommandTimeout(&'static str),
    #[error("agent certificate command {description} failed with status {status:?}: {stderr}")]
    CommandFailed {
        description: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("agent certificate signing request is invalid")]
    InvalidCsr,
    #[error("agent certificate signing request exceeds the supported size")]
    CsrTooLarge,
    #[error("agent certificate output exceeds the supported size")]
    CertificateTooLarge,
    #[error("agent certificate file {path} exceeds {maximum} bytes")]
    FileTooLarge { path: PathBuf, maximum: usize },
    #[error("agent certificate PEM is not UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("{0} and private key do not match")]
    KeyMismatch(&'static str),
    #[error("agent certificate expiration is outside the supported timestamp range")]
    TimestampOutOfRange,
}
