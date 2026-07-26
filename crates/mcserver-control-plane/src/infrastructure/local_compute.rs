use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use mcserver_protocol::node_agent::{ShutdownResult, method};
use thiserror::Error;
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    agent::AgentRegistry,
    domain::{
        Clock, ComputeInstance, ComputeInstanceId, ComputeTerminalResult, ServerInstance,
        SystemClock, UnixTimestampMillis,
    },
};

use super::{ComputeInstanceRepository, RepositoryError};

#[derive(Clone)]
pub struct LocalComputeManager {
    repository: ComputeInstanceRepository,
    agents: AgentRegistry,
    node_agent_binary: PathBuf,
    node_agent_root: PathBuf,
    control_plane_address: String,
    command_timeout: Duration,
    max_frame_bytes: usize,
    process_stop_timeout: Duration,
    children: Arc<Mutex<HashMap<ComputeInstanceId, Child>>>,
    clock: SystemClock,
}

impl LocalComputeManager {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        repository: ComputeInstanceRepository,
        agents: AgentRegistry,
        node_agent_binary: PathBuf,
        node_agent_root: PathBuf,
        control_plane_address: String,
        command_timeout: Duration,
        max_frame_bytes: usize,
        process_stop_timeout: Duration,
    ) -> Self {
        Self {
            repository,
            agents,
            node_agent_binary,
            node_agent_root,
            control_plane_address,
            command_timeout,
            max_frame_bytes,
            process_stop_timeout,
            children: Arc::new(Mutex::new(HashMap::new())),
            clock: SystemClock,
        }
    }

    pub async fn ensure_for_instance(
        &self,
        instance: &ServerInstance,
        now: UnixTimestampMillis,
    ) -> Result<(ComputeInstance, bool), LocalComputeError> {
        let (compute, mut changed) = match self
            .repository
            .get_active_for_instance(instance.id)
            .await?
        {
            Some(compute) => (compute, false),
            None => {
                let token = Uuid::new_v4().to_string();
                let compute = self
                    .repository
                    .create_for_instance(instance.id, &token, now)
                    .await?
                    .ok_or(LocalComputeError::CreateConflict)?;
                (compute, true)
            }
        };

        if !self.process_is_running(&compute).await? {
            let (process_id, mut child) = self.spawn_agent(&compute).await?;
            let spawned_at = self.clock.now()?;
            match self
                .repository
                .record_process_id(compute.id, process_id, spawned_at)
                .await
            {
                Ok(true) => {
                    self.children.lock().await.insert(compute.id, child);
                }
                Ok(false) => {
                    terminate_spawned_child(&mut child, compute.id).await;
                    return Err(LocalComputeError::MissingAfterUpdate);
                }
                Err(error) => {
                    terminate_spawned_child(&mut child, compute.id).await;
                    return Err(error.into());
                }
            }
            changed = true;
        }

        let compute = self
            .repository
            .get(compute.id)
            .await?
            .ok_or(LocalComputeError::MissingAfterUpdate)?;
        Ok((compute, changed))
    }

    pub async fn delete(
        &self,
        compute: &ComputeInstance,
    ) -> Result<bool, LocalComputeError> {
        let shutdown_requested_at = self.clock.now()?;
        self.repository
            .request_shutdown(compute.id, shutdown_requested_at)
            .await?;

        if self.agents.is_connected(compute.id).await {
            let result = self
                .agents
                .call::<_, ShutdownResult>(
                    compute.id,
                    method::NODE_SHUTDOWN,
                    &serde_json::json!({}),
                    self.command_timeout,
                )
                .await;
            if let Err(error) = result {
                warn!(compute_instance_id = %compute.id, %error, "node-agent shutdown request failed");
            }
        }

        let tracked_child = self.children.lock().await.remove(&compute.id);
        match (tracked_child, compute.process_id) {
            (Some(child), _) => {
                wait_for_tracked_process_exit(child, self.process_stop_timeout).await?;
            }
            (None, Some(process_id)) => {
                wait_for_untracked_process_exit(
                    process_id,
                    compute.id,
                    self.process_stop_timeout,
                )
                .await?;
            }
            (None, None) => {}
        }

        let state_directory = self.state_directory(compute.id);
        match tokio::fs::remove_dir_all(&state_directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let deleted_at = self.clock.now()?;
        let changed = self
            .repository
            .terminate(compute.id, ComputeTerminalResult::Deleted, None, deleted_at)
            .await?;
        if changed {
            info!(compute_instance_id = %compute.id, "local compute instance deleted");
        }
        Ok(changed)
    }

    async fn process_is_running(
        &self,
        compute: &ComputeInstance,
    ) -> Result<bool, LocalComputeError> {
        {
            let mut children = self.children.lock().await;
            if let Some(child) = children.get_mut(&compute.id) {
                if child.try_wait()?.is_none() {
                    return Ok(true);
                }
                children.remove(&compute.id);
                return Ok(false);
            }
        }

        match compute.process_id {
            Some(process_id) => Ok(process_matches_compute(process_id, compute.id).await?),
            None => Ok(false),
        }
    }

    async fn spawn_agent(
        &self,
        compute: &ComputeInstance,
    ) -> Result<(u32, Child), LocalComputeError> {
        let state_directory = self.state_directory(compute.id);
        tokio::fs::create_dir_all(&state_directory).await?;
        tokio::fs::set_permissions(
            &state_directory,
            std::fs::Permissions::from_mode(0o700),
        )
        .await?;

        let mut command = Command::new(&self.node_agent_binary);
        command
            .env(
                "MCSERVER_NODE_AGENT_CONTROL_PLANE_ADDRESS",
                &self.control_plane_address,
            )
            .env(
                "MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID",
                compute.id.to_string(),
            )
            .env(
                "MCSERVER_NODE_AGENT_CONNECTION_TOKEN",
                &compute.connection_token,
            )
            .env(
                "MCSERVER_NODE_AGENT_STATE_DIRECTORY",
                &state_directory,
            )
            .env(
                "MCSERVER_NODE_AGENT_MAX_FRAME_BYTES",
                self.max_frame_bytes.to_string(),
            )
            .env(
                "MCSERVER_NODE_AGENT_COMMAND_TIMEOUT_SECONDS",
                node_operation_timeout(self.command_timeout).as_secs().to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(false);

        let mut child = command.spawn().map_err(|source| LocalComputeError::Spawn {
            binary: self.node_agent_binary.clone(),
            source,
        })?;
        let Some(process_id) = child.id() else {
            terminate_spawned_child(&mut child, compute.id).await;
            return Err(LocalComputeError::MissingProcessId);
        };
        info!(
            compute_instance_id = %compute.id,
            process_id,
            "local node agent spawned"
        );
        Ok((process_id, child))
    }

    fn state_directory(&self, id: ComputeInstanceId) -> PathBuf {
        self.node_agent_root.join(id.to_string())
    }
}

fn node_operation_timeout(agent_call_timeout: Duration) -> Duration {
    agent_call_timeout
        .checked_sub(Duration::from_secs(5))
        .filter(|duration| !duration.is_zero())
        .unwrap_or(Duration::from_secs(1))
}

async fn terminate_spawned_child(child: &mut Child, compute_id: ComputeInstanceId) {
    if let Err(error) = child.kill().await {
        warn!(
            compute_instance_id = %compute_id,
            %error,
            "failed to terminate an unpersisted node-agent process"
        );
        return;
    }
    if let Err(error) = child.wait().await {
        warn!(
            compute_instance_id = %compute_id,
            %error,
            "failed to reap an unpersisted node-agent process"
        );
    }
}

async fn process_matches_compute(
    process_id: u32,
    compute_instance_id: ComputeInstanceId,
) -> Result<bool, std::io::Error> {
    let environment_path = PathBuf::from(format!("/proc/{process_id}/environ"));
    let environment = match tokio::fs::read(environment_path).await {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let expected = format!("MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID={compute_instance_id}");
    Ok(environment
        .split(|byte| *byte == 0)
        .any(|entry| entry == expected.as_bytes()))
}

async fn wait_for_tracked_process_exit(
    mut child: Child,
    timeout: Duration,
) -> Result<(), LocalComputeError> {
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => {
            result?;
            Ok(())
        }
        Err(_) => {
            warn!(process_id = ?child.id(), "node-agent did not stop in time; forcing termination");
            child.kill().await?;
            child.wait().await?;
            Ok(())
        }
    }
}

async fn wait_for_untracked_process_exit(
    process_id: u32,
    compute_instance_id: ComputeInstanceId,
    timeout: Duration,
) -> Result<(), LocalComputeError> {
    if !process_matches_compute(process_id, compute_instance_id).await? {
        return Ok(());
    }

    send_signal(process_id, "TERM").await?;
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_matches_compute(process_id, compute_instance_id).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    send_signal(process_id, "KILL").await?;
    let kill_started = Instant::now();
    while kill_started.elapsed() < Duration::from_secs(2) {
        if !process_matches_compute(process_id, compute_instance_id).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(LocalComputeError::ProcessDidNotExit(process_id))
}

async fn send_signal(process_id: u32, signal: &str) -> Result<(), LocalComputeError> {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(process_id.to_string())
        .status()
        .await?;
    if status.success() {
        return Ok(());
    }
    if !PathBuf::from(format!("/proc/{process_id}")).exists() {
        return Ok(());
    }
    Err(LocalComputeError::SignalFailed {
        process_id,
        signal: OsString::from(signal),
        status: status.code(),
    })
}

#[derive(Debug, Error)]
pub enum LocalComputeError {
    #[error("local compute persistence failed")]
    Repository(#[from] RepositoryError),
    #[error("local compute filesystem or process operation failed")]
    Io(#[from] std::io::Error),
    #[error("local compute timestamp generation failed")]
    Timestamp(#[from] crate::domain::TimestampError),
    #[error("failed to spawn node-agent binary {binary}")]
    Spawn {
        binary: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("spawned node-agent has no process id")]
    MissingProcessId,
    #[error("local compute instance creation conflicted")]
    CreateConflict,
    #[error("local compute instance disappeared after update")]
    MissingAfterUpdate,
    #[error("failed to signal process {process_id} with {signal:?}; status {status:?}")]
    SignalFailed {
        process_id: u32,
        signal: OsString,
        status: Option<i32>,
    },
    #[error("process {0} did not exit after forced termination")]
    ProcessDidNotExit(u32),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::node_operation_timeout;

    #[test]
    fn node_operation_timeout_leaves_response_budget() {
        assert_eq!(
            node_operation_timeout(Duration::from_secs(900)),
            Duration::from_secs(895)
        );
        assert_eq!(
            node_operation_timeout(Duration::from_secs(3)),
            Duration::from_secs(1)
        );
    }
}
