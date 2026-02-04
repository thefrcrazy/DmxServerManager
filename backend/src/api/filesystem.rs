use axum::{
    routing::get,
    extract::Query,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::{AppState, AppError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/list", get(list_directory))
}

#[derive(Debug, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,
}

async fn list_directory(Query(query): Query<ListQuery>) -> Result<Json<serde_json::Value>, AppError> {
    let base_path = query.path.clone().unwrap_or_else(|| "/".to_string());
    let path = PathBuf::from(&base_path);

    if !path.exists() {
        return Err(AppError::NotFound(format!("Path not found: {base_path}")));
    }

    if !path.is_dir() {
        return Err(AppError::BadRequest("Path is not a directory".into()));
    }

    let mut entries: Vec<DirectoryEntry> = Vec::new();

    // Add parent directory if not at root
    if base_path != "/" {
        if let Some(parent) = path.parent() {
            entries.push(DirectoryEntry {
                name: "..".to_string(),
                path: parent.to_string_lossy().to_string(),
                is_dir: true,
            });
        }
    }

    // Read directory entries
    let read_dir = std::fs::read_dir(&path)
        .map_err(|e| AppError::Internal(format!("Failed to read directory: {e}")))?;

    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        let is_dir = entry_path.is_dir();
        
        // Only show directories for the picker
        if is_dir {
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy().to_string();
                // Skip hidden directories
                if !name_str.starts_with('.') {
                    entries.push(DirectoryEntry {
                        name: name_str,
                        path: entry_path.to_string_lossy().to_string(),
                        is_dir: true,
                    });
                }
            }
        }
    }

    // Sort alphabetically
    entries.sort_by(|a, b| {
        if a.name == ".." {
            std::cmp::Ordering::Less
        } else if b.name == ".." {
            std::cmp::Ordering::Greater
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    Ok(Json(serde_json::json!({
        "current_path": base_path,
        "entries": entries
    })))
}
