use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::Admin;
use crate::api::{
    AppState,
    error::{ApiError, ErrorResponse},
    ids::{self, USER},
};

/// Server-wide numbers; usage covers today (UTC) and the last 30 days.
#[derive(Serialize, ToSchema)]
pub struct Overview {
    users: i64,
    admins: i64,
    workspaces: i64,
    chats: i64,
    /// Runs going right now.
    running: i64,
    /// In credits (millionths of a US dollar).
    spend_today: i64,
    tokens_today: i64,
    spend_month: i64,
    tokens_month: i64,
    /// The last 30 days' most expensive models.
    top_models: Vec<ModelUsage>,
}

#[derive(Serialize, ToSchema)]
pub struct ModelUsage {
    model: String,
    credits: i64,
    input_tokens: i64,
    output_tokens: i64,
    responses: i64,
}

fn model_usage(usage: trex_store::admin::ModelUsage) -> ModelUsage {
    ModelUsage {
        model: usage.model,
        credits: usage.credits,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        responses: usage.responses,
    }
}

/// Get the overview
#[utoipa::path(
    get,
    operation_id = "get_overview",
    path = "/admin/overview",
    tag = "admin",
    responses((status = 200, body = Overview), (status = 403, response = ErrorResponse)),
)]
pub async fn overview(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Result<Json<Overview>, ApiError> {
    let overview = state.store.overview().await?;
    Ok(Json(Overview {
        users: overview.users,
        admins: overview.admins,
        workspaces: overview.workspaces,
        chats: overview.chats,
        running: overview.running,
        spend_today: overview.spend_today,
        tokens_today: overview.tokens_today,
        spend_month: overview.spend_month,
        tokens_month: overview.tokens_month,
        top_models: overview.top_models.into_iter().map(model_usage).collect(),
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UsageQuery {
    /// How many days back, 1 to 365.
    #[param(default = 30, minimum = 1, maximum = 365)]
    days: Option<i32>,
}

const DEFAULT_USAGE_DAYS: i32 = 30;
const MAX_USAGE_DAYS: i32 = 365;

#[derive(Serialize, ToSchema)]
pub struct UsageReport {
    days: i32,
    /// Days with usage, oldest first.
    daily: Vec<DayUsage>,
    models: Vec<ModelUsage>,
    /// The 20 accounts whose workspaces spent the most; a workspace counts for its owner.
    accounts: Vec<AccountUsage>,
}

#[derive(Serialize, ToSchema)]
pub struct DayUsage {
    /// Unix seconds at midnight UTC.
    day: i64,
    credits: i64,
    input_tokens: i64,
    output_tokens: i64,
    responses: i64,
}

#[derive(Serialize, ToSchema)]
pub struct AccountUsage {
    user: String,
    name: String,
    email: String,
    credits: i64,
    tokens: i64,
    responses: i64,
}

/// Get usage
#[utoipa::path(
    get,
    operation_id = "get_usage_report",
    path = "/admin/usage",
    tag = "admin",
    params(UsageQuery),
    responses(
        (status = 200, body = UsageReport),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
    ),
)]
pub async fn usage(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageReport>, ApiError> {
    let days = query.days.unwrap_or(DEFAULT_USAGE_DAYS);
    if !(1..=MAX_USAGE_DAYS).contains(&days) {
        return Err(ApiError::invalid(
            format!("days must be between 1 and {MAX_USAGE_DAYS}"),
            "days",
        ));
    }
    let report = state.store.usage_report(days).await?;
    Ok(Json(UsageReport {
        days,
        daily: report
            .days
            .into_iter()
            .map(|day| DayUsage {
                day: day.day,
                credits: day.credits,
                input_tokens: day.input_tokens,
                output_tokens: day.output_tokens,
                responses: day.responses,
            })
            .collect(),
        models: report.models.into_iter().map(model_usage).collect(),
        accounts: report
            .accounts
            .into_iter()
            .map(|account| AccountUsage {
                user: ids::encode(USER, account.user),
                name: account.name,
                email: account.email,
                credits: account.credits,
                tokens: account.tokens,
                responses: account.responses,
            })
            .collect(),
    }))
}
