use std::sync::Arc;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use trex_harness::{
    attachment,
    files::{self, FileError},
};
use trex_sandbox::Sandbox;
use utoipa::ToSchema;

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    sessions::find_session,
};
use crate::runs;

/// A file in the session's sandbox, under its home folder.
#[derive(Serialize, ToSchema)]
pub struct SandboxFile {
    #[schema(example = "sandbox_file")]
    object: &'static str,
    #[schema(example = "src/App.tsx")]
    path: String,
    /// Bytes.
    size: u64,
    /// Unix seconds.
    modified_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DeletedSandboxFile {
    #[schema(example = "sandbox_file")]
    object: &'static str,
    path: String,
    deleted: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct MoveSandboxFile {
    #[schema(example = "notes.md")]
    from: String,
    #[schema(example = "docs/notes.md")]
    to: String,
}

/// List sandbox files
///
/// Every file in the session's sandbox home, except dependency and cache folders such as
/// `node_modules` and `.git`; `has_more` means the listing was cut short. A stopped sandbox is
/// started; a session without one has no files. Up to 5000 files.
#[utoipa::path(
    get,
    operation_id = "list_sandbox_files",
    path = "/sessions/{id}/files",
    tag = "files",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<SandboxFile>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<SandboxFile>>, ApiError> {
    let Some(sandbox) = sandbox(&state, workspace, &id, false).await? else {
        return Ok(Json(List::new(Vec::new(), false)));
    };
    let listing = files::list(&state.openshell, &sandbox).await?;
    let data = listing
        .files
        .into_iter()
        .map(|file| SandboxFile {
            object: "sandbox_file",
            path: file.path,
            size: file.size,
            modified_at: file.modified_at,
        })
        .collect();
    Ok(Json(List::new(data, listing.truncated)))
}

/// Move a sandbox file
///
/// Renames a file or moves it to another folder; never overwrites an existing file.
#[utoipa::path(
    post,
    operation_id = "move_sandbox_file",
    path = "/sessions/{id}/files/move",
    tag = "files",
    params(("id" = String, Path, description = "Session id")),
    request_body = MoveSandboxFile,
    responses(
        (status = 200, body = SandboxFile),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "A file already exists at `to`", body = ErrorResponse),
    ),
)]
pub async fn move_file(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<MoveSandboxFile>,
) -> Result<Json<SandboxFile>, ApiError> {
    let sandbox = existing(&state, workspace, &id, &body.from).await?;
    files::rename(&state.openshell, &sandbox, &body.from, &body.to)
        .await
        .map_err(|error| file_error(error, &body.from))?;
    let listing = files::list(&state.openshell, &sandbox).await?;
    let moved = listing
        .files
        .into_iter()
        .find(|file| file.path == body.to)
        .ok_or_else(|| ApiError::NotFound(format!("no sandbox file {}", body.to)))?;
    Ok(Json(SandboxFile {
        object: "sandbox_file",
        path: moved.path,
        size: moved.size,
        modified_at: moved.modified_at,
    }))
}

// served under a {*path} wildcard, which openapi can't express, so these are documented with
// their full path and registered by hand
/// Download a sandbox file
///
/// Up to 10 MB; the response carries the sniffed content type.
#[utoipa::path(
    get,
    operation_id = "download_sandbox_file",
    path = "/v1/sessions/{id}/files/{path}",
    tag = "files",
    params(
        ("id" = String, Path, description = "Session id"),
        ("path" = String, Path, description = "Path under the sandbox home, may contain slashes"),
    ),
    responses(
        (status = 200, content_type = "application/octet-stream", body = super::library::FileContent),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path((id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let sandbox = existing(&state, workspace, &id, &path).await?;
    let content = files::read(&state.openshell, &sandbox, &path)
        .await
        .map_err(|error| file_error(error, &path))?;
    Ok((
        [(header::CONTENT_TYPE, attachment::sniff(&content))],
        content,
    ))
}

/// Upload a sandbox file
///
/// Creates or replaces a file, making its folders; the body is the raw content. A session
/// without a sandbox gets one.
#[utoipa::path(
    put,
    operation_id = "upload_sandbox_file",
    path = "/v1/sessions/{id}/files/{path}",
    tag = "files",
    params(
        ("id" = String, Path, description = "Session id"),
        ("path" = String, Path, description = "Path under the sandbox home, may contain slashes"),
    ),
    request_body(content = super::library::FileContent, content_type = "application/octet-stream"),
    responses(
        (status = 201, body = SandboxFile),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn upload(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path((id, path)): Path<(String, String)>,
    body: Bytes,
) -> Result<(StatusCode, Json<SandboxFile>), ApiError> {
    files::absolute(&path).map_err(|error| file_error(error, &path))?;
    let sandbox = sandbox(&state, workspace, &id, true)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("no sandbox after creating one")))?;
    let size = body.len() as u64;
    files::write(&state.openshell, &sandbox, &path, body.to_vec())
        .await
        .map_err(|error| file_error(error, &path))?;
    Ok((
        StatusCode::CREATED,
        Json(SandboxFile {
            object: "sandbox_file",
            path,
            size,
            modified_at: chrono::Utc::now().timestamp(),
        }),
    ))
}

/// Delete a sandbox file
#[utoipa::path(
    delete,
    operation_id = "delete_sandbox_file",
    path = "/v1/sessions/{id}/files/{path}",
    tag = "files",
    params(
        ("id" = String, Path, description = "Session id"),
        ("path" = String, Path, description = "Path under the sandbox home, may contain slashes"),
    ),
    responses(
        (status = 200, body = DeletedSandboxFile),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path((id, path)): Path<(String, String)>,
) -> Result<Json<DeletedSandboxFile>, ApiError> {
    let sandbox = existing(&state, workspace, &id, &path).await?;
    files::remove(&state.openshell, &sandbox, &path)
        .await
        .map_err(|error| file_error(error, &path))?;
    Ok(Json(DeletedSandboxFile {
        object: "sandbox_file",
        path,
        deleted: true,
    }))
}

async fn sandbox(
    state: &AppState,
    workspace: uuid::Uuid,
    id: &str,
    create: bool,
) -> Result<Option<Sandbox>, ApiError> {
    let session = find_session(state, workspace, id).await?;
    Ok(runs::files_sandbox(state, workspace, &session, create).await?)
}

// reading, moving or deleting needs a sandbox that already has the file
async fn existing(
    state: &AppState,
    workspace: uuid::Uuid,
    id: &str,
    path: &str,
) -> Result<Sandbox, ApiError> {
    files::absolute(path).map_err(|error| file_error(error, path))?;
    sandbox(state, workspace, id, false)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no sandbox file {path}")))
}

fn file_error(error: FileError, path: &str) -> ApiError {
    match error {
        FileError::InvalidPath(reason) => ApiError::invalid(reason, "path"),
        FileError::NotFound => ApiError::NotFound(format!("no sandbox file {path}")),
        FileError::TooLarge => ApiError::invalid(error.to_string(), "path"),
        FileError::Exists => ApiError::Conflict(format!("a file already exists at {path}")),
        FileError::Failed(error) => ApiError::Internal(error),
    }
}
