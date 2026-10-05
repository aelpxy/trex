use std::time::Duration;

use tokio::time::{Instant, sleep};
use trex_store::admin::AdminChange;
use uuid::Uuid;

use crate::{
    api::{AppState, error::ApiError},
    runs::{self, RUN_STALE_AFTER},
};

// runs see a cancel at their next heartbeat (every 10s), so a couple of them is enough
const STOP_WAIT: Duration = Duration::from_secs(25);
const STOP_POLL: Duration = Duration::from_millis(500);

// stops every run in `workspaces`, wherever it runs, and waits until none is active; false if
// some are still going after `STOP_WAIT`
async fn stop_runs(state: &AppState, workspaces: &[Uuid]) -> Result<bool, ApiError> {
    for &workspace in workspaces {
        for session in state.store.workspace_session_ids(workspace).await? {
            runs::cancel(state, Some(workspace), session).await?;
        }
    }
    let deadline = Instant::now() + STOP_WAIT;
    while state.store.active_runs(workspaces, RUN_STALE_AFTER).await? > 0 {
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(STOP_POLL).await;
    }
    Ok(true)
}

// deletes a user with everything in the workspaces only they belong to: chats, sandboxes, files,
// projects and credits. the outside cleanup runs first, so a failure leaves the user to retry on
pub async fn delete(state: &AppState, user: Uuid) -> Result<bool, ApiError> {
    if state.store.user(user).await?.is_none() {
        return Ok(false);
    }
    state.store.delete_user_sessions(user, None).await?;
    let workspaces = state.store.sole_workspaces(user).await?;
    // a run still working would recreate the sandboxes and files deleted below
    if !stop_runs(state, &workspaces).await? {
        return Err(ApiError::Conflict(
            "their chats are still stopping; try again in a minute".into(),
        ));
    }
    for &workspace in &workspaces {
        for session in state.store.workspace_session_ids(workspace).await? {
            state.store.delete_events(session).await?;
        }
        state.openshell.delete_workspace(workspace).await?;
        state.library.delete_workspace(workspace).await?;
    }
    Ok(state.store.delete_user(user, &workspaces).await?)
}

// suspending signs them out and stops what runs in the workspaces only they belong to; their
// scheduled tasks wait in the store until they're unsuspended
pub async fn set_suspended(
    state: &AppState,
    user: Uuid,
    suspended: bool,
) -> Result<AdminChange, ApiError> {
    let change = state.store.set_user_suspended(user, suspended).await?;
    if change != AdminChange::Changed {
        return Ok(change);
    }
    if suspended {
        state.store.delete_user_sessions(user, None).await?;
        for workspace in state.store.sole_workspaces(user).await? {
            for session in state.store.workspace_session_ids(workspace).await? {
                runs::cancel(state, Some(workspace), session).await?;
            }
        }
    }
    Ok(AdminChange::Changed)
}

// a new password from an admin, for users locked out; it signs them out everywhere
pub async fn set_password(state: &AppState, user: Uuid, hash: &str) -> Result<bool, ApiError> {
    if state.store.user(user).await?.is_none() {
        return Ok(false);
    }
    state.store.update_user(user, None, Some(hash)).await?;
    state.store.delete_user_sessions(user, None).await?;
    Ok(true)
}
