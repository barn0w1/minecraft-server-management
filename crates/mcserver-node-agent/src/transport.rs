use std::{
    collections::BTreeMap,
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
    sync::Arc,
    time::Duration,
};

use mcserver_protocol::{
    json_rpc::{self, ErrorObject, Request, Response},
    node_agent::{
        self, AgentInspectParams, CleanupInstanceParams, EnrollParams, EnrollResult,
        RegisterParams, RegisterResult, RestoreDataParams, ShutdownResult, SnapshotDataParams,
        StartServerParams, StopServerParams, method,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::Command,
    sync::RwLock,
};
use tokio_rustls::{TlsConnector, rustls};
use tracing::{info, warn};

use crate::{
    cancellation::CancellationToken,
    config::Config,
    executor::{AgentExecutor, ExecutorError},
};

const CONNECTION_TOKEN_FILE_NAME: &str = "connection-token";
const CLIENT_PRIVATE_KEY_FILE_NAME: &str = "client-private-key.pem";
const CLIENT_CSR_FILE_NAME: &str = "client-request.pem";
const CLIENT_CERTIFICATE_FILE_NAME: &str = "client-certificate-chain.pem";
const MAX_CONNECTION_TOKEN_CHARS: usize = 256;
const MAX_RUNTIME_ENVIRONMENT_ENTRIES: usize = 64;
const MAX_RUNTIME_ENVIRONMENT_VALUE_CHARS: usize = 16 * 1024;
const REQUIRED_REMOTE_RUNTIME_ENVIRONMENT_KEYS: [&str; 5] = [
    "RESTIC_PASSWORD",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_DEFAULT_REGION",
];
const OPENSSL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DIAGNOSTIC_BYTES: usize = 8192;

pub async fn run(
    config: Config,
    executor: AgentExecutor,
    runtime_environment: Arc<RwLock<BTreeMap<String, String>>>,
    cancellation: CancellationToken,
) -> Result<(), TransportError> {
    let stable_credentials =
        config.tls.is_some() && stable_client_credentials_exist(&config).await?;
    let mut connection_token = if stable_credentials {
        load_persisted_connection_token(&config).await?
    } else {
        validate_connection_token(&config.connection_token)?;
        config.connection_token.clone()
    };
    if config.tls.is_some() && !stable_credentials {
        let result = enroll(&config, &connection_token, cancellation.child_token()).await?;
        validate_connection_token(&result.connection_token)?;
        persist_client_certificate(&config, &result.client_certificate_chain_pem).await?;
        persist_connection_token(&config, &result.connection_token).await?;
        connection_token = result.connection_token;
        info!(
            compute_instance_id = %config.compute_instance_id,
            "persisted mTLS client certificate and reconnect credential"
        );
    }

    let mut backoff = config.reconnect_min;
    loop {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        match run_session(
            &config,
            &connection_token,
            &executor,
            Arc::clone(&runtime_environment),
            cancellation.child_token(),
        )
        .await
        {
            Ok(SessionOutcome::Shutdown) => return Ok(()),
            Ok(SessionOutcome::Disconnected) => {
                backoff = config.reconnect_min;
                warn!("control-plane connection closed; reconnecting");
            }
            Err(error) => warn!(%error, "control-plane session failed; reconnecting"),
        }
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = std::cmp::min(
            backoff.checked_mul(2).unwrap_or(config.reconnect_max),
            config.reconnect_max,
        );
    }
}

async fn enroll(
    config: &Config,
    enrollment_token: &str,
    cancellation: CancellationToken,
) -> Result<EnrollResult, TransportError> {
    validate_connection_token(enrollment_token)?;
    let csr = load_or_create_csr(config).await?;
    let stream = tokio::select! {
        () = cancellation.cancelled() => return Err(TransportError::Disconnected),
        connected = connect(config, false) => connected?,
    };
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let request_id = json!(1);
    write_value(
        &mut writer,
        &json!({
            "jsonrpc": json_rpc::VERSION,
            "method": method::AGENT_ENROLL,
            "params": EnrollParams {
                protocol_version: node_agent::PROTOCOL_VERSION,
                compute_instance_id: config.compute_instance_id,
                enrollment_token: enrollment_token.to_owned(),
                certificate_signing_request_pem: csr,
            },
            "id": request_id,
        }),
    )
    .await?;
    let response = read_response(&mut reader, config.max_frame_bytes).await?;
    if response.id != request_id {
        return Err(TransportError::Protocol(
            "enrollment response id mismatch".to_owned(),
        ));
    }
    response_result(response)
}

async fn run_session(
    config: &Config,
    connection_token: &str,
    executor: &AgentExecutor,
    runtime_environment: Arc<RwLock<BTreeMap<String, String>>>,
    cancellation: CancellationToken,
) -> Result<SessionOutcome, TransportError> {
    let stream = tokio::select! {
        () = cancellation.cancelled() => return Ok(SessionOutcome::Disconnected),
        connected = connect(config, config.tls.is_some()) => connected?,
    };
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let registration_id = json!(1);
    write_value(
        &mut writer,
        &json!({
            "jsonrpc": json_rpc::VERSION,
            "method": method::AGENT_REGISTER,
            "params": RegisterParams {
                protocol_version: node_agent::PROTOCOL_VERSION,
                compute_instance_id: config.compute_instance_id,
                connection_token: connection_token.to_owned(),
            },
            "id": registration_id,
        }),
    )
    .await?;
    let registration = read_response(&mut reader, config.max_frame_bytes).await?;
    if registration.id != registration_id {
        return Err(TransportError::Protocol(
            "registration response id mismatch".to_owned(),
        ));
    }
    let result = response_result::<RegisterResult>(registration)?;
    if !result.accepted {
        return Err(TransportError::RegistrationRejected);
    }
    validate_runtime_environment(&result.runtime_environment, config.tls.is_some())?;
    *runtime_environment.write().await = result.runtime_environment;
    info!(
        compute_instance_id = %config.compute_instance_id,
        mtls = config.tls.is_some(),
        "registered with control plane"
    );

    loop {
        let request = tokio::select! {
            () = cancellation.cancelled() => return Ok(SessionOutcome::Disconnected),
            request = read_request(&mut reader, config.max_frame_bytes) => match request {
                Ok(request) => request,
                Err(TransportError::Disconnected) => return Ok(SessionOutcome::Disconnected),
                Err(error) => return Err(error),
            }
        };
        let Some(response_id) = request.id.response_id() else {
            continue;
        };
        let (response, shutdown) = dispatch(executor, request, response_id).await;
        write_value(&mut writer, &serde_json::to_value(response)?).await?;
        if shutdown {
            return Ok(SessionOutcome::Shutdown);
        }
    }
}

async fn load_persisted_connection_token(config: &Config) -> Result<String, TransportError> {
    let path = connection_token_path(config);
    let token = tokio::fs::read_to_string(&path)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: path.clone(),
            source,
        })?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    validate_connection_token(&token)?;
    Ok(token)
}

