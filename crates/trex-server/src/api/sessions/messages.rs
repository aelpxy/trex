use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_harness::{attachment, history};
use trex_store::sessions::{self as store, SessionStatus};
use utoipa::ToSchema;

use super::{MAX_ATTACHMENTS, Session, find_session, session_object};
use crate::api::{
    AppState,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, SESSION},
};
use crate::runs;

#[derive(Deserialize, ToSchema)]
pub struct CreateMessage {
    #[schema(example = "Plot the CSV in my library")]
    content: String,
    /// While a run is in progress, stop the agent's current step so it reads the message right
    /// away instead of after the step finishes.
    #[serde(default)]
    interrupt: bool,
    /// Images (PNG, JPEG, GIF, WebP), PDFs or text files the model should see, up to 10.
    #[serde(default)]
    attachments: Vec<AttachmentInput>,
}

/// One attachment: either inline `data` or a file from the user's library.
#[derive(Deserialize, ToSchema)]
pub struct AttachmentInput {
    /// A base64 data URL, e.g. `data:image/png;base64,...`.
    data: Option<String>,
    /// A path in the user's library.
    library_path: Option<String>,
    /// Shown to the model for PDFs and text files.
    filename: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Run {
    #[schema(example = "run")]
    object: &'static str,
    session: String,
    #[schema(example = "running")]
    status: &'static str,
    /// True when the session was already running: the message waits for the agent's next step
    /// and arrives on the events stream as `message.received`.
    queued: bool,
}

/// Send a message
///
/// Starts a run; follow it on the events stream. While a run is in progress the message is
/// queued for the agent instead, and `interrupt` makes it stop the current step to read it.
#[utoipa::path(
    post,
    operation_id = "create_message",
    path = "/sessions/{id}/messages",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    request_body = CreateMessage,
    responses(
        (status = 202, body = Run),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "The run was changing state; retry", body = ErrorResponse),
    ),
)]
pub async fn create_message(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<CreateMessage>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    if body.content.trim().is_empty() && body.attachments.is_empty() {
        return Err(ApiError::invalid(
            "a message needs content or attachments",
            "content",
        ));
    }
    if body.attachments.len() > MAX_ATTACHMENTS {
        return Err(ApiError::invalid(
            format!("a message can have up to {MAX_ATTACHMENTS} attachments"),
            "attachments",
        ));
    }
    let session = find_session(&state, workspace, &id).await?;
    let mut parts = Vec::new();
    for input in body.attachments {
        let bytes = match (input.data, input.library_path) {
            (Some(data), None) => attachment::decode_data_url(&data)
                .map_err(|error| ApiError::invalid(format!("{error:#}"), "attachments"))?,
            (None, Some(path)) => state.library.get(workspace, &path).await.map_err(|error| {
                ApiError::invalid(format!("cannot attach {path}: {error:#}"), "attachments")
            })?,
            _ => {
                return Err(ApiError::invalid(
                    "each attachment needs exactly one of data or library_path",
                    "attachments",
                ));
            }
        };
        let part = attachment::store(&state.library, workspace, input.filename.as_deref(), bytes)
            .await
            .map_err(|error| ApiError::invalid(format!("{error:#}"), "attachments"))?;
        parts.push(part);
    }
    let mut message = history::to_json(&[attachment::user_message(&body.content, parts)])?
        .pop()
        .expect("one item was serialized");
    // the model sees attachments directly, and is told where its tools find them
    let items = state.store.session_items(workspace, session.id).await?;
    let uploads = attachment::new_uploads(&items, &message);
    if let (Some(note), Some(content)) = (
        attachment::uploads_note(&uploads),
        message["content"].as_array_mut(),
    ) {
        content.push(serde_json::to_value(note).map_err(anyhow::Error::from)?);
    }
    let started = runs::send_message(&state, workspace, &session, message, body.interrupt).await?;
    let queued = matches!(started, runs::Started::Queued);
    Ok((StatusCode::ACCEPTED, Json(run_object(&session, queued))))
}

