use actix_multipart::Multipart;
use actix_web::{web, HttpResponse, Result};
use futures::StreamExt;
use std::io::Write;
use uuid::Uuid;

use crate::error::AppError;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/upload")
            .route("/image", web::post().to(upload_image)),
    );
}

async fn upload_image(mut payload: Multipart) -> Result<HttpResponse, AppError> {
    // Create uploads directory if it doesn't exist
    let upload_dir_str = std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "./data/uploads".into());
    let upload_dir = std::path::Path::new(&upload_dir_str);
    if !upload_dir.exists() {
        std::fs::create_dir_all(upload_dir)
            .map_err(|e| AppError::Internal(format!("Failed to create upload directory: {}", e)))?;
    }

    // Process the multipart form
    while let Some(item) = payload.next().await {
        let mut field = item.map_err(|e| AppError::Internal(format!("Multipart error: {}", e)))?;
        
        // Get content type and validate it's an image
        let content_type = field.content_type().map(|ct| ct.to_string());
        let is_image = content_type
            .as_ref()
            .map(|ct| ct.starts_with("image/"))
            .unwrap_or(false);

        if !is_image {
            return Err(AppError::BadRequest("Only image files are allowed".into()));
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

        // Write file
        let mut file = std::fs::File::create(&filepath)
            .map_err(|e| AppError::Internal(format!("Failed to create file: {}", e)))?;

        // Stream the file chunks
        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| AppError::Internal(format!("Chunk error: {}", e)))?;
            file.write_all(&data)
                .map_err(|e| AppError::Internal(format!("Failed to write file: {}", e)))?;
        }

        // Return the URL to access the uploaded file
        let url = format!("/uploads/{}", filename);
        return Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "url": url,
            "filename": filename
        })));
    }

    Err(AppError::BadRequest("No file uploaded".into()))
}
