use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Response, StatusCode, header},
    routing::{get, post},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio_util::io::ReaderStream;

use crate::{
    api::{SuccessResponse, auth::AuthUser},
    core::{AppState, database, error::AppError},
    domain::v1::Job,
    services::{catalog, jobs},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/catalog", get(list))
        .route("/catalog/import", post(import))
        .route(
            "/catalog/theme",
            get(get_active_theme).put(set_active_theme),
        )
        .route("/catalog/{kind}/{id}/revisions", get(revisions))
        .route(
            "/catalog/{kind}/{id}/revisions/{revision}",
            get(get_revision).delete(remove_revision),
        )
        .route(
            "/catalog/{kind}/{id}/revisions/{revision}/assets/{asset}",
            get(asset),
        )
}

async fn get_active_theme(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<(HeaderMap, Json<catalog::ActiveTheme>), AppError> {
    let theme = catalog::active_theme(&state.pool).await?;
    Ok((version_headers(theme.version)?, Json(theme)))
}

async fn set_active_theme(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(selection): Json<catalog::ThemeSelection>,
) -> Result<(HeaderMap, Json<catalog::ActiveTheme>), AppError> {
    require_owner(&auth)?;
    let expected_version = parse_if_match(&headers)?;
    let theme = catalog::select_theme(&state.pool, &auth.id, &selection, expected_version).await?;
    state.events.publish(
        "catalog.theme_changed",
        None,
        serde_json::json!({"selection": &theme.selection, "version": theme.version}),
    );
    Ok((version_headers(theme.version)?, Json(theme)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    kind: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<catalog::CatalogPackage>>, AppError> {
    auth.require("profile.read")?;
    Ok(Json(
        catalog::list(&state.pool, query.kind.as_deref()).await?,
    ))
}

async fn revisions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Json<Vec<catalog::CatalogPackage>>, AppError> {
    auth.require("profile.read")?;
    Ok(Json(catalog::revisions(&state.pool, &kind, &id).await?))
}

async fn get_revision(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((kind, id, revision)): Path<(String, String, u32)>,
) -> Result<Json<catalog::CatalogPackage>, AppError> {
    auth.require("profile.read")?;
    Ok(Json(catalog::get(&state.pool, &kind, &id, revision).await?))
}

async fn import(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<Job>), AppError> {
    require_owner(&auth)?;
    validate_upload_headers(&headers)?;
    let archive_sha256 = archive_checksum(&headers)?;
    let key = idempotency_key(&headers)?.map(|key| namespace_idempotency_key(&auth.id, &key));
    let kind = format!("catalog.import:{archive_sha256}");
    let (job, created, claim) =
        jobs::create_global_claimed(&state.pool, &kind, &auth.id, key.as_deref()).await?;
    if !created {
        debug_assert!(claim.is_none());
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let claim = claim.ok_or_else(|| {
        AppError::Internal("new catalogue import job was created without a claim".into())
    })?;

    let upload = catalog::stage_upload(
        &state.settings,
        &job.id,
        &archive_sha256,
        body.into_data_stream(),
    )
    .await;
    if let Err(error) = upload {
        if let Err(fail_error) = jobs::fail(
            &state.pool,
            &job.id,
            "catalog_upload_rejected",
            "catalog.upload_rejected",
        )
        .await
        {
            tracing::error!(
                job_id = %job.id,
                %fail_error,
                "failed to persist rejected catalogue upload job state"
            );
        }
        if let Err(claim_error) = claim.disarm_terminal().await {
            // A still-active claim schedules its interruption as this method
            // returns. A transient database error is also retried by Drop.
            tracing::error!(
                job_id = %job.id,
                %claim_error,
                "rejected catalogue upload job was not terminal when releasing its claim"
            );
        }
        let _ = database::audit(
            &state.pool,
            Some(&auth.id),
            "catalog.import_rejected",
            "catalog_revision",
            None,
            "failure",
            serde_json::json!({"job_id": job.id, "archive_sha256": archive_sha256}),
        )
        .await;
        return Err(error);
    }

    spawn_import(state.clone(), job.clone(), auth.id, archive_sha256, claim);
    state.events.publish(
        "job.queued",
        None,
        serde_json::to_value(&job).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(job)))
}

fn spawn_import(
    state: AppState,
    job: Job,
    actor_id: String,
    archive_sha256: String,
    claim: jobs::JobClaim,
) {
    // Construct the future (and move the claim into it) before handing it to
    // Tokio. Aborting the JoinHandle before its first poll therefore still
    // drops the armed claim and interrupts the job.
    let import_task = async move {
        let result = async {
            if !jobs::begin(&state.pool, &job.id).await? {
                return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
            }
            let package = catalog::import_staged(
                &state.pool,
                &state.settings,
                &state.profiles,
                &job.id,
                &actor_id,
                &archive_sha256,
            )
            .await?;
            jobs::succeed(&state.pool, &job.id).await?;
            if let Err(error) = database::audit(
                &state.pool,
                Some(&actor_id),
                "catalog.revision_imported",
                "catalog_revision",
                Some(&package.id),
                "success",
                serde_json::json!({
                    "job_id": job.id,
                    "kind": package.kind,
                    "revision": package.revision,
                    "archive_sha256": package.archive_sha256,
                }),
            )
            .await
            {
                tracing::error!(job_id = %job.id, %error, "catalogue import audit write failed");
            }
            state.events.publish(
                "catalog.revision_imported",
                None,
                serde_json::to_value(&package).unwrap_or_default(),
            );
            Ok::<(), AppError>(())
        }
        .await;

        let mut failure_persisted = false;
        if let Err(error) = result {
            tracing::warn!(job_id = %job.id, %error, "catalogue import job failed");
            match jobs::fail(
                &state.pool,
                &job.id,
                "catalog_import_failed",
                "catalog.import_failed",
            )
            .await
            {
                Ok(()) => failure_persisted = true,
                Err(fail_error) => tracing::error!(
                    job_id = %job.id,
                    %fail_error,
                    "failed to persist catalogue import job failure"
                ),
            }
            let _ = database::audit(
                &state.pool,
                Some(&actor_id),
                "catalog.import_failed",
                "catalog_revision",
                None,
                "failure",
                serde_json::json!({"job_id": job.id, "archive_sha256": archive_sha256}),
            )
            .await;
            if failure_persisted {
                state.events.publish(
                    "job.failed",
                    None,
                    serde_json::json!({"job_id": job.id, "error_code": "catalog_import_failed"}),
                );
            }
        }
        catalog::discard_staging(&state.settings, &job.id).await;
        if let Err(error) = claim.disarm_terminal().await {
            tracing::error!(
                job_id = %job.id,
                %error,
                "catalogue import worker exited without a persisted terminal job state"
            );
        }
    };
    std::mem::drop(tokio::spawn(import_task));
}

async fn remove_revision(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((kind, id, revision)): Path<(String, String, u32)>,
) -> Result<Json<SuccessResponse>, AppError> {
    require_owner(&auth)?;
    let removed = catalog::remove(
        &state.pool,
        &state.settings,
        &state.profiles,
        &kind,
        &id,
        revision,
    )
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "catalog.revision_deleted",
        "catalog_revision",
        Some(&id),
        "success",
        serde_json::json!({
            "kind": kind,
            "revision": revision,
            "archive_sha256": removed.archive_sha256,
        }),
    )
    .await?;
    state.events.publish(
        "catalog.revision_deleted",
        None,
        serde_json::json!({"kind": kind, "id": id, "revision": revision}),
    );
    Ok(SuccessResponse::with_message("catalog.revision_deleted"))
}

async fn asset(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((kind, id, revision, asset)): Path<(String, String, u32, String)>,
) -> Result<Response<Body>, AppError> {
    if kind != "theme" {
        auth.require("profile.read")?;
    }
    let asset =
        catalog::open_asset(&state.pool, &state.settings, &kind, &id, revision, &asset).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, asset.media_type)
        .header(header::CONTENT_LENGTH, asset.size_bytes.to_string())
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .header(header::ETAG, format!("\"{}\"", asset.sha256))
        .body(Body::from_stream(ReaderStream::new(asset.file)))
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn require_owner(auth: &AuthUser) -> Result<(), AppError> {
    if auth.role == "owner" {
        Ok(())
    } else {
        Err(AppError::Forbidden("catalog.owner_required".into()))
    }
}

fn validate_upload_headers(headers: &HeaderMap) -> Result<(), AppError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .ok_or_else(|| AppError::BadRequest("catalog.invalid_content_type".into()))?;
    if !matches!(
        content_type,
        "application/vnd.dmxpack+zip" | "application/zip" | "application/octet-stream"
    ) {
        return Err(AppError::BadRequest("catalog.invalid_content_type".into()));
    }
    if let Some(length) = headers.get(header::CONTENT_LENGTH) {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| AppError::BadRequest("catalog.invalid_content_length".into()))?;
        if length == 0 || length > catalog::MAX_ARCHIVE_BYTES {
            return Err(AppError::BadRequest("catalog.archive_too_large".into()));
        }
    }
    Ok(())
}

fn archive_checksum(headers: &HeaderMap) -> Result<String, AppError> {
    let checksum = headers
        .get("x-dmx-package-sha256")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("catalog.archive_checksum_required".into()))?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest(
            "catalog.invalid_archive_checksum".into(),
        ));
    }
    Ok(checksum.to_ascii_lowercase())
}

