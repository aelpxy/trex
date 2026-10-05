use std::sync::{Arc, LazyLock};

use anyhow::Context;
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use trex_store::{
    accounts,
    user_sessions::{NewSession, UserSession},
};
use utoipa::ToSchema;

use super::{
    AppState, List,
    auth::{
        Account, Client, SESSION_LIFETIME, cleared_cookie, hash_token, new_token, session_cookie,
    },
    error::{ApiError, ErrorResponse},
    ids::{self, SIGN_IN, USER, WORKSPACE},
};

type SetCookie = [(header::HeaderName, HeaderValue); 1];
const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_BYTES: usize = 1024;
const MAX_NAME_CHARS: usize = 100;
const MAX_EMAIL_BYTES: usize = 254;

// verified against when the email is unknown, so a login takes as long either way
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    Argon2::default()
        .hash_password(b"not a real password")
        .expect("hashing a constant password cannot fail")
        .to_string()
});

#[derive(Deserialize, ToSchema)]
pub struct Signup {
    #[schema(example = "sam@example.com")]
    email: String,
    #[schema(example = "Sam")]
    name: String,
    /// At least 8 characters.
    password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct Login {
    #[schema(example = "sam@example.com")]
    email: String,
    password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateAccount {
    name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePassword {
    current_password: String,
    /// At least 8 characters. The account's other sessions are signed out.
    new_password: String,
}

/// A signed-in browser.
#[derive(Serialize, ToSchema)]
pub struct SignInSession {
    #[schema(example = "signin_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "sign_in_session")]
    object: &'static str,
    /// Where it signed in from.
    ip: Option<String>,
    /// Where it was last used from.
    last_ip: Option<String>,
    user_agent: Option<String>,
    /// Whether this is the session making the request.
    current: bool,
    /// Unix seconds.
    created_at: i64,
    last_used_at: i64,
    expires_at: i64,
}

pub fn session_object(session: &UserSession, current: Option<uuid::Uuid>) -> SignInSession {
    SignInSession {
        id: ids::encode(SIGN_IN, session.id),
        object: "sign_in_session",
        ip: session.ip.clone(),
        last_ip: session.last_ip.clone(),
        user_agent: session.user_agent.clone(),
        current: current == Some(session.id),
        created_at: session.created_at,
        last_used_at: session.last_used_at,
        expires_at: session.expires_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct User {
    #[schema(example = "user_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "user")]
    object: &'static str,
    email: String,
    name: String,
    /// `admin` users can use the admin endpoints.
    role: Role,
    /// Unix seconds.
    created_at: i64,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Admin,
}

impl From<accounts::UserRole> for Role {
    fn from(role: accounts::UserRole) -> Self {
        match role {
            accounts::UserRole::User => Self::User,
            accounts::UserRole::Admin => Self::Admin,
        }
    }
}

impl From<Role> for accounts::UserRole {
    fn from(role: Role) -> Self {
        match role {
            Role::User => Self::User,
            Role::Admin => Self::Admin,
        }
    }
}

/// Chats, projects, the library and credits belong to a workspace. Requests act in the
/// workspace named by the `Trex-Workspace` header, or the user's first one.
#[derive(Serialize, ToSchema)]
pub struct Workspace {
    #[schema(example = "ws_0199b3c1d6a07c3e8b1f2a4d5e6f7a8b")]
    id: String,
    #[schema(example = "workspace")]
    object: &'static str,
    #[schema(example = "Personal")]
    name: String,
    #[schema(example = "free")]
    plan: String,
    /// `owner` or `member`.
    role: String,
    /// The credit balance; see `GET /v1/credits`.
    credits: i64,
    /// Unix seconds.
    created_at: i64,
}

/// The signed-in user and their workspaces.
#[derive(Serialize, ToSchema)]
pub struct Me {
    user: User,
    workspaces: Vec<Workspace>,
}

/// Sign up
///
/// Creates an account with a personal workspace and signs it in with an HttpOnly session cookie.
#[utoipa::path(
    post,
    operation_id = "signup",
    path = "/auth/signup",
    tag = "account",
    request_body = Signup,
    responses(
        (status = 201, body = Me),
        (status = 400, response = ErrorResponse),
        (status = 409, description = "The email is already registered", body = ErrorResponse),
    ),
)]
pub async fn signup(
    State(state): State<Arc<AppState>>,
    client: Client,
    Json(body): Json<Signup>,
) -> Result<(StatusCode, SetCookie, Json<Me>), ApiError> {
    let email = valid_email(&body.email)?;
    let name = valid_name(&body.name)?;
    valid_password(&body.password, "password")?;
    let hash = hash_password(body.password).await?;
    let (user, workspace) = state
        .store
        .create_user(&email, &name, &hash)
        .await?
        .ok_or_else(|| ApiError::Conflict("an account with this email already exists".into()))?;
    let cookie = sign_in(&state, user.id, &client).await?;
    let me = Me {
        user: user_object(&user),
        workspaces: vec![workspace_object(&workspace)],
    };
    Ok((StatusCode::CREATED, cookie, Json(me)))
}

/// Log in
///
/// Signs in with an HttpOnly session cookie, valid for 30 days.
#[utoipa::path(
    post,
    operation_id = "login",
    path = "/auth/login",
    tag = "account",
    request_body = Login,
    responses(
        (status = 200, body = Me),
        (status = 401, description = "Wrong email or password", body = ErrorResponse),
    ),
)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    client: Client,
    Json(body): Json<Login>,
) -> Result<(SetCookie, Json<Me>), ApiError> {
    let found = state.store.user_by_email(body.email.trim()).await?;
    let hash = found
        .as_ref()
        .map_or_else(|| DUMMY_HASH.clone(), |(_, hash)| hash.clone());
    let matches = verify_password(body.password, hash).await?;
    let Some((user, _)) = found.filter(|_| matches) else {
        return Err(ApiError::Authentication("wrong email or password".into()));
    };
    let cookie = sign_in(&state, user.id, &client).await?;
    let workspaces = state.store.workspaces(user.id).await?;
    let me = Me {
        user: user_object(&user),
        workspaces: workspaces.iter().map(workspace_object).collect(),
    };
    Ok((cookie, Json(me)))
}

/// Log out
///
/// Ends this session and clears its cookie.
#[utoipa::path(
    post,
    operation_id = "logout",
    path = "/auth/logout",
    tag = "account",
    responses((status = 204), (status = 401, response = ErrorResponse)),
)]
pub async fn logout(
    State(state): State<Arc<AppState>>,
    account: Account,
) -> Result<(StatusCode, SetCookie), ApiError> {
    state
        .store
        .delete_user_session(account.user, account.session)
        .await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cleared_cookie(state.secure_cookies))],
    ))
}

