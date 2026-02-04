// Unit tests for database functions
use crate::db::{generate_jwt_secret, get_or_create_jwt_secret};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::io::{Error, ErrorKind};

pub type DbPool = Pool<Sqlite>;

pub async fn create_test_pool() -> DbPool {
    // Create an in-memory database for testing
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test pool")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;

    #[tokio::test]
    async fn test_generate_jwt_secret() {
        // Test that generated secret has correct length
        let secret = generate_jwt_secret();
        assert_eq!(secret.len(), 64);
        
        // Test that generated secret contains only alphanumeric characters
        assert!(secret.chars().all(|c| c.is_ascii_alphanumeric()));
        
        // Test that two generated secrets are different
        let secret2 = generate_jwt_secret();
        assert_ne!(secret, secret2);
    }

    #[tokio::test]
    async fn test_get_or_create_jwt_secret() {
        let pool = create_test_pool().await;
        
        // Run migrations to create the app_secrets table
        run_migrations(&pool).await.expect("Failed to run migrations");
        
        // First call should generate and store a new secret
        let secret1 = get_or_create_jwt_secret(&pool).await.expect("Failed to get secret");
        assert_eq!(secret1.len(), 64);
        
        // Second call should return the same secret
        let secret2 = get_or_create_jwt_secret(&pool).await.expect("Failed to get secret");
        assert_eq!(secret1, secret2);
        
        // Verify the secret was stored in the database
        let stored_secret: Option<String> = sqlx::query_scalar(
            "SELECT value FROM app_secrets WHERE key = 'jwt_secret'"
        )
        .fetch_optional(&pool)
        .await
        .expect("Failed to query secret");
        
        assert!(stored_secret.is_some());
        assert_eq!(stored_secret.unwrap(), secret1);
    }

    #[tokio::test]
    async fn test_jwt_secret_persistence() {
        let pool = create_test_pool().await;
        
        // Run migrations
        run_migrations(&pool).await.expect("Failed to run migrations");
        
        // Manually insert a secret
        let manual_secret = "test_secret_1234567890123456789012345678901234567890123456789012345678901234";
        sqlx::query(
            "INSERT INTO app_secrets (key, value, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))"
        )
        .bind("jwt_secret")
        .bind(manual_secret)
        .execute(&pool)
        .await
        .expect("Failed to insert secret");
        
        // get_or_create_jwt_secret should return the manually inserted secret
        let retrieved_secret = get_or_create_jwt_secret(&pool).await.expect("Failed to get secret");
        assert_eq!(retrieved_secret, manual_secret);
    }
}