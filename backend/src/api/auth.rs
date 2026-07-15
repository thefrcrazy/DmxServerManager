use std::{collections::HashMap, net::SocketAddr};

use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
        rand_core::OsRng as PasswordOsRng,
    },
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration as ChronoDuration, Utc};
use rand::{RngCore, rngs::OsRng};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use tracing::warn;
use uuid::Uuid;

use crate::{
    api::SuccessResponse,
    core::{
        AppState, database,
        error::{AppError, codes::ErrorCode},
    },
};

const SESSION_COOKIE: &str = "dmx_session";
const CSRF_HEADER: &str = "x-csrf-token";
const MAX_LOGIN_ATTEMPTS: i64 = 5;
const RATE_LIMIT_WINDOW_SECONDS: i64 = 300;

lazy_static::lazy_static! {
    static ref USERNAME_PATTERN: Regex = Regex::new(r"^[A-Za-z0-9_-]{3,32}$").expect("valid username regex");
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
    pub permissions: Vec<String>,
    pub accent_color: String,
    pub must_change_password: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: UserInfo,
    pub csrf_token: String,
}

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub permissions: Vec<String>,
    pub accent_color: Option<String>,
    pub must_change_password: bool,
    pub session_id: String,
    csrf_hash: String,
}

#[derive(Debug, Clone)]
pub(crate) struct InstanceGrantScope {
    unrestricted: bool,
    grants: HashMap<String, Vec<String>>,
}

impl InstanceGrantScope {
    pub(crate) fn allows(&self, auth: &AuthUser, instance_id: &str, permission: &str) -> bool {
        if !auth.has_permission(permission) {
            return false;
        }
        if self.unrestricted {
            return true;
        }
        self.grants.get(instance_id).is_some_and(|permissions| {
            permissions.is_empty()
                || permissions
                    .iter()
                    .any(|value| value == "*" || value == permission)
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        unrestricted: bool,
        grants: impl IntoIterator<Item = (String, Vec<String>)>,
    ) -> Self {
        Self {
            unrestricted,
            grants: grants.into_iter().collect(),
        }
    }
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role_id: String,
    permissions: String,
    accent_color: String,
    must_change_password: bool,
}

#[derive(Debug, FromRow)]
struct SessionRow {
    session_id: String,
    csrf_hash: String,
    user_id: String,
    username: String,
    role_id: String,
    permissions: String,
    accent_color: String,
    must_change_password: bool,
}

pub fn routes(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/me", get(me))
        .route("/logout", post(logout))
        .route("/password", put(change_password))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            csrf_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    Router::new()
        .route("/status", get(check_setup_status))
        .route("/setup", post(setup))
        .route("/login", post(login))
        .merge(protected)
}

pub async fn require_session(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = cookie_value(request.headers(), SESSION_COOKIE)
        .ok_or_else(|| AppError::Unauthorized("auth.session_required".into()))?;
    let token_hash = hash_token(&token);
    let now = Utc::now().to_rfc3339();
    let row: SessionRow = sqlx::query_as(
        r#"
        SELECT s.id AS session_id, s.csrf_hash, u.id AS user_id, u.username,
               u.role_id, r.permissions, u.accent_color, u.must_change_password
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        JOIN roles r ON r.id = u.role_id
        WHERE s.token_hash = ? AND s.expires_at > ? AND u.is_active = 1
        "#,
    )
    .bind(token_hash)
    .bind(&now)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("auth.invalid_session".into()))?;

    let auth = auth_from_session_row(row);
    request.extensions_mut().insert(auth);
    Ok(next.run(request).await)
}

pub async fn csrf_middleware(
    State(_state): State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return Ok(next.run(request).await);
    }

    let auth = request
        .extensions()
        .get::<AuthUser>()
        .ok_or_else(|| AppError::Unauthorized("auth.session_required".into()))?;
    let supplied = request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::Forbidden("auth.csrf_required".into()))?;
    let supplied_hash = hash_token(supplied);
    if supplied_hash
        .as_bytes()
        .ct_eq(auth.csrf_hash.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(AppError::Forbidden("auth.csrf_invalid".into()));
    }
    Ok(next.run(request).await)
}

