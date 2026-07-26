use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("mcserver-node-agent started; transport is not implemented yet");
    tokio::signal::ctrl_c().await?;
    info!("mcserver-node-agent stopped");

    Ok(())
}
