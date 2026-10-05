use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, header, request::Parts},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use trex_harness::attachment;
use utoipa::{IntoParams, ToSchema};

use trex_store::accounts::UserRole;

use super::{
    AppState, List,
    auth::Account,
    error::{ApiError, ErrorResponse},
    ids::{self, USER, WORKSPACE},
    library::{File, FileContent, file, library_error},
};

// a signed-in user with the admin role; admins are made with `trex admin grant <email>`
pub struct Admin {
    pub user: uuid::Uuid,
    pub session: uuid::Uuid,
}

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
        Ok(Self {
            user: account.user,
            session: account.session,
        })
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
    /// The models it may use; null means every model.
    allowed_models: Option<Vec<String>>,
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
            allowed_models: workspace.allowed_models,
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
    /// The 20 workspaces that spent the most.
    workspaces: Vec<WorkspaceUsage>,
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
pub struct WorkspaceUsage {
    workspace: String,
    name: String,
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
        workspaces: report
            .workspaces
            .into_iter()
            .map(|workspace| WorkspaceUsage {
                workspace: ids::encode(WORKSPACE, workspace.workspace),
                name: workspace.name,
                credits: workspace.credits,
                tokens: workspace.tokens,
                responses: workspace.responses,
            })
            .collect(),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct AdminUser {
    #[schema(example = "user_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "user")]
    object: &'static str,
    email: String,
    name: String,
    role: super::account::Role,
    workspaces: i64,
    /// Unix seconds.
    created_at: i64,
    /// Unix seconds, the last request from any of their signed-in devices.
    last_active_at: Option<i64>,
}

/// List users
///
/// Every user, newest first.
#[utoipa::path(
    get,
    operation_id = "list_users",
    path = "/admin/users",
    tag = "admin",
    responses((status = 200, body = List<AdminUser>), (status = 403, response = ErrorResponse)),
)]
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    _: Admin,
) -> Result<Json<List<AdminUser>>, ApiError> {
    let data = state
        .store
        .all_users()
        .await?
        .into_iter()
        .map(|user| AdminUser {
            id: ids::encode(USER, user.id),
            object: "user",
            email: user.email,
            name: user.name,
            role: super::account::Role::from(user.role),
            workspaces: user.workspaces,
            created_at: user.created_at,
            last_active_at: user.last_active_at,
        })
        .collect();
    Ok(Json(List::new(data, false)))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    role: super::account::Role,
}

/// Change a user's role
///
/// Admins can't change their own role, so there is always one left.
#[utoipa::path(
    patch,
    operation_id = "update_user",
    path = "/admin/users/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    request_body = UpdateUser,
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, response = ErrorResponse),
    ),
)]
pub async fn update_user(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
    Json(body): Json<UpdateUser>,
) -> Result<StatusCode, ApiError> {
    let not_found = || ApiError::NotFound(format!("no user {id}"));
    let user = ids::decode(USER, &id).ok_or_else(not_found)?;
    if user == admin.user {
        return Err(ApiError::Conflict("you can't change your own role".into()));
    }
    let role = UserRole::from(body.role);
    if !state.store.set_user_role_by_id(user, role).await? {
        return Err(not_found());
    }
    tracing::info!(admin = %admin.user, user = %user, role = role.as_str(), "changed user role");
    Ok(StatusCode::NO_CONTENT)
}

/// Sign a user out everywhere
#[utoipa::path(
    post,
    operation_id = "sign_out_user",
    path = "/admin/users/{id}/sign_out",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn sign_out_user(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = ids::decode(USER, &id).ok_or_else(|| ApiError::NotFound(format!("no user {id}")))?;
    let ended = state.store.delete_user_sessions(user, None).await?;
    tracing::info!(admin = %admin.user, user = %user, ended, "signed a user out everywhere");
    Ok(StatusCode::NO_CONTENT)
}

/// List a workspace's library
#[utoipa::path(
    get,
    operation_id = "list_workspace_library",
    path = "/admin/workspaces/{id}/library",
    tag = "admin",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, body = List<File>),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn workspace_library(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path(id): Path<String>,
) -> Result<Json<List<File>>, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    let files = state.library.list(workspace).await?;
    Ok(Json(List::new(files.iter().map(file).collect(), false)))
}

