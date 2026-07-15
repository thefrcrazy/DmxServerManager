use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::auth::{AuthUser, authorize_instance},
    core::{AppState, database, error::AppError},
    domain::v1::Job,
    services::{jobs, runtime::RuntimeAction},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/actions/install", post(install))
        .route("/servers/{id}/actions/start", post(start))
        .route("/servers/{id}/actions/stop", post(stop))
        .route("/servers/{id}/actions/restart", post(restart))
        .route("/servers/{id}/actions/kill", post(kill))
        .route("/servers/{id}/console", post(console))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsoleRequest {
    command: String,
}

#[derive(Debug, Serialize)]
struct ConsoleResponse {
    accepted: bool,
}

async fn install(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    submit(
        state,
        auth,
        id,
        headers,
        RuntimeAction::Install,
        &["server.update_game"],
    )
    .await
}

async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    submit(
        state,
        auth,
        id,
        headers,
        RuntimeAction::Start,
        &["server.start"],
    )
    .await
}

async fn stop(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    submit(
        state,
        auth,
        id,
        headers,
        RuntimeAction::Stop,
        &["server.stop"],
    )
    .await
}

async fn restart(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    submit(
        state,
        auth,
        id,
        headers,
        RuntimeAction::Restart,
        &["server.start", "server.stop"],
    )
    .await
}

async fn kill(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Job>), AppError> {
    submit(
        state,
        auth,
        id,
        headers,
        RuntimeAction::Kill,
        &["server.kill"],
    )
    .await
}

async fn submit(
    state: AppState,
    auth: AuthUser,
    id: String,
    headers: HeaderMap,
    action: RuntimeAction,
    permissions: &[&str],
) -> Result<(StatusCode, Json<Job>), AppError> {
    super::validate_instance_id(&id)?;
    for permission in permissions {
        authorize_instance(&state, &auth, &id, permission).await?;
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id = ?)")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(AppError::NotFound("servers.not_found".into()));
    }
    let idempotency_key = idempotency_key(&headers)?;
    let (job, created, claim) = jobs::create_claimed(
        &state.pool,
        &id,
        action.as_str(),
        &auth.id,
        idempotency_key.as_deref(),
    )
    .await?;
    if created {
        let claim = claim.expect("newly-created jobs always carry a claim");
        if let Err(error) = state
            .runtime
            .enqueue_claimed(job.clone(), action, claim)
            .await
        {
            let _ = jobs::fail(
                &state.pool,
                &job.id,
                "runtime_enqueue_failed",
                "servers.runtime_unavailable",
            )
            .await;
            return Err(error);
        }
    }
    state.events.publish(
        "job.queued",
        Some(id),
        serde_json::to_value(&job).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn console(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ConsoleRequest>,
) -> Result<(StatusCode, Json<ConsoleResponse>), AppError> {
    super::validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.console.write").await?;
    let profile_id: String = sqlx::query_scalar("SELECT profile_id FROM instances WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    let supports_console = state
        .profiles
        .get(&profile_id)
        .is_some_and(|profile| profile.capabilities.iter().any(|value| value == "console"));
    if !supports_console {
        return Err(AppError::BadRequest("servers.console_not_supported".into()));
    }
    state.runtime.send_console(&id, body.command).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.console_command",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({"contents_recorded": false}),
    )
    .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ConsoleResponse { accepted: true }),
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn idempotency_keys_are_bounded_visible_values() {
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", HeaderValue::from_static("install-42"));
        assert_eq!(
            idempotency_key(&headers).unwrap().as_deref(),
            Some("install-42")
        );
        headers.insert("idempotency-key", HeaderValue::from_static(""));
        assert!(idempotency_key(&headers).is_err());
    }
}