async fn persist_connection_token(config: &Config, token: &str) -> Result<(), TransportError> {
    validate_connection_token(token)?;
    persist_private_file(
        &config.state_directory,
        &connection_token_path(config),
        "connection-token.tmp",
        token.as_bytes(),
    )
    .await
}

async fn persist_client_certificate(
    config: &Config,
    certificate: &str,
) -> Result<(), TransportError> {
    if certificate.contains('\0')
        || !certificate.contains("-----BEGIN CERTIFICATE-----")
        || certificate.len() > 64 * 1024
    {
        return Err(TransportError::TlsConfiguration(
            "issued client certificate chain is invalid".to_owned(),
        ));
    }
    persist_private_file(
        &config.state_directory,
        &client_certificate_path(config),
        "client-certificate-chain.tmp",
        certificate.as_bytes(),
    )
    .await
}

async fn persist_private_file(
    directory: &Path,
    destination: &Path,
    temporary_name: &str,
    value: &[u8],
) -> Result<(), TransportError> {
    ensure_private_directory(directory).await?;
    let temporary = directory.join(temporary_name);
    tokio::fs::write(&temporary, value)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    let file =
        tokio::fs::File::open(&temporary)
            .await
            .map_err(|source| TransportError::CredentialIo {
                path: temporary.clone(),
                source,
            })?;
    file.sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.clone(),
            source,
        })?;
    tokio::fs::rename(&temporary, destination)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: destination.to_path_buf(),
            source,
        })?;
    let directory_file =
        tokio::fs::File::open(directory)
            .await
            .map_err(|source| TransportError::CredentialIo {
                path: directory.to_path_buf(),
                source,
            })?;
    directory_file
        .sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: directory.to_path_buf(),
            source,
        })?;
    Ok(())
}

