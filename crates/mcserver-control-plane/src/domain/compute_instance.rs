use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{ServerInstanceId, UnixTimestampMillis};

const MAX_CONNECTION_TOKEN_CHARS: usize = 256;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeInstance {
    pub id: ComputeInstanceId,
    pub server_instance_id: ServerInstanceId,
    pub connection_token: String,
    pub process_id: Option<u32>,
    pub agent_connected_at: Option<UnixTimestampMillis>,
    pub shutdown_requested_at: Option<UnixTimestampMillis>,
    pub terminated_at: Option<UnixTimestampMillis>,
    pub terminal_result: Option<ComputeTerminalResult>,
    pub failure_message: Option<String>,
    pub created_at: UnixTimestampMillis,
    pub updated_at: UnixTimestampMillis,
}

impl ComputeInstance {
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ComputeInstanceId,
        server_instance_id: ServerInstanceId,
        connection_token: String,
        process_id: Option<u32>,
        agent_connected_at: Option<UnixTimestampMillis>,
        shutdown_requested_at: Option<UnixTimestampMillis>,
        terminated_at: Option<UnixTimestampMillis>,
        terminal_result: Option<ComputeTerminalResult>,
        failure_message: Option<String>,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ComputeInstanceValidationError> {
        if connection_token.trim().is_empty() {
            return Err(ComputeInstanceValidationError::BlankConnectionToken);
        }
        if connection_token.contains('\0') {
            return Err(ComputeInstanceValidationError::NulByte("connection_token"));
        }
        if connection_token.chars().count() > MAX_CONNECTION_TOKEN_CHARS {
            return Err(ComputeInstanceValidationError::FieldTooLong {
                field: "connection_token",
                maximum: MAX_CONNECTION_TOKEN_CHARS,
            });
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
        if failure_message
            .as_deref()
            .is_some_and(|message| message.trim().is_empty())
        {
            return Err(ComputeInstanceValidationError::BlankFailureMessage);
        }
        if failure_message
            .as_deref()
            .is_some_and(|message| message.contains('\0'))
        {
            return Err(ComputeInstanceValidationError::NulByte("failure_message"));
        }
        if failure_message
            .as_deref()
            .is_some_and(|message| message.chars().count() > MAX_FAILURE_MESSAGE_CHARS)
        {
            return Err(ComputeInstanceValidationError::FieldTooLong {
                field: "failure_message",
                maximum: MAX_FAILURE_MESSAGE_CHARS,
            });
        }

        Ok(Self {
            id,
            server_instance_id,
            connection_token,
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

#[derive(Debug, Error)]
pub enum ComputeInstanceValidationError {
    #[error("compute instance connection token must not be blank")]
    BlankConnectionToken,
    #[error("terminated_at and terminal_result must either both be set or both be absent")]
    IncompleteTerminalState,
    #[error("compute instance timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("compute instance failure message must not be blank")]
    BlankFailureMessage,
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{field} must be no longer than {maximum} characters")]
    FieldTooLong { field: &'static str, maximum: usize },
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}
