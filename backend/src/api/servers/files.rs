use axum::{
    extract::{Path, Query, State},
    Json,
};
use std::path::Path as StdPath;
use tracing::info;
use crate::{AppState, error::AppError};
use super::models::{FileEntry, FilesQuery, ReadFileQuery, WriteFileRequest, DeleteFileRequest};

pub async fn list_server_files(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?
        .0;
    
    // Build the path - relative to working_dir (includes server/ and manager/)
    let base_path = StdPath::new(&working_dir);
    let relative_path = query.path.clone().unwrap_or_default();
    let full_path = if relative_path.is_empty() {
        base_path.to_path_buf()
    } else {
        base_path.join(&relative_path)
    };
    
    // Security: ensure path is within server directory
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("Path not found".into()));
    }
    
    if !full_path.is_dir() {
        return Err(AppError::BadRequest("Path is not a directory".into()));
    }
    
    let mut entries: Vec<FileEntry> = Vec::new();
    
    // Add parent directory if not at root
    if !relative_path.is_empty() {
        let parent = StdPath::new(&relative_path).parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        entries.push(FileEntry {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            size: None,
        });
    }
    
    // Read directory entries
    let read_dir = std::fs::read_dir(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read directory: {}", e)))?;
    
    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        
        // Auto-delete .log.lck files
        if let Some(name) = entry_path.file_name() {
             let name_str = name.to_string_lossy();
             if name_str.ends_with(".log.lck") {
                 let _ = std::fs::remove_file(&entry_path);
                 continue;
             }
        }

        let is_dir = entry_path.is_dir();
        let size = if is_dir { None } else { entry_path.metadata().ok().map(|m| m.len()) };
        
        if let Some(name) = entry_path.file_name() {
            let name_str = name.to_string_lossy().to_string();
            let rel_path = if relative_path.is_empty() {
                name_str.clone()
            } else {
                format!("{}/{}", relative_path, name_str)
            };
            
            entries.push(FileEntry {
                name: name_str,
                path: rel_path,
                is_dir,
                size,
            });
        }
    }
    
    // Sort: directories first, then alphabetically
    entries.sort_by(|a, b| {
        if a.name == ".." {
            std::cmp::Ordering::Less
        } else if b.name == ".." {
            std::cmp::Ordering::Greater
        } else if a.is_dir && !b.is_dir {
            std::cmp::Ordering::Less
        } else if !a.is_dir && b.is_dir {
            std::cmp::Ordering::Greater
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    
    Ok(Json(serde_json::json!({
        "current_path": relative_path,
        "entries": entries
    })))
}

pub async fn read_server_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let full_path = base_path.join(&query.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot read a directory".into()));
    }
    
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read file: {}", e)))?;
    
    Ok(Json(serde_json::json!({
        "path": query.path,
        "content": content
    })))
}

pub async fn write_server_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<WriteFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let full_path = base_path.join(&body.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    std::fs::write(&full_path, &body.content)
        .map_err(|e| AppError::Internal(format!("Failed to write file: {}", e)))?;
    
    info!("File written: {:?}", full_path);
    
    Ok(Json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

pub async fn delete_server_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<DeleteFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let full_path = base_path.join(&body.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
         return Err(AppError::BadRequest("Cannot delete a directory with this endpoint".into()));
    }
    
    std::fs::remove_file(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to delete file: {}", e)))?;
    
    info!("File deleted: {:?}", full_path);
    
    Ok(Json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}
