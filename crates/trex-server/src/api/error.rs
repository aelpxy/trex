use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

// every error response has the same shape: {"error": {"type", "message", "param"}}
pub enum ApiError {
    InvalidRequest {
        message: String,
        param: Option<&'static str>,
    },
    Authentication(String),
    NotFound(String),
    Conflict(String),
    Internal(anyhow::Error),
}

impl ApiError {
    pub fn invalid(message: impl Into<String>, param: &'static str) -> Self {
        Self::InvalidRequest {
            message: message.into(),
            param: Some(param),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message, param) = match self {
            Self::InvalidRequest { message, param } => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                message,
                param,
            ),
            Self::Authentication(message) => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                message,
                None,
            ),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, "not_found_error", message, None),
            Self::Conflict(message) => (StatusCode::CONFLICT, "conflict_error", message, None),
            Self::Internal(error) => {
                tracing::error!(error = format!("{error:#}"), "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "api_error",
                    "internal server error".to_owned(),
                    None,
                )
            }
        };
        let body = json!({"error": {"type": kind, "message": message, "param": param}});
        (status, Json(body)).into_response()
    }
}
