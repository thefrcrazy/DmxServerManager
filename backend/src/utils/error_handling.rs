// Error handling utilities
use crate::error::AppError;

/// Safely unwrap an Option with a custom error message
pub fn unwrap_or_error<T>(option: Option<T>, error_msg: &str) -> Result<T, AppError> {
    option.ok_or_else(|| AppError::Internal(error_msg.into()))
}

/// Safely unwrap a Result with a custom error message
pub fn unwrap_result_or_error<T, E>(result: Result<T, E>, error_msg: &str) -> Result<T, AppError>
where
    E: std::fmt::Debug,
{
    result.map_err(|e| {
        eprintln!("Error: {e:?}");
        AppError::Internal(error_msg.into())
    })
}

/// Convert string to value with default fallback
pub fn parse_with_default<T>(value: &str, default: T, error_msg: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    value.parse::<T>().map_err(|e| {
        eprintln!("Parse error: {e:?}");
        AppError::BadRequest(format!("{error_msg}: {value}"))
    })
}

/// Get environment variable with default
pub fn get_env_var(var_name: &str, default: &str) -> String {
    std::env::var(var_name).unwrap_or_else(|_| default.to_string())
}

/// Safe path conversion
pub fn path_to_string(path: &std::path::Path) -> Result<String, AppError> {
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Internal("Invalid path encoding".into()))
}

/// Safe file operations
pub async fn read_file_safe(path: &std::path::Path) -> Result<String, AppError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| {
            eprintln!("File read error: {e:?}");
            AppError::Internal(format!("Failed to read file: {}", path.display()))
        })
}

pub async fn write_file_safe(path: &std::path::Path, content: &str) -> Result<(), AppError> {
    tokio::fs::write(path, content)
        .await
        .map_err(|e| {
            eprintln!("File write error: {e:?}");
            AppError::Internal(format!("Failed to write file: {}", path.display()))
        })
}