use std::{env, error::Error, fmt::Display, time::Duration};

use mcserver_control_plane::{
    agent::{AgentRegistry, AgentServer, AgentServerError, TlsAgentServer, TlsAgentServerOptions},
    application::{ServerInstanceService, ServerService, ServerStatusService},
    config::Config,
    infrastructure::{
        AgentCertificateAuthority, AgentCertificateError, AkamaiComputeManager, ComputeError,
        ComputeInstanceRepository, ComputeManager, LocalComputeManager, R2TemporaryCredentialError,
        R2TemporaryCredentialManager, ServerInstanceRepository, ServerRepository,
        SnapshotRepository, connect_database,
    },
    interface::{ClientRpcHandler, UnixSocketError, UnixSocketServer},
    reconciliation::{ReconcileFatalError, ReconcileWorker},
    shutdown::{CancellationToken, ShutdownSignals},
};
use thiserror::Error;
use tokio::{task::JoinSet, time::timeout};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let command = Command::parse(env::args().skip(1))?;
    match command {
        Command::Version => {
            println!("mcserver-control-plane {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::Help => {
            print_help();
            return Ok(());
        }
        Command::Run | Command::Preflight => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(
                "mcserver_control_plane=info,mcserver_node_agent=info,mcserver_protocol=info",
            )
        }))
        .init();

    let config = Config::from_env()?;
    match command {
        Command::Run => run(config).await?,
        Command::Preflight => preflight(config).await?,
        Command::Version | Command::Help => return Ok(()),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Run,
    Preflight,
    Version,
    Help,
}

impl Command {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, CommandError> {
        let arguments = arguments.collect::<Vec<_>>();
        match arguments.as_slice() {
            [] => Ok(Self::Run),
            [argument] if argument == "run" => Ok(Self::Run),
            [argument] if argument == "--preflight" || argument == "preflight" => {
                Ok(Self::Preflight)
            }
            [argument] if argument == "--version" || argument == "-V" => Ok(Self::Version),
            [argument] if argument == "--help" || argument == "-h" => Ok(Self::Help),
            _ => Err(CommandError(arguments)),
        }
    }
}

fn print_help() {
    println!(
        "mcserver-control-plane {}\n\nUSAGE:\n    mcserver-control-plane [run | --preflight | --version]",
        env!("CARGO_PKG_VERSION")
    );
}

#[derive(Debug, Error)]
#[error("invalid command line: {0:?}; use --help for usage")]
struct CommandError(Vec<String>);

async fn preflight(config: Config) -> Result<(), ControlPlaneError> {
    let pool = connect_database(&config.database_url).await?;
    let compute_repository = ComputeInstanceRepository::new(pool.clone());

    if let Some(remote) = config.remote_agent.as_ref() {
        AgentCertificateAuthority::new(remote).preflight().await?;
    }
    let r2_credentials = config
        .r2
        .clone()
        .map(R2TemporaryCredentialManager::new)
        .transpose()?;
    if let Some(manager) = r2_credentials.as_ref() {
        manager.preflight().await?;
    }

    let managed_instances = match (config.akamai.as_ref(), config.remote_agent.as_ref()) {
        (Some(akamai), Some(remote)) => {
            let manager = AkamaiComputeManager::new(
                compute_repository,
                AgentRegistry::default(),
                akamai.clone(),
                remote.clone(),
                config.agent_command_timeout,
            )
            .map_err(ComputeError::from)?;
            Some(
                manager
                    .preflight()
                    .await
                    .map_err(ComputeError::from)?
                    .managed_instances,
            )
        }
        (None, _) => None,
        (Some(_), None) => {
            return Err(ControlPlaneError::InvalidConfiguration(
                "Akamai provider requires remote TLS agent configuration".to_owned(),
            ));
        }
    };

    pool.close().await;
    match managed_instances {
        Some(count) => println!(
            "preflight ok: database, remote agent PKI, R2 temporary credentials, and Akamai provider; managed_instances={count}"
        ),
        None if config.remote_agent.is_some() => {
            println!("preflight ok: database and remote agent PKI")
        }
        None => println!("preflight ok: database and local compute configuration"),
    }
    Ok(())
}

