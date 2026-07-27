use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use crate::domain::{
    ComputeInstance, ComputeInstanceId, ComputeProvider, ComputeTerminalResult, ServerInstanceId,
    UnixTimestampMillis,
};

use super::{
    RepositoryError,
    server_repository::{decode_timestamp, decode_uuid},
};

const MAX_FAILURE_MESSAGE_CHARS: usize = 8192;

const SELECT_BY_ID: &str = r#"
    SELECT
        id,
        server_instance_id,
        provider,
        provider_instance_id,
        public_ipv4,
        connection_token,
        enrollment_token,
        process_id,
        agent_connected_at_ms,
        shutdown_requested_at_ms,
        terminated_at_ms,
        terminal_result,
        failure_message,
        created_at_ms,
        updated_at_ms
    FROM compute_instances
    WHERE id = ?
"#;

const SELECT_ACTIVE_FOR_INSTANCE: &str = r#"
    SELECT
        id,
        server_instance_id,
        provider,
        provider_instance_id,
        public_ipv4,
        connection_token,
        enrollment_token,
        process_id,
        agent_connected_at_ms,
        shutdown_requested_at_ms,
        terminated_at_ms,
        terminal_result,
        failure_message,
        created_at_ms,
        updated_at_ms
    FROM compute_instances
    WHERE server_instance_id = ? AND terminated_at_ms IS NULL
"#;

const SELECT_ACTIVE_LOCAL_OWNERSHIP: &str = r#"
    SELECT id, server_instance_id
    FROM compute_instances
    WHERE terminated_at_ms IS NULL AND provider = 'local_process'
    ORDER BY id
"#;

const SELECT_ACTIVE_AKAMAI: &str = r#"
    SELECT
        id,
        server_instance_id,
        provider,
        provider_instance_id,
        public_ipv4,
        connection_token,
        enrollment_token,
        process_id,
        agent_connected_at_ms,
        shutdown_requested_at_ms,
        terminated_at_ms,
        terminal_result,
        failure_message,
        created_at_ms,
        updated_at_ms
    FROM compute_instances
    WHERE terminated_at_ms IS NULL AND provider = 'akamai'
    ORDER BY id
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAuthentication {
    Accepted,
    ReplaceToken(String),
    Rejected,
}

#[derive(Debug, Clone)]
pub struct ComputeInstanceRepository {
    pool: SqlitePool,
}

