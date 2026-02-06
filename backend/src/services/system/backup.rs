use std::fs::File;
use std::path::Path;
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use tar::Archive;
use crate::core::error::AppError;
use crate::core::error::codes::ErrorCode;

pub async fn create_archive(source_dir: String, backup_file_path: String) -> Result<u64, AppError> {
    tokio::task::spawn_blocking(move || {
        let source_path = Path::new(&source_dir);
        let backup_path = Path::new(&backup_file_path);

        if !source_path.exists() {
            return Err(AppError::NotFound(format!("Source directory not found: {source_dir}"))
                .with_code(ErrorCode::ServerDirMissing));
        }

        if let Some(parent) = backup_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let tar_gz = File::create(backup_path)?;
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);

        tar.append_dir_all(".", source_path)?;
        tar.finish()?;

        let metadata = std::fs::metadata(backup_path)?;
        Ok(metadata.len())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Spawn blocking failed: {e}")))?
}

pub async fn extract_archive(backup_file_path: String, dest_dir: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || {
        let backup_path = Path::new(&backup_file_path);
        let dest_path = Path::new(&dest_dir);

        if !backup_path.exists() {
            return Err(AppError::NotFound(format!("Backup file not found: {backup_file_path}"))
                .with_code(ErrorCode::BackupNotFound));
        }

        if !dest_path.exists() {
            let _ = std::fs::create_dir_all(dest_path);
        }

        let tar_gz = File::open(backup_path)?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);

        archive.unpack(dest_path)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(format!("Spawn blocking failed: {e}")))?
}
