use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use thiserror::Error;
use uuid::Uuid;

const DEFAULT_PODMAN_BINARY: &str = "podman";
const DEFAULT_RESTIC_BINARY: &str = "restic";
const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_RESTIC_RETRY_LOCK_SECONDS: u64 = 300;
const DEFAULT_RECONNECT_MIN_SECONDS: u64 = 1;
const DEFAULT_RECONNECT_MAX_SECONDS: u64 = 30;

#[derive(Debug, Clone)]
pub struct Config {
    pub control_plane_address: SocketAddr,
    pub compute_instance_id: Uuid,
    pub connection_token: String,
    pub state_directory: PathBuf,
    pub podman_binary: PathBuf,
    pub restic_binary: PathBuf,
    pub max_frame_bytes: usize,
    pub command_timeout: Duration,
    pub restic_retry_lock: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let control_plane_address = required("MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS")?
            .parse()
            .map_err(ConfigError::InvalidSocketAddress)?;
        if !control_plane_address.ip().is_loopback() {
            return Err(ConfigError::ControlPlaneAddressMustBeLoopback(
                control_plane_address,
            ));
        }
        let compute_instance_id = required("MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID")?
            .parse()
            .map_err(ConfigError::InvalidUuid)?;
        let connection_token = required("MCSERVER_NODE_AGENT_CONNECTION_TOKEN")?;
        if connection_token.trim().is_empty() {
            return Err(ConfigError::BlankValue(
                "MCSERVER_NODE_AGENT_CONNECTION_TOKEN",
            ));
        }
        let state_directory = PathBuf::from(required("MCSERVER_NODE_AGENT_STATE_DIRECTORY")?);
        let podman_binary = env::var_os("MCSERVER_NODE_AGENT_PODMAN_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PODMAN_BINARY));
        let restic_binary = env::var_os("MCSERVER_NODE_AGENT_RESTIC_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_RESTIC_BINARY));
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
            compute_instance_id,
            connection_token,
            state_directory,
            podman_binary,
            restic_binary,
            max_frame_bytes,
            command_timeout,
            restic_retry_lock,
            reconnect_min,
            reconnect_max,
        })
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => Err(ConfigError::BlankValue(name)),
        Ok(value) => Ok(value),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
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
    #[error("{0} must not be blank")]
    BlankValue(&'static str),
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
    #[error("local control-plane address must be loopback, got {0}")]
    ControlPlaneAddressMustBeLoopback(SocketAddr),
    #[error("compute instance id is invalid")]
    InvalidUuid(#[source] uuid::Error),
    #[error("reconnect maximum must not be less than the minimum")]
    InvalidReconnectRange,
}
