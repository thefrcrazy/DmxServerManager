use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, put},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{
        SuccessResponse,
        auth::{
            AuthUser, InstanceGrantScope, hash_password, instance_grant_scope,
            validate_password_strength, validate_username,
        },
    },
    core::{AppState, database, error::AppError},
};

const PERMISSIONS: &[&str] = &[
    "audit.read",
    "chat.read",
    "chat.write",
    "job.read",
    "mods.manage",
    "notifications.read",
    "profile.manage",
    "profile.read",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.console.read",
    "server.console.write",
    "server.create",
    "server.delete",
    "server.files.read",
    "server.files.write",
    "server.kill",
    "server.read",
    "server.start",
    "server.stop",
    "server.update",
    "server.update_game",
    "user.create",
    "user.read",
    "user.update",
];

const INSTANCE_PERMISSIONS: &[&str] = &[
    "job.read",
    "mods.manage",
    "schedule.manage",
    "server.backup",
    "server.backup.read",
    "server.console.read",
    "server.console.write",
    "server.files.read",
    "server.files.write",
    "server.kill",
    "server.read",
    "server.start",
    "server.stop",
    "server.update",
    "server.update_game",
];

const HIGH_RISK_PERMISSIONS: &[&str] = &[
    "profile.manage",
    "server.console.write",
    "server.files.write",
];

#[derive(Debug, Serialize)]
struct PermissionDescription {
    id: &'static str,
    high_risk: bool,
    instance_scoped: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct RoleResponse {
    id: String,
    name: String,
    #[sqlx(json)]
    permissions: Vec<String>,
    is_system: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, FromRow)]
struct UserResponse {
    id: String,
    username: String,
    role_id: String,
    role_name: String,
    is_active: bool,
    language: String,
    accent_color: String,
    must_change_password: bool,
    last_login_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, FromRow)]
