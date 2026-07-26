use std::{collections::HashMap, time::Duration};

use mcserver_protocol::node_agent::{
    AgentInspectParams, AgentInspectResult, ChangedResult, CleanupInstanceParams,
    InstanceIdentity, ProcessSpec as AgentProcessSpec, RestoreDataParams, SnapshotDataParams,
    SnapshotDataResult, StartServerParams, StopServerParams, method,
};
use thiserror::Error;
use tokio::{sync::mpsc, time::Instant};
use tracing::{debug, error, info, warn};

use crate::{
    agent::{AgentCallError, AgentRegistry},
    domain::{
        Clock, ComputeInstance, DesiredState, Server, ServerId, ServerInstance,
        ServerInstanceId, SystemClock, TerminalResult, UnixTimestampMillis,
    },
    shutdown::CancellationToken,
    infrastructure::{
        ComputeInstanceRepository, LocalComputeError, LocalComputeManager, RepositoryError,
        ServerInstanceRepository, ServerRepository, SnapshotRepository,
    },
};

const RECONCILE_QUEUE_CAPACITY: usize = 256;
const MAX_IMMEDIATE_TRANSITIONS: usize = 32;
const MAX_ERROR_RETRY_DELAY: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy)]
enum ReconcileRequest {
    Server(ServerId),
    ServerInstance(ServerInstanceId),
}

#[derive(Debug, Clone)]
pub struct ReconcileScheduler {
    sender: mpsc::Sender<ReconcileRequest>,
}

impl ReconcileScheduler {
    pub fn enqueue_best_effort(&self, server_id: ServerId) {
        if let Err(error) = self.sender.try_send(ReconcileRequest::Server(server_id)) {
            debug!(%server_id, %error, "reconcile notification was coalesced into periodic resync");
        }
    }

    pub fn enqueue_best_effort_for_instance(&self, instance_id: ServerInstanceId) {
        if let Err(error) = self
            .sender
            .try_send(ReconcileRequest::ServerInstance(instance_id))
        {
            debug!(%instance_id, %error, "instance reconcile notification was coalesced into periodic resync");
        }
    }
}

pub struct ReconcileWorker {
    server_repository: ServerRepository,
    instance_repository: ServerInstanceRepository,
    compute_repository: ComputeInstanceRepository,
    snapshot_repository: SnapshotRepository,
    local_compute: LocalComputeManager,
    agents: AgentRegistry,
    receiver: mpsc::Receiver<ReconcileRequest>,
    resync_interval: Duration,
    retry_interval: Duration,
    agent_command_timeout: Duration,
    clock: SystemClock,
}

