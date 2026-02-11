use axum::{
    routing::get,
    extract::{Path, State},
    Json, Router,
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::core::AppState;
use crate::api::SuccessResponse;
use crate::core::error::AppError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route("/:id", get(get_user).put(update_user).delete(delete_user))
}

#[derive(Debug, Serialize, FromRow)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub role: String,
    pub is_active: bool,
    pub language: String,
    pub accent_color: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_login: Option<String>,
    pub last_ip: Option<String>,
    pub allocated_servers: Option<String>,
    #[sqlx(default)]
    pub must_change_password: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Option<String>,
    pub is_active: Option<bool>,
    pub language: Option<String>,
    pub accent_color: Option<String>,
    pub allocated_servers: Option<Vec<String>>,
    pub must_change_password: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub password: Option<String>,
    pub role: Option<String>,
    pub is_active: Option<bool>,
    pub language: Option<String>,
    pub accent_color: Option<String>,
    pub allocated_servers: Option<Vec<String>>,
    pub must_change_password: Option<bool>,
}

async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let users: Vec<UserResponse> = sqlx::query_as(
        r#"SELECT id, username, role, 
           COALESCE(is_active, 1) as is_active,
           COALESCE(language, 'fr') as language,
           COALESCE(accent_color, '#3A82F6') as accent_color,
           created_at, updated_at, last_login, last_ip, allocated_servers,
           (COALESCE(must_change_password, 0) != 0) as must_change_password
           FROM users ORDER BY created_at DESC"#,
    )
    .fetch_all(&state.pool)
    .await?;

    // Parse allocated_servers JSON for each user
    let users_with_servers: Vec<serde_json::Value> = users
        .into_iter()
        .map(|user| {
            let servers: Vec<String> = user
                .allocated_servers
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            serde_json::json!({
                "id": user.id,
                "username": user.username,
                "role": user.role,
                "is_active": user.is_active,
                "language": user.language,
                "accent_color": user.accent_color,
                "created_at": user.created_at,
                "updated_at": user.updated_at,
                "last_login": user.last_login,
                "last_ip": user.last_ip,
                "allocated_servers": servers,
                "must_change_password": user.must_change_password
            })
        })
        .collect();

    Ok(Json(users_with_servers))
}

async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user: UserResponse = sqlx::query_as(
        r#"SELECT id, username, role,
           COALESCE(is_active, 1) as is_active,
           COALESCE(language, 'fr') as language,
           COALESCE(accent_color, '#3A82F6') as accent_color,
           created_at, updated_at, last_login, last_ip, allocated_servers,
           (COALESCE(must_change_password, 0) != 0) as must_change_password
           FROM users WHERE id = ?"#,
    )
    .bind(&user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("users.not_found".into()))?;

    let servers: Vec<String> = user
        .allocated_servers
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
        "is_active": user.is_active,
        "language": user.language,
        "accent_color": user.accent_color,
        "created_at": user.created_at,
        "updated_at": user.updated_at,
        "last_login": user.last_login,
        "last_ip": user.last_ip,
        "allocated_servers": servers,
        "must_change_password": user.must_change_password
    })))
}

use regex::Regex;

lazy_static::lazy_static! {
    static ref USERNAME_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9_-]{3,32}$").unwrap();
}

fn validate_username(username: &str) -> Result<(), AppError> {
    if !USERNAME_REGEX.is_match(username) {
        return Err(AppError::BadRequest("Invalid username format (3-32 chars, alphanumeric, _ -)".into()));
    }
    Ok(())
}

