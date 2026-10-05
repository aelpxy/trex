use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use trex_harness::attachment;
use trex_store::library::{LibraryFile, is_invalid_path, is_not_found};
use utoipa::{IntoParams, ToSchema};

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
};

#[derive(Serialize, ToSchema)]
pub struct File {
    #[schema(example = "file")]
    object: &'static str,
    #[schema(example = "reports/q3.pdf")]
    path: String,
    /// Bytes.
    size: u64,
    /// Unix seconds.
    modified_at: i64,
}

/// Raw file bytes.
#[derive(ToSchema)]
#[schema(format = Binary)]
#[allow(
    dead_code,
    reason = "never built, it only describes request and response bodies"
)]
pub struct FileContent(String);

#[derive(Serialize, ToSchema)]
pub struct DeletedFile {
    #[schema(example = "file")]
    object: &'static str,
    path: String,
    deleted: bool,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Only files under this folder, e.g. `reports/`.
    #[param(example = "reports/")]
    prefix: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct MoveFile {
    #[schema(example = "draft.md")]
    from: String,
    #[schema(example = "reports/final.md")]
    to: String,
}

/// List library files
///
/// The workspace's files, shared by all its chats; agents reach them with the library tools.
#[utoipa::path(
    get,
    operation_id = "list_files",
    path = "/library",
    tag = "library",
    params(ListQuery),
    responses((status = 200, body = List<File>)),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Query(query): Query<ListQuery>,
) -> Result<Json<List<File>>, ApiError> {
    let files = state.library.list(workspace).await?;
    let prefix = query.prefix.unwrap_or_default();
    let data = files
        .iter()
        .filter(|entry| entry.path.starts_with(&prefix))
        .map(file)
        .collect();
    Ok(Json(List::new(data, false)))
}

/// Move a file
///
/// Renames a file or moves it to another folder; never overwrites an existing file.
#[utoipa::path(
    post,
    operation_id = "move_file",
    path = "/library/move",
    tag = "library",
    request_body = MoveFile,
    responses(
        (status = 200, body = File),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "A file already exists at `to`", body = ErrorResponse),
    ),
)]
pub async fn move_file(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Json(body): Json<MoveFile>,
) -> Result<Json<File>, ApiError> {
    let moved = state
        .library
        .rename(workspace, &body.from, &body.to)
        .await
        .map_err(|error| library_error(error, &body.from))?;
    if !moved {
        return Err(ApiError::Conflict(format!(
            "a file already exists at {}",
            body.to
        )));
    }
    let files = state.library.list(workspace).await?;
    let moved = files
        .iter()
        .find(|entry| entry.path == body.to)
        .ok_or_else(|| ApiError::NotFound(format!("no library file {}", body.to)))?;
    Ok(Json(file(moved)))
}

// the router serves these under a {*path} wildcard, which openapi can't express, so they are
// documented with their full path and registered by hand
/// Download a file
#[utoipa::path(
    get,
    operation_id = "download_file",
    path = "/v1/library/files/{path}",
    tag = "library",
    params(("path" = String, Path, description = "File path, may contain slashes")),
    responses(
        (status = 200, content_type = "application/octet-stream", body = FileContent),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
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

/// Upload a file
///
/// Creates or replaces a file; the body is the raw content, up to 100 MB.
#[utoipa::path(
    put,
    operation_id = "upload_file",
    path = "/v1/library/files/{path}",
    tag = "library",
    params(("path" = String, Path, description = "File path, may contain slashes")),
    request_body(content = FileContent, content_type = "application/octet-stream"),
    responses(
        (status = 201, body = File),
        (status = 400, response = ErrorResponse),
    ),
)]
pub async fn upload(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(path): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<File>), ApiError> {
    let size = body.len() as u64;
    state
        .library
        .put(workspace, &path, body.to_vec())
        .await
        .map_err(|error| library_error(error, &path))?;
    let modified_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    Ok((
        StatusCode::CREATED,
        Json(File {
            object: "file",
            path,
            size,
            modified_at,
        }),
    ))
}

/// Delete a file
#[utoipa::path(
    delete,
    operation_id = "delete_file",
    path = "/v1/library/files/{path}",
    tag = "library",
    params(("path" = String, Path, description = "File path, may contain slashes")),
    responses(
        (status = 200, body = DeletedFile),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(path): Path<String>,
) -> Result<Json<DeletedFile>, ApiError> {
    state
        .library
        .delete(workspace, &path)
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok(Json(DeletedFile {
        object: "file",
        path,
        deleted: true,
    }))
}

fn library_error(error: anyhow::Error, path: &str) -> ApiError {
    if is_not_found(&error) {
        return ApiError::NotFound(format!("no library file {path}"));
    }
    if is_invalid_path(&error) {
        return ApiError::invalid(error.to_string(), "path");
    }
    ApiError::Internal(error)
}

fn file(file: &LibraryFile) -> File {
    File {
        object: "file",
        path: file.path.clone(),
        size: file.size,
        modified_at: file.modified_at,
    }
}
