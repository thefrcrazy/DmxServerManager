use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::io::Error;
use tracing::info;
use chrono::Utc;

pub type DbPool = Pool<Sqlite>;

/// Generate a secure random JWT secret
pub fn generate_jwt_secret() -> String {
    use rand::Rng;
    use rand::distributions::Alphanumeric;
    
    let mut rng = rand::thread_rng();
    let secret: String = (0..64)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect();
    secret
}

/// Get or create JWT secret from database
pub async fn get_or_create_jwt_secret(pool: &DbPool) -> Result<String, sqlx::Error> {
    // Try to get existing secret
    let result = sqlx::query_scalar::<_, String>(
        "SELECT value FROM app_secrets WHERE key = 'jwt_secret'"
    )
    .fetch_optional(pool)
    .await?;
    
    match result {
        Some(secret) => Ok(secret),
        None => {
            // Generate new secret and store it
            let secret = generate_jwt_secret();
            let now = Utc::now().to_rfc3339();
            
            sqlx::query(
                "INSERT INTO app_secrets (key, value, created_at, updated_at) VALUES (?, ?, ?, ?)"
            )
            .bind("jwt_secret")
            .bind(&secret)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
            
            Ok(secret)
        }
    }
}

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
        .map_err(|e| Error::other(e.to_string()))
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
            updated_at TEXT NOT NULL,
            
            -- New settings (formerly manager.json)
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
            player_id TEXT,
            player_ip TEXT,
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

        CREATE TABLE IF NOT EXISTS server_metrics (
            id TEXT PRIMARY KEY,
            server_id TEXT NOT NULL,
            cpu_usage REAL NOT NULL,
            memory_bytes INTEGER NOT NULL,
            disk_bytes INTEGER NOT NULL,
            player_count INTEGER NOT NULL,
            recorded_at TEXT NOT NULL,
            FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_metrics_server_recorded ON server_metrics(server_id, recorded_at);

        CREATE TABLE IF NOT EXISTS app_secrets (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| Error::other(e.to_string()))?;

    // Run migrations for existing databases
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as("PRAGMA table_info(users)")
        .fetch_all(pool)
        .await
        .map_err(|e| Error::other(e.to_string()))?;

    let column_names: Vec<&str> = columns.iter().map(|c| c.1.as_str()).collect();

    // User table migrations
    if !column_names.contains(&"is_active") {
        sqlx::query("ALTER TABLE users ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1").execute(pool).await.ok();
    }
    if !column_names.contains(&"language") {
        sqlx::query("ALTER TABLE users ADD COLUMN language TEXT NOT NULL DEFAULT 'fr'").execute(pool).await.ok();
    }
    if !column_names.contains(&"accent_color") {
        sqlx::query("ALTER TABLE users ADD COLUMN accent_color TEXT NOT NULL DEFAULT '#3A82F6'").execute(pool).await.ok();
    }
    if !column_names.contains(&"last_login") {
        sqlx::query("ALTER TABLE users ADD COLUMN last_login TEXT").execute(pool).await.ok();
    }
    if !column_names.contains(&"last_ip") {
        sqlx::query("ALTER TABLE users ADD COLUMN last_ip TEXT").execute(pool).await.ok();
    }
    if !column_names.contains(&"allocated_servers") {
        sqlx::query("ALTER TABLE users ADD COLUMN allocated_servers TEXT").execute(pool).await.ok();
    }

    // Server table migrations
    let server_columns: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as("PRAGMA table_info(servers)")
        .fetch_all(pool)
        .await
        .map_err(|e| Error::other(e.to_string()))?;

    let server_column_names: Vec<&str> = server_columns.iter().map(|c| c.1.as_str()).collect();

    if !server_column_names.contains(&"config") {
        sqlx::query("ALTER TABLE servers ADD COLUMN config TEXT").execute(pool).await.ok();
    }
    
    // New migrations for manager.json fields
    if !server_column_names.contains(&"backup_enabled") {
        sqlx::query("ALTER TABLE servers ADD COLUMN backup_enabled INTEGER NOT NULL DEFAULT 1").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"backup_frequency") {
        sqlx::query("ALTER TABLE servers ADD COLUMN backup_frequency INTEGER NOT NULL DEFAULT 30").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"backup_max_backups") {
        sqlx::query("ALTER TABLE servers ADD COLUMN backup_max_backups INTEGER NOT NULL DEFAULT 7").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"backup_prefix") {
        sqlx::query("ALTER TABLE servers ADD COLUMN backup_prefix TEXT NOT NULL DEFAULT 'hytale_backup'").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"discord_username") {
        sqlx::query("ALTER TABLE servers ADD COLUMN discord_username TEXT DEFAULT 'Hytale Bot'").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"discord_avatar") {
        sqlx::query("ALTER TABLE servers ADD COLUMN discord_avatar TEXT DEFAULT ''").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"discord_webhook_url") {
        sqlx::query("ALTER TABLE servers ADD COLUMN discord_webhook_url TEXT DEFAULT ''").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"discord_notifications") {
        sqlx::query("ALTER TABLE servers ADD COLUMN discord_notifications TEXT DEFAULT '{}'").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"logs_retention_days") {
        sqlx::query("ALTER TABLE servers ADD COLUMN logs_retention_days INTEGER NOT NULL DEFAULT 7").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"watchdog_enabled") {
        sqlx::query("ALTER TABLE servers ADD COLUMN watchdog_enabled INTEGER NOT NULL DEFAULT 1").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"auth_mode") {
        sqlx::query("ALTER TABLE servers ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'authenticated'").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"bind_address") {
        sqlx::query("ALTER TABLE servers ADD COLUMN bind_address TEXT NOT NULL DEFAULT '0.0.0.0'").execute(pool).await.ok();
    }
    if !server_column_names.contains(&"port") {
        sqlx::query("ALTER TABLE servers ADD COLUMN port INTEGER NOT NULL DEFAULT 5520").execute(pool).await.ok();
    }

    // Server players table migrations
    let players_columns: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as("PRAGMA table_info(server_players)")
        .fetch_all(pool)
        .await
        .map_err(|e| Error::other(e.to_string()))?;

    let players_column_names: Vec<&str> = players_columns.iter().map(|c| c.1.as_str()).collect();

    if !players_column_names.contains(&"player_id") {
        sqlx::query("ALTER TABLE server_players ADD COLUMN player_id TEXT").execute(pool).await.ok();
    }
    if !players_column_names.contains(&"player_ip") {
        sqlx::query("ALTER TABLE server_players ADD COLUMN player_ip TEXT").execute(pool).await.ok();
    }

    info!("✅ Migrations completed");
    Ok(())
}

