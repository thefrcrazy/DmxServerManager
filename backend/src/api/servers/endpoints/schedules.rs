use axum::{
    extract::{Path, State},
    Json,
    http::StatusCode,
};
use chrono::Utc;
use uuid::Uuid;

use crate::core::AppState;
use crate::core::error::AppError;
use crate::api::servers::models::{ScheduleRow, ScheduleResponse, CreateScheduleRequest, ToggleScheduleRequest};

pub async fn list_schedules(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
) -> Result<Json<Vec<ScheduleResponse>>, AppError> {
    let schedules: Vec<ScheduleRow> = sqlx::query_as(
        "SELECT * FROM schedules WHERE server_id = ? ORDER BY created_at DESC"
    )
    .bind(&server_id)
    .fetch_all(&state.pool)
    .await?;

    let responses = schedules.into_iter().map(|s| ScheduleResponse {
        id: s.id,
        server_id: s.server_id,
        name: s.name,
        task_type: s.task_type,
        action: s.action,
        interval: s.interval,
        unit: s.unit,
        time: s.time,
        cron_expression: s.cron_expression,
        enabled: s.enabled != 0,
        delete_after: s.delete_after != 0,
        created_at: s.created_at,
    }).collect();

    Ok(Json(responses))
}

pub async fn create_schedule(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO schedules (
            id, server_id, name, task_type, action, interval, unit, time, cron_expression, enabled, delete_after, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&server_id)
    .bind(&body.name)
    .bind(&body.task_type)
    .bind(&body.action)
    .bind(body.interval)
    .bind(&body.unit)
    .bind(&body.time)
    .bind(&body.cron_expression)
    .bind(body.enabled.unwrap_or(true) as i32)
    .bind(body.delete_after.unwrap_or(false) as i32)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(ScheduleResponse {
        id,
        server_id,
        name: body.name,
        task_type: body.task_type,
        action: body.action,
        interval: body.interval,
        unit: body.unit,
        time: body.time,
        cron_expression: body.cron_expression,
        enabled: body.enabled.unwrap_or(true),
        delete_after: body.delete_after.unwrap_or(false),
        created_at: now,
    })))
}

pub async fn update_schedule(
    State(state): State<AppState>,
    Path((_server_id, schedule_id)): Path<(String, String)>,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<Json<ScheduleResponse>, AppError> {
    sqlx::query(
        "UPDATE schedules SET 
        name = ?, task_type = ?, action = ?, interval = ?, unit = ?, time = ?, cron_expression = ?, enabled = ?, delete_after = ?
        WHERE id = ?"
    )
    .bind(&body.name)
    .bind(&body.task_type)
    .bind(&body.action)
    .bind(body.interval)
    .bind(&body.unit)
    .bind(&body.time)
    .bind(&body.cron_expression)
    .bind(body.enabled.unwrap_or(true) as i32)
    .bind(body.delete_after.unwrap_or(false) as i32)
    .bind(&schedule_id)
    .execute(&state.pool)
    .await?;

    let s: ScheduleRow = sqlx::query_as("SELECT * FROM schedules WHERE id = ?")
        .bind(&schedule_id)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(ScheduleResponse {
        id: s.id,
        server_id: s.server_id,
        name: s.name,
        task_type: s.task_type,
        action: s.action,
        interval: s.interval,
        unit: s.unit,
        time: s.time,
        cron_expression: s.cron_expression,
        enabled: s.enabled != 0,
        delete_after: s.delete_after != 0,
        created_at: s.created_at,
    }))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    Path((_server_id, schedule_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("DELETE FROM schedules WHERE id = ?")
        .bind(&schedule_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn toggle_schedule(
    State(state): State<AppState>,
    Path((_server_id, schedule_id)): Path<(String, String)>,
    Json(body): Json<ToggleScheduleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    sqlx::query("UPDATE schedules SET enabled = ? WHERE id = ?")
        .bind(body.enabled as i32)
        .bind(&schedule_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
