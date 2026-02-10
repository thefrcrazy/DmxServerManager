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
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::warn;

use crate::{core::AppState, core::error::AppError, core::database::get_or_create_jwt_secret};
use crate::core::error::codes::ErrorCode;
use crate::api::SuccessResponse;

// ============= Rate Limiting =============

// Simple in-memory rate limiter - tracks login attempts per IP address
lazy_static::lazy_static! {
    static ref LOGIN_ATTEMPTS: Arc<RwLock<HashMap<String, Vec<Instant>>>> = 
        Arc::new(RwLock::new(HashMap::new()));
}

const MAX_LOGIN_ATTEMPTS: usize = 5;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(300); // 5 minutes

async fn check_rate_limit(ip: &str) -> Result<(), AppError> {
    let mut attempts = LOGIN_ATTEMPTS.write().await;
    let now = Instant::now();
    
    // Clean old attempts
    if let Some(ip_attempts) = attempts.get_mut(ip) {
        ip_attempts.retain(|t| now.duration_since(*t) < RATE_LIMIT_WINDOW);
        
        if ip_attempts.len() >= MAX_LOGIN_ATTEMPTS {
            warn!(ip = ip, attempts = ip_attempts.len(), "Rate limit exceeded for login");
            return Err(AppError::Unauthorized("auth.rate_limited".into())
                .with_code(ErrorCode::AuthRateLimited));
        }
    }
    
    Ok(())
}

async fn record_login_attempt(ip: &str) {
    let mut attempts = LOGIN_ATTEMPTS.write().await;
    attempts
        .entry(ip.to_string())
        .or_insert_with(Vec::new)
        .push(Instant::now());
}

async fn clear_login_attempts(ip: &str) {
    let mut attempts = LOGIN_ATTEMPTS.write().await;
    attempts.remove(ip);
}

// ============= Password Validation =============

fn validate_password_strength(password: &str) -> Result<(), AppError> {
    if password.len() < 12 {
        return Err(AppError::BadRequest("auth.password_too_short_12".into())
            .with_code(ErrorCode::AuthPasswordTooWeak));
    }
    
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_lowercase = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());
    
    if !has_uppercase || !has_lowercase || !has_digit || !has_special {
        return Err(AppError::BadRequest("auth.password_weak_complexity".into())
            .with_code(ErrorCode::AuthPasswordTooWeak));
    }
    
    Ok(())
}

// ============= JWT & Auth =============

/// Get JWT secret from database or generate if not exists
async fn get_jwt_secret(state: &AppState) -> Result<String, AppError> {
    get_or_create_jwt_secret(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))
}

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
    pub permissions: Vec<String>,
    pub accent_color: Option<String>,
    pub must_change_password: bool,
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
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    // Get client IP from headers (X-Forwarded-For or X-Real-IP)
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    
    // Check rate limit
    check_rate_limit(&ip).await?;
    
    // Record this attempt
    record_login_attempt(&ip).await;
    
    // Fetch user including must_change_password
    let user: UserRow = sqlx::query_as(
        "SELECT id, username, password_hash, role, accent_color, COALESCE(must_change_password, 0) as must_change_password FROM users WHERE username = ?",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("auth.invalid_credentials".into())
        .with_code(ErrorCode::AuthInvalidCredentials))?;

    if !bcrypt::verify(&body.password, &user.password_hash)
        .map_err(|_| AppError::Internal("Password verification failed".into()))?
    {
        return Err(AppError::Unauthorized("auth.invalid_credentials".into())
            .with_code(ErrorCode::AuthInvalidCredentials));
    }

    // Clear rate limit on successful login
    clear_login_attempts(&ip).await;

    // Update last login info in DB
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query("UPDATE users SET last_login = ?, last_ip = ? WHERE id = ?")
        .bind(&now)
        .bind(&ip)
        .bind(&user.id)
        .execute(&state.pool)
        .await;

    let token = create_token(&user, &state).await?;

    // Fetch permissions
    let role_perms: Option<(String,)> = sqlx::query_as("SELECT permissions FROM roles WHERE id = ?")
        .bind(&user.role)
        .fetch_optional(&state.pool)
        .await.unwrap_or(None);
    
    let permissions: Vec<String> = if let Some((p,)) = role_perms {
        serde_json::from_str(&p).unwrap_or_default()
    } else if user.role == "admin" {
        vec!["*".to_string()]
    } else {
        vec![]
    };

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
            permissions,
            accent_color: user.accent_color,
            must_change_password: user.must_change_password != 0,
        },
    }))
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    // Validate password strength
    validate_password_strength(&body.password)?;
    
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
        "INSERT INTO users (id, username, password_hash, role, accent_color, created_at, updated_at, must_change_password) VALUES (?, ?, ?, ?, ?, ?, ?, 0)",
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
        must_change_password: 0,
    };

    let token = create_token(&user, &state).await?;

    // Fetch permissions (for newly created user)
    // Note: If roles table empty, migration creates admin/user. If it failed, fallback.
    let role_perms: Option<(String,)> = sqlx::query_as("SELECT permissions FROM roles WHERE id = ?")
        .bind(&user.role)
        .fetch_optional(&state.pool)
        .await.unwrap_or(None);
    
    let permissions: Vec<String> = if let Some((p,)) = role_perms {
        serde_json::from_str(&p).unwrap_or_default()
    } else if user.role == "admin" {
        vec!["*".to_string()]
    } else {
        vec![]
    };

    Ok((StatusCode::CREATED, Json(AuthResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
            permissions,
            accent_color: Some(accent_color),
            must_change_password: false,
        },
    })))
}

