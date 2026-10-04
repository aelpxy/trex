use std::sync::Arc;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde_json::{Value, json};
use trex_store::library::{LibraryFile, is_invalid_path, is_not_found};

use super::{AppState, auth::CurrentUser, error::ApiError};

pub async fn list(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let files = state.library.list(user).await?;
    let data: Vec<Value> = files.iter().map(file_json).collect();
    Ok(Json(
        json!({"object": "list", "data": data, "has_more": false}),
    ))
}

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

pub async fn upload(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(path): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let size = body.len() as u64;
    state
        .library
        .put(user, &path, body.to_vec())
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"object": "file", "path": path, "size": size})),
    ))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(path): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .library
        .delete(user, &path)
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok(Json(
        json!({"object": "file", "path": path, "deleted": true}),
    ))
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

fn file_json(file: &LibraryFile) -> Value {
    json!({"object": "file", "path": file.path, "size": file.size, "modified_at": file.modified_at})
}
