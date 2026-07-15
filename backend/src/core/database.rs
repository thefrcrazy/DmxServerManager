use std::{str::FromStr, time::Duration};

use serde_json::Value;
use sqlx::{
    Pool, Sqlite,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tracing::info;

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
