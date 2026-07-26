use std::{env, path::PathBuf, time::Duration};

use thiserror::Error;

const DEFAULT_SOCKET_PATH: &str = "/run/mcserver/control-plane.sock";
const DEFAULT_DATABASE_URL: &str = "sqlite:///var/lib/mcserver/control-plane.db?mode=rwc";
const DEFAULT_SOCKET_MODE: u32 = 0o660;
const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_RECONCILE_INTERVAL_SECONDS: u64 = 30;

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: PathBuf,
    pub database_url: String,
    pub socket_mode: u32,
    pub max_frame_bytes: usize,
    pub reconcile_interval: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let socket_path = env::var_os("MCSERVER_CONTROL_PLANE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));
        let database_url = env::var("MCSERVER_CONTROL_PLANE_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        let socket_mode = parse_socket_mode(env::var("MCSERVER_CONTROL_PLANE_SOCKET_MODE").ok())?;
        let max_frame_bytes = parse_usize(
            "MCSERVER_CONTROL_PLANE_MAX_FRAME_BYTES",
            DEFAULT_MAX_FRAME_BYTES,
        )?;
        let reconcile_interval_seconds = parse_u64(
            "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS",
            DEFAULT_RECONCILE_INTERVAL_SECONDS,
        )?;

        if max_frame_bytes == 0 {
            return Err(ConfigError::ZeroValue(
                "MCSERVER_CONTROL_PLANE_MAX_FRAME_BYTES",
            ));
        }
        if reconcile_interval_seconds == 0 {
            return Err(ConfigError::ZeroValue(
                "MCSERVER_CONTROL_PLANE_RECONCILE_INTERVAL_SECONDS",
            ));
        }

        Ok(Self {
            socket_path,
            database_url,
            socket_mode,
            max_frame_bytes,
            reconcile_interval: Duration::from_secs(reconcile_interval_seconds),
        })
    }
}

fn parse_socket_mode(value: Option<String>) -> Result<u32, ConfigError> {
    match value {
        Some(value) => u32::from_str_radix(value.trim_start_matches("0o"), 8).map_err(|source| {
            ConfigError::InvalidSocketMode {
                value,
                source,
            }
        }),
        None => Ok(DEFAULT_SOCKET_MODE),
    }
}

fn parse_usize(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
}

fn parse_u64(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|source| ConfigError::InvalidInteger {
                name,
                value,
                source,
            }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(source) => Err(ConfigError::Environment { name, source }),
    }
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
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
}