pub async fn require_password_change_completed(
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let auth = request
        .extensions()
        .get::<AuthUser>()
        .ok_or_else(|| AppError::Unauthorized("auth.session_required".into()))?;
    if auth.must_change_password {
        return Err(AppError::Forbidden("auth.password_change_required".into())
            .with_code(ErrorCode::AuthPasswordChangeRequired));
    }
    Ok(next.run(request).await)
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| AppError::Unauthorized("auth.session_required".into()))
    }
}

impl AuthUser {
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions
            .iter()
            .any(|value| value == "*" || value == permission)
    }

    pub fn require(&self, permission: &str) -> Result<(), AppError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden("auth.permission_denied".into()))
        }
    }

    fn as_user_info(&self) -> UserInfo {
        UserInfo {
            id: self.id.clone(),
            username: self.username.clone(),
            role: self.role.clone(),
            permissions: self.permissions.clone(),
            accent_color: self
                .accent_color
                .clone()
                .unwrap_or_else(|| "#3A82F6".into()),
            must_change_password: self.must_change_password,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        id: &str,
        role: &str,
        permissions: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id: id.into(),
            username: id.into(),
            role: role.into(),
            permissions: permissions.into_iter().map(str::to_string).collect(),
            accent_color: None,
            must_change_password: false,
            session_id: "test-session".into(),
            csrf_hash: "test-csrf".into(),
        }
    }
}

pub async fn authorize_instance(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
    permission: &str,
) -> Result<(), AppError> {
    auth.require(permission)?;
    if matches!(auth.role.as_str(), "owner" | "admin") {
        return Ok(());
    }

    let grant: Option<String> = sqlx::query_scalar(
        "SELECT permissions FROM user_instance_grants WHERE user_id = ? AND instance_id = ?",
    )
    .bind(&auth.id)
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(grant) = grant else {
        return Err(AppError::Forbidden("auth.instance_not_assigned".into()));
    };
    let grant_permissions = parse_permissions(&grant);
    if grant_permissions.is_empty()
        || grant_permissions
            .iter()
            .any(|value| value == "*" || value == permission)
    {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.permission_denied".into()))
    }
}

pub(crate) async fn instance_grant_scope(
    state: &AppState,
    auth: &AuthUser,
) -> Result<InstanceGrantScope, AppError> {
    if matches!(auth.role.as_str(), "owner" | "admin") {
        return Ok(InstanceGrantScope {
            unrestricted: true,
            grants: HashMap::new(),
        });
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT instance_id, permissions FROM user_instance_grants WHERE user_id = ?",
    )
    .bind(&auth.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(InstanceGrantScope {
        unrestricted: false,
        grants: rows
            .into_iter()
            .map(|(instance_id, permissions)| (instance_id, parse_permissions(&permissions)))
            .collect(),
    })
}

pub(crate) async fn refresh_session_auth(
    pool: &crate::core::DbPool,
    session_id: &str,
) -> Result<Option<AuthUser>, AppError> {
    let row: Option<SessionRow> = sqlx::query_as(
        r#"
        SELECT s.id AS session_id, s.csrf_hash, u.id AS user_id, u.username,
               u.role_id, r.permissions, u.accent_color, u.must_change_password
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        JOIN roles r ON r.id = u.role_id
        WHERE s.id = ? AND s.expires_at > ? AND u.is_active = 1
        "#,
    )
    .bind(session_id)
    .bind(Utc::now().to_rfc3339())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(auth_from_session_row))
}

fn auth_from_session_row(row: SessionRow) -> AuthUser {
    AuthUser {
        id: row.user_id,
        username: row.username,
        role: row.role_id,
        permissions: parse_permissions(&row.permissions),
        accent_color: Some(row.accent_color),
        must_change_password: row.must_change_password,
        session_id: row.session_id,
        csrf_hash: row.csrf_hash,
    }
}

async fn check_setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, AppError> {
    let completed: bool =
        sqlx::query_scalar("SELECT completed FROM setup_state WHERE singleton = 1")
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(SetupStatus {
        needs_setup: !completed,
    }))
}

