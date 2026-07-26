use std::error::Error;

use mcserver_control_plane::{
    application::ServerService,
    config::Config,
    infrastructure::{ServerRepository, connect_database},
    interface::{ClientRpcHandler, UnixSocketServer},
    reconciliation::{ReconcileWorker, log_worker_failure},
};
use tokio::sync::watch;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("mcserver_control_plane=info,mcserver_protocol=info")
        }))
        .init();

    let config = Config::from_env()?;
    let pool = connect_database(&config.database_url).await?;
    let repository = ServerRepository::new(pool);
    let (reconcile_scheduler, reconcile_worker) =
        ReconcileWorker::channel(repository.clone(), config.reconcile_interval);
    let server_service = ServerService::new(repository, reconcile_scheduler);
    let rpc_handler = ClientRpcHandler::new(server_service);
    let socket_server = UnixSocketServer::bind(
        config.socket_path,
        config.socket_mode,
        config.max_frame_bytes,
        rpc_handler,
    )
    .await?;

    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let reconcile_shutdown = shutdown_receiver.clone();

    let reconcile_task = tokio::spawn(async move {
        let result = reconcile_worker.run(reconcile_shutdown).await;
        if let Err(error) = &result {
            log_worker_failure(error);
        }
        result
    });
    let socket_task = tokio::spawn(socket_server.run(shutdown_receiver));

    info!("mcserver-control-plane started");
    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");

    if shutdown_sender.send(true).is_err() {
        error!("shutdown receivers ended before shutdown signal was sent");
    }

    socket_task.await??;
    reconcile_task.await??;
    info!("mcserver-control-plane stopped");

    Ok(())
}
