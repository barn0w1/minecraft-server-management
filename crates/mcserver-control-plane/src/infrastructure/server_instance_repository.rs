use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use uuid::Uuid;

use crate::domain::{
    ServerId, ServerInstance, ServerInstanceId, ServerSpec, TerminalResult, UnixTimestampMillis,
};

use super::RepositoryError;

const INSTANCE_COLUMNS: &str = r#"
    id,
    server_id,
    server_generation,
    resolved_spec_json,
    fencing_token,
    stop_requested_at_ms,
    terminated_at_ms,
    terminal_result,
    created_at_ms,
    updated_at_ms
"#;

#[derive(Debug, Clone)]
pub struct ServerInstanceRepository {
    pool: SqlitePool,
}

impl ServerInstanceRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Creates an active instance only when the server currently desires
    /// running and does not already have an active instance.
    ///
    /// The fencing token allocation and instance insertion occur in one SQLite
    /// transaction, so concurrent reconcilers cannot commit two active
    /// instances for the same server.
    pub async fn create_for_running_server(
        &self,
        server_id: ServerId,
        now: UnixTimestampMillis,
    ) -> Result<Option<ServerInstance>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let instance_id = ServerInstanceId::new();

        let inserted = sqlx::query(
            r#"
            WITH source AS (
                SELECT
                    server.*,
                    max(
                        ?,
                        server.updated_at_ms,
                        coalesce((
                            SELECT max(instance.updated_at_ms)
                            FROM server_instances AS instance
                            WHERE instance.server_id = server.id
                        ), 0)
                    ) AS instance_time_ms
                FROM servers AS server
                WHERE server.id = ?
            )
            INSERT INTO server_instances (
                id,
                server_id,
                server_generation,
                resolved_spec_json,
                fencing_token,
                created_at_ms,
                updated_at_ms
            )
            SELECT
                ?,
                id,
                generation,
                spec_json,
                next_fencing_token,
                instance_time_ms,
                instance_time_ms
            FROM source
            WHERE desired_state = 'running'
              AND NOT EXISTS (
                  SELECT 1
                  FROM server_instances
                  WHERE server_id = source.id AND terminated_at_ms IS NULL
              )
            "#,
        )
        .bind(now.as_millis())
        .bind(server_id.to_string())
        .bind(instance_id.to_string())
        .execute(&mut *transaction)
        .await?;

        if inserted.rows_affected() == 0 {
            transaction.commit().await?;
            return Ok(None);
        }

        let advanced = sqlx::query(
            r#"
            UPDATE servers
            SET next_fencing_token = next_fencing_token + 1
            WHERE id = ?
            "#,
        )
        .bind(server_id.to_string())
        .execute(&mut *transaction)
        .await?;

        if advanced.rows_affected() != 1 {
            return Err(RepositoryError::CorruptData(
                "created a server instance without advancing its fencing token".to_owned(),
            ));
        }

        let query = format!("SELECT {INSTANCE_COLUMNS} FROM server_instances WHERE id = ?");
        let row = sqlx::query(&query)
            .bind(instance_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
        let instance = decode_server_instance(&row)?;
        transaction.commit().await?;
        Ok(Some(instance))
    }

    pub async fn get(
        &self,
        id: ServerInstanceId,
    ) -> Result<Option<ServerInstance>, RepositoryError> {
        let query = format!("SELECT {INSTANCE_COLUMNS} FROM server_instances WHERE id = ?");
        let row = sqlx::query(&query)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_server_instance).transpose()
    }

    pub async fn get_active_for_server(
        &self,
        server_id: ServerId,
    ) -> Result<Option<ServerInstance>, RepositoryError> {
        let query = format!(
            "SELECT {INSTANCE_COLUMNS} FROM server_instances \
             WHERE server_id = ? AND terminated_at_ms IS NULL"
        );
        let row = sqlx::query(&query)
            .bind(server_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_server_instance).transpose()
    }

    pub async fn list_for_server(
        &self,
        server_id: ServerId,
    ) -> Result<Vec<ServerInstance>, RepositoryError> {
        let query = format!(
            "SELECT {INSTANCE_COLUMNS} FROM server_instances \
             WHERE server_id = ? ORDER BY fencing_token DESC, id"
        );
        let rows = sqlx::query(&query)
            .bind(server_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_server_instance).collect()
    }

    pub async fn request_stop(
        &self,
        id: ServerInstanceId,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE server_instances
            SET
                stop_requested_at_ms = max(?, created_at_ms, updated_at_ms),
                updated_at_ms = max(?, created_at_ms, updated_at_ms)
            WHERE id = ?
              AND stop_requested_at_ms IS NULL
              AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn complete(
        &self,
        id: ServerInstanceId,
        result: TerminalResult,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let updated = sqlx::query(
            r#"
            UPDATE server_instances
            SET
                terminated_at_ms = max(?, stop_requested_at_ms, updated_at_ms),
                terminal_result = ?,
                updated_at_ms = max(?, stop_requested_at_ms, updated_at_ms)
            WHERE id = ?
              AND stop_requested_at_ms IS NOT NULL
              AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(result.as_str())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }
}

fn decode_server_instance(row: &SqliteRow) -> Result<ServerInstance, RepositoryError> {
    let id = decode_uuid(row.try_get("id")?).map(ServerInstanceId::from_uuid)?;
    let server_id = decode_uuid(row.try_get("server_id")?).map(ServerId::from_uuid)?;
    let server_generation = decode_positive_u64(row.try_get("server_generation")?)?;
    let resolved_spec_json = row.try_get::<String, _>("resolved_spec_json")?;
    let resolved_spec = serde_json::from_str::<ServerSpec>(&resolved_spec_json)?;
    let fencing_token = decode_positive_u64(row.try_get("fencing_token")?)?;
    let stop_requested_at = decode_optional_timestamp(row.try_get("stop_requested_at_ms")?)?;
    let terminated_at = decode_optional_timestamp(row.try_get("terminated_at_ms")?)?;
    let terminal_result = row
        .try_get::<Option<String>, _>("terminal_result")?
        .as_deref()
        .map(TerminalResult::parse)
        .transpose()?;
    let created_at = decode_timestamp(row.try_get("created_at_ms")?)?;
    let updated_at = decode_timestamp(row.try_get("updated_at_ms")?)?;

    ServerInstance::rehydrate(
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
    )
    .map_err(RepositoryError::from)
}

fn decode_uuid(value: String) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(&value).map_err(|source| RepositoryError::CorruptData(source.to_string()))
}

