use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use trex_harness::{
    attachment, history,
    question::{self, Answer},
};
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::sessions::{self as store, SessionFilter, SessionStatus};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, PROJECT, SESSION},
};
use crate::runs;

const DEFAULT_LIMIT: i64 = 20;
const MAX_ATTACHMENTS: usize = 10;
const MAX_TITLE_CHARS: usize = 120;
pub const ATTACHMENT_PREFIX: &str = "att_";
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize, ToSchema)]
pub struct CreateSession {
    /// A model id from `GET /v1/models`.
    #[schema(example = "gpt-6.1-sol")]
    model: String,
    /// One of the model's `reasoning_efforts` from `GET /v1/models` (any of `none`, `minimal`, `low`,
    /// `medium`, `high`, `xhigh`, `max` when it lists none); the provider default when omitted.
    #[schema(example = "medium")]
    reasoning_effort: Option<String>,
    /// Faster responses at a higher cost, on models whose `fast` is true.
    #[serde(default)]
    fast: bool,
    /// Start the chat inside this project; it then follows the project's instructions.
    #[schema(example = "proj_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    project_id: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Page size, 1 to 100.
    #[param(default = 20, minimum = 1, maximum = 100)]
    limit: Option<i64>,
    /// A session id; returns the sessions created before it.
    starting_after: Option<String>,
    /// A project id for that project's chats, or `none` for chats outside any project.
    #[param(example = "none")]
    project_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateSession {
    #[schema(example = "Fix SSE reconnect on resume")]
    title: Option<String>,
    /// Move the chat into a project, or out of its project with null.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    project_id: Option<Option<String>>,
    /// Switch the model for the next run.
    model: Option<String>,
    /// The effort for the next run; null uses the provider's default.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    reasoning_effort: Option<Option<String>>,
    fast: Option<bool>,
}

// tells an explicit null (Some(None)) apart from a missing field (None)
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

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

/// A file attached to a message; download it from `GET /v1/attachments/{id}`.
#[derive(Serialize, ToSchema)]
pub struct Attachment {
    #[schema(example = "att_9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
    id: String,
    #[schema(example = "image")]
    kind: &'static str,
    #[schema(example = "image/png")]
    mime_type: String,
    filename: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateAnswers {
    /// One answer per pending question, in order.
    answers: Vec<AnswerInput>,
}

#[derive(Deserialize, ToSchema)]
pub struct AnswerInput {
    /// Labels of the chosen options.
    #[serde(default)]
    selected: Vec<String>,
    /// Free text, alongside or instead of options.
    text: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct RejectAccess {
    /// Told to the agent; defaults to "rejected by the user".
    reason: Option<String>,
}

#[derive(Deserialize)]
struct PendingQuestion {
    call_id: String,
    questions: Vec<question::Question>,
}

#[derive(Serialize, ToSchema)]
pub struct Session {
    #[schema(example = "sess_0199b2c4e5f67a8b9c0d1e2f3a4b5c6d")]
    id: String,
    #[schema(example = "session")]
    object: &'static str,
    /// Generated from the first message unless set with `PATCH`; null until then.
    #[schema(example = "Fix SSE reconnect on resume")]
    title: Option<String>,
    /// The project the chat belongs to; null for chats outside any project.
    project_id: Option<String>,
    model: String,
    reasoning_effort: Option<String>,
    /// Faster responses at a higher cost.
    fast: bool,
    status: Status,
    /// Set while `status` is `needs_input`; answer with `POST /v1/sessions/{id}/answers`.
    pending_questions: Option<Vec<Question>>,
    /// Why the last run failed, when `status` is `failed`.
    last_error: Option<String>,
    /// Unix seconds.
    created_at: i64,
    /// Unix seconds.
    updated_at: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Running,
    NeedsInput,
    Failed,
}

#[derive(Serialize, ToSchema)]
pub struct Question {
    question: String,
    /// Empty for a free-text question.
    options: Vec<QuestionOption>,
    multi_select: bool,
}

#[derive(Serialize, ToSchema)]
pub struct QuestionOption {
    label: String,
    description: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct DeletedSession {
    id: String,
    #[schema(example = "session")]
    object: &'static str,
    deleted: bool,
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

/// A conversation item, rendered by `type`.
#[derive(Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Message {
        #[schema(example = "assistant")]
        role: String,
        text: String,
        /// Files the user attached; empty for other messages.
        attachments: Vec<Attachment>,
    },
    ToolCall {
        call_id: String,
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    ToolResult {
        call_id: String,
        output: String,
    },
    Reasoning {
        summary: String,
    },
    /// Everything before this item was summarized to free up context; the model sees only this
    /// summary and the items after it.
    Compaction {
        summary: String,
    },
}

/// An item with when it was saved.
#[derive(Serialize, ToSchema)]
pub struct TimedItem {
    #[serde(flatten)]
    item: Item,
    /// The item's position in the conversation, counting from 1; see `run.started`'s `items`.
    seq: i64,
    /// Unix milliseconds.
    created_at: i64,
}

/// One model response.
#[derive(Serialize, ToSchema)]
pub struct UsageEntry {
    /// Unix milliseconds, when the response finished.
    created_at: i64,
    model: String,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    credits: i64,
    /// From sending the request to the end of the response.
    duration_ms: i64,
    /// Until the first streamed output; null for older records or when nothing streamed.
    first_token_ms: Option<i64>,
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

/// Create a session
#[utoipa::path(
    post,
    operation_id = "create_session",
    path = "/sessions",
    tag = "sessions",
    request_body = CreateSession,
    responses(
        (status = 201, body = Session),
        (status = 400, response = ErrorResponse),
    ),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Json(body): Json<CreateSession>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    check_settings(
        &state,
        &body.model,
        body.reasoning_effort.as_deref(),
        body.fast,
    )?;
    let project = match body.project_id.as_deref() {
        Some(id) => Some(find_project_id(&state, workspace, id).await?),
        None => None,
    };
    let session = state
        .store
        .create_session(
            workspace,
            &body.model,
            body.reasoning_effort.as_deref(),
            body.fast,
            project,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(session_object(&session))))
}

fn check_settings(
    state: &AppState,
    model: &str,
    effort: Option<&str>,
    fast: bool,
) -> Result<(), ApiError> {
    let Some(config) = state.models.get(model) else {
        return Err(ApiError::invalid(format!("unknown model {model}"), "model"));
    };
    if fast && !config.supports_fast() {
        return Err(ApiError::invalid(
            format!("{model} has no fast mode"),
            "fast",
        ));
    }
    if let Some(effort) = effort {
        config
            .check_effort(effort)
            .map_err(|error| ApiError::invalid(format!("{error:#}"), "reasoning_effort"))?;
    }
    Ok(())
}

/// List sessions
///
/// Newest first.
#[utoipa::path(
    get,
    operation_id = "list_sessions",
    path = "/sessions",
    tag = "sessions",
    params(ListQuery),
    responses(
        (status = 200, body = List<Session>),
        (status = 400, response = ErrorResponse),
    ),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Query(query): Query<ListQuery>,
) -> Result<Json<List<Session>>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::invalid(
            format!("limit must be between 1 and {MAX_LIMIT}"),
            "limit",
        ));
    }
    let before = match &query.starting_after {
        Some(cursor) => Some(
            ids::decode(SESSION, cursor)
                .ok_or_else(|| ApiError::invalid("invalid session id", "starting_after"))?,
        ),
        None => None,
    };
    let filter = match query.project_id.as_deref() {
        None => SessionFilter::All,
        Some("none") => SessionFilter::NoProject,
        Some(id) => SessionFilter::Project(
            ids::decode(PROJECT, id)
                .ok_or_else(|| ApiError::invalid("invalid project id", "project_id"))?,
        ),
    };
    // one extra row tells whether another page exists
    let mut sessions = state
        .store
        .sessions(workspace, limit + 1, before, filter)
        .await?;
    let has_more = sessions.len() as i64 > limit;
    sessions.truncate(limit as usize);
    Ok(Json(List::new(
        sessions.iter().map(session_object).collect(),
        has_more,
    )))
}

