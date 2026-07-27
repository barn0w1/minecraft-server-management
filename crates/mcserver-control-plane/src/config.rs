use std::{
    env, fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use thiserror::Error;

const DEFAULT_SOCKET_PATH: &str = "/run/mcserver/control-plane.sock";
const DEFAULT_DATABASE_URL: &str = "sqlite:///var/lib/mcserver/control-plane.db?mode=rwc";
const DEFAULT_SOCKET_MODE: u32 = 0o660;
const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_RECONCILE_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_RECONCILE_RETRY_SECONDS: u64 = 5;
const DEFAULT_SHUTDOWN_TIMEOUT_SECONDS: u64 = 15;
const DEFAULT_AGENT_LISTEN_ADDRESS: &str = "127.0.0.1:39001";
const DEFAULT_AGENT_COMMAND_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_NODE_AGENT_BINARY: &str = "mcserver-node-agent";
const DEFAULT_NODE_AGENT_ROOT: &str = "/var/lib/mcserver/local-agents";
const DEFAULT_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_LOCAL_CONTROL_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_PODMAN_BINARY: &str = "podman";
const DEFAULT_LOCAL_SCOPE: &str = "default";
const DEFAULT_AKAMAI_SCOPE: &str = "default";
const DEFAULT_AKAMAI_API_BASE_URL: &str = "https://api.linode.com/v4";
const DEFAULT_AKAMAI_REQUEST_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_AKAMAI_REAP_ORPHANS_ON_START: bool = false;
const MAX_SCOPE_CHARS: usize = 128;
const MAX_AKAMAI_SCOPE_CHARS: usize = 35;
const MAX_ADDRESS_CHARS: usize = 512;
const MAX_URL_CHARS: usize = 4096;
const MAX_SERVER_NAME_CHARS: usize = 253;
const SHA256_HEX_CHARS: usize = 64;
const DEFAULT_REAP_ORPHANS_ON_START: bool = true;

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: PathBuf,
    pub database_url: String,
    pub socket_mode: u32,
    pub max_frame_bytes: usize,
    pub reconcile_interval: Duration,
    pub reconcile_retry: Duration,
    pub shutdown_timeout: Duration,
    pub agent_listen_address: SocketAddr,
    pub agent_command_timeout: Duration,
    pub node_agent_binary: PathBuf,
    pub node_agent_root: PathBuf,
    pub podman_binary: PathBuf,
    pub local_scope: String,
    pub reap_orphans_on_start: bool,
    pub local_control_timeout: Duration,
    pub local_process_stop_timeout: Duration,
    pub remote_agent: Option<RemoteAgentConfig>,
    pub akamai: Option<AkamaiConfig>,
}

#[derive(Debug, Clone)]
pub struct RemoteAgentConfig {
    pub listen_address: SocketAddr,
    pub public_address: String,
    pub tls_server_name: String,
    pub tls_certificate: PathBuf,
    pub tls_private_key: PathBuf,
    pub tls_ca_certificate: PathBuf,
    pub node_agent_download_url: String,
    pub node_agent_sha256: String,
}

#[derive(Clone)]
pub struct AkamaiConfig {
    pub api_token: String,
    pub api_base_url: String,
    pub authorized_keys_file: PathBuf,
    pub node_agent_environment_file: PathBuf,
    pub scope: String,
    pub request_timeout: Duration,
    pub reap_orphans_on_start: bool,
}

