use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{Admin, PageQuery, WORKSPACE_SORTS, not_found};
use crate::api::{
    AppState, List, Page,
    error::{ApiError, ErrorResponse},
    ids::{self, WORKSPACE},
};

/// A workspace, as admins see it.
#[derive(Serialize, ToSchema)]
pub struct AdminWorkspace {
    #[schema(example = "ws_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "workspace")]
    object: &'static str,
    name: String,
    plan: String,
    credits: i64,
    /// The owner's email, when it has one.
    owner_email: Option<String>,
    /// The models it may use; null means every model.
    allowed_models: Option<Vec<String>>,
    /// Unix seconds.
    created_at: i64,
}

/// List all workspaces
///
/// Every workspace on the server, newest first unless `sort` (`created_at`, `name`,
/// `owner_email`, `plan`, `credits`) says otherwise. `q` matches a workspace id, or text in the name or
/// a member's email.
#[utoipa::path(
    get,
    operation_id = "list_all_workspaces",
    path = "/admin/workspaces",
    tag = "admin",
    params(PageQuery),
    responses(
        (status = 200, body = Page<AdminWorkspace>),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
    ),
)]
pub async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<AdminWorkspace>>, ApiError> {
    let (limit, offset) = query.window()?;
    let sort = query.sort(WORKSPACE_SORTS)?;
    let id = query.search().and_then(|q| ids::decode(WORKSPACE, q));
    let search = if id.is_some() { None } else { query.search() };
    let total_count = state.store.workspace_count(search, id).await?;
    let mut workspaces = state
        .store
        .workspaces_page(search, id, sort, limit + 1, offset)
        .await?;
    let has_more = workspaces.len() as i64 > limit;
    workspaces.truncate(limit as usize);
    let data = workspaces
        .into_iter()
        .map(|workspace| AdminWorkspace {
            id: ids::encode(WORKSPACE, workspace.id),
            object: "workspace",
            name: workspace.name,
            plan: workspace.plan,
            credits: workspace.credits,
            owner_email: workspace.owner_email,
            allowed_models: workspace.allowed_models,
            created_at: workspace.created_at,
        })
        .collect();
    Ok(Json(Page::new(data, has_more, total_count)))
}

#[derive(Deserialize, ToSchema)]
pub struct AdjustCredits {
    /// Positive to add credits, negative to remove them.
    #[schema(example = 5000)]
    amount: i64,
    #[schema(example = "support top-up")]
    description: String,
}

#[derive(Deserialize, ToSchema)]
pub struct SetPlan {
    #[schema(example = "pro")]
    plan: String,
}

#[derive(Serialize, ToSchema)]
pub struct Balance {
    workspace: String,
    balance: i64,
}

/// Adjust a workspace's credits
#[utoipa::path(
    post,
    operation_id = "adjust_credits",
    path = "/admin/workspaces/{id}/credits",
    tag = "admin",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = AdjustCredits,
    responses(
        (status = 200, body = Balance),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn adjust_credits(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path(id): Path<String>,
    Json(body): Json<AdjustCredits>,
) -> Result<Json<Balance>, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    let description = body.description.trim();
    if description.is_empty() {
        return Err(ApiError::invalid(
            "say why the credits change",
            "description",
        ));
    }
    let balance = state
        .store
        .adjust_credits(workspace, body.amount, description)
        .await?
        .ok_or_else(|| not_found(&id))?;
    tracing::info!(workspace = %workspace, amount = body.amount, balance, "adjusted credits");
    Ok(Json(Balance {
        workspace: id,
        balance,
    }))
}

/// Change a workspace's plan
#[utoipa::path(
    post,
    operation_id = "set_plan",
    path = "/admin/workspaces/{id}/plan",
    tag = "admin",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = SetPlan,
    responses(
        (status = 204),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn set_plan(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path(id): Path<String>,
    Json(body): Json<SetPlan>,
) -> Result<axum::http::StatusCode, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    if state.plans.get(&body.plan).is_none() {
        return Err(ApiError::invalid(
            format!("unknown plan {}", body.plan),
            "plan",
        ));
    }
    if !state
        .store
        .set_workspace_plan(workspace, &body.plan)
        .await?
    {
        return Err(not_found(&id));
    }
    tracing::info!(workspace = %workspace, plan = body.plan, "changed plan");
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// List every model
///
/// The whole catalog, for choosing what a workspace may use.
#[utoipa::path(
    get,
    operation_id = "list_all_models",
    path = "/admin/models",
    tag = "admin",
    responses((status = 200, body = List<crate::api::Model>), (status = 403, response = ErrorResponse)),
)]
pub async fn all_models(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Json<List<crate::api::Model>> {
    Json(List::new(
        state.models.all().map(crate::api::model_object).collect(),
        false,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateWorkspace {
    /// The models it may use; null allows every model, including ones added later.
    #[schema(value_type = Option<Vec<String>>)]
    allowed_models: Option<Vec<String>>,
}

/// Change what a workspace can use
#[utoipa::path(
    patch,
    operation_id = "update_workspace",
    path = "/admin/workspaces/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = UpdateWorkspace,
    responses(
        (status = 204),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn update_workspace(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
    Json(body): Json<UpdateWorkspace>,
) -> Result<StatusCode, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    if let Some(unknown) = body
        .allowed_models
        .iter()
        .flatten()
        .find(|model| state.models.get(model).is_none())
    {
        return Err(ApiError::invalid(
            format!("unknown model {unknown}"),
            "allowed_models",
        ));
    }
    if !state
        .store
        .set_workspace_models(workspace, body.allowed_models.as_deref())
        .await?
    {
        return Err(not_found(&id));
    }
    tracing::info!(admin = %admin.user, workspace = %workspace, models = ?body.allowed_models, "changed workspace models");
    Ok(StatusCode::NO_CONTENT)
}