async fn create_user(
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    validate_username(&body.username)?;

    // Check if username already exists
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM users WHERE username = ?")
            .bind(&body.username)
            .fetch_optional(&state.pool)
            .await?;

    if exists.is_some() {
        return Err(AppError::BadRequest("users.exists".into()));
    }

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::Internal("Password hashing failed".into()))?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let role = body.role.clone().unwrap_or_else(|| "user".to_string());
    let is_active = body.is_active.unwrap_or(true);
    let language = body.language.clone().unwrap_or_else(|| "fr".to_string());
    let must_change_password = body.must_change_password.unwrap_or(false);
    
    // Get default color from settings if not provided
    let accent_color = if let Some(ref color) = body.accent_color {
        color.clone()
    } else {
        // Try to get from settings
        let default_color: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM settings WHERE key = 'login_default_color'"
        )
        .fetch_optional(&state.pool)
        .await?;
        default_color.map(|c| c.0).unwrap_or_else(|| "#3A82F6".to_string())
    };
    
    let allocated_servers = body
        .allocated_servers
        .as_ref()
        .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "[]".to_string()));

    sqlx::query(
        r#"INSERT INTO users (id, username, password_hash, role, is_active, language, accent_color, allocated_servers, created_at, updated_at, must_change_password)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&id)
    .bind(&body.username)
    .bind(&password_hash)
    .bind(&role)
    .bind(is_active)
    .bind(&language)
    .bind(&accent_color)
    .bind(&allocated_servers)
    .bind(&now)
    .bind(&now)
    .bind(must_change_password)
    .execute(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "id": id,
        "username": body.username,
        "role": role,
        "is_active": is_active,
        "message": "users.create_success"
    }))))
}

async fn update_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<SuccessResponse>, AppError> {
    let now = Utc::now().to_rfc3339();

    // Check if user exists
    let exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&state.pool)
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound("users.not_found".into()));
    }

    // Validate username if provided
    if let Some(ref username) = body.username {
        validate_username(username)?;
        
        // Check duplication (exclude self)
        let dup: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE username = ? AND id != ?")
            .bind(username)
            .bind(&user_id)
            .fetch_optional(&state.pool)
            .await?;
            
        if dup.is_some() {
            return Err(AppError::BadRequest("Username already taken".into()));
        }
    }

    // Build dynamic update query
    let mut updates = vec!["updated_at = ?"];
    let mut has_password = false;

    if body.username.is_some() {
        updates.push("username = ?");
    }
    if body.password.is_some() {
        updates.push("password_hash = ?");
        has_password = true;
    }
    if body.role.is_some() {
        updates.push("role = ?");
    }
    if body.is_active.is_some() {
        updates.push("is_active = ?");
    }
    if body.language.is_some() {
        updates.push("language = ?");
    }
    if body.accent_color.is_some() {
        updates.push("accent_color = ?");
    }
    if body.allocated_servers.is_some() {
        updates.push("allocated_servers = ?");
    }
    if body.must_change_password.is_some() {
        updates.push("must_change_password = ?");
    }

    let query = format!("UPDATE users SET {} WHERE id = ?", updates.join(", "));
    let mut sql_query = sqlx::query(&query);

    // Bind updated_at first
    sql_query = sql_query.bind(&now);

    // Bind optional fields in order
    if let Some(ref username) = body.username {
        sql_query = sql_query.bind(username);
    }
    if has_password {
        let password_hash = bcrypt::hash(body.password.as_ref().unwrap(), bcrypt::DEFAULT_COST)
            .map_err(|_| AppError::Internal("Password hashing failed".into()))?;
        sql_query = sql_query.bind(password_hash);
    }
    if let Some(ref role) = body.role {
        sql_query = sql_query.bind(role);
    }
    if let Some(is_active) = body.is_active {
        sql_query = sql_query.bind(is_active);
    }
    if let Some(ref language) = body.language {
        sql_query = sql_query.bind(language);
    }
    if let Some(ref accent_color) = body.accent_color {
        sql_query = sql_query.bind(accent_color);
    }
    if let Some(ref servers) = body.allocated_servers {
        let servers_json = serde_json::to_string(servers).unwrap_or_else(|_| "[]".to_string());
        sql_query = sql_query.bind(servers_json);
    }
    if let Some(must_change) = body.must_change_password {
        sql_query = sql_query.bind(must_change);
    }

    // Bind user_id last
    sql_query = sql_query.bind(&user_id);

    sql_query.execute(&state.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to update user: {e}")))?;

    Ok(SuccessResponse::with_message("users.update_success"))
}

async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    let result = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("users.not_found".into()));
    }

    Ok(SuccessResponse::with_message("users.delete_success"))
}