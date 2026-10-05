use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::{ToResponse, ToSchema};

// every error response has the same shape: {"error": {"type", "message", "param"}}
pub enum ApiError {
    InvalidRequest {
        message: String,
        param: Option<&'static str>,
    },
    Authentication(String),
    Permission(String),
    InsufficientCredits(String),
    NotFound(String),
    Conflict(String),
    Internal(anyhow::Error),
}

#[derive(Serialize, ToSchema, ToResponse)]
#[response(description = "Error")]
pub struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    #[serde(rename = "type")]
    kind: ErrorType,
    message: String,
    /// The request parameter the error relates to.
    param: Option<&'static str>,
}

#[derive(Serialize, ToSchema)]
pub enum ErrorType {
    #[serde(rename = "invalid_request_error")]
    InvalidRequest,
    #[serde(rename = "authentication_error")]
    Authentication,
    #[serde(rename = "permission_error")]
    Permission,
    #[serde(rename = "insufficient_credits_error")]
    InsufficientCredits,
    #[serde(rename = "not_found_error")]
    NotFound,
    #[serde(rename = "conflict_error")]
    Conflict,
    #[serde(rename = "api_error")]
    Api,
}

impl ApiError {
    // what the client would be told, for errors recorded instead of returned
    pub fn message(&self) -> String {
        match self {
            Self::InvalidRequest { message, .. }
            | Self::Authentication(message)
            | Self::Permission(message)
            | Self::InsufficientCredits(message)
            | Self::NotFound(message)
            | Self::Conflict(message) => message.clone(),
            Self::Internal(_) => "internal server error".to_owned(),
        }
    }

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
                ErrorType::InvalidRequest,
                message,
                param,
            ),
            Self::Authentication(message) => (
                StatusCode::UNAUTHORIZED,
                ErrorType::Authentication,
                message,
                None,
            ),
            Self::Permission(message) => {
                (StatusCode::FORBIDDEN, ErrorType::Permission, message, None)
            }
            Self::InsufficientCredits(message) => (
                StatusCode::PAYMENT_REQUIRED,
                ErrorType::InsufficientCredits,
                message,
                None,
            ),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, ErrorType::NotFound, message, None),
            Self::Conflict(message) => (StatusCode::CONFLICT, ErrorType::Conflict, message, None),
            Self::Internal(error) => {
                tracing::error!(error = format!("{error:#}"), "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorType::Api,
                    "internal server error".to_owned(),
                    None,
                )
            }
        };
        let body = ErrorResponse {
            error: ErrorDetail {
                kind,
                message,
                param,
            },
        };
        (status, Json(body)).into_response()
    }
}
