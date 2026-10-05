use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
};
use trex_harness::attachment;
use trex_store::library::{is_invalid_path, is_not_found};

use super::{
    AppState,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    library::FileContent,
    sessions::ATTACHMENT_PREFIX,
};

/// Download an attachment
///
/// The file attached to a message, from its item's `attachments`.
#[utoipa::path(
    get,
    operation_id = "download_attachment",
    path = "/attachments/{id}",
    tag = "attachments",
    params(("id" = String, Path, description = "Attachment id")),
    responses(
        (status = 200, content_type = "application/octet-stream", body = FileContent),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let missing = || ApiError::NotFound(format!("no attachment {id}"));
    let hash = id.strip_prefix(ATTACHMENT_PREFIX).ok_or_else(missing)?;
    let content = state
        .library
        .get_attachment(workspace, hash)
        .await
        .map_err(|error| {
            if is_not_found(&error) || is_invalid_path(&error) {
                missing()
            } else {
                ApiError::Internal(error)
            }
        })?;
    Ok((
        [(header::CONTENT_TYPE, attachment::sniff(&content))],
        content,
    ))
}
