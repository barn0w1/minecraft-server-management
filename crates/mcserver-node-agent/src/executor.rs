use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
};

use mcserver_protocol::node_agent::{
    AgentInspectResult, ChangedResult, CleanupInstanceParams, InstanceIdentity, ProcessSpec,
    RestoreDataParams, SnapshotDataParams, SnapshotDataResult, StartServerParams, StopServerParams,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt, process::Command};
use tracing::{debug, info};
use uuid::Uuid;

use crate::config::{Config, DataAccessMode};

const STATE_FILE_NAME: &str = "agent-state.json";
const DATA_DIRECTORY_NAME: &str = "data";
const RESTORE_STAGING_DIRECTORY_NAME: &str = "restore-staging";
const PREVIOUS_DATA_DIRECTORY_NAME: &str = "data-previous";
const AGENT_STATE_SCHEMA_VERSION: u32 = 1;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const MAX_DIAGNOSTIC_BYTES: usize = 8192;
const MAX_SNAPSHOT_ID_CHARS: usize = 256;
const MANAGED_LABEL: &str = "io.mcserver.managed";
const LOCAL_SCOPE_LABEL: &str = "io.mcserver.local-scope";
const SERVER_ID_LABEL: &str = "io.mcserver.server-id";
const SERVER_INSTANCE_ID_LABEL: &str = "io.mcserver.server-instance-id";
const COMPUTE_INSTANCE_ID_LABEL: &str = "io.mcserver.compute-instance-id";

#[derive(Debug, Clone)]
pub struct AgentExecutor {
    config: Config,
}

impl AgentExecutor {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn inspect(
        &self,
        identity: InstanceIdentity,
    ) -> Result<AgentInspectResult, ExecutorError> {
        let state = self.load_state().await?;
        if let Some(instance) = state.instance.as_ref() {
            validate_identity(instance, identity)?;
        }
        let process_running = self.container_running(identity.server_instance_id).await?;
        Ok(AgentInspectResult {
            data_prepared: state
                .instance
                .as_ref()
                .is_some_and(|value| value.data_prepared),
            process_running,
            last_snapshot_id: state.instance.and_then(|value| value.last_snapshot_id),
        })
    }

    pub async fn restore_data(
        &self,
        params: RestoreDataParams,
    ) -> Result<ChangedResult, ExecutorError> {
        let mut state = self.load_state().await?;
        ensure_instance_state(
            &mut state,
            params.instance,
            params.server_id,
            &params.repository,
            params.source_snapshot_id.as_deref(),
        )?;
        if state
            .instance
            .as_ref()
            .is_some_and(|instance| instance.data_prepared)
        {
            return Ok(ChangedResult { changed: false });
        }

        ensure_private_directory(&self.config.state_directory).await?;
        match params.source_snapshot_id.as_deref() {
            Some(snapshot_id) => {
                self.restore_snapshot(&params.repository, snapshot_id)
                    .await?;
            }
            None => {
                self.remove_paths([self.data_directory()])
                    .await?;
                fs::create_dir_all(self.data_directory()).await?;
            }
        }
        let instance = state
            .instance
            .as_mut()
            .ok_or(ExecutorError::UnknownInstance)?;
        validate_identity(instance, params.instance)?;
        instance.data_prepared = true;
        self.store_state(&state).await?;
        info!(server_instance_id = %params.instance.server_instance_id, "server data prepared");
        Ok(ChangedResult { changed: true })
    }

    pub async fn start_server(
        &self,
        params: StartServerParams,
    ) -> Result<ChangedResult, ExecutorError> {
        let state = self.load_state().await?;
        let instance = state
            .instance
            .as_ref()
            .ok_or(ExecutorError::DataNotPrepared)?;
        validate_identity(instance, params.instance)?;
        if !instance.data_prepared {
            return Err(ExecutorError::DataNotPrepared);
        }
        if self
            .container_running(params.instance.server_instance_id)
            .await?
        {
            return Ok(ChangedResult { changed: false });
        }

        let server_id = instance.server_id;
        self.remove_container(params.instance.server_instance_id)
            .await?;
        self.create_container(
            params.instance.server_instance_id,
            server_id,
            &params.process,
        )
        .await?;
        self.run_podman(
            ["start", &container_name(params.instance.server_instance_id)],
            None,
        )
        .await?;
        info!(server_instance_id = %params.instance.server_instance_id, "Minecraft container started");
        Ok(ChangedResult { changed: true })
    }