/// List your sessions
///
/// Every browser signed in to this account, most recently used first.
#[utoipa::path(
    get,
    operation_id = "list_my_sessions",
    path = "/me/sessions",
    tag = "account",
    responses((status = 200, body = List<SignInSession>), (status = 401, response = ErrorResponse)),
)]
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    account: Account,
) -> Result<Json<List<SignInSession>>, ApiError> {
    let sessions = state.store.user_sessions(account.user).await?;
    let data = sessions
        .iter()
        .map(|session| session_object(session, Some(account.session)))
        .collect();
    Ok(Json(List::new(data, false)))
}

/// Sign out a session
///
/// Ending the current session also clears its cookie.
#[utoipa::path(
    delete,
    operation_id = "delete_my_session",
    path = "/me/sessions/{id}",
    tag = "account",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 204),
        (status = 401, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    account: Account,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let not_found = || ApiError::NotFound(format!("no session {id}"));
    let session = ids::decode(SIGN_IN, &id).ok_or_else(not_found)?;
    if !state
        .store
        .delete_user_session(account.user, session)
        .await?
    {
        return Err(not_found());
    }
    if session == account.session {
        let cookie = [(header::SET_COOKIE, cleared_cookie(state.secure_cookies))];
        return Ok((StatusCode::NO_CONTENT, cookie).into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Sign out other sessions
///
/// Ends every session of this account except the one making the request.
#[utoipa::path(
    post,
    operation_id = "delete_other_sessions",
    path = "/me/sessions/sign_out_others",
    tag = "account",
    responses((status = 204), (status = 401, response = ErrorResponse)),
)]
pub async fn sign_out_others(
    State(state): State<Arc<AppState>>,
    account: Account,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_user_sessions(account.user, Some(account.session))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Get the signed-in user
#[utoipa::path(
    get,
    operation_id = "get_me",
    path = "/me",
    tag = "account",
    responses((status = 200, body = Me), (status = 401, response = ErrorResponse)),
)]
pub async fn me(
    State(state): State<Arc<AppState>>,
    account: Account,
) -> Result<Json<Me>, ApiError> {
    Ok(Json(load_me(&state, account.user).await?))
}

/// Update the signed-in user
#[utoipa::path(
    patch,
    operation_id = "update_me",
    path = "/me",
    tag = "account",
    request_body = UpdateAccount,
    responses(
        (status = 200, body = Me),
        (status = 400, response = ErrorResponse),
        (status = 401, response = ErrorResponse),
    ),
)]
pub async fn update_me(
    State(state): State<Arc<AppState>>,
    account: Account,
    Json(body): Json<UpdateAccount>,
) -> Result<Json<Me>, ApiError> {
    let name = body.name.as_deref().map(valid_name).transpose()?;
    state
        .store
        .update_user(account.user, name.as_deref(), None)
        .await?;
    Ok(Json(load_me(&state, account.user).await?))
}

