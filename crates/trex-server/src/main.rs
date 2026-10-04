mod api;
mod config;
mod logging;
mod runs;

use std::sync::Arc;

use tokio::{net::TcpListener, signal};
use trex_harness::tool::Tools;
use trex_sandbox::OpenShell;
use trex_store::Store;

use crate::{api::AppState, config::Config, runs::Runs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    logging::init(config.log_format);

    let openshell =
        OpenShell::connect(&config.openshell_endpoint, &config.openshell_tls_dir).await?;
    tracing::info!(
        endpoint = config.openshell_endpoint,
        version = openshell.version().await?,
        "connected to openshell gateway"
    );

    let store = Store::connect(&config.database_url, &config.redis_url).await?;
    tracing::info!("connected to postgres and redis");
    let interrupted = store.fail_interrupted_runs().await?;
    if interrupted > 0 {
        tracing::warn!(
            sessions = interrupted,
            "marked runs interrupted by a restart as failed"
        );
    }

    let models = config.models;
    tracing::info!(models = ?models.ids().collect::<Vec<_>>(), "loaded models");

    let state = Arc::new(AppState {
        store,
        openshell,
        models,
        tools: Tools::standard()?,
        library: config.library,
        sandbox_image: config.sandbox_image,
        sandbox_policy: config.sandbox_policy,
        runs: Runs::default(),
    });

    let listener = TcpListener::bind(config.addr).await?;
    tracing::info!(addr = %listener.local_addr()?, version = env!("CARGO_PKG_VERSION"), "listening");

    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;

    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to listen for ctrl-c");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to listen for sigterm")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutting down");
}
