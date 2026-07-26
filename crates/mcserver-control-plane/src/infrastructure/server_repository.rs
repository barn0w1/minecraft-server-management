use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{DesiredState, Server, ServerId, ServerName, ServerSpec, ValidationError};

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
                created_at_ms,
                updated_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(server.id.to_string())
        .bind(server.name.as_str())
        .bind(generation_to_i64(server.generation)?)
        .bind(server.desired_state.as_str())
        .bind(spec_json)
        .bind(server.created_at_ms)
        .bind(server.updated_at_ms)
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
            SELECT id, name, generation, desired_state, spec_json, created_at_ms, updated_at_ms
            FROM servers
            WHERE id = ?
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(decode_server).transpose()
    }

    pub async fn list(&self) -> Result<Vec<Server>, RepositoryError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, generation, desired_state, spec_json, created_at_ms, updated_at_ms
            FROM servers
            ORDER BY name, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_server).collect()
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
            WHERE id = ? AND generation = ?
            "#,
        )
        .bind(server.desired_state.as_str())
        .bind(generation_to_i64(server.generation)?)
        .bind(server.updated_at_ms)
        .bind(server.id.to_string())
        .bind(generation_to_i64(previous_generation)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}

fn decode_server(row: &SqliteRow) -> Result<Server, RepositoryError> {
    let id = row.try_get::<String, _>("id")?;
    let id = Uuid::parse_str(&id)
        .map(ServerId::from_uuid)
        .map_err(|source| RepositoryError::CorruptData(source.to_string()))?;
    let name = ServerName::new(row.try_get::<String, _>("name")?)?;
    let generation = i64_to_generation(row.try_get("generation")?)?;
    let desired_state = row.try_get::<String, _>("desired_state")?;
    let desired_state = DesiredState::parse(&desired_state)?;
    let spec_json = row.try_get::<String, _>("spec_json")?;
    let spec = serde_json::from_str::<ServerSpec>(&spec_json)?;
    let created_at_ms = row.try_get("created_at_ms")?;
    let updated_at_ms = row.try_get("updated_at_ms")?;

    Server::rehydrate(
        id,
        name,
        generation,
        desired_state,
        spec,
        created_at_ms,
        updated_at_ms,
    )
    .map_err(RepositoryError::from)
}

fn generation_to_i64(generation: u64) -> Result<i64, RepositoryError> {
    i64::try_from(generation).map_err(|_| RepositoryError::GenerationOutOfRange)
}

fn i64_to_generation(generation: i64) -> Result<u64, RepositoryError> {
    u64::try_from(generation).map_err(|_| RepositoryError::GenerationOutOfRange)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
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
    #[error("server data serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("persisted server data is invalid")]
    Validation(#[from] ValidationError),
    #[error("persisted server data is corrupt: {0}")]
    CorruptData(String),
    #[error("server generation is outside the supported range")]
    GenerationOutOfRange,
    #[error("resource conflict: {0}")]
    Conflict(&'static str),
}
