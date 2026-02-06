use axum::{
    routing::{get, post},
    extract::{Path, Query, State},
    Json, Router,
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use tokio::fs;

use crate::core::AppState;
use crate::core::error::AppError;
use crate::core::error::codes::ErrorCode;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_backups).post(create_backup))
        .route("/:id", get(get_backup).delete(delete_backup))
        .route("/:id/restore", post(restore_backup))
}

#[derive(Debug, Serialize)]
pub struct BackupResponse {
    pub id: String,
    pub server_id: String,
    pub filename: String,
    pub size_bytes: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateBackupRequest {
    pub server_id: String,
}

#[derive(Debug, Deserialize)]
struct ListBackupsQuery {
    server_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct BackupRow {
    id: String,
    server_id: String,
    filename: String,
    size_bytes: i64,
    created_at: String,
}

async fn list_backups(
    State(state): State<AppState>,
    Query(query): Query<ListBackupsQuery>,
) -> Result<Json<Vec<BackupResponse>>, AppError> {
    let backups: Vec<BackupRow> = if let Some(server_id) = &query.server_id {
        sqlx::query_as(
            "SELECT id, server_id, filename, size_bytes, created_at FROM backups WHERE server_id = ? ORDER BY created_at DESC"
        )
        .bind(server_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, server_id, filename, size_bytes, created_at FROM backups ORDER BY created_at DESC"
        )
        .fetch_all(&state.pool)
        .await?
    };

    let responses: Vec<BackupResponse> = backups
        .into_iter()
        .map(|b| BackupResponse {
            id: b.id,
            server_id: b.server_id,
            filename: b.filename,
            size_bytes: b.size_bytes,
            created_at: b.created_at,
        })
        .collect();

    Ok(Json(responses))
}

async fn create_backup(
    State(state): State<AppState>,
    Json(body): Json<CreateBackupRequest>,
) -> Result<(StatusCode, Json<BackupResponse>), AppError> {
    let server: (String, String) = sqlx::query_as("SELECT name, working_dir FROM servers WHERE id = ?")
        .bind(&body.server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Server not found".into()).with_code(ErrorCode::ServerNotFound))?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let filename = format!(
        "backup_{}_{}.tar.gz",
        body.server_id,
        now.format("%Y%m%d_%H%M%S")
    );

    let backups_dir = std::path::Path::new("backups");
    if !backups_dir.exists() {
        fs::create_dir_all(backups_dir).await?;
    }

    let backup_path = backups_dir.join(&filename);
    let working_dir = server.1;
    let server_name = server.0;

    // If server is running, try to send a save command if supported (for Hytale, we can send /save-all if it exists)
    if state.process_manager.is_running(&body.server_id) {
        let _ = state.process_manager.send_command(&body.server_id, "/save-all").await;
        // Give it a moment to save
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    
    // Call service
    let size_bytes = crate::services::system::backup::create_archive(working_dir, backup_path.to_string_lossy().to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Backup failed: {e}"))
            .with_code(ErrorCode::BackupCreateFailed))?;

    let created_at = now.to_rfc3339();

    sqlx::query(
        "INSERT INTO backups (id, server_id, filename, size_bytes, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&body.server_id)
    .bind(&filename)
    .bind(size_bytes as i64)
    .bind(&created_at)
    .execute(&state.pool)
    .await?;

    // Discord notification
    let pool_clone = state.pool.clone();
    tokio::spawn(async move {
        let _ = crate::services::system::discord::send_notification(
            &pool_clone,
            "💾 Sauvegarde Créée",
            &format!("Une nouvelle sauvegarde a été créée pour le serveur **{server_name}**."),
            crate::services::system::discord::COLOR_SUCCESS,
            Some(&server_name),
            None,
        ).await;
    });

    Ok((StatusCode::CREATED, Json(BackupResponse {
        id,
        server_id: body.server_id.clone(),
        filename,
        size_bytes: size_bytes as i64,
        created_at,
    })))
}

async fn get_backup(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BackupResponse>, AppError> {
    let backup: BackupRow = sqlx::query_as(
        "SELECT id, server_id, filename, size_bytes, created_at FROM backups WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Backup not found".into()).with_code(ErrorCode::BackupNotFound))?;

    Ok(Json(BackupResponse {
        id: backup.id,
        server_id: backup.server_id,
        filename: backup.filename,
        size_bytes: backup.size_bytes,
        created_at: backup.created_at,
    }))
}

async fn delete_backup(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let backup: Option<(String,)> = sqlx::query_as("SELECT filename FROM backups WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?;

    if let Some((filename,)) = backup {
         let backups_dir = std::path::Path::new("backups");
         let file_path = backups_dir.join(filename);
         if file_path.exists() {
             fs::remove_file(file_path).await?;
         }
    }

    let result = sqlx::query("DELETE FROM backups WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Backup not found".into()).with_code(ErrorCode::BackupNotFound));
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn restore_backup(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let backup: BackupRow = sqlx::query_as(
        "SELECT id, server_id, filename, size_bytes, created_at FROM backups WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Backup not found".into()).with_code(ErrorCode::BackupNotFound))?;

    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&backup.server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Server not found".into()).with_code(ErrorCode::ServerNotFound))?;

    let backups_dir = std::path::Path::new("backups");
    let file_path = backups_dir.join(&backup.filename);
    
    // If server is running, stop it first
    if state.process_manager.is_running(&backup.server_id) {
        state.process_manager.stop(&backup.server_id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    crate::services::system::backup::extract_archive(file_path.to_string_lossy().to_string(), server.0)
        .await
        .map_err(|e| AppError::Internal(format!("Restore failed: {e}"))
            .with_code(ErrorCode::BackupRestoreFailed))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Restoring backup {} for server {}", backup.filename, backup.server_id)
    })))
}