    pub async fn stop_server(
        &self,
        params: StopServerParams,
    ) -> Result<ChangedResult, ExecutorError> {
        let state = self.load_state().await?;
        let instance = state
            .instance
            .as_ref()
            .ok_or(ExecutorError::UnknownInstance)?;
        validate_identity(instance, params.instance)?;
        if !self
            .container_running(params.instance.server_instance_id)
            .await?
        {
            return Ok(ChangedResult { changed: false });
        }

        let timeout = params.stop_timeout_seconds.to_string();
        let name = container_name(params.instance.server_instance_id);
        self.run_podman(["stop", "--time", &timeout, &name], None)
            .await?;
        info!(server_instance_id = %params.instance.server_instance_id, "Minecraft container stopped");
        Ok(ChangedResult { changed: true })
    }

    pub async fn snapshot_data(
        &self,
        params: SnapshotDataParams,
    ) -> Result<SnapshotDataResult, ExecutorError> {
        let mut state = self.load_state().await?;
        {
            let instance = state
                .instance
                .as_ref()
                .ok_or(ExecutorError::UnknownInstance)?;
            validate_identity(instance, params.instance)?;
            if instance.server_id != params.server_id || instance.repository != params.repository {
                return Err(ExecutorError::ImmutableInstanceConfigurationChanged);
            }
        }
        if self
            .container_running(params.instance.server_instance_id)
            .await?
        {
            return Err(ExecutorError::ProcessStillRunning);
        }
        self.ensure_restic_repository(&params.repository).await?;

        let server_tag = format!("mcserver-server:{}", params.server_id);
        let instance_tag = format!("mcserver-instance:{}", params.instance.server_instance_id);
        let output = self
            .run_restic(
                &params.repository,
                [
                    "backup",
                    DATA_DIRECTORY_NAME,
                    "--json",
                    "--quiet",
                    "--tag",
                    &server_tag,
                    "--tag",
                    &instance_tag,
                ],
                Some(&self.config.state_directory),
            )
            .await?;
        let snapshot_id = parse_backup_snapshot_id(&output.stdout)?;
        let instance = state
            .instance
            .as_mut()
            .ok_or(ExecutorError::UnknownInstance)?;
        validate_identity(instance, params.instance)?;
        instance.last_snapshot_id = Some(snapshot_id.clone());
        self.store_state(&state).await?;
        info!(server_instance_id = %params.instance.server_instance_id, %snapshot_id, "server data snapshot created");
        Ok(SnapshotDataResult { snapshot_id })
    }

    pub async fn cleanup_instance(
        &self,
        params: CleanupInstanceParams,
    ) -> Result<ChangedResult, ExecutorError> {
        let state = self.load_state().await?;
        if let Some(instance) = state.instance.as_ref() {
            validate_identity(instance, params.instance)?;
        }
        let container_existed = self
            .container_exists(params.instance.server_instance_id)
            .await?;
        self.remove_container(params.instance.server_instance_id)
            .await?;

        let storage_existed = self.instance_storage_exists().await?;
        self.remove_instance_storage().await?;
        Ok(ChangedResult {
            changed: container_existed || storage_existed,
        })
    }

