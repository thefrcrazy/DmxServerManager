use axum::{
    routing::get,
    extract::{State, Path},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::Utc;

use crate::core::AppState;
use crate::api::auth::AuthUser;
use crate::api::SuccessResponse;
use crate::core::error::AppError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/messages", get(list_messages).post(create_message))
        .route("/messages/:id", axum::routing::delete(delete_message))
}

#[derive(Debug, Serialize, FromRow)]
pub struct MessageRow {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    #[sqlx(rename = "type")]
    pub type_name: String,
    pub is_deleted: i32,
    pub created_at: String,
    pub accent_color: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub content: String,
    pub msg_type: String, // 'chat' or 'note'
}

async fn list_messages(
    State(state): State<AppState>,
) -> Result<Json<Vec<MessageRow>>, AppError> {
    let messages: Vec<MessageRow> = sqlx::query_as(
        r#"
        SELECT m.id, m.user_id, m.content, m.type, m.is_deleted, m.created_at, u.username, u.accent_color
        FROM messages m
        JOIN users u ON m.user_id = u.id
        ORDER BY m.created_at ASC
        LIMIT 100
        "#
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(messages))
}

async fn create_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateMessageRequest>,
) -> Result<Json<MessageRow>, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO messages (id, user_id, content, type, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&auth.id)
    .bind(&body.content)
    .bind(&body.msg_type)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    Ok(Json(MessageRow {
        id,
        user_id: auth.id,
        username: auth.username,
        content: body.content,
        type_name: body.msg_type,
        is_deleted: 0,
        created_at: now,
        accent_color: auth.accent_color,
    }))
}

async fn delete_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    // Check if message exists and user is owner or admin
    let message: (String, String) = sqlx::query_as("SELECT id, user_id FROM messages WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound("collaboration.message_not_found".into()))?;

    if message.1 != auth.id && auth.role != "admin" {
        return Err(AppError::Forbidden("collaboration.delete_forbidden".into()));
    }

    // Soft delete: keep row but mark as deleted
    sqlx::query("UPDATE messages SET is_deleted = 1, content = 'auth.message_deleted' WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    Ok(SuccessResponse::ok())
}