async fn run(config: Config) -> Result<(), ControlPlaneError> {
    let pool = connect_database(&config.database_url).await?;
    let server_repository = ServerRepository::new(pool.clone());
    let instance_repository = ServerInstanceRepository::new(pool.clone());
    let compute_repository = ComputeInstanceRepository::new(pool.clone());
    let snapshot_repository = SnapshotRepository::new(pool.clone());
    let agents = AgentRegistry::default();
    let remote_certificate_authority = config
        .remote_agent
        .as_ref()
        .map(AgentCertificateAuthority::new);
    if let Some(authority) = remote_certificate_authority.as_ref() {
        authority.preflight().await?;
    }
    let r2_credentials = config
        .r2
        .clone()
        .map(R2TemporaryCredentialManager::new)
        .transpose()?;
    if let Some(manager) = r2_credentials.as_ref() {
        manager.preflight().await?;
        info!("Cloudflare R2 temporary credential preflight succeeded");
    }

    let local_compute = LocalComputeManager::new(
        compute_repository.clone(),
        agents.clone(),
        config.node_agent_binary.clone(),
        config.node_agent_root.clone(),
        config.podman_binary.clone(),
        config.local_scope.clone(),
        config.agent_listen_address.to_string(),
        config.agent_command_timeout,
        config.max_frame_bytes,
        config.local_control_timeout,
        config.local_process_stop_timeout,
    );
    if config.reap_orphans_on_start {
        let active_compute_ownership = compute_repository.list_active_local_ownership().await?;
        let summary = local_compute
            .reap_orphans(&active_compute_ownership)
            .await
            .map_err(ComputeError::from)?;
        if summary.containers_removed > 0
            || summary.processes_stopped > 0
            || summary.state_directories_removed > 0
        {
            info!(
                containers_removed = summary.containers_removed,
                processes_stopped = summary.processes_stopped,
                state_directories_removed = summary.state_directories_removed,
                "reaped orphaned local runtime resources"
            );
        }
    }

    let akamai_compute = match (config.akamai.clone(), config.remote_agent.clone()) {
        (Some(akamai), Some(remote)) => {
            let reap_orphans = akamai.reap_orphans_on_start;
            let manager = AkamaiComputeManager::new(
                compute_repository.clone(),
                agents.clone(),
                akamai,
                remote,
                config.agent_command_timeout,
            )
            .map_err(ComputeError::from)?;
            let preflight = manager.preflight().await.map_err(ComputeError::from)?;
            info!(
                managed_instances = preflight.managed_instances,
                "Akamai provider preflight succeeded"
            );
            if reap_orphans {
                let active = compute_repository.list_active_akamai().await?;
                let summary = manager
                    .reap_orphans(&active)
                    .await
                    .map_err(ComputeError::from)?;
                if summary.instances_adopted > 0 || summary.instances_deleted > 0 {
                    info!(
                        instances_adopted = summary.instances_adopted,
                        instances_deleted = summary.instances_deleted,
                        "reconciled managed Akamai instances during startup"
                    );
                }
            }
            Some(manager)
        }
        (None, _) => None,
        (Some(_), None) => {
            return Err(ControlPlaneError::InvalidConfiguration(
                "Akamai provider requires remote TLS agent configuration".to_owned(),
            ));
        }
    };
    let compute_manager = ComputeManager::new(local_compute, akamai_compute);

    let (reconcile_scheduler, reconcile_worker) = ReconcileWorker::channel(
        server_repository.clone(),
        instance_repository.clone(),
        compute_repository.clone(),
        snapshot_repository,
        compute_manager,
        agents.clone(),
        config.reconcile_interval,
        config.reconcile_retry,
        config.agent_command_timeout,
    );

    let server_status_service = ServerStatusService::new(
        server_repository.clone(),
        instance_repository.clone(),
        compute_repository.clone(),
        agents.clone(),
    );
    let server_service = ServerService::new(server_repository, reconcile_scheduler.clone());
    let server_instance_service = ServerInstanceService::new(instance_repository.clone());
    let rpc_handler = ClientRpcHandler::new(
        server_service,
        server_instance_service,
        server_status_service,
    );

    let agent_server = AgentServer::bind(
        config.agent_listen_address,
        agents.clone(),
        compute_repository.clone(),
        reconcile_scheduler.clone(),
        config.max_frame_bytes,
        config.agent_command_timeout,
    )
    .await?;
    let remote_agent_server = match (config.remote_agent.as_ref(), r2_credentials) {
        (Some(remote), Some(r2_credentials)) => Some(
            TlsAgentServer::bind(TlsAgentServerOptions {
                address: remote.listen_address,
                certificate_path: remote.tls_certificate.clone(),
                private_key_path: remote.tls_private_key.clone(),
                client_ca_certificate_path: remote.client_ca_certificate.clone(),
                certificate_authority: remote_certificate_authority.clone().ok_or_else(|| {
                    ControlPlaneError::InvalidConfiguration(
                        "remote agent certificate authority is unavailable".to_owned(),
                    )
                })?,
                r2_credentials,
                registry: agents.clone(),
                compute_repository: compute_repository.clone(),
                server_instance_repository: instance_repository.clone(),
                reconcile_scheduler: reconcile_scheduler.clone(),
                max_frame_bytes: config.max_frame_bytes,
                command_timeout: config.agent_command_timeout,
            })
            .await?,
        ),
        (None, None) => None,
        (Some(_), None) => {
            return Err(ControlPlaneError::InvalidConfiguration(
                "remote Akamai agent listener requires R2 temporary credentials".to_owned(),
            ));
        }
        (None, Some(_)) => {
            return Err(ControlPlaneError::InvalidConfiguration(
                "R2 temporary credentials require the remote Akamai agent listener".to_owned(),
            ));
        }
    };
    let socket_server = UnixSocketServer::bind(
        config.socket_path,
        config.socket_mode,
        config.max_frame_bytes,
        rpc_handler,
    )
    .await?;

    let mut shutdown_signals = ShutdownSignals::new()?;
    let cancellation = CancellationToken::new();
    let mut services = JoinSet::new();
    services.spawn({
        let cancellation = cancellation.child_token();
        async move { ServiceExit::ClientSocket(socket_server.run(cancellation).await) }
    });
    services.spawn({
        let cancellation = cancellation.child_token();
        async move { ServiceExit::AgentServer(agent_server.run(cancellation).await) }
    });
    if let Some(remote_agent_server) = remote_agent_server {
        services.spawn({
            let cancellation = cancellation.child_token();
            async move {
                ServiceExit::RemoteAgentServer(remote_agent_server.run(cancellation).await)
            }
        });
    }
    services.spawn({
        let cancellation = cancellation.child_token();
        async move { ServiceExit::Reconciler(reconcile_worker.run(cancellation).await) }
    });

    info!("mcserver-control-plane started");
    let first_error = tokio::select! {
        signal = shutdown_signals.recv() => {
            info!(signal = signal.as_str(), "shutdown signal received");
            None
        }
        completed = services.join_next() => {
            Some(unexpected_service_result(completed))
        }
    };

    cancellation.cancel();
    let drain_result = timeout(config.shutdown_timeout, drain_services(&mut services)).await;
    let shutdown_error = match drain_result {
        Ok(result) => result.err(),
        Err(_) => {
            warn!(timeout = ?config.shutdown_timeout, "graceful shutdown timed out; aborting remaining tasks");
            services.abort_all();
            while services.join_next().await.is_some() {}
            Some(ControlPlaneError::ShutdownTimeout {
                timeout: config.shutdown_timeout,
            })
        }
    };

    pool.close().await;
    if let Some(error) = first_error {
        return Err(error);
    }
    if let Some(error) = shutdown_error {
        return Err(error);
    }

    info!("mcserver-control-plane stopped");
    Ok(())
}

