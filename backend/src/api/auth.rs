use axum::{
    routing::{get, post, put},
    extract::{State, FromRequestParts},
    Json, Router,
    http::{StatusCode, HeaderMap, request::Parts},
};
use axum::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, error::AppError};

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
    pub accent_color: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub accent_color: Option<String>,
    pub exp: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(check_setup_status))
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/me", get(me))
        .route("/password", put(change_password))
}

/// Check if first-time setup is needed (no users exist)
async fn check_setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, AppError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(SetupStatus {
        needs_setup: count.0 == 0,
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user: UserRow = sqlx::query_as(
        "SELECT id, username, password_hash, role, accent_color FROM users WHERE username = ?",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("auth.invalid_credentials".into()))?;

    if !bcrypt::verify(&body.password, &user.password_hash)
        .map_err(|_| AppError::Internal("Password verification failed".into()))?
    {
        return Err(AppError::Unauthorized("auth.invalid_credentials".into()));
    }

    let token = create_token(&user)?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
            accent_color: user.accent_color,
        },
    }))
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    // Check if any users exist (first user becomes admin)
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;

    let role = if count.0 == 0 { "admin" } else { "user" };

    // Get default accent color from settings
    let default_color: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM settings WHERE key = 'login_default_color'"
    )
    .fetch_optional(&state.pool)
    .await?;
    let accent_color = default_color.map(|c| c.0).unwrap_or_else(|| "#3A82F6".to_string());

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::Internal("Password hashing failed".into()))?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, accent_color, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&body.username)
    .bind(&password_hash)
    .bind(role)
    .bind(&accent_color)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    let user = UserRow {
        id: id.clone(),
        username: body.username.clone(),
        password_hash,
        role: role.to_string(),
        accent_color: Some(accent_color.clone()),
    };

    let token = create_token(&user)?;

    Ok((StatusCode::CREATED, Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
            accent_color: Some(accent_color),
        },
    })))
}

async fn me(auth: AuthUser) -> Result<Json<UserInfo>, AppError> {
    Ok(Json(UserInfo {
        id: auth.id,
        username: auth.username,
        role: auth.role,
        accent_color: auth.accent_color,
    }))
}

pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub accent_color: Option<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("auth.missing_auth_header".into()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("auth.invalid_auth_header".into()))?;

        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "change-me-in-production".into());
        
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized("auth.invalid_token".into()))?;

        Ok(AuthUser {
            id: token_data.claims.sub,
            username: token_data.claims.username,
            role: token_data.claims.role,
            accent_color: token_data.claims.accent_color,
        })
    }
}


#[derive(Debug, FromRow)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role: String,
    accent_color: Option<String>,
}

fn create_token(user: &UserRow) -> Result<String, AppError> {
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "change-me-in-production".into());

    let claims = Claims {
        sub: user.id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
        accent_color: user.accent_color.clone(),
        exp: (Utc::now() + chrono::Duration::days(7)).timestamp(),
    };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    #[allow(dead_code)]
    pub current_password: Option<String>,
    pub new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Extract user_id from Authorization header
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("auth.missing_auth_header".into()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("auth.invalid_auth_header".into()))?;

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "change-me-in-production".into());
    
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("auth.invalid_token".into()))?;

    let user_id = token_data.claims.sub;

    // Validate new password length
    if body.new_password.len() < 8 {
        return Err(AppError::BadRequest("auth.password_length".into()));
    }

    // Hash new password
    let new_hash = bcrypt::hash(&body.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::Internal("Password hashing failed".into()))?;

    let now = Utc::now().to_rfc3339();

    // Update password
    let result = sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("auth.user_not_found".into()));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "auth.password_updated"
    })))
}
