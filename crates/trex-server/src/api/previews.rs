use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_store::previews::{PREVIEW_TTL, Preview as PreviewRecord};
use utoipa::ToSchema;

use super::{
    AppState,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    sessions::find_session,
};
use crate::preview::new_preview_id;

#[derive(Deserialize, ToSchema)]
pub struct CreatePreview {
    /// The port the app listens on inside the sandbox.
    #[schema(example = 5173)]
    port: u16,
}

/// A link to an app running in the session's sandbox, on its own origin.
#[derive(Serialize, ToSchema)]
pub struct Preview {
    #[schema(example = "preview")]
    object: &'static str,
    port: u16,
    #[schema(example = "http://3f2a0c1e9b8d4f6a7c5e1b2d3a4f5e6d.preview.localhost:8081/")]
    url: String,
    /// Unix seconds; open the preview again for a new link.
    expires_at: i64,
}

/// Create a preview
///
/// Returns a link that shows whatever listens on `port` in the session's sandbox, including
/// websockets such as a dev server's hot reload. Anyone with the link can open it until it
/// expires, a day later. The app should listen on `::` or `localhost`.
#[utoipa::path(
    post,
    operation_id = "create_preview",
    path = "/sessions/{id}/previews",
    tag = "previews",
    params(("id" = String, Path, description = "Session id")),
    request_body = CreatePreview,
    responses(
        (status = 201, body = Preview),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<CreatePreview>,
) -> Result<(StatusCode, Json<Preview>), ApiError> {
    if body.port == 0 {
        return Err(ApiError::invalid(
            "port must be between 1 and 65535",
            "port",
        ));
    }
    let session = find_session(&state, workspace, &id).await?;
    let preview_id = new_preview_id()?;
    let record = PreviewRecord {
        workspace,
        session: session.id,
        port: body.port,
    };
    state.store.create_preview(&preview_id, &record).await?;
    Ok((
        StatusCode::CREATED,
        Json(Preview {
            object: "preview",
            port: body.port,
            url: state.preview_url.of(&preview_id),
            expires_at: chrono::Utc::now().timestamp() + PREVIEW_TTL.as_secs() as i64,
        }),
    ))
}