fn decode_positive_u64(value: i64) -> Result<u64, RepositoryError> {
    let value = u64::try_from(value).map_err(|_| RepositoryError::IntegerOutOfRange)?;
    if value == 0 {
        return Err(RepositoryError::IntegerOutOfRange);
    }
    Ok(value)
}

fn decode_timestamp(value: i64) -> Result<UnixTimestampMillis, RepositoryError> {
    UnixTimestampMillis::from_millis(value).map_err(RepositoryError::from)
}

fn decode_optional_timestamp(
    value: Option<i64>,
) -> Result<Option<UnixTimestampMillis>, RepositoryError> {
    value.map(decode_timestamp).transpose()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sqlx::SqlitePool;

    use super::*;
    use crate::{
        domain::{
            ComputeSpec, DataSpec, DesiredState, ProcessSpec, Server, ServerName, ServerSpec,
        },
        infrastructure::ServerRepository,
    };

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

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_one_active_instance_exists_and_fencing_tokens_increase(
        pool: SqlitePool,
    ) -> Result<(), RepositoryError> {
        let server_repository = ServerRepository::new(pool.clone());
        let instance_repository = ServerInstanceRepository::new(pool);
        let mut server = Server::new(ServerName::new("community")?, valid_spec())?;
        server_repository.create(&server).await?;

        let previous_generation = server.generation;
        assert!(server.set_desired_state(DesiredState::Running)?);
        assert!(
            server_repository
                .update_desired_state(&server, previous_generation)
                .await?
        );

        let first_time = UnixTimestampMillis::from_millis(1_000)?;
        let first = instance_repository
            .create_for_running_server(server.id, first_time)
            .await?
            .ok_or_else(|| {
                RepositoryError::CorruptData("first instance was not created".to_owned())
            })?;
        assert_eq!(first.fencing_token, 1);
        assert!(
            instance_repository
                .create_for_running_server(server.id, first_time)
                .await?
                .is_none()
        );

        let stop_time = UnixTimestampMillis::from_millis(2_000)?;
        assert!(
            instance_repository
                .request_stop(first.id, stop_time)
                .await?
        );
        assert!(
            instance_repository
                .complete(first.id, TerminalResult::Completed, stop_time)
                .await?
        );

        let second = instance_repository
            .create_for_running_server(server.id, stop_time)
            .await?
            .ok_or_else(|| {
                RepositoryError::CorruptData("second instance was not created".to_owned())
            })?;
        assert_eq!(second.fencing_token, 2);
        assert_ne!(first.id, second.id);
        Ok(())
    }
}
