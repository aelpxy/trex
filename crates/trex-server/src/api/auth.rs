use axum::{extract::FromRequestParts, http::request::Parts};
use uuid::Uuid;

use super::error::ApiError;

const USER_HEADER: &str = "x-trex-user";

// temporary: the caller names its user id in a header until registration and api keys exist
pub struct CurrentUser(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(USER_HEADER)
            .ok_or_else(|| ApiError::Authentication(format!("missing {USER_HEADER} header")))?;
        header
            .to_str()
            .ok()
            .and_then(|value| Uuid::try_parse(value).ok())
            .map(Self)
            .ok_or_else(|| ApiError::Authentication(format!("{USER_HEADER} must be a uuid")))
    }
}
