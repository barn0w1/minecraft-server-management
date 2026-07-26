mod cancellation;
mod config;
mod executor;
mod transport;

use std::{error::Error, os::unix::fs::PermissionsExt};

use cancellation::CancellationToken;
use config::Config;
use executor::AgentExecutor;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("mcserver_node_agent=info")),
        )
        .init();

    let config = Config::from_env()?;
    tokio::fs::create_dir_all(&config.state_directory).await?;
    tokio::fs::set_permissions(
        &config.state_directory,
        std::fs::Permissions::from_mode(0o700),
    )
    .await?;
    let executor = AgentExecutor::new(config.clone());
    let cancellation = CancellationToken::new();
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let signal_cancellation = cancellation.clone();
    let signal_task = tokio::spawn(async move {
        tokio::select! {
            () = signal_cancellation.cancelled() => return Ok::<(), std::io::Error>(()),
            _ = interrupt.recv() => info!("SIGINT received"),
            _ = terminate.recv() => info!("SIGTERM received"),
        }
        signal_cancellation.cancel();
        Ok::<(), std::io::Error>(())
    });

    info!(
        compute_instance_id = %config.compute_instance_id,
        "mcserver-node-agent started"
    );
    let result = transport::run(config, executor, cancellation.clone()).await;
    cancellation.cancel();
    match signal_task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warn!(%error, "signal listener failed"),
        Err(error) => warn!(%error, "signal listener task failed"),
    }
    result?;
    info!("mcserver-node-agent stopped");
    Ok(())
}
