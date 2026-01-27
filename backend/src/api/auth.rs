use actix_web::{web, HttpResponse, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::db::DbPool;
use crate::error::AppError;

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
    pub exp: i64,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/status", web::get().to(check_setup_status))
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register))
            .route("/me", web::get().to(me))
            .route("/password", web::put().to(change_password)),
    );
}

/// Check if first-time setup is needed (no users exist)
async fn check_setup_status(pool: web::Data<DbPool>) -> Result<HttpResponse, AppError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json(SetupStatus {
        needs_setup: count.0 == 0,
    }))
}

async fn login(
    pool: web::Data<DbPool>,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    let user: UserRow = sqlx::query_as(
        "SELECT id, username, password_hash, role, accent_color FROM users WHERE username = ?",
    )
    .bind(&body.username)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::Unauthorized("auth.invalid_credentials".into()))?;

    if !bcrypt::verify(&body.password, &user.password_hash)
        .map_err(|_| AppError::Internal("Password verification failed".into()))?
    {
        return Err(AppError::Unauthorized("auth.invalid_credentials".into()));
    }

    let token = create_token(&user)?;

    Ok(HttpResponse::Ok().json(AuthResponse {
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
    pool: web::Data<DbPool>,
    body: web::Json<RegisterRequest>,
) -> Result<HttpResponse, AppError> {
    // Check if any users exist (first user becomes admin)
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool.get_ref())
        .await?;

    let role = if count.0 == 0 { "admin" } else { "user" };

    // Get default accent color from settings
    let default_color: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM settings WHERE key = 'login_default_color'"
    )
    .fetch_optional(pool.get_ref())
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
    .execute(pool.get_ref())
    .await?;

    let user = UserRow {
        id: id.clone(),
        username: body.username.clone(),
        password_hash,
        role: role.to_string(),
        accent_color: Some(accent_color.clone()),
    };

    let token = create_token(&user)?;

    Ok(HttpResponse::Created().json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
            accent_color: Some(accent_color),
        },
    }))
}

async fn me() -> Result<HttpResponse, AppError> {
    // TODO: Extract user from JWT middleware
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Implement JWT middleware"
    })))
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
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    body: web::Json<ChangePasswordRequest>,
) -> Result<HttpResponse, AppError> {
    // Extract user_id from Authorization header
    let auth_header = req
        .headers()
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
        .execute(pool.get_ref())
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("auth.user_not_found".into()));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "auth.password_updated"
    })))
}

