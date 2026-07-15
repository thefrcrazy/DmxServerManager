use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    api::{SuccessResponse, auth::AuthUser},
    core::{AppState, database, error::AppError},
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;
const MAX_MESSAGE_CHARS: usize = 4_000;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatQuery {
    before_id: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMessageRequest {
    body: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct ChatMessage {
    id: String,
    author_user_id: Option<String>,
    author_username: Option<String>,
    body: Option<String>,
    created_at: String,
    deleted_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatPage {
    items: Vec<ChatMessage>,
    next_before_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct MessageOwner {
    author_user_id: Option<String>,
    deleted_at: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/chat", get(list).post(create))
        .route("/chat/{id}", axum::routing::delete(remove))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ChatQuery>,
) -> Result<Json<ChatPage>, AppError> {
    auth.require("chat.read")?;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let rows: Vec<ChatMessage> = if let Some(before_id) = query.before_id.as_deref() {
        validate_id(before_id)?;
        let cursor: (String, String) =
            sqlx::query_as("SELECT created_at, id FROM chat_messages WHERE id = ?")
                .bind(before_id)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| AppError::BadRequest("chat.invalid_cursor".into()))?;
        sqlx::query_as(
            r#"
            SELECT m.id, m.author_user_id, u.username AS author_username, m.body,
                   m.created_at, m.deleted_at
            FROM chat_messages m
            LEFT JOIN users u ON u.id = m.author_user_id
            WHERE m.created_at < ? OR (m.created_at = ? AND m.id < ?)
            ORDER BY m.created_at DESC, m.id DESC
            LIMIT ?
            "#,
        )
        .bind(&cursor.0)
        .bind(&cursor.0)
        .bind(&cursor.1)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT m.id, m.author_user_id, u.username AS author_username, m.body,
                   m.created_at, m.deleted_at
            FROM chat_messages m
            LEFT JOIN users u ON u.id = m.author_user_id
            ORDER BY m.created_at DESC, m.id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };
    let next_before_id = (rows.len() == usize::from(limit))
        .then(|| rows.last().map(|message| message.id.clone()))
        .flatten();
    Ok(Json(ChatPage {
        items: rows,
        next_before_id,
    }))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<ChatMessage>), AppError> {
    auth.require("chat.write")?;
    let body = validate_body(&body.body)?;
    let message = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        author_user_id: Some(auth.id.clone()),
        author_username: Some(auth.username.clone()),
        body: Some(body),
        created_at: chrono::Utc::now().to_rfc3339(),
        deleted_at: None,
    };
    sqlx::query(
        "INSERT INTO chat_messages (id, author_user_id, body, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&message.id)
    .bind(&auth.id)
    .bind(message.body.as_deref())
    .bind(&message.created_at)
    .execute(&state.pool)
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "chat.message_created",
        "chat_message",
        Some(&message.id),
        "success",
        serde_json::json!({"contents_recorded": false}),
    )
    .await?;
    state.events.publish(
        "chat.message_created",
        None,
        serde_json::to_value(&message).map_err(|error| AppError::Internal(error.to_string()))?,
    );
    Ok((StatusCode::CREATED, Json(message)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    auth.require("chat.write")?;
    validate_id(&id)?;
    let owner: MessageOwner =
        sqlx::query_as("SELECT author_user_id, deleted_at FROM chat_messages WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("chat.not_found".into()))?;
    if owner.deleted_at.is_some() {
        return Ok(SuccessResponse::with_message("chat.deleted"));
    }
    if owner.author_user_id.as_deref() != Some(&auth.id)
        && !matches!(auth.role.as_str(), "owner" | "admin")
    {
        return Err(AppError::Forbidden("chat.delete_forbidden".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE chat_messages SET body = NULL, deleted_at = ?, deleted_by = ? \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&auth.id)
    .bind(&id)
    .execute(&state.pool)
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "chat.message_deleted",
        "chat_message",
        Some(&id),
        "success",
        serde_json::json!({"contents_recorded": false}),
    )
    .await?;
    state.events.publish(
        "chat.message_deleted",
        None,
        serde_json::json!({"id": id, "deleted_at": now}),
    );
    Ok(SuccessResponse::with_message("chat.deleted"))
}

fn validate_id(id: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest("chat.invalid_id".into()))
}

fn validate_body(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_MESSAGE_CHARS
        || value.len() > MAX_MESSAGE_BYTES
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(AppError::BadRequest("chat.invalid_message".into()));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_messages_are_bounded_plain_text() {
        assert_eq!(validate_body(" hello\nworld ").unwrap(), "hello\nworld");
        assert!(validate_body("\0secret").is_err());
        assert!(validate_body(&"x".repeat(MAX_MESSAGE_CHARS + 1)).is_err());
        assert!(validate_body("   ").is_err());
    }
}
