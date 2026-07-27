use std::{env, fmt, net::SocketAddr, os::unix::fs::MetadataExt, path::PathBuf, time::Duration};

use thiserror::Error;
use uuid::Uuid;

const DEFAULT_PODMAN_BINARY: &str = "podman";
const DEFAULT_RESTIC_BINARY: &str = "restic";
const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_RESTIC_RETRY_LOCK_SECONDS: u64 = 300;
const DEFAULT_RECONNECT_MIN_SECONDS: u64 = 1;
const DEFAULT_RECONNECT_MAX_SECONDS: u64 = 30;
const DEFAULT_LOCAL_SCOPE: &str = "default";
const MAX_LOCAL_SCOPE_CHARS: usize = 128;
const MAX_ADDRESS_CHARS: usize = 512;
const MAX_SERVER_NAME_CHARS: usize = 253;
const MAX_CONNECTION_TOKEN_CHARS: usize = 256;
const DEFAULT_DATA_ACCESS_MODE: &str = "auto";

#[derive(Clone)]
pub struct Config {
    pub control_plane_address: String,
    pub tls: Option<TlsConfig>,
    pub compute_instance_id: Uuid,
    pub connection_token: String,
    pub state_directory: PathBuf,
    pub local_scope: String,
    pub podman_binary: PathBuf,
    pub restic_binary: PathBuf,
    pub data_access_mode: DataAccessMode,
    pub max_frame_bytes: usize,
    pub command_timeout: Duration,
    pub restic_retry_lock: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("control_plane_address", &self.control_plane_address)
            .field("tls", &self.tls)
            .field("compute_instance_id", &self.compute_instance_id)
            .field("connection_token", &"[REDACTED]")
            .field("state_directory", &self.state_directory)
            .field("local_scope", &self.local_scope)
            .field("podman_binary", &self.podman_binary)
            .field("restic_binary", &self.restic_binary)
            .field("data_access_mode", &self.data_access_mode)
            .field("max_frame_bytes", &self.max_frame_bytes)
            .field("command_timeout", &self.command_timeout)
            .field("restic_retry_lock", &self.restic_retry_lock)
            .field("reconnect_min", &self.reconnect_min)
            .field("reconnect_max", &self.reconnect_max)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataAccessMode {
    PodmanUserNamespace,
    Host,
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub ca_certificate: PathBuf,
    pub server_name: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let control_plane_address = required("MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS")?;
        validate_text(
            "MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS",
            &control_plane_address,
            MAX_ADDRESS_CHARS,
        )?;
        let tls = tls_config()?;
        if tls.is_none() {
            let address: SocketAddr = control_plane_address
                .parse()
                .map_err(ConfigError::InvalidSocketAddress)?;
            if !address.ip().is_loopback() {
                return Err(ConfigError::ControlPlaneAddressMustBeLoopback(address));
            }
        }
        let compute_instance_id = required("MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID")?
            .parse()
            .map_err(ConfigError::InvalidUuid)?;
        let connection_token = required("MCSERVER_NODE_AGENT_CONNECTION_TOKEN")?;
        validate_text(
            "MCSERVER_NODE_AGENT_CONNECTION_TOKEN",
            &connection_token,
            MAX_CONNECTION_TOKEN_CHARS,
        )?;
        let state_directory = PathBuf::from(required("MCSERVER_NODE_AGENT_STATE_DIRECTORY")?);
        let local_scope =
            optional_non_blank("MCSERVER_NODE_AGENT_LOCAL_SCOPE", DEFAULT_LOCAL_SCOPE)?;
        let podman_binary = env::var_os("MCSERVER_NODE_AGENT_PODMAN_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PODMAN_BINARY));
        let restic_binary = env::var_os("MCSERVER_NODE_AGENT_RESTIC_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_RESTIC_BINARY));
        let data_access_mode = data_access_mode()?;
        let max_frame_bytes = parse_positive_usize(
            "MCSERVER_NODE_AGENT_MAX_FRAME_BYTES",
            DEFAULT_MAX_FRAME_BYTES,
        )?;
        let command_timeout = parse_positive_duration(
            "MCSERVER_NODE_AGENT_COMMAND_TIMEOUT_SECONDS",
            DEFAULT_COMMAND_TIMEOUT_SECONDS,
        )?;
        let restic_retry_lock = parse_positive_duration(
            "MCSERVER_NODE_AGENT_RESTIC_RETRY_LOCK_SECONDS",
            DEFAULT_RESTIC_RETRY_LOCK_SECONDS,
        )?;
        let reconnect_min = parse_positive_duration(
            "MCSERVER_NODE_AGENT_RECONNECT_MIN_SECONDS",
            DEFAULT_RECONNECT_MIN_SECONDS,
        )?;
        let reconnect_max = parse_positive_duration(
            "MCSERVER_NODE_AGENT_RECONNECT_MAX_SECONDS",
            DEFAULT_RECONNECT_MAX_SECONDS,
        )?;
        if reconnect_max < reconnect_min {
            return Err(ConfigError::InvalidReconnectRange);
        }

        Ok(Self {
            control_plane_address,
            tls,
            compute_instance_id,
            connection_token,
            state_directory,
            local_scope,
            podman_binary,
            restic_binary,
            data_access_mode,
            max_frame_bytes,
            command_timeout,
            restic_retry_lock,
            reconnect_min,
            reconnect_max,
        })
    }
}

fn data_access_mode() -> Result<DataAccessMode, ConfigError> {
    let value = optional("MCSERVER_NODE_AGENT_DATA_ACCESS_MODE")?
        .unwrap_or_else(|| DEFAULT_DATA_ACCESS_MODE.to_owned());
    match parse_data_access_mode(value)? {
        Some(mode) => Ok(mode),
        None => automatic_data_access_mode(),
    }
}

fn parse_data_access_mode(value: String) -> Result<Option<DataAccessMode>, ConfigError> {
    match value.as_str() {
        "auto" => Ok(None),
        "podman_user_namespace" => Ok(Some(DataAccessMode::PodmanUserNamespace)),
        "host" => Ok(Some(DataAccessMode::Host)),
        _ => Err(ConfigError::InvalidDataAccessMode(value)),
    }
}

fn automatic_data_access_mode() -> Result<DataAccessMode, ConfigError> {
    let effective_uid = std::fs::metadata("/proc/self")
        .map_err(ConfigError::ProcessMetadata)?
        .uid();
    Ok(if effective_uid == 0 {
        DataAccessMode::Host
    } else {
        DataAccessMode::PodmanUserNamespace
    })
}

fn tls_config() -> Result<Option<TlsConfig>, ConfigError> {
    let ca = optional("MCSERVER_NODE_AGENT_TLS_CA_CERTIFICATE")?;
    let server_name = optional("MCSERVER_NODE_AGENT_TLS_SERVER_NAME")?;
    match (ca, server_name) {
        (None, None) => Ok(None),
        (Some(ca_certificate), Some(server_name)) => {
            validate_text(
                "MCSERVER_NODE_AGENT_TLS_SERVER_NAME",
                &server_name,
                MAX_SERVER_NAME_CHARS,
            )?;
            validate_dns_name(&server_name)?;
            Ok(Some(TlsConfig {
                ca_certificate: PathBuf::from(ca_certificate),
                server_name,
            }))
        }
        _ => Err(ConfigError::IncompleteTlsConfiguration),
    }
}

fn optional_non_blank(name: &'static str, default: &str) -> Result<String, ConfigError> {
    let value = optional(name)?.unwrap_or_else(|| default.to_owned());
    validate_text(name, &value, MAX_LOCAL_SCOPE_CHARS)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ConfigError::InvalidLocalScope(value));
    }
    Ok(value)
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    optional(name)?.ok_or(ConfigError::MissingValue(name))
}