async fn finalize_private_file(
    directory: &Path,
    temporary: &Path,
    destination: &Path,
) -> Result<(), TransportError> {
    tokio::fs::set_permissions(temporary, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.to_path_buf(),
            source,
        })?;
    let file =
        tokio::fs::File::open(temporary)
            .await
            .map_err(|source| TransportError::CredentialIo {
                path: temporary.to_path_buf(),
                source,
            })?;
    file.sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: temporary.to_path_buf(),
            source,
        })?;
    tokio::fs::rename(temporary, destination)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: destination.to_path_buf(),
            source,
        })?;
    let directory_file =
        tokio::fs::File::open(directory)
            .await
            .map_err(|source| TransportError::CredentialIo {
                path: directory.to_path_buf(),
                source,
            })?;
    directory_file
        .sync_all()
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: directory.to_path_buf(),
            source,
        })?;
    Ok(())
}

async fn ensure_private_directory(path: &Path) -> Result<(), TransportError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: path.to_path_buf(),
            source,
        })?;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .await
        .map_err(|source| TransportError::CredentialIo {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(())
}

async fn stable_client_credentials_exist(config: &Config) -> Result<bool, TransportError> {
    Ok(path_is_file(&connection_token_path(config)).await?
        && path_is_file(&client_private_key_path(config)).await?
        && path_is_file(&client_certificate_path(config)).await?)
}

async fn path_is_file(path: &Path) -> Result<bool, TransportError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(TransportError::CredentialIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

async fn load_or_create_csr(config: &Config) -> Result<String, TransportError> {
    ensure_private_directory(&config.state_directory).await?;
    let key_path = client_private_key_path(config);
    if !path_is_file(&key_path).await? {
        let temporary = config.state_directory.join("client-private-key.tmp");
        run_openssl(
            Command::new(&config.openssl_binary)
                .arg("genpkey")
                .arg("-algorithm")
                .arg("EC")
                .arg("-pkeyopt")
                .arg("ec_paramgen_curve:P-256")
                .arg("-out")
                .arg(&temporary),
            "generate agent client private key",
        )
        .await?;
        finalize_private_file(&config.state_directory, &temporary, &key_path).await?;
    }
    let csr_path = client_csr_path(config);
    if !path_is_file(&csr_path).await? {
        let temporary = config.state_directory.join("client-request.tmp");
        run_openssl(
            Command::new(&config.openssl_binary)
                .arg("req")
                .arg("-new")
                .arg("-key")
                .arg(&key_path)
                .arg("-subj")
                .arg(format!("/CN={}", config.compute_instance_id))
                .arg("-out")
                .arg(&temporary),
            "generate agent certificate signing request",
        )
        .await?;
        finalize_private_file(&config.state_directory, &temporary, &csr_path).await?;
    }
    let csr = tokio::fs::read_to_string(&csr_path).await?;
    if csr.len() > 32 * 1024
        || !csr.contains("-----BEGIN CERTIFICATE REQUEST-----")
        || !csr.contains("-----END CERTIFICATE REQUEST-----")
    {
        return Err(TransportError::InvalidCertificateSigningRequest);
    }
    Ok(csr)
}

async fn run_openssl(
    command: &mut Command,
    description: &'static str,
) -> Result<Output, TransportError> {
    let output = tokio::time::timeout(OPENSSL_TIMEOUT, command.output())
        .await
        .map_err(|_| TransportError::OpenSslTimeout(description))??;
    if output.status.success() {
        Ok(output)
    } else {
        Err(TransportError::OpenSslFailed {
            description,
            status: output.status.code(),
            stderr: bounded_diagnostic(&output.stderr),
        })
    }
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

fn connection_token_path(config: &Config) -> PathBuf {
    config.state_directory.join(CONNECTION_TOKEN_FILE_NAME)
}

fn client_private_key_path(config: &Config) -> PathBuf {
    config.state_directory.join(CLIENT_PRIVATE_KEY_FILE_NAME)
}

fn client_csr_path(config: &Config) -> PathBuf {
    config.state_directory.join(CLIENT_CSR_FILE_NAME)
}

fn client_certificate_path(config: &Config) -> PathBuf {
    config.state_directory.join(CLIENT_CERTIFICATE_FILE_NAME)
}

fn validate_connection_token(token: &str) -> Result<(), TransportError> {
    if token.trim().is_empty()
        || token.contains('\0')
        || token.chars().count() > MAX_CONNECTION_TOKEN_CHARS
    {
        return Err(TransportError::InvalidConnectionToken);
    }
    Ok(())
}

fn validate_runtime_environment(
    values: &BTreeMap<String, String>,
    require_remote_storage_credentials: bool,
) -> Result<(), TransportError> {
    if values.len() > MAX_RUNTIME_ENVIRONMENT_ENTRIES {
        return Err(TransportError::InvalidRuntimeEnvironment);
    }
    for (key, value) in values {
        if !(key.starts_with("RESTIC_") || key.starts_with("AWS_"))
            || !key
                .bytes()
                .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
            || matches!(
                key.as_str(),
                "RESTIC_REPOSITORY" | "RESTIC_PASSWORD_FILE" | "RESTIC_PASSWORD_COMMAND"
            )
            || value.contains('\0')
            || value.chars().count() > MAX_RUNTIME_ENVIRONMENT_VALUE_CHARS
        {
            return Err(TransportError::InvalidRuntimeEnvironment);
        }
    }
    if require_remote_storage_credentials
        && REQUIRED_REMOTE_RUNTIME_ENVIRONMENT_KEYS
            .iter()
            .any(|key| values.get(*key).is_none_or(|value| value.trim().is_empty()))
    {
        return Err(TransportError::InvalidRuntimeEnvironment);
    }
    Ok(())
}

type BoxedTransport = Box<dyn TransportStream>;

trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> TransportStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

async fn connect(
    config: &Config,
    use_client_identity: bool,
) -> Result<BoxedTransport, TransportError> {
    let stream = TcpStream::connect(config.control_plane_address.as_str()).await?;
    stream.set_nodelay(true)?;
    let Some(tls) = config.tls.as_ref() else {
        return Ok(Box::new(stream));
    };
    let ca_pem = tokio::fs::read(&tls.ca_certificate).await?;
    let mut reader = std::io::BufReader::new(ca_pem.as_slice());
    let certificates = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certificates.is_empty() {
        return Err(TransportError::TlsConfiguration(
            "CA certificate file contains no certificates".to_owned(),
        ));
    }
    let mut roots = rustls::RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(certificate)
            .map_err(|error| TransportError::TlsConfiguration(error.to_string()))?;
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let client_config = if use_client_identity {
        let certificate_pem = tokio::fs::read(client_certificate_path(config)).await?;
        let mut certificate_reader = std::io::BufReader::new(certificate_pem.as_slice());
        let certificates =
            rustls_pemfile::certs(&mut certificate_reader).collect::<Result<Vec<_>, _>>()?;
        if certificates.is_empty() {
            return Err(TransportError::TlsConfiguration(
                "client certificate file contains no certificates".to_owned(),
            ));
        }
        let private_key_pem = tokio::fs::read(client_private_key_path(config)).await?;
        let mut private_key_reader = std::io::BufReader::new(private_key_pem.as_slice());
        let private_key =
            rustls_pemfile::private_key(&mut private_key_reader)?.ok_or_else(|| {
                TransportError::TlsConfiguration(
                    "client private key file contains no supported private key".to_owned(),
                )
            })?;
        builder
            .with_client_auth_cert(certificates, private_key)
            .map_err(|error| TransportError::TlsConfiguration(error.to_string()))?
    } else {
        builder.with_no_client_auth()
    };
    let server_name = rustls::pki_types::ServerName::try_from(tls.server_name.clone())
        .map_err(|error| TransportError::TlsConfiguration(error.to_string()))?;
    let connector = TlsConnector::from(Arc::new(client_config));
    let stream = connector.connect(server_name, stream).await?;
    Ok(Box::new(stream))
}
async fn dispatch(
    executor: &AgentExecutor,
    request: Request,
    response_id: Value,
) -> (Response, bool) {
    if request.jsonrpc != json_rpc::VERSION
        || request.method.is_empty()
        || !request.id.is_valid()
        || !matches!(
            &request.params,
            Value::Null | Value::Array(_) | Value::Object(_)
        )
    {
        return (
            Response::error(
                response_id,
                ErrorObject::new(json_rpc::error_code::INVALID_REQUEST, "Invalid request"),
            ),
            false,
        );
    }

    let method_name = request.method;
    let shutdown_requested = method_name == method::NODE_SHUTDOWN;
    match dispatch_method(executor, &method_name, request.params).await {
        Ok(value) => (Response::success(response_id, value), shutdown_requested),
        Err(error) => (
            Response::error(response_id, error.into_error_object()),
            false,
        ),
    }
}

async fn dispatch_method(
    executor: &AgentExecutor,
    method_name: &str,
    params: Value,
) -> Result<Value, DispatchError> {
    match method_name {
        method::AGENT_INSPECT => {
            let params = parse_params::<AgentInspectParams>(params)?;
            to_value(executor.inspect(params.instance).await?)
        }
        method::DATA_RESTORE => {
            let params = parse_params::<RestoreDataParams>(params)?;
            to_value(executor.restore_data(params).await?)
        }
        method::SERVER_START => {
            let params = parse_params::<StartServerParams>(params)?;
            to_value(executor.start_server(params).await?)
        }
        method::SERVER_STOP => {
            let params = parse_params::<StopServerParams>(params)?;
            to_value(executor.stop_server(params).await?)
        }
        method::DATA_SNAPSHOT => {
            let params = parse_params::<SnapshotDataParams>(params)?;
            to_value(executor.snapshot_data(params).await?)
        }
        method::INSTANCE_CLEANUP => {
            let params = parse_params::<CleanupInstanceParams>(params)?;
            to_value(executor.cleanup_instance(params).await?)
        }
        method::NODE_SHUTDOWN => {
            require_no_params(&params)?;
            to_value(ShutdownResult { accepted: true })
        }
        _ => Err(DispatchError::MethodNotFound),
    }
}

fn parse_params<T>(value: Value) -> Result<T, DispatchError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| DispatchError::InvalidParams(error.to_string()))
}

fn require_no_params(params: &Value) -> Result<(), DispatchError> {
    match params {
        Value::Null => Ok(()),
        Value::Array(values) if values.is_empty() => Ok(()),
        Value::Object(values) if values.is_empty() => Ok(()),
        _ => Err(DispatchError::InvalidParams(
            "this method does not accept params".to_owned(),
        )),
    }
}

fn to_value<T>(value: T) -> Result<Value, DispatchError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(DispatchError::Serialization)
}

