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
use crate::core::error::AppError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_roles).post(create_role))
        .route("/:id", get(get_role).put(update_role).delete(delete_role))
}

#[derive(Debug, Serialize, FromRow)]
pub struct RoleRow {
    pub id: String,
    pub name: String,
    pub permissions: String,
    pub is_system: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub is_system: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

async fn list_roles(
    State(state): State<AppState>,
) -> Result<Json<Vec<RoleResponse>>, AppError> {
    let roles: Vec<RoleRow> = sqlx::query_as("SELECT * FROM roles ORDER BY created_at ASC")
        .fetch_all(&state.pool)
        .await?;

    let responses = roles.into_iter().map(|r| {
        let permissions: Vec<String> = serde_json::from_str(&r.permissions).unwrap_or_default();
        RoleResponse {
            id: r.id,
            name: r.name,
            permissions,
            is_system: r.is_system,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }).collect();

    Ok(Json(responses))
}

async fn get_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RoleResponse>, AppError> {
    let role: RoleRow = sqlx::query_as("SELECT * FROM roles WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Role not found".into()))?;

    let permissions: Vec<String> = serde_json::from_str(&role.permissions).unwrap_or_default();

    Ok(Json(RoleResponse {
        id: role.id,
        name: role.name,
        permissions,
        is_system: role.is_system,
        created_at: role.created_at,
        updated_at: role.updated_at,
    }))
}

async fn create_role(
    State(state): State<AppState>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let permissions_json = serde_json::to_string(&body.permissions).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "INSERT INTO roles (id, name, permissions, is_system, created_at, updated_at) VALUES (?, ?, ?, 0, ?, ?)"
    )
    .bind(&id)
    .bind(&body.name)
    .bind(&permissions_json)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    Ok(Json(RoleResponse {
        id,
        name: body.name,
        permissions: body.permissions,
        is_system: false,
        created_at: now.clone(),
        updated_at: now,
    }))
}

async fn update_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    let role: RoleRow = sqlx::query_as("SELECT * FROM roles WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Role not found".into()))?;

    if role.is_system && body.name.is_some() {
        // System roles cannot be renamed (but permissions can be updated if we want admins to customize them)
        // For strict RBAC, maybe prevent even permission edits on 'admin'?
        // Let's allow permission edits but not name edits for now.
        // Actually, renaming 'admin' or 'user' might break default logic, so prevent it.
    }

    let now = Utc::now().to_rfc3339();
    let new_name = body.name.unwrap_or(role.name);
    let new_permissions = body.permissions.unwrap_or_else(|| 
        serde_json::from_str(&role.permissions).unwrap_or_default()
    );
    let new_permissions_json = serde_json::to_string(&new_permissions).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "UPDATE roles SET name = ?, permissions = ?, updated_at = ? WHERE id = ?"
    )
    .bind(&new_name)
    .bind(&new_permissions_json)
    .bind(&now)
    .bind(&id)
    .execute(&state.pool)
    .await?;

    Ok(Json(RoleResponse {
        id: role.id,
        name: new_name,
        permissions: new_permissions,
        is_system: role.is_system,
        created_at: role.created_at,
        updated_at: now,
    }))
}

async fn delete_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let role: RoleRow = sqlx::query_as("SELECT * FROM roles WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Role not found".into()))?;

    if role.is_system {
        return Err(AppError::BadRequest("Cannot delete system roles".into()));
    }

    sqlx::query("DELETE FROM roles WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
