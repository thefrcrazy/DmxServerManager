use axum::{
    extract::{Path, Query, State, Multipart},
    Json,
    body::Body,
    http::header,
    response::Response,
};
use std::path::Path as StdPath;
use tracing::info;

use crate::{AppState, error::AppError};
use super::models::{FileEntry, FilesQuery, ReadFileQuery, WriteFileRequest, DeleteFileRequest, CreateFolderRequest, CreateFileRequest, RenameFileRequest, CopyFileRequest, MoveFileRequest};

fn calculate_dir_size(path: &StdPath) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                total += calculate_dir_size(&entry_path);
            } else if let Ok(metadata) = entry_path.metadata() {
                total += metadata.len();
            }
        }
    }
    total
}

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
    if !full_path.starts_with(base_path) {
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
            modified_at: None,
        });
    }
    
    // Read directory entries
    let read_dir = std::fs::read_dir(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read directory: {e}")))?;
    
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
        let metadata = entry_path.metadata().ok();
        let size = if is_dir { 
            Some(calculate_dir_size(&entry_path)) 
        } else { 
            metadata.as_ref().map(|m| m.len()) 
        };
        let modified_at = metadata.and_then(|m| {
            m.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
            })
        });
        
        if let Some(name) = entry_path.file_name() {
            let name_str = name.to_string_lossy().to_string();
            let rel_path = if relative_path.is_empty() {
                name_str.clone()
            } else {
                format!("{relative_path}/{name_str}")
            };
            
            entries.push(FileEntry {
                name: name_str,
                path: rel_path,
                is_dir,
                size,
                modified_at,
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot read a directory".into()));
    }
    
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read file: {e}")))?;
    
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    std::fs::write(&full_path, &body.content)
        .map_err(|e| AppError::Internal(format!("Failed to write file: {e}")))?;
    
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    // Delete file or directory
    if full_path.is_dir() {
        std::fs::remove_dir_all(&full_path)
            .map_err(|e| AppError::Internal(format!("Failed to delete directory: {e}")))?;
        info!("Directory deleted: {:?}", full_path);
    } else {
        std::fs::remove_file(&full_path)
            .map_err(|e| AppError::Internal(format!("Failed to delete file: {e}")))?;
        info!("File deleted: {:?}", full_path);
    }
    
    Ok(Json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

pub async fn create_folder(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<CreateFolderRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if full_path.exists() {
        return Err(AppError::BadRequest("Folder already exists".into()));
    }
    
    std::fs::create_dir_all(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to create folder: {e}")))?;
    
    info!("Folder created: {:?}", full_path);
    
    Ok(Json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

pub async fn create_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<CreateFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if full_path.exists() {
        return Err(AppError::BadRequest("File already exists".into()));
    }
    
    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Internal(format!("Failed to create parent directories: {e}")))?;
        }
    }
    
    let content = body.content.unwrap_or_default();
    std::fs::write(&full_path, &content)
        .map_err(|e| AppError::Internal(format!("Failed to create file: {e}")))?;
    
    info!("File created: {:?}", full_path);
    
    Ok(Json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

pub async fn upload_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let mut uploaded_files: Vec<String> = Vec::new();
    let mut target_path = String::new();
    
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::BadRequest(format!("Failed to read multipart: {e}")))? {
        let name = field.name().unwrap_or("").to_string();
        
        if name == "path" {
            target_path = field.text().await.unwrap_or_default();
            continue;
        }
        
        if name == "files" || name == "file" {
            let file_name = field.file_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unnamed".to_string());
            
            let data = field.bytes().await.map_err(|e| AppError::Internal(format!("Failed to read file data: {e}")))?;
            
            let file_path = if target_path.is_empty() {
                base_path.join(&file_name)
            } else {
                base_path.join(&target_path).join(&file_name)
            };
            
            // Security check
            if !file_path.starts_with(base_path) {
                return Err(AppError::BadRequest("Invalid path".into()));
            }
            
            // Create parent directories if needed
            if let Some(parent) = file_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| AppError::Internal(format!("Failed to create directories: {e}")))?;
                }
            }
            
            std::fs::write(&file_path, &data)
                .map_err(|e| AppError::Internal(format!("Failed to write file: {e}")))?;
            
            info!("File uploaded: {:?}", file_path);
            uploaded_files.push(file_name);
        }
    }
    
    Ok(Json(serde_json::json!({
        "success": true,
        "uploaded": uploaded_files
    })))
}

pub async fn download_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Response<Body>, AppError> {
    
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
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot download a directory".into()));
    }
    
    let file = tokio::fs::File::open(&full_path).await
        .map_err(|e| AppError::Internal(format!("Failed to open file: {e}")))?;
    
    let metadata = file.metadata().await
        .map_err(|e| AppError::Internal(format!("Failed to get file metadata: {e}")))?;
    let size = metadata.len();

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);
    
    let file_name = full_path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    
    let content_disposition = format!("attachment; filename=\"{file_name}\"");
    
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .header(header::CONTENT_LENGTH, size.to_string())
        .body(body)
        .unwrap())
}

pub async fn rename_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<RenameFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let full_path = base_path.join(&body.path);
    
    if !full_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    // Validate new name (no path separators)
    if body.new_name.contains('/') || body.new_name.contains('\\') {
        return Err(AppError::BadRequest("Invalid file name".into()));
    }
    
    let new_path = full_path.parent()
        .ok_or_else(|| AppError::Internal("Cannot get parent directory".into()))?
        .join(&body.new_name);
    
    if !new_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid destination path".into()));
    }
    
    std::fs::rename(&full_path, &new_path)
        .map_err(|e| AppError::Internal(format!("Failed to rename: {e}")))?;
    
    info!("Renamed {} to {}", body.path, body.new_name);
    
    Ok(Json(serde_json::json!({ "success": true, "message": "File renamed" })))
}

pub async fn copy_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<CopyFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let source_path = base_path.join(&body.source);
    let dest_path = base_path.join(&body.destination);
    
    if !source_path.starts_with(base_path) || !dest_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !source_path.exists() {
        return Err(AppError::NotFound("Source file not found".into()));
    }
    
    // Create parent directories if needed
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("Failed to create directories: {e}")))?;
    }
    
    if source_path.is_dir() {
        // Recursive copy for directories
        copy_dir_recursive(&source_path, &dest_path)
            .map_err(|e| AppError::Internal(format!("Failed to copy directory: {e}")))?;
    } else {
        std::fs::copy(&source_path, &dest_path)
            .map_err(|e| AppError::Internal(format!("Failed to copy file: {e}")))?;
    }
    
    info!("Copied {} to {}", body.source, body.destination);
    
    Ok(Json(serde_json::json!({ "success": true, "message": "File copied" })))
}

fn copy_dir_recursive(src: &StdPath, dst: &StdPath) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            std::fs::copy(&entry_path, &dest_path)?;
        }
    }
    Ok(())
}

pub async fn move_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<MoveFileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = StdPath::new(&working_dir);
    let source_path = base_path.join(&body.source);
    let dest_path = base_path.join(&body.destination);
    
    if !source_path.starts_with(base_path) || !dest_path.starts_with(base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !source_path.exists() {
        return Err(AppError::NotFound("Source file not found".into()));
    }
    
    // Create parent directories if needed
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("Failed to create directories: {e}")))?;
    }
    
    std::fs::rename(&source_path, &dest_path)
        .map_err(|e| AppError::Internal(format!("Failed to move file: {e}")))?;
    
    info!("Moved {} to {}", body.source, body.destination);
    
    Ok(Json(serde_json::json!({ "success": true, "message": "File moved" })))
}