async fn setup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Result<Response, AppError> {
    let client = client_identity(&state, Some(peer), &headers);
    let is_loopback = client
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback());
    let supplied_setup_token = headers
        .get("x-setup-token")
        .and_then(|value| value.to_str().ok());
    let setup_token_valid = state
        .settings
        .setup_token
        .as_deref()
        .zip(supplied_setup_token)
        .is_some_and(|(expected, supplied)| {
            expected.as_bytes().ct_eq(supplied.as_bytes()).unwrap_u8() == 1
        });
    if !is_loopback && !setup_token_valid {
        return Err(AppError::Forbidden("auth.setup_local_or_token_only".into()));
    }
    validate_username(&body.username)?;
    validate_password_strength(&body.password)?;

    let mut transaction = state.pool.begin().await?;
    let setup_claim =
        sqlx::query("UPDATE setup_state SET completed = 1 WHERE singleton = 1 AND completed = 0")
            .execute(&mut *transaction)
            .await?;
    if setup_claim.rows_affected() != 1 {
        return Err(AppError::BadRequest("auth.setup_already_completed".into()));
    }

    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let password_hash = hash_password(&body.password)?;
    sqlx::query(
        r#"
        INSERT INTO users
            (id, username, password_hash, role_id, created_at, updated_at)
        VALUES (?, ?, ?, 'owner', ?, ?)
        "#,
    )
    .bind(&user_id)
    .bind(&body.username)
    .bind(password_hash)
    .bind(&now)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    database::audit(
        &state.pool,
        Some(&user_id),
        "auth.setup",
        "user",
        Some(&user_id),
        "success",
        serde_json::json!({}),
    )
    .await?;

    issue_session(
        &state,
        UserRow {
            id: user_id,
            username: body.username,
            password_hash: String::new(),
            role_id: "owner".into(),
            permissions: "[\"*\"]".into(),
            accent_color: "#3A82F6".into(),
            must_change_password: false,
        },
        StatusCode::CREATED,
    )
    .await
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let client = client_identity(&state, Some(peer), &headers);
    let rate_limit_keys = rate_limit_keys(&client, &body.username);
    check_rate_limit(&state.pool, &rate_limit_keys).await?;

    let user: Option<UserRow> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.password_hash, u.role_id, r.permissions,
               u.accent_color, u.must_change_password
        FROM users u
        JOIN roles r ON r.id = u.role_id
        WHERE u.username = ? COLLATE NOCASE AND u.is_active = 1
        "#,
    )
    .bind(body.username.trim())
    .fetch_optional(&state.pool)
    .await?;
    let Some(user) = user else {
        record_login_failure(&state.pool, &rate_limit_keys).await?;
        audit_failed_login(&state, &rate_limit_keys[0]).await;
        return Err(AppError::Unauthorized("auth.invalid_credentials".into()));
    };
    if !verify_password(&body.password, &user.password_hash) {
        record_login_failure(&state.pool, &rate_limit_keys).await?;
        audit_failed_login(&state, &rate_limit_keys[0]).await;
        return Err(AppError::Unauthorized("auth.invalid_credentials".into()));
    }

    clear_login_failures(&state.pool, &rate_limit_keys).await?;
    sqlx::query("UPDATE users SET last_login_at = ?, updated_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(&user.id)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&user.id),
        "auth.login",
        "session",
        None,
        "success",
        serde_json::json!({"client": client}),
    )
    .await?;
    issue_session(&state, user, StatusCode::OK).await
}

async fn me(State(state): State<AppState>, auth: AuthUser) -> Result<Json<AuthResponse>, AppError> {
    let csrf_token = random_token();
    sqlx::query("UPDATE sessions SET csrf_hash = ?, last_seen_at = ? WHERE id = ?")
        .bind(hash_token(&csrf_token))
        .bind(Utc::now().to_rfc3339())
        .bind(&auth.session_id)
        .execute(&state.pool)
        .await?;
    Ok(Json(AuthResponse {
        user: auth.as_user_info(),
        csrf_token,
    }))
}

async fn logout(State(state): State<AppState>, auth: AuthUser) -> Result<Response, AppError> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(&auth.session_id)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "auth.logout",
        "session",
        Some(&auth.session_id),
        "success",
        serde_json::json!({}),
    )
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, expired_cookie(&state)?);
    Ok((headers, SuccessResponse::ok()).into_response())
}

async fn change_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Response, AppError> {
    validate_password_strength(&body.new_password)?;
    let current_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(&auth.id)
        .fetch_one(&state.pool)
        .await?;
    if !verify_password(&body.current_password, &current_hash) {
        return Err(AppError::Unauthorized(
            "auth.invalid_current_password".into(),
        ));
    }
    if body.new_password == body.current_password {
        return Err(AppError::BadRequest("auth.password_must_differ".into()));
    }

    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "UPDATE users SET password_hash = ?, must_change_password = 0, updated_at = ? WHERE id = ?",
    )
    .bind(hash_password(&body.new_password)?)
    .bind(Utc::now().to_rfc3339())
    .bind(&auth.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&auth.id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "auth.password_changed",
        "user",
        Some(&auth.id),
        "success",
        serde_json::json!({}),
    )
    .await?;

    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, expired_cookie(&state)?);
    Ok((
        headers,
        SuccessResponse::with_message("auth.password_updated"),
    )
        .into_response())
}

