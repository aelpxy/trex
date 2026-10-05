use std::{sync::Arc, time::Duration};

use tokio::time::{interval, timeout};
use trex_sandbox::{Sandbox, workspace_name};

use crate::api::AppState;

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

// stops the sandboxes of sessions nobody has used for `idle`; the next tool call starts them again
pub async fn stop_idle_sandboxes(state: Arc<AppState>, idle: Duration) {
    let mut ticks = interval(CHECK_INTERVAL);
    loop {
        ticks.tick().await;
        if let Err(error) = stop_batch(&state, idle).await {
            tracing::warn!(
                error = format!("{error:#}"),
                "failed to stop idle sandboxes"
            );
        }
    }
}

async fn stop_batch(state: &AppState, idle: Duration) -> anyhow::Result<()> {
    while let Some(session) = state.store.next_idle_sandbox(idle).await? {
        let sandbox = Sandbox {
            workspace: workspace_name(session.workspace),
            name: session.sandbox.clone(),
        };
        // marked stopped even when stopping failed, so a broken sandbox isn't retried forever;
        // starting checks the real state anyway
        match timeout(STOP_TIMEOUT, state.openshell.stop(&sandbox)).await {
            Ok(Ok(())) => {
                tracing::info!(session = %session.session, sandbox = sandbox.name, "stopped idle sandbox")
            }
            Ok(Err(error)) => {
                tracing::warn!(session = %session.session, error = format!("{error:#}"), "failed to stop idle sandbox")
            }
            Err(_) => {
                tracing::warn!(session = %session.session, "timed out stopping idle sandbox")
            }
        }
        session.mark_stopped().await?;
    }
    Ok(())
}