impl fmt::Debug for AkamaiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AkamaiConfig")
            .field("api_token", &"[redacted]")
            .field("api_base_url", &self.api_base_url)
            .field("authorized_keys_file", &self.authorized_keys_file)
            .field(
                "node_agent_environment_file",
                &self.node_agent_environment_file,
            )
            .field("scope", &self.scope)
            .field("request_timeout", &self.request_timeout)
            .field("reap_orphans_on_start", &self.reap_orphans_on_start)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let socket_path = env::var_os("MCSERVER_CONTROL_PLANE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));
        let database_url =
            optional_string("MCSERVER_CONTROL_PLANE_DATABASE_URL", DEFAULT_DATABASE_URL)?;
        let socket_mode =
            parse_socket_mode(optional_string_value("MCSERVER_CONTROL_PLANE_SOCKET_MODE")?)?;
        let max_frame_bytes = parse_positive_usize(
            "MCSERVER_CONTROL_PLANE_MAX_FRAME_BYTES",
            DEFAULT_MAX_FRAME_BYTES,
        )?;
        let reconcile_interval = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS",
            DEFAULT_RECONCILE_INTERVAL_SECONDS,
        )?;
        let reconcile_retry = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_RECONCILE_RETRY_SECONDS",
            DEFAULT_RECONCILE_RETRY_SECONDS,
        )?;
        let shutdown_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_SHUTDOWN_TIMEOUT_SECONDS",
            DEFAULT_SHUTDOWN_TIMEOUT_SECONDS,
        )?;
        let agent_listen_address: SocketAddr = optional_string(
            "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS",
            DEFAULT_AGENT_LISTEN_ADDRESS,
        )?
        .parse()
        .map_err(ConfigError::InvalidSocketAddress)?;
        if !agent_listen_address.ip().is_loopback() {
            return Err(ConfigError::AgentAddressMustBeLoopback(
                agent_listen_address,
            ));
        }
        if agent_listen_address.port() == 0 {
            return Err(ConfigError::ZeroAgentPort);
        }
        let agent_command_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_AGENT_COMMAND_TIMEOUT_SECONDS",
            DEFAULT_AGENT_COMMAND_TIMEOUT_SECONDS,
        )?;
        let node_agent_binary = env::var_os("MCSERVER_CONTROL_PLANE_NODE_AGENT_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_NODE_AGENT_BINARY));
        let node_agent_root = env::var_os("MCSERVER_CONTROL_PLANE_NODE_AGENT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_NODE_AGENT_ROOT));
        let podman_binary = env::var_os("MCSERVER_CONTROL_PLANE_PODMAN_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PODMAN_BINARY));
        let local_scope =
            optional_scope("MCSERVER_CONTROL_PLANE_LOCAL_SCOPE", DEFAULT_LOCAL_SCOPE)?;
        let reap_orphans_on_start = parse_bool(
            "MCSERVER_CONTROL_PLANE_REAP_ORPHANS_ON_START",
            DEFAULT_REAP_ORPHANS_ON_START,
        )?;
        let local_control_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_LOCAL_CONTROL_TIMEOUT_SECONDS",
            DEFAULT_LOCAL_CONTROL_TIMEOUT_SECONDS,
        )?;
        let local_process_stop_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS",
            DEFAULT_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS,
        )?;
        let remote_agent = remote_agent_config()?;
        let akamai = akamai_config(remote_agent.as_ref())?;

        Ok(Self {
            socket_path,
            database_url,
            socket_mode,
            max_frame_bytes,
            reconcile_interval,
            reconcile_retry,
            shutdown_timeout,
            agent_listen_address,
            agent_command_timeout,
            node_agent_binary,
            node_agent_root,
            podman_binary,
            local_scope,
            reap_orphans_on_start,
            local_control_timeout,
            local_process_stop_timeout,
            remote_agent,
            akamai,
        })
    }
}

fn remote_agent_config() -> Result<Option<RemoteAgentConfig>, ConfigError> {
    let Some(listen_value) =
        optional_string_value("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS")?
    else {
        reject_partial_remote_config()?;
        return Ok(None);
    };
    let listen_address = listen_value
        .parse::<SocketAddr>()
        .map_err(ConfigError::InvalidRemoteSocketAddress)?;
    if listen_address.port() == 0 {
        return Err(ConfigError::ZeroRemoteAgentPort);
    }

    let public_address = required_non_blank(
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS",
        MAX_ADDRESS_CHARS,
    )?;
    validate_host_port(&public_address)?;
    let tls_server_name = required_non_blank(
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME",
        MAX_SERVER_NAME_CHARS,
    )?;
    validate_dns_name(&tls_server_name)?;
    let tls_certificate = required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE")?;
    let tls_private_key = required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY")?;
    let tls_ca_certificate =
        required_path("MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE")?;
    let node_agent_download_url = required_non_blank(
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        MAX_URL_CHARS,
    )?;
    validate_https_url(
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        &node_agent_download_url,
    )?;
    let node_agent_sha256 =
        required_non_blank("MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256", SHA256_HEX_CHARS)?;
    if node_agent_sha256.len() != SHA256_HEX_CHARS
        || !node_agent_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ConfigError::InvalidSha256(node_agent_sha256));
    }

    Ok(Some(RemoteAgentConfig {
        listen_address,
        public_address,
        tls_server_name,
        tls_certificate,
        tls_private_key,
        tls_ca_certificate,
        node_agent_download_url,
        node_agent_sha256: node_agent_sha256.to_ascii_lowercase(),
    }))
}

fn reject_partial_remote_config() -> Result<(), ConfigError> {
    for name in [
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_PUBLIC_ADDRESS",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_SERVER_NAME",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CERTIFICATE",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_PRIVATE_KEY",
        "MCSERVER_CONTROL_PLANE_REMOTE_AGENT_TLS_CA_CERTIFICATE",
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_DOWNLOAD_URL",
        "MCSERVER_CONTROL_PLANE_NODE_AGENT_SHA256",
    ] {
        if optional_string_value(name)?.is_some() {
            return Err(ConfigError::RemoteAgentListenAddressRequired);
        }
    }
    Ok(())
}

