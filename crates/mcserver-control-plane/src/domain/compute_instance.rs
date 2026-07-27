use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{ServerInstanceId, UnixTimestampMillis};

const MAX_CONNECTION_TOKEN_CHARS: usize = 256;
const MAX_PROVIDER_INSTANCE_ID_CHARS: usize = 256;
const MAX_PUBLIC_IPV4_CHARS: usize = 64;
const MAX_FAILURE_MESSAGE_CHARS: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComputeInstanceId(Uuid);

impl ComputeInstanceId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ComputeInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ComputeInstanceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeProvider {
    LocalProcess,
    Akamai,
}

impl ComputeProvider {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalProcess => "local_process",
            Self::Akamai => "akamai",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ComputeInstanceValidationError> {
        match value {
            "local_process" => Ok(Self::LocalProcess),
            "akamai" => Ok(Self::Akamai),
            _ => Err(ComputeInstanceValidationError::InvalidPersistedValue {
                field: "provider",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeTerminalResult {
    Deleted,
    Failed,
}

impl ComputeTerminalResult {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deleted => "deleted",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ComputeInstanceValidationError> {
        match value {
            "deleted" => Ok(Self::Deleted),
            "failed" => Ok(Self::Failed),
            _ => Err(ComputeInstanceValidationError::InvalidPersistedValue {
                field: "terminal_result",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ComputeInstance {
    pub id: ComputeInstanceId,
    pub server_instance_id: ServerInstanceId,
    pub provider: ComputeProvider,
    pub provider_instance_id: Option<String>,
    pub public_ipv4: Option<String>,
    pub connection_token: String,
    pub enrollment_token: Option<String>,
    pub process_id: Option<u32>,
    pub agent_connected_at: Option<UnixTimestampMillis>,
    pub shutdown_requested_at: Option<UnixTimestampMillis>,
    pub terminated_at: Option<UnixTimestampMillis>,
    pub terminal_result: Option<ComputeTerminalResult>,
    pub failure_message: Option<String>,
    pub created_at: UnixTimestampMillis,
    pub updated_at: UnixTimestampMillis,
}

impl std::fmt::Debug for ComputeInstance {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComputeInstance")
            .field("id", &self.id)
            .field("server_instance_id", &self.server_instance_id)
            .field("provider", &self.provider)
            .field("provider_instance_id", &self.provider_instance_id)
            .field("public_ipv4", &self.public_ipv4)
            .field("connection_token", &"[REDACTED]")
            .field(
                "enrollment_token",
                &self.enrollment_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("process_id", &self.process_id)
            .field("agent_connected_at", &self.agent_connected_at)
            .field("shutdown_requested_at", &self.shutdown_requested_at)
            .field("terminated_at", &self.terminated_at)
            .field("terminal_result", &self.terminal_result)
            .field("failure_message", &self.failure_message)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl ComputeInstance {
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ComputeInstanceId,
        server_instance_id: ServerInstanceId,
        provider: ComputeProvider,
        provider_instance_id: Option<String>,
        public_ipv4: Option<String>,
        connection_token: String,
        enrollment_token: Option<String>,
        process_id: Option<u32>,
        agent_connected_at: Option<UnixTimestampMillis>,
        shutdown_requested_at: Option<UnixTimestampMillis>,
        terminated_at: Option<UnixTimestampMillis>,
        terminal_result: Option<ComputeTerminalResult>,
        failure_message: Option<String>,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ComputeInstanceValidationError> {
        validate_required("connection_token", &connection_token, MAX_CONNECTION_TOKEN_CHARS)?;
        validate_optional(
            "enrollment_token",
            enrollment_token.as_deref(),
            MAX_CONNECTION_TOKEN_CHARS,
        )?;
        validate_optional(
            "provider_instance_id",
            provider_instance_id.as_deref(),
            MAX_PROVIDER_INSTANCE_ID_CHARS,
        )?;
        validate_optional(
            "public_ipv4",
            public_ipv4.as_deref(),
            MAX_PUBLIC_IPV4_CHARS,
        )?;
        match provider {
            ComputeProvider::LocalProcess => {
                if provider_instance_id.is_some()
                    || public_ipv4.is_some()
                    || enrollment_token.is_some()
                {
                    return Err(ComputeInstanceValidationError::InvalidProviderFields(provider));
                }
            }
            ComputeProvider::Akamai => {
                if process_id.is_some() {
                    return Err(ComputeInstanceValidationError::InvalidProviderFields(provider));
                }
                if let Some(address) = public_ipv4.as_deref() {
                    address.parse::<std::net::Ipv4Addr>().map_err(|_| {
                        ComputeInstanceValidationError::InvalidPublicIpv4(address.to_owned())
                    })?;
                }
            }
        }
        if terminated_at.is_some() != terminal_result.is_some() {
            return Err(ComputeInstanceValidationError::IncompleteTerminalState);
        }
        for timestamp in [agent_connected_at, shutdown_requested_at, terminated_at]
            .into_iter()
            .flatten()
        {
            if timestamp < created_at || timestamp > updated_at {
                return Err(ComputeInstanceValidationError::InvalidTimestampOrder);
            }
        }
        if updated_at < created_at {
            return Err(ComputeInstanceValidationError::InvalidTimestampOrder);
        }
        if let Some(terminated) = terminated_at {
            for prior in [agent_connected_at, shutdown_requested_at]
                .into_iter()
                .flatten()
            {
                if terminated < prior {
                    return Err(ComputeInstanceValidationError::InvalidTimestampOrder);
                }
            }
        }
        validate_optional(
            "failure_message",
            failure_message.as_deref(),
            MAX_FAILURE_MESSAGE_CHARS,
        )?;

        Ok(Self {
            id,
            server_instance_id,
            provider,
            provider_instance_id,
            public_ipv4,
            connection_token,
            enrollment_token,
            process_id,
            agent_connected_at,
            shutdown_requested_at,
            terminated_at,
            terminal_result,
            failure_message,
            created_at,
            updated_at,
        })
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.terminated_at.is_none()
    }
}

fn validate_required(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), ComputeInstanceValidationError> {
    if value.trim().is_empty() {
        return Err(ComputeInstanceValidationError::BlankField(field));
    }
    if value.contains('\0') {
        return Err(ComputeInstanceValidationError::NulByte(field));
    }
    if value.chars().count() > maximum {
        return Err(ComputeInstanceValidationError::FieldTooLong { field, maximum });
    }
    Ok(())
}

fn validate_optional(
    field: &'static str,
    value: Option<&str>,
    maximum: usize,
) -> Result<(), ComputeInstanceValidationError> {
    if let Some(value) = value {
        validate_required(field, value, maximum)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ComputeInstanceValidationError {
    #[error("{0} must not be blank")]
    BlankField(&'static str),
    #[error("provider-specific compute fields are invalid for {0:?}")]
    InvalidProviderFields(ComputeProvider),
    #[error("invalid public IPv4 address: {0}")]
    InvalidPublicIpv4(String),
    #[error("terminated_at and terminal_result must either both be set or both be absent")]
    IncompleteTerminalState,
    #[error("compute instance timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{field} must be no longer than {maximum} characters")]
    FieldTooLong { field: &'static str, maximum: usize },
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}
