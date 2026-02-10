use axum::{
    extract::{Path, Query, State, Multipart},
    Json,
    body::Body,
    http::header,
    response::Response,
};
use std::path::{Path as StdPath, PathBuf};
use tracing::info;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::core::{AppState, error::AppError};
use crate::api::SuccessResponse;
use crate::api::servers::models::{
    FileEntry, FilesQuery, ReadFileQuery, WriteFileRequest, DeleteFileRequest, 
    CreateFolderRequest, CreateFileRequest, RenameFileRequest, CopyFileRequest, MoveFileRequest
};
use crate::utils::files::{calculate_dir_size, ensure_within_base, copy_dir_recursive};

pub async fn list_server_files(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let relative_path = query.path.clone().unwrap_or_default();
    let full_path = ensure_within_base(&working_dir, StdPath::new(&relative_path)).await?;
    
    if !full_path.exists() {
        return Err(AppError::NotFound("Path not found".into()));
    }
    
    if !full_path.is_dir() {
        return Err(AppError::BadRequest("Path is not a directory".into()));
    }
    
    let mut entries: Vec<FileEntry> = Vec::new();
    
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
    
    let mut read_dir = fs::read_dir(&full_path).await?;
    
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let entry_path = entry.path();
        
        if let Some(name) = entry_path.file_name() {
             let name_str = name.to_string_lossy();
             if name_str.ends_with(".log.lck") {
                 let _ = fs::remove_file(&entry_path).await;
                 continue;
             }
        }

        let metadata = entry.metadata().await?;
        let is_dir = metadata.is_dir();
        let size = if is_dir { 
            Some(calculate_dir_size(&entry_path).await) 
        } else { 
            Some(metadata.len()) 
        };
        let modified_at = metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&query.path)).await?;
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot read a directory".into()));
    }
    
    let content = if let Some(n) = query.tail {
        let mut file = fs::File::open(&full_path).await?;
        let metadata = file.metadata().await?;
        let len = metadata.len();
        
        let max_bytes = 256 * 1024; // 256KB
        let start_pos = len.saturating_sub(max_bytes);
        
        file.seek(std::io::SeekFrom::Start(start_pos)).await?;
        
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        
        let full_text = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = full_text.lines().collect();
        
        if lines.len() > n as usize {
            lines[lines.len() - n as usize..].join("\n")
        } else {
            full_text.into_owned()
        }
    } else {
        fs::read_to_string(&full_path).await?
    };
    
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&body.path)).await?;
    
    fs::write(&full_path, &body.content).await?;
    
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&body.path)).await?;
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        fs::remove_dir_all(&full_path).await?;
        info!("Directory deleted: {:?}", full_path);
    } else {
        fs::remove_file(&full_path).await?;
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&body.path)).await?;
    
    if full_path.exists() {
        return Err(AppError::BadRequest("Folder already exists".into()));
    }
    
    fs::create_dir_all(&full_path).await?;
    
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&body.path)).await?;
    
    if full_path.exists() {
        return Err(AppError::BadRequest("File already exists".into()));
    }
    
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    let content = body.content.unwrap_or_default();
    fs::write(&full_path, &content).await?;
    
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let mut uploaded_files: Vec<String> = Vec::new();
    let mut target_path = String::new();
    
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        
        if name == "path" {
            target_path = field.text().await.unwrap_or_default();
            continue;
        }
        
        if name == "files" || name == "file" {
            let file_name = field.file_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unnamed".to_string());
            
            // Limit file size to 100MB for now
            let data = field.bytes().await?;
            if data.len() > 100 * 1024 * 1024 {
                return Err(AppError::BadRequest(format!("File {file_name} exceeds the 100MB limit")));
            }
            
            let relative_file_path = if target_path.is_empty() {
                PathBuf::from(&file_name)
            } else {
                StdPath::new(&target_path).join(&file_name)
            };
            
            let file_path = ensure_within_base(&working_dir, &relative_file_path).await?;
            
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            
            fs::write(&file_path, &data).await?;
            
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
    
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&query.path)).await?;
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot download a directory".into()));
    }
    
    let file = fs::File::open(&full_path).await?;
    let metadata = file.metadata().await?;
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
) -> Result<Json<SuccessResponse>, AppError> {
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let full_path = ensure_within_base(&working_dir, StdPath::new(&body.path)).await?;
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if body.new_name.contains('/') || body.new_name.contains('\\') {
        return Err(AppError::BadRequest("Invalid file name".into()));
    }
    
    let new_path = full_path.parent()
        .ok_or_else(|| AppError::Internal("Cannot get parent directory".into()))?
        .join(&body.new_name);
    
    ensure_within_base(&working_dir, &new_path).await?;
    
    fs::rename(&full_path, &new_path).await?;
    
    info!("Renamed {} to {}", body.path, body.new_name);
    
    Ok(SuccessResponse::with_message("File renamed"))
}

pub async fn copy_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<CopyFileRequest>,
) -> Result<Json<SuccessResponse>, AppError> {
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let source_path = ensure_within_base(&working_dir, StdPath::new(&body.source)).await?;
    let dest_path = ensure_within_base(&working_dir, StdPath::new(&body.destination)).await?;
    
    if !source_path.exists() {
        return Err(AppError::NotFound("Source file not found".into()));
    }
    
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    if source_path.is_dir() {
        copy_dir_recursive(&source_path, &dest_path).await?;
    } else {
        fs::copy(&source_path, &dest_path).await?;
    }
    
    info!("Copied {} to {}", body.source, body.destination);
    
    Ok(SuccessResponse::with_message("File copied"))
}

pub async fn move_file(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<MoveFileRequest>,
) -> Result<Json<SuccessResponse>, AppError> {
    let server: (String,) = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?;
    
    let working_dir = PathBuf::from(server.0);
    let source_path = ensure_within_base(&working_dir, StdPath::new(&body.source)).await?;
    let dest_path = ensure_within_base(&working_dir, StdPath::new(&body.destination)).await?;
    
    if !source_path.exists() {
        return Err(AppError::NotFound("Source file not found".into()));
    }
    
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    fs::rename(&source_path, &dest_path).await?;
    
    info!("Moved {} to {}", body.source, body.destination);
    
    Ok(SuccessResponse::with_message("File moved"))
}