struct GrantResponse {
    instance_id: String,
    instance_name: String,
    #[sqlx(json)]
    permissions: Vec<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRoleRequest {
    name: String,
    permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRoleRequest {
    name: Option<String>,
    permissions: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUserRequest {
    username: String,
    password: String,
    role_id: String,
    #[serde(default = "default_language")]
    language: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateUserRequest {
    role_id: Option<String>,
    is_active: Option<bool>,
    language: Option<String>,
    accent_color: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetGrantRequest {
    #[serde(default)]
    permissions: Vec<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/permissions", get(list_permissions))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", patch(update_role).delete(delete_role))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", patch(update_user))
        .route("/users/{id}/instances", get(list_grants))
        .route(
            "/users/{user_id}/instances/{instance_id}",
            put(set_grant).delete(delete_grant),
        )
}

async fn list_permissions(auth: AuthUser) -> Result<Json<Vec<PermissionDescription>>, AppError> {
    require_owner(&auth)?;
    Ok(Json(
        PERMISSIONS
            .iter()
            .map(|id| PermissionDescription {
                id,
                high_risk: HIGH_RISK_PERMISSIONS.contains(id),
                instance_scoped: INSTANCE_PERMISSIONS.contains(id),
            })
            .collect(),
    ))
}

async fn list_roles(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<RoleResponse>>, AppError> {
    auth.require("user.read")?;
    let roles = sqlx::query_as(
        "SELECT id, name, permissions, \
         is_system, created_at, updated_at FROM roles ORDER BY is_system DESC, name COLLATE NOCASE",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(roles))
}

async fn create_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<RoleResponse>), AppError> {
    require_owner(&auth)?;
    let name = validate_role_name(&body.name)?;
    let permissions = validate_permissions(body.permissions, false)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO roles (id, name, permissions, is_system, created_at, updated_at) \
         VALUES (?, ?, ?, 0, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(serde_json::to_string(&permissions).map_err(internal_json_error)?)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await;
    if let Err(error) = result {
        if is_unique_violation(&error) {
            return Err(AppError::Conflict("roles.name_already_exists".into()));
        }
        return Err(error.into());
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "role.created",
        "role",
        Some(&id),
        "success",
        serde_json::json!({"permissions": permissions}),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(fetch_role(&state, &id).await?)))
}

async fn update_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    require_owner(&auth)?;
    let current = fetch_role(&state, &id).await?;
    if current.is_system {
        return Err(AppError::Forbidden("roles.system_role_immutable".into()));
    }
    if body.name.is_none() && body.permissions.is_none() {
        return Err(AppError::BadRequest("roles.empty_update".into()));
    }
    let name = body
        .name
        .as_deref()
        .map(validate_role_name)
        .transpose()?
        .unwrap_or(current.name);
    let permissions = body
        .permissions
        .map(|values| validate_permissions(values, false))
        .transpose()?
        .unwrap_or(current.permissions);
    let now = chrono::Utc::now().to_rfc3339();
    let result =
        sqlx::query("UPDATE roles SET name = ?, permissions = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(serde_json::to_string(&permissions).map_err(internal_json_error)?)
            .bind(&now)
            .bind(&id)
            .execute(&state.pool)
            .await;
    if let Err(error) = result {
        if is_unique_violation(&error) {
            return Err(AppError::Conflict("roles.name_already_exists".into()));
        }
        return Err(error.into());
    }
    revoke_role_sessions(&state, &id).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "role.updated",
        "role",
        Some(&id),
        "success",
        serde_json::json!({"permissions": permissions}),
    )
    .await?;
    Ok(Json(fetch_role(&state, &id).await?))
}

async fn delete_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    require_owner(&auth)?;
    let current = fetch_role(&state, &id).await?;
    if current.is_system {
        return Err(AppError::Forbidden("roles.system_role_immutable".into()));
    }
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role_id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if users != 0 {
        return Err(AppError::Conflict("roles.in_use".into()));
    }
    sqlx::query("DELETE FROM roles WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "role.deleted",
        "role",
        Some(&id),
        "success",
        serde_json::json!({"name": current.name}),
    )
    .await?;
    Ok(SuccessResponse::with_message("roles.deleted"))
}

async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    auth.require("user.read")?;
    let users = if auth.role == "owner" {
        sqlx::query_as(
            "SELECT u.id, u.username, u.role_id, r.name AS role_name, u.is_active, \
             u.language, u.accent_color, u.must_change_password, u.last_login_at, \
             u.created_at, u.updated_at FROM users u JOIN roles r ON r.id = u.role_id \
             ORDER BY u.username COLLATE NOCASE",
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT u.id, u.username, u.role_id, r.name AS role_name, u.is_active, \
             u.language, u.accent_color, u.must_change_password, u.last_login_at, \
             u.created_at, u.updated_at FROM users u JOIN roles r ON r.id = u.role_id \
             WHERE u.role_id != 'owner' ORDER BY u.username COLLATE NOCASE",
        )
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(users))
}

async fn create_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), AppError> {
    auth.require("user.create")?;
    validate_username(body.username.trim())?;
    validate_password_strength(&body.password)?;
    validate_language(&body.language)?;
    let role = fetch_role(&state, &body.role_id).await?;
    ensure_role_assignable(&auth, &role)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO users (id, username, password_hash, role_id, language, \
         must_change_password, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(body.username.trim())
    .bind(hash_password(&body.password)?)
    .bind(&role.id)
    .bind(&body.language)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await;
    if let Err(error) = result {
        if is_unique_violation(&error) {
            return Err(AppError::Conflict("users.username_already_exists".into()));
        }
        return Err(error.into());
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "user.created",
        "user",
        Some(&id),
        "success",
        serde_json::json!({"role_id": role.id}),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(fetch_user(&state, &id).await?)))
}

