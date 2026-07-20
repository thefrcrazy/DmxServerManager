use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use sqlx::FromRow;

use crate::{
    api::auth::{AuthUser, InstanceGrantScope, authorize_instance, instance_grant_scope},
    core::{AppState, database, error::AppError},
    domain::v1::{Job, JobState},
    services::jobs,
};

#[derive(Debug, FromRow)]
pub(crate) struct JobRow {
    id: String,
    instance_id: Option<String>,
    kind: String,
    state: String,
    progress: i64,
    requested_by: String,
    error_code: Option<String>,
    error_message: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    interaction_payload: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/jobs", get(list))
        .route("/jobs/{id}", get(get_one))
        .route("/jobs/{id}/cancel", post(cancel))
}

async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Job>), AppError> {
    let job = jobs::get(&state.pool, &id).await?;
    let instance_id = job
        .instance_id
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?;
    authorize_instance(&state, &auth, instance_id, "server.update_game").await?;
    if job.kind != "install" {
        return Err(AppError::Conflict("jobs.cancellation_not_supported".into()));
    }
    if !matches!(
        job.state,
        JobState::Queued | JobState::Running | JobState::WaitingForUser
    ) {
        return Err(AppError::Conflict("jobs.not_cancellable".into()));
    }

    let accepted = if job.state == JobState::Running {
        state
            .runtime
            .request_install_cancel(instance_id, &job.id)
            .await?
    } else {
        state
            .runtime
            .cancel_waiting_install(instance_id, &job.id)
            .await?
    };
    if !accepted {
        return Err(AppError::Conflict("jobs.cancellation_unavailable".into()));
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "job.cancel",
        "job",
        Some(&job.id),
        "success",
        serde_json::json!({"instance_id": instance_id, "kind": job.kind}),
    )
    .await?;
    let updated = jobs::get(&state.pool, &job.id).await?;
    state.events.publish(
        "job.updated",
        Some(instance_id.to_string()),
        serde_json::to_value(&updated).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(updated)))
}

async fn list(State(state): State<AppState>, auth: AuthUser) -> Result<Json<Vec<Job>>, AppError> {
    auth.require("job.read")?;
    let rows: Vec<JobRow> = if matches!(auth.role.as_str(), "owner" | "admin") {
        sqlx::query_as(
            "SELECT j.*, CASE WHEN j.state = 'waiting_for_user' THEN ( \
                 SELECT e.payload FROM job_events e \
                 WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user' \
                 ORDER BY e.id DESC LIMIT 1 \
             ) ELSE NULL END AS interaction_payload \
             FROM jobs j ORDER BY j.created_at DESC LIMIT 200",
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT j.*, CASE WHEN j.state = 'waiting_for_user' THEN (
                SELECT e.payload FROM job_events e
                WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user'
                ORDER BY e.id DESC LIMIT 1
            ) ELSE NULL END AS interaction_payload
            FROM jobs j
            JOIN user_instance_grants g ON g.instance_id = j.instance_id
            WHERE g.user_id = ? AND (
                json_array_length(g.permissions) = 0 OR EXISTS (
                    SELECT 1 FROM json_each(g.permissions)
                    WHERE value IN ('*', 'job.read')
                )
            )
            ORDER BY j.created_at DESC LIMIT 200
            "#,
        )
        .bind(&auth.id)
        .fetch_all(&state.pool)
        .await?
    };
    let interaction_scope = instance_grant_scope(&state, &auth).await?;
    rows.into_iter()
        .map(job_from_row)
        .map(|result| result.map(|job| protect_job_interaction(job, &auth, &interaction_scope)))
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    auth.require("job.read")?;
    let row: JobRow = sqlx::query_as(
        "SELECT j.*, CASE WHEN j.state = 'waiting_for_user' THEN ( \
             SELECT e.payload FROM job_events e \
             WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user' \
             ORDER BY e.id DESC LIMIT 1 \
         ) ELSE NULL END AS interaction_payload \
         FROM jobs j WHERE j.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("jobs.not_found".into()))?;
    if let Some(instance_id) = &row.instance_id {
        authorize_instance(&state, &auth, instance_id, "job.read").await?;
    } else if !matches!(auth.role.as_str(), "owner" | "admin") {
        return Err(AppError::Forbidden("auth.permission_denied".into()));
    }
    let interaction_scope = instance_grant_scope(&state, &auth).await?;
    Ok(Json(protect_job_interaction(
        job_from_row(row)?,
        &auth,
        &interaction_scope,
    )))
}

pub(crate) fn protect_job_interaction(
    mut job: Job,
    auth: &AuthUser,
    interaction_scope: &InstanceGrantScope,
) -> Job {
    let may_view = job.instance_id.as_deref().is_some_and(|instance_id| {
        interaction_scope.allows(auth, instance_id, "server.update_game")
    });
    if !may_view {
        job.interaction = None;
    }
    job
}

pub(crate) fn job_from_row(row: JobRow) -> Result<Job, AppError> {
    let state = match row.state.as_str() {
        "queued" => JobState::Queued,
        "running" => JobState::Running,
        "waiting_for_user" => JobState::WaitingForUser,
        "succeeded" => JobState::Succeeded,
        "failed" => JobState::Failed,
        "cancelled" => JobState::Cancelled,
        "interrupted" => JobState::Interrupted,
        _ => return Err(AppError::Internal("invalid job state".into())),
    };
    let interaction = jobs::validated_interaction(
        &row.id,
        &state,
        row.instance_id.as_deref(),
        row.interaction_payload.as_deref(),
    );
    Ok(Job {
        id: row.id,
        instance_id: row.instance_id,
        kind: row.kind,
        state,
        progress: u8::try_from(row.progress)
            .map_err(|_| AppError::Internal("invalid job progress".into()))?,
        requested_by: row.requested_by,
        error_code: row.error_code,
        error_message: row.error_message,
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        interaction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::v1::JobInteraction;

    fn waiting_job() -> Job {
        Job {
            id: "job-id".into(),
            instance_id: Some("instance-id".into()),
            kind: "install".into(),
            state: JobState::WaitingForUser,
            progress: 25,
            requested_by: "operator-id".into(),
            error_code: None,
            error_message: None,
            created_at: "2026-07-13T00:00:00Z".into(),
            started_at: Some("2026-07-13T00:00:01Z".into()),
            finished_at: None,
            interaction: Some(JobInteraction::OauthDevice {
                verification_uri: "https://accounts.hytale.com/device?user_code=SECRET-CODE".into(),
                user_code: Some("SECRET-CODE".into()),
            }),
        }
    }

    #[test]
    fn viewer_job_responses_never_expose_waiting_interactions() {
        let scope =
            InstanceGrantScope::for_test(false, [("instance-id".into(), Vec::<String>::new())]);
        let viewer = AuthUser::for_test("viewer-id", "viewer", ["server.read", "job.read"]);
        assert_eq!(
            protect_job_interaction(waiting_job(), &viewer, &scope).interaction,
            None
        );

        let operator = AuthUser::for_test(
            "operator-id",
            "operator",
            ["server.read", "job.read", "server.update_game"],
        );
        assert!(
            protect_job_interaction(waiting_job(), &operator, &scope)
                .interaction
                .is_some()
        );
    }
}
