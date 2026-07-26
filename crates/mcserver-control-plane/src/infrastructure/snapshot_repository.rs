use sqlx::{Row, SqlitePool};

use crate::domain::{ServerInstanceId, UnixTimestampMillis};

use super::{RepositoryError, server_repository::positive_u64_to_i64};

const MAX_SNAPSHOT_ID_CHARS: usize = 256;

#[derive(Debug, Clone)]
pub struct SnapshotRepository {
    pool: SqlitePool,
}

impl SnapshotRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn commit(
        &self,
        server_instance_id: ServerInstanceId,
        fencing_token: u64,
        snapshot_id: &str,
        now: UnixTimestampMillis,
    ) -> Result<bool, RepositoryError> {
        if snapshot_id.trim().is_empty()
            || snapshot_id.contains('\0')
            || snapshot_id.chars().count() > MAX_SNAPSHOT_ID_CHARS
        {
            return Err(RepositoryError::CorruptData(
                "snapshot id must be non-blank, bounded, and contain no NUL byte".to_owned(),
            ));
        }

        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT
                instance.server_id,
                instance.fencing_token,
                instance.terminated_at_ms,
                instance.result_snapshot_id,
                instance.updated_at_ms AS instance_updated_at_ms,
                server.updated_at_ms AS server_updated_at_ms
            FROM server_instances AS instance
            JOIN servers AS server ON server.id = instance.server_id
            WHERE instance.id = ?
            "#,
        )
        .bind(server_instance_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(RepositoryError::Conflict("server instance does not exist"))?;

        let server_id: String = row.try_get("server_id")?;
        let actual_token: i64 = row.try_get("fencing_token")?;
        let terminated_at: Option<i64> = row.try_get("terminated_at_ms")?;
        let existing_snapshot: Option<String> = row.try_get("result_snapshot_id")?;
        let instance_updated_at_ms: i64 = row.try_get("instance_updated_at_ms")?;
        let server_updated_at_ms: i64 = row.try_get("server_updated_at_ms")?;

        if actual_token != positive_u64_to_i64(fencing_token)? || terminated_at.is_some() {
            transaction.rollback().await?;
            return Err(RepositoryError::Conflict(
                "stale server instance cannot publish a snapshot",
            ));
        }
        if let Some(existing_snapshot) = existing_snapshot {
            transaction.rollback().await?;
            if existing_snapshot == snapshot_id {
                return Ok(false);
            }
            return Err(RepositoryError::Conflict(
                "server instance already published another snapshot",
            ));
        }

        let committed_at_ms = now
            .as_millis()
            .max(instance_updated_at_ms)
            .max(server_updated_at_ms);

        sqlx::query(
            r#"
            INSERT INTO snapshots (
                id,
                server_id,
                server_instance_id,
                fencing_token,
                created_at_ms
            ) VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(snapshot_id)
        .bind(&server_id)
        .bind(server_instance_id.to_string())
        .bind(positive_u64_to_i64(fencing_token)?)
        .bind(committed_at_ms)
        .execute(&mut *transaction)
        .await?;

        let instance_update = sqlx::query(
            r#"
            UPDATE server_instances
            SET result_snapshot_id = ?, updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ? AND terminated_at_ms IS NULL AND fencing_token = ?
            "#,
        )
        .bind(snapshot_id)
        .bind(committed_at_ms)
        .bind(server_instance_id.to_string())
        .bind(positive_u64_to_i64(fencing_token)?)
        .execute(&mut *transaction)
        .await?;
        if instance_update.rows_affected() != 1 {
            transaction.rollback().await?;
            return Err(RepositoryError::Conflict(
                "server instance stopped before its snapshot could be published",
            ));
        }

        let server_update = sqlx::query(
            r#"
            UPDATE servers
            SET current_snapshot_id = ?, updated_at_ms = max(?, updated_at_ms, created_at_ms)
            WHERE id = ?
              AND EXISTS (
                  SELECT 1
                  FROM server_instances
                  WHERE id = ?
                    AND server_id = servers.id
                    AND fencing_token = ?
                    AND terminated_at_ms IS NULL
              )
            "#,
        )
        .bind(snapshot_id)
        .bind(committed_at_ms)
        .bind(&server_id)
        .bind(server_instance_id.to_string())
        .bind(positive_u64_to_i64(fencing_token)?)
        .execute(&mut *transaction)
        .await?;
        if server_update.rows_affected() != 1 {
            transaction.rollback().await?;
            return Err(RepositoryError::Conflict(
                "stale server instance cannot publish a snapshot",
            ));
        }

        transaction.commit().await?;
        Ok(true)
    }
}
