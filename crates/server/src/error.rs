//! The one way a handler can fail, and how it reaches the client.
//!
//! Every error leaves as the same JSON object, so the TypeScript client has a
//! single error shape to parse rather than a status code and a guess.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// What went wrong, and the status it maps to.
#[derive(Debug)]
pub enum ApiError {
    /// The request asked for something that does not exist.
    NotFound(String),
    /// The request was malformed or out of bounds.
    BadRequest(String),
    /// A run, a judgment or the backend failed.
    Internal(String),
}

impl ApiError {
    /// The status this error answers with.
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// A short machine-readable tag, so the UI can branch without parsing prose.
    pub fn kind(&self) -> &'static str {
        match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Internal(_) => "internal",
        }
    }

    /// The human-readable half.
    pub fn message(&self) -> &str {
        match self {
            ApiError::NotFound(m) | ApiError::BadRequest(m) | ApiError::Internal(m) => m,
        }
    }
}

/// The body every failure answers with.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// `not_found`, `bad_request` or `internal`.
    pub error: &'static str,
    /// What went wrong, in words.
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody { error: self.kind(), message: self.message().to_owned() };
        (self.status(), Json(body)).into_response()
    }
}

impl From<jev_core::JevError> for ApiError {
    fn from(e: jev_core::JevError) -> Self {
        ApiError::Internal(e.to_string())
    }
}

/// A handler's result.
pub type ApiResult<T> = std::result::Result<T, ApiError>;
