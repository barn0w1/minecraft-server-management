use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::UnixTimestampMillis;

const RESERVED_ENVIRONMENT_KEYS: [&str; 4] = ["EULA", "TYPE", "VERSION", "SKIP_SERVER_PROPERTIES"];
const MAX_CONTAINER_IMAGE_CHARS: usize = 512;
const MAX_SERVER_TYPE_CHARS: usize = 128;
const MAX_VERSION_CHARS: usize = 128;
const MAX_REPOSITORY_CHARS: usize = 4096;
const MAX_ENVIRONMENT_ENTRIES: usize = 128;
const MAX_ENVIRONMENT_KEY_CHARS: usize = 128;
const MAX_ENVIRONMENT_VALUE_CHARS: usize = 8192;
const MAX_SNAPSHOT_ID_CHARS: usize = 256;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ComputeSpec {
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub container_image: String,
    pub server_type: String,
    pub version: String,
    pub host_port: u16,
    pub stop_timeout_seconds: u64,
    pub accept_eula: bool,
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
        for (field, value, maximum) in [
            (
                "process.container_image",
                self.process.container_image.as_str(),
                MAX_CONTAINER_IMAGE_CHARS,
            ),
            (
                "process.server_type",
                self.process.server_type.as_str(),
                MAX_SERVER_TYPE_CHARS,
            ),
            (
                "process.version",
                self.process.version.as_str(),
                MAX_VERSION_CHARS,
            ),
            (
                "data.repository",
                self.data.repository.as_str(),
                MAX_REPOSITORY_CHARS,
            ),
        ] {
            require_non_blank(field, value)?;
            require_maximum_length(field, value, maximum)?;
            reject_nul(field, value)?;
        }

        if self.process.host_port == 0 {
            return Err(ValidationError::ZeroValue("process.host_port"));
        }
        if self.process.stop_timeout_seconds == 0 {
            return Err(ValidationError::ZeroValue(
                "process.stop_timeout_seconds",
            ));
        }
        if !self.process.accept_eula {
            return Err(ValidationError::EulaNotAccepted);
        }

        if self.process.environment.len() > MAX_ENVIRONMENT_ENTRIES {
            return Err(ValidationError::TooManyEnvironmentVariables {
                maximum: MAX_ENVIRONMENT_ENTRIES,
            });
        }
        for (key, value) in &self.process.environment {
            require_non_blank("process.environment key", key)?;
            require_maximum_length(
                "process.environment key",
                key,
                MAX_ENVIRONMENT_KEY_CHARS,
            )?;
            require_maximum_length(
                "process.environment value",
                value,
                MAX_ENVIRONMENT_VALUE_CHARS,
            )?;
            reject_nul("process.environment key", key)?;
            reject_nul("process.environment value", value)?;
            if !is_valid_environment_key(key) {
                return Err(ValidationError::InvalidEnvironmentKey(key.clone()));
            }
            if RESERVED_ENVIRONMENT_KEYS.contains(&key.as_str()) {
                return Err(ValidationError::ReservedEnvironmentKey(key.clone()));
            }
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
    pub current_snapshot_id: Option<String>,
    pub created_at: UnixTimestampMillis,
    pub updated_at: UnixTimestampMillis,
}

impl Server {
    pub fn new(
        id: ServerId,
        name: ServerName,
        spec: ServerSpec,
        now: UnixTimestampMillis,
    ) -> Result<Self, ValidationError> {
        spec.validate()?;

        Ok(Self {
            id,
            name,
            generation: 1,
            desired_state: DesiredState::Stopped,
            spec,
            current_snapshot_id: None,
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
        current_snapshot_id: Option<String>,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ValidationError> {
        if generation == 0 {
            return Err(ValidationError::ZeroGeneration);
        }
        if updated_at < created_at {
            return Err(ValidationError::InvalidTimestampOrder);
        }
        if let Some(snapshot_id) = current_snapshot_id.as_deref() {
            require_non_blank("current_snapshot_id", snapshot_id)?;
            require_maximum_length(
                "current_snapshot_id",
                snapshot_id,
                MAX_SNAPSHOT_ID_CHARS,
            )?;
            reject_nul("current_snapshot_id", snapshot_id)?;
        }
        spec.validate()?;

        Ok(Self {
            id,
            name,
            generation,
            desired_state,
            spec,
            current_snapshot_id,
            created_at,
            updated_at,
        })
    }

    pub fn set_desired_state(
        &mut self,
        desired_state: DesiredState,
        now: UnixTimestampMillis,
    ) -> Result<bool, ValidationError> {
        if self.desired_state == desired_state {
            return Ok(false);
        }

        self.desired_state = desired_state;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ValidationError::GenerationOverflow)?;
        self.updated_at = std::cmp::max(self.updated_at, now);
        Ok(true)
    }
}

fn require_non_blank(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::BlankField(field));
    }
    Ok(())
}

fn require_maximum_length(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), ValidationError> {
    if value.chars().count() > maximum {
        return Err(ValidationError::FieldTooLong { field, maximum });
    }
    Ok(())
}

fn is_valid_environment_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn reject_nul(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.contains('\0') {
        return Err(ValidationError::NulByte(field));
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
    #[error("{0} must not contain a NUL byte")]
    NulByte(&'static str),
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
    #[error("Minecraft EULA acceptance must be explicit")]
    EulaNotAccepted,
    #[error("process environment contains more than {maximum} entries")]
    TooManyEnvironmentVariables { maximum: usize },
    #[error("process environment key is invalid: {0}")]
    InvalidEnvironmentKey(String),
    #[error("process environment key is managed by the system: {0}")]
    ReservedEnvironmentKey(String),
    #[error("server generation must be greater than zero")]
    ZeroGeneration,
    #[error("server generation overflowed")]
    GenerationOverflow,
    #[error("server timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_spec() -> ServerSpec {
        ServerSpec {
            compute: ComputeSpec::Local,
            process: ProcessSpec {
                container_image: "docker.io/itzg/minecraft-server:latest".to_owned(),
                server_type: "VANILLA".to_owned(),
                version: "LATEST".to_owned(),
                host_port: 25_565,
                stop_timeout_seconds: 30,
                accept_eula: true,
                environment: BTreeMap::new(),
            },
            data: DataSpec {
                repository: "/tmp/mcserver-restic".to_owned(),
            },
        }
    }

    #[test]
    fn new_server_starts_stopped_at_generation_one() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let server = Server::new(ServerId::new(), ServerName::new("community")?, valid_spec(), now)?;

        assert_eq!(server.generation, 1);
        assert_eq!(server.desired_state, DesiredState::Stopped);
        Ok(())
    }

    #[test]
    fn setting_same_desired_state_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let mut server = Server::new(ServerId::new(), ServerName::new("community")?, valid_spec(), now)?;

        assert!(!server.set_desired_state(DesiredState::Stopped, now)?);
        assert_eq!(server.generation, 1);
        Ok(())
    }

    #[test]
    fn rejects_system_managed_environment_keys() {
        let mut spec = valid_spec();
        spec.process
            .environment
            .insert("EULA".to_owned(), "TRUE".to_owned());

        assert!(matches!(
            spec.validate(),
            Err(ValidationError::ReservedEnvironmentKey(key)) if key == "EULA"
        ));
    }

    #[test]
    fn rejects_invalid_environment_keys() {
        let mut spec = valid_spec();
        spec.process
            .environment
            .insert("INVALID-KEY".to_owned(), "value".to_owned());

        assert!(matches!(
            spec.validate(),
            Err(ValidationError::InvalidEnvironmentKey(key)) if key == "INVALID-KEY"
        ));
    }

    #[test]
    fn requires_explicit_eula_acceptance() {
        let mut spec = valid_spec();
        spec.process.accept_eula = false;

        assert!(matches!(spec.validate(), Err(ValidationError::EulaNotAccepted)));
    }
}
