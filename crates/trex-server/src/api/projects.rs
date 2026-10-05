use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_store::projects as store;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, PROJECT},
};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;
const MAX_NAME_CHARS: usize = 100;
const MAX_INSTRUCTIONS_CHARS: usize = 20_000;

#[derive(Deserialize, ToSchema)]
pub struct CreateProject {
    #[schema(example = "Trex backend")]
    name: String,
    /// Followed by every chat in the project, on top of the agent's own instructions.
    #[schema(example = "The code is Rust; run cargo clippy before finishing.")]
    instructions: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProject {
    name: Option<String>,
    /// New instructions, or null to remove them.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    instructions: Option<Option<String>>,
}

// tells an explicit null (Some(None)) apart from a missing field (None)
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Page size, 1 to 100.
    #[param(default = 50, minimum = 1, maximum = 100)]
    limit: Option<i64>,
    /// A project id; returns the projects created before it.
    starting_after: Option<String>,
}

/// A group of chats; list them with `GET /v1/sessions?project_id=`.
#[derive(Serialize, ToSchema)]
pub struct Project {
    #[schema(example = "proj_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "project")]
    object: &'static str,
    name: String,
    instructions: Option<String>,
    /// Unix seconds.
    created_at: i64,
    /// Unix seconds.
    updated_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DeletedProject {
    id: String,
    #[schema(example = "project")]
    object: &'static str,
    deleted: bool,
}

/// Create a project
#[utoipa::path(
    post,
    operation_id = "create_project",
    path = "/projects",
    tag = "projects",
    request_body = CreateProject,
    responses((status = 201, body = Project), (status = 400, response = ErrorResponse)),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Json(body): Json<CreateProject>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let name = valid_name(&body.name)?;
    let instructions = body
        .instructions
        .as_deref()
        .map(valid_instructions)
        .transpose()?;
    let project = state
        .store
        .create_project(workspace, &name, instructions.flatten().as_deref())
        .await?;
    Ok((StatusCode::CREATED, Json(project_object(&project))))
}

/// List projects
///
/// Newest first.
#[utoipa::path(
    get,
    operation_id = "list_projects",
    path = "/projects",
    tag = "projects",
    params(ListQuery),
    responses((status = 200, body = List<Project>), (status = 400, response = ErrorResponse)),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Query(query): Query<ListQuery>,
) -> Result<Json<List<Project>>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::invalid(
            format!("limit must be between 1 and {MAX_LIMIT}"),
            "limit",
        ));
    }
    let before = query
        .starting_after
        .as_deref()
        .map(|cursor| {
            ids::decode(PROJECT, cursor)
                .ok_or_else(|| ApiError::invalid("invalid project id", "starting_after"))
        })
        .transpose()?;
    let mut projects = state.store.projects(workspace, limit + 1, before).await?;
    let has_more = projects.len() as i64 > limit;
    projects.truncate(limit as usize);
    Ok(Json(List::new(
        projects.iter().map(project_object).collect(),
        has_more,
    )))
}

/// Get a project
#[utoipa::path(
    get,
    operation_id = "get_project",
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = Project), (status = 404, response = ErrorResponse)),
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<Project>, ApiError> {
    let project = state
        .store
        .project(workspace, decode(&id)?)
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(project_object(&project)))
}

/// Update a project
#[utoipa::path(
    patch,
    operation_id = "update_project",
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    request_body = UpdateProject,
    responses(
        (status = 200, body = Project),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<UpdateProject>,
) -> Result<Json<Project>, ApiError> {
    let name = body.name.as_deref().map(valid_name).transpose()?;
    let instructions = match body.instructions {
        Some(Some(text)) => Some(valid_instructions(&text)?),
        Some(None) => Some(None),
        None => None,
    };
    let project = state
        .store
        .update_project(
            workspace,
            decode(&id)?,
            name.as_deref(),
            instructions.as_ref().map(Option::as_deref),
        )
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(project_object(&project)))
}

/// Delete a project
///
/// Its chats are kept and move out of the project.
#[utoipa::path(
    delete,
    operation_id = "delete_project",
    path = "/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Project id")),
    responses((status = 200, body = DeletedProject), (status = 404, response = ErrorResponse)),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<DeletedProject>, ApiError> {
    if !state.store.delete_project(workspace, decode(&id)?).await? {
        return Err(not_found(&id));
    }
    Ok(Json(DeletedProject {
        id,
        object: "project",
        deleted: true,
    }))
}

fn decode(id: &str) -> Result<Uuid, ApiError> {
    ids::decode(PROJECT, id).ok_or_else(|| not_found(id))
}

fn not_found(id: &str) -> ApiError {
    ApiError::NotFound(format!("no project {id}"))
}

fn valid_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::invalid(
            format!("name must be 1 to {MAX_NAME_CHARS} characters"),
            "name",
        ));
    }
    Ok(name.to_owned())
}

// blank instructions are the same as none
fn valid_instructions(text: &str) -> Result<Option<String>, ApiError> {
    let text = text.trim();
    if text.chars().count() > MAX_INSTRUCTIONS_CHARS {
        return Err(ApiError::invalid(
            format!("instructions must be at most {MAX_INSTRUCTIONS_CHARS} characters"),
            "instructions",
        ));
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

fn project_object(project: &store::Project) -> Project {
    Project {
        id: ids::encode(PROJECT, project.id),
        object: "project",
        name: project.name.clone(),
        instructions: project.instructions.clone(),
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
}
