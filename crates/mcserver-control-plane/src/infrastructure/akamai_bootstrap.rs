use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use thiserror::Error;

use crate::{
    config::RemoteAgentConfig,
    domain::{ComputeInstance, ServerInstance},
};

const MAX_AUTHORIZED_KEYS_BYTES: usize = 64 * 1024;
const MAX_CA_BYTES: usize = 64 * 1024;
const REMOTE_STATE_DIRECTORY: &str = "/var/lib/mcserver/node-agent";
const REMOTE_BINARY_PATH: &str = "/usr/local/bin/mcserver-node-agent";
const REMOTE_CA_PATH: &str = "/etc/mcserver/control-plane-ca.pem";
const REMOTE_ENV_PATH: &str = "/etc/mcserver/node-agent.env";
const REMOTE_UNIT_PATH: &str = "/etc/systemd/system/mcserver-node-agent.service";

#[derive(Debug, Clone)]
pub struct BootstrapArtifacts {
    pub authorized_keys: Vec<String>,
    pub user_data_base64: String,
}

pub async fn build_bootstrap(
    remote: &RemoteAgentConfig,
    authorized_keys_file: &Path,
    provider_scope: &str,
    instance: &ServerInstance,
    compute: &ComputeInstance,
) -> Result<BootstrapArtifacts, BootstrapError> {
    let authorized_keys = read_authorized_keys(authorized_keys_file).await?;
    let ca = read_bounded(&remote.tls_ca_certificate, MAX_CA_BYTES).await?;
    if ca.contains(&0) {
        return Err(BootstrapError::CaContainsNul);
    }

    let enrollment_token = compute
        .enrollment_token
        .as_deref()
        .ok_or(BootstrapError::MissingEnrollmentToken)?;
    let manifest = BootstrapManifest {
        schema_version: 2,
        compute_instance_id: compute.id.to_string(),
        server_instance_id: instance.id.to_string(),
        enrollment_token: enrollment_token.to_owned(),
        control_plane_address: remote.public_address.clone(),
        tls_server_name: remote.tls_server_name.clone(),
        provider_scope: provider_scope.to_owned(),
    };
    let manifest_base64 = STANDARD.encode(serde_json::to_vec(&manifest)?);
    let ca_base64 = STANDARD.encode(ca);
    let unit_base64 = STANDARD.encode(systemd_unit());

    let script = format!(
        r#"#!/bin/sh
set -eu
umask 077
# mcserver-bootstrap: {manifest_base64}

install_packages() {{
  if command -v dnf >/dev/null 2>&1; then
    dnf -y install ca-certificates curl openssl podman restic
  elif command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y ca-certificates curl openssl podman restic
  else
    echo 'unsupported distribution: dnf or apt-get is required' >&2
    exit 1
  fi
}}

install_packages
install -d -m 0700 /etc/mcserver {state_directory}
printf '%s' '{ca_base64}' | base64 -d > {ca_path}
cat > {env_path} <<'MCSERVER_AGENT_ENV'
MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS={control_plane_address}
MCSERVER_NODE_AGENT_TLS_CA_CERTIFICATE={ca_path}
MCSERVER_NODE_AGENT_TLS_SERVER_NAME={tls_server_name}
MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID={compute_instance_id}
MCSERVER_NODE_AGENT_CONNECTION_TOKEN={enrollment_token}
MCSERVER_NODE_AGENT_STATE_DIRECTORY={state_directory}
MCSERVER_NODE_AGENT_LOCAL_SCOPE={provider_scope}
MCSERVER_NODE_AGENT_DATA_ACCESS_MODE=host
MCSERVER_AGENT_ENV
chmod 0600 {ca_path} {env_path}

curl --fail --location --proto '=https' --tlsv1.2 \
  --output {binary_path}.tmp {download_url}
printf '%s  %s\n' '{binary_sha256}' '{binary_path}.tmp' | sha256sum --check --strict
install -m 0755 {binary_path}.tmp {binary_path}
rm -f {binary_path}.tmp
printf '%s' '{unit_base64}' | base64 -d > {unit_path}
chmod 0644 {unit_path}
systemctl daemon-reload
systemctl enable --now mcserver-node-agent.service
"#,
        state_directory = REMOTE_STATE_DIRECTORY,
        ca_path = REMOTE_CA_PATH,
        env_path = REMOTE_ENV_PATH,
        control_plane_address = shell_value(&remote.public_address),
        tls_server_name = shell_value(&remote.tls_server_name),
        compute_instance_id = compute.id,
        enrollment_token = shell_value(enrollment_token),
        provider_scope = shell_value(provider_scope),
        binary_path = REMOTE_BINARY_PATH,
        download_url = shell_value(&remote.node_agent_download_url),
        binary_sha256 = remote.node_agent_sha256,
        unit_path = REMOTE_UNIT_PATH,
    );

    Ok(BootstrapArtifacts {
        authorized_keys,
        user_data_base64: STANDARD.encode(script),
    })
}

async fn read_authorized_keys(path: &Path) -> Result<Vec<String>, BootstrapError> {
    let bytes = read_bounded(path, MAX_AUTHORIZED_KEYS_BYTES).await?;
    let text = String::from_utf8(bytes).map_err(BootstrapError::InvalidAuthorizedKeysEncoding)?;
    let keys = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Err(BootstrapError::NoAuthorizedKeys);
    }
    if keys.iter().any(|key| key.contains('\0')) {
        return Err(BootstrapError::AuthorizedKeyContainsNul);
    }
    Ok(keys)
}

async fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, BootstrapError> {
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() > maximum as u64 {
        return Err(BootstrapError::FileTooLarge {
            path: path.display().to_string(),
            maximum,
        });
    }
    Ok(tokio::fs::read(path).await?)
}

fn systemd_unit() -> &'static str {
    r#"[Unit]
Description=Minecraft Server Management node agent
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
EnvironmentFile=/etc/mcserver/node-agent.env
ExecStart=/usr/local/bin/mcserver-node-agent
Restart=on-failure
RestartSec=5s
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ProtectKernelTunables=true
ProtectKernelModules=true
RestrictSUIDSGID=true
LockPersonality=true
Delegate=yes
TasksMax=infinity
KillMode=mixed
ReadWritePaths=/var/lib/mcserver /var/lib/containers /run/containers

[Install]
WantedBy=multi-user.target
"#
}

fn shell_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for character in value.chars() {
        if character == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(character);
        }
    }
    escaped.push('\'');
    escaped
}

#[derive(Debug, Serialize)]
struct BootstrapManifest {
    schema_version: u32,
    compute_instance_id: String,
    server_instance_id: String,
    enrollment_token: String,
    control_plane_address: String,
    tls_server_name: String,
    provider_scope: String,
}

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("bootstrap file I/O failed")]
    Io(#[from] std::io::Error),
    #[error("bootstrap file {path} exceeds {maximum} bytes")]
    FileTooLarge { path: String, maximum: usize },
    #[error("authorized_keys is not UTF-8")]
    InvalidAuthorizedKeysEncoding(#[source] std::string::FromUtf8Error),
    #[error("authorized_keys contains no usable public key")]
    NoAuthorizedKeys,
    #[error("authorized key contains a NUL byte")]
    AuthorizedKeyContainsNul,
    #[error("TLS CA certificate contains a NUL byte")]
    CaContainsNul,
    #[error("Akamai compute instance has no enrollment token")]
    MissingEnrollmentToken,
    #[error("bootstrap manifest serialization failed")]
    Serialization(#[from] serde_json::Error),
}
