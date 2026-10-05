mod api;
mod config;
mod credits;
mod idle;
mod logging;
mod preview;
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

    let models = config.models;
    tracing::info!(models = ?models.ids().collect::<Vec<_>>(), "loaded models");
    if config.plans.enforced() {
        tracing::info!(plans = ?config.plans.ids().collect::<Vec<_>>(), "enforcing credits");
    } else {
        tracing::warn!("no plans configured, so credits are tracked but not enforced");
    }

    let state = Arc::new(AppState {
        store,
        openshell,
        models,
        plans: config.plans,
        admin_token: config.admin_token,
        tools: Tools::standard()?,
        library: config.library,
        sandbox_image: config.sandbox_image,
        sandbox_policy: config.sandbox_policy,
        runs: Runs::default(),
        preview_url: config.preview_url,
    });
    tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(error) = preview::serve(state, config.preview_addr).await {
                tracing::error!(error = format!("{error:#}"), "preview server stopped");
            }
        }
    });
    tokio::spawn(runs::resume_stale_runs(state.clone()));
    tokio::spawn(idle::stop_idle_sandboxes(
        state.clone(),
        config.sandbox_idle_timeout,
    ));

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
