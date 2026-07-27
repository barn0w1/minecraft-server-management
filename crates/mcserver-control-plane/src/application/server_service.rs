use thiserror::Error;

use crate::{
    domain::{
        Clock, DesiredServerSpec, DesiredState, Server, ServerId, ServerName, SystemClock,
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
    r2_repository_base: Option<String>,
}

impl ServerService {
    #[must_use]
    pub fn new(
        repository: ServerRepository,
        reconcile_scheduler: ReconcileScheduler,
        r2_repository_base: Option<String>,
    ) -> Self {
        Self {
            repository,
            reconcile_scheduler,
            clock: SystemClock,
            r2_repository_base,
        }
    }

    pub async fn create(
        &self,
        name: String,
        desired_spec: DesiredServerSpec,
    ) -> Result<Server, ApplicationError> {
        let now = self.clock.now()?;
        let name = ServerName::new(name)?;
        let id = ServerId::new();
        let spec = desired_spec.resolve(None, self.r2_repository(&name))?;
        let server = Server::new(id, name, spec, now)?;
        self.repository.create(&server).await?;
        self.reconcile_scheduler.enqueue_best_effort(server.id);
        Ok(server)
    }

    pub async fn apply(
        &self,
        name: String,
        desired_spec: DesiredServerSpec,
        expected_generation: Option<u64>,
    ) -> Result<Server, ApplicationError> {
        let name = ServerName::new(name)?;
        for _ in 0..3 {
            let Some(mut server) = self.repository.get_by_name(&name).await? else {
                if expected_generation.is_some() {
                    return Err(ApplicationError::NotFound);
                }
                return self
                    .create(name.as_str().to_owned(), desired_spec.clone())
                    .await;
            };
            if expected_generation.is_some_and(|expected| expected != server.generation) {
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual: server.generation,
                });
            }
            let spec = desired_spec
                .clone()
                .resolve(Some(&server.spec.data), None)?;
            let previous_generation = server.generation;
            let now = self.clock.now()?;
            if !server.update_spec(spec, now)? {
                return Ok(server);
            }
            if self
                .repository
                .update_spec(&server, previous_generation)
                .await?
            {
                self.reconcile_scheduler.enqueue_best_effort(server.id);
                return Ok(server);
            }
            if expected_generation.is_some() {
                let actual = self.get(server.id).await?.generation;
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual,
                });
            }
        }
        Err(ApplicationError::ConcurrentUpdate)
    }

    pub async fn get(&self, id: ServerId) -> Result<Server, ApplicationError> {
        self.repository
            .get(id)
            .await?
            .ok_or(ApplicationError::NotFound)
    }

    pub async fn get_by_name(&self, name: String) -> Result<Server, ApplicationError> {
        let name = ServerName::new(name)?;
        self.repository
            .get_by_name(&name)
            .await?
            .ok_or(ApplicationError::NotFound)
    }

    pub async fn list(&self, include_archived: bool) -> Result<Vec<Server>, ApplicationError> {
        self.repository
            .list(include_archived)
            .await
            .map_err(Into::into)
    }

    pub async fn set_desired_state(
        &self,
        name: String,
        desired_state: DesiredState,
        expected_generation: Option<u64>,
    ) -> Result<Server, ApplicationError> {
        let name = ServerName::new(name)?;
        for _ in 0..3 {
            let mut server = self
                .repository
                .get_by_name(&name)
                .await?
                .ok_or(ApplicationError::NotFound)?;

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
                let actual = self.get_by_name(name.as_str().to_owned()).await?.generation;
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual,
                });
            }
        }

        Err(ApplicationError::ConcurrentUpdate)
    }

    pub async fn archive(
        &self,
        name: String,
        expected_generation: Option<u64>,
    ) -> Result<Server, ApplicationError> {
        let name = ServerName::new(name)?;
        for _ in 0..3 {
            let mut server = self
                .repository
                .get_by_name(&name)
                .await?
                .ok_or(ApplicationError::NotFound)?;
            if expected_generation.is_some_and(|expected| expected != server.generation) {
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual: server.generation,
                });
            }
            if server.archived_at.is_some() {
                return Ok(server);
            }
            if self.repository.has_active_instance(server.id).await? {
                return Err(ApplicationError::ServerHasActiveInstance);
            }
            let previous_generation = server.generation;
            server.archive(self.clock.now()?)?;
            if self
                .repository
                .archive(&server, previous_generation)
                .await?
            {
                return Ok(server);
            }
            if expected_generation.is_some() {
                let actual = self.get_by_name(name.as_str().to_owned()).await?.generation;
                return Err(ApplicationError::GenerationConflict {
                    expected: expected_generation,
                    actual,
                });
            }
        }
        Err(ApplicationError::ConcurrentUpdate)
    }

    fn r2_repository(&self, server_name: &ServerName) -> Option<String> {
        self.r2_repository_base
            .as_ref()
            .map(|base| format!("{base}/servers/{}/restic", server_name.as_str()))
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
    #[error("server still has an active instance")]
    ServerHasActiveInstance,
    #[error("generation conflict: expected {expected:?}, actual {actual}")]
    GenerationConflict { expected: Option<u64>, actual: u64 },
    #[error("server was updated concurrently")]
    ConcurrentUpdate,
    #[error("persistence failed")]
    Repository(#[from] RepositoryError),
    #[error("timestamp generation failed")]
    Timestamp(#[from] crate::domain::TimestampError),
}
