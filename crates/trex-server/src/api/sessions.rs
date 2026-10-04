use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use trex_harness::{
    history,
    model::parse_effort,
    question::{self, Answer},
};
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::sessions::{self as store, SessionStatus};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{
    AppState, List,
    auth::CurrentUser,
    error::{ApiError, ErrorResponse},
    ids::{self, SESSION},
};
use crate::runs;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize, ToSchema)]
pub struct CreateSession {
    /// A model id from `GET /v1/models`.
    #[schema(example = "gpt-6.1-sol")]
    model: String,
    /// One of `none`, `minimal`, `low`, `medium`, `high`, `xhigh`; the provider default when omitted.
    #[schema(example = "medium")]
    reasoning_effort: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Page size, 1 to 100.
    #[param(default = 20, minimum = 1, maximum = 100)]
    limit: Option<i64>,
    /// A session id; returns the sessions created before it.
    starting_after: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateMessage {
    #[schema(example = "Plot the CSV in my library")]
    content: String,
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
    model: String,
    reasoning_effort: Option<String>,
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
}

/// A conversation item, rendered by `type`.
#[derive(Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Message {
        #[schema(example = "assistant")]
        role: String,
        text: String,
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
    CurrentUser(user): CurrentUser,
    Json(body): Json<CreateSession>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    if state.models.get(&body.model).is_none() {
        return Err(ApiError::invalid(
            format!("unknown model {}", body.model),
            "model",
        ));
    }
    if let Some(effort) = &body.reasoning_effort {
        parse_effort(effort)
            .map_err(|error| ApiError::invalid(error.to_string(), "reasoning_effort"))?;
    }
    let session = state
        .store
        .create_session(user, &body.model, body.reasoning_effort.as_deref())
        .await?;
    Ok((StatusCode::CREATED, Json(session_object(&session))))
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
    CurrentUser(user): CurrentUser,
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
    // one extra row tells whether another page exists
    let mut sessions = state.store.sessions(user, limit + 1, before).await?;
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Session>, ApiError> {
    let session = find_session(&state, user, &id).await?;
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<DeletedSession>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    state.runs.cancel(session.id);
    if let Some(sandbox) = sandbox_of(user, &session) {
        state.openshell.delete(&sandbox).await?;
    }
    state.store.delete_session(user, session.id).await?;
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
        (status = 200, body = List<Item>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn items(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<List<Item>>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let items = state.store.session_items(user, session.id).await?;
    Ok(Json(List::new(
        items.iter().filter_map(item).collect(),
        false,
    )))
}

/// Send a message
///
/// Starts a run; follow it on the events stream.
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
        (status = 409, description = "A run is already in progress", body = ErrorResponse),
    ),
)]
pub async fn create_message(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<CreateMessage>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    if body.content.trim().is_empty() {
        return Err(ApiError::invalid("content must not be empty", "content"));
    }
    let session = find_session(&state, user, &id).await?;
    let input = history::to_json(&[history::user_message(&body.content)])?;
    runs::start(&state, user, &session, input).await?;
    Ok((StatusCode::ACCEPTED, Json(run_object(&session))))
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<CreateAnswers>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    let session = find_session(&state, user, &id).await?;
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
    runs::start(&state, user, &session, input).await?;
    Ok((StatusCode::ACCEPTED, Json(run_object(&session))))
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let session = find_session(&state, user, &id).await?;
    if !state.runs.cancel(session.id) {
        return Err(ApiError::Conflict("no run is in progress".into()));
    }
    Ok((StatusCode::ACCEPTED, Json(session_object(&session))))
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<List<AccessRequest>>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let requests = match sandbox_of(user, &session) {
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
    CurrentUser(user): CurrentUser,
    Path((id, request_id)): Path<(String, String)>,
) -> Result<Json<AccessRequest>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let (sandbox, request) = find_access_request(&state, user, &session, &request_id).await?;
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
    CurrentUser(user): CurrentUser,
    Path((id, request_id)): Path<(String, String)>,
    body: Option<Json<RejectAccess>>,
) -> Result<Json<AccessRequest>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let (sandbox, request) = find_access_request(&state, user, &session, &request_id).await?;
    let reason = body
        .and_then(|Json(body)| body.reason)
        .unwrap_or_else(|| "rejected by the user".into());
    state
        .openshell
        .reject_access(&sandbox, &request, &reason)
        .await?;
    Ok(Json(access_request(request, AccessStatus::Rejected)))
}

pub async fn find_session(
    state: &AppState,
    user: Uuid,
    id: &str,
) -> Result<store::Session, ApiError> {
    let not_found = || ApiError::NotFound(format!("no session {id}"));
    let uuid = ids::decode(SESSION, id).ok_or_else(not_found)?;
    state.store.session(user, uuid).await?.ok_or_else(not_found)
}

async fn find_access_request(
    state: &AppState,
    user: Uuid,
    session: &store::Session,
    request_id: &str,
) -> Result<(Sandbox, trex_sandbox::AccessRequest), ApiError> {
    let not_found = || ApiError::NotFound(format!("no pending access request {request_id}"));
    let sandbox = sandbox_of(user, session).ok_or_else(not_found)?;
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
fn sandbox_of(user: Uuid, session: &store::Session) -> Option<Sandbox> {
    session.sandbox.as_ref().map(|name| Sandbox {
        workspace: workspace_name(user),
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
        model: session.model.clone(),
        reasoning_effort: session.reasoning_effort.clone(),
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

fn run_object(session: &store::Session) -> Run {
    Run {
        object: "run",
        session: ids::encode(SESSION, session.id),
        status: "running",
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
        "message" => {
            let text = match &item["content"] {
                Value::String(text) => text.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(""),
                _ => return None,
            };
            Some(Item::Message {
                role: text_of(&item["role"]),
                text,
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
    }
}
