use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use tracing::error;
use thiserror::Error;

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
    #[error("Internal error")]
    Internal,
    #[error("Database error")]
    Database,
}

impl AppError {
    pub fn with_code(self, code: ErrorCode) -> Self {
        match self {
            Self::Rich { kind, message, context, .. } => Self::Rich {
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
            Self::Internal(_) => AppErrorKind::Internal,
            Self::Database(_) => AppErrorKind::Database,
            Self::Rich { kind, .. } => *kind,
        }
    }
    
    fn get_message(&self) -> &str {
        match self {
            Self::NotFound(msg) | Self::BadRequest(msg) | Self::Unauthorized(msg) 
            | Self::Internal(msg) | Self::Database(msg) => msg,
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
        
        let status = match kind {
            AppErrorKind::NotFound => StatusCode::NOT_FOUND,
            AppErrorKind::BadRequest => StatusCode::BAD_REQUEST,
            AppErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
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
                    error_kind = ?kind,
                    message = %message,
                    "Client error"
                );
            }
        }

        let mut body = serde_json::json!({
            "error": client_message
        });
        
        if let Some(c) = code {
            body["code"] = serde_json::json!(c.as_str());
        }
        
        #[cfg(debug_assertions)]
        {
            let mut debug = serde_json::Map::new();
            if let Some(ref server_id) = context.server_id {
                debug.insert("server_id".to_string(), serde_json::json!(server_id));
            }
            if let Some(ref file_path) = context.file_path {
                debug.insert("file_path".to_string(), serde_json::json!(file_path));
            }
            if !debug.is_empty() {
                body["debug"] = serde_json::Value::Object(debug);
            }
        }

        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err.to_string()).with_code(ErrorCode::DatabaseQuery)
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        AppError::Unauthorized(err.to_string()).with_code(ErrorCode::AuthInvalidToken)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(err.to_string()).with_code(ErrorCode::InternalError)
    }
}

impl From<axum::extract::multipart::MultipartError> for AppError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        AppError::BadRequest(err.to_string())
    }
}
