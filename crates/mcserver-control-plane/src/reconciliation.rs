use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info};

use crate::{
    domain::{DesiredState, ServerId, TerminalResult, UnixTimestampMillis},
    infrastructure::{RepositoryError, ServerInstanceRepository, ServerRepository},
};

const RECONCILE_QUEUE_CAPACITY: usize = 256;
const MAX_IMMEDIATE_TRANSITIONS: usize = 8;

#[derive(Debug, Clone)]
pub struct ReconcileScheduler {
    sender: mpsc::Sender<ServerId>,
}

impl ReconcileScheduler {
    pub async fn enqueue(&self, server_id: ServerId) -> Result<(), ScheduleError> {
        self.sender
            .send(server_id)
            .await
            .map_err(|_| ScheduleError::Unavailable)
    }
}

pub struct ReconcileWorker {
    server_repository: ServerRepository,
    server_instance_repository: ServerInstanceRepository,
    receiver: mpsc::Receiver<ServerId>,
    resync_interval: Duration,
}

impl ReconcileWorker {
    pub fn channel(
        server_repository: ServerRepository,
        server_instance_repository: ServerInstanceRepository,
        resync_interval: Duration,
    ) -> (ReconcileScheduler, Self) {
        let (sender, receiver) = mpsc::channel(RECONCILE_QUEUE_CAPACITY);
        (
            ReconcileScheduler { sender },
            Self {
                server_repository,
                server_instance_repository,
                receiver,
                resync_interval,
            },
        )
    }

    pub async fn run(
        mut self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ReconcileError> {
        let mut interval = tokio::time::interval(self.resync_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                },
                maybe_server_id = self.receiver.recv() => {
                    match maybe_server_id {
                        Some(server_id) => self.reconcile_until_stable(server_id).await?,
                        None => break,
                    }
                }
                _ = interval.tick() => {
                    self.resync().await?;
                }
            }
        }

        info!("reconciliation worker stopped");
        Ok(())
    }

    async fn resync(&self) -> Result<(), ReconcileError> {
        let servers = self.server_repository.list().await?;
        debug!(
            server_count = servers.len(),
            "starting periodic server resync"
        );

        for server in servers {
            self.reconcile_until_stable(server.id).await?;
        }

        Ok(())
    }

    async fn reconcile_until_stable(&self, server_id: ServerId) -> Result<(), ReconcileError> {
        for _ in 0..MAX_IMMEDIATE_TRANSITIONS {
            if !self.reconcile_once(server_id).await? {
                return Ok(());
            }
        }

        Err(ReconcileError::DidNotConverge(server_id))
    }

    /// Applies at most one durable transition.
    ///
    /// ServerInstance termination is completed immediately for now because no
    /// compute or node-agent work exists yet. Later milestones will replace
    /// that transition with observed facts produced by those reconcilers.
    async fn reconcile_once(&self, server_id: ServerId) -> Result<bool, ReconcileError> {
        let Some(server) = self.server_repository.get(server_id).await? else {
            return Ok(false);
        };
        let active_instance = self
            .server_instance_repository
            .get_active_for_server(server_id)
            .await?;

        if let Some(instance) = active_instance {
            if instance.stop_requested_at.is_some() {
                let now = UnixTimestampMillis::now()?;
                let completed = self
                    .server_instance_repository
                    .complete(instance.id, TerminalResult::Completed, now)
                    .await?;
                if completed {
                    info!(
                        server_id = %server.id,
                        server_instance_id = %instance.id,
                        "server instance terminated"
                    );
                }
                return Ok(completed);
            }

            if server.desired_state == DesiredState::Stopped {
                let now = UnixTimestampMillis::now()?;
                let requested = self
                    .server_instance_repository
                    .request_stop(instance.id, now)
                    .await?;
                if requested {
                    info!(
                        server_id = %server.id,
                        server_instance_id = %instance.id,
                        "server instance stop requested"
                    );
                }
                return Ok(requested);
            }

            return Ok(false);
        }

        if server.desired_state == DesiredState::Running {
            let now = UnixTimestampMillis::now()?;
            let created = self
                .server_instance_repository
                .create_for_running_server(server.id, now)
                .await?;
            if let Some(instance) = created {
                info!(
                    server_id = %server.id,
                    server_instance_id = %instance.id,
                    server_generation = instance.server_generation,
                    fencing_token = instance.fencing_token,
                    "server instance created"
                );
                return Ok(true);
            }
        }

        Ok(false)
    }
}

#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error("reconciliation scheduler is unavailable")]
    Unavailable,
}

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("reconciliation persistence operation failed")]
    Repository(#[from] RepositoryError),
    #[error("reconciliation timestamp operation failed")]
    Timestamp(#[from] crate::domain::TimestampError),
    #[error("server reconciliation did not converge for {0}")]
    DidNotConverge(ServerId),
}

pub fn log_worker_failure(error: &ReconcileError) {
    error!(%error, "reconciliation worker stopped unexpectedly");
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use sqlx::SqlitePool;

    use super::*;
    use crate::{
        domain::{ComputeSpec, DataSpec, ProcessSpec, Server, ServerName, ServerSpec},
        infrastructure::{RepositoryError, ServerInstanceRepository, ServerRepository},
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
    async fn reconciles_server_instance_to_running_and_stopped(
        pool: SqlitePool,
    ) -> Result<(), ReconcileError> {
        let server_repository = ServerRepository::new(pool.clone());
        let instance_repository = ServerInstanceRepository::new(pool);
        let mut server = Server::new(
            ServerName::new("community").map_err(RepositoryError::from)?,
            valid_spec(),
        )
        .map_err(RepositoryError::from)?;
        server_repository.create(&server).await?;

        let previous_generation = server.generation;
        server
            .set_desired_state(DesiredState::Running)
            .map_err(RepositoryError::from)?;
        assert!(
            server_repository
                .update_desired_state(&server, previous_generation)
                .await?
        );

        let (_, worker) = ReconcileWorker::channel(
            server_repository.clone(),
            instance_repository.clone(),
            Duration::from_secs(30),
        );
        worker.reconcile_until_stable(server.id).await?;
        assert!(
            instance_repository
                .get_active_for_server(server.id)
                .await?
                .is_some()
        );

        let previous_generation = server.generation;
        server
            .set_desired_state(DesiredState::Stopped)
            .map_err(RepositoryError::from)?;
        assert!(
            server_repository
                .update_desired_state(&server, previous_generation)
                .await?
        );

        worker.reconcile_until_stable(server.id).await?;
        assert!(
            instance_repository
                .get_active_for_server(server.id)
                .await?
                .is_none()
        );
        let history = instance_repository.list_for_server(server.id).await?;
        assert_eq!(history.len(), 1);
        assert!(history.iter().all(|instance| {
            instance.stop_requested_at.is_some()
                && instance.terminated_at.is_some()
                && instance.terminal_result == Some(TerminalResult::Completed)
        }));
        Ok(())
    }
}