async fn issue_session(
    state: &AppState,
    user: UserRow,
    status: StatusCode,
) -> Result<Response, AppError> {
    let session_id = Uuid::new_v4().to_string();
    let session_token = random_token();
    let csrf_token = random_token();
    let now = Utc::now();
    let expires = now + ChronoDuration::hours(state.settings.session_ttl_hours);
    sqlx::query(
        r#"
        INSERT INTO sessions
            (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(session_id)
    .bind(&user.id)
    .bind(hash_token(&session_token))
    .bind(hash_token(&csrf_token))
    .bind(expires.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(&state.pool)
    .await?;

    let permissions = parse_permissions(&user.permissions);
    let response = AuthResponse {
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role_id,
            permissions,
            accent_color: user.accent_color,
            must_change_password: user.must_change_password,
        },
        csrf_token,
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, session_cookie(state, &session_token)?);
    Ok((status, headers, Json(response)).into_response())
}

fn session_cookie(state: &AppState, token: &str) -> Result<HeaderValue, AppError> {
    let secure = if state.settings.secure_cookies() {
        "; Secure"
    } else {
        ""
    };
    let value = format!(
        "{SESSION_COOKIE}={token}; Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age={}{}",
        state.settings.session_ttl_hours * 3600,
        secure
    );
    HeaderValue::from_str(&value).map_err(|error| AppError::Internal(error.to_string()))
}

fn expired_cookie(state: &AppState) -> Result<HeaderValue, AppError> {
    let secure = if state.settings.secure_cookies() {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age=0{secure}"
    ))
    .map_err(|error| AppError::Internal(error.to_string()))
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_string()))
}

pub(crate) fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut PasswordOsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::Internal("password hashing failed".into()))
}

fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

pub(crate) fn validate_password_strength(password: &str) -> Result<(), AppError> {
    if password.len() < 12
        || password.len() > 256
        || !password.chars().any(char::is_uppercase)
        || !password.chars().any(char::is_lowercase)
        || !password.chars().any(|value| value.is_ascii_digit())
        || !password.chars().any(|value| !value.is_alphanumeric())
    {
        return Err(AppError::BadRequest("auth.password_too_weak".into()));
    }
    Ok(())
}

pub(crate) fn validate_username(username: &str) -> Result<(), AppError> {
    if USERNAME_PATTERN.is_match(username) {
        Ok(())
    } else {
        Err(AppError::BadRequest("auth.invalid_username".into()))
    }
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

fn parse_permissions(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn client_identity(state: &AppState, peer: Option<SocketAddr>, headers: &HeaderMap) -> String {
    let peer = peer.map(|address| address.ip());
    if state.settings.reverse_proxy
        && peer.is_some_and(|ip| state.settings.trusted_proxies.contains(&ip))
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|value| value.trim().parse::<std::net::IpAddr>().ok())
    {
        return forwarded.to_string();
    }
    peer.map_or_else(|| "unknown".into(), |ip| ip.to_string())
}

fn rate_limit_keys(client: &str, username: &str) -> [String; 2] {
    let client = hash_token(&format!("login-client:{client}"));
    let pair = hash_token(&format!(
        "login-pair:{client}:{}",
        username.trim().to_ascii_lowercase()
    ));
    [client, pair]
}

async fn check_rate_limit(pool: &crate::core::DbPool, keys: &[String; 2]) -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let locked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM login_rate_limits \
         WHERE key_hash IN (?, ?) AND locked_until > ?)",
    )
    .bind(&keys[0])
    .bind(&keys[1])
    .bind(now)
    .fetch_one(pool)
    .await?;
    if locked {
        warn!(client_hash = %keys[0], "login rate limit exceeded");
        Err(AppError::TooManyRequests("auth.rate_limited".into()))
    } else {
        Ok(())
    }
}

