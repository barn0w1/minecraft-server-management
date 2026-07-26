use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Output, Stdio},
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
        ServerInstanceId, SystemClock, UnixTimestampMillis,
    },
};

use super::{ComputeInstanceRepository, RepositoryError};

#[derive(Clone)]
pub struct LocalComputeManager {
    repository: ComputeInstanceRepository,
    agents: AgentRegistry,
    node_agent_binary: PathBuf,
    node_agent_root: PathBuf,
    podman_binary: PathBuf,
    local_scope: String,
    control_plane_address: String,
    command_timeout: Duration,
    max_frame_bytes: usize,
    control_timeout: Duration,
    process_stop_timeout: Duration,
    children: Arc<Mutex<HashMap<ComputeInstanceId, Child>>>,
    clock: SystemClock,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrphanReapSummary {
    pub containers_removed: usize,
    pub processes_stopped: usize,
    pub state_directories_removed: usize,
}

impl LocalComputeManager {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        repository: ComputeInstanceRepository,
        agents: AgentRegistry,
        node_agent_binary: PathBuf,
        node_agent_root: PathBuf,
        podman_binary: PathBuf,
        local_scope: String,
        control_plane_address: String,
        command_timeout: Duration,
        max_frame_bytes: usize,
        control_timeout: Duration,
        process_stop_timeout: Duration,
    ) -> Self {
        Self {
            repository,
            agents,
            node_agent_binary,
            node_agent_root,
            podman_binary,
            local_scope,
            control_plane_address,
            command_timeout,
            max_frame_bytes,
            control_timeout,
            process_stop_timeout,
            children: Arc::new(Mutex::new(HashMap::new())),
            clock: SystemClock,
        }
    }

