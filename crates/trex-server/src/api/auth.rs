use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    AppState,
    error::ApiError,
    ids::{self, WORKSPACE},
};

pub const WORKSPACE_HEADER: &str = "trex-workspace";
const TOKEN_PREFIX: &str = "trex_";
const TOKEN_BYTES: usize = 32;

// a signed-in user acting in one of their workspaces: the one named in the Trex-Workspace header,
// or their first (personal) workspace
pub struct Auth {
    pub workspace: Uuid,
}

// a signed-in user, for endpoints about the account rather than a workspace
pub struct Account {
    pub user: Uuid,
    pub token_hash: Vec<u8>,
}

impl FromRequestParts<Arc<AppState>> for Account {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| {
                ApiError::Authentication("missing Authorization: Bearer <token> header".into())
            })?;
        let token_hash = hash_token(token.trim());
        let user = state
            .store
            .token_user(&token_hash)
            .await?
            .ok_or_else(|| ApiError::Authentication("invalid or expired token".into()))?;
        Ok(Self { user, token_hash })
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

pub fn new_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("failed to generate token: {error}"))?;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("{TOKEN_PREFIX}{hex}"))
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