    async fn instance_storage_exists(&self) -> Result<bool, ExecutorError> {
        for path in [
            self.data_directory(),
            self.config
                .state_directory
                .join(RESTORE_STAGING_DIRECTORY_NAME),
            self.config
                .state_directory
                .join(PREVIOUS_DATA_DIRECTORY_NAME),
            self.state_path(),
        ] {
            match fs::metadata(path).await {
                Ok(_) => return Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(false)
    }

    async fn remove_instance_storage(&self) -> Result<(), ExecutorError> {
        self.remove_paths([
            self.data_directory(),
            self.config
                .state_directory
                .join(RESTORE_STAGING_DIRECTORY_NAME),
            self.config
                .state_directory
                .join(PREVIOUS_DATA_DIRECTORY_NAME),
        ])
        .await?;

        match fs::remove_file(self.state_path()).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    async fn restore_snapshot(
        &self,
        repository: &str,
        snapshot_id: &str,
    ) -> Result<(), ExecutorError> {
        let staging = self
            .config
            .state_directory
            .join(RESTORE_STAGING_DIRECTORY_NAME);
        let previous = self
            .config
            .state_directory
            .join(PREVIOUS_DATA_DIRECTORY_NAME);
        let data = self.data_directory();

        // Recover an interrupted directory swap before discarding stale staging data.
        if !path_exists(&data).await? && path_exists(&previous).await? {
            self.move_path(&previous, &data).await?;
        }
        self.remove_paths([staging.clone(), previous.clone()])
            .await?;
        fs::create_dir_all(&staging).await?;
        let staging_arg = path_to_string(&staging)?;
        self.run_restic(
            repository,
            [
                "restore",
                snapshot_id,
                "--target",
                &staging_arg,
                "--json",
                "--quiet",
            ],
            None,
        )
        .await?;
        let restored_data = staging.join(DATA_DIRECTORY_NAME);
        let restored_metadata = fs::metadata(&restored_data).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ExecutorError::RestoreMissingDataDirectory(restored_data.clone())
            } else {
                ExecutorError::Io(error)
            }
        })?;
        if !restored_metadata.is_dir() {
            return Err(ExecutorError::RestoreMissingDataDirectory(restored_data));
        }

        if path_exists(&data).await? {
            self.move_path(&data, &previous).await?;
        }
        if let Err(error) = self
            .move_path(&restored_data, &data)
            .await
        {
            if path_exists(&previous).await.unwrap_or(false)
                && !path_exists(&data).await.unwrap_or(true)
            {
                let _ = self.move_path(&previous, &data).await;
            }
            return Err(error);
        }
        self.remove_paths([previous, staging])
            .await?;
        Ok(())
    }

    async fn ensure_restic_repository(&self, repository: &str) -> Result<(), ExecutorError> {
        let check = self
            .run_restic_allow_failure(repository, ["cat", "config"], None)
            .await?;
        if check.status.success() {
            return Ok(());
        }
        Err(command_failure("restic cat config", &check))
    }

    async fn create_container(
        &self,
        server_instance_id: Uuid,
        server_id: Uuid,
        process: &ProcessSpec,
    ) -> Result<(), ExecutorError> {
        if !process.accept_eula {
            return Err(ExecutorError::EulaNotAccepted);
        }
        let data = path_to_string(&self.data_directory())?;
        let volume = format!("{data}:/data:Z");
        let publish = format!("{}:25565/tcp", process.host_port);
        let name = container_name(server_instance_id);
        let mut command = Command::new(&self.config.podman_binary);
        command
            .arg("create")
            .arg("--replace")
            .arg("--name")
            .arg(&name)
            .arg("--label")
            .arg(format!("{MANAGED_LABEL}=true"))
            .arg("--label")
            .arg(format!("{LOCAL_SCOPE_LABEL}={}", self.config.local_scope))
            .arg("--label")
            .arg(format!("{SERVER_ID_LABEL}={server_id}"))
            .arg("--label")
            .arg(format!("{SERVER_INSTANCE_ID_LABEL}={server_instance_id}"))
            .arg("--label")
            .arg(format!(
                "{COMPUTE_INSTANCE_ID_LABEL}={}",
                self.config.compute_instance_id
            ))
            .arg("--publish")
            .arg(publish)
            .arg("--volume")
            .arg(volume)
            .arg("--env")
            .arg("EULA=TRUE")
            .arg("--env")
            .arg(format!("TYPE={}", process.server_type))
            .arg("--env")
            .arg(format!("VERSION={}", process.version))
            .arg("--env")
            .arg("SKIP_SERVER_PROPERTIES=TRUE")
            .arg("--restart")
            .arg("no");
        for (key, value) in &process.environment {
            command.arg("--env").arg(format!("{key}={value}"));
        }
        command.arg("--").arg(&process.container_image);
        self.run_command(command, "podman create").await?;
        Ok(())
    }

