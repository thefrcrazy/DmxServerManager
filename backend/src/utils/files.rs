use std::path::{Path, PathBuf};
use tokio::fs;
use crate::core::error::AppError;
use crate::core::error::codes::ErrorCode;

pub async fn calculate_dir_size(path: &Path) -> u64 {
    // Try native 'du' command on Unix systems for speed
    #[cfg(unix)]
    {
        if let Ok(output) = tokio::process::Command::new("du")
            .arg("-sk") // -s: summary, -k: kilobytes (more portable than -b)
            .arg(path)
            .output()
            .await 
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(size_str) = stdout.split_whitespace().next() {
                    if let Ok(kib) = size_str.parse::<u64>() {
                        return kib * 1024;
                    }
                }
            }
        }
    }

    // Fallback (Windows or if du fails) - Recursive Rust implementation
    // Using spawn_blocking to avoid blocking the async runtime with heavy IO
    let path_buf = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use walkdir::WalkDir;
        WalkDir::new(path_buf)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }).await.unwrap_or(0)
}

pub async fn ensure_within_base(base: &Path, path: &Path) -> Result<PathBuf, AppError> {
    // Canonicalize base path first to resolve any symlinks in the base itself
    let base_canonical = fs::canonicalize(base).await
        .map_err(|_| AppError::Internal("Invalid base directory configuration".into()))?;

    // Use canonicalize for the check path if it exists to resolve symlinks
    if path.is_absolute() && path.exists() {
         let canonical = fs::canonicalize(path).await
            .map_err(|e| AppError::Internal(format!("Failed to resolve path: {e}")))?;
            
        if !canonical.starts_with(&base_canonical) {
            return Err(AppError::BadRequest("Access denied: path resolves outside base directory".into())
                .with_code(ErrorCode::FileAccessDenied));
        }
        return Ok(path.to_path_buf());
    }

    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };

    if full_path.exists() {
        let canonical = fs::canonicalize(&full_path).await
            .map_err(|e| AppError::Internal(format!("Failed to resolve path: {e}")))?;
            
        if !canonical.starts_with(&base_canonical) {
            return Err(AppError::BadRequest("Access denied: path resolves outside base directory".into())
                .with_code(ErrorCode::FileAccessDenied));
        }
    } else {
        // For non-existent paths, we must rely on logical check of the parent
        // But we must also ensure the parent itself doesn't resolve outside base
        if let Some(parent) = full_path.parent() {
            if parent.exists() {
                let parent_canonical = fs::canonicalize(parent).await
                    .map_err(|e| AppError::Internal(format!("Failed to resolve parent path: {e}")))?;
                
                if !parent_canonical.starts_with(&base_canonical) {
                    return Err(AppError::BadRequest("Access denied: parent path resolves outside base directory".into())
                        .with_code(ErrorCode::FileAccessDenied));
                }
            }
        }
    }

    Ok(full_path)
}

pub async fn copy_dir_recursive(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> tokio::io::Result<()> {
    fs::create_dir_all(&dst).await?;
    let mut entries = fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        let dest_path = dst.as_ref().join(entry.file_name());
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(entry_path, dest_path)).await?;
        } else {
            fs::copy(entry_path, dest_path).await?;
        }
    }
    Ok(())
}