fn akamai_config(
    remote_agent: Option<&RemoteAgentConfig>,
) -> Result<Option<AkamaiConfig>, ConfigError> {
    let Some(api_token) = optional_string_value("MCSERVER_AKAMAI_API_TOKEN")? else {
        for name in [
            "MCSERVER_AKAMAI_API_BASE_URL",
            "MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE",
            "MCSERVER_AKAMAI_NODE_AGENT_ENVIRONMENT_FILE",
            "MCSERVER_AKAMAI_SCOPE",
            "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS",
            "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START",
        ] {
            if optional_string_value(name)?.is_some() {
                return Err(ConfigError::AkamaiTokenRequired);
            }
        }
        return Ok(None);
    };
    validate_non_blank("MCSERVER_AKAMAI_API_TOKEN", &api_token, 4096)?;
    if remote_agent.is_none() {
        return Err(ConfigError::RemoteAgentRequiredForAkamai);
    }
    let api_base_url =
        optional_string("MCSERVER_AKAMAI_API_BASE_URL", DEFAULT_AKAMAI_API_BASE_URL)?;
    validate_api_base_url(&api_base_url)?;
    let authorized_keys_file = required_path("MCSERVER_AKAMAI_AUTHORIZED_KEYS_FILE")?;
    let node_agent_environment_file = required_path("MCSERVER_AKAMAI_NODE_AGENT_ENVIRONMENT_FILE")?;
    let scope = optional_scope("MCSERVER_AKAMAI_SCOPE", DEFAULT_AKAMAI_SCOPE)?;
    if scope.chars().count() > MAX_AKAMAI_SCOPE_CHARS {
        return Err(ConfigError::AkamaiScopeTooLong {
            maximum: MAX_AKAMAI_SCOPE_CHARS,
        });
    }
    let request_timeout = parse_positive_duration(
        "MCSERVER_AKAMAI_REQUEST_TIMEOUT_SECONDS",
        DEFAULT_AKAMAI_REQUEST_TIMEOUT_SECONDS,
    )?;
    let reap_orphans_on_start = parse_bool(
        "MCSERVER_AKAMAI_REAP_ORPHANS_ON_START",
        DEFAULT_AKAMAI_REAP_ORPHANS_ON_START,
    )?;
    Ok(Some(AkamaiConfig {
        api_token,
        api_base_url: api_base_url.trim_end_matches('/').to_owned(),
        authorized_keys_file,
        node_agent_environment_file,
        scope,
        request_timeout,
        reap_orphans_on_start,
    }))
}

fn required_path(name: &'static str) -> Result<PathBuf, ConfigError> {
    Ok(PathBuf::from(required_non_blank(name, 4096)?))
}

fn required_non_blank(name: &'static str, maximum: usize) -> Result<String, ConfigError> {
    let value = optional_string_value(name)?.ok_or(ConfigError::MissingValue(name))?;
    validate_non_blank(name, &value, maximum)?;
    Ok(value)
}

fn validate_non_blank(name: &'static str, value: &str, maximum: usize) -> Result<(), ConfigError> {
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

fn optional_scope(name: &'static str, default: &str) -> Result<String, ConfigError> {
    let value = optional_string(name, default)?;
    validate_non_blank(name, &value, MAX_SCOPE_CHARS)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ConfigError::InvalidScope { name, value });
    }
    Ok(value)
}

fn validate_host_port(value: &str) -> Result<(), ConfigError> {
    if let Ok(address) = value.parse::<SocketAddr>() {
        return if address.port() == 0 {
            Err(ConfigError::ZeroRemoteAgentPort)
        } else {
            Ok(())
        };
    }

    let Some((host, port)) = value.rsplit_once(':') else {
        return Err(ConfigError::InvalidRemotePublicAddress(value.to_owned()));
    };
    validate_dns_name(host)?;
    let port = port
        .parse::<u16>()
        .map_err(|_| ConfigError::InvalidRemotePublicAddress(value.to_owned()))?;
    if port == 0 {
        return Err(ConfigError::ZeroRemoteAgentPort);
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

fn validate_https_url(name: &'static str, value: &str) -> Result<(), ConfigError> {
    let Ok(url) = reqwest::Url::parse(value) else {
        return Err(ConfigError::InvalidUrl { name });
    };
    if url.scheme() != "https" {
        return Err(ConfigError::HttpsRequired { name });
    }
    if !has_explicit_authority(value)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidUrl { name });
    }
    Ok(())
}

