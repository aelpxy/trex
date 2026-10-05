use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use trex_sandbox::{Sandbox, SandboxState, workspace_name};
use utoipa::ToSchema;
use uuid::Uuid;

use super::Admin;
use crate::api::{
    AppState, List,
    error::{ApiError, ErrorResponse},
    ids::{self, SESSION, WORKSPACE},
};

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPhase {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
    Deleting,
    Unknown,
}

impl From<SandboxState> for SandboxPhase {
    fn from(state: SandboxState) -> Self {
        match state {
            SandboxState::Starting => Self::Starting,
            SandboxState::Running => Self::Running,
            SandboxState::Stopping => Self::Stopping,
            SandboxState::Stopped => Self::Stopped,
            SandboxState::Error => Self::Error,
            SandboxState::Deleting => Self::Deleting,
            SandboxState::Unknown => Self::Unknown,
        }
    }
}

/// The chat a sandbox belongs to.
#[derive(Serialize, ToSchema)]
pub struct SandboxChat {
    #[schema(example = "sess_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    session: String,
    title: Option<String>,
    /// The chat's agent is working, so the sandbox can't be stopped or deleted.
    running: bool,
    /// Unix seconds, the chat's last activity; idle sandboxes stop a while after it.
    active_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct AdminSandbox {
    #[schema(example = "vocal-oriole")]
    name: String,
    #[schema(example = "sandbox")]
    object: &'static str,
    workspace: String,
    workspace_name: String,
    owner_email: Option<String>,
    state: SandboxPhase,
    /// Null for a sandbox no chat uses any more, which is safe to delete.
    chat: Option<SandboxChat>,
}

/// List sandboxes
///
/// Every sandbox the gateway runs for trex workspaces, with the chat it belongs to.
#[utoipa::path(
    get,
    operation_id = "list_sandboxes",
    path = "/admin/sandboxes",
    tag = "admin",
    responses((status = 200, body = List<AdminSandbox>), (status = 403, response = ErrorResponse)),
)]
pub async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Result<Json<List<AdminSandbox>>, ApiError> {
    // the gateway names workspaces by a hash of trex's ids, so ours are recognised by recomputing it
    let workspaces: HashMap<String, _> = state
        .store
        .workspace_owners()
        .await?
        .into_iter()
        .map(|owner| (workspace_name(owner.workspace), owner))
        .collect();
    let chats: HashMap<(Uuid, String), _> = state
        .store
        .sandbox_chats()
        .await?
        .into_iter()
        .map(|chat| ((chat.workspace, chat.sandbox.clone()), chat))
        .collect();
    let mut data: Vec<AdminSandbox> = state
        .openshell
        .list_all()
        .await?
        .into_iter()
        .filter_map(|listed| {
            let owner = workspaces.get(&listed.sandbox.workspace)?;
            let chat = chats.get(&(owner.workspace, listed.sandbox.name.clone()));
            Some(AdminSandbox {
                object: "sandbox",
                workspace: ids::encode(WORKSPACE, owner.workspace),
                workspace_name: owner.name.clone(),
                owner_email: owner.owner_email.clone(),
                state: listed.state.into(),
                chat: chat.map(|chat| SandboxChat {
                    session: ids::encode(SESSION, chat.session),
                    title: chat.title.clone(),
                    running: chat.running,
                    active_at: chat.updated_at,
                }),
                name: listed.sandbox.name,
            })
        })
        .collect();
    // leftovers first, then the rest by workspace
    data.sort_by(|a, b| {
        (a.chat.is_some(), &a.workspace_name, &a.name).cmp(&(
            b.chat.is_some(),
            &b.workspace_name,
            &b.name,
        ))
    });
    Ok(Json(List::new(data, false)))
}

// a sandbox of one of trex's workspaces that no running chat is using
async fn idle_sandbox(
    state: &AppState,
    workspace: &str,
    name: &str,
) -> Result<(Uuid, Sandbox), ApiError> {
    let workspace = ids::decode(WORKSPACE, workspace)
        .ok_or_else(|| ApiError::NotFound(format!("no workspace {workspace}")))?;
    if state.store.sandbox_in_use(workspace, name).await? {
        return Err(ApiError::Conflict(
            "its chat is running; stop the run first".into(),
        ));
    }
    Ok((
        workspace,
        Sandbox {
            workspace: workspace_name(workspace),
            name: name.to_owned(),
        },
    ))
}

/// Stop a sandbox
///
/// Its files stay; the chat starts it again when it next needs it.
#[utoipa::path(
    post,
    operation_id = "stop_sandbox",
    path = "/admin/sandboxes/{workspace}/{name}/stop",
    tag = "admin",
    params(
        ("workspace" = String, Path, description = "Workspace id"),
        ("name" = String, Path, description = "Sandbox name"),
    ),
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "Its chat is running", body = ErrorResponse),
    ),
)]
pub async fn stop_sandbox(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path((workspace, name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let (workspace, sandbox) = idle_sandbox(&state, &workspace, &name).await?;
    state.openshell.stop(&sandbox).await?;
    state.store.mark_sandbox_stopped(workspace, &name).await?;
    tracing::info!(admin = %admin.user, workspace = %workspace, sandbox = name, "stopped a sandbox");
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a sandbox
///
/// Its files are gone. A chat that still uses it gets an empty one when it next needs a sandbox,
/// and the agent is told its files are gone.
#[utoipa::path(
    delete,
    operation_id = "delete_sandbox",
    path = "/admin/sandboxes/{workspace}/{name}",
    tag = "admin",
    params(
        ("workspace" = String, Path, description = "Workspace id"),
        ("name" = String, Path, description = "Sandbox name"),
    ),
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "Its chat is running", body = ErrorResponse),
    ),
)]
pub async fn delete_sandbox(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path((workspace, name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let (workspace, sandbox) = idle_sandbox(&state, &workspace, &name).await?;
    state.openshell.delete(&sandbox).await?;
    tracing::info!(admin = %admin.user, workspace = %workspace, sandbox = name, "deleted a sandbox");
    Ok(StatusCode::NO_CONTENT)
}