async fn me(auth: AuthUser) -> Result<Json<UserInfo>, AppError> {
    // AuthUser already has permissions loaded
    Ok(Json(UserInfo {
        id: auth.id,
        username: auth.username,
        role: auth.role,
        permissions: auth.permissions,
        accent_color: auth.accent_color,
        must_change_password: false, 
    }))
}

pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub permissions: Vec<String>,
    pub accent_color: Option<String>,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let auth_header = parts.headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| {
                warn!("Auth failed: Missing Authorization header");
                AppError::Unauthorized("auth.missing_auth_header".into())
                    .with_code(ErrorCode::AuthMissingHeader)
            })?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                warn!("Auth failed: Invalid Authorization header format");
                AppError::Unauthorized("auth.invalid_auth_header".into())
                    .with_code(ErrorCode::AuthInvalidHeader)
            })?;

        // Use state directly passed by Axum
        let secret = get_jwt_secret(state).await?;
        
        let token_data = jsonwebtoken::decode::<Claims>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &jsonwebtoken::Validation::default(),
        )
        .map_err(|e| {
            warn!("Auth failed: Invalid token: {}", e);
            AppError::Unauthorized("auth.invalid_token".into())
                .with_code(ErrorCode::AuthInvalidToken)
        })?;

        // Fetch permissions for the role
        let role_perms: Option<(String,)> = sqlx::query_as(
            "SELECT permissions FROM roles WHERE id = ?"
        )
        .bind(&token_data.claims.role)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None); // Fail safe, empty permissions if role deleted

        let permissions: Vec<String> = if let Some((perms_json,)) = role_perms {
            serde_json::from_str(&perms_json).unwrap_or_default()
        } else if token_data.claims.role == "admin" {
            vec!["*".to_string()]
        } else {
            vec![]
        };

        Ok(AuthUser {
            id: token_data.claims.sub,
            username: token_data.claims.username,
            role: token_data.claims.role,
            permissions,
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
    #[sqlx(default)]
    must_change_password: i32,
}

async fn create_token(user: &UserRow, state: &AppState) -> Result<String, AppError> {
    let secret = get_jwt_secret(state).await?;

    let claims = Claims {
        sub: user.id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
        accent_color: user.accent_color.clone(),
        exp: (Utc::now() + chrono::Duration::hours(24)).timestamp(),
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
) -> Result<Json<SuccessResponse>, AppError> {
    // Extract user_id from Authorization header
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("auth.missing_auth_header".into()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("auth.invalid_auth_header".into()))?;

    let secret = get_jwt_secret(&state).await?;
    
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("auth.invalid_token".into()))?;

    let user_id = token_data.claims.sub;

    // Validate new password strength
    validate_password_strength(&body.new_password)?;

    // Hash new password
    let new_hash = bcrypt::hash(&body.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| AppError::Internal("Password hashing failed".into()))?;

    let now = Utc::now().to_rfc3339();

    // Update password AND reset must_change_password
    let result = sqlx::query("UPDATE users SET password_hash = ?, updated_at = ?, must_change_password = 0 WHERE id = ?")
        .bind(&new_hash)
        .bind(&now)
        .bind(&user_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("auth.user_not_found".into()));
    }

    Ok(SuccessResponse::with_message("auth.password_updated"))
}