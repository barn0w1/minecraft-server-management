use std::{error::Error, time::Duration};

use mcserver_control_plane::{
    application::{ServerInstanceService, ServerService},
    config::Config,
    infrastructure::{ServerInstanceRepository, ServerRepository, connect_database},
    interface::{ClientRpcHandler, UnixSocketError, UnixSocketServer},
    reconciliation::{ReconcileError, ReconcileWorker},
    shutdown::{ShutdownSignal, ShutdownSignals},
};
use thiserror::Error;
use tokio::{
    sync::watch,
    task::{JoinError, JoinHandle},
    time::timeout,
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("mcserver_control_plane=info,mcserver_protocol=info")
        }))
        .init();

    let config = Config::from_env()?;
    run(config).await?;
    Ok(())
}

async fn run(config: Config) -> Result<(), ControlPlaneError> {
    let pool = connect_database(&config.database_url).await?;
    let server_repository = ServerRepository::new(pool.clone());
    let server_instance_repository = ServerInstanceRepository::new(pool.clone());
    let (reconcile_scheduler, reconcile_worker) = ReconcileWorker::channel(
        server_repository.clone(),
        server_instance_repository.clone(),
        config.reconcile_interval,
    );
    let server_service = ServerService::new(server_repository, reconcile_scheduler);
    let server_instance_service = ServerInstanceService::new(server_instance_repository);
    let rpc_handler = ClientRpcHandler::new(server_service, server_instance_service);
    let socket_server = UnixSocketServer::bind(
        config.socket_path,
        config.socket_mode,
        config.max_frame_bytes,
        rpc_handler,
    )
    .await?;

    let mut shutdown_signals = ShutdownSignals::new()?;
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut reconcile_task = tokio::spawn(reconcile_worker.run(shutdown_receiver.clone()));
    let mut socket_task = tokio::spawn(socket_server.run(shutdown_receiver));

    info!("mcserver-control-plane started");
    let first_exit = tokio::select! {
        signal = shutdown_signals.recv() => FirstExit::Signal(signal),
        result = &mut socket_task => FirstExit::Socket(result),
        result = &mut reconcile_task => FirstExit::Reconcile(result),
    };

    let (first_service, mut socket_result, mut reconcile_result) = match first_exit {
        FirstExit::Signal(signal) => {
            info!(signal = signal.as_str(), "shutdown signal received");
            (None, None, None)
        }
        FirstExit::Socket(result) => {
            error!("client JSON-RPC socket service exited before shutdown");
            (Some(ServiceName::ClientSocket), Some(result), None)
        }
        FirstExit::Reconcile(result) => {
            error!("reconciliation service exited before shutdown");
            (Some(ServiceName::Reconciler), None, Some(result))
        }
    };

    if shutdown_sender.send(true).is_err() {
        warn!("all shutdown receivers had already exited");
    }

    let wait_result = timeout(
        config.shutdown_timeout,
        collect_task_results(
            &mut socket_task,
            &mut reconcile_task,
            &mut socket_result,
            &mut reconcile_result,
        ),
    )
    .await;

    if wait_result.is_err() {
        if socket_result.is_none() {
            socket_task.abort();
        }
        if reconcile_result.is_none() {
            reconcile_task.abort();
        }
        if socket_result.is_none() {
            let _ = socket_task.await;
        }
        if reconcile_result.is_none() {
            let _ = reconcile_task.await;
        }
        pool.close().await;
        return Err(ControlPlaneError::ShutdownTimeout {
            timeout: config.shutdown_timeout,
        });
    }

    pool.close().await;
    let socket_result = socket_result.ok_or(ControlPlaneError::MissingTaskResult(
        ServiceName::ClientSocket,
    ))?;
    let reconcile_result = reconcile_result.ok_or(ControlPlaneError::MissingTaskResult(
        ServiceName::Reconciler,
    ))?;
    if first_service == Some(ServiceName::ClientSocket) {
        return match socket_result {
            Ok(Ok(())) => Err(ControlPlaneError::UnexpectedServiceExit(
                ServiceName::ClientSocket,
            )),
            Ok(Err(error)) => Err(ControlPlaneError::Socket(error)),
            Err(error) => Err(ControlPlaneError::TaskJoin(error)),
        };
    }
    if first_service == Some(ServiceName::Reconciler) {
        return match reconcile_result {
            Ok(Ok(())) => Err(ControlPlaneError::UnexpectedServiceExit(
                ServiceName::Reconciler,
            )),
            Ok(Err(error)) => Err(ControlPlaneError::Reconcile(error)),
            Err(error) => Err(ControlPlaneError::TaskJoin(error)),
        };
    }

    socket_result.map_err(ControlPlaneError::TaskJoin)??;
    reconcile_result.map_err(ControlPlaneError::TaskJoin)??;
    info!("mcserver-control-plane stopped");
    Ok(())
}

async fn collect_task_results(
    socket_task: &mut JoinHandle<Result<(), UnixSocketError>>,
    reconcile_task: &mut JoinHandle<Result<(), ReconcileError>>,
    socket_result: &mut Option<Result<Result<(), UnixSocketError>, JoinError>>,
    reconcile_result: &mut Option<Result<Result<(), ReconcileError>, JoinError>>,
) {
    if socket_result.is_none() {
        *socket_result = Some((&mut *socket_task).await);
    }
    if reconcile_result.is_none() {
        *reconcile_result = Some((&mut *reconcile_task).await);
    }
}

#[derive(Debug)]
enum FirstExit {
    Signal(ShutdownSignal),
    Socket(Result<Result<(), UnixSocketError>, JoinError>),
    Reconcile(Result<Result<(), ReconcileError>, JoinError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceName {
    ClientSocket,
    Reconciler,
}

impl std::fmt::Display for ServiceName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientSocket => formatter.write_str("client JSON-RPC socket"),
            Self::Reconciler => formatter.write_str("reconciler"),
        }
    }
}

#[derive(Debug, Error)]
enum ControlPlaneError {
    #[error("database operation failed")]
    Repository(#[from] mcserver_control_plane::infrastructure::RepositoryError),
    #[error("client JSON-RPC socket failed")]
    Socket(#[from] UnixSocketError),
    #[error("reconciliation failed")]
    Reconcile(#[from] ReconcileError),
    #[error("shutdown signal registration failed")]
    Signal(#[from] std::io::Error),
    #[error("service task failed")]
    TaskJoin(#[source] JoinError),
    #[error("{0} exited unexpectedly")]
    UnexpectedServiceExit(ServiceName),
    #[error("missing completion result for {0}")]
    MissingTaskResult(ServiceName),
    #[error("graceful shutdown exceeded {timeout:?}")]
    ShutdownTimeout { timeout: Duration },
}