/// Get a session
#[utoipa::path(
    get,
    operation_id = "get_session",
    path = "/sessions/{id}",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = Session),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<Session>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    Ok(Json(session_object(&session)))
}

/// Update a session
///
/// Renames the chat, moves it into or out of a project, or changes the model, effort or fast mode for its next run.
#[utoipa::path(
    patch,
    operation_id = "update_session",
    path = "/sessions/{id}",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    request_body = UpdateSession,
    responses(
        (status = 200, body = Session),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<UpdateSession>,
) -> Result<Json<Session>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let title = body.title.as_deref().map(str::trim);
    if title.is_some_and(|title| title.is_empty() || title.chars().count() > MAX_TITLE_CHARS) {
        return Err(ApiError::invalid(
            format!("title must be 1 to {MAX_TITLE_CHARS} characters"),
            "title",
        ));
    }
    let project = match body.project_id {
        Some(Some(id)) => Some(Some(find_project_id(&state, workspace, &id).await?)),
        Some(None) => Some(None),
        None => None,
    };
    if body.model.is_some() || body.reasoning_effort.is_some() || body.fast.is_some() {
        let model_id = body.model.as_deref().unwrap_or(&session.model);
        let effort = match &body.reasoning_effort {
            Some(effort) => effort.as_deref(),
            None => session.reasoning_effort.as_deref(),
        };
        let fast = body.fast.unwrap_or(session.fast);
        check_settings(&state, model_id, effort, fast)?;
        state
            .store
            .set_session_model(workspace, session.id, model_id, effort, fast)
            .await?;
    }
    let session = state
        .store
        .update_session(workspace, session.id, title, project)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no session {id}")))?;
    Ok(Json(session_object(&session)))
}