#[derive(Debug, Error)]
enum DispatchError {
    #[error("method not found")]
    MethodNotFound,
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("node operation failed")]
    Execution(#[from] ExecutorError),
    #[error("response serialization failed")]
    Serialization(serde_json::Error),
}

impl DispatchError {
    fn into_error_object(self) -> ErrorObject {
        match self {
            Self::MethodNotFound => {
                ErrorObject::new(json_rpc::error_code::METHOD_NOT_FOUND, "Method not found")
            }
            Self::InvalidParams(detail) => {
                ErrorObject::new(json_rpc::error_code::INVALID_PARAMS, "Invalid params")
                    .with_data(json!({ "detail": detail }))
            }
            Self::Execution(error) => {
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Execution failed")
                    .with_data(json!({ "detail": error.to_string() }))
            }
            Self::Serialization(error) => {
                ErrorObject::new(json_rpc::error_code::INTERNAL_ERROR, "Serialization failed")
                    .with_data(json!({ "detail": error.to_string() }))
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct WireResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<WireError>,
    id: Value,
}

#[derive(Debug, serde::Deserialize)]
struct WireError {
    code: i64,
    message: String,
}

fn response_result<T>(response: WireResponse) -> Result<T, TransportError>
where
    T: DeserializeOwned,
{
    if response.jsonrpc != json_rpc::VERSION {
        return Err(TransportError::Protocol(
            "control-plane response JSON-RPC version mismatch".to_owned(),
        ));
    }
    match (response.result, response.error) {
        (Some(value), None) => serde_json::from_value(value).map_err(TransportError::Serialization),
        (None, Some(error)) => Err(TransportError::Remote {
            code: error.code,
            message: error.message,
        }),
        _ => Err(TransportError::Protocol(
            "response must contain exactly one of result or error".to_owned(),
        )),
    }
}

async fn read_request<R>(reader: &mut R, maximum: usize) -> Result<Request, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let value = read_value(reader, maximum).await?;
    serde_json::from_value(value).map_err(TransportError::Serialization)
}

async fn read_response<R>(reader: &mut R, maximum: usize) -> Result<WireResponse, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let value = read_value(reader, maximum).await?;
    serde_json::from_value(value).map_err(TransportError::Serialization)
}

async fn read_value<R>(reader: &mut R, maximum: usize) -> Result<Value, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let read = read_frame(reader, &mut frame, maximum).await?;
    if read == 0 {
        return Err(TransportError::Disconnected);
    }
    let input = trim_line_ending(&frame);
    serde_json::from_slice(input).map_err(TransportError::Serialization)
}

