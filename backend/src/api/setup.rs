use axum::{
    routing::{get, post},
    extract::State,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use crate::error::AppError;
use crate::api::auth::AuthResponse;
use crate::db::get_or_create_jwt_secret;

#[derive(Serialize)]
struct SetupStatusResponse {
    is_setup: bool,
}

#[derive(Deserialize)]
struct SetupRequest {
    username: String,
    password: String,
    servers_dir: String,
    backups_dir: String,
    theme_color: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(get_setup_status))
        .route("/", post(perform_setup))
}

async fn get_setup_status(State(state): State<AppState>) -> Result<Json<SetupStatusResponse>, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(SetupStatusResponse {
        is_setup: count > 0,
    }))
}

async fn perform_setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    // 1. Check if setup is already done
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;

    if count > 0 {
        return Err(AppError::BadRequest("Setup already completed".into()));
    }

    // 2. Create Admin User
    // Note: We're reusing auth logic but we need to hash password manually if we don't use auth service helper
    // Let's use bcrypt directly as in auth.rs
    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let user_id = uuid::Uuid::new_v4().to_string();
    
    // Use role text instead of is_admin boolean
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, accent_color, created_at, updated_at) VALUES (?, ?, ?, ?, ?, datetime('now'), datetime('now'))"
    )
    .bind(&user_id)
    .bind(&body.username)
    .bind(&password_hash)
    .bind("admin") // Role
    .bind(&body.theme_color) // Sync accent_color with theme_color
    .execute(&state.pool)
    .await?;

    // 3. Update Settings
    async fn upsert_setting(pool: &crate::db::DbPool, key: &str, value: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now')) 
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
        Ok(())
    }

    upsert_setting(&state.pool, "servers_dir", &body.servers_dir).await?;
    upsert_setting(&state.pool, "backups_dir", &body.backups_dir).await?;
    upsert_setting(&state.pool, "login_default_color", &body.theme_color).await?;

    // 4. Return Login Token (Auto-login)
    // Generate JWT
    let secret = get_or_create_jwt_secret(&state.pool).await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .expect("valid timestamp")
        .timestamp();

    let claims = crate::api::auth::Claims {
        sub: user_id.clone(),
        username: body.username.clone(),
        role: "admin".to_string(),
        accent_color: Some(body.theme_color.clone()),
        exp: expiration,
    };

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(AuthResponse {
        token,
        user: crate::api::auth::UserInfo {
            id: user_id,
            username: body.username.clone(),
            role: "admin".to_string(),
            accent_color: Some(body.theme_color.clone()),
        },
    }))
}
