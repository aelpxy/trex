use uuid::Uuid;

use crate::api::{AppState, error::ApiError};

// deletes a user with everything in the workspaces only they belong to: chats, sandboxes, files,
// projects and credits. the outside cleanup runs first, so a failure leaves the user to retry on
pub async fn delete(state: &AppState, user: Uuid) -> Result<bool, ApiError> {
    if state.store.user(user).await?.is_none() {
        return Ok(false);
    }
    state.store.delete_user_sessions(user, None).await?;
    let workspaces = state.store.sole_workspaces(user).await?;
    for &workspace in &workspaces {
        for session in state.store.workspace_session_ids(workspace).await? {
            // runs on other instances stop once their session row is gone and the lease can't renew
            state.runs.cancel(session);
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
) -> Result<bool, ApiError> {
    if !state.store.set_user_suspended(user, suspended).await? {
        return Ok(false);
    }
    if suspended {
        state.store.delete_user_sessions(user, None).await?;
        for workspace in state.store.sole_workspaces(user).await? {
            for session in state.store.workspace_session_ids(workspace).await? {
                state.runs.cancel(session);
            }
        }
    }
    Ok(true)
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