/// Change the password
///
/// Other sessions are signed out; this one stays signed in.
#[utoipa::path(
    post,
    operation_id = "change_password",
    path = "/me/password",
    tag = "account",
    request_body = ChangePassword,
    responses(
        (status = 204),
        (status = 400, response = ErrorResponse),
        (status = 401, description = "The current password is wrong", body = ErrorResponse),
    ),
)]
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    account: Account,
    Json(body): Json<ChangePassword>,
) -> Result<StatusCode, ApiError> {
    valid_password(&body.new_password, "new_password")?;
    let (_, hash) = state
        .store
        .user(account.user)
        .await?
        .context("signed-in user no longer exists")?;
    if !verify_password(body.current_password, hash).await? {
        return Err(ApiError::Authentication(
            "the current password is wrong".into(),
        ));
    }
    let hash = hash_password(body.new_password).await?;
    state
        .store
        .update_user(account.user, None, Some(&hash))
        .await?;
    state
        .store
        .delete_user_sessions(account.user, Some(account.session))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn load_me(state: &AppState, user: uuid::Uuid) -> Result<Me, ApiError> {
    let (user, _) = state
        .store
        .user(user)
        .await?
        .context("signed-in user no longer exists")?;
    let workspaces = state.store.workspaces(user.id).await?;
    Ok(Me {
        user: user_object(&user),
        workspaces: workspaces.iter().map(workspace_object).collect(),
    })
}

// starts a session and returns the cookie that carries it
async fn sign_in(
    state: &AppState,
    user: uuid::Uuid,
    client: &Client,
) -> Result<SetCookie, ApiError> {
    let token = new_token()?;
    state
        .store
        .create_user_session(NewSession {
            user,
            token_hash: &hash_token(&token),
            lifetime: SESSION_LIFETIME,
            ip: client.ip.as_deref(),
            user_agent: client.user_agent.as_deref(),
        })
        .await?;
    Ok([(
        header::SET_COOKIE,
        session_cookie(&token, state.secure_cookies),
    )])
}

// argon2 is deliberately slow, so it runs off the async workers
async fn hash_password(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
            .map_err(|error| anyhow::anyhow!("failed to hash password: {error}"))
    })
    .await
    .context("password hashing panicked")?
}

async fn verify_password(password: String, hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&hash)
            .map_err(|error| anyhow::anyhow!("invalid stored password hash: {error}"))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .context("password verification panicked")?
}

fn valid_email(email: &str) -> Result<String, ApiError> {
    let email = email.trim();
    let valid = email.len() <= MAX_EMAIL_BYTES
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && !email.contains(char::is_whitespace);
    if !valid {
        return Err(ApiError::invalid("enter a valid email address", "email"));
    }
    Ok(email.to_owned())
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

fn valid_password(password: &str, param: &'static str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD_CHARS || password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::invalid(
            format!("passwords need at least {MIN_PASSWORD_CHARS} characters"),
            param,
        ));
    }
    Ok(())
}

fn user_object(user: &accounts::User) -> User {
    User {
        id: ids::encode(USER, user.id),
        object: "user",
        email: user.email.clone(),
        name: user.name.clone(),
        role: Role::from(user.role),
        created_at: user.created_at,
    }
}

pub fn workspace_object(workspace: &accounts::Workspace) -> Workspace {
    Workspace {
        id: ids::encode(WORKSPACE, workspace.id),
        object: "workspace",
        name: workspace.name.clone(),
        plan: workspace.plan.clone(),
        role: workspace.role.clone(),
        credits: workspace.credits,
        created_at: workspace.created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_signup_fields() {
        assert_eq!(
            valid_email(" Sam@Example.com ").ok().as_deref(),
            Some("Sam@Example.com")
        );
        for bad in [
            "",
            "sam",
            "@example.com",
            "sam@localhost",
            "sam @example.com",
        ] {
            assert!(valid_email(bad).is_err(), "{bad:?}");
        }
        assert!(valid_password("short", "password").is_err());
        assert!(valid_password("long enough", "password").is_ok());
        assert!(valid_name("   ").is_err());
    }

    #[tokio::test]
    async fn hashes_and_verifies_passwords() {
        let hash = hash_password("correct horse".into()).await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(
            verify_password("correct horse".into(), hash.clone())
                .await
                .unwrap()
        );
        assert!(!verify_password("wrong horse".into(), hash).await.unwrap());
        assert!(
            !verify_password("x".into(), DUMMY_HASH.clone())
                .await
                .unwrap()
        );
    }
}