async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    auth.require("user.update")?;
    if body.role_id.is_none()
        && body.is_active.is_none()
        && body.language.is_none()
        && body.accent_color.is_none()
        && body.password.is_none()
    {
        return Err(AppError::BadRequest("users.empty_update".into()));
    }
    let current = ensure_manageable_user(&state, &auth, &id).await?;
    if id == auth.id && body.is_active == Some(false) {
        return Err(AppError::BadRequest("users.cannot_disable_self".into()));
    }
    if let Some(language) = body.language.as_deref() {
        validate_language(language)?;
    }
    if let Some(color) = body.accent_color.as_deref() {
        validate_accent_color(color)?;
    }
    if let Some(password) = body.password.as_deref() {
        validate_password_strength(password)?;
    }
    let target_role = if let Some(role_id) = body.role_id.as_deref() {
        let role = fetch_role(&state, role_id).await?;
        ensure_role_assignable(&auth, &role)?;
        role.id
    } else {
        current.role_id.clone()
    };
    let active = body.is_active.unwrap_or(current.is_active);
    let removes_active_owner =
        current.role_id == "owner" && current.is_active && (target_role != "owner" || !active);
    let password_hash = body.password.as_deref().map(hash_password).transpose()?;
    let now = chrono::Utc::now().to_rfc3339();
    let updated = sqlx::query(
        "UPDATE users SET role_id = ?, is_active = ?, language = COALESCE(?, language), \
         accent_color = COALESCE(?, accent_color), password_hash = COALESCE(?, password_hash), \
         must_change_password = CASE WHEN ? IS NULL THEN must_change_password ELSE 1 END, \
         updated_at = ? WHERE id = ? AND \
         (? = 0 OR (SELECT COUNT(*) FROM users WHERE role_id = 'owner' AND is_active = 1) > 1)",
    )
    .bind(&target_role)
    .bind(active)
    .bind(body.language.as_deref())
    .bind(body.accent_color.as_deref())
    .bind(password_hash.as_deref())
    .bind(password_hash.as_deref())
    .bind(&now)
    .bind(&id)
    .bind(removes_active_owner)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict("users.last_owner_required".into()));
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "user.updated",
        "user",
        Some(&id),
        "success",
        serde_json::json!({
            "role_id": target_role,
            "is_active": active,
            "password_reset": password_hash.is_some(),
        }),
    )
    .await?;
    Ok(Json(fetch_user(&state, &id).await?))
}

async fn list_grants(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<GrantResponse>>, AppError> {
    auth.require("user.read")?;
    ensure_manageable_user(&state, &auth, &id).await?;
    let rows = sqlx::query_as(
        "SELECT g.instance_id, i.name AS instance_name, g.permissions, g.created_at \
         FROM user_instance_grants g JOIN instances i ON i.id = g.instance_id \
         WHERE g.user_id = ? ORDER BY i.name COLLATE NOCASE",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;
    let scope = instance_grant_scope(&state, &auth).await?;
    Ok(Json(
        rows.into_iter()
            .filter(|grant: &GrantResponse| scope.allows(&auth, &grant.instance_id, "server.read"))
            .collect(),
    ))
}

async fn set_grant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((user_id, instance_id)): Path<(String, String)>,
    Json(body): Json<SetGrantRequest>,
) -> Result<Json<GrantResponse>, AppError> {
    auth.require("user.update")?;
    let user = ensure_manageable_user(&state, &auth, &user_id).await?;
    Uuid::parse_str(&instance_id).map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id = ?)")
        .bind(&instance_id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(AppError::NotFound("servers.not_found".into()));
    }
    let permissions = validate_permissions(body.permissions, true)?;
    ensure_grant_within_role(&state, &user.role_id, &permissions).await?;
    let target_role = fetch_role(&state, &user.role_id).await?;
    let scope = instance_grant_scope(&state, &auth).await?;
    if !grant_permissions_within_actor(&auth, &scope, &instance_id, &target_role, &permissions) {
        return Err(AppError::Forbidden(
            "assignments.permission_exceeds_actor".into(),
        ));
    }
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO user_instance_grants (user_id, instance_id, permissions, created_at) \
         VALUES (?, ?, ?, ?) ON CONFLICT(user_id, instance_id) DO UPDATE SET \
         permissions = excluded.permissions, created_at = excluded.created_at",
    )
    .bind(&user_id)
    .bind(&instance_id)
    .bind(serde_json::to_string(&permissions).map_err(internal_json_error)?)
    .bind(&now)
    .execute(&state.pool)
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "instance.assignment_updated",
        "instance",
        Some(&instance_id),
        "success",
        serde_json::json!({"user_id": user_id, "permissions": permissions}),
    )
    .await?;
    Ok(Json(fetch_grant(&state, &user_id, &instance_id).await?))
}

async fn delete_grant(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((user_id, instance_id)): Path<(String, String)>,
) -> Result<Json<SuccessResponse>, AppError> {
    auth.require("user.update")?;
    ensure_manageable_user(&state, &auth, &user_id).await?;
    Uuid::parse_str(&instance_id).map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    let scope = instance_grant_scope(&state, &auth).await?;
    if !scope.allows(&auth, &instance_id, "server.read") {
        return Err(AppError::Forbidden("auth.instance_not_assigned".into()));
    }
    let deleted =
        sqlx::query("DELETE FROM user_instance_grants WHERE user_id = ? AND instance_id = ?")
            .bind(&user_id)
            .bind(&instance_id)
            .execute(&state.pool)
            .await?;
    if deleted.rows_affected() != 1 {
        return Err(AppError::NotFound("assignments.not_found".into()));
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "instance.assignment_deleted",
        "instance",
        Some(&instance_id),
        "success",
        serde_json::json!({"user_id": user_id}),
    )
    .await?;
    Ok(SuccessResponse::with_message("assignments.deleted"))
}