async fn read_frame<R>(
    reader: &mut R,
    frame: &mut Vec<u8>,
    maximum: usize,
) -> Result<usize, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    frame.clear();
    loop {
        let (consumed, terminated) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok(frame.len());
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            let actual = frame.len().saturating_add(consumed);
            if actual > maximum {
                return Err(TransportError::FrameTooLarge { actual, maximum });
            }
            frame.extend_from_slice(&available[..consumed]);
            (consumed, newline.is_some())
        };
        reader.consume(consumed);
        if terminated {
            return Ok(frame.len());
        }
    }
}

fn trim_line_ending(frame: &[u8]) -> &[u8] {
    let frame = frame.strip_suffix(b"\n").unwrap_or(frame);
    frame.strip_suffix(b"\r").unwrap_or(frame)
}

async fn write_value<W>(writer: &mut W, value: &Value) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&serde_json::to_vec(value)?).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOutcome {
    Disconnected,
    Shutdown,
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("node-agent transport I/O failed")]
    Io(#[from] io::Error),
    #[error("node-agent protocol serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("node-agent credential file {path:?} failed")]
    CredentialIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("node-agent connection token is invalid")]
    InvalidConnectionToken,
    #[error("node-agent certificate signing request is invalid")]
    InvalidCertificateSigningRequest,
    #[error("control plane disconnected")]
    Disconnected,
    #[error("node-agent frame is too large: at least {actual} bytes, maximum {maximum} bytes")]
    FrameTooLarge { actual: usize, maximum: usize },
    #[error("node-agent protocol violation: {0}")]
    Protocol(String),
    #[error("node-agent registration was rejected")]
    RegistrationRejected,
    #[error("control plane returned an error: {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("TLS configuration is invalid: {0}")]
    TlsConfiguration(String),
    #[error("runtime environment returned by the control plane is invalid")]
    InvalidRuntimeEnvironment,
    #[error("OpenSSL command {0} timed out")]
    OpenSslTimeout(&'static str),
    #[error("OpenSSL command {description} failed with status {status:?}: {stderr}")]
    OpenSslFailed {
        description: &'static str,
        status: Option<i32>,
        stderr: String,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::validate_runtime_environment;

    #[test]
    fn remote_registration_requires_complete_storage_credentials() {
        let values = BTreeMap::from([("RESTIC_PASSWORD".to_owned(), "test".to_owned())]);
        assert!(validate_runtime_environment(&values, true).is_err());
        assert!(validate_runtime_environment(&values, false).is_ok());
    }

    #[test]
    fn remote_registration_accepts_complete_storage_credentials() {
        let values = BTreeMap::from([
            ("RESTIC_PASSWORD".to_owned(), "test".to_owned()),
            ("AWS_ACCESS_KEY_ID".to_owned(), "id".to_owned()),
            ("AWS_SECRET_ACCESS_KEY".to_owned(), "secret".to_owned()),
            ("AWS_SESSION_TOKEN".to_owned(), "session".to_owned()),
            ("AWS_DEFAULT_REGION".to_owned(), "auto".to_owned()),
        ]);
        assert!(validate_runtime_environment(&values, true).is_ok());
    }
}
