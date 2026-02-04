// Unit tests for error handling utilities
use super::error_handling::*;
use crate::error::AppError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unwrap_or_error() {
        // Test with Some value
        let result: Result<i32, AppError> = unwrap_or_error(Some(42), "test error");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        // Test with None value
        let result: Result<i32, AppError> = unwrap_or_error(None, "test error");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Internal(msg) => assert_eq!(msg, "test error"),
            _ => panic!("Expected Internal error"),
        }
    }

    #[test]
    fn test_parse_with_default() {
        // Test successful parse
        let result: Result<i32, AppError> = parse_with_default("42", 0, "parse error");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        // Test failed parse
        let result: Result<i32, AppError> = parse_with_default("not_a_number", 0, "parse error");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::BadRequest(msg) => {
                assert!(msg.contains("parse error"));
                assert!(msg.contains("not_a_number"));
            },
            _ => panic!("Expected BadRequest error"),
        }
    }

    #[test]
    fn test_get_env_var() {
        // Set a test environment variable
        std::env::set_var("TEST_VAR", "test_value");

        // Test with existing variable
        let result = get_env_var("TEST_VAR", "default");
        assert_eq!(result, "test_value");

        // Test with non-existing variable
        let result = get_env_var("NON_EXISTING_VAR", "default");
        assert_eq!(result, "default");

        // Clean up
        std::env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_path_to_string() {
        use std::path::Path;

        // Test with valid path
        let path = Path::new("/valid/path");
        let result = path_to_string(path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "/valid/path");

        // Test with invalid UTF-8 path (simulated)
        // This is tricky to test on all platforms, so we'll skip it
        // In real usage, this would handle paths with invalid UTF-8
    }
}