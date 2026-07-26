use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info};

use crate::{
    domain::{DesiredState, ServerId},
    infrastructure::{RepositoryError, ServerRepository},
};

const RECONCILE_QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct ReconcileScheduler {
    sender: mpsc::Sender<ServerId>,
}

impl ReconcileScheduler {
    pub async fn enqueue(
        &self,
        server_id: ServerId,
    ) -> Result<(), crate::application::ApplicationError> {
        self.sender
            .send(server_id)
            .await
            .map_err(|_| crate::application::ApplicationError::ReconcileSchedulerUnavailable)
    }
}

pub struct ReconcileWorker {
    repository: ServerRepository,
    receiver: mpsc::Receiver<ServerId>,
    resync_interval: Duration,
}

impl ReconcileWorker {
    pub fn channel(
        repository: ServerRepository,
        resync_interval: Duration,
    ) -> (ReconcileScheduler, Self) {
        let (sender, receiver) = mpsc::channel(RECONCILE_QUEUE_CAPACITY);
        (
            ReconcileScheduler { sender },
            Self {
                repository,
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
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                maybe_server_id = self.receiver.recv() => {
                    match maybe_server_id {
                        Some(server_id) => self.reconcile(server_id).await?,
                        None => break,
                    }
                }
                _ = interval.tick() => {
                    self.resync().await?;
                }
            }
        }

        Ok(())
    }

    async fn resync(&self) -> Result<(), ReconcileError> {
        let servers = self.repository.list().await?;
        debug!(server_count = servers.len(), "starting periodic server resync");

        for server in servers {
            self.reconcile(server.id).await?;
        }

        Ok(())
    }

    async fn reconcile(&self, server_id: ServerId) -> Result<(), ReconcileError> {
        let Some(server) = self.repository.get(server_id).await? else {
            return Ok(());
        };

        match server.desired_state {
            DesiredState::Running => info!(
                server_id = %server.id,
                generation = server.generation,
                "server desires running; ServerInstance reconciliation is the next milestone"
            ),
            DesiredState::Stopped => debug!(
                server_id = %server.id,
                generation = server.generation,
                "server desires stopped"
            ),
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("reconciliation persistence operation failed")]
    Repository(#[from] RepositoryError),
}

pub fn log_worker_failure(error: &ReconcileError) {
    error!(%error, "reconciliation worker stopped unexpectedly");
}
