mod cancellation;
mod config;
mod executor;
mod transport;

use std::{collections::BTreeMap, env, error::Error, os::unix::fs::PermissionsExt, sync::Arc};

use cancellation::CancellationToken;
use config::Config;
use executor::AgentExecutor;
use tokio::{signal::unix::{SignalKind, signal}, sync::RwLock};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => {}
        [argument] if argument == "--version" || argument == "-V" => {
            println!("mcserver-node-agent {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [argument] if argument == "--help" || argument == "-h" => {
            println!(
                "mcserver-node-agent {}\n\nUSAGE:\n    mcserver-node-agent [--version]",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid command line: {arguments:?}; use --help for usage"),
            )
            .into());
        }
    }

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
    let runtime_environment = Arc::new(RwLock::new(BTreeMap::new()));
    let executor = AgentExecutor::new(config.clone(), Arc::clone(&runtime_environment));
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
    let result = transport::run(config, executor, runtime_environment, cancellation.clone()).await;
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
