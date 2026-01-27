use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::io::{Error, ErrorKind};
use tracing::info;

pub type DbPool = Pool<Sqlite>;

pub async fn init_pool(database_url: &str) -> std::io::Result<DbPool> {
    // Ensure the data directory exists
    if let Some(path) = database_url.strip_prefix("sqlite:") {
        if let Some(path) = path.split('?').next() {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
    }

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
}

pub async fn run_migrations(pool: &DbPool) -> std::io::Result<()> {
    info!("📦 Running database migrations...");

    // Create tables
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
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS backups (
            id TEXT PRIMARY KEY,
            server_id TEXT NOT NULL,
            filename TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            created_at TEXT NOT NULL,
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

        CREATE TABLE IF NOT EXISTS server_players (
            server_id TEXT NOT NULL,
            player_name TEXT NOT NULL,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            is_online INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (server_id, player_name),
            FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    // Run migrations for existing databases (add new columns if they don't exist)
    // SQLite doesn't support IF NOT EXISTS for columns, so we use PRAGMA to check
    // PRAGMA table_info returns: cid (INTEGER), name (TEXT), type (TEXT), notnull (INTEGER), dflt_value, pk (INTEGER)
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as("PRAGMA table_info(users)")
        .fetch_all(pool)
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    let column_names: Vec<&str> = columns.iter().map(|c| c.1.as_str()).collect();

    if !column_names.contains(&"is_active") {
        sqlx::query("ALTER TABLE users ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1")
            .execute(pool)
            .await
            .ok(); // Ignore errors if column exists
    }

    if !column_names.contains(&"language") {
        sqlx::query("ALTER TABLE users ADD COLUMN language TEXT NOT NULL DEFAULT 'fr'")
            .execute(pool)
            .await
            .ok();
    }

    if !column_names.contains(&"accent_color") {
        sqlx::query("ALTER TABLE users ADD COLUMN accent_color TEXT NOT NULL DEFAULT '#3A82F6'")
            .execute(pool)
            .await
            .ok();
    }

    if !column_names.contains(&"last_login") {
        sqlx::query("ALTER TABLE users ADD COLUMN last_login TEXT")
            .execute(pool)
            .await
            .ok();
    }

    if !column_names.contains(&"last_ip") {
        sqlx::query("ALTER TABLE users ADD COLUMN last_ip TEXT")
            .execute(pool)
            .await
            .ok();
    }

    if !column_names.contains(&"allocated_servers") {
        sqlx::query("ALTER TABLE users ADD COLUMN allocated_servers TEXT")
            .execute(pool)
            .await
            .ok();
    }

    // Check servers table for config column
    let server_columns: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as("PRAGMA table_info(servers)")
        .fetch_all(pool)
        .await
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    let server_column_names: Vec<&str> = server_columns.iter().map(|c| c.1.as_str()).collect();

    if !server_column_names.contains(&"config") {
        sqlx::query("ALTER TABLE servers ADD COLUMN config TEXT")
            .execute(pool)
            .await
            .ok();
    }

    info!("✅ Migrations completed");
    Ok(())
}

