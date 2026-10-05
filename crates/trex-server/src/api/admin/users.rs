use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_store::accounts::UserRole;
use utoipa::ToSchema;

use super::{Admin, PageQuery, USER_SORTS};
use crate::api::{
    AppState, List, Page,
    error::{ApiError, ErrorResponse},
    ids::{self, USER},
};

#[derive(Serialize, ToSchema)]
pub struct AdminUser {
    #[schema(example = "user_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "user")]
    object: &'static str,
    email: String,
    name: String,
    role: crate::api::account::Role,
    workspaces: i64,
    /// Unix seconds.
    created_at: i64,
    /// The balance of the first workspace they own, in millionths of a dollar.
    credits: Option<i64>,
    /// Unix seconds, the last request from any of their signed-in devices.
    last_active_at: Option<i64>,
    /// Unix seconds; suspended users can't sign in and their scheduled tasks wait.
    suspended_at: Option<i64>,
}

/// List users
///
/// Every user, newest first unless `sort` (`created_at`, `name`, `email`, `last_active_at`,
/// `credits`) says otherwise. `q` matches text in the name or email.
#[utoipa::path(
    get,
    operation_id = "list_users",
    path = "/admin/users",
    tag = "admin",
    params(PageQuery),
    responses(
        (status = 200, body = Page<AdminUser>),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
    ),
)]
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    _: Admin,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<AdminUser>>, ApiError> {
    let (limit, offset) = query.window()?;
    let sort = query.sort(USER_SORTS)?;
    let search = query.search();
    let total_count = state.store.user_count(search).await?;
    let mut users = state
        .store
        .users_page(search, sort, limit + 1, offset)
        .await?;
    let has_more = users.len() as i64 > limit;
    users.truncate(limit as usize);
    let data = users
        .into_iter()
        .map(|user| AdminUser {
            id: ids::encode(USER, user.id),
            object: "user",
            email: user.email,
            name: user.name,
            role: crate::api::account::Role::from(user.role),
            workspaces: user.workspaces,
            credits: user.credits,
            created_at: user.created_at,
            last_active_at: user.last_active_at,
            suspended_at: user.suspended_at,
        })
        .collect();
    Ok(Json(Page::new(data, has_more, total_count)))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateUser {
    role: Option<crate::api::account::Role>,
    /// Suspending signs them out, stops their runs and blocks signing in until it's lifted.
    suspended: Option<bool>,
}

/// Update a user
///
/// Changes their role or suspends them. Admins can't change themselves, so there is always one
/// left.
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
        return Err(ApiError::Conflict(
            "you can't change your own account here".into(),
        ));
    }
    if let Some(role) = body.role.map(UserRole::from) {
        if !state.store.set_user_role_by_id(user, role).await? {
            return Err(not_found());
        }
        tracing::info!(admin = %admin.user, user = %user, role = role.as_str(), "changed user role");
    }
    if let Some(suspended) = body.suspended {
        if !crate::users::set_suspended(&state, user, suspended).await? {
            return Err(not_found());
        }
        tracing::info!(admin = %admin.user, user = %user, suspended, "changed user suspension");
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct SetPassword {
    password: String,
}

/// Set a user's password
///
/// For users locked out of their account. Signs them out everywhere; admins change their own
/// password from their account.
#[utoipa::path(
    post,
    operation_id = "set_user_password",
    path = "/admin/users/{id}/password",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    request_body = SetPassword,
    responses(
        (status = 204),
        (status = 400, response = ErrorResponse),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, response = ErrorResponse),
    ),
)]
pub async fn set_user_password(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
    Json(body): Json<SetPassword>,
) -> Result<StatusCode, ApiError> {
    let not_found = || ApiError::NotFound(format!("no user {id}"));
    let user = ids::decode(USER, &id).ok_or_else(not_found)?;
    if user == admin.user {
        return Err(ApiError::Conflict(
            "change your own password from your account".into(),
        ));
    }
    crate::api::account::valid_password(&body.password, "password")?;
    let hash = crate::api::account::hash_password(body.password).await?;
    if !crate::users::set_password(&state, user, &hash).await? {
        return Err(not_found());
    }
    tracing::info!(admin = %admin.user, user = %user, "set a user's password");
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a user
///
/// Signs them out and deletes them with every workspace only they belong to, including its
/// chats, sandboxes, library, projects, scheduled tasks and credits. Admins can't delete
/// themselves.
#[utoipa::path(
    delete,
    operation_id = "delete_user",
    path = "/admin/users/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    responses(
        (status = 204),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, response = ErrorResponse),
    ),
)]
pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let not_found = || ApiError::NotFound(format!("no user {id}"));
    let user = ids::decode(USER, &id).ok_or_else(not_found)?;
    if user == admin.user {
        return Err(ApiError::Conflict("you can't delete yourself".into()));
    }
    if !crate::users::delete(&state, user).await? {
        return Err(not_found());
    }
    tracing::info!(admin = %admin.user, user = %user, "deleted a user");
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

/// List a user's sessions
#[utoipa::path(
    get,
    operation_id = "list_user_sessions",
    path = "/admin/users/{id}/sessions",
    tag = "admin",
    params(("id" = String, Path, description = "User id")),
    responses(
        (status = 200, body = List<crate::api::account::SignInSession>),
        (status = 403, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn user_sessions(
    State(state): State<Arc<AppState>>,
    admin: Admin,
    Path(id): Path<String>,
) -> Result<Json<List<crate::api::account::SignInSession>>, ApiError> {
    let user = ids::decode(USER, &id).ok_or_else(|| ApiError::NotFound(format!("no user {id}")))?;
    let sessions = state.store.user_sessions(user).await?;
    let current = (user == admin.user).then_some(admin.session);
    let data = sessions
        .iter()
        .map(|session| crate::api::account::session_object(session, current))
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
    let session = ids::decode(ids::SIGN_IN, &session_id).ok_or_else(not_found)?;
    if !state.store.delete_user_session(user, session).await? {
        return Err(not_found());
    }
    tracing::info!(admin = %admin.user, user = %user, session = %session, "signed out a user's session");
    Ok(StatusCode::NO_CONTENT)
}