    async fn container_running(&self, server_instance_id: Uuid) -> Result<bool, ExecutorError> {
        if !self.container_exists(server_instance_id).await? {
            return Ok(false);
        }
        let name = container_name(server_instance_id);
        let output = self
            .run_podman(
                [
                    "container",
                    "inspect",
                    "--format",
                    "{{.State.Running}}",
                    &name,
                ],
                None,
            )
            .await?;
        let value = String::from_utf8(output.stdout)?;
        match value.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(ExecutorError::UnexpectedPodmanOutput(other.to_owned())),
        }
    }

    async fn container_exists(&self, server_instance_id: Uuid) -> Result<bool, ExecutorError> {
        let name = container_name(server_instance_id);
        let output = self
            .run_podman_allow_failure(["container", "exists", &name], None)
            .await?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(command_failure("podman container exists", &output)),
        }
    }

    async fn remove_container(&self, server_instance_id: Uuid) -> Result<(), ExecutorError> {
        let name = container_name(server_instance_id);
        self.run_podman(["rm", "--force", "--ignore", &name], None)
            .await?;
        Ok(())
    }

    async fn run_podman<const N: usize>(
        &self,
        arguments: [&str; N],
        current_directory: Option<&Path>,
    ) -> Result<Output, ExecutorError> {
        let output = self
            .run_podman_allow_failure(arguments, current_directory)
            .await?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(command_failure("podman", &output))
        }
    }

    async fn run_podman_allow_failure<const N: usize>(
        &self,
        arguments: [&str; N],
        current_directory: Option<&Path>,
    ) -> Result<Output, ExecutorError> {
        let mut command = Command::new(&self.config.podman_binary);
        command.args(arguments);
        if let Some(directory) = current_directory {
            command.current_dir(directory);
        }
        self.run_command_allow_failure(command, "podman").await
    }

    async fn run_restic<const N: usize>(
        &self,
        repository: &str,
        arguments: [&str; N],
        current_directory: Option<&Path>,
    ) -> Result<Output, ExecutorError> {
        let output = self
            .run_restic_allow_failure(repository, arguments, current_directory)
            .await?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(command_failure("restic", &output))
        }
    }

    async fn run_restic_allow_failure<const N: usize>(
        &self,
        repository: &str,
        arguments: [&str; N],
        current_directory: Option<&Path>,
    ) -> Result<Output, ExecutorError> {
        let retry_lock = format!("{}s", self.config.restic_retry_lock.as_secs());
        let (mut command, description) = match self.config.data_access_mode {
            DataAccessMode::PodmanUserNamespace => {
                let mut command = Command::new(&self.config.podman_binary);
                command.arg("unshare").arg(&self.config.restic_binary);
                (command, "restic in Podman user namespace")
            }
            DataAccessMode::Host => (Command::new(&self.config.restic_binary), "restic"),
        };
        command
            .env("RESTIC_REPOSITORY", repository)
            .arg("--retry-lock")
            .arg(retry_lock)
            .args(arguments);
        if let Some(directory) = current_directory {
            command.current_dir(directory);
        }
        self.run_command_allow_failure(command, description).await
    }

    async fn remove_paths<const N: usize>(
        &self,
        paths: [PathBuf; N],
    ) -> Result<(), ExecutorError> {
        match self.config.data_access_mode {
            DataAccessMode::PodmanUserNamespace => {
                let mut command = Command::new(&self.config.podman_binary);
                command.arg("unshare").arg("rm").arg("-rf").arg("--");
                command.args(paths);
                self.run_command(command, "podman unshare cleanup").await?;
            }
            DataAccessMode::Host => {
                for path in paths {
                    remove_path(&path).await?;
                }
            }
        }
        Ok(())
    }

    async fn move_path(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), ExecutorError> {
        match self.config.data_access_mode {
            DataAccessMode::PodmanUserNamespace => {
                let mut command = Command::new(&self.config.podman_binary);
                command
                    .arg("unshare")
                    .arg("mv")
                    .arg("--")
                    .arg(source)
                    .arg(destination);
                self.run_command(command, "podman unshare move").await?;
            }
            DataAccessMode::Host => fs::rename(source, destination).await?,
        }
        Ok(())
    }

    async fn run_command(
        &self,
        command: Command,
        description: &'static str,
    ) -> Result<Output, ExecutorError> {
        let output = self.run_command_allow_failure(command, description).await?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(command_failure(description, &output))
        }
    }

    async fn run_command_allow_failure(
        &self,
        mut command: Command,
        description: &'static str,
    ) -> Result<Output, ExecutorError> {
        debug!(%description, "executing node operation");
        command.kill_on_drop(true);
        tokio::time::timeout(self.config.command_timeout, command.output())
            .await
            .map_err(|_| ExecutorError::CommandTimeout(description))?
            .map_err(ExecutorError::Io)
    }

    async fn load_state(&self) -> Result<AgentState, ExecutorError> {
        let path = self.state_path();
        match fs::read(&path).await {
            Ok(value) => {
                let state = serde_json::from_slice::<AgentState>(&value)?;
                if state.schema_version != AGENT_STATE_SCHEMA_VERSION {
                    return Err(ExecutorError::UnsupportedStateSchemaVersion(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AgentState::default()),
            Err(error) => Err(error.into()),
        }
    }

    async fn store_state(&self, state: &AgentState) -> Result<(), ExecutorError> {
        ensure_private_directory(&self.config.state_directory).await?;
        let path = self.state_path();
        let temporary = self
            .config
            .state_directory
            .join(format!("{STATE_FILE_NAME}.tmp"));
        let encoded = serde_json::to_vec_pretty(state)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .await?;
        file.write_all(&encoded).await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&temporary, path).await?;
        let directory = fs::File::open(&self.config.state_directory).await?;
        directory.sync_all().await?;
        Ok(())
    }

    fn state_path(&self) -> PathBuf {
        self.config.state_directory.join(STATE_FILE_NAME)
    }

    fn data_directory(&self) -> PathBuf {
        self.config.state_directory.join(DATA_DIRECTORY_NAME)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentState {
    schema_version: u32,
    instance: Option<InstanceState>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            schema_version: AGENT_STATE_SCHEMA_VERSION,
            instance: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct InstanceState {
    server_instance_id: Uuid,
    fencing_token: u64,
    server_id: Uuid,
    repository: String,
    source_snapshot_id: Option<String>,
    data_prepared: bool,
    last_snapshot_id: Option<String>,
}

fn ensure_instance_state(
    state: &mut AgentState,
    identity: InstanceIdentity,
    server_id: Uuid,
    repository: &str,
    source_snapshot_id: Option<&str>,
) -> Result<(), ExecutorError> {
    if state.instance.is_none() {
        state.instance = Some(InstanceState {
            server_instance_id: identity.server_instance_id,
            fencing_token: identity.fencing_token,
            server_id,
            repository: repository.to_owned(),
            source_snapshot_id: source_snapshot_id.map(str::to_owned),
            data_prepared: false,
            last_snapshot_id: None,
        });
    }
    let instance = state
        .instance
        .as_mut()
        .ok_or(ExecutorError::UnknownInstance)?;
    validate_identity(instance, identity)?;
    if instance.server_id != server_id
        || instance.repository != repository
        || instance.source_snapshot_id.as_deref() != source_snapshot_id
    {
        return Err(ExecutorError::ImmutableInstanceConfigurationChanged);
    }
    Ok(())
}

fn validate_identity(
    state: &InstanceState,
    identity: InstanceIdentity,
) -> Result<(), ExecutorError> {
    if state.server_instance_id != identity.server_instance_id
        || state.fencing_token != identity.fencing_token
    {
        return Err(ExecutorError::StaleInstance);
    }
    Ok(())
}

fn container_name(server_instance_id: Uuid) -> String {
    format!("mcserver-{server_instance_id}")
}

fn path_to_string(path: &Path) -> Result<String, ExecutorError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ExecutorError::NonUtf8Path(path.to_path_buf()))
}

async fn remove_path(path: &Path) -> Result<(), ExecutorError> {
    let metadata = match fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path).await?;
    } else {
        fs::remove_file(path).await?;
    }
    Ok(())
}

