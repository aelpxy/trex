use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use utoipa::ToSchema;

use super::Admin;
use crate::api::{
    AppState, List,
    error::{ApiError, ErrorResponse},
    ids::{self, SESSION, WORKSPACE},
};

#[derive(Serialize, ToSchema)]
pub struct LiveRun {
    /// The chat the run belongs to.
    #[schema(example = "sess_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    session: String,
    #[schema(example = "run")]
    object: &'static str,
    title: Option<String>,
    workspace: String,
    workspace_name: String,
    owner_email: Option<String>,
    model: String,
    reasoning_effort: Option<String>,
    fast: bool,
    /// Started by a scheduled task rather than a person.
    scheduled: bool,
    /// Unix seconds; null for runs started before trex recorded it.
    started_at: Option<i64>,
    /// Unix seconds, when the instance running it last renewed its lease (every 10s). Over 30s
    /// old means that instance went away and another will resume it.
    heartbeat_at: Option<i64>,
    /// A cancel was asked for; the run stops at its next heartbeat.
    cancel_requested: bool,
    /// Spent since the run started, in millionths of a US dollar.
    credits: i64,
    responses: i64,
}

/// List live runs
///
/// Every chat whose agent is working right now, across all workspaces, longest running first.
#[utoipa::path(
    get,
    operation_id = "list_live_runs",
    path = "/admin/runs",
    tag = "admin",
    responses((status = 200, body = List<LiveRun>), (status = 403, response = ErrorResponse)),
)]
pub async fn list_runs(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Result<Json<List<LiveRun>>, ApiError> {
    let data = state
        .store
        .live_runs()
        .await?
        .into_iter()
        .map(|run| LiveRun {
            session: ids::encode(SESSION, run.session),
            object: "run",
            title: run.title,
            workspace: ids::encode(WORKSPACE, run.workspace),
            workspace_name: run.workspace_name,
            owner_email: run.owner_email,
            model: run.model,
            reasoning_effort: run.reasoning_effort,
            fast: run.fast,
            scheduled: run.scheduled,
            started_at: run.started_at,
            heartbeat_at: run.heartbeat_at,
            cancel_requested: run.cancel_requested,
            credits: run.credits,
            responses: run.responses,
        })
        .collect();
    Ok(Json(List::new(data, false)))
}

/// Cancel a run
///
/// Stops a chat's run in any workspace; it ends with `run.cancelled` within one heartbeat (10s).
#[utoipa::path(
    post,
    operation_id = "cancel_live_run",
    path = "/admin/runs/{id}/cancel",
    tag = "admin",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 202),
        (status = 403, response = ErrorResponse),
        (status = 409, description = "Nothing is running in that chat", body = ErrorResponse),
    ),
)]
pub async fn cancel_run(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let session =
        ids::decode(SESSION, &id).ok_or_else(|| ApiError::NotFound(format!("no session {id}")))?;
    if !crate::runs::cancel(&state, None, session).await? {
        return Err(ApiError::Conflict("nothing is running in that chat".into()));
    }
    tracing::info!(admin = %admin.user, session = %session, "cancelled a run");
    Ok(StatusCode::ACCEPTED)
}
