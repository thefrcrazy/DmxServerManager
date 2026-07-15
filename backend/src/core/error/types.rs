use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use thiserror::Error;
use tracing::error;

use crate::core::error::codes::ErrorCode;

/// Context information for debugging
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    pub server_id: Option<String>,
    pub user_id: Option<String>,
    pub file_path: Option<String>,
    // pub request_id: Option<String>,
}

/// Main error type for the application
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Precondition required: {0}")]
    PreconditionRequired(String),

    #[error("Too many requests: {0}")]
    TooManyRequests(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("{message}")]
    Rich {
        kind: AppErrorKind,
        message: String,
        code: Option<ErrorCode>,
        context: ErrorContext,
    },
}

#[derive(Debug, Clone, Copy, Error)]
pub enum AppErrorKind {
    #[error("Not found")]
    NotFound,
    #[error("Bad request")]
    BadRequest,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Forbidden")]
    Forbidden,
    #[error("Conflict")]
    Conflict,
    #[error("Precondition required")]
    PreconditionRequired,
    #[error("Too many requests")]
    TooManyRequests,
    #[error("Internal error")]
    Internal,
    #[error("Database error")]
    Database,
}

impl AppError {
    pub fn with_code(self, code: ErrorCode) -> Self {
        match self {
            Self::Rich {
                kind,
                message,
                context,
                ..
            } => Self::Rich {
                kind,
                message,
                code: Some(code),
                context,
            },
            _ => {
                let kind = self.get_kind();
                let message = self.get_message().to_string();
                Self::Rich {
                    kind,
                    message,
                    code: Some(code),
                    context: ErrorContext::default(),
                }
            }
        }
    }

    fn get_kind(&self) -> AppErrorKind {
        match self {
            Self::NotFound(_) => AppErrorKind::NotFound,
            Self::BadRequest(_) => AppErrorKind::BadRequest,
            Self::Unauthorized(_) => AppErrorKind::Unauthorized,
            Self::Forbidden(_) => AppErrorKind::Forbidden,
            Self::Conflict(_) => AppErrorKind::Conflict,
            Self::PreconditionRequired(_) => AppErrorKind::PreconditionRequired,
            Self::TooManyRequests(_) => AppErrorKind::TooManyRequests,
            Self::Internal(_) => AppErrorKind::Internal,
            Self::Database(_) => AppErrorKind::Database,
            Self::Rich { kind, .. } => *kind,
        }
    }

    fn get_message(&self) -> &str {
        match self {
            Self::NotFound(msg)
            | Self::BadRequest(msg)
            | Self::Unauthorized(msg)
            | Self::Forbidden(msg)
            | Self::Conflict(msg)
            | Self::PreconditionRequired(msg)
            | Self::TooManyRequests(msg)
            | Self::Internal(msg)
            | Self::Database(msg) => msg,
            Self::Rich { message, .. } => message,
        }
    }

    fn get_code(&self) -> Option<ErrorCode> {
        match self {
            Self::Rich { code, .. } => *code,
            _ => None,
        }
    }

    fn get_context(&self) -> ErrorContext {
        match self {
            Self::Rich { context, .. } => context.clone(),
            _ => ErrorContext::default(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let kind = self.get_kind();
        let message = self.get_message().to_string();
        let code = self.get_code();
        let context = self.get_context();
        let trace_id = uuid::Uuid::new_v4().to_string();

        let status = match kind {
            AppErrorKind::NotFound => StatusCode::NOT_FOUND,
            AppErrorKind::BadRequest => StatusCode::BAD_REQUEST,
            AppErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
            AppErrorKind::Forbidden => StatusCode::FORBIDDEN,
            AppErrorKind::Conflict => StatusCode::CONFLICT,
            AppErrorKind::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            AppErrorKind::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            AppErrorKind::Internal | AppErrorKind::Database => StatusCode::INTERNAL_SERVER_ERROR,
        };

        // Determine client-facing message
        let client_message = match kind {
            AppErrorKind::Internal => "errors.internal".to_string(),
            AppErrorKind::Database => "errors.database".to_string(),
            _ => message.clone(),
        };

        // Log with tracing
        let code_str = code.map(|c| c.as_str()).unwrap_or("UNKNOWN");

        match kind {
            AppErrorKind::Internal | AppErrorKind::Database => {
                error!(
                    error_code = code_str,
                    trace_id = %trace_id,
                    error_kind = ?kind,
                    message = %message,
                    server_id = ?context.server_id,
                    user_id = ?context.user_id,
                    file_path = ?context.file_path,
                    "Internal error occurred"
                );
            }
            _ => {
                tracing::warn!(
                    error_code = code_str,
                    trace_id = %trace_id,
                    error_kind = ?kind,
                    message = %message,
                    "Client error"
                );
            }
        }

        let mut body = serde_json::json!({
            "type": "about:blank",
            "title": client_message,
            "status": status.as_u16(),
            "trace_id": trace_id.clone()
        });

        if let Some(c) = code {
            body["code"] = serde_json::json!(c.as_str());
        }

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Ok(value) = HeaderValue::from_str(&trace_id) {
            response
                .headers_mut()
                .insert(header::HeaderName::from_static("x-trace-id"), value);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
        }
        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err.to_string()).with_code(ErrorCode::DatabaseQuery)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(err.to_string()).with_code(ErrorCode::InternalError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_responses_expose_a_trace_header_and_retry_delay() {
        let response = AppError::TooManyRequests("auth.rate_limited".into()).into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "300");
        let trace = response
            .headers()
            .get("x-trace-id")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(uuid::Uuid::parse_str(trace).is_ok());
    }
}