    pub async fn reap_orphans(
        &self,
        active_compute_ownership: &[(ComputeInstanceId, ServerInstanceId)],
    ) -> Result<OrphanReapSummary, LocalComputeError> {
        let active_computes = active_compute_ownership
            .iter()
            .copied()
            .collect::<HashMap<_, _>>();
        let mut summary = OrphanReapSummary::default();

        let scope_filter = format!("label=io.mcserver.local-scope={}", self.local_scope);
        let output = self
            .run_podman_control([
                "ps",
                "--all",
                "--filter",
                "label=io.mcserver.managed=true",
                "--filter",
                &scope_filter,
                "--format",
                r#"{{.Names}}|{{.Label "io.mcserver.server-instance-id"}}|{{.Label "io.mcserver.compute-instance-id"}}"#,
            ])
            .await?;
        let listed = String::from_utf8(output.stdout)
            .map_err(LocalComputeError::InvalidPodmanOutputEncoding)?;
        for line in listed.lines().filter(|line| !line.trim().is_empty()) {
            let mut fields = line.split('|');
            let name = fields.next().unwrap_or_default().trim();
            let instance_id = fields.next().unwrap_or_default();
            let compute_id = fields.next().unwrap_or_default();
            let has_extra_fields = fields.next().is_some();
            if name.is_empty() {
                warn!(output = %line, "ignoring managed container without a name");
                continue;
            }
            let instance_id = instance_id
                .parse::<Uuid>()
                .ok()
                .map(ServerInstanceId::from_uuid);
            let compute_id = compute_id
                .parse::<Uuid>()
                .ok()
                .map(ComputeInstanceId::from_uuid);
            let belongs_to_active_runtime = !has_extra_fields
                && compute_id
                    .and_then(|id| active_computes.get(&id))
                    .is_some_and(|expected_instance_id| Some(*expected_instance_id) == instance_id);
            if belongs_to_active_runtime {
                continue;
            }
            if has_extra_fields || instance_id.is_none() || compute_id.is_none() {
                warn!(
                    container = name,
                    output = %line,
                    "removing managed container with invalid ownership labels"
                );
            }
            self.run_podman_control(["rm", "--force", "--ignore", name])
                .await?;
            summary.containers_removed = summary.containers_removed.saturating_add(1);
            info!(container = name, "removed orphaned managed container");
        }

        for (process_id, compute_id) in list_processes_for_scope(&self.local_scope).await? {
            if active_computes.contains_key(&compute_id) {
                continue;
            }
            wait_for_untracked_process_exit(
                process_id,
                compute_id,
                &self.local_scope,
                self.process_stop_timeout,
            )
            .await?;
            summary.processes_stopped = summary.processes_stopped.saturating_add(1);
        }

        let mut entries = match tokio::fs::read_dir(&self.node_agent_root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(summary),
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                warn!(path = %entry.path().display(), "ignoring non-UTF-8 local compute directory");
                continue;
            };
            let Ok(id) = name.parse::<Uuid>() else {
                warn!(path = %entry.path().display(), "ignoring local compute directory with invalid id");
                continue;
            };
            let compute_id = ComputeInstanceId::from_uuid(id);
            if active_computes.contains_key(&compute_id) {
                continue;
            }
            self.remove_state_directory_in_user_namespace(&entry.path())
                .await?;
            summary.state_directories_removed = summary.state_directories_removed.saturating_add(1);
            info!(compute_instance_id = %compute_id, "removed orphaned local compute state");
        }
        Ok(summary)
    }

    async fn run_podman_control<const N: usize>(
        &self,
        arguments: [&str; N],
    ) -> Result<Output, LocalComputeError> {
        let mut command = Command::new(&self.podman_binary);
        command.args(arguments).kill_on_drop(true);
        let output = tokio::time::timeout(self.control_timeout, command.output())
            .await
            .map_err(|_| LocalComputeError::ControlCommandTimeout(self.control_timeout))??;
        if output.status.success() {
            Ok(output)
        } else {
            Err(LocalComputeError::PodmanCommandFailed {
                status: output.status.code(),
                stderr: bounded_diagnostic(&output.stderr),
            })
        }
    }

    async fn remove_state_directory_in_user_namespace(
        &self,
        path: &std::path::Path,
    ) -> Result<(), LocalComputeError> {
        let mut command = Command::new(&self.podman_binary);
        command
            .arg("unshare")
            .arg("rm")
            .arg("-rf")
            .arg("--")
            .arg(path)
            .kill_on_drop(true);
        let output = tokio::time::timeout(self.control_timeout, command.output())
            .await
            .map_err(|_| LocalComputeError::ControlCommandTimeout(self.control_timeout))??;
        if output.status.success() {
            Ok(())
        } else {
            Err(LocalComputeError::PodmanCommandFailed {
                status: output.status.code(),
                stderr: bounded_diagnostic(&output.stderr),
            })
        }
    }

    pub async fn ensure_for_instance(
        &self,
        instance: &ServerInstance,
        now: UnixTimestampMillis,
    ) -> Result<(ComputeInstance, bool), LocalComputeError> {
        let (compute, mut changed) =
            match self.repository.get_active_for_instance(instance.id).await? {
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

    pub async fn delete(&self, compute: &ComputeInstance) -> Result<bool, LocalComputeError> {
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
                    &self.local_scope,
                    self.process_stop_timeout,
                )
                .await?;
            }
            (None, None) => {}
        }

        let state_directory = self.state_directory(compute.id);
        self.remove_state_directory_in_user_namespace(&state_directory)
            .await?;

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
            Some(process_id) => {
                Ok(process_matches_compute(process_id, compute.id, &self.local_scope).await?)
            }
            None => Ok(false),
        }
    }

    async fn spawn_agent(
        &self,
        compute: &ComputeInstance,
    ) -> Result<(u32, Child), LocalComputeError> {
        let state_directory = self.state_directory(compute.id);
        tokio::fs::create_dir_all(&state_directory).await?;
        tokio::fs::set_permissions(&state_directory, std::fs::Permissions::from_mode(0o700))
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
            .env("MCSERVER_NODE_AGENT_STATE_DIRECTORY", &state_directory)
            .env("MCSERVER_NODE_AGENT_LOCAL_SCOPE", &self.local_scope)
            .env(
                "MCSERVER_NODE_AGENT_MAX_FRAME_BYTES",
                self.max_frame_bytes.to_string(),
            )
            .env(
                "MCSERVER_NODE_AGENT_COMMAND_TIMEOUT_SECONDS",
                node_operation_timeout(self.command_timeout)
                    .as_secs()
                    .to_string(),
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

async fn list_processes_for_scope(
    local_scope: &str,
) -> Result<Vec<(u32, ComputeInstanceId)>, std::io::Error> {
    let mut matches = Vec::new();
    let mut entries = tokio::fs::read_dir("/proc").await?;
    while let Some(entry) = entries.next_entry().await? {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(process_id) = name.parse::<u32>() else {
            continue;
        };
        match process_runtime_identity(process_id).await {
            Ok(Some((compute_id, process_scope))) if process_scope == local_scope => {
                matches.push((process_id, compute_id));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(matches)
}

async fn process_matches_compute(
    process_id: u32,
    compute_instance_id: ComputeInstanceId,
    local_scope: &str,
) -> Result<bool, std::io::Error> {
    Ok(process_runtime_identity(process_id)
        .await?
        .is_some_and(|(id, scope)| id == compute_instance_id && scope == local_scope))
}

async fn process_runtime_identity(
    process_id: u32,
) -> Result<Option<(ComputeInstanceId, String)>, std::io::Error> {
    let environment_path = PathBuf::from(format!("/proc/{process_id}/environ"));
    let environment = match tokio::fs::read(environment_path).await {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut compute_id = None;
    let mut local_scope = None;
    for entry in environment.split(|byte| *byte == 0) {
        if let Some(value) = entry.strip_prefix(b"MCSERVER_NODE_AGENT_COMPUTE_INSTANCE_ID=") {
            compute_id = std::str::from_utf8(value)
                .ok()
                .and_then(|value| value.parse::<Uuid>().ok())
                .map(ComputeInstanceId::from_uuid);
        } else if let Some(value) = entry.strip_prefix(b"MCSERVER_NODE_AGENT_LOCAL_SCOPE=") {
            local_scope = std::str::from_utf8(value).ok().map(str::to_owned);
        }
    }
    Ok(compute_id.zip(local_scope))
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
    local_scope: &str,
    timeout: Duration,
) -> Result<(), LocalComputeError> {
    if !process_matches_compute(process_id, compute_instance_id, local_scope).await? {
        return Ok(());
    }

    send_signal(process_id, "TERM").await?;
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_matches_compute(process_id, compute_instance_id, local_scope).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    send_signal(process_id, "KILL").await?;
    let kill_started = Instant::now();
    while kill_started.elapsed() < Duration::from_secs(2) {
        if !process_matches_compute(process_id, compute_instance_id, local_scope).await? {
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

fn bounded_diagnostic(input: &[u8]) -> String {
    const MAX_BYTES: usize = 8192;
    let truncated = input.len() > MAX_BYTES;
    let input = &input[..input.len().min(MAX_BYTES)];
    let mut diagnostic = String::from_utf8_lossy(input).trim().to_owned();
    if truncated {
        diagnostic.push_str(" …[truncated]");
    }
    diagnostic
}

#[derive(Debug, Error)]
pub enum LocalComputeError {
    #[error("local compute persistence failed: {0}")]
    Repository(#[from] RepositoryError),
    #[error("local compute filesystem or process operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("local compute timestamp generation failed: {0}")]
    Timestamp(#[from] crate::domain::TimestampError),
    #[error("Podman output was not valid UTF-8")]
    InvalidPodmanOutputEncoding(#[source] std::string::FromUtf8Error),
    #[error("local Podman control command exceeded {0:?}")]
    ControlCommandTimeout(Duration),
    #[error("Podman command failed with status {status:?}: {stderr}")]
    PodmanCommandFailed { status: Option<i32>, stderr: String },
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
