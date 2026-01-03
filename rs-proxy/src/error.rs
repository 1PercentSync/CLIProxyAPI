//! Error types for RS-Proxy.
//!
//! This module defines unified error types using thiserror.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Proxy error types.
#[derive(Error, Debug)]
pub enum ProxyError {
    /// Error when forwarding request to upstream.
    #[error("request forwarding failed: {0}")]
    ForwardingFailed(#[from] reqwest::Error),

    /// Error when parsing request body as JSON.
    #[error("invalid request body: {0}")]
    InvalidBody(#[from] serde_json::Error),

    /// Error when model with thinking suffix is not in registry.
    #[error("unknown model with thinking suffix: {0}")]
    UnknownModel(String),

    /// Error when thinking level is invalid.
    #[error("invalid thinking level: {0}")]
    InvalidLevel(String),

    /// Internal server error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ProxyError::ForwardingFailed(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            ProxyError::InvalidBody(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyError::UnknownModel(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyError::InvalidLevel(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = json!({
            "error": {
                "message": message,
                "type": "proxy_error"
            }
        });

        (status, Json(body)).into_response()
    }
}

/// Result type alias for proxy operations.
pub type ProxyResult<T> = Result<T, ProxyError>;
