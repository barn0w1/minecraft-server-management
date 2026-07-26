use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{ServerId, ServerSpec, UnixTimestampMillis};

const MAX_SNAPSHOT_ID_CHARS: usize = 256;
const MAX_ERROR_CHARS: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerInstanceId(Uuid);

impl ServerInstanceId {
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

impl Default for ServerInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ServerInstanceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalResult {
    Completed,
    Failed,
}

impl TerminalResult {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ServerInstanceValidationError> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(ServerInstanceValidationError::InvalidPersistedValue {
                field: "terminal_result",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInstance {
    pub id: ServerInstanceId,
    pub server_id: ServerId,
    pub server_generation: u64,
    pub resolved_spec: ServerSpec,
    pub fencing_token: u64,
    pub source_snapshot_id: Option<String>,
    pub data_prepared_at: Option<UnixTimestampMillis>,
    pub process_running: bool,
    pub process_observed_at: Option<UnixTimestampMillis>,
    pub result_snapshot_id: Option<String>,
    pub stop_requested_at: Option<UnixTimestampMillis>,
    pub terminated_at: Option<UnixTimestampMillis>,
    pub terminal_result: Option<TerminalResult>,
    pub last_error: Option<String>,
    pub created_at: UnixTimestampMillis,
    pub updated_at: UnixTimestampMillis,
}

impl ServerInstance {
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ServerInstanceId,
        server_id: ServerId,
        server_generation: u64,
        resolved_spec: ServerSpec,
        fencing_token: u64,
        source_snapshot_id: Option<String>,
        data_prepared_at: Option<UnixTimestampMillis>,
        process_running: bool,
        process_observed_at: Option<UnixTimestampMillis>,
        result_snapshot_id: Option<String>,
        stop_requested_at: Option<UnixTimestampMillis>,
        terminated_at: Option<UnixTimestampMillis>,
        terminal_result: Option<TerminalResult>,
        last_error: Option<String>,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ServerInstanceValidationError> {
        if server_generation == 0 {
            return Err(ServerInstanceValidationError::ZeroServerGeneration);
        }
        if fencing_token == 0 {
            return Err(ServerInstanceValidationError::ZeroFencingToken);
        }
        resolved_spec
            .validate()
            .map_err(ServerInstanceValidationError::InvalidResolvedSpec)?;
        if terminated_at.is_some() != terminal_result.is_some() {
            return Err(ServerInstanceValidationError::IncompleteTerminalState);
        }
        if process_running && process_observed_at.is_none() {
            return Err(ServerInstanceValidationError::MissingProcessObservation);
        }
        if last_error
            .as_deref()
            .is_some_and(|message| message.trim().is_empty())
        {
            return Err(ServerInstanceValidationError::BlankLastError);
        }
        if last_error
            .as_deref()
            .is_some_and(|message| message.contains('\0'))
        {
            return Err(ServerInstanceValidationError::NulByte("last_error"));
        }
        if last_error
            .as_deref()
            .is_some_and(|message| message.chars().count() > MAX_ERROR_CHARS)
        {
            return Err(ServerInstanceValidationError::FieldTooLong {
                field: "last_error",
                maximum: MAX_ERROR_CHARS,
            });
        }
        for value in [
            data_prepared_at,
            process_observed_at,
            stop_requested_at,
            terminated_at,
        ]
        .into_iter()
        .flatten()
        {
            if value < created_at || value > updated_at {
                return Err(ServerInstanceValidationError::InvalidTimestampOrder);
            }
        }
        if updated_at < created_at {
            return Err(ServerInstanceValidationError::InvalidTimestampOrder);
        }
        if let Some(terminated) = terminated_at {
            for prior in [data_prepared_at, process_observed_at, stop_requested_at]
                .into_iter()
                .flatten()
            {
                if terminated < prior {
                    return Err(ServerInstanceValidationError::InvalidTimestampOrder);
                }
            }
        }
        for (field, value) in [
            ("source_snapshot_id", source_snapshot_id.as_deref()),
            ("result_snapshot_id", result_snapshot_id.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(ServerInstanceValidationError::BlankField(field));
            }
            if value.is_some_and(|value| value.contains('\0')) {
                return Err(ServerInstanceValidationError::NulByte(field));
            }
            if value.is_some_and(|value| value.chars().count() > MAX_SNAPSHOT_ID_CHARS) {
                return Err(ServerInstanceValidationError::FieldTooLong {
                    field,
                    maximum: MAX_SNAPSHOT_ID_CHARS,
                });
            }
        }

        Ok(Self {
            id,
            server_id,
            server_generation,
            resolved_spec,
            fencing_token,
            source_snapshot_id,
            data_prepared_at,
            process_running,
            process_observed_at,
            result_snapshot_id,
            stop_requested_at,
            terminated_at,
            terminal_result,
            last_error,
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
pub enum ServerInstanceValidationError {
    #[error("server generation must be greater than zero")]
    ZeroServerGeneration,
    #[error("fencing token must be greater than zero")]
    ZeroFencingToken,
    #[error("resolved server specification is invalid")]
    InvalidResolvedSpec(#[source] super::ValidationError),
    #[error("terminated_at and terminal_result must either both be set or both be absent")]
    IncompleteTerminalState,
    #[error("a running process must have an observation timestamp")]
    MissingProcessObservation,
    #[error("server instance timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("{0} must not be blank")]
    BlankField(&'static str),
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{field} must be no longer than {maximum} characters")]
    FieldTooLong { field: &'static str, maximum: usize },
    #[error("last_error must not be blank")]
    BlankLastError,
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}
