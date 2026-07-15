use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Response, StatusCode, header},
    routing::{get, post},
};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, database, error::AppError},
    domain::v1::Job,
    services::{backups, jobs},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/backups", get(list).post(create))
        .route("/backups/{id}", get(get_one).delete(remove))
        .route("/backups/{id}/download", get(download))
        .route("/backups/{id}/restore", post(restore))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstanceRequest {
    instance_id: String,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(query): axum::extract::Query<InstanceRequest>,
) -> Result<Json<Vec<backups::Backup>>, AppError> {
    validate_instance_id(&query.instance_id)?;
    authorize_instance(&state, &auth, &query.instance_id, "server.backup.read").await?;
    ensure_instance_exists(&state, &query.instance_id, false).await?;
    Ok(Json(backups::list(&state.pool, &query.instance_id).await?))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<backups::Backup>, AppError> {
    validate_backup_id(&id)?;
    let backup = backups::get(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &backup.instance_id, "server.backup.read").await?;
    Ok(Json(backup))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<InstanceRequest>,
) -> Result<(StatusCode, Json<Job>), AppError> {
    validate_instance_id(&body.instance_id)?;
    authorize_instance(&state, &auth, &body.instance_id, "server.backup").await?;
    ensure_instance_exists(&state, &body.instance_id, false).await?;
    let key = idempotency_key(&headers)?;
    let (job, created, claim) = jobs::create_claimed(
        &state.pool,
        &body.instance_id,
        "backup.create",
        &auth.id,
        key.as_deref(),
    )
    .await?;
    if created {
        let claim = claim.expect("newly-created jobs always carry a claim");
        let backup = match backups::insert(
            &state.pool,
            &body.instance_id,
            Some(&job.id),
            "manual",
            &auth.id,
        )
        .await
        {
            Ok(backup) => backup,
            Err(error) => {
                if jobs::fail(
                    &state.pool,
                    &job.id,
                    "backup_record_failed",
                    "backups.creation_failed",
                )
                .await
                .is_ok()
                    && let Err(disarm_error) = claim.disarm_terminal().await
                {
                    tracing::error!(job_id = %job.id, %disarm_error, "failed to disarm terminal backup job claim");
                }
                return Err(error);
            }
        };
        backups::spawn_create(state.clone(), job.clone(), backup.id, claim);
    } else {
        let _ = backups::get_by_creation_job(&state.pool, &job.id).await?;
    }
    state.events.publish(
        "job.queued",
        Some(body.instance_id),
        serde_json::to_value(&job).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn download(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Response<Body>, AppError> {
    validate_backup_id(&id)?;
    let instance_id = backups::instance_id_for(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &instance_id, "server.backup.read").await?;
    let (file, size, backup) =
        backups::open_verified_archive(&state.settings, &state.pool, &id).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "backup.downloaded",
        "instance",
        Some(&instance_id),
        "success",
        serde_json::json!({"backup_id": id, "size_bytes": size}),
    )
    .await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_LENGTH, size.to_string())
        .header(header::CACHE_CONTROL, "no-store")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"dmx-backup-{}.zip\"", backup.id),
        )
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|error| AppError::Internal(error.to_string()))
}

async fn restore(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    validate_backup_id(&id)?;
    let instance_id = backups::instance_id_for(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &instance_id, "server.backup").await?;
    ensure_instance_exists(&state, &instance_id, true).await?;
    let key = idempotency_key(&headers)?;
    let kind = format!("backup.restore:{id}");
    let (job, created, claim) =
        jobs::create_claimed(&state.pool, &instance_id, &kind, &auth.id, key.as_deref()).await?;
    if created {
        backups::spawn_restore(
            state.clone(),
            job.clone(),
            id,
            claim.expect("newly-created jobs always carry a claim"),
        );
    }
    state.events.publish(
        "job.queued",
        Some(instance_id),
        serde_json::to_value(&job).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    validate_backup_id(&id)?;
    let instance_id = backups::instance_id_for(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &instance_id, "server.backup").await?;
    backups::remove(&state, &id).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "backup.deleted",
        "instance",
        Some(&instance_id),
        "success",
        serde_json::json!({"backup_id": id}),
    )
    .await?;
    state.events.publish(
        "backup.deleted",
        Some(instance_id),
        serde_json::json!({"backup_id": id}),
    );
    Ok(SuccessResponse::with_message("backups.deleted"))
}

async fn ensure_instance_exists(
    state: &AppState,
    instance_id: &str,
    require_stopped: bool,
) -> Result<(), AppError> {
    let runtime_state: String =
        sqlx::query_scalar("SELECT runtime_state FROM instances WHERE id = ?")
            .bind(instance_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    if require_stopped && runtime_state != "stopped" {
        return Err(AppError::Conflict("backups.server_must_be_stopped".into()));
    }
    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| AppError::BadRequest("jobs.invalid_idempotency_key".into()))?;
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(|character| character.is_control())
    {
        return Err(AppError::BadRequest("jobs.invalid_idempotency_key".into()));
    }
    Ok(Some(value.to_string()))
}

fn validate_instance_id(id: &str) -> Result<(), AppError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))
}

fn validate_backup_id(id: &str) -> Result<(), AppError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest("backups.invalid_id".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn backup_ids_and_idempotency_keys_are_bounded() {
        assert!(validate_backup_id(&Uuid::new_v4().to_string()).is_ok());
        assert!(validate_backup_id("../../archive").is_err());
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", HeaderValue::from_static("backup-1"));
        assert_eq!(
            idempotency_key(&headers).unwrap().as_deref(),
            Some("backup-1")
        );
    }
}
