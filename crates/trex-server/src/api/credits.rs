use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    ids::{self, CREDIT_ENTRY},
};
use crate::credits;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

/// The workspace's credits. Every model response is charged by the model's `price`; the plan
/// tops the balance up to `monthly_credits` once a month.
#[derive(Serialize, ToSchema)]
pub struct Credits {
    #[schema(example = "credits")]
    object: &'static str,
    balance: i64,
    plan: Option<Plan>,
    /// Whether messages are refused at zero; off when the server has no plans configured.
    enforced: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Plan {
    #[schema(example = "free")]
    id: String,
    #[schema(example = "Free")]
    name: String,
    monthly_credits: i64,
}

#[derive(Serialize, ToSchema)]
pub struct LedgerEntry {
    #[schema(example = "cred_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "credit_entry")]
    object: &'static str,
    /// Positive for grants and top-ups, negative for usage.
    amount: i64,
    /// The balance right after this entry.
    balance: i64,
    /// `grant`, `usage` or `adjustment`.
    kind: String,
    description: String,
    /// Unix seconds.
    created_at: i64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LedgerQuery {
    /// Page size, 1 to 100.
    #[param(default = 50, minimum = 1, maximum = 100)]
    limit: Option<i64>,
    /// An entry id; returns older entries.
    starting_after: Option<String>,
}

/// Get credits
#[utoipa::path(
    get,
    operation_id = "get_credits",
    path = "/credits",
    tag = "credits",
    responses((status = 200, body = Credits), (status = 401, response = ErrorResponse)),
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
) -> Result<Json<Credits>, ApiError> {
    let enforced = state.plans.enforced();
    let balance = if enforced {
        credits::refilled_balance(&state, workspace).await?
    } else {
        state.store.credit_balance(workspace).await?
    };
    let plan_id = state
        .store
        .workspace_plan(workspace)
        .await?
        .unwrap_or_default();
    let plan = state.plans.get(&plan_id).map(|plan| Plan {
        id: plan_id.clone(),
        name: plan.name.clone(),
        monthly_credits: plan.monthly_credits,
    });
    Ok(Json(Credits {
        object: "credits",
        balance,
        plan,
        enforced,
    }))
}

/// List credit history
///
/// Grants, usage charges and adjustments, newest first.
#[utoipa::path(
    get,
    operation_id = "list_credit_entries",
    path = "/credits/ledger",
    tag = "credits",
    params(LedgerQuery),
    responses((status = 200, body = List<LedgerEntry>), (status = 400, response = ErrorResponse)),
)]
pub async fn ledger(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<List<LedgerEntry>>, ApiError> {
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
            ids::decode(CREDIT_ENTRY, cursor)
                .ok_or_else(|| ApiError::invalid("invalid entry id", "starting_after"))
        })
        .transpose()?;
    let mut entries = state.store.ledger(workspace, limit + 1, before).await?;
    let has_more = entries.len() as i64 > limit;
    entries.truncate(limit as usize);
    let data = entries
        .into_iter()
        .map(|entry| LedgerEntry {
            id: ids::encode(CREDIT_ENTRY, entry.id),
            object: "credit_entry",
            amount: entry.amount,
            balance: entry.balance,
            kind: entry.kind,
            description: entry.description,
            created_at: entry.created_at,
        })
        .collect();
    Ok(Json(List::new(data, has_more)))
}
