use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRequestParts, Path, State},
    http::{header, request::Parts},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    AppState,
    auth::hash_token,
    error::{ApiError, ErrorResponse},
    ids::{self, WORKSPACE},
};

// the operator, authenticated by TREX_ADMIN_TOKEN; without one the admin endpoints are off
pub struct Admin;

impl FromRequestParts<Arc<AppState>> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(expected) = &state.admin_token else {
            return Err(ApiError::Permission("the admin api is disabled".into()));
        };
        let given = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        // comparing hashes keeps the comparison time independent of the secret
        if hash_token(given.trim()) != hash_token(expected) {
            return Err(ApiError::Permission("not an admin token".into()));
        }
        Ok(Self)
    }
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