fn require_owner(auth: &AuthUser) -> Result<(), AppError> {
    if auth.role == "owner" {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.owner_required".into()))
    }
}

async fn ensure_manageable_user(
    state: &AppState,
    auth: &AuthUser,
    id: &str,
) -> Result<UserResponse, AppError> {
    let user = fetch_user(state, id).await?;
    if auth.role != "owner" {
        let role = fetch_role(state, &user.role_id).await?;
        ensure_role_assignable(auth, &role)?;
    }
    Ok(user)
}

fn ensure_role_assignable(auth: &AuthUser, role: &RoleResponse) -> Result<(), AppError> {
    if !role_is_within_actor_permissions(&auth.role, &auth.permissions, role) {
        return Err(AppError::Forbidden("users.role_exceeds_permissions".into()));
    }
    Ok(())
}

fn role_is_within_actor_permissions(
    actor_role: &str,
    actor_permissions: &[String],
    role: &RoleResponse,
) -> bool {
    actor_role == "owner"
        || (role.id != "owner"
            && !role.permissions.iter().any(|permission| permission == "*")
            && role.permissions.iter().all(|permission| {
                actor_permissions
                    .iter()
                    .any(|owned| owned == "*" || owned == permission)
            }))
}

async fn fetch_role(state: &AppState, id: &str) -> Result<RoleResponse, AppError> {
    sqlx::query_as(
        "SELECT id, name, permissions, \
         is_system, created_at, updated_at FROM roles WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("roles.not_found".into()))
}

async fn fetch_user(state: &AppState, id: &str) -> Result<UserResponse, AppError> {
    sqlx::query_as(
        "SELECT u.id, u.username, u.role_id, r.name AS role_name, u.is_active, \
         u.language, u.accent_color, u.must_change_password, u.last_login_at, \
         u.created_at, u.updated_at FROM users u JOIN roles r ON r.id = u.role_id \
         WHERE u.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("users.not_found".into()))
}

async fn fetch_grant(
    state: &AppState,
    user_id: &str,
    instance_id: &str,
) -> Result<GrantResponse, AppError> {
    sqlx::query_as(
        "SELECT g.instance_id, i.name AS instance_name, g.permissions, g.created_at \
         FROM user_instance_grants g JOIN instances i ON i.id = g.instance_id \
         WHERE g.user_id = ? AND g.instance_id = ?",
    )
    .bind(user_id)
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("assignments.not_found".into()))
}

async fn revoke_role_sessions(state: &AppState, role_id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE user_id IN (SELECT id FROM users WHERE role_id = ?)")
        .bind(role_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn ensure_grant_within_role(
    state: &AppState,
    role_id: &str,
    permissions: &[String],
) -> Result<(), AppError> {
    if permissions.is_empty() {
        return Ok(());
    }
    let role = fetch_role(state, role_id).await?;
    if role.permissions.iter().any(|permission| permission == "*")
        || permissions
            .iter()
            .all(|permission| role.permissions.contains(permission))
    {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "assignments.permission_exceeds_role".into(),
        ))
    }
}

fn grant_permissions_within_actor(
    auth: &AuthUser,
    scope: &InstanceGrantScope,
    instance_id: &str,
    target_role: &RoleResponse,
    requested: &[String],
) -> bool {
    if !scope.allows(auth, instance_id, "server.read") {
        return false;
    }
    let effective = if requested.is_empty() {
        target_role
            .permissions
            .iter()
            .filter(|permission| INSTANCE_PERMISSIONS.contains(&permission.as_str()))
            .collect::<Vec<_>>()
    } else {
        requested.iter().collect::<Vec<_>>()
    };
    effective
        .into_iter()
        .all(|permission| scope.allows(auth, instance_id, permission))
}

fn validate_permissions(
    permissions: Vec<String>,
    instance_only: bool,
) -> Result<Vec<String>, AppError> {
    let allowed = if instance_only {
        INSTANCE_PERMISSIONS
    } else {
        PERMISSIONS
    };
    if permissions.len() > allowed.len() {
        return Err(AppError::BadRequest("roles.invalid_permissions".into()));
    }
    let mut unique = HashSet::new();
    for permission in &permissions {
        if permission == "*"
            || !allowed.contains(&permission.as_str())
            || !unique.insert(permission)
        {
            return Err(AppError::BadRequest("roles.invalid_permissions".into()));
        }
    }
    let mut permissions = permissions;
    permissions.sort_unstable();
    Ok(permissions)
}

fn validate_role_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
        Err(AppError::BadRequest("roles.invalid_name".into()))
    } else {
        Ok(value.to_string())
    }
}