fn unexpected_service_result(
    completed: Option<Result<ServiceExit, tokio::task::JoinError>>,
) -> ControlPlaneError {
    match completed {
        Some(Ok(exit)) => exit
            .into_error(true)
            .unwrap_or_else(|| ControlPlaneError::UnexpectedServiceExit(ServiceName::Unknown)),
        Some(Err(error)) => ControlPlaneError::TaskJoin(error),
        None => ControlPlaneError::UnexpectedServiceExit(ServiceName::Unknown),
    }
}

async fn drain_services(services: &mut JoinSet<ServiceExit>) -> Result<(), ControlPlaneError> {
    let mut first_error = None;
    while let Some(completed) = services.join_next().await {
        let result = match completed {
            Ok(exit) => exit.into_error(false),
            Err(error) if error.is_cancelled() => None,
            Err(error) => Some(ControlPlaneError::TaskJoin(error)),
        };
        if first_error.is_none() {
            first_error = result;
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[derive(Debug)]
enum ServiceExit {
    ClientSocket(Result<(), UnixSocketError>),
    AgentServer(Result<(), AgentServerError>),
    RemoteAgentServer(Result<(), AgentServerError>),
    Reconciler(Result<(), ReconcileFatalError>),
}

impl ServiceExit {
    fn into_error(self, unexpected: bool) -> Option<ControlPlaneError> {
        match self {
            Self::ClientSocket(Ok(())) if unexpected => Some(
                ControlPlaneError::UnexpectedServiceExit(ServiceName::ClientSocket),
            ),
            Self::AgentServer(Ok(())) if unexpected => Some(
                ControlPlaneError::UnexpectedServiceExit(ServiceName::AgentServer),
            ),
            Self::RemoteAgentServer(Ok(())) if unexpected => Some(
                ControlPlaneError::UnexpectedServiceExit(ServiceName::RemoteAgentServer),
            ),
            Self::Reconciler(Ok(())) if unexpected => Some(
                ControlPlaneError::UnexpectedServiceExit(ServiceName::Reconciler),
            ),
            Self::ClientSocket(Err(error)) => Some(ControlPlaneError::Socket(error)),
            Self::AgentServer(Err(error)) | Self::RemoteAgentServer(Err(error)) => {
                Some(ControlPlaneError::AgentServer(error))
            }
            Self::Reconciler(Err(error)) => Some(ControlPlaneError::Reconcile(error)),
            Self::ClientSocket(Ok(()))
            | Self::AgentServer(Ok(()))
            | Self::RemoteAgentServer(Ok(()))
            | Self::Reconciler(Ok(())) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceName {
    ClientSocket,
    AgentServer,
    RemoteAgentServer,
    Reconciler,
    Unknown,
}

impl Display for ServiceName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientSocket => formatter.write_str("client JSON-RPC socket"),
            Self::AgentServer => formatter.write_str("local node-agent JSON-RPC listener"),
            Self::RemoteAgentServer => formatter.write_str("remote TLS node-agent listener"),
            Self::Reconciler => formatter.write_str("reconciler"),
            Self::Unknown => formatter.write_str("service supervisor"),
        }
    }
}

#[derive(Debug, Error)]
enum ControlPlaneError {
    #[error("database operation failed")]
    Repository(#[from] mcserver_control_plane::infrastructure::RepositoryError),
    #[error("compute operation failed: {0}")]
    Compute(#[from] ComputeError),
    #[error("agent certificate operation failed: {0}")]
    Certificate(#[from] AgentCertificateError),
    #[error("R2 temporary credential operation failed: {0}")]
    R2TemporaryCredential(#[from] R2TemporaryCredentialError),
    #[error("control-plane configuration is inconsistent: {0}")]
    InvalidConfiguration(String),
    #[error("client JSON-RPC socket failed")]
    Socket(#[from] UnixSocketError),
    #[error("node-agent JSON-RPC listener failed")]
    AgentServer(#[from] AgentServerError),
    #[error("reconciliation failed")]
    Reconcile(#[from] ReconcileFatalError),
    #[error("shutdown signal registration failed")]
    Signal(#[from] std::io::Error),
    #[error("service task failed")]
    TaskJoin(#[source] tokio::task::JoinError),
    #[error("{0} exited unexpectedly")]
    UnexpectedServiceExit(ServiceName),
    #[error("graceful shutdown exceeded {timeout:?}")]
    ShutdownTimeout { timeout: Duration },
}
