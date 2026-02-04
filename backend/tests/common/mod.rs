// Common test utilities and setup
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    Router,
};
use serde_json::Value;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::sync::Arc;
use tower::ServiceExt;

// Re-export for tests
pub use axum::body::to_bytes;

/// Create an in-memory database pool for testing
pub async fn create_test_pool() -> Pool<Sqlite> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test pool")
}

/// Run migrations on the test database
pub async fn run_test_migrations(pool: &Pool<Sqlite>) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'user',
            is_active INTEGER NOT NULL DEFAULT 1,
            language TEXT NOT NULL DEFAULT 'fr',
            accent_color TEXT NOT NULL DEFAULT '#3A82F6',
            last_login TEXT,
            last_ip TEXT,
            allocated_servers TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            game_type TEXT NOT NULL,
            executable_path TEXT NOT NULL,
            working_dir TEXT NOT NULL,
            java_path TEXT,
            min_memory TEXT,
            max_memory TEXT,
            extra_args TEXT,
            config TEXT,
            auto_start INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            backup_enabled INTEGER NOT NULL DEFAULT 1,
            backup_frequency INTEGER NOT NULL DEFAULT 30,
            backup_max_backups INTEGER NOT NULL DEFAULT 7,
            backup_prefix TEXT NOT NULL DEFAULT 'hytale_backup',
            discord_username TEXT DEFAULT 'Hytale Bot',
            discord_avatar TEXT DEFAULT '',
            discord_webhook_url TEXT DEFAULT '',
            discord_notifications TEXT DEFAULT '{}',
            logs_retention_days INTEGER NOT NULL DEFAULT 7,
            watchdog_enabled INTEGER NOT NULL DEFAULT 1,
            auth_mode TEXT NOT NULL DEFAULT 'authenticated',
            bind_address TEXT NOT NULL DEFAULT '0.0.0.0',
            port INTEGER NOT NULL DEFAULT 5520
        );

        CREATE TABLE IF NOT EXISTS backups (
            id TEXT PRIMARY KEY,
            server_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_secrets (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS server_players (
            server_id TEXT NOT NULL,
            player_name TEXT NOT NULL,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            is_online INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (server_id, player_name),
            FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS schedules (
            id TEXT PRIMARY KEY,
            server_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            cron_expression TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("Failed to run migrations");
}

/// Extract JSON body from response
pub async fn get_body_json(response: Response<Body>) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read body");
    serde_json::from_slice(&body).expect("Failed to parse JSON")
}

/// Create a test app instance - placeholder that needs main crate access
/// Note: This requires the main crate to expose a test helper
pub async fn create_test_app() -> Router {
    // For now, return an empty router - actual implementation needs
    // the main crate to expose AppState creation
    todo!("Implement test app creation - requires exposing AppState from main crate")
}

/// Create a test app with an authenticated user
pub async fn create_test_app_with_auth() -> (Router, String) {
    todo!("Implement authenticated test app creation")
}
