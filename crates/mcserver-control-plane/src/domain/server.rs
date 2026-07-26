use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{TimestampError, UnixTimestampMillis};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(Uuid);

impl ServerId {
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

impl Default for ServerId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ServerId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerName(String);

impl ServerName {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        let trimmed = value.trim();

        if trimmed.is_empty() {
            return Err(ValidationError::BlankField("name"));
        }

        if trimmed.chars().count() > 128 {
            return Err(ValidationError::FieldTooLong {
                field: "name",
                maximum: 128,
            });
        }

        if trimmed.chars().any(char::is_control) {
            return Err(ValidationError::ControlCharacter("name"));
        }

        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Stopped,
}

impl DesiredState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ValidationError> {
        match value {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            _ => Err(ValidationError::InvalidPersistedValue {
                field: "desired_state",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeSpec {
    pub region: String,
    pub instance_type: String,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub container_image: String,
    pub server_type: String,
    pub version: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSpec {
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSpec {
    pub compute: ComputeSpec,
    pub process: ProcessSpec,
    pub data: DataSpec,
}

impl ServerSpec {
    pub fn validate(&self) -> Result<(), ValidationError> {
        require_non_blank("compute.region", &self.compute.region)?;
        require_non_blank("compute.instance_type", &self.compute.instance_type)?;
        require_non_blank("compute.image", &self.compute.image)?;
        require_non_blank("process.container_image", &self.process.container_image)?;
        require_non_blank("process.server_type", &self.process.server_type)?;
        require_non_blank("process.version", &self.process.version)?;
        require_non_blank("data.repository", &self.data.repository)?;

        for key in self.process.environment.keys() {
            require_non_blank("process.environment key", key)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub id: ServerId,
    pub name: ServerName,
    pub generation: u64,
    pub desired_state: DesiredState,
    pub spec: ServerSpec,
    pub created_at: UnixTimestampMillis,
    pub updated_at: UnixTimestampMillis,
}

impl Server {
    pub fn new(name: ServerName, spec: ServerSpec) -> Result<Self, ValidationError> {
        spec.validate()?;
        let now = UnixTimestampMillis::now()?;

        Ok(Self {
            id: ServerId::new(),
            name,
            generation: 1,
            desired_state: DesiredState::Stopped,
            spec,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: ServerId,
        name: ServerName,
        generation: u64,
        desired_state: DesiredState,
        spec: ServerSpec,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ValidationError> {
        if generation == 0 {
            return Err(ValidationError::ZeroGeneration);
        }
        if updated_at < created_at {
            return Err(ValidationError::InvalidTimestampOrder);
        }
        spec.validate()?;

        Ok(Self {
            id,
            name,
            generation,
            desired_state,
            spec,
            created_at,
            updated_at,
        })
    }

    pub fn set_desired_state(
        &mut self,
        desired_state: DesiredState,
    ) -> Result<bool, ValidationError> {
        if self.desired_state == desired_state {
            return Ok(false);
        }

        self.desired_state = desired_state;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ValidationError::GenerationOverflow)?;
        self.updated_at = std::cmp::max(self.updated_at, UnixTimestampMillis::now()?);
        Ok(true)
    }
}

fn require_non_blank(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::BlankField(field));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("{0} must not be blank")]
    BlankField(&'static str),
    #[error("{field} must be no longer than {maximum} characters")]
    FieldTooLong { field: &'static str, maximum: usize },
    #[error("{0} must not contain control characters")]
    ControlCharacter(&'static str),
    #[error("server generation must be greater than zero")]
    ZeroGeneration,
    #[error("server generation overflowed")]
    GenerationOverflow,
    #[error("server timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("system clock timestamp is invalid")]
    Timestamp(#[from] TimestampError),
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn new_server_starts_stopped_at_generation_one() -> Result<(), ValidationError> {
        let server = Server::new(ServerName::new("community")?, valid_spec())?;

        assert_eq!(server.generation, 1);
        assert_eq!(server.desired_state, DesiredState::Stopped);
        Ok(())
    }

    #[test]
    fn setting_same_desired_state_is_idempotent() -> Result<(), ValidationError> {
        let mut server = Server::new(ServerName::new("community")?, valid_spec())?;

        assert!(!server.set_desired_state(DesiredState::Stopped)?);
        assert_eq!(server.generation, 1);
        Ok(())
    }
}
