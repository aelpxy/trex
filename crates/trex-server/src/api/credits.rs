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
    /// Credits are millionths of a US dollar: 1,000,000 is $1.
    balance: i64,
    plan: Option<Plan>,
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
    /// In credits (millionths of a US dollar); positive for grants and top-ups, negative for usage.
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
    /// A page number, from 1, of `limit` entries each; for paged views. Can't be combined with
    /// `starting_after`.
    #[param(minimum = 1)]
    page: Option<i64>,
}

/// A page of the ledger, newest first.
#[derive(Serialize, ToSchema)]
pub struct LedgerPage {
    #[schema(example = "list")]
    object: &'static str,
    data: Vec<LedgerEntry>,
    has_more: bool,
    /// Entries in the whole ledger.
    total_count: i64,
}

/// List plans
///
/// The plans this server offers, sorted by monthly credits.
#[utoipa::path(
    get,
    operation_id = "list_plans",
    path = "/plans",
    tag = "credits",
    responses((status = 200, body = List<Plan>), (status = 401, response = ErrorResponse)),
)]
pub async fn plans(
    State(state): State<Arc<AppState>>,
    Auth { .. }: Auth,
) -> Result<Json<List<Plan>>, ApiError> {
    let mut data: Vec<Plan> = state
        .plans
        .ids()
        .filter_map(|id| {
            state.plans.get(id).map(|plan| Plan {
                id: id.to_owned(),
                name: plan.name.clone(),
                monthly_credits: plan.monthly_credits,
            })
        })
        .collect();
    data.sort_by(|a, b| {
        a.monthly_credits
            .cmp(&b.monthly_credits)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(Json(List::new(data, false)))
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
    let balance = credits::refilled_balance(&state, workspace).await?;
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
    }))
}

#[derive(Serialize, ToSchema)]
pub struct MonthUsage {
    #[schema(example = "usage")]
    object: &'static str,
    /// Unix seconds, when this month began; plans top up for the same month.
    period_start: i64,
    /// Credits spent this month, in millionths of a US dollar.
    credits: i64,
    input_tokens: i64,
    output_tokens: i64,
    responses: i64,
    /// Per model, most spent first.
    models: Vec<ModelUsage>,
}

#[derive(Serialize, ToSchema)]
pub struct ModelUsage {
    model: String,
    /// The model's display name from the catalog, or its id if it's gone.
    name: String,
    credits: i64,
    input_tokens: i64,
    output_tokens: i64,
    responses: i64,
}

/// Get this month's usage
///
/// What the workspace spent this month, in total and per model.
#[utoipa::path(
    get,
    operation_id = "get_month_usage",
    path = "/credits/usage",
    tag = "credits",
    responses((status = 200, body = MonthUsage), (status = 401, response = ErrorResponse)),
)]
pub async fn usage(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
) -> Result<Json<MonthUsage>, ApiError> {
    let (period_start, models) = state.store.month_usage(workspace).await?;
    Ok(Json(MonthUsage {
        object: "usage",
        period_start,
        credits: models.iter().map(|model| model.credits).sum(),
        input_tokens: models.iter().map(|model| model.input_tokens).sum(),
        output_tokens: models.iter().map(|model| model.output_tokens).sum(),
        responses: models.iter().map(|model| model.responses).sum(),
        models: models
            .into_iter()
            .map(|usage| ModelUsage {
                name: state
                    .models
                    .get(&usage.model)
                    .map_or_else(|| usage.model.clone(), |model| model.name().to_owned()),
                model: usage.model,
                credits: usage.credits,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                responses: usage.responses,
            })
            .collect(),
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
    responses((status = 200, body = LedgerPage), (status = 400, response = ErrorResponse)),
)]
pub async fn ledger(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<LedgerPage>, ApiError> {
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
    let offset = match (query.page, before) {
        (Some(page), _) if page < 1 => return Err(ApiError::invalid("page starts at 1", "page")),
        (Some(_), Some(_)) => {
            return Err(ApiError::invalid(
                "use page or starting_after, not both",
                "page",
            ));
        }
        (Some(page), None) => (page - 1) * limit,
        (None, _) => 0,
    };
    let total_count = state.store.ledger_count(workspace).await?;
    let mut entries = state
        .store
        .ledger(workspace, limit + 1, before, offset)
        .await?;
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
    Ok(Json(LedgerPage {
        object: "list",
        data,
        has_more,
        total_count,
    }))
}
