mod api;
mod config;
mod logging;
#[allow(dead_code)]
mod openshell;

use tokio::{net::TcpListener, signal};

use crate::{config::Config, openshell::OpenShell};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    logging::init(config.log_format);

    let openshell =
        OpenShell::connect(&config.openshell_endpoint, &config.openshell_tls_dir).await?;
    tracing::info!(
        endpoint = config.openshell_endpoint,
        version = openshell.version().await?,
        "connected to openshell gateway"
    );

    let listener = TcpListener::bind(config.addr).await?;
    tracing::info!(addr = %listener.local_addr()?, version = env!("CARGO_PKG_VERSION"), "listening");

    axum::serve(listener, api::router())
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