fn optional(name: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn validate_text(name: &'static str, value: &str, maximum: usize) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::BlankValue(name));
    }
    if value.contains('\0') {
        return Err(ConfigError::NulByte(name));
    }
    if value.chars().count() > maximum {
        return Err(ConfigError::ValueTooLong { name, maximum });
    }
    Ok(())
}

fn validate_dns_name(value: &str) -> Result<(), ConfigError> {
    if value.len() > MAX_SERVER_NAME_CHARS
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(ConfigError::InvalidTlsServerName(value.to_owned()));
    }
    Ok(())
}

fn parse_positive_usize(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            })?,
        Err(env::VarError::NotPresent) => default,
        Err(source) => return Err(ConfigError::Environment { name, source }),
    };
    if value == 0 {
        return Err(ConfigError::ZeroValue(name));
    }
    Ok(value)
}

fn parse_positive_duration(name: &'static str, default: u64) -> Result<Duration, ConfigError> {
    let seconds = match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            })?,
        Err(env::VarError::NotPresent) => default,
        Err(source) => return Err(ConfigError::Environment { name, source }),
    };
    if seconds == 0 {
        return Err(ConfigError::ZeroValue(name));
    }
    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable {name} is invalid")]
    Environment {
        name: &'static str,
        #[source]
        source: env::VarError,
    },
    #[error("required environment variable {0} is missing")]
    MissingValue(&'static str),
    #[error("{0} must not be blank")]
    BlankValue(&'static str),
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{name} must be no longer than {maximum} characters")]
    ValueTooLong { name: &'static str, maximum: usize },
    #[error("local scope must contain only ASCII letters, digits, dot, underscore, or hyphen: {0}")]
    InvalidLocalScope(String),
    #[error("{name} must be an integer, got {value}")]
    InvalidInteger {
        name: &'static str,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
    #[error("control-plane address is invalid")]
    InvalidSocketAddress(#[source] std::net::AddrParseError),
    #[error("plain control-plane address must be loopback, got {0}")]
    ControlPlaneAddressMustBeLoopback(SocketAddr),
    #[error("remote TLS requires both CA certificate and server name")]
    IncompleteTlsConfiguration,
    #[error("invalid TLS server name: {0}")]
    InvalidTlsServerName(String),
    #[error("compute instance id is invalid")]
    InvalidUuid(#[source] uuid::Error),
    #[error(
        "MCSERVER_NODE_AGENT_DATA_ACCESS_MODE must be auto, podman_user_namespace, or host: {0}"
    )]
    InvalidDataAccessMode(String),
    #[error("cannot determine the node-agent effective user")]
    ProcessMetadata(#[source] std::io::Error),
    #[error("reconnect maximum must not be less than the minimum")]
    InvalidReconnectRange,
}

#[cfg(test)]
mod tests {
    use super::{DataAccessMode, parse_data_access_mode};

    #[test]
    fn parses_explicit_data_access_modes() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_data_access_mode("auto".to_owned())?, None);
        assert_eq!(
            parse_data_access_mode("podman_user_namespace".to_owned())?,
            Some(DataAccessMode::PodmanUserNamespace)
        );
        assert_eq!(
            parse_data_access_mode("host".to_owned())?,
            Some(DataAccessMode::Host)
        );
        assert!(parse_data_access_mode("invalid".to_owned()).is_err());
        Ok(())
    }
}