// served under a {*path} wildcard, so documented with its full path and registered by hand
/// Download a file from a workspace's library
#[utoipa::path(
    get,
    operation_id = "download_workspace_file",
    path = "/v1/admin/workspaces/{id}/library/files/{path}",
    tag = "admin",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("path" = String, Path, description = "File path, may contain slashes"),
    ),
    responses(
        (status = 200, content_type = "application/octet-stream", body = FileContent),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn workspace_file(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Path((id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let workspace = ids::decode(WORKSPACE, &id).ok_or_else(|| not_found(&id))?;
    let content = state
        .library
        .get(workspace, &path)
        .await
        .map_err(|error| library_error(error, &path))?;
    Ok((
        [(header::CONTENT_TYPE, attachment::sniff(&content))],
        content,
    ))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// Only lines after this sequence number, to follow the log.
    after: Option<u64>,
    /// At most this many of the newest lines, up to 1000.
    #[param(default = 500, minimum = 1, maximum = 1000)]
    limit: Option<usize>,
}

const DEFAULT_LOG_LINES: usize = 500;
const MAX_LOG_LINES: usize = 1000;

#[derive(Serialize, ToSchema)]
pub struct LogLine {
    seq: u64,
    /// Unix milliseconds.
    time: i64,
    #[schema(example = "info")]
    level: String,
    #[schema(example = "trex::runs")]
    target: String,
    message: String,
    /// The event's other fields, as `key=value` pairs.
    fields: String,
}

/// Read recent logs
///
/// This instance's most recent log lines, oldest first. Each instance keeps the last 2000.
#[utoipa::path(
    get,
    operation_id = "list_logs",
    path = "/admin/logs",
    tag = "admin",
    params(LogsQuery),
    responses((status = 200, body = List<LogLine>), (status = 403, response = ErrorResponse)),
)]
pub async fn logs(_: Admin, Query(query): Query<LogsQuery>) -> Json<List<LogLine>> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LOG_LINES)
        .clamp(1, MAX_LOG_LINES);
    let data = crate::logging::recent(query.after.unwrap_or(0), limit)
        .into_iter()
        .map(|line| LogLine {
            seq: line.seq,
            time: line.time_ms,
            level: line.level.as_str().to_ascii_lowercase(),
            target: line.target,
            message: line.message,
            fields: line.fields,
        })
        .collect();
    Json(List::new(data, false))
}

/// List every model
///
/// The whole catalog, for choosing what a workspace may use.
#[utoipa::path(
    get,
    operation_id = "list_all_models",
    path = "/admin/models",
    tag = "admin",
    responses((status = 200, body = List<super::Model>), (status = 403, response = ErrorResponse)),
)]
pub async fn all_models(State(state): State<Arc<AppState>>, _: Admin) -> Json<List<super::Model>> {
    Json(List::new(
        state.models.all().map(super::model_object).collect(),
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

/// List a user's sessions
#[utoipa::path(
    get,
    operation_id = "list_user_sessions",
    path = "/admin/users/{id}/sessions",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    responses(
        (status = 200, body = List<super::account::SignInSession>),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn user_sessions(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
) -> Result<Json<List<super::account::SignInSession>>, ApiError> {
    let user = ids::decode(USER, &id).ok_or_else(|| ApiError::NotFound(format!("no user {id}")))?;
    let sessions = state.store.user_sessions(user).await?;
    let current = (user == admin.user).then_some(admin.session);
    let data = sessions
        .iter()
        .map(|session| super::account::session_object(session, current))
        .collect();
    Ok(Json(List::new(data, false)))
}

/// Sign out one of a user's sessions
#[utoipa::path(
    delete,
    operation_id = "delete_user_session",
    path = "/admin/users/{id}/sessions/{session_id}",
    tag = "admin",
    params(
        ("id" = String, Path, description = "User id"),
        ("session_id" = String, Path, description = "Session id"),
    ),
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn delete_user_session(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path((id, session_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let not_found = || ApiError::NotFound(format!("no session {session_id}"));
    let user = ids::decode(USER, &id).ok_or_else(not_found)?;
    let session = ids::decode(super::ids::SIGN_IN, &session_id).ok_or_else(not_found)?;
    if !state.store.delete_user_session(user, session).await? {
        return Err(not_found());
    }
    tracing::info!(admin = %admin.user, user = %user, session = %session, "signed out a user's session");
    Ok(StatusCode::NO_CONTENT)
}
