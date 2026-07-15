use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, database, error::AppError},
    services::schedules::{self, Schedule, ScheduleAction, ScheduleSpec, ScheduleTrigger},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/schedules", get(list).post(create))
        .route("/schedules/{id}", get(get_one).put(update).delete(remove))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateScheduleRequest {
    instance_id: String,
    name: String,
    trigger: ScheduleTrigger,
    action: ScheduleAction,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateScheduleRequest {
    name: String,
    trigger: ScheduleTrigger,
    action: ScheduleAction,
    enabled: bool,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Schedule>>, AppError> {
    validate_uuid(&query.instance_id, "servers.invalid_id")?;
    authorize_instance(&state, &auth, &query.instance_id, "schedule.manage").await?;
    ensure_instance(&state, &query.instance_id).await?;
    Ok(Json(
        schedules::list(&state.pool, &query.instance_id).await?,
    ))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<(HeaderMap, Json<Schedule>), AppError> {
    validate_uuid(&id, "schedules.invalid_id")?;
    let schedule = schedules::get(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &schedule.instance_id, "schedule.manage").await?;
    Ok((etag_headers(schedule.version)?, Json(schedule)))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, HeaderMap, Json<Schedule>), AppError> {
    validate_uuid(&body.instance_id, "servers.invalid_id")?;
    authorize_instance(&state, &auth, &body.instance_id, "schedule.manage").await?;
    validate_action_for_instance(&state, &auth, &body.instance_id, &body.action).await?;
    let schedule = schedules::create(
        &state.pool,
        &body.instance_id,
        ScheduleSpec {
            name: body.name,
            trigger: body.trigger,
            action: body.action,
            enabled: body.enabled,
        },
        &auth.id,
    )
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "schedule.created",
        "schedule",
        Some(&schedule.id),
        "success",
        serde_json::json!({
            "instance_id": schedule.instance_id,
            "trigger": trigger_kind(&schedule.trigger),
            "action": schedule.action.kind(),
            "enabled": schedule.enabled,
        }),
    )
    .await?;
    state.events.publish(
        "schedule.created",
        Some(schedule.instance_id.clone()),
        serde_json::json!({"schedule_id": schedule.id}),
    );
    Ok((
        StatusCode::CREATED,
        etag_headers(schedule.version)?,
        Json(schedule),
    ))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateScheduleRequest>,
) -> Result<(HeaderMap, Json<Schedule>), AppError> {
    validate_uuid(&id, "schedules.invalid_id")?;
    let current = schedules::get(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &current.instance_id, "schedule.manage").await?;
    validate_action_for_instance(&state, &auth, &current.instance_id, &body.action).await?;
    let expected_version = parse_if_match(&headers)?;
    if current.version != expected_version {
        return Err(AppError::Conflict("schedules.version_conflict".into()));
    }
    let previous_version = current.version;
    let schedule = schedules::update(
        &state.pool,
        &id,
        expected_version,
        ScheduleSpec {
            name: body.name,
            trigger: body.trigger,
            action: body.action,
            enabled: body.enabled,
        },
        &auth.id,
    )
    .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "schedule.updated",
        "schedule",
        Some(&schedule.id),
        "success",
        serde_json::json!({
            "instance_id": schedule.instance_id,
            "previous_version": previous_version,
            "trigger": trigger_kind(&schedule.trigger),
            "action": schedule.action.kind(),
            "console_contents_recorded": false,
            "enabled": schedule.enabled,
        }),
    )
    .await?;
    state.events.publish(
        "schedule.updated",
        Some(schedule.instance_id.clone()),
        serde_json::json!({"schedule_id": schedule.id}),
    );
    Ok((etag_headers(schedule.version)?, Json(schedule)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    validate_uuid(&id, "schedules.invalid_id")?;
    let schedule = schedules::get(&state.pool, &id).await?;
    authorize_instance(&state, &auth, &schedule.instance_id, "schedule.manage").await?;
    schedules::remove(&state.pool, &id).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "schedule.deleted",
        "schedule",
        Some(&id),
        "success",
        serde_json::json!({"instance_id": schedule.instance_id}),
    )
    .await?;
    state.events.publish(
        "schedule.deleted",
        Some(schedule.instance_id),
        serde_json::json!({"schedule_id": id}),
    );
    Ok(SuccessResponse::with_message("schedules.deleted"))
}

async fn validate_action_for_instance(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
    action: &ScheduleAction,
) -> Result<(), AppError> {
    for permission in action.required_permissions() {
        authorize_instance(state, auth, instance_id, permission).await?;
    }
    let profile_id: String = sqlx::query_scalar("SELECT profile_id FROM instances WHERE id = ?")
        .bind(instance_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    let profile = state
        .profiles
        .get(&profile_id)
        .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    let required_capability = match action {
        ScheduleAction::Start {} | ScheduleAction::Stop {} | ScheduleAction::Restart {} => {
            "lifecycle"
        }
        ScheduleAction::Backup {} => "backups",
        ScheduleAction::Update {} => "install",
        ScheduleAction::Console { .. } => "console",
    };
    if !profile
        .capabilities
        .iter()
        .any(|capability| capability == required_capability)
    {
        return Err(AppError::BadRequest(
            "schedules.action_not_supported".into(),
        ));
    }
    Ok(())
}

async fn ensure_instance(state: &AppState, id: &str) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id = ?)")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound("servers.not_found".into()))
    }
}

fn etag_headers(version: u32) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{version}\""))
            .map_err(|error| AppError::Internal(error.to_string()))?,
    );
    Ok(headers)
}

fn parse_if_match(headers: &HeaderMap) -> Result<u32, AppError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().trim_start_matches("W/").trim_matches('"'))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| AppError::PreconditionRequired("schedules.if_match_required".into()))
}

fn validate_uuid(id: &str, message: &str) -> Result<(), AppError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest(message.into()))
}

fn trigger_kind(trigger: &ScheduleTrigger) -> &'static str {
    match trigger {
        ScheduleTrigger::Cron { .. } => "cron",
        ScheduleTrigger::Interval { .. } => "interval",
    }
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_etags_are_required_and_strictly_numeric() {
        let headers = HeaderMap::new();
        assert!(parse_if_match(&headers).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"12\""));
        assert_eq!(parse_if_match(&headers).unwrap(), 12);
        headers.insert(header::IF_MATCH, HeaderValue::from_static("*"));
        assert!(parse_if_match(&headers).is_err());
    }
}
