use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use trex_sandbox::Sandbox;
use trex_store::sessions as store;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{find_session, sandbox_of};
use crate::api::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
};

#[derive(Deserialize, ToSchema)]
pub struct RejectAccess {
    /// Told to the agent; defaults to "rejected by the user".
    reason: Option<String>,
}

/// Outbound network access the sandbox was denied; approving it lets the agent retry.
#[derive(Serialize, ToSchema)]
pub struct AccessRequest {
    id: String,
    #[schema(example = "access_request")]
    object: &'static str,
    status: AccessStatus,
    #[schema(example = json!(["pypi.org:443"]))]
    endpoints: Vec<String>,
    /// The program that tried to connect.
    binary: String,
    rationale: String,
    security_notes: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccessStatus {
    Pending,
    Approved,
    Rejected,
}

// denials are batched into requests every few seconds, so ones raised at the end of a run
// arrive after its live watcher stopped; the ui lists them here at any time
/// List access requests
#[utoipa::path(
    get,
    operation_id = "list_access_requests",
    path = "/sessions/{id}/access_requests",
    tag = "access requests",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<AccessRequest>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn list_access(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<AccessRequest>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let requests = match sandbox_of(workspace, &session) {
        Some(sandbox) => state.openshell.pending_access(&sandbox).await?,
        None => Vec::new(),
    };
    Ok(Json(List::new(
        requests
            .into_iter()
            .map(|request| access_request(request, AccessStatus::Pending))
            .collect(),
        false,
    )))
}

/// Approve an access request
///
/// Adds the endpoints to the sandbox's network policy.
#[utoipa::path(
    post,
    operation_id = "approve_access_request",
    path = "/sessions/{id}/access_requests/{request_id}/approve",
    tag = "access requests",
    params(
        ("id" = String, Path, description = "Session id"),
        ("request_id" = String, Path, description = "Access request id"),
    ),
    responses(
        (status = 200, body = AccessRequest),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn approve_access(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path((id, request_id)): Path<(String, String)>,
) -> Result<Json<AccessRequest>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let (sandbox, request) = find_access_request(&state, workspace, &session, &request_id).await?;
    state.openshell.approve_access(&sandbox, &request).await?;
    Ok(Json(access_request(request, AccessStatus::Approved)))
}

/// Reject an access request
#[utoipa::path(
    post,
    operation_id = "reject_access_request",
    path = "/sessions/{id}/access_requests/{request_id}/reject",
    tag = "access requests",
    params(
        ("id" = String, Path, description = "Session id"),
        ("request_id" = String, Path, description = "Access request id"),
    ),
    request_body(content = Option<RejectAccess>),
    responses(
        (status = 200, body = AccessRequest),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn reject_access(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path((id, request_id)): Path<(String, String)>,
    body: Option<Json<RejectAccess>>,
) -> Result<Json<AccessRequest>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let (sandbox, request) = find_access_request(&state, workspace, &session, &request_id).await?;
    let reason = body
        .and_then(|Json(body)| body.reason)
        .unwrap_or_else(|| "rejected by the user".into());
    state
        .openshell
        .reject_access(&sandbox, &request, &reason)
        .await?;
    Ok(Json(access_request(request, AccessStatus::Rejected)))
}

async fn find_access_request(
    state: &AppState,
    workspace: Uuid,
    session: &store::Session,
    request_id: &str,
) -> Result<(Sandbox, trex_sandbox::AccessRequest), ApiError> {
    let not_found = || ApiError::NotFound(format!("no pending access request {request_id}"));
    let sandbox = sandbox_of(workspace, session).ok_or_else(not_found)?;
    let request = state
        .openshell
        .pending_access(&sandbox)
        .await?
        .into_iter()
        .find(|request| request.id == request_id)
        .ok_or_else(not_found)?;
    Ok((sandbox, request))
}

fn access_request(request: trex_sandbox::AccessRequest, status: AccessStatus) -> AccessRequest {
    AccessRequest {
        id: request.id,
        object: "access_request",
        status,
        endpoints: request.endpoints,
        binary: request.binary,
        rationale: request.rationale,
        security_notes: request.security_notes,
    }
}
