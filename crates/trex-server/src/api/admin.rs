use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRequestParts, Path, State},
    http::request::Parts,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use trex_store::accounts::UserRole;

use super::{
    AppState, List,
    auth::Account,
    error::{ApiError, ErrorResponse},
    ids::{self, WORKSPACE},
};

// a signed-in user with the admin role; admins are made with `trex admin grant <email>`
pub struct Admin;

impl FromRequestParts<Arc<AppState>> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let account = Account::from_request_parts(parts, state).await?;
        let role = state
            .store
            .user(account.user)
            .await?
            .map(|(user, _)| user.role);
        if role != Some(UserRole::Admin) {
            return Err(ApiError::Permission("only admins can do this".into()));
        }
        Ok(Self)
    }
}

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
    /// Unix seconds.
    created_at: i64,
}

/// List all workspaces
///
/// Every workspace on the server, newest first.
#[utoipa::path(
    get,
    operation_id = "list_all_workspaces",
    path = "/admin/workspaces",
    tag = "admin",
    responses(
        (status = 200, body = List<AdminWorkspace>),
        (status = 403, response = ErrorResponse),
    ),
)]
pub async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Result<Json<List<AdminWorkspace>>, ApiError> {
    let data = state
        .store
        .all_workspaces()
        .await?
        .into_iter()
        .map(|workspace| AdminWorkspace {
            id: ids::encode(WORKSPACE, workspace.id),
            object: "workspace",
            name: workspace.name,
            plan: workspace.plan,
            credits: workspace.credits,
            owner_email: workspace.owner_email,
            created_at: workspace.created_at,
        })
        .collect();
    Ok(Json(List::new(data, false)))
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

fn not_found(id: &str) -> ApiError {
    ApiError::NotFound(format!("no workspace {id}"))
}
