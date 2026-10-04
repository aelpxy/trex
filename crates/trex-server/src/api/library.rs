use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Serialize;
use trex_store::library::{LibraryFile, is_invalid_path, is_not_found};
use utoipa::ToSchema;

use super::{
    AppState, List,
    auth::CurrentUser,
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

/// List library files
///
/// The user's files, shared by all their sessions; agents reach them with the library tools.
#[utoipa::path(
    get,
    operation_id = "list_files",
    path = "/library",
    tag = "library",
    responses((status = 200, body = List<File>)),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<List<File>>, ApiError> {
    let files = state.library.list(user).await?;
    Ok(Json(List::new(files.iter().map(file).collect(), false)))
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
    CurrentUser(user): CurrentUser,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let content = state
        .library
        .get(user, &path)
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
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
    CurrentUser(user): CurrentUser,
    Path(path): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<File>), ApiError> {
    let size = body.len() as u64;
    state
        .library
        .put(user, &path, body.to_vec())
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
    CurrentUser(user): CurrentUser,
    Path(path): Path<String>,
) -> Result<Json<DeletedFile>, ApiError> {
    state
        .library
        .delete(user, &path)
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
