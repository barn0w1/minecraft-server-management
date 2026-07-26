use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

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
    pub local_process_stop_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let socket_path = env::var_os("MCSERVER_CONTROL_PLANE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));
        let database_url = optional_string(
            "MCSERVER_CONTROL_PLANE_DATABASE_URL",
            DEFAULT_DATABASE_URL,
        )?;
        let socket_mode = parse_socket_mode(optional_string_value(
            "MCSERVER_CONTROL_PLANE_SOCKET_MODE",
        )?)?;
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
        let agent_listen_address = optional_string(
            "MCSERVER_CONTROL_PLANE_AGENT_LISTEN_ADDRESS",
            DEFAULT_AGENT_LISTEN_ADDRESS,
        )?
            .parse()
            .map_err(ConfigError::InvalidSocketAddress)?;
        if !agent_listen_address.ip().is_loopback() {
            return Err(ConfigError::AgentAddressMustBeLoopback(agent_listen_address));
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
        let local_process_stop_timeout = parse_positive_duration(
            "MCSERVER_CONTROL_PLANE_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS",
            DEFAULT_LOCAL_PROCESS_STOP_TIMEOUT_SECONDS,
        )?;

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
            local_process_stop_timeout,
        })
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
        Ok(value) => value.parse().map_err(|source| ConfigError::InvalidInteger {
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
        Ok(value) => value.parse().map_err(|source| ConfigError::InvalidInteger {
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
    #[error("agent listen address is invalid")]
    InvalidSocketAddress(#[source] std::net::AddrParseError),
    #[error("local agent listen address must be loopback, got {0}")]
    AgentAddressMustBeLoopback(SocketAddr),
    #[error("local agent listen port must be greater than zero")]
    ZeroAgentPort,
}
