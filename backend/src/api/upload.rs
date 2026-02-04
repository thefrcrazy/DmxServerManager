use axum::{
    routing::post,
    extract::Multipart,
    Json, Router,
};
use uuid::Uuid;
use std::io::Write;
use crate::core::{AppState, AppError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/image", post(upload_image))
}

async fn upload_image(mut multipart: Multipart) -> Result<Json<serde_json::Value>, AppError> {
    // Create uploads directory if it doesn't exist
    let upload_dir_str = std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "./data/uploads".into());
    let upload_dir = std::path::Path::new(&upload_dir_str);
    if !upload_dir.exists() {
        std::fs::create_dir_all(upload_dir)
            .map_err(|e| AppError::Internal(format!("Failed to create upload directory: {e}")))?;
    }

    // Process the multipart form
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Internal(format!("Multipart error: {e}")))? {
        
        // Get content type and validate it's an image
        let content_type = field.content_type().map(|ct| ct.to_string());
        let is_image = content_type
            .as_ref()
            .map(|ct| ct.starts_with("image/"))
            .unwrap_or(false);

        if !is_image {
            // Skip non-image fields or return error? Original code returned error on first non-image or processed first file.
            // Let's assume we want to stop if it's not an image, or just find the one that is.
            // Original code: check item, if not image return error.
            if field.name() == Some("file") || field.name().is_some() {
                 return Err(AppError::BadRequest("Only image files are allowed".into()));
            }
            continue; 
        }

        // Generate unique filename
        let extension = match content_type.as_deref() {
            Some("image/png") => "png",
            Some("image/jpeg") | Some("image/jpg") => "jpg",
            Some("image/gif") => "gif",
            Some("image/webp") => "webp",
            _ => "png",
        };
        
        let filename = format!("{}.{}", Uuid::new_v4(), extension);
        let filepath = upload_dir.join(&filename);

        // Read data
        let data = field.bytes().await.map_err(|e| AppError::Internal(format!("Failed to read chunk: {e}")))?;

        // Write file
        let mut file = std::fs::File::create(&filepath)
            .map_err(|e| AppError::Internal(format!("Failed to create file: {e}")))?;
        
        file.write_all(&data)
            .map_err(|e| AppError::Internal(format!("Failed to write file: {e}")))?;

        // Return the URL to access the uploaded file
        let url = format!("/uploads/{filename}");
        return Ok(Json(serde_json::json!({
            "success": true,
            "url": url,
            "filename": filename
        })));
    }

    Err(AppError::BadRequest("No file uploaded".into()))
}