async fn path_exists(path: &Path) -> Result<bool, ExecutorError> {
    match fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn ensure_private_directory(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path).await?;
    fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
    )
    .await
}

fn parse_backup_snapshot_id(output: &[u8]) -> Result<String, ExecutorError> {
    for line in output.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_slice(line)?;
        if value.get("message_type").and_then(Value::as_str) == Some("summary")
            && let Some(snapshot_id) = value.get("snapshot_id").and_then(Value::as_str)
            && !snapshot_id.trim().is_empty()
            && snapshot_id.chars().count() <= MAX_SNAPSHOT_ID_CHARS
            && !snapshot_id.contains('\0')
        {
            return Ok(snapshot_id.to_owned());
        }
    }
    Err(ExecutorError::MissingSnapshotId)
}

fn bounded_diagnostic(input: &[u8]) -> String {
    let truncated = input.len() > MAX_DIAGNOSTIC_BYTES;
    let input = &input[..input.len().min(MAX_DIAGNOSTIC_BYTES)];
    let mut diagnostic = String::from_utf8_lossy(input).trim().to_owned();
    if truncated {
        diagnostic.push_str(" …[truncated]");
    }
    diagnostic
}

fn command_failure(description: &'static str, output: &Output) -> ExecutorError {
    ExecutorError::CommandFailed {
        description,
        status: output.status.code(),
        stderr: bounded_diagnostic(&output.stderr),
    }
}

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("node operation I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("node state serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("node state schema version {0} is not supported")]
    UnsupportedStateSchemaVersion(u32),
    #[error("node command output was not UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("node command {0} timed out")]
    CommandTimeout(&'static str),
    #[error("node command {description} failed with status {status:?}: {stderr}")]
    CommandFailed {
        description: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("node-agent command refers to another or stale server instance")]
    StaleInstance,
    #[error("node-agent has not been assigned a server instance")]
    UnknownInstance,
    #[error("server data has not been prepared")]
    DataNotPrepared,
    #[error("Minecraft process is still running")]
    ProcessStillRunning,
    #[error("immutable server instance configuration changed")]
    ImmutableInstanceConfigurationChanged,
    #[error("Minecraft EULA acceptance was not provided")]
    EulaNotAccepted,
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[error("restic restore did not produce the expected data directory: {0}")]
    RestoreMissingDataDirectory(PathBuf),
    #[error("restic backup output did not contain a snapshot id")]
    MissingSnapshotId,
    #[error("unexpected Podman output: {0}")]
    UnexpectedPodmanOutput(String),
}

#[cfg(test)]
mod tests {
    use super::parse_backup_snapshot_id;

    #[test]
    fn parses_restic_summary_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
        let output = b"{\"message_type\":\"status\",\"percent_done\":1}\n{\"message_type\":\"summary\",\"snapshot_id\":\"abc123\"}\n";
        assert_eq!(parse_backup_snapshot_id(output)?, "abc123");
        Ok(())
    }
}
