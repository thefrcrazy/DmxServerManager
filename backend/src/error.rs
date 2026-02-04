use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::fmt;
use tracing::error;

use crate::error_codes::ErrorCode;

/// Context information for debugging
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    pub server_id: Option<String>,
    pub user_id: Option<String>,
    pub file_path: Option<String>,
    pub request_id: Option<String>,
}

impl ErrorContext {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_server(mut self, id: &str) -> Self {
        self.server_id = Some(id.to_string());
        self
    }
    
    pub fn with_user(mut self, id: &str) -> Self {
        self.user_id = Some(id.to_string());
        self
    }
    
    pub fn with_file(mut self, path: &str) -> Self {
        self.file_path = Some(path.to_string());
        self
    }
}

/// Main error type - maintains backward compatibility with enum-style usage
#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Internal(String),
    Database(String),
    
    // New: Rich error with full context
    Rich {
        kind: AppErrorKind,
        message: String,
        code: Option<ErrorCode>,
        context: ErrorContext,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum AppErrorKind {
    NotFound,
    BadRequest,
    Unauthorized,
    Internal,
    Database,
}

impl AppError {
    // Builder methods for rich errors
    pub fn not_found_rich(msg: impl Into<String>) -> Self {
        Self::Rich {
            kind: AppErrorKind::NotFound,
            message: msg.into(),
            code: None,
            context: ErrorContext::default(),
        }
    }
    
    pub fn bad_request_rich(msg: impl Into<String>) -> Self {
        Self::Rich {
            kind: AppErrorKind::BadRequest,
            message: msg.into(),
            code: None,
            context: ErrorContext::default(),
        }
    }
    
    pub fn unauthorized_rich(msg: impl Into<String>) -> Self {
        Self::Rich {
            kind: AppErrorKind::Unauthorized,
            message: msg.into(),
            code: None,
            context: ErrorContext::default(),
        }
    }
    
    pub fn internal_rich(msg: impl Into<String>) -> Self {
        Self::Rich {
            kind: AppErrorKind::Internal,
            message: msg.into(),
            code: None,
            context: ErrorContext::default(),
        }
    }
    
    pub fn database_rich(msg: impl Into<String>) -> Self {
        Self::Rich {
            kind: AppErrorKind::Database,
            message: msg.into(),
            code: None,
            context: ErrorContext::default(),
        }
    }
    
    // Add code to any error
    pub fn with_code(self, code: ErrorCode) -> Self {
        match self {
            Self::Rich { kind, message, context, .. } => Self::Rich {
                kind,
                message,
                code: Some(code),
                context,
            },
            // Convert legacy errors to rich errors with code
            Self::NotFound(msg) => Self::Rich {
                kind: AppErrorKind::NotFound,
                message: msg,
                code: Some(code),
                context: ErrorContext::default(),
            },
            Self::BadRequest(msg) => Self::Rich {
                kind: AppErrorKind::BadRequest,
                message: msg,
                code: Some(code),
                context: ErrorContext::default(),
            },
            Self::Unauthorized(msg) => Self::Rich {
                kind: AppErrorKind::Unauthorized,
                message: msg,
                code: Some(code),
                context: ErrorContext::default(),
            },
            Self::Internal(msg) => Self::Rich {
                kind: AppErrorKind::Internal,
                message: msg,
                code: Some(code),
                context: ErrorContext::default(),
            },
            Self::Database(msg) => Self::Rich {
                kind: AppErrorKind::Database,
                message: msg,
                code: Some(code),
                context: ErrorContext::default(),
            },
        }
    }
    
    pub fn with_context(self, context: ErrorContext) -> Self {
        match self {
            Self::Rich { kind, message, code, .. } => Self::Rich {
                kind,
                message,
                code,
                context,
            },
            Self::NotFound(msg) => Self::Rich {
                kind: AppErrorKind::NotFound,
                message: msg,
                code: None,
                context,
            },
            Self::BadRequest(msg) => Self::Rich {
                kind: AppErrorKind::BadRequest,
                message: msg,
                code: None,
                context,
            },
            Self::Unauthorized(msg) => Self::Rich {
                kind: AppErrorKind::Unauthorized,
                message: msg,
                code: None,
                context,
            },
            Self::Internal(msg) => Self::Rich {
                kind: AppErrorKind::Internal,
                message: msg,
                code: None,
                context,
            },
            Self::Database(msg) => Self::Rich {
                kind: AppErrorKind::Database,
                message: msg,
                code: None,
                context,
            },
        }
    }
    
    pub fn with_server(self, id: &str) -> Self {
        let ctx = ErrorContext::new().with_server(id);
        self.with_context(ctx)
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

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.get_kind() {
            AppErrorKind::NotFound => write!(f, "Not found: {}", self.get_message()),
            AppErrorKind::BadRequest => write!(f, "Bad request: {}", self.get_message()),
            AppErrorKind::Unauthorized => write!(f, "Unauthorized: {}", self.get_message()),
            AppErrorKind::Internal => write!(f, "Internal error: {}", self.get_message()),
            AppErrorKind::Database => write!(f, "Database error: {}", self.get_message()),
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

        // Log with tracing (structured logging)
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
        
        // Add debug info in development mode
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
