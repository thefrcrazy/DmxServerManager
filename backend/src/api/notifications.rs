use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{SuccessResponse, auth::AuthUser},
    core::{AppState, error::AppError},
    services::notifications::Notification,
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationQuery {
    before_id: Option<String>,
    limit: Option<u16>,
    unread_only: Option<bool>,
}

#[derive(Debug, Serialize)]
struct NotificationPage {
    items: Vec<Notification>,
    next_before_id: Option<String>,
    unread_count: u64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(list))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/{id}/read", put(read_one))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<NotificationQuery>,
) -> Result<Json<NotificationPage>, AppError> {
    auth.require("notifications.read")?;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let unread_only = query.unread_only.unwrap_or(false);
    let rows: Vec<Notification> = if let Some(before_id) = query.before_id.as_deref() {
        validate_id(before_id)?;
        let cursor: (String, String) =
            sqlx::query_as("SELECT created_at, id FROM notifications WHERE id = ? AND user_id = ?")
                .bind(before_id)
                .bind(&auth.id)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| AppError::BadRequest("notifications.invalid_cursor".into()))?;
        sqlx::query_as(
            r#"
            SELECT id, kind, message_key, data, read_at, created_at
            FROM notifications
            WHERE user_id = ? AND (? = 0 OR read_at IS NULL)
              AND (created_at < ? OR (created_at = ? AND id < ?))
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(&auth.id)
        .bind(unread_only)
        .bind(&cursor.0)
        .bind(&cursor.0)
        .bind(&cursor.1)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT id, kind, message_key, data, read_at, created_at
            FROM notifications
            WHERE user_id = ? AND (? = 0 OR read_at IS NULL)
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(&auth.id)
        .bind(unread_only)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    let unread_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read_at IS NULL",
    )
    .bind(&auth.id)
    .fetch_one(&state.pool)
    .await?;
    let next_before_id = (rows.len() == usize::from(limit))
        .then(|| rows.last().map(|notification| notification.id.clone()))
        .flatten();
    Ok(Json(NotificationPage {
        items: rows,
        next_before_id,
        unread_count: u64::try_from(unread_count)
            .map_err(|_| AppError::Internal("invalid unread notification count".into()))?,
    }))
}

async fn read_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    auth.require("notifications.read")?;
    validate_id(&id)?;
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ? AND user_id = ?",
    )
    .bind(&now)
    .bind(&id)
    .bind(&auth.id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("notifications.not_found".into()));
    }
    state.events.publish_to_user(
        "notification.read",
        &auth.id,
        serde_json::json!({"id": id, "read_at": now}),
    );
    Ok(SuccessResponse::with_message("notifications.read"))
}

async fn read_all(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<SuccessResponse>, AppError> {
    auth.require("notifications.read")?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE notifications SET read_at = ? WHERE user_id = ? AND read_at IS NULL")
        .bind(&now)
        .bind(&auth.id)
        .execute(&state.pool)
        .await?;
    state.events.publish_to_user(
        "notification.read_all",
        &auth.id,
        serde_json::json!({"read_at": now}),
    );
    Ok(SuccessResponse::with_message("notifications.read_all"))
}

fn validate_id(id: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest("notifications.invalid_id".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_id_must_be_a_uuid() {
        assert!(validate_id(&uuid::Uuid::new_v4().to_string()).is_ok());
        assert!(validate_id("../other-user").is_err());
    }
}