fn validate_api_base_url(value: &str) -> Result<(), ConfigError> {
    let Ok(url) = reqwest::Url::parse(value) else {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    };
    if !has_explicit_authority(value) {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    }
    let Some(host) = url.host_str() else {
        return Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()));
    };
    let allowed = match url.scheme() {
        "https" => true,
        "http" => {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(ConfigError::InvalidAkamaiApiBaseUrl(value.to_owned()))
    }
}

fn has_explicit_authority(value: &str) -> bool {
    value
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split(['/', '?', '#']).next())
        .is_some_and(|authority| !authority.is_empty())
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
            "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
            _ => Err(ConfigError::InvalidBoolean { name, value }),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn optional_string(name: &'static str, default: &str) -> Result<String, ConfigError> {
    match optional_string_value(name)? {
        Some(value) => Ok(value),
        None => Ok(default.to_owned()),
    }
}

fn optional_string_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn parse_socket_mode(value: Option<String>) -> Result<u32, ConfigError> {
    let mode = match value {
        Some(value) => u32::from_str_radix(value.trim_start_matches("0o"), 8)
            .map_err(|source| ConfigError::InvalidSocketMode { value, source })?,
        None => DEFAULT_SOCKET_MODE,
    };
    if mode > 0o777 {
        return Err(ConfigError::SocketModeOutOfRange(mode));
    }
    Ok(mode)
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
    #[error("{name} must be an integer, got {value}")]
    InvalidInteger {
        name: &'static str,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("socket mode must be an octal integer, got {value}")]
    InvalidSocketMode {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("socket mode must be between 0000 and 0777, got {0:o}")]
    SocketModeOutOfRange(u32),
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
    #[error("required environment variable {0} is missing")]
    MissingValue(&'static str),
    #[error("{0} must not be blank")]
    BlankValue(&'static str),
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{name} must be no longer than {maximum} characters")]
    ValueTooLong { name: &'static str, maximum: usize },
    #[error("{name} contains invalid scope characters: {value}")]
    InvalidScope { name: &'static str, value: String },
    #[error("{name} must be a boolean, got {value}")]
    InvalidBoolean { name: &'static str, value: String },
    #[error("local agent listen address is invalid")]
    InvalidSocketAddress(#[source] std::net::AddrParseError),
    #[error("remote agent listen address is invalid")]
    InvalidRemoteSocketAddress(#[source] std::net::AddrParseError),
    #[error("local agent listen address must be loopback, got {0}")]
    AgentAddressMustBeLoopback(SocketAddr),
    #[error("local agent listen port must be greater than zero")]
    ZeroAgentPort,
    #[error("remote agent listen port must be greater than zero")]
    ZeroRemoteAgentPort,
    #[error("remote agent public address must be HOST:PORT or a socket address: {0}")]
    InvalidRemotePublicAddress(String),
    #[error("remote agent settings require MCSERVER_CONTROL_PLANE_REMOTE_AGENT_LISTEN_ADDRESS")]
    RemoteAgentListenAddressRequired,
    #[error("Akamai settings require MCSERVER_AKAMAI_API_TOKEN")]
    AkamaiTokenRequired,
    #[error("Akamai provider requires remote agent TLS configuration")]
    RemoteAgentRequiredForAkamai,
    #[error("invalid TLS server name: {0}")]
    InvalidTlsServerName(String),
    #[error("{name} is not a valid URL")]
    InvalidUrl { name: &'static str },
    #[error("{name} must use https")]
    HttpsRequired { name: &'static str },
    #[error("invalid Akamai API base URL: {0}; use https, or loopback http for tests")]
    InvalidAkamaiApiBaseUrl(String),
    #[error("Akamai scope must be no longer than {maximum} characters")]
    AkamaiScopeTooLong { maximum: usize },
    #[error("node-agent SHA-256 must be 64 hexadecimal characters: {0}")]
    InvalidSha256(String),
}

#[cfg(test)]
mod tests {
    use super::{validate_api_base_url, validate_https_url};

    #[test]
    fn remote_download_requires_structurally_valid_https() {
        assert!(
            validate_https_url("TEST", "https://downloads.example.com/agent?version=1").is_ok()
        );
        assert!(validate_https_url("TEST", "http://downloads.example.com/agent").is_err());
        assert!(validate_https_url("TEST", "https:///missing-host").is_err());
        assert!(validate_https_url("TEST", "https://user:secret@example.com/agent").is_err());
    }

    #[test]
    fn akamai_api_allows_http_only_for_loopback_tests() {
        assert!(validate_api_base_url("https://api.linode.com/v4").is_ok());
        assert!(validate_api_base_url("http://127.0.0.1:3000/v4").is_ok());
        assert!(validate_api_base_url("http://[::1]:3000/v4").is_ok());
        assert!(validate_api_base_url("http://localhost.evil.example/v4").is_err());
        assert!(validate_api_base_url("https://api.linode.com/v4?token=secret").is_err());
    }
}
