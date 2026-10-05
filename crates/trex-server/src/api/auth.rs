use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request},
    http::{HeaderMap, HeaderValue, Method, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    AppState,
    error::ApiError,
    ids::{self, WORKSPACE},
};

pub const WORKSPACE_HEADER: &str = "trex-workspace";
pub const SESSION_COOKIE: &str = "session";
pub const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
// browsers only send custom headers cross-origin after a cors preflight, which trex never allows
pub const CSRF_HEADER: &str = "x-requested-with";
const TOKEN_BYTES: usize = 32;
const MAX_USER_AGENT_BYTES: usize = 512;

// a signed-in user acting in one of their workspaces: the one named in the Trex-Workspace header,
// or their first (personal) workspace
pub struct Auth {
    pub workspace: Uuid,
}

// a signed-in user, for endpoints about the account rather than a workspace
pub struct Account {
    pub user: Uuid,
    // the sign-in session this request came with
    pub session: Uuid,
}

// where a request came from, recorded on sign-in sessions
pub struct Client {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl FromRequestParts<Arc<AppState>> for Client {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // a client can set x-forwarded-for to anything, so it only counts behind a trusted proxy
        let forwarded = state
            .trust_proxy_headers
            .then(|| parts.headers.get("x-forwarded-for"))
            .flatten()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(|ip| ip.trim().to_owned());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().to_string());
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(|agent| agent.chars().take(MAX_USER_AGENT_BYTES).collect());
        Ok(Self {
            ip: forwarded.or(peer),
            user_agent,
        })
    }
}

impl FromRequestParts<Arc<AppState>> for Account {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = session_token(&parts.headers)
            .ok_or_else(|| ApiError::Authentication("sign in to continue".into()))?;
        let client = Client::from_request_parts(parts, state).await?;
        let session = state
            .store
            .use_user_session(&hash_token(&token), client.ip.as_deref())
            .await?
            .ok_or_else(|| {
                ApiError::Authentication("your session expired; sign in again".into())
            })?;
        Ok(Self {
            user: session.user,
            session: session.id,
        })
    }
}

impl FromRequestParts<Arc<AppState>> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Account { user, .. } = Account::from_request_parts(parts, state).await?;
        let requested = parts
            .headers
            .get(WORKSPACE_HEADER)
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| ids::decode(WORKSPACE, value))
                    .ok_or_else(|| {
                        ApiError::invalid("Trex-Workspace must be a workspace id", "Trex-Workspace")
                    })
            })
            .transpose()?;
        let workspace = match requested {
            Some(workspace) => {
                state
                    .store
                    .membership(user, workspace)
                    .await?
                    .ok_or_else(|| {
                        ApiError::Permission("you are not a member of that workspace".into())
                    })?
                    .id
            }
            None => {
                let workspaces = state.store.workspaces(user).await?;
                workspaces
                    .first()
                    .ok_or_else(|| ApiError::Permission("you have no workspace".into()))?
                    .id
            }
        };
        Ok(Self { workspace })
    }
}

// cookies ride along on every request to the site, so requests that change something must also
// carry a header other sites can't add
pub async fn require_csrf_header(request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && !request.headers().contains_key(CSRF_HEADER) {
        return ApiError::Permission(format!(
            "requests that change data need the {CSRF_HEADER} header"
        ))
        .into_response();
    }
    next.run(request).await
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| {
            cookie
                .trim()
                .strip_prefix(SESSION_COOKIE)?
                .strip_prefix('=')
        })
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

pub fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    cookie(token, SESSION_LIFETIME.as_secs(), secure)
}

pub fn cleared_cookie(secure: bool) -> HeaderValue {
    cookie("", 0, secure)
}

fn cookie(value: &str, max_age: u64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}"
    ))
    .expect("session cookies are ascii")
}

pub fn new_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("failed to generate token: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; session=abc123; other=1"),
        );
        assert_eq!(session_token(&headers).as_deref(), Some("abc123"));
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("sessions=nope; session="),
        );
        assert_eq!(session_token(&headers), None);
        let cookie = session_cookie("abc", true);
        let cookie = cookie.to_str().unwrap();
        assert!(
            cookie.starts_with("session=abc; Path=/;")
                && cookie.contains("HttpOnly")
                && cookie.ends_with("SameSite=Lax; Secure"),
            "{cookie}"
        );
        assert!(
            cleared_cookie(false)
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }
}