async fn record_login_failure(
    pool: &crate::core::DbPool,
    keys: &[String; 2],
) -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let cutoff = now - RATE_LIMIT_WINDOW_SECONDS;
    for key in keys {
        sqlx::query(
            "INSERT INTO login_rate_limits \
             (key_hash, failure_count, window_started, locked_until, updated_at) \
             VALUES (?, 1, ?, 0, ?) \
             ON CONFLICT(key_hash) DO UPDATE SET \
               failure_count = CASE WHEN window_started < ? THEN 1 ELSE failure_count + 1 END, \
               window_started = CASE WHEN window_started < ? THEN ? ELSE window_started END, \
               locked_until = CASE \
                 WHEN (CASE WHEN window_started < ? THEN 1 ELSE failure_count + 1 END) >= ? \
                 THEN ? ELSE locked_until END, \
               updated_at = ?",
        )
        .bind(key)
        .bind(now)
        .bind(now)
        .bind(cutoff)
        .bind(cutoff)
        .bind(now)
        .bind(cutoff)
        .bind(MAX_LOGIN_ATTEMPTS)
        .bind(now + RATE_LIMIT_WINDOW_SECONDS)
        .bind(now)
        .execute(pool)
        .await?;
    }
    // Keep the table bounded even when attacked with many synthetic usernames/IPs.
    sqlx::query("DELETE FROM login_rate_limits WHERE updated_at < ?")
        .bind(now - 7 * 24 * 60 * 60)
        .execute(pool)
        .await?;
    Ok(())
}

async fn clear_login_failures(
    pool: &crate::core::DbPool,
    keys: &[String; 2],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM login_rate_limits WHERE key_hash IN (?, ?)")
        .bind(&keys[0])
        .bind(&keys[1])
        .execute(pool)
        .await?;
    Ok(())
}

async fn audit_failed_login(state: &AppState, client_hash: &str) {
    let _ = database::audit(
        &state.pool,
        None,
        "auth.login",
        "session",
        None,
        "denied",
        serde_json::json!({"client_hash": client_hash}),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_argon2id_and_verify() {
        let encoded = hash_password("A-secure-password-42!").unwrap();
        assert!(encoded.starts_with("$argon2"));
        assert!(verify_password("A-secure-password-42!", &encoded));
        assert!(!verify_password("wrong", &encoded));
    }

    #[test]
    fn cookie_parser_matches_exact_cookie_name() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; dmx_session=abc; dmx_session_extra=x"),
        );
        assert_eq!(
            cookie_value(&headers, SESSION_COOKIE).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn permission_checks_are_closed_by_default() {
        let auth = AuthUser {
            id: "id".into(),
            username: "user".into(),
            role: "viewer".into(),
            permissions: vec!["server.read".into()],
            accent_color: None,
            must_change_password: false,
            session_id: "session".into(),
            csrf_hash: hash_token("csrf"),
        };
        assert!(auth.require("server.read").is_ok());
        assert!(auth.require("server.delete").is_err());
    }

    #[tokio::test]
    async fn login_rate_limit_is_persistent_and_clearable() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let keys = rate_limit_keys("192.0.2.1", "TestUser");

        for _ in 0..MAX_LOGIN_ATTEMPTS {
            record_login_failure(&pool, &keys).await.unwrap();
        }

        assert!(matches!(
            check_rate_limit(&pool, &keys).await,
            Err(AppError::TooManyRequests(_))
        ));
        clear_login_failures(&pool, &keys).await.unwrap();
        check_rate_limit(&pool, &keys).await.unwrap();

        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_rate_limits")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, 0);
    }

    #[tokio::test]
    async fn live_session_revalidation_observes_revocation_and_account_state() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES ('viewer-id', 'viewer-test', 'unused', 'viewer', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) \
             VALUES ('session-id', 'viewer-id', 'token-hash', 'csrf-hash', ?, ?, ?)",
        )
        .bind((Utc::now() + ChronoDuration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let refreshed = refresh_session_auth(&pool, "session-id")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(refreshed.id, "viewer-id");
        assert!(refreshed.has_permission("server.read"));

        sqlx::query("UPDATE users SET is_active = 0 WHERE id = 'viewer-id'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            refresh_session_auth(&pool, "session-id")
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("UPDATE users SET is_active = 1 WHERE id = 'viewer-id'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM sessions WHERE id = 'session-id'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            refresh_session_auth(&pool, "session-id")
                .await
                .unwrap()
                .is_none()
        );
    }
}
