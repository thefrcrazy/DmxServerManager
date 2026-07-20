use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, error::AppError},
    services::{config_files, instance_storage},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigPathQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueConfigRequest {
    content: String,
    expected_sha256: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/config-files", get(list))
        .route(
            "/servers/{id}/config-files/text",
            get(read).put(queue).delete(cancel),
        )
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(instance_id): Path<String>,
) -> Result<Json<config_files::ConfigFileList>, AppError> {
    super::servers::validate_instance_id(&instance_id)?;
    authorize_instance(&state, &auth, &instance_id, "server.files.read").await?;
    authorize_instance(&state, &auth, &instance_id, "server.config.raw.read").await?;
    let root = instance_storage::resolve(&state.pool, &state.settings, &instance_id)
        .await?
        .root;
    config_files::list(&state.pool, &root, &instance_id)
        .await
        .map(Json)
}

async fn read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(instance_id): Path<String>,
    Query(query): Query<ConfigPathQuery>,
) -> Result<Json<config_files::ConfigFileDocument>, AppError> {
    super::servers::validate_instance_id(&instance_id)?;
    authorize_instance(&state, &auth, &instance_id, "server.files.read").await?;
    authorize_instance(&state, &auth, &instance_id, "server.config.raw.read").await?;
    let root = instance_storage::resolve(&state.pool, &state.settings, &instance_id)
        .await?
        .root;
    config_files::read(
        &state.pool,
        &state.secrets,
        &root,
        &instance_id,
        &query.path,
    )
    .await
    .map(Json)
}

async fn queue(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(instance_id): Path<String>,
    Query(query): Query<ConfigPathQuery>,
    Json(body): Json<QueueConfigRequest>,
) -> Result<Json<config_files::ConfigFileDocument>, AppError> {
    super::servers::validate_instance_id(&instance_id)?;
    authorize_instance(&state, &auth, &instance_id, "server.files.write").await?;
    authorize_instance(&state, &auth, &instance_id, "server.config.raw.write").await?;
    let root = instance_storage::resolve(&state.pool, &state.settings, &instance_id)
        .await?
        .root;
    config_files::queue(
        &state.pool,
        &state.secrets,
        &state.events,
        &root,
        &instance_id,
        config_files::QueueConfigChange {
            path: &query.path,
            content: &body.content,
            expected_sha256: body.expected_sha256.as_deref(),
            queued_by: &auth.id,
        },
    )
    .await
    .map(Json)
}

async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(instance_id): Path<String>,
    Query(query): Query<ConfigPathQuery>,
) -> Result<Json<SuccessResponse>, AppError> {
    super::servers::validate_instance_id(&instance_id)?;
    authorize_instance(&state, &auth, &instance_id, "server.files.write").await?;
    authorize_instance(&state, &auth, &instance_id, "server.config.raw.write").await?;
    let root = instance_storage::resolve(&state.pool, &state.settings, &instance_id)
        .await?
        .root;
    // Resolve through the profile-owned allowlist before mutating queue state.
    config_files::read(
        &state.pool,
        &state.secrets,
        &root,
        &instance_id,
        &query.path,
    )
    .await?;
    config_files::cancel(
        &state.pool,
        &state.events,
        &instance_id,
        &query.path,
        &auth.id,
    )
    .await?;
    Ok(SuccessResponse::with_message("config_files.cancelled"))
}
