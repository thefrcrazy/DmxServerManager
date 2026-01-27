use actix_web::{HttpResponse, ResponseError};
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Internal(String),
    Database(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),
            AppError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            AppError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            AppError::Internal(msg) => write!(f, "Internal error: {}", msg),
            AppError::Database(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        match self {
            AppError::NotFound(msg) => {
                HttpResponse::NotFound().json(serde_json::json!({ "error": msg }))
            }
            AppError::BadRequest(msg) => {
                HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }))
            }
            AppError::Unauthorized(msg) => {
                HttpResponse::Unauthorized().json(serde_json::json!({ "error": msg }))
            }
            AppError::Internal(msg) => {
                // Log the actual error for debugging
                eprintln!("Internal Server Error: {}", msg);
                // Return generic key to client
                HttpResponse::InternalServerError().json(serde_json::json!({ "error": "errors.internal" }))
            }
            AppError::Database(msg) => {
                // Log the actual error for debugging
                eprintln!("Database Error: {}", msg);
                // Return generic key to client
                HttpResponse::InternalServerError().json(serde_json::json!({ "error": "errors.database" }))
            }
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        AppError::Unauthorized(err.to_string())
    }
}
