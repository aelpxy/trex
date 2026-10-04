use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use trex_harness::{
    history,
    model::parse_effort,
    question::{self, Answer, Question},
};
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::sessions::{Session, SessionStatus};
use uuid::Uuid;

use super::{
    AppState,
    auth::CurrentUser,
    error::ApiError,
    ids::{self, SESSION},
};
use crate::runs;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize)]
pub struct CreateSession {
    model: String,
    reasoning_effort: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    limit: Option<i64>,
    starting_after: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateMessage {
    content: String,
}

#[derive(Deserialize)]
pub struct CreateAnswers {
    answers: Vec<AnswerInput>,
}

#[derive(Deserialize)]
pub struct AnswerInput {
    #[serde(default)]
    selected: Vec<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
pub struct RejectAccess {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct PendingQuestion {
    call_id: String,
    questions: Vec<Question>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Json(body): Json<CreateSession>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
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
    Ok((StatusCode::CREATED, Json(session_json(&session))))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
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
    let data: Vec<Value> = sessions.iter().map(session_json).collect();
    Ok(Json(
        json!({"object": "list", "data": data, "has_more": has_more}),
    ))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    Ok(Json(session_json(&session)))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    state.runs.cancel(session.id);
    if let Some(sandbox) = sandbox_of(user, &session) {
        state.openshell.delete(&sandbox).await?;
    }
    state.store.delete_session(user, session.id).await?;
    state.store.delete_events(session.id).await?;
    Ok(Json(
        json!({"id": ids::encode(SESSION, session.id), "object": "session", "deleted": true}),
    ))
}

pub async fn items(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let items = state.store.session_items(user, session.id).await?;
    let data: Vec<Value> = items.iter().filter_map(item_json).collect();
    Ok(Json(
        json!({"object": "list", "data": data, "has_more": false}),
    ))
}

pub async fn create_message(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<CreateMessage>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if body.content.trim().is_empty() {
        return Err(ApiError::invalid("content must not be empty", "content"));
    }
    let session = find_session(&state, user, &id).await?;
    let input = history::to_json(&[history::user_message(&body.content)])?;
    runs::start(&state, user, &session, input).await?;
    Ok((StatusCode::ACCEPTED, Json(run_json(&session))))
}

pub async fn create_answers(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<CreateAnswers>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
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
    Ok((StatusCode::ACCEPTED, Json(run_json(&session))))
}

pub async fn cancel(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let session = find_session(&state, user, &id).await?;
    if !state.runs.cancel(session.id) {
        return Err(ApiError::Conflict("no run is in progress".into()));
    }
    Ok((StatusCode::ACCEPTED, Json(session_json(&session))))
}

// denials are batched into requests every few seconds, so ones raised at the end of a run
// arrive after its live watcher stopped; the ui lists them here at any time
pub async fn list_access(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let requests = match sandbox_of(user, &session) {
        Some(sandbox) => state.openshell.pending_access(&sandbox).await?,
        None => Vec::new(),
    };
    let data: Vec<Value> = requests
        .iter()
        .map(|request| {
            json!({
                "id": request.id,
                "object": "access_request",
                "endpoints": request.endpoints,
                "binary": request.binary,
                "rationale": request.rationale,
                "security_notes": request.security_notes,
            })
        })
        .collect();
    Ok(Json(
        json!({"object": "list", "data": data, "has_more": false}),
    ))
}

pub async fn approve_access(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path((id, request_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let (sandbox, request) = find_access_request(&state, user, &session, &request_id).await?;
    state.openshell.approve_access(&sandbox, &request).await?;
    Ok(Json(
        json!({"id": request.id, "object": "access_request", "status": "approved"}),
    ))
}

pub async fn reject_access(
    State(state): State<Arc<AppState>>,
    CurrentUser(user): CurrentUser,
    Path((id, request_id)): Path<(String, String)>,
    body: Option<Json<RejectAccess>>,
) -> Result<Json<Value>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let (sandbox, request) = find_access_request(&state, user, &session, &request_id).await?;
    let reason = body
        .and_then(|Json(body)| body.reason)
        .unwrap_or_else(|| "rejected by the user".into());
    state
        .openshell
        .reject_access(&sandbox, &request, &reason)
        .await?;
    Ok(Json(
        json!({"id": request.id, "object": "access_request", "status": "rejected"}),
    ))
}

pub async fn find_session(state: &AppState, user: Uuid, id: &str) -> Result<Session, ApiError> {
    let not_found = || ApiError::NotFound(format!("no session {id}"));
    let uuid = ids::decode(SESSION, id).ok_or_else(not_found)?;
    state.store.session(user, uuid).await?.ok_or_else(not_found)
}

async fn find_access_request(
    state: &AppState,
    user: Uuid,
    session: &Session,
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
fn sandbox_of(user: Uuid, session: &Session) -> Option<Sandbox> {
    session.sandbox.as_ref().map(|name| Sandbox {
        workspace: workspace_name(user),
        name: name.clone(),
    })
}

fn validate_answers(
    questions: &[Question],
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

fn session_json(session: &Session) -> Value {
    let questions = session
        .pending_question
        .as_ref()
        .map(|pending| pending["questions"].clone());
    json!({
        "id": ids::encode(SESSION, session.id),
        "object": "session",
        "model": session.model,
        "reasoning_effort": session.reasoning_effort,
        "status": session.status.as_str(),
        "pending_questions": questions,
        "last_error": session.last_error,
        "created_at": session.created_at,
        "updated_at": session.updated_at,
    })
}

fn run_json(session: &Session) -> Value {
    json!({"object": "run", "session": ids::encode(SESSION, session.id), "status": "running"})
}

// converts stored responses api items into trex's own item shapes for the ui
fn item_json(item: &Value) -> Option<Value> {
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
            Some(json!({"type": "message", "role": item["role"], "text": text}))
        }
        "function_call" => Some(json!({
            "type": "tool_call",
            "call_id": item["call_id"],
            "name": item["name"],
            "arguments": item["arguments"],
        })),
        "function_call_output" => Some(json!({
            "type": "tool_result",
            "call_id": item["call_id"],
            "output": item["output"],
        })),
        "reasoning" => {
            let summary: Vec<&str> = item["summary"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect();
            (!summary.is_empty())
                .then(|| json!({"type": "reasoning", "summary": summary.join("\n\n")}))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
