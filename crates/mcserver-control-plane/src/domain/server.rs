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
const MAX_AKAMAI_REGION_CHARS: usize = 64;
const MAX_AKAMAI_TYPE_CHARS: usize = 128;
const MAX_AKAMAI_IMAGE_CHARS: usize = 256;
const MAX_SERVER_NAME_CHARS: usize = 63;

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
        if value.is_empty() {
            return Err(ValidationError::BlankField("name"));
        }
        if value.len() > MAX_SERVER_NAME_CHARS {
            return Err(ValidationError::FieldTooLong {
                field: "name",
                maximum: MAX_SERVER_NAME_CHARS,
            });
        }
        let bytes = value.as_bytes();
        if !bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || !bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(ValidationError::InvalidServerName(value));
        }

        Ok(Self(value))
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
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ComputeSpec {
    Local,
    Akamai {
        region: String,
        instance_type: String,
        image: String,
        firewall_id: u64,
    },
}

impl ComputeSpec {
    fn validate(&self) -> Result<(), ValidationError> {
        let Self::Akamai {
            region,
            instance_type,
            image,
            firewall_id,
        } = self
        else {
            return Ok(());
        };

        for (field, value, maximum) in [
            ("compute.region", region.as_str(), MAX_AKAMAI_REGION_CHARS),
            (
                "compute.instance_type",
                instance_type.as_str(),
                MAX_AKAMAI_TYPE_CHARS,
            ),
            ("compute.image", image.as_str(), MAX_AKAMAI_IMAGE_CHARS),
        ] {
            require_non_blank(field, value)?;
            require_maximum_length(field, value, maximum)?;
            reject_nul(field, value)?;
            if !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'.')
            }) {
                return Err(ValidationError::InvalidComputeIdentifier {
                    field,
                    value: value.to_owned(),
                });
            }
        }
        if *firewall_id == 0 {
            return Err(ValidationError::ZeroValue("compute.firewall_id"));
        }
        Ok(())
    }
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
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum DesiredDataSpec {
    LocalRestic { repository: String },
    R2Restic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBackend {
    LocalRestic,
    R2Restic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSpec {
    pub backend: DataBackend,
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredServerSpec {
    pub compute: ComputeSpec,
    pub process: ProcessSpec,
    pub data: DesiredDataSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSpec {
    pub compute: ComputeSpec,
    pub process: ProcessSpec,
    pub data: DataSpec,
}

impl ServerSpec {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.compute.validate()?;
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
            return Err(ValidationError::ZeroValue("process.stop_timeout_seconds"));
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
            require_maximum_length("process.environment key", key, MAX_ENVIRONMENT_KEY_CHARS)?;
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

impl DesiredServerSpec {
    pub fn resolve(
        self,
        existing_data: Option<&DataSpec>,
        r2_repository: Option<String>,
    ) -> Result<ServerSpec, ValidationError> {
        let data = match self.data {
            DesiredDataSpec::LocalRestic { repository } => match existing_data {
                Some(data) if data.backend != DataBackend::LocalRestic => {
                    return Err(ValidationError::DataBackendImmutable);
                }
                Some(data) if data.repository != repository => {
                    return Err(ValidationError::DataRepositoryImmutable);
                }
                Some(data) => data.clone(),
                None => DataSpec {
                    backend: DataBackend::LocalRestic,
                    repository,
                },
            },
            DesiredDataSpec::R2Restic => {
                let repository = match existing_data {
                    Some(data) if data.backend == DataBackend::R2Restic => data.repository.clone(),
                    Some(_) => return Err(ValidationError::DataBackendImmutable),
                    None => r2_repository.ok_or(ValidationError::R2Unavailable)?,
                };
                DataSpec {
                    backend: DataBackend::R2Restic,
                    repository,
                }
            }
        };
        let spec = ServerSpec {
            compute: self.compute,
            process: self.process,
            data,
        };
        spec.validate()?;
        Ok(spec)
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
    pub archived_at: Option<UnixTimestampMillis>,
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
            archived_at: None,
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
        archived_at: Option<UnixTimestampMillis>,
        created_at: UnixTimestampMillis,
        updated_at: UnixTimestampMillis,
    ) -> Result<Self, ValidationError> {
        if generation == 0 {
            return Err(ValidationError::ZeroGeneration);
        }
        if updated_at < created_at {
            return Err(ValidationError::InvalidTimestampOrder);
        }
        if archived_at
            .is_some_and(|archived_at| archived_at < created_at || archived_at > updated_at)
        {
            return Err(ValidationError::InvalidTimestampOrder);
        }
        if archived_at.is_some() && desired_state != DesiredState::Stopped {
            return Err(ValidationError::ArchivedServerMustBeStopped);
        }
        if let Some(snapshot_id) = current_snapshot_id.as_deref() {
            require_non_blank("current_snapshot_id", snapshot_id)?;
            require_maximum_length("current_snapshot_id", snapshot_id, MAX_SNAPSHOT_ID_CHARS)?;
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
            archived_at,
            created_at,
            updated_at,
        })
    }

    pub fn set_desired_state(
        &mut self,
        desired_state: DesiredState,
        now: UnixTimestampMillis,
    ) -> Result<bool, ValidationError> {
        if self.archived_at.is_some() {
            return Err(ValidationError::ServerArchived);
        }
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

    pub fn update_spec(
        &mut self,
        spec: ServerSpec,
        now: UnixTimestampMillis,
    ) -> Result<bool, ValidationError> {
        spec.validate()?;
        if self.archived_at.is_some() {
            return Err(ValidationError::ServerArchived);
        }
        if self.spec == spec {
            return Ok(false);
        }
        if self.desired_state != DesiredState::Stopped {
            return Err(ValidationError::ServerMustBeStopped);
        }
        self.spec = spec;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ValidationError::GenerationOverflow)?;
        self.updated_at = std::cmp::max(self.updated_at, now);
        Ok(true)
    }

    pub fn archive(&mut self, now: UnixTimestampMillis) -> Result<bool, ValidationError> {
        if self.archived_at.is_some() {
            return Ok(false);
        }
        if self.desired_state != DesiredState::Stopped {
            return Err(ValidationError::ServerMustBeStopped);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ValidationError::GenerationOverflow)?;
        self.updated_at = std::cmp::max(self.updated_at, now);
        self.archived_at = Some(self.updated_at);
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
    #[error(
        "server name must be a 1-63 character lowercase DNS label using only letters, digits, and hyphens"
    )]
    InvalidServerName(String),
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
    #[error("server specification can only be changed while stopped")]
    ServerMustBeStopped,
    #[error("archived server must be stopped")]
    ArchivedServerMustBeStopped,
    #[error("archived server cannot be changed or started")]
    ServerArchived,
    #[error("server data backend cannot be changed")]
    DataBackendImmutable,
    #[error("server data repository cannot be changed")]
    DataRepositoryImmutable,
    #[error("R2 storage is not configured")]
    R2Unavailable,
    #[error("server timestamps are not in chronological order")]
    InvalidTimestampOrder,
    #[error("invalid persisted value for {field}: {value}")]
    InvalidPersistedValue { field: &'static str, value: String },
    #[error("{field} contains unsupported characters: {value}")]
    InvalidComputeIdentifier { field: &'static str, value: String },
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
                backend: DataBackend::LocalRestic,
                repository: "/tmp/mcserver-restic".to_owned(),
            },
        }
    }

    #[test]
    fn new_server_starts_stopped_at_generation_one() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            now,
        )?;

        assert_eq!(server.generation, 1);
        assert_eq!(server.desired_state, DesiredState::Stopped);
        assert!(server.archived_at.is_none());
        Ok(())
    }

    #[test]
    fn server_names_are_lowercase_dns_labels() -> Result<(), Box<dyn std::error::Error>> {
        for valid in ["a", "1", "community", "survival-2026", &"a".repeat(63)] {
            assert_eq!(ServerName::new(valid)?.as_str(), valid);
        }

        for invalid in [
            "",
            "-community",
            "community-",
            "Community",
            "community_server",
            "community server",
            "コミュニティ",
        ] {
            assert!(
                ServerName::new(invalid).is_err(),
                "{invalid:?} was accepted"
            );
        }
        assert!(ServerName::new("a".repeat(64)).is_err());
        Ok(())
    }

    #[test]
    fn setting_same_desired_state_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let mut server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            now,
        )?;

        assert!(!server.set_desired_state(DesiredState::Stopped, now)?);
        assert_eq!(server.generation, 1);
        Ok(())
    }

    #[test]
    fn resolves_new_r2_repository_and_preserves_it_on_update()
    -> Result<(), Box<dyn std::error::Error>> {
        let desired = DesiredServerSpec {
            compute: ComputeSpec::Local,
            process: valid_spec().process,
            data: DesiredDataSpec::R2Restic,
        };
        let repository = "s3:https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com/mcserver/servers/id/restic";
        let created = desired.clone().resolve(None, Some(repository.to_owned()))?;
        let updated = desired.resolve(Some(&created.data), None)?;

        assert_eq!(created.data.backend, DataBackend::R2Restic);
        assert_eq!(created.data.repository, repository);
        assert_eq!(updated.data, created.data);
        Ok(())
    }

    #[test]
    fn rejects_data_backend_changes() {
        let desired = DesiredServerSpec {
            compute: ComputeSpec::Local,
            process: valid_spec().process,
            data: DesiredDataSpec::R2Restic,
        };

        assert!(matches!(
            desired.resolve(Some(&valid_spec().data), None),
            Err(ValidationError::DataBackendImmutable)
        ));
    }

    #[test]
    fn rejects_local_repository_changes() {
        let existing = valid_spec().data;
        let desired = DesiredServerSpec {
            compute: ComputeSpec::Local,
            process: valid_spec().process,
            data: DesiredDataSpec::LocalRestic {
                repository: "/tmp/other-restic".to_owned(),
            },
        };

        assert!(matches!(
            desired.resolve(Some(&existing), None),
            Err(ValidationError::DataRepositoryImmutable)
        ));
    }

    #[test]
    fn specification_changes_require_a_stopped_server() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let mut server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            now,
        )?;
        server.set_desired_state(DesiredState::Running, now)?;
        let mut changed = valid_spec();
        changed.process.version = "1.21.8".to_owned();

        assert!(matches!(
            server.update_spec(changed, now),
            Err(ValidationError::ServerMustBeStopped)
        ));
        Ok(())
    }

    #[test]
    fn identical_specification_is_idempotent_while_running()
    -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let mut server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            now,
        )?;
        server.set_desired_state(DesiredState::Running, now)?;

        assert!(!server.update_spec(valid_spec(), now)?);
        assert_eq!(server.generation, 2);
        Ok(())
    }

    #[test]
    fn stopped_server_can_be_archived_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UnixTimestampMillis::from_millis(1_000)?;
        let archived_at = UnixTimestampMillis::from_millis(2_000)?;
        let mut server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            created_at,
        )?;

        assert!(server.archive(archived_at)?);
        assert_eq!(server.generation, 2);
        assert_eq!(server.archived_at, Some(archived_at));
        assert!(!server.archive(UnixTimestampMillis::from_millis(3_000)?)?);
        assert_eq!(server.generation, 2);
        assert!(matches!(
            server.set_desired_state(DesiredState::Running, archived_at),
            Err(ValidationError::ServerArchived)
        ));
        assert!(matches!(
            server.update_spec(valid_spec(), archived_at),
            Err(ValidationError::ServerArchived)
        ));
        Ok(())
    }

    #[test]
    fn running_server_cannot_be_archived() -> Result<(), Box<dyn std::error::Error>> {
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let mut server = Server::new(
            ServerId::new(),
            ServerName::new("community")?,
            valid_spec(),
            now,
        )?;
        server.set_desired_state(DesiredState::Running, now)?;

        assert!(matches!(
            server.archive(now),
            Err(ValidationError::ServerMustBeStopped)
        ));
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

        assert!(matches!(
            spec.validate(),
            Err(ValidationError::EulaNotAccepted)
        ));
    }

    #[test]
    fn accepts_valid_akamai_compute_specification() {
        let mut spec = valid_spec();
        spec.compute = ComputeSpec::Akamai {
            region: "jp-tyo-3".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            firewall_id: 123,
        };

        assert!(spec.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_akamai_compute_identifier() {
        let mut spec = valid_spec();
        spec.compute = ComputeSpec::Akamai {
            region: "us east".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            firewall_id: 123,
        };

        assert!(matches!(
            spec.validate(),
            Err(ValidationError::InvalidComputeIdentifier {
                field: "compute.region",
                ..
            })
        ));
    }

    #[test]
    fn rejects_zero_akamai_firewall_id() {
        let mut spec = valid_spec();
        spec.compute = ComputeSpec::Akamai {
            region: "jp-tyo-3".to_owned(),
            instance_type: "g6-nanode-1".to_owned(),
            image: "linode/debian13".to_owned(),
            firewall_id: 0,
        };

        assert!(matches!(
            spec.validate(),
            Err(ValidationError::ZeroValue("compute.firewall_id"))
        ));
    }
}