fn validate_language(value: &str) -> Result<(), AppError> {
    if matches!(value, "fr" | "en") {
        Ok(())
    } else {
        Err(AppError::BadRequest("users.invalid_language".into()))
    }
}

fn validate_accent_color(value: &str) -> Result<(), AppError> {
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(AppError::BadRequest("users.invalid_accent_color".into()))
    }
}

fn default_language() -> String {
    "fr".into()
}

fn internal_json_error(error: serde_json::Error) -> AppError {
    AppError::Internal(error.to_string())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_permissions_are_closed_and_deduplicated() {
        assert!(validate_permissions(vec!["server.read".into()], false).is_ok());
        assert!(validate_permissions(vec!["*".into()], false).is_err());
        assert!(
            validate_permissions(vec!["server.read".into(), "server.read".into()], false).is_err()
        );
        assert!(validate_permissions(vec!["system.shell".into()], false).is_err());
    }

    #[test]
    fn global_permissions_cannot_be_assigned_per_instance() {
        assert!(validate_permissions(vec!["user.update".into()], true).is_err());
        assert!(validate_permissions(vec!["server.console.write".into()], true).is_ok());
    }

    #[test]
    fn role_names_and_colors_are_bounded() {
        assert_eq!(validate_role_name("  Moderators ").unwrap(), "Moderators");
        assert!(validate_role_name("\n").is_err());
        assert!(validate_accent_color("#3A82F6").is_ok());
        assert!(validate_accent_color("red").is_err());
    }

    #[test]
    fn non_owner_cannot_assign_a_role_above_their_current_permissions() {
        let actor_permissions = vec!["user.update".into(), "server.read".into()];
        let role = |id: &str, permissions: &[&str]| RoleResponse {
            id: id.into(),
            name: id.into(),
            permissions: permissions.iter().map(|value| (*value).into()).collect(),
            is_system: false,
            created_at: "2026-07-13T00:00:00Z".into(),
            updated_at: "2026-07-13T00:00:00Z".into(),
        };

        assert!(role_is_within_actor_permissions(
            "admin",
            &actor_permissions,
            &role("viewer", &["server.read"]),
        ));
        assert!(!role_is_within_actor_permissions(
            "admin",
            &actor_permissions,
            &role("custom", &["profile.manage"]),
        ));
        assert!(!role_is_within_actor_permissions(
            "admin",
            &actor_permissions,
            &role("owner", &["*"]),
        ));
        assert!(role_is_within_actor_permissions(
            "owner",
            &[],
            &role("owner", &["*"]),
        ));
    }

    #[test]
    fn delegated_assignment_cannot_cross_instances_or_extend_the_actor_grant() {
        let actor = AuthUser::for_test(
            "delegated-admin",
            "custom",
            [
                "user.update",
                "server.read",
                "job.read",
                "server.console.read",
            ],
        );
        let scope = InstanceGrantScope::for_test(
            false,
            [(
                "server-a".into(),
                vec!["server.read".into(), "job.read".into()],
            )],
        );
        let target_role = RoleResponse {
            id: "custom-target".into(),
            name: "Custom target".into(),
            permissions: vec![
                "server.read".into(),
                "job.read".into(),
                "server.console.read".into(),
            ],
            is_system: false,
            created_at: "2026-07-13T00:00:00Z".into(),
            updated_at: "2026-07-13T00:00:00Z".into(),
        };

        assert!(grant_permissions_within_actor(
            &actor,
            &scope,
            "server-a",
            &target_role,
            &["server.read".into(), "job.read".into()],
        ));
        assert!(!grant_permissions_within_actor(
            &actor,
            &scope,
            "server-a",
            &target_role,
            &[],
        ));
        assert!(!grant_permissions_within_actor(
            &actor,
            &scope,
            "server-b",
            &target_role,
            &["server.read".into()],
        ));
    }
}