fn parse_if_match(headers: &HeaderMap) -> Result<u32, AppError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().trim_start_matches("W/").trim_matches('"'))
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| AppError::PreconditionRequired("catalog.theme.if_match_required".into()))
}

fn version_headers(version: u32) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ETAG,
        format!("\"{version}\"")
            .parse()
            .map_err(|error| AppError::Internal(format!("invalid theme ETag: {error}")))?,
    );
    Ok(headers)
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

fn namespace_idempotency_key(actor_id: &str, key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"catalog-import\0");
    hash.update(actor_id.as_bytes());
    hash.update(b"\0");
    hash.update(key.as_bytes());
    format!("catalog:{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn upload_headers_require_a_supported_content_type_and_bounded_length() {
        let mut headers = HeaderMap::new();
        assert!(validate_upload_headers(&headers).is_err());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.dmxpack+zip"),
        );
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("16777216"));
        assert!(validate_upload_headers(&headers).is_ok());
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("16777217"));
        assert!(validate_upload_headers(&headers).is_err());
    }

    #[test]
    fn archive_checksum_is_normalized_but_must_be_sha256_hex() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-dmx-package-sha256",
            HeaderValue::from_str(&"A".repeat(64)).unwrap(),
        );
        assert_eq!(archive_checksum(&headers).unwrap(), "a".repeat(64));
        headers.insert(
            "x-dmx-package-sha256",
            HeaderValue::from_static("not-a-sha256"),
        );
        assert!(archive_checksum(&headers).is_err());
    }

    #[test]
    fn idempotency_namespace_is_actor_scoped_and_does_not_expose_the_key() {
        let first = namespace_idempotency_key("user-a", "private-value");
        let repeated = namespace_idempotency_key("user-a", "private-value");
        let other = namespace_idempotency_key("user-b", "private-value");
        assert_eq!(first, repeated);
        assert_ne!(first, other);
        assert!(!first.contains("private-value"));
    }

    #[test]
    fn theme_updates_require_a_positive_entity_tag() {
        let mut headers = HeaderMap::new();
        assert!(parse_if_match(&headers).is_err());
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"3\""));
        assert_eq!(parse_if_match(&headers).unwrap(), 3);
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"0\""));
        assert!(parse_if_match(&headers).is_err());
    }
}
