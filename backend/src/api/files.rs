use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderMap, Response, StatusCode, header},
    routing::{get, post},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::time::timeout;
use tokio_util::io::ReaderStream;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, database, error::AppError},
    services::{
        instance_storage,
        runtime::FilesystemLease,
        secure_fs::{self, MAX_TEXT_BYTES, MAX_UPLOAD_BYTES, ManagedEntry},
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/files", get(list).delete(remove))
        .route("/files/content", get(download).put(upload))
        .route("/files/text", get(read_text).put(write_text))
        .route("/files/directories", post(make_directory))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileQuery {
    instance_id: String,
    #[serde(default)]
    path: String,
}

#[derive(Debug, Serialize)]
struct FileList {
    items: Vec<ManagedEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextWriteRequest {
    content: String,
}

#[derive(Debug, Serialize)]
struct TextReadResponse {
    content: String,
}

#[derive(Debug, Serialize)]
struct FileWriteResponse {
    bytes_written: u64,
}

struct AuthorizedInstanceRoot {
    root: std::path::PathBuf,
    lease: FilesystemLease,
}

const FILE_UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_UPLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileList>, AppError> {
    let root = authorized_read_root(&state, &auth, &query.instance_id).await?;
    let items = secure_fs::list_directory(&root, &query.path).await?;
    Ok(Json(FileList { items }))
}

async fn download(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
) -> Result<Response<Body>, AppError> {
    let root = authorized_read_root(&state, &auth, &query.instance_id).await?;
    let (file, size) = secure_fs::open_regular_file(&root, &query.path).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "file.downloaded",
        "instance",
        Some(&query.instance_id),
        "success",
        serde_json::json!({"path": query.path, "size_bytes": size}),
    )
    .await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size.to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<FileWriteResponse>), AppError> {
    reject_oversized_content_length(&headers, MAX_UPLOAD_BYTES)?;
    super::servers::validate_instance_id(&query.instance_id)?;
    authorize_instance(&state, &auth, &query.instance_id, "server.files.write").await?;
    let bytes = read_upload_body(
        body,
        MAX_UPLOAD_BYTES,
        FILE_UPLOAD_IDLE_TIMEOUT,
        FILE_UPLOAD_TOTAL_TIMEOUT,
    )
    .await?;
    let authorized =
        authorized_instance_root(&state, &auth, &query.instance_id, "server.files.write").await?;
    let bytes_written = secure_fs::write_bytes(
        &authorized.root,
        &query.path,
        bytes,
        MAX_UPLOAD_BYTES as usize,
    )
    .await?;
    audit_mutation(
        &state,
        &auth,
        &query,
        "file.uploaded",
        serde_json::json!({"size_bytes": bytes_written}),
    )
    .await?;
    authorized.lease.release().await?;
    Ok((
        StatusCode::CREATED,
        Json(FileWriteResponse { bytes_written }),
    ))
}

async fn read_text(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
) -> Result<Json<TextReadResponse>, AppError> {
    let root = authorized_read_root(&state, &auth, &query.instance_id).await?;
    let (file, size) = secure_fs::open_regular_file(&root, &query.path).await?;
    if size > MAX_TEXT_BYTES as u64 {
        return Err(AppError::BadRequest("files.text_too_large".into()));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_TEXT_BYTES || bytes.contains(&0) {
        return Err(AppError::BadRequest("files.not_text".into()));
    }
    let content =
        String::from_utf8(bytes).map_err(|_| AppError::BadRequest("files.not_utf8".into()))?;
    Ok(Json(TextReadResponse { content }))
}

async fn write_text(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
    Json(body): Json<TextWriteRequest>,
) -> Result<Json<FileWriteResponse>, AppError> {
    if body.content.len() > MAX_TEXT_BYTES
        || body.content.contains('\0')
        || body
            .content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(AppError::BadRequest("files.invalid_text".into()));
    }
    let authorized =
        authorized_instance_root(&state, &auth, &query.instance_id, "server.files.write").await?;
    let bytes_written = secure_fs::write_bytes(
        &authorized.root,
        &query.path,
        Bytes::from(body.content),
        MAX_TEXT_BYTES,
    )
    .await?;
    audit_mutation(
        &state,
        &auth,
        &query,
        "file.text_written",
        serde_json::json!({"size_bytes": bytes_written}),
    )
    .await?;
    authorized.lease.release().await?;
    Ok(Json(FileWriteResponse { bytes_written }))
}

async fn make_directory(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
) -> Result<(StatusCode, Json<SuccessResponse>), AppError> {
    let authorized =
        authorized_instance_root(&state, &auth, &query.instance_id, "server.files.write").await?;
    secure_fs::create_directory(&authorized.root, &query.path).await?;
    audit_mutation(
        &state,
        &auth,
        &query,
        "file.directory_created",
        serde_json::json!({}),
    )
    .await?;
    authorized.lease.release().await?;
    Ok((StatusCode::CREATED, SuccessResponse::ok()))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<FileQuery>,
) -> Result<Json<SuccessResponse>, AppError> {
    let authorized =
        authorized_instance_root(&state, &auth, &query.instance_id, "server.files.write").await?;
    secure_fs::delete_entry(&authorized.root, &query.path).await?;
    audit_mutation(&state, &auth, &query, "file.deleted", serde_json::json!({})).await?;
    authorized.lease.release().await?;
    Ok(SuccessResponse::with_message("files.deleted"))
}

async fn authorized_instance_root(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
    permission: &str,
) -> Result<AuthorizedInstanceRoot, AppError> {
    super::servers::validate_instance_id(instance_id)?;
    authorize_instance(state, auth, instance_id, permission).await?;
    let lease = state
        .runtime
        .begin_filesystem_maintenance(instance_id)
        .await?;
    let root = instance_storage::resolve(&state.pool, &state.settings, instance_id)
        .await?
        .root;
    Ok(AuthorizedInstanceRoot { root, lease })
}

/// Read-only file operations use descriptor/path confinement and do not mutate
/// the instance tree, so they remain available while a game is running or has
/// crashed. Exclusive runtime maintenance is reserved for writes, deletes and
/// directory creation; otherwise a crashed instance with a desired running
/// state could never expose its diagnostic files.
async fn authorized_read_root(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
) -> Result<std::path::PathBuf, AppError> {
    super::servers::validate_instance_id(instance_id)?;
    authorize_instance(state, auth, instance_id, "server.files.read").await?;
    Ok(
        instance_storage::resolve(&state.pool, &state.settings, instance_id)
            .await?
            .root,
    )
}

async fn read_upload_body(
    body: Body,
    maximum: u64,
    idle_timeout: Duration,
    total_timeout: Duration,
) -> Result<Bytes, AppError> {
    timeout(total_timeout, async move {
        let mut stream = body.into_data_stream();
        let mut bytes = Vec::new();
        loop {
            let next = timeout(idle_timeout, stream.next())
                .await
                .map_err(|_| AppError::BadRequest("files.upload_timeout".into()))?;
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|_| AppError::BadRequest("files.upload_invalid".into()))?;
            let new_length = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| AppError::BadRequest("files.upload_too_large".into()))?;
            if u64::try_from(new_length).unwrap_or(u64::MAX) > maximum {
                return Err(AppError::BadRequest("files.upload_too_large".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(bytes))
    })
    .await
    .map_err(|_| AppError::BadRequest("files.upload_timeout".into()))?
}

fn reject_oversized_content_length(headers: &HeaderMap, maximum: u64) -> Result<(), AppError> {
    if let Some(length) = headers.get(header::CONTENT_LENGTH) {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| AppError::BadRequest("files.invalid_content_length".into()))?;
        if length > maximum {
            return Err(AppError::BadRequest("files.upload_too_large".into()));
        }
    }
    Ok(())
}

async fn audit_mutation(
    state: &AppState,
    auth: &AuthUser,
    query: &FileQuery,
    action: &str,
    mut metadata: serde_json::Value,
) -> Result<(), AppError> {
    metadata["path"] = serde_json::Value::String(query.path.clone());
    database::audit(
        &state.pool,
        Some(&auth.id),
        action,
        "instance",
        Some(&query.instance_id),
        "success",
        metadata,
    )
    .await?;
    state.events.publish(
        action,
        Some(query.instance_id.clone()),
        serde_json::json!({"path": query.path}),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use futures::stream;

    #[test]
    fn content_length_is_strictly_bounded() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("1048577"));
        assert!(reject_oversized_content_length(&headers, MAX_UPLOAD_BYTES).is_err());
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("1048576"));
        assert!(reject_oversized_content_length(&headers, MAX_UPLOAD_BYTES).is_ok());
    }

    #[tokio::test]
    async fn stalled_upload_body_is_bounded_by_idle_timeout() {
        let body = Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
        let error = read_upload_body(
            body,
            MAX_UPLOAD_BYTES,
            Duration::from_millis(10),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, AppError::BadRequest(message) if message == "files.upload_timeout")
        );
    }
}
