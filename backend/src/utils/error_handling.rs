use tracing::error;
use crate::core::error::AppError;

/// Safely unwrap a Result with a custom error message
pub fn _unwrap_result_or_error<T, E>(result: Result<T, E>, error_msg: &str) -> Result<T, AppError>
where
    E: std::fmt::Debug,
{
    result.map_err(|e| {
        error!("Error: {e:?}");
        AppError::Internal(error_msg.into())
    })
}

/// Convert string to value with default fallback
pub fn _parse_with_default<T>(value: &str, _default: T, error_msg: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    value.parse::<T>().map_err(|e| {
        error!("Parse error: {e:?}");
        AppError::BadRequest(format!("{error_msg}: {value}"))
    })
}
