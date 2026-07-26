use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{ServerId, ServerSpec, UnixTimestampMillis};

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
    pub stop_requested_at: Option<UnixTimestampMillis>,
    pub terminated_at: Option<UnixTimestampMillis>,
    pub terminal_result: Option<TerminalResult>,
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
        stop_requested_at: Option<UnixTimestampMillis>,
        terminated_at: Option<UnixTimestampMillis>,
        terminal_result: Option<TerminalResult>,
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
        if updated_at < created_at
            || stop_requested_at.is_some_and(|requested| requested > updated_at)
            || terminated_at.is_some_and(|terminated| terminated > updated_at)
        {
            return Err(ServerInstanceValidationError::InvalidTimestampOrder);
        }
        if terminated_at.is_some_and(|terminated| terminated < created_at) {
            return Err(ServerInstanceValidationError::InvalidTimestampOrder);
        }
        if stop_requested_at.is_some_and(|requested| requested < created_at) {
            return Err(ServerInstanceValidationError::InvalidTimestampOrder);
        }
        if stop_requested_at
            .zip(terminated_at)
            .is_some_and(|(requested, terminated)| terminated < requested)
        {
            return Err(ServerInstanceValidationError::InvalidTimestampOrder);
        }

        Ok(Self {
            id,
            server_id,
            server_generation,
            resolved_spec,
            fencing_token,
            stop_requested_at,
            terminated_at,
            terminal_result,
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
    #[error("server instance timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::{ComputeSpec, DataSpec, ProcessSpec};

    fn valid_spec() -> ServerSpec {
        ServerSpec {
            compute: ComputeSpec {
                region: "jp-osa".to_owned(),
                instance_type: "g6-standard-2".to_owned(),
                image: "debian-13".to_owned(),
            },
            process: ProcessSpec {
                container_image: "docker.io/itzg/minecraft-server:latest".to_owned(),
                server_type: "VANILLA".to_owned(),
                version: "LATEST".to_owned(),
                environment: BTreeMap::new(),
            },
            data: DataSpec {
                repository: "r2:mcserver/example".to_owned(),
            },
        }
    }

    #[test]
    fn terminal_fields_must_be_set_together() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let result = ServerInstance::rehydrate(
            ServerInstanceId::new(),
            ServerId::new(),
            1,
            valid_spec(),
            1,
            None,
            Some(now),
            None,
            now,
            now,
        );

        assert!(matches!(
            result,
            Err(ServerInstanceValidationError::IncompleteTerminalState)
        ));
        Ok(())
    }
}
