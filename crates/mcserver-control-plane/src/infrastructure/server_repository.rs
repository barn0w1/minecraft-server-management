use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    DesiredState, Server, ServerId, ServerName, ServerSpec, UnixTimestampMillis, ValidationError,
};

#[derive(Debug, Clone)]
pub struct ServerRepository {
    pool: SqlitePool,
}

impl ServerRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, server: &Server) -> Result<(), RepositoryError> {
        let spec_json = serde_json::to_string(&server.spec)?;
        let result = sqlx::query(
            r#"
            INSERT INTO servers (
                id,
                name,
                generation,
                desired_state,
                spec_json,
                current_snapshot_id,
                archived_at_ms,
                created_at_ms,
                updated_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(server.id.to_string())
        .bind(server.name.as_str())
        .bind(generation_to_i64(server.generation)?)
        .bind(server.desired_state.as_str())
        .bind(spec_json)
        .bind(server.current_snapshot_id.as_deref())
        .bind(server.archived_at.map(UnixTimestampMillis::as_millis))
        .bind(server.created_at.as_millis())
        .bind(server.updated_at.as_millis())
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(error) if is_unique_violation(&error) => {
                Err(RepositoryError::Conflict("server name already exists"))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn get(&self, id: ServerId) -> Result<Option<Server>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                name,
                generation,
                desired_state,
                spec_json,
                current_snapshot_id,
                archived_at_ms,
                created_at_ms,
                updated_at_ms
            FROM servers
            WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(decode_server).transpose()
    }

    pub async fn get_by_name(&self, name: &ServerName) -> Result<Option<Server>, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                name,
                generation,
                desired_state,
                spec_json,
                current_snapshot_id,
                archived_at_ms,
                created_at_ms,
                updated_at_ms
            FROM servers
            WHERE name = ?
            "#,
        )
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(decode_server).transpose()
    }