/// Delete a session
///
/// Cancels any run and deletes the session's sandbox and history.
#[utoipa::path(
    delete,
    operation_id = "delete_session",
    path = "/sessions/{id}",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = DeletedSession),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<DeletedSession>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    state.runs.cancel(session.id);
    if let Some(sandbox) = sandbox_of(workspace, &session) {
        state.openshell.delete(&sandbox).await?;
    }
    state.store.delete_session(workspace, session.id).await?;
    state.store.delete_events(session.id).await?;
    Ok(Json(DeletedSession {
        id: ids::encode(SESSION, session.id),
        object: "session",
        deleted: true,
    }))
}

/// List conversation items
///
/// The whole conversation, oldest first.
#[utoipa::path(
    get,
    operation_id = "list_items",
    path = "/sessions/{id}/items",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<TimedItem>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn items(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<TimedItem>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let items = state
        .store
        .session_items_timed(workspace, session.id)
        .await?;
    let data = items
        .iter()
        .filter_map(|(value, seq, created_at)| {
            item(value).map(|item| TimedItem {
                item,
                seq: *seq,
                created_at: *created_at,
            })
        })
        .collect();
    Ok(Json(List::new(data, false)))
}

/// List usage
///
/// Every model response of the session with its tokens, credits and duration, oldest first. Match
/// them to turns by `created_at` against the items'.
#[utoipa::path(
    get,
    operation_id = "list_session_usage",
    path = "/sessions/{id}/usage",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<UsageEntry>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn usage(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<UsageEntry>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let entries = state.store.session_usage(workspace, session.id).await?;
    let data = entries
        .into_iter()
        .map(|entry| UsageEntry {
            created_at: entry.created_at_ms,
            model: entry.model,
            input_tokens: entry.input_tokens,
            cached_input_tokens: entry.cached_input_tokens,
            output_tokens: entry.output_tokens,
            reasoning_tokens: entry.reasoning_tokens,
            credits: entry.credits,
            duration_ms: entry.duration_ms,
            first_token_ms: entry.first_token_ms,
            cache_write_tokens: entry.cache_write_tokens,
        })
        .collect();
    Ok(Json(List::new(data, false)))
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

/// Answer questions
///
/// Answers the session's `pending_questions` and resumes the agent.
#[utoipa::path(
    post,
    operation_id = "create_answers",
    path = "/sessions/{id}/answers",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    request_body = CreateAnswers,
    responses(
        (status = 202, body = Run),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "The session is not waiting for an answer", body = ErrorResponse),
    ),
)]
pub async fn create_answers(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<CreateAnswers>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let pending = match (&session.status, &session.pending_question) {
        (SessionStatus::NeedsInput, Some(pending)) => {
            serde_json::from_value::<PendingQuestion>(pending.clone())
                .map_err(|error| ApiError::Internal(error.into()))?
        }
        _ => {
            return Err(ApiError::Conflict(
                "this session is not waiting for an answer".into(),
            ));
        }
    };
    let answers = validate_answers(&pending.questions, body.answers)?;
    let item = question::answer_item(&pending.call_id, &pending.questions, &answers);
    let input = history::to_json(&[item])?;
    runs::start(&state, workspace, &session, input).await?;
    Ok((StatusCode::ACCEPTED, Json(run_object(&session, false))))
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
    if !state.runs.cancel(session.id) {
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

async fn find_project_id(state: &AppState, workspace: Uuid, id: &str) -> Result<Uuid, ApiError> {
    let invalid = || ApiError::invalid(format!("no project {id}"), "project_id");
    let uuid = ids::decode(PROJECT, id).ok_or_else(invalid)?;
    let project = state
        .store
        .project(workspace, uuid)
        .await?
        .ok_or_else(invalid)?;
    Ok(project.id)
}

pub async fn find_session(
    state: &AppState,
    workspace: Uuid,
    id: &str,
) -> Result<store::Session, ApiError> {
    let not_found = || ApiError::NotFound(format!("no session {id}"));
    let uuid = ids::decode(SESSION, id).ok_or_else(not_found)?;
    state
        .store
        .session(workspace, uuid)
        .await?
        .ok_or_else(not_found)
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

// a session's sandbox always lives in its owner's workspace
fn sandbox_of(workspace: Uuid, session: &store::Session) -> Option<Sandbox> {
    session.sandbox.as_ref().map(|name| Sandbox {
        workspace: workspace_name(workspace),
        name: name.clone(),
    })
}

fn validate_answers(
    questions: &[question::Question],
    answers: Vec<AnswerInput>,
) -> Result<Vec<Answer>, ApiError> {
    if answers.len() > questions.len() {
        return Err(ApiError::invalid(
            format!("expected at most {} answers", questions.len()),
            "answers",
        ));
    }
    answers
        .into_iter()
        .zip(questions)
        .map(|(answer, question)| {
            for label in &answer.selected {
                if !question.options.iter().any(|option| &option.label == label) {
                    return Err(ApiError::invalid(
                        format!("{label:?} is not an option for {:?}", question.question),
                        "answers",
                    ));
                }
            }
            if !question.multi_select && answer.selected.len() > 1 {
                return Err(ApiError::invalid(
                    format!("{:?} allows only one option", question.question),
                    "answers",
                ));
            }
            Ok(Answer {
                selected: answer.selected,
                text: answer.text,
            })
        })
        .collect()
}

fn session_object(session: &store::Session) -> Session {
    let pending_questions = session.pending_question.as_ref().map(|pending| {
        serde_json::from_value::<PendingQuestion>(pending.clone())
            .map(|pending| pending.questions.into_iter().map(Question::from).collect())
            .unwrap_or_default()
    });
    Session {
        id: ids::encode(SESSION, session.id),
        object: "session",
        title: session.title.clone(),
        project_id: session.project_id.map(|id| ids::encode(PROJECT, id)),
        model: session.model.clone(),
        reasoning_effort: session.reasoning_effort.clone(),
        fast: session.fast,
        status: match session.status {
            SessionStatus::Idle => Status::Idle,
            SessionStatus::Running => Status::Running,
            SessionStatus::NeedsInput => Status::NeedsInput,
            SessionStatus::Failed => Status::Failed,
        },
        pending_questions,
        last_error: session.last_error.clone(),
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}

impl From<question::Question> for Question {
    fn from(question: question::Question) -> Self {
        Self {
            question: question.question,
            options: question
                .options
                .into_iter()
                .map(|option| QuestionOption {
                    label: option.label,
                    description: option.description,
                })
                .collect(),
            multi_select: question.multi_select,
        }
    }
}

fn run_object(session: &store::Session, queued: bool) -> Run {
    Run {
        object: "run",
        session: ids::encode(SESSION, session.id),
        status: "running",
        queued,
    }
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

// converts stored responses api items into trex's own item shapes for the ui
fn item(item: &Value) -> Option<Item> {
    let kind = item["type"].as_str().unwrap_or("message");
    match kind {
        "message" if history::checkpoint_text(item).is_some() => Some(Item::Compaction {
            summary: history::checkpoint_text(item)?.trim().to_owned(),
        }),
        // developer messages are the harness talking to the model, not part of the conversation
        "message" if item["role"] == "developer" => None,
        "message" => {
            if !matches!(item["content"], Value::String(_) | Value::Array(_)) {
                return None;
            }
            let text = history::message_text(item);
            let attachments = attachment::references(item)
                .into_iter()
                .map(|(kind, hash, mime_type, filename)| Attachment {
                    id: format!("{ATTACHMENT_PREFIX}{hash}"),
                    kind,
                    mime_type,
                    filename,
                })
                .collect();
            Some(Item::Message {
                role: text_of(&item["role"]),
                text,
                attachments,
            })
        }
        "function_call" => Some(Item::ToolCall {
            call_id: text_of(&item["call_id"]),
            name: text_of(&item["name"]),
            arguments: text_of(&item["arguments"]),
        }),
        "function_call_output" => Some(Item::ToolResult {
            call_id: text_of(&item["call_id"]),
            output: text_of(&item["output"]),
        }),
        "reasoning" => {
            let summary: Vec<&str> = item["summary"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect();
            (!summary.is_empty()).then(|| Item::Reasoning {
                summary: summary.join("\n\n"),
            })
        }
        _ => None,
    }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn item_json(value: &Value) -> Option<Value> {
        item(value).map(|item| serde_json::to_value(item).unwrap())
    }

    #[test]
    fn maps_stored_items_to_api_items() {
        let user = json!({"role": "user", "content": "hi"});
        let assistant = json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]});
        let call =
            json!({"type": "function_call", "call_id": "c1", "name": "bash", "arguments": "{}"});
        let output = json!({"type": "function_call_output", "call_id": "c1", "output": "ok"});
        let reasoning = json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "thinking"}], "encrypted_content": "secret"});
        let hidden = json!({"type": "reasoning", "summary": [], "encrypted_content": "secret"});

        assert_eq!(item_json(&user).unwrap()["text"], "hi");
        assert_eq!(item_json(&assistant).unwrap()["text"], "hello");
        assert_eq!(item_json(&call).unwrap()["type"], "tool_call");
        assert_eq!(item_json(&output).unwrap()["output"], "ok");
        assert_eq!(
            item_json(&reasoning).unwrap(),
            json!({"type": "reasoning", "summary": "thinking"})
        );
        assert!(item_json(&hidden).is_none());

        let checkpoint =
            serde_json::to_value(history::checkpoint(&["hi".into()], "did things")).unwrap();
        let compaction = item_json(&checkpoint).unwrap();
        assert_eq!(compaction["type"], "compaction");
        assert!(
            compaction["summary"]
                .as_str()
                .unwrap()
                .ends_with("<summary>\ndid things\n</summary>")
        );
    }
}
