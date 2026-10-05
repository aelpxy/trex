pub mod access;
pub mod items;
pub mod messages;
pub mod questions;

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_harness::history;
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::sessions::{self as store, SessionFilter, SessionStatus};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, PROJECT, SESSION, TASK},
};
use crate::runs;

pub use self::questions::Question;
use self::{
    items::{Attachment, attachments_of},
    questions::PendingQuestion,
};

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
    /// Approve the sandbox's network access requests without asking.
    #[serde(default)]
    auto_approve: bool,
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
    /// A scheduled task's runs; without it, runs of scheduled tasks aren't listed.
    scheduled_task_id: Option<String>,
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
    auto_approve: Option<bool>,
}

// tells an explicit null (Some(None)) apart from a missing field (None)
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
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
    /// The scheduled task this chat is a run of; null for chats people started.
    scheduled_task_id: Option<String>,
    model: String,
    reasoning_effort: Option<String>,
    /// Faster responses at a higher cost.
    fast: bool,
    /// The sandbox's network access requests are approved without asking; each still appears as
    /// `access.requested`, followed by `access.decided` with `automatic: true`.
    auto_approve: bool,
    status: Status,
    /// Set while `status` is `needs_input`; answer with `POST /v1/sessions/{id}/answers`.
    pending_questions: Option<Vec<Question>>,
    /// Why the last run failed, when `status` is `failed`.
    last_error: Option<String>,
    /// Messages sent during the current run that the agent will read next, oldest first.
    queued_messages: Vec<QueuedMessage>,
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
pub struct DeletedSession {
    id: String,
    #[schema(example = "session")]
    object: &'static str,
    deleted: bool,
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
    runs::require_model(&state, workspace, &body.model).await?;
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
    let session = if body.auto_approve {
        state
            .store
            .set_session_auto_approve(workspace, session.id, true)
            .await?
            .unwrap_or(session)
    } else {
        session
    };
    Ok((StatusCode::CREATED, Json(session_object(&session))))
}

pub(crate) fn check_settings(
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
    let task = query
        .scheduled_task_id
        .as_deref()
        .map(|id| {
            ids::decode(TASK, id)
                .ok_or_else(|| ApiError::invalid("invalid scheduled task id", "scheduled_task_id"))
        })
        .transpose()?;
    let filter = match (task, query.project_id.as_deref()) {
        (Some(task), _) => SessionFilter::ScheduledTask(task),
        (None, None) => SessionFilter::All,
        (None, Some("none")) => SessionFilter::NoProject,
        (None, Some(id)) => SessionFilter::Project(
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
        runs::require_model(&state, workspace, model_id).await?;
        state
            .store
            .set_session_model(workspace, session.id, model_id, effort, fast)
            .await?;
    }
    if let Some(auto_approve) = body.auto_approve {
        state
            .store
            .set_session_auto_approve(workspace, session.id, auto_approve)
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

pub(super) async fn find_project_id(
    state: &AppState,
    workspace: Uuid,
    id: &str,
) -> Result<Uuid, ApiError> {
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

// a session's sandbox always lives in its owner's workspace
fn sandbox_of(workspace: Uuid, session: &store::Session) -> Option<Sandbox> {
    session.sandbox.as_ref().map(|name| Sandbox {
        workspace: workspace_name(workspace),
        name: name.clone(),
    })
}

#[derive(Serialize, ToSchema)]
pub struct QueuedMessage {
    content: String,
    attachments: Vec<Attachment>,
}

pub(super) fn session_object(session: &store::Session) -> Session {
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
        scheduled_task_id: session.scheduled_task_id.map(|id| ids::encode(TASK, id)),
        model: session.model.clone(),
        reasoning_effort: session.reasoning_effort.clone(),
        fast: session.fast,
        auto_approve: session.auto_approve,
        status: match session.status {
            SessionStatus::Idle => Status::Idle,
            SessionStatus::Running => Status::Running,
            SessionStatus::NeedsInput => Status::NeedsInput,
            SessionStatus::Failed => Status::Failed,
        },
        pending_questions,
        last_error: session.last_error.clone(),
        queued_messages: session
            .queued_messages
            .iter()
            .map(|message| QueuedMessage {
                content: history::message_text(message),
                attachments: attachments_of(message),
            })
            .collect(),
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}
