use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
};
use trex_harness::attachment;

use super::{Admin, not_found};
use crate::api::{
    AppState, List,
    error::{ApiError, ErrorResponse},
    ids::{self, WORKSPACE},
    library::{File, FileContent, file, library_error},
};

/// List a workspace's library
#[utoipa::path(
    get,
    operation_id = "list_workspace_library",
    path = "/admin/workspaces/{id}/library",
    tag = "admin",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, body = List<File>),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn workspace_library(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path(id): Path<String>,
) -> Result<Json<List<File>>, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    let files = state.library.list(workspace).await?;
    Ok(Json(List::new(files.iter().map(file).collect(), false)))
}

// served under a {*path} wildcard, so documented with its full path and registered by hand
/// Download a file from a workspace's library
#[utoipa::path(
    get,
    operation_id = "download_workspace_file",
    path = "/v1/admin/workspaces/{id}/library/files/{path}",
    tag = "admin",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Path, description = "File path, may contain slashes"),
    ),
    responses(
        (status = 200, content_type = "application/octet-stream", body = FileContent),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn workspace_file(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path((id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    let content = state
        .library
        .get(workspace, &path)
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok((
        [(header::CONTENT_TYPE, attachment::sniff(&content))],
        content,
    ))
}
