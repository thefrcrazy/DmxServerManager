use axum::{
    routing::get,
    extract::State,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::core::AppState;
use crate::core::database::upsert_setting;
use crate::core::error::AppError;
use crate::api::SuccessResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings).put(update_settings))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub version: String,
    pub servers_dir: String,
    pub backups_dir: String,
    pub database_path: String,
    pub webhook_url: Option<String>,
    pub is_docker: bool,
    pub login_default_color: Option<String>,
    pub login_background_url: Option<String>,
}

#[derive(Deserialize)]
struct UpdateSettingsRequest {
    webhook_url: Option<String>,
    servers_dir: Option<String>,
    backups_dir: Option<String>,
    database_path: Option<String>,
    login_default_color: Option<String>,
    login_background_url: Option<String>,
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<SettingsResponse>, AppError> {
    // Read from DB
    let settings_rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(&state.pool)
        .await?;

    // Create map for easier lookup
    let settings_map: std::collections::HashMap<String, String> = settings_rows.into_iter().collect();

    // Priority: Env > DB > Default
    let servers_dir = std::env::var("SERVERS_DIR")
        .ok()
        .or_else(|| settings_map.get("servers_dir").cloned())
        .unwrap_or_else(|| "./data/servers".into());
    
    let backups_dir = std::env::var("BACKUPS_DIR")
        .ok()
        .or_else(|| settings_map.get("backups_dir").cloned())
        .unwrap_or_else(|| "./data/backups".into());

    let settings = SettingsResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        servers_dir,
        backups_dir,
        database_path: std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:data/database.db".into()),
        webhook_url: settings_map.get("webhook_url").cloned(),
        is_docker: std::env::var("IS_DOCKER").is_ok(),
        login_default_color: settings_map.get("login_default_color").cloned(),
        login_background_url: settings_map.get("login_background_url").cloned(),
    };

    Ok(Json(settings))
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<Json<SuccessResponse>, AppError> {
    if let Some(ref webhook_url) = body.webhook_url {
        upsert_setting(&state.pool, "webhook_url", webhook_url).await?;
    }
    
    // Only update directories if not in Docker 
    let is_docker = std::env::var("IS_DOCKER").is_ok();
    if !is_docker {
        if let Some(ref dir) = body.servers_dir {
            upsert_setting(&state.pool, "servers_dir", dir).await?;
        }
        if let Some(ref dir) = body.backups_dir {
            upsert_setting(&state.pool, "backups_dir", dir).await?;
        }
        if let Some(ref db_path) = body.database_path {
            if let Err(e) = update_env_file("DATABASE_URL", db_path).await {
                error!("Failed to update .env: {e}");
            }
        }
    }

    if let Some(ref color) = body.login_default_color {
        upsert_setting(&state.pool, "login_default_color", color).await?;
    }

    if let Some(ref url) = body.login_background_url {
        upsert_setting(&state.pool, "login_background_url", url).await?;
    }

    Ok(SuccessResponse::with_message("Settings updated successfully"))
}

async fn update_env_file(key: &str, value: &str) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    let env_path = ".env";
    let mut content = String::new();
    
    // If file doesn't exist, create it
    if tokio::fs::metadata(env_path).await.is_err() {
         let mut file = tokio::fs::File::create(env_path).await?;
         file.write_all(b"").await?;
    }

    if let Ok(mut file) = tokio::fs::File::open(env_path).await {
        file.read_to_string(&mut content).await?;
    }

    let mut new_lines = Vec::new();
    let mut key_found = false;
    let new_line = format!("{key}={value}");

    for line in content.lines() {
        if line.starts_with(key) && line.contains('=') {
            new_lines.push(new_line.as_str());
            key_found = true;
        } else {
            new_lines.push(line);
        }
    }

    if !key_found {
        new_lines.push(new_line.as_str());
    }

    let new_content = new_lines.join("\n");
    let mut file = tokio::fs::File::create(env_path).await?;
    file.write_all(new_content.as_bytes()).await?;
    
    Ok(())
}