impl ReconcileWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn channel(
        server_repository: ServerRepository,
        instance_repository: ServerInstanceRepository,
        compute_repository: ComputeInstanceRepository,
        snapshot_repository: SnapshotRepository,
        local_compute: LocalComputeManager,
        agents: AgentRegistry,
        resync_interval: Duration,
        retry_interval: Duration,
        agent_command_timeout: Duration,
    ) -> (ReconcileScheduler, Self) {
        let (sender, receiver) = mpsc::channel(RECONCILE_QUEUE_CAPACITY);
        (
            ReconcileScheduler { sender },
            Self {
                server_repository,
                instance_repository,
                compute_repository,
                snapshot_repository,
                local_compute,
                agents,
                receiver,
                resync_interval,
                retry_interval,
                agent_command_timeout,
                clock: SystemClock,
            },
        )
    }

    pub async fn run(
        mut self,
        cancellation: CancellationToken,
    ) -> Result<(), ReconcileFatalError> {
        let mut resync = tokio::time::interval(self.resync_interval);
        resync.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut retry = tokio::time::interval(self.retry_interval);
        retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut pending = HashMap::<ServerId, RetryState>::new();

        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                request = self.receiver.recv() => {
                    let Some(request) = request else {
                        break;
                    };
                    if let Some(server_id) = self.resolve_request(request).await {
                        let previous_failures = pending
                            .remove(&server_id)
                            .map_or(0, |state| state.consecutive_failures);
                        self.reconcile_and_schedule(
                            server_id,
                            previous_failures,
                            &mut pending,
                        )
                        .await;
                    }
                }
                _ = retry.tick() => {
                    let now = Instant::now();
                    let server_ids = pending
                        .iter()
                        .filter_map(|(server_id, state)| {
                            (state.retry_at <= now).then_some(*server_id)
                        })
                        .collect::<Vec<_>>();
                    for server_id in server_ids {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let previous_failures = pending
                            .remove(&server_id)
                            .map_or(0, |state| state.consecutive_failures);
                        self.reconcile_and_schedule(
                            server_id,
                            previous_failures,
                            &mut pending,
                        )
                        .await;
                    }
                }
                _ = resync.tick() => {
                    match self.server_repository.list().await {
                        Ok(servers) => {
                            debug!(server_count = servers.len(), "starting periodic server resync");
                            let now = Instant::now();
                            for server in servers {
                                if cancellation.is_cancelled() {
                                    break;
                                }
                                if pending
                                    .get(&server.id)
                                    .is_some_and(|state| state.retry_at > now)
                                {
                                    continue;
                                }
                                let previous_failures = pending
                                    .remove(&server.id)
                                    .map_or(0, |state| state.consecutive_failures);
                                self.reconcile_and_schedule(
                                    server.id,
                                    previous_failures,
                                    &mut pending,
                                )
                                .await;
                            }
                        }
                        Err(error) => error!(%error, "periodic server resync could not list servers"),
                    }
                }
            }
        }

        info!("reconciliation worker stopped");
        Ok(())
    }

    async fn resolve_request(&self, request: ReconcileRequest) -> Option<ServerId> {
        match request {
            ReconcileRequest::Server(server_id) => Some(server_id),
            ReconcileRequest::ServerInstance(instance_id) => match self.instance_repository.get(instance_id).await {
                Ok(Some(instance)) => Some(instance.server_id),
                Ok(None) => None,
                Err(error) => {
                    warn!(%instance_id, %error, "failed to resolve instance reconcile notification");
                    None
                }
            },
        }
    }

    async fn reconcile_and_schedule(
        &self,
        server_id: ServerId,
        previous_failures: u32,
        pending: &mut HashMap<ServerId, RetryState>,
    ) {
        match self.reconcile_until_blocked(server_id).await {
            Ok(ReconcileOutcome::Stable) => {}
            Ok(ReconcileOutcome::Retry) => {
                pending.insert(
                    server_id,
                    RetryState {
                        consecutive_failures: 0,
                        retry_at: Instant::now() + self.retry_interval,
                    },
                );
            }
            Err(error) => {
                let consecutive_failures = previous_failures.saturating_add(1);
                let retry_delay = error_retry_delay(self.retry_interval, consecutive_failures);
                warn!(
                    %server_id,
                    %error,
                    ?retry_delay,
                    consecutive_failures,
                    "server reconciliation failed; retry scheduled"
                );
                pending.insert(
                    server_id,
                    RetryState {
                        consecutive_failures,
                        retry_at: Instant::now() + retry_delay,
                    },
                );
                self.record_error(server_id, &error).await;
            }
        }
    }

    async fn record_error(&self, server_id: ServerId, error: &ReconcileError) {
        let Ok(Some(instance)) = self
            .instance_repository
            .get_active_for_server(server_id)
            .await
        else {
            return;
        };
        let Ok(now) = self.clock.now() else {
            return;
        };
        if let Err(repository_error) = self
            .instance_repository
            .record_error(instance.id, &error.to_string(), now)
            .await
        {
            warn!(%server_id, %repository_error, "failed to persist reconcile error");
        }
    }

    async fn reconcile_until_blocked(
        &self,
        server_id: ServerId,
    ) -> Result<ReconcileOutcome, ReconcileError> {
        for _ in 0..MAX_IMMEDIATE_TRANSITIONS {
            match self.reconcile_once(server_id).await? {
                StepOutcome::Changed => continue,
                StepOutcome::Stable => return Ok(ReconcileOutcome::Stable),
                StepOutcome::Awaiting => return Ok(ReconcileOutcome::Retry),
            }
        }
        Err(ReconcileError::DidNotConverge(server_id))
    }

    async fn reconcile_once(&self, server_id: ServerId) -> Result<StepOutcome, ReconcileError> {
        let Some(server) = self.server_repository.get(server_id).await? else {
            return Ok(StepOutcome::Stable);
        };
        let active_instance = self
            .instance_repository
            .get_active_for_server(server_id)
            .await?;

        let Some(instance) = active_instance else {
            if server.desired_state == DesiredState::Running {
                let now = self.clock.now()?;
                if let Some(instance) = self
                    .instance_repository
                    .create_for_running_server(server.id, now)
                    .await?
                {
                    info!(
                        server_id = %server.id,
                        server_instance_id = %instance.id,
                        fencing_token = instance.fencing_token,
                        "server instance created"
                    );
                    return Ok(StepOutcome::Changed);
                }
            }
            return Ok(StepOutcome::Stable);
        };

        if server.desired_state == DesiredState::Stopped && instance.stop_requested_at.is_none() {
            let now = self.clock.now()?;
            if self.instance_repository.request_stop(instance.id, now).await? {
                info!(server_id = %server.id, server_instance_id = %instance.id, "server instance stop requested");
                return Ok(StepOutcome::Changed);
            }
        }

        if instance.stop_requested_at.is_some() {
            return self.reconcile_stopping(&server, &instance).await;
        }
        self.reconcile_running(&server, &instance).await
    }

    async fn reconcile_running(
        &self,
        server: &Server,
        instance: &ServerInstance,
    ) -> Result<StepOutcome, ReconcileError> {
        let provisioned_at = self.clock.now()?;
        let (compute, changed) = self
            .local_compute
            .ensure_for_instance(instance, provisioned_at)
            .await?;
        if changed {
            return Ok(StepOutcome::Changed);
        }
        if !self.agents.is_connected(compute.id).await {
            return Ok(StepOutcome::Awaiting);
        }

        let inspect = self.inspect(instance, &compute).await?;
        if instance.data_prepared_at.is_some()
            && instance.result_snapshot_id.is_none()
            && !inspect.data_prepared
        {
            return Err(ReconcileError::WritableDataLost(instance.id));
        }
        let observed_at = self.clock.now()?;
        if self
            .persist_observations(instance, &inspect, observed_at)
            .await?
        {
            return Ok(StepOutcome::Changed);
        }
        if !inspect.data_prepared {
            let params = RestoreDataParams {
                instance: identity(instance),
                server_id: server.id.as_uuid(),
                repository: instance.resolved_spec.data.repository.clone(),
                source_snapshot_id: instance.source_snapshot_id.clone(),
            };
            self.agents
                .call::<_, ChangedResult>(
                    compute.id,
                    method::DATA_RESTORE,
                    &params,
                    self.agent_command_timeout,
                )
                .await?;
            let prepared_at = self.clock.now()?;
            self.instance_repository
                .mark_data_prepared(instance.id, prepared_at)
                .await?;
            return Ok(StepOutcome::Changed);
        }
        if !inspect.process_running {
            let params = StartServerParams {
                instance: identity(instance),
                process: to_agent_process(&instance.resolved_spec.process),
            };
            self.agents
                .call::<_, ChangedResult>(
                    compute.id,
                    method::SERVER_START,
                    &params,
                    self.agent_command_timeout,
                )
                .await?;
            let started_at = self.clock.now()?;
            self.instance_repository
                .observe_process(instance.id, true, started_at)
                .await?;
            return Ok(StepOutcome::Changed);
        }

        Ok(StepOutcome::Stable)
    }

    async fn reconcile_stopping(
        &self,
        server: &Server,
        instance: &ServerInstance,
    ) -> Result<StepOutcome, ReconcileError> {
        let active_compute = self
            .compute_repository
            .get_active_for_instance(instance.id)
            .await?;
        let Some(compute) = active_compute else {
            if instance.data_prepared_at.is_some() && instance.result_snapshot_id.is_none() {
                return Err(ReconcileError::WritableDataLost(instance.id));
            }
            let now = self.clock.now()?;
            if self
                .instance_repository
                .complete(instance.id, TerminalResult::Completed, now)
                .await?
            {
                return Ok(StepOutcome::Changed);
            }
            return Ok(StepOutcome::Stable);
        };

        let provisioned_at = self.clock.now()?;
        let (_, compute_changed) = self
            .local_compute
            .ensure_for_instance(instance, provisioned_at)
            .await?;
        if compute_changed {
            return Ok(StepOutcome::Changed);
        }
        if !self.agents.is_connected(compute.id).await {
            return Ok(StepOutcome::Awaiting);
        }

        let inspect = self.inspect(instance, &compute).await?;
        if instance.data_prepared_at.is_some()
            && instance.result_snapshot_id.is_none()
            && !inspect.data_prepared
        {
            return Err(ReconcileError::WritableDataLost(instance.id));
        }
        let observed_at = self.clock.now()?;
        if self
            .persist_observations(instance, &inspect, observed_at)
            .await?
        {
            return Ok(StepOutcome::Changed);
        }
        if inspect.process_running {
            let params = StopServerParams {
                instance: identity(instance),
                stop_timeout_seconds: instance.resolved_spec.process.stop_timeout_seconds,
            };
            self.agents
                .call::<_, ChangedResult>(
                    compute.id,
                    method::SERVER_STOP,
                    &params,
                    self.agent_command_timeout,
                )
                .await?;
            let stopped_at = self.clock.now()?;
            self.instance_repository
                .observe_process(instance.id, false, stopped_at)
                .await?;
            return Ok(StepOutcome::Changed);
        }

        if instance.data_prepared_at.is_some() && instance.result_snapshot_id.is_none() {
            if let Some(snapshot_id) = inspect.last_snapshot_id {
                let committed_at = self.clock.now()?;
                self.snapshot_repository
                    .commit(
                        instance.id,
                        instance.fencing_token,
                        &snapshot_id,
                        committed_at,
                    )
                    .await?;
                return Ok(StepOutcome::Changed);
            }
            let params = SnapshotDataParams {
                instance: identity(instance),
                server_id: server.id.as_uuid(),
                repository: instance.resolved_spec.data.repository.clone(),
            };
            let result = self
                .agents
                .call::<_, SnapshotDataResult>(
                    compute.id,
                    method::DATA_SNAPSHOT,
                    &params,
                    self.agent_command_timeout,
                )
                .await?;
            let committed_at = self.clock.now()?;
            self.snapshot_repository
                .commit(
                    instance.id,
                    instance.fencing_token,
                    &result.snapshot_id,
                    committed_at,
                )
                .await?;
            return Ok(StepOutcome::Changed);
        }

        let cleanup = self
            .agents
            .call::<_, ChangedResult>(
                compute.id,
                method::INSTANCE_CLEANUP,
                &CleanupInstanceParams {
                    instance: identity(instance),
                },
                self.agent_command_timeout,
            )
            .await?;
        if cleanup.changed {
            return Ok(StepOutcome::Changed);
        }

        if self.local_compute.delete(&compute).await? {
            return Ok(StepOutcome::Changed);
        }
        Ok(StepOutcome::Awaiting)
    }

    async fn inspect(
        &self,
        instance: &ServerInstance,
        compute: &ComputeInstance,
    ) -> Result<AgentInspectResult, ReconcileError> {
        self.agents
            .call(
                compute.id,
                method::AGENT_INSPECT,
                &AgentInspectParams {
                    instance: identity(instance),
                },
                self.agent_command_timeout,
            )
            .await
            .map_err(ReconcileError::from)
    }

    async fn persist_observations(
        &self,
        instance: &ServerInstance,
        inspect: &AgentInspectResult,
        now: UnixTimestampMillis,
    ) -> Result<bool, ReconcileError> {
        if inspect.data_prepared && instance.data_prepared_at.is_none() {
            self.instance_repository.mark_data_prepared(instance.id, now).await?;
            return Ok(true);
        }
        if instance.process_observed_at.is_none() || instance.process_running != inspect.process_running {
            self.instance_repository
                .observe_process(instance.id, inspect.process_running, now)
                .await?;
            return Ok(true);
        }
        Ok(false)
    }
}