    pub async fn list(&self, include_archived: bool) -> Result<Vec<Server>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                name,
                generation,
                desired_state,
                spec_json,
                current_snapshot_id,
                archived_at_ms,
                created_at_ms,
                updated_at_ms
            FROM servers
            WHERE archived_at_ms IS NULL OR ?
            ORDER BY name, id
            "#,
        )
        .bind(include_archived)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_server).collect()
    }

    pub async fn list_for_reconciliation(&self) -> Result<Vec<Server>, RepositoryError> {
        self.list(false).await
    }

    pub async fn has_active_instance(&self, id: ServerId) -> Result<bool, RepositoryError> {
        let active = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM server_instances
                WHERE server_id = ?
                  AND terminated_at_ms IS NULL
            )
            "#,
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(active)
    }

    pub async fn update_desired_state(
        &self,
        server: &Server,
        previous_generation: u64,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE servers
            SET desired_state = ?, generation = ?, updated_at_ms = ?
            WHERE id = ? AND generation = ? AND archived_at_ms IS NULL
            "#,
        )
        .bind(server.desired_state.as_str())
        .bind(generation_to_i64(server.generation)?)
        .bind(server.updated_at.as_millis())
        .bind(server.id.to_string())
        .bind(generation_to_i64(previous_generation)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn update_spec(
        &self,
        server: &Server,
        previous_generation: u64,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE servers
            SET spec_json = ?, generation = ?, updated_at_ms = ?
            WHERE id = ?
              AND generation = ?
              AND desired_state = 'stopped'
              AND archived_at_ms IS NULL
            "#,
        )
        .bind(serde_json::to_string(&server.spec)?)
        .bind(generation_to_i64(server.generation)?)
        .bind(server.updated_at.as_millis())
        .bind(server.id.to_string())
        .bind(generation_to_i64(previous_generation)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn archive(
        &self,
        server: &Server,
        previous_generation: u64,
    ) -> Result<bool, RepositoryError> {
        let archived_at = server
            .archived_at
            .ok_or(RepositoryError::InvalidArchiveState)?;
        let result = sqlx::query(
            r#"
            UPDATE servers
            SET archived_at_ms = ?, generation = ?, updated_at_ms = ?
            WHERE id = ?
              AND generation = ?
              AND desired_state = 'stopped'
              AND archived_at_ms IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM server_instances
                  WHERE server_id = servers.id
                    AND terminated_at_ms IS NULL
              )
            "#,
        )
        .bind(archived_at.as_millis())
        .bind(generation_to_i64(server.generation)?)
        .bind(server.updated_at.as_millis())
        .bind(server.id.to_string())
        .bind(generation_to_i64(previous_generation)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}

fn decode_server(row: &SqliteRow) -> Result<Server, RepositoryError> {
    let id = decode_uuid(row.try_get::<String, _>("id")?).map(ServerId::from_uuid)?;
    let name = ServerName::new(row.try_get::<String, _>("name")?)?;
    let generation = i64_to_positive_u64(row.try_get("generation")?)?;
    let desired_state = DesiredState::parse(&row.try_get::<String, _>("desired_state")?)?;
    let spec = serde_json::from_str::<ServerSpec>(&row.try_get::<String, _>("spec_json")?)?;
    let current_snapshot_id = row.try_get("current_snapshot_id")?;
    let archived_at = row
        .try_get::<Option<i64>, _>("archived_at_ms")?
        .map(decode_timestamp)
        .transpose()?;
    let created_at = decode_timestamp(row.try_get("created_at_ms")?)?;
    let updated_at = decode_timestamp(row.try_get("updated_at_ms")?)?;

    Server::rehydrate(
        id,
        name,
        generation,
        desired_state,
        spec,
        current_snapshot_id,
        archived_at,
        created_at,
        updated_at,
    )
    .map_err(RepositoryError::from)
}

pub(crate) fn decode_uuid(value: String) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(&value).map_err(|source| RepositoryError::CorruptData(source.to_string()))
}

pub(crate) fn decode_timestamp(value: i64) -> Result<UnixTimestampMillis, RepositoryError> {
    UnixTimestampMillis::from_millis(value).map_err(RepositoryError::from)
}

pub(crate) fn positive_u64_to_i64(value: u64) -> Result<i64, RepositoryError> {
    if value == 0 {
        return Err(RepositoryError::IntegerOutOfRange);
    }
    i64::try_from(value).map_err(|_| RepositoryError::IntegerOutOfRange)
}

pub(crate) fn i64_to_positive_u64(value: i64) -> Result<u64, RepositoryError> {
    let value = u64::try_from(value).map_err(|_| RepositoryError::IntegerOutOfRange)?;
    if value == 0 {
        return Err(RepositoryError::IntegerOutOfRange);
    }
    Ok(value)
}

fn generation_to_i64(generation: u64) -> Result<i64, RepositoryError> {
    positive_u64_to_i64(generation)
}

pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database_error) => database_error.is_unique_violation(),
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("persisted JSON data is invalid")]
    Serialization(#[from] serde_json::Error),
    #[error("persisted server data is invalid")]
    Validation(#[from] ValidationError),
    #[error("persisted timestamp is invalid")]
    Timestamp(#[from] crate::domain::TimestampError),
    #[error("persisted server instance data is invalid")]
    ServerInstanceValidation(#[from] crate::domain::ServerInstanceValidationError),
    #[error("persisted compute instance data is invalid")]
    ComputeInstanceValidation(#[from] crate::domain::ComputeInstanceValidationError),
    #[error("persisted data is corrupt: {0}")]
    CorruptData(String),
    #[error("persisted integer is outside the supported positive 64-bit range")]
    IntegerOutOfRange,
    #[error("resource conflict: {0}")]
    Conflict(&'static str),
    #[error("server repository received a non-archived server for archive persistence")]
    InvalidArchiveState,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        domain::{ComputeSpec, DataBackend, DataSpec, ProcessSpec, ServerSpec},
        infrastructure::connect_database,
    };

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
                repository: "/tmp/community-restic".to_owned(),
            },
        }
    }

    #[tokio::test]
    async fn archived_server_is_hidden_but_name_remains_reserved()
    -> Result<(), Box<dyn std::error::Error>> {
        let database_path =
            std::env::temp_dir().join(format!("mcserver-repository-{}.db", Uuid::new_v4()));
        let database_url = format!("sqlite://{}", database_path.display());
        let pool = connect_database(&database_url).await?;
        let repository = ServerRepository::new(pool.clone());
        let now = UnixTimestampMillis::from_millis(1_000)?;
        let name = ServerName::new("community")?;
        let mut server = Server::new(ServerId::new(), name.clone(), valid_spec(), now)?;
        repository.create(&server).await?;

        let previous_generation = server.generation;
        server.archive(UnixTimestampMillis::from_millis(2_000)?)?;
        assert!(repository.archive(&server, previous_generation).await?);
        assert!(repository.list(false).await?.is_empty());
        assert_eq!(repository.list(true).await?, vec![server.clone()]);

        let replacement = Server::new(ServerId::new(), name, valid_spec(), now)?;
        assert!(matches!(
            repository.create(&replacement).await,
            Err(RepositoryError::Conflict(_))
        ));

        pool.close().await;
        std::fs::remove_file(database_path)?;
        Ok(())
    }
}
