use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::auth::AuthUser,
    core::{AppState, database, error::AppError},
    services::webhooks,
};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct WebhookResponse {
    pub id: String,
    pub name: String,
    #[sqlx(json)]
    pub events: Vec<String>,
    pub enabled: bool,
    pub configured: bool,
    pub version: u32,
    pub last_delivery_at: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateWebhookRequest {
    name: String,
    url: String,
    events: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateWebhookRequest {
    name: String,
    #[serde(default)]
    url: Option<String>,
    events: Vec<String>,
    enabled: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks", get(list).post(create))
        .route("/webhooks/{id}", put(update).delete(remove))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<WebhookResponse>>, AppError> {
    require_owner(&auth)?;
    let webhooks = sqlx::query_as(
        "SELECT id, name, events, enabled, 1 AS configured, version, last_delivery_at, \
         last_error_code, created_at, updated_at FROM discord_webhooks ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(webhooks))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, HeaderMap, Json<WebhookResponse>), AppError> {
    require_owner(&auth)?;
    let id = Uuid::new_v4().to_string();
    let name = validate_name(&body.name)?;
    let events = webhooks::validate_event_set(body.events)?;
    let (nonce, ciphertext) = webhooks::encrypted_url(&state.secrets, &id, &body.url)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO discord_webhooks \
         (id, name, url_nonce, url_ciphertext, events, enabled, version, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(nonce)
    .bind(ciphertext)
    .bind(serde_json::to_string(&events).map_err(json_error)?)
    .bind(body.enabled)
    .bind(&auth.id)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(|error| {
        if error.to_string().contains("webhook limit") {
            AppError::Conflict("webhooks.limit_reached".into())
        } else {
            error.into()
        }
    })?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "webhook.create",
        "webhook",
        Some(&id),
        "success",
        serde_json::json!({"events": events, "enabled": body.enabled}),
    )
    .await?;
    state
        .events
        .publish("webhook.created", None, serde_json::json!({"id": id}));
    let webhook = get_by_id(&state, &id).await?;
    Ok((
        StatusCode::CREATED,
        etag_headers(webhook.version)?,
        Json(webhook),
    ))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateWebhookRequest>,
) -> Result<(HeaderMap, Json<WebhookResponse>), AppError> {
    require_owner(&auth)?;
    validate_id(&id)?;
    let expected = parse_if_match(&headers)?;
    let name = validate_name(&body.name)?;
    let events = webhooks::validate_event_set(body.events)?;
    let encrypted = body
        .url
        .as_deref()
        .map(|url| webhooks::encrypted_url(&state.secrets, &id, url))
        .transpose()?;
    let (nonce, ciphertext) = encrypted
        .map(|(nonce, ciphertext)| (Some(nonce), Some(ciphertext)))
        .unwrap_or((None, None));
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE discord_webhooks SET name = ?, url_nonce = COALESCE(?, url_nonce), \
         url_ciphertext = COALESCE(?, url_ciphertext), events = ?, enabled = ?, \
         version = version + 1, updated_at = ? WHERE id = ? AND version = ?",
    )
    .bind(&name)
    .bind(nonce)
    .bind(ciphertext)
    .bind(serde_json::to_string(&events).map_err(json_error)?)
    .bind(body.enabled)
    .bind(&now)
    .bind(&id)
    .bind(expected)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        if sqlx::query_scalar::<_, i64>("SELECT count(*) FROM discord_webhooks WHERE id = ?")
            .bind(&id)
            .fetch_one(&state.pool)
            .await?
            == 0
        {
            return Err(AppError::NotFound("webhooks.not_found".into()));
        }
        return Err(AppError::Conflict("webhooks.version_conflict".into()));
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "webhook.update",
        "webhook",
        Some(&id),
        "success",
        serde_json::json!({
            "events": events,
            "enabled": body.enabled,
            "url_changed": body.url.is_some(),
        }),
    )
    .await?;
    state
        .events
        .publish("webhook.updated", None, serde_json::json!({"id": id}));
    let webhook = get_by_id(&state, &id).await?;
    Ok((etag_headers(webhook.version)?, Json(webhook)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    require_owner(&auth)?;
    validate_id(&id)?;
    let result = sqlx::query("DELETE FROM discord_webhooks WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("webhooks.not_found".into()));
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "webhook.delete",
        "webhook",
        Some(&id),
        "success",
        serde_json::json!({}),
    )
    .await?;
    state
        .events
        .publish("webhook.deleted", None, serde_json::json!({"id": id}));
    Ok(StatusCode::NO_CONTENT)
}

async fn get_by_id(state: &AppState, id: &str) -> Result<WebhookResponse, AppError> {
    sqlx::query_as(
        "SELECT id, name, events, enabled, 1 AS configured, version, last_delivery_at, \
         last_error_code, created_at, updated_at FROM discord_webhooks WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("webhooks.not_found".into()))
}

fn validate_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
        return Err(AppError::BadRequest("webhooks.name_invalid".into()));
    }
    Ok(value.to_string())
}

fn validate_id(id: &str) -> Result<(), AppError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::NotFound("webhooks.not_found".into()))
}

fn require_owner(auth: &AuthUser) -> Result<(), AppError> {
    if auth.role == "owner" {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.owner_required".into()))
    }
}

fn parse_if_match(headers: &HeaderMap) -> Result<u32, AppError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| AppError::PreconditionRequired("webhooks.if_match_required".into()))
}

fn etag_headers(version: u32) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{version}\""))
            .map_err(|_| AppError::Internal("invalid webhook ETag".into()))?,
    );
    Ok(headers)
}

fn json_error(error: serde_json::Error) -> AppError {
    AppError::Internal(format!("JSON serialization failed: {error}"))
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_etags_are_strict() {
        let mut headers = HeaderMap::new();
        assert!(parse_if_match(&headers).is_err());
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"3\""));
        assert_eq!(parse_if_match(&headers).unwrap(), 3);
        headers.insert(header::IF_MATCH, HeaderValue::from_static("*"));
        assert!(parse_if_match(&headers).is_err());
    }
}
