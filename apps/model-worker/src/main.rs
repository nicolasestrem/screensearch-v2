//! Supervised model-worker process entry point.

use std::time::Duration;

use anyhow::Context;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "screensearch_model_worker=info".into()),
        )
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize model-worker tracing: {error}"))?;

    info!(
        status = "reserved-boundary",
        "model worker has no production inference assignment; adapters currently run in the daemon"
    );
    tokio::signal::ctrl_c()
        .await
        .context("wait for shutdown signal")?;
    tokio::time::sleep(Duration::from_millis(10)).await;
    info!("model worker stopped");
    Ok(())
}
