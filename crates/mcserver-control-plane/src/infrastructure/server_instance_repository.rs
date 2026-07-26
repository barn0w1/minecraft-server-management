use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use crate::domain::{
    ServerId, ServerInstance, ServerInstanceId, ServerSpec, TerminalResult, UnixTimestampMillis,
};

use super::{
    RepositoryError,
    server_repository::{
        decode_timestamp, decode_uuid, i64_to_positive_u64,
    },
};

const MAX_ERROR_CHARS: usize = 8192;

const SELECT_BY_ID: &str = r#"
            SELECT
                id,
                server_id,
                server_generation,
                resolved_spec_json,
                fencing_token,
                source_snapshot_id,
                data_prepared_at_ms,
                process_running,
                process_observed_at_ms,
                result_snapshot_id,
                stop_requested_at_ms,
                terminated_at_ms,
                terminal_result,
                last_error,
                created_at_ms,
                updated_at_ms
            FROM server_instances
            WHERE id = ?
            "#;

const SELECT_ACTIVE_FOR_SERVER: &str = r#"
            SELECT
                id,
                server_id,
                server_generation,
                resolved_spec_json,
                fencing_token,
                source_snapshot_id,
                data_prepared_at_ms,
                process_running,
                process_observed_at_ms,
                result_snapshot_id,
                stop_requested_at_ms,
                terminated_at_ms,
                terminal_result,
                last_error,
                created_at_ms,
                updated_at_ms
            FROM server_instances
            WHERE server_id = ? AND terminated_at_ms IS NULL
            "#;

const SELECT_FOR_SERVER: &str = r#"
            SELECT
                id,
                server_id,
                server_generation,
                resolved_spec_json,
                fencing_token,
                source_snapshot_id,
                data_prepared_at_ms,
                process_running,
                process_observed_at_ms,
                result_snapshot_id,
                stop_requested_at_ms,
                terminated_at_ms,
                terminal_result,
                last_error,
                created_at_ms,
                updated_at_ms
            FROM server_instances
            WHERE server_id = ?
            ORDER BY fencing_token DESC, id
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
                source_snapshot_id,
                created_at_ms,
                updated_at_ms
            )
            SELECT
                ?,
                id,
                generation,
                spec_json,
                next_fencing_token,
                current_snapshot_id,
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
            transaction.rollback().await?;
            return Ok(None);
        }

        sqlx::query(
            r#"
            UPDATE servers
            SET next_fencing_token = next_fencing_token + 1
            WHERE id = ?
            "#,
        )
        .bind(server_id.to_string())
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        self.get(instance_id).await
    }

    pub async fn get(
        &self,
        id: ServerInstanceId,
    ) -> Result<Option<ServerInstance>, RepositoryError> {
        let row = sqlx::query(SELECT_BY_ID)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_instance).transpose()
    }

    pub async fn get_active_for_server(
        &self,
        server_id: ServerId,
    ) -> Result<Option<ServerInstance>, RepositoryError> {
        let row = sqlx::query(SELECT_ACTIVE_FOR_SERVER)
            .bind(server_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_instance).transpose()
    }

    pub async fn list_for_server(
        &self,
        server_id: ServerId,
    ) -> Result<Vec<ServerInstance>, RepositoryError> {
        let rows = sqlx::query(SELECT_FOR_SERVER)
            .bind(server_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_instance).collect()
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
              AND terminated_at_ms IS NULL
              AND stop_requested_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_data_prepared(
        &self,
        id: ServerInstanceId,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE server_instances
            SET
                data_prepared_at_ms = coalesce(data_prepared_at_ms, max(?, created_at_ms)),
                last_error = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn observe_process(
        &self,
        id: ServerInstanceId,
        running: bool,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE server_instances
            SET
                process_running = ?,
                process_observed_at_ms = max(
                    coalesce(process_observed_at_ms, 0),
                    ?,
                    updated_at_ms,
                    created_at_ms
                ),
                last_error = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL
            "#,
        )
        .bind(running)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn record_error(
        &self,
        id: ServerInstanceId,
        message: &str,
        now: UnixTimestampMillis,
    ) -> Result<(), RepositoryError> {
        let message = truncate_chars(message, MAX_ERROR_CHARS);
        sqlx::query(
            r#"
            UPDATE server_instances
            SET last_error = ?, updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL
            "#,
        )
        .bind(message)
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete(
        &self,
        id: ServerInstanceId,
        result: TerminalResult,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE server_instances
            SET
                process_running = 0,
                process_observed_at_ms = max(
                    coalesce(process_observed_at_ms, 0),
                    ?,
                    updated_at_ms,
                    created_at_ms
                ),
                terminated_at_ms = max(
                    ?,
                    updated_at_ms,
                    created_at_ms,
                    coalesce(stop_requested_at_ms, 0)
                ),
                terminal_result = ?,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(result.as_str())
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn decode_instance(row: &SqliteRow) -> Result<ServerInstance, RepositoryError> {
    let id = decode_uuid(row.try_get::<String, _>("id")?)
        .map(ServerInstanceId::from_uuid)?;
    let server_id = decode_uuid(row.try_get::<String, _>("server_id")?)
        .map(ServerId::from_uuid)?;
    let server_generation = i64_to_positive_u64(row.try_get("server_generation")?)?;
    let resolved_spec =
        serde_json::from_str::<ServerSpec>(&row.try_get::<String, _>("resolved_spec_json")?)?;
    let fencing_token = i64_to_positive_u64(row.try_get("fencing_token")?)?;
    let source_snapshot_id = row.try_get("source_snapshot_id")?;
    let data_prepared_at = optional_timestamp(row.try_get("data_prepared_at_ms")?)?;
    let process_running = row.try_get("process_running")?;
    let process_observed_at = optional_timestamp(row.try_get("process_observed_at_ms")?)?;
    let result_snapshot_id = row.try_get("result_snapshot_id")?;
    let stop_requested_at = optional_timestamp(row.try_get("stop_requested_at_ms")?)?;
    let terminated_at = optional_timestamp(row.try_get("terminated_at_ms")?)?;
    let terminal_result = row
        .try_get::<Option<String>, _>("terminal_result")?
        .as_deref()
        .map(TerminalResult::parse)
        .transpose()?;
    let last_error = row.try_get("last_error")?;
    let created_at = decode_timestamp(row.try_get("created_at_ms")?)?;
    let updated_at = decode_timestamp(row.try_get("updated_at_ms")?)?;

    ServerInstance::rehydrate(
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
    )
    .map_err(RepositoryError::from)
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        return value.to_owned();
    }
    value.chars().take(maximum).collect()
}

fn optional_timestamp(
    value: Option<i64>,
) -> Result<Option<UnixTimestampMillis>, RepositoryError> {
    value.map(decode_timestamp).transpose()
}
