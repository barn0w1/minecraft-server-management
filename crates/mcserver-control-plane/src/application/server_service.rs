use thiserror::Error;

use crate::{
    domain::{
        Clock, DesiredState, Server, ServerId, ServerName, ServerSpec, SystemClock,
        ValidationError,
    },
    infrastructure::{RepositoryError, ServerRepository},
    reconciliation::ReconcileScheduler,
};

#[derive(Debug, Clone)]
pub struct ServerService {
    repository: ServerRepository,
    reconcile_scheduler: ReconcileScheduler,
    clock: SystemClock,
}

impl ServerService {
    #[must_use]
    pub fn new(repository: ServerRepository, reconcile_scheduler: ReconcileScheduler) -> Self {
        Self {
            repository,
            reconcile_scheduler,
            clock: SystemClock,
        }
    }

    pub async fn create(&self, name: String, spec: ServerSpec) -> Result<Server, ApplicationError> {
        let now = self.clock.now()?;
        let server = Server::new(ServerId::new(), ServerName::new(name)?, spec, now)?;
        self.repository.create(&server).await?;
        self.reconcile_scheduler.enqueue_best_effort(server.id);
        Ok(server)
    }

    pub async fn get(&self, id: ServerId) -> Result<Server, ApplicationError> {
        self.repository
            .get(id)
            .await?
            .ok_or(ApplicationError::NotFound)
    }

    pub async fn list(&self) -> Result<Vec<Server>, ApplicationError> {
        self.repository.list().await.map_err(Into::into)
    }

    pub async fn set_desired_state(
        &self,
        id: ServerId,
        desired_state: DesiredState,
        expected_generation: Option<u64>,
    ) -> Result<Server, ApplicationError> {
        for _ in 0..3 {
            let mut server = self.get(id).await?;

            if expected_generation.is_some_and(|expected| expected != server.generation) {
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual: server.generation,
                });
            }

            let previous_generation = server.generation;
            let now = self.clock.now()?;
            if !server.set_desired_state(desired_state, now)? {
                return Ok(server);
            }

            if self
                .repository
                .update_desired_state(&server, previous_generation)
                .await?
            {
                self.reconcile_scheduler.enqueue_best_effort(server.id);
                return Ok(server);
            }

            if expected_generation.is_some() {
                let actual = self.get(id).await?.generation;
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual,
                });
            }
        }

        Err(ApplicationError::ConcurrentUpdate)
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("invalid request: {0}")]
    Validation(#[from] ValidationError),
    #[error("server not found")]
    NotFound,
    #[error("server instance not found")]
    ServerInstanceNotFound,
    #[error("generation conflict: expected {expected:?}, actual {actual}")]
    GenerationConflict { expected: Option<u64>, actual: u64 },
    #[error("server was updated concurrently")]
    ConcurrentUpdate,
    #[error("persistence failed")]
    Repository(#[from] RepositoryError),
    #[error("timestamp generation failed")]
    Timestamp(#[from] crate::domain::TimestampError),
}