impl ComputeInstanceRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_for_instance(
        &self,
        server_instance_id: ServerInstanceId,
        provider: ComputeProvider,
        connection_token: &str,
        enrollment_token: Option<&str>,
        now: UnixTimestampMillis,
    ) -> Result<Option<ComputeInstance>, RepositoryError> {
        let id = ComputeInstanceId::new();
        let result = sqlx::query(
            r#"
            INSERT INTO compute_instances (
                id,
                server_instance_id,
                provider,
                connection_token,
                enrollment_token,
                created_at_ms,
                updated_at_ms
            )
            SELECT ?, ?, ?, ?, ?, ?, ?
            WHERE EXISTS (
                SELECT 1
                FROM server_instances
                WHERE id = ? AND terminated_at_ms IS NULL
            )
            AND NOT EXISTS (
                SELECT 1
                FROM compute_instances
                WHERE server_instance_id = ? AND terminated_at_ms IS NULL
            )
            "#,
        )
        .bind(id.to_string())
        .bind(server_instance_id.to_string())
        .bind(provider.as_str())
        .bind(connection_token)
        .bind(enrollment_token)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(server_instance_id.to_string())
        .bind(server_instance_id.to_string())
        .execute(&self.pool)
        .await;

        match result {
            Ok(result) if result.rows_affected() == 1 => self.get(id).await,
            Ok(_) => Ok(None),
            Err(error) if super::server_repository::is_unique_violation(&error) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn get(
        &self,
        id: ComputeInstanceId,
    ) -> Result<Option<ComputeInstance>, RepositoryError> {
        let row = sqlx::query(SELECT_BY_ID)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_compute_instance).transpose()
    }

    pub async fn get_active_for_instance(
        &self,
        server_instance_id: ServerInstanceId,
    ) -> Result<Option<ComputeInstance>, RepositoryError> {
        let row = sqlx::query(SELECT_ACTIVE_FOR_INSTANCE)
            .bind(server_instance_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(decode_compute_instance).transpose()
    }

    pub async fn list_active_local_ownership(
        &self,
    ) -> Result<Vec<(ComputeInstanceId, ServerInstanceId)>, RepositoryError> {
        let rows = sqlx::query(SELECT_ACTIVE_LOCAL_OWNERSHIP)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let compute_id = decode_uuid(row.try_get::<String, _>("id")?)?;
                let server_instance_id =
                    decode_uuid(row.try_get::<String, _>("server_instance_id")?)?;
                Ok((
                    ComputeInstanceId::from_uuid(compute_id),
                    ServerInstanceId::from_uuid(server_instance_id),
                ))
            })
            .collect()
    }

    pub async fn list_active_akamai(&self) -> Result<Vec<ComputeInstance>, RepositoryError> {
        let rows = sqlx::query(SELECT_ACTIVE_AKAMAI)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(decode_compute_instance).collect()
    }

    pub async fn record_process_id(
        &self,
        id: ComputeInstanceId,
        process_id: u32,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                process_id = ?,
                failure_message = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL AND provider = 'local_process'
            "#,
        )
        .bind(i64::from(process_id))
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn record_provider_instance(
        &self,
        id: ComputeInstanceId,
        provider_instance_id: &str,
        public_ipv4: Option<&str>,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                provider_instance_id = ?,
                public_ipv4 = ?,
                failure_message = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL AND provider = 'akamai'
            "#,
        )
        .bind(provider_instance_id)
        .bind(public_ipv4)
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn authenticate_agent(
        &self,
        id: ComputeInstanceId,
        expected_provider: ComputeProvider,
        presented_token: &str,
        now: UnixTimestampMillis,
    ) -> Result<AgentAuthentication, RepositoryError> {
        let accepted = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                enrollment_token = CASE
                    WHEN provider = 'akamai' THEN NULL
                    ELSE enrollment_token
                END,
                agent_connected_at_ms = max(
                    coalesce(agent_connected_at_ms, 0),
                    ?,
                    updated_at_ms,
                    created_at_ms
                ),
                failure_message = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE
                id = ?
                AND provider = ?
                AND connection_token = ?
                AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .bind(expected_provider.as_str())
        .bind(presented_token)
        .execute(&self.pool)
        .await?;
        if accepted.rows_affected() == 1 {
            return Ok(AgentAuthentication::Accepted);
        }

        if expected_provider != ComputeProvider::Akamai {
            return Ok(AgentAuthentication::Rejected);
        }

        let enrollment = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                agent_connected_at_ms = max(
                    coalesce(agent_connected_at_ms, 0),
                    ?,
                    updated_at_ms,
                    created_at_ms
                ),
                failure_message = NULL,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE
                id = ?
                AND provider = 'akamai'
                AND enrollment_token = ?
                AND terminated_at_ms IS NULL
            RETURNING connection_token
            "#,
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(id.to_string())
        .bind(presented_token)
        .fetch_optional(&self.pool)
        .await?;
        match enrollment {
            Some(row) => Ok(AgentAuthentication::ReplaceToken(
                row.try_get("connection_token")?,
            )),
            None => Ok(AgentAuthentication::Rejected),
        }
    }

    pub async fn request_shutdown(
        &self,
        id: ComputeInstanceId,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                shutdown_requested_at_ms = coalesce(
                    shutdown_requested_at_ms,
                    max(?, created_at_ms)
                ),
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

    pub async fn terminate(
        &self,
        id: ComputeInstanceId,
        result: ComputeTerminalResult,
        failure_message: Option<&str>,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        let updated = sqlx::query(
            r#"
            UPDATE compute_instances
            SET
                terminated_at_ms = max(?, updated_at_ms, created_at_ms),
                terminal_result = ?,
                failure_message = ?,
                updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL
            "#,
        )
        .bind(now.as_millis())
        .bind(result.as_str())
        .bind(failure_message)
        .bind(now.as_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() == 1)
    }

    pub async fn record_failure(
        &self,
        id: ComputeInstanceId,
        message: &str,
        now: UnixTimestampMillis,
    ) -> Result<(), RepositoryError> {
        let message = truncate_chars(message, MAX_FAILURE_MESSAGE_CHARS);
        sqlx::query(
            r#"
            UPDATE compute_instances
            SET failure_message = ?, updated_at_ms = max(?, updated_at_ms, created_at_ms)
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
}

fn decode_compute_instance(row: &SqliteRow) -> Result<ComputeInstance, RepositoryError> {
    let id = decode_uuid(row.try_get::<String, _>("id")?).map(ComputeInstanceId::from_uuid)?;
    let server_instance_id = decode_uuid(row.try_get::<String, _>("server_instance_id")?)
        .map(ServerInstanceId::from_uuid)?;
    let provider = ComputeProvider::parse(&row.try_get::<String, _>("provider")?)?;
    let provider_instance_id = row.try_get("provider_instance_id")?;
    let public_ipv4 = row.try_get("public_ipv4")?;
    let connection_token = row.try_get("connection_token")?;
    let enrollment_token = row.try_get("enrollment_token")?;
    let process_id = row
        .try_get::<Option<i64>, _>("process_id")?
        .map(|value| u32::try_from(value).map_err(|_| RepositoryError::IntegerOutOfRange))
        .transpose()?;
    let agent_connected_at = optional_timestamp(row.try_get("agent_connected_at_ms")?)?;
    let shutdown_requested_at = optional_timestamp(row.try_get("shutdown_requested_at_ms")?)?;
    let terminated_at = optional_timestamp(row.try_get("terminated_at_ms")?)?;
    let terminal_result = row
        .try_get::<Option<String>, _>("terminal_result")?
        .as_deref()
        .map(ComputeTerminalResult::parse)
        .transpose()?;
    let failure_message = row.try_get("failure_message")?;
    let created_at = decode_timestamp(row.try_get("created_at_ms")?)?;
    let updated_at = decode_timestamp(row.try_get("updated_at_ms")?)?;

    ComputeInstance::rehydrate(
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
    )
    .map_err(RepositoryError::from)
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

fn optional_timestamp(value: Option<i64>) -> Result<Option<UnixTimestampMillis>, RepositoryError> {
    value.map(decode_timestamp).transpose()
}
