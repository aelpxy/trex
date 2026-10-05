use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_store::scheduled::ScheduledTask as StoredTask;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, PROJECT, TASK},
    sessions::{Session, find_project_id, session_object},
};
use crate::{
    scheduler,
    tasks::{self, TaskSettings},
};

/// A prompt that runs on a schedule. Each run starts a new chat, listed with
/// `GET /v1/sessions?scheduled_task_id=`; the agent can't ask questions during it.
#[derive(Serialize, ToSchema)]
pub struct ScheduledTask {
    #[schema(example = "task_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "scheduled_task")]
    object: &'static str,
    #[schema(example = "Morning news digest")]
    title: String,
    prompt: String,
    model: String,
    reasoning_effort: Option<String>,
    /// A five-field cron expression, at most hourly.
    #[schema(example = "0 9 * * 1-5")]
    schedule: String,
    /// The IANA timezone the schedule is read in.
    #[schema(example = "Europe/Berlin")]
    timezone: String,
    paused: bool,
    project_id: Option<String>,
    /// Unix seconds; null while paused.
    next_run_at: Option<i64>,
    last_run_at: Option<i64>,
    /// Why the last run couldn't start, such as an empty balance.
    last_error: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn task_object(task: &StoredTask) -> ScheduledTask {
    ScheduledTask {
        id: ids::encode(TASK, task.id),
        object: "scheduled_task",
        title: task.title.clone(),
        prompt: task.prompt.clone(),
        model: task.model.clone(),
        reasoning_effort: task.reasoning_effort.clone(),
        schedule: task.schedule.clone(),
        timezone: task.timezone.clone(),
        paused: task.paused,
        project_id: task.project_id.map(|id| ids::encode(PROJECT, id)),
        next_run_at: task.next_run_at,
        last_run_at: task.last_run_at,
        last_error: task.last_error.clone(),
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTask {
    title: String,
    prompt: String,
    model: String,
    reasoning_effort: Option<String>,
    #[schema(example = "0 9 * * 1-5")]
    schedule: String,
    #[schema(example = "Europe/Berlin")]
    timezone: String,
    project_id: Option<String>,
    #[serde(default)]
    paused: bool,
}

/// Fields left out keep their value; `reasoning_effort` and `project_id` take null to clear.
#[derive(Deserialize, ToSchema)]
pub struct UpdateTask {
    title: Option<String>,
    prompt: Option<String>,
    model: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    reasoning_effort: Option<Option<String>>,
    schedule: Option<String>,
    timezone: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    project_id: Option<Option<String>>,
    paused: Option<bool>,
}

// tells an explicit null (Some(None)) apart from a missing field (None)
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

async fn find_task(state: &AppState, workspace: Uuid, id: &str) -> Result<StoredTask, ApiError> {
    let not_found = || ApiError::NotFound(format!("no scheduled task {id}"));
    let task = ids::decode(TASK, id).ok_or_else(not_found)?;
    state
        .store
        .scheduled_task(workspace, task)
        .await?
        .ok_or_else(not_found)
}

/// Create a scheduled task
#[utoipa::path(
    post,
    operation_id = "create_scheduled_task",
    path = "/scheduled_tasks",
    tag = "scheduled tasks",
    request_body = CreateTask,
    responses(
        (status = 201, body = ScheduledTask),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
        (status = 409, description = "The workspace has the most tasks it can have", body = ErrorResponse),
    ),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Json(body): Json<CreateTask>,
) -> Result<(StatusCode, Json<ScheduledTask>), ApiError> {
    let project = match body.project_id.as_deref() {
        Some(id) => Some(find_project_id(&state, workspace, id).await?),
        None => None,
    };
    let task = tasks::create(
        &state,
        workspace,
        TaskSettings {
            project,
            title: body.title,
            prompt: body.prompt,
            model: body.model,
            reasoning_effort: body.reasoning_effort,
            schedule: body.schedule,
            timezone: body.timezone,
            paused: body.paused,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(task_object(&task))))
}

/// List scheduled tasks
#[utoipa::path(
    get,
    operation_id = "list_scheduled_tasks",
    path = "/scheduled_tasks",
    tag = "scheduled tasks",
    responses((status = 200, body = List<ScheduledTask>), (status = 401, response = ErrorResponse)),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
) -> Result<Json<List<ScheduledTask>>, ApiError> {
    let tasks = state.store.scheduled_tasks(workspace).await?;
    Ok(Json(List::new(
        tasks.iter().map(task_object).collect(),
        false,
    )))
}

/// Get a scheduled task
#[utoipa::path(
    get,
    operation_id = "get_scheduled_task",
    path = "/scheduled_tasks/{id}",
    tag = "scheduled tasks",
    params(("id" = String, Path, description = "Scheduled task id")),
    responses((status = 200, body = ScheduledTask), (status = 404, response = ErrorResponse)),
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<ScheduledTask>, ApiError> {
    Ok(Json(task_object(&find_task(&state, workspace, &id).await?)))
}

/// Update a scheduled task
///
/// Pausing stops future runs; unpausing or changing the schedule picks the next run from now.
#[utoipa::path(
    patch,
    operation_id = "update_scheduled_task",
    path = "/scheduled_tasks/{id}",
    tag = "scheduled tasks",
    params(("id" = String, Path, description = "Scheduled task id")),
    request_body = UpdateTask,
    responses(
        (status = 200, body = ScheduledTask),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<UpdateTask>,
) -> Result<Json<ScheduledTask>, ApiError> {
    let task = find_task(&state, workspace, &id).await?;
    let project = match body.project_id {
        Some(Some(id)) => Some(find_project_id(&state, workspace, &id).await?),
        Some(None) => None,
        None => task.project_id,
    };
    let valid = TaskSettings {
        project,
        title: body.title.unwrap_or(task.title),
        prompt: body.prompt.unwrap_or(task.prompt),
        model: body.model.unwrap_or(task.model),
        reasoning_effort: body.reasoning_effort.unwrap_or(task.reasoning_effort),
        schedule: body.schedule.unwrap_or(task.schedule),
        timezone: body.timezone.unwrap_or(task.timezone),
        paused: body.paused.unwrap_or(task.paused),
    }
    .check(&state, workspace)
    .await?;
    let task = state
        .store
        .update_scheduled_task(workspace, task.id, &valid.fields())
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no scheduled task {id}")))?;
    Ok(Json(task_object(&task)))
}

/// Delete a scheduled task
///
/// Chats from its earlier runs are kept.
#[utoipa::path(
    delete,
    operation_id = "delete_scheduled_task",
    path = "/scheduled_tasks/{id}",
    tag = "scheduled tasks",
    params(("id" = String, Path, description = "Scheduled task id")),
    responses((status = 204), (status = 404, response = ErrorResponse)),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let task = find_task(&state, workspace, &id).await?;
    state
        .store
        .delete_scheduled_task(workspace, task.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Run a scheduled task now
///
/// Starts a run in a new chat right away; the schedule is unchanged.
#[utoipa::path(
    post,
    operation_id = "run_scheduled_task",
    path = "/scheduled_tasks/{id}/run",
    tag = "scheduled tasks",
    params(("id" = String, Path, description = "Scheduled task id")),
    responses(
        (status = 201, body = Session),
        (status = 402, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn run(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let task = find_task(&state, workspace, &id).await?;
    let session = scheduler::start(&state, &task).await?;
    Ok((StatusCode::CREATED, Json(session_object(&session))))
}