fn identity(instance: &ServerInstance) -> InstanceIdentity {
    InstanceIdentity {
        server_instance_id: instance.id.as_uuid(),
        fencing_token: instance.fencing_token,
    }
}

fn to_agent_process(process: &crate::domain::ProcessSpec) -> AgentProcessSpec {
    AgentProcessSpec {
        container_image: process.container_image.clone(),
        server_type: process.server_type.clone(),
        version: process.version.clone(),
        host_port: process.host_port,
        stop_timeout_seconds: process.stop_timeout_seconds,
        accept_eula: process.accept_eula,
        environment: process.environment.clone(),
    }
}

#[derive(Debug, Clone, Copy)]
struct RetryState {
    consecutive_failures: u32,
    retry_at: Instant,
}

fn error_retry_delay(base: Duration, consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    let multiplier = 1_u32 << exponent;
    base.checked_mul(multiplier)
        .unwrap_or(MAX_ERROR_RETRY_DELAY)
        .min(MAX_ERROR_RETRY_DELAY)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepOutcome {
    Changed,
    Stable,
    Awaiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileOutcome {
    Stable,
    Retry,
}

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("reconciliation persistence operation failed")]
    Repository(#[from] RepositoryError),
    #[error("reconciliation timestamp operation failed")]
    Timestamp(#[from] crate::domain::TimestampError),
    #[error("local compute operation failed")]
    LocalCompute(#[from] LocalComputeError),
    #[error("node-agent operation failed")]
    Agent(#[from] AgentCallError),
    #[error("server reconciliation did not converge for {0}")]
    DidNotConverge(ServerId),
    #[error("writable data for server instance {0} has no surviving compute instance")]
    WritableDataLost(ServerInstanceId),
}

#[derive(Debug, Error)]
pub enum ReconcileFatalError {
    #[error("reconciliation worker channel closed unexpectedly")]
    ChannelClosed,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MAX_ERROR_RETRY_DELAY, error_retry_delay};

    #[test]
    fn error_retry_delay_grows_and_is_capped() {
        let base = Duration::from_secs(5);

        assert_eq!(error_retry_delay(base, 1), Duration::from_secs(5));
        assert_eq!(error_retry_delay(base, 2), Duration::from_secs(10));
        assert_eq!(error_retry_delay(base, 7), MAX_ERROR_RETRY_DELAY);
        assert_eq!(error_retry_delay(base, u32::MAX), MAX_ERROR_RETRY_DELAY);
    }
}