/// Cancel the run
///
/// Stops the running agent; the run ends with `run.cancelled`.
#[utoipa::path(
    post,
    operation_id = "cancel_run",
    path = "/sessions/{id}/cancel",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 202, body = Session),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "No run is in progress", body = ErrorResponse),
    ),
)]
pub async fn cancel(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    if !runs::cancel(&state, Some(workspace), session.id).await? {
        return Err(ApiError::Conflict("no run is in progress".into()));
    }
    Ok((StatusCode::ACCEPTED, Json(session_object(&session))))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateBranch {
    /// Which of the user's messages to branch at, counting from 0.
    message: usize,
    /// Replaces that message's text, keeping its attachments; without it the branch keeps the
    /// message and the agent answers it again.
    content: Option<String>,
}

/// Branch a session
///
/// Starts a new chat with the history before one of the user's messages, then either an edited
/// version of that message or the same message answered again, and runs the agent on it (201).
/// The original chat is untouched. The branch gets its own sandbox, so files from the original
/// are only there if they were saved to the library.
#[utoipa::path(
    post,
    operation_id = "create_branch",
    path = "/sessions/{id}/branch",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    request_body = CreateBranch,
    responses(
        (status = 201, body = Session),
        (status = 400, response = ErrorResponse),
        (status = 402, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn branch(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<CreateBranch>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let source = find_session(&state, workspace, &id).await?;
    let items = state.store.session_items(workspace, source.id).await?;
    let Some(&position) = history::user_message_positions(&items).get(body.message) else {
        return Err(ApiError::invalid(
            format!("the chat has no message {}", body.message),
            "message",
        ));
    };
    let (keep, input) = match body.content {
        Some(content) => {
            let edited = history::with_text(&items[position], &content);
            if history::message_text(&edited).trim().is_empty()
                && attachment::references(&edited).is_empty()
            {
                return Err(ApiError::invalid(
                    "a message needs content or attachments",
                    "content",
                ));
            }
            (position, vec![edited])
        }
        None => (position + 1, Vec::new()),
    };
    let branch = state
        .store
        .branch_session(workspace, source.id, keep)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no session {id}")))?;
    runs::start(&state, workspace, &branch, input).await?;
    let branch = state
        .store
        .session(workspace, branch.id)
        .await?
        .unwrap_or(branch);
    Ok((StatusCode::CREATED, Json(session_object(&branch))))
}

/// Retry the run
///
/// Continues a run that failed or was stopped before the agent replied, from the saved history;
/// nothing new is added to the conversation.
#[utoipa::path(
    post,
    operation_id = "retry_run",
    path = "/sessions/{id}/retry",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 202, body = Run),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "A run is in progress, the session is waiting for answers, or the agent already replied", body = ErrorResponse),
    ),
)]
pub async fn retry(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    match session.status {
        SessionStatus::Running => {
            return Err(ApiError::Conflict("a run is already in progress".into()));
        }
        SessionStatus::NeedsInput => {
            return Err(ApiError::Conflict(
                "the session is waiting for answers".into(),
            ));
        }
        SessionStatus::Idle | SessionStatus::Failed => {}
    }
    let history = history::from_json(state.store.session_items(workspace, session.id).await?)?;
    if history.is_empty() || history::ends_with_reply(&history) {
        return Err(ApiError::Conflict(
            "the agent already replied; send a message to continue".into(),
        ));
    }
    runs::start(&state, workspace, &session, Vec::new()).await?;
    Ok((StatusCode::ACCEPTED, Json(run_object(&session, false))))
}

pub(super) fn run_object(session: &store::Session, queued: bool) -> Run {
    Run {
        object: "run",
        session: ids::encode(SESSION, session.id),
        status: "running",
        queued,
    }
}
