use axum::{
    routing::get,
    extract::State,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::Utc;

use crate::core::AppState;
use crate::api::auth::AuthUser;
use crate::core::error::AppError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/messages", get(list_messages).post(create_message))
}

#[derive(Debug, Serialize, FromRow)]
pub struct MessageRow {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    #[sqlx(rename = "type")]
    pub type_name: String,
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
        SELECT m.id, m.user_id, m.content, m.type, m.created_at, u.username, u.accent_color
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
        created_at: now,
        accent_color: auth.accent_color,
    }))
}
