use std::{path::PathBuf, str::FromStr, time::Duration};

use serde_json::Value;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tracing::{info, warn};

use crate::core::error::AppError;

pub type DbPool = Pool<Sqlite>;

pub async fn init_pool(database_url: &str) -> anyhow::Result<DbPool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    info!("running DmxServerManager database migrations");
    create_pre_v1_1_backup_if_needed(pool).await?;
    sqlx::migrate!("./migrations").run(pool).await?;

    // Installers are idempotent inside their managed staging directory. Keep the
    // same job identifier so a complete staging tree can be committed and an
    // incomplete provider download can resume safely after a panel restart.
    sqlx::query(
        "UPDATE jobs SET state = 'queued', progress = 0, started_at = NULL, finished_at = NULL, \
         error_code = NULL, error_message = NULL \
         WHERE kind = 'install' AND state IN ('running', 'waiting_for_user')",
    )
    .execute(pool)
    .await?;
    // Operations that cannot prove an idempotent recovery contract are never
    // replayed implicitly after a crash.
    sqlx::query(
        "UPDATE jobs SET state = 'interrupted', finished_at = datetime('now'), \
         error_code = 'manager_restarted' \
         WHERE kind <> 'install' AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .execute(pool)
    .await?;
    // Processes are deliberately never reattached. systemd control groups, Linux
    // parent-death signals and Windows Job Objects terminate them with the panel.
    sqlx::query(
        "UPDATE instances SET runtime_state = 'stopped', updated_at = datetime('now') \
         WHERE runtime_state IN ('starting', 'running', 'stopping', 'unknown')",
    )
    .execute(pool)
    .await?;
    // A supervised process is never reattached after a manager restart, so no
    // player session can still be considered online either.
    sqlx::query(
        "UPDATE server_players SET online = 0, disconnected_at = COALESCE(disconnected_at, datetime('now')) \
         WHERE online = 1",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn create_pre_v1_1_backup_if_needed(pool: &DbPool) -> anyhow::Result<()> {
    let legacy_tables_exist: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name IN ('chat_messages', 'notifications'))",
    )
    .fetch_one(pool)
    .await?;
    if !legacy_tables_exist {
        return Ok(());
    }

    let migration_table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if migration_table_exists {
        let already_migrated: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version = 10 AND success = 1)",
        )
        .fetch_one(pool)
        .await?;
        if already_migrated {
            return Ok(());
        }
    }

    let databases: Vec<(i64, String, String)> = sqlx::query_as("PRAGMA database_list")
        .fetch_all(pool)
        .await?;
    let Some((_, _, database_file)) = databases.into_iter().find(|(_, name, _)| name == "main")
    else {
        anyhow::bail!("the main SQLite database could not be located before migration 0010");
    };
    if database_file.is_empty() {
        warn!("skipping the pre-v1.1.0 backup for an in-memory SQLite database");
        return Ok(());
    }

    let source = PathBuf::from(database_file);
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dmx-server-manager.sqlite3");
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup = source.with_file_name(format!(
        "{file_name}.pre-v1.1.0-{timestamp}-{}.backup",
        uuid::Uuid::new_v4()
    ));
    let backup_string = backup.to_string_lossy().to_string();

    sqlx::query("VACUUM main INTO ?")
        .bind(&backup_string)
        .execute(pool)
        .await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600)).await?;
    }

    let metadata = tokio::fs::metadata(&backup).await?;
    if metadata.len() == 0 {
        anyhow::bail!("the pre-v1.1.0 SQLite backup is empty");
    }
    let verification_options = SqliteConnectOptions::new()
        .filename(&backup)
        .read_only(true)
        .create_if_missing(false)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let verification_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(verification_options)
        .await?;
    let quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&verification_pool)
        .await?;
    verification_pool.close().await;
    if quick_check != "ok" {
        anyhow::bail!("the pre-v1.1.0 SQLite backup failed integrity verification: {quick_check}");
    }
    info!(path = %backup.display(), bytes = metadata.len(), "verified pre-v1.1.0 SQLite backup");
    Ok(())
}

pub async fn audit(
    pool: &DbPool,
    actor_user_id: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    outcome: &str,
    metadata: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO audit_events
            (actor_user_id, action, resource_type, resource_id, outcome, metadata, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(outcome)
    .bind(metadata.to_string())
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_0010_backs_up_and_removes_legacy_collaboration_data() {
        let root = tempfile::tempdir().unwrap();
        let database_path = root.path().join("upgrade.sqlite3");
        let database_url = format!("sqlite:{}?mode=rwc", database_path.display());
        let pool = init_pool(&database_url).await.unwrap();
        sqlx::migrate!("./migrations")
            .run_to(9, &pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES ('legacy-owner', 'owner', 'unused', 'owner', datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_messages (id, author_user_id, body, created_at) \
             VALUES ('legacy-chat', 'legacy-owner', 'legacy content', datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notifications (id, user_id, kind, message_key, data, created_at) \
             VALUES ('legacy-notification', 'legacy-owner', 'legacy', 'legacy.message', '{}', datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let legacy_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' \
             AND name IN ('chat_messages', 'notifications')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(legacy_tables, 0);

        let admin_permissions: String =
            sqlx::query_scalar("SELECT permissions FROM roles WHERE id = 'admin'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let admin_permissions: Vec<String> = serde_json::from_str(&admin_permissions).unwrap();
        for permission in [
            "audit.read",
            "panel.network.manage",
            "server.config.raw.read",
            "server.config.raw.write",
        ] {
            assert!(admin_permissions.iter().any(|item| item == permission));
        }
        let removed_permissions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM roles r, json_each(r.permissions) p \
             WHERE p.value IN ('chat.read', 'chat.write', 'notifications.read')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removed_permissions, 0);

        let mut backups = tokio::fs::read_dir(root.path()).await.unwrap();
        let mut backup_path = None;
        while let Some(entry) = backups.next_entry().await.unwrap() {
            if entry.file_name().to_string_lossy().contains(".pre-v1.1.0-") {
                backup_path = Some(entry.path());
            }
        }
        let backup_path = backup_path.expect("pre-v1.1.0 backup");
        assert!(tokio::fs::metadata(&backup_path).await.unwrap().len() > 0);

        let backup_options = SqliteConnectOptions::new()
            .filename(&backup_path)
            .read_only(true)
            .create_if_missing(false);
        let backup_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(backup_options)
            .await
            .unwrap();
        let preserved_chat: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE id = 'legacy-chat'")
                .fetch_one(&backup_pool)
                .await
                .unwrap();
        assert_eq!(preserved_chat, 1);
        backup_pool.close().await;
    }
}
