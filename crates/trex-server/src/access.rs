use std::{sync::Arc, time::Duration};

use tokio::time::{MissedTickBehavior, interval, timeout};
use trex_sandbox::{AccessRequest, Sandbox, workspace_name};
use trex_store::sessions::LiveSandbox;

use crate::{
    api::{AppState, events::SessionEvent},
    runs::publish,
};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

// requests raised between runs, by processes still running in the background, which no run is
// watching: shown in the chat as they come, or approved in chats that approve automatically
pub async fn watch_between_runs(state: Arc<AppState>, idle: Duration) {
    let mut ticks = interval(POLL_INTERVAL);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let live = match state.store.live_sandboxes(idle).await {
            Ok(live) => live,
            Err(error) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "failed to list sandboxes to watch"
                );
                continue;
            }
        };
        for live in live {
            match timeout(CHECK_TIMEOUT, check(&state, &live)).await {
                Ok(Ok(())) => {}
                // the next tick checks again, so one failed check only delays the request
                Ok(Err(error)) => {
                    tracing::debug!(session = %live.session, error = format!("{error:#}"), "failed to check access requests")
                }
                Err(_) => {
                    tracing::debug!(session = %live.session, "timed out checking access requests")
                }
            }
        }
    }
}

async fn check(state: &AppState, live: &LiveSandbox) -> anyhow::Result<()> {
    let sandbox = Sandbox {
        workspace: workspace_name(live.workspace),
        name: live.sandbox.clone(),
    };
    for request in state.openshell.pending_access(&sandbox).await? {
        if !state
            .store
            .claim_access_request(live.session, &request.id)
            .await?
        {
            continue;
        }
        let id = request.id.clone();
        publish(state, live.session, requested(&request)).await;
        if !live.auto_approve {
            continue;
        }
        match state.openshell.approve_access(&sandbox, &request).await {
            Ok(()) => {
                let decided = SessionEvent::AccessDecided {
                    id,
                    approved: true,
                    automatic: true,
                };
                publish(state, live.session, decided).await;
            }
            Err(error) => {
                tracing::warn!(
                    session = %live.session,
                    request = id,
                    error = format!("{error:#}"),
                    "failed to approve access automatically, asking the user"
                );
            }
        }
    }
    Ok(())
}

fn requested(request: &AccessRequest) -> SessionEvent {
    SessionEvent::AccessRequested {
        id: request.id.clone(),
        endpoints: request.endpoints.clone(),
        binary: request.binary.clone(),
        rationale: request.rationale.clone(),
        security_notes: request.security_notes.clone(),
    }
}
