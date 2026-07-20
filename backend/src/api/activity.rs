use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};
use uuid::Uuid;

use crate::{
    api::{
        auth::{AuthUser, instance_grant_scope},
        jobs::{JobRow, job_from_row, protect_job_interaction},
    },
    core::{AppState, error::AppError},
    domain::v1::Job,
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const MAX_PAGE_SIZE: u16 = 100;
const JOB_SELECT: &str = "SELECT j.*, CASE WHEN j.state = 'waiting_for_user' THEN (\
         SELECT e.payload FROM job_events e WHERE e.job_id = j.id \
         AND e.event_type = 'job.waiting_for_user' ORDER BY e.id DESC LIMIT 1\
     ) ELSE NULL END AS interaction_payload FROM jobs j";

#[derive(Debug, Serialize)]
pub struct ActivitySummary {
    pub active_jobs: i64,
    pub waiting_for_user: i64,
    pub failed_jobs_24h: i64,
    pub crashed_servers: i64,
    pub config_conflicts: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityJobsQuery {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
    pub state: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActivityJobsPage {
    pub items: Vec<Job>,
    pub next_cursor: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/activity/summary", get(summary))
        .route("/activity/jobs", get(list_jobs))
}

async fn summary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ActivitySummary>, AppError> {
    auth.require("server.read")?;
    let unrestricted = matches!(auth.role.as_str(), "owner" | "admin");
    let active_jobs = if auth.has_permission("job.read") {
        count_jobs(&state, &auth, unrestricted, "active", None).await?
    } else {
        0
    };
    let waiting_for_user = if auth.has_permission("job.read") {
        count_jobs(&state, &auth, unrestricted, "waiting", None).await?
    } else {
        0
    };
    let failed_jobs_24h = if auth.has_permission("job.read") {
        count_jobs(
            &state,
            &auth,
            unrestricted,
            "failed",
            Some(&(Utc::now() - Duration::hours(24)).to_rfc3339()),
        )
        .await?
    } else {
        0
    };
    let crashed_servers = count_instances(&state, &auth, unrestricted, true).await?;
    let config_conflicts = count_config_conflicts(&state, &auth, unrestricted).await?;
    Ok(Json(ActivitySummary {
        active_jobs,
        waiting_for_user,
        failed_jobs_24h,
        crashed_servers,
        config_conflicts,
    }))
}

async fn list_jobs(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ActivityJobsQuery>,
) -> Result<Json<ActivityJobsPage>, AppError> {
    auth.require("job.read")?;
    validate_jobs_query(&query)?;
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);
    let unrestricted = matches!(auth.role.as_str(), "owner" | "admin");
    let cursor = if let Some(cursor) = query.cursor.as_deref() {
        sqlx::query_as::<_, (String, String)>(
            "SELECT j.created_at, j.id FROM jobs j WHERE j.id = ? AND \
             (? = 1 OR EXISTS (SELECT 1 FROM user_instance_grants g \
                 WHERE g.instance_id = j.instance_id AND g.user_id = ? \
                 AND (json_array_length(g.permissions) = 0 OR EXISTS (\
                     SELECT 1 FROM json_each(g.permissions) WHERE value IN ('*', 'job.read')\
                 ))))",
        )
        .bind(cursor)
        .bind(unrestricted)
        .bind(&auth.id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("activity.invalid_cursor".into()))?
        .into()
    } else {
        None
    };

    let mut statement = QueryBuilder::<Sqlite>::new(JOB_SELECT);
    if unrestricted {
        statement.push(" WHERE 1 = 1");
    } else {
        statement.push(
            " JOIN user_instance_grants g ON g.instance_id = j.instance_id \
             WHERE g.user_id = ",
        );
        statement.push_bind(&auth.id);
        statement.push(
            " AND (json_array_length(g.permissions) = 0 OR EXISTS (\
             SELECT 1 FROM json_each(g.permissions) WHERE value IN ('*', 'job.read')))",
        );
    }
    if let Some(state_filter) = query.state.as_deref() {
        statement.push(" AND j.state = ").push_bind(state_filter);
    }
    if let Some(instance_id) = query.instance_id.as_deref() {
        statement
            .push(" AND j.instance_id = ")
            .push_bind(instance_id);
    }
    if let Some((created_at, id)) = cursor {
        statement
            .push(" AND (j.created_at < ")
            .push_bind(created_at.clone())
            .push(" OR (j.created_at = ")
            .push_bind(created_at)
            .push(" AND j.id < ")
            .push_bind(id)
            .push("))");
    }
    statement
        .push(" ORDER BY j.created_at DESC, j.id DESC LIMIT ")
        .push_bind(i64::from(limit) + 1);

    let mut rows = statement
        .build_query_as::<JobRow>()
        .fetch_all(&state.pool)
        .await?;
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let interaction_scope = instance_grant_scope(&state, &auth).await?;
    let items = rows
        .into_iter()
        .map(job_from_row)
        .map(|result| result.map(|job| protect_job_interaction(job, &auth, &interaction_scope)))
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| items.last().map(|job| job.id.clone()))
        .flatten();
    Ok(Json(ActivityJobsPage { items, next_cursor }))
}

fn validate_jobs_query(query: &ActivityJobsQuery) -> Result<(), AppError> {
    if query.limit == Some(0)
        || query
            .cursor
            .as_deref()
            .is_some_and(|value| Uuid::parse_str(value).is_err())
        || query
            .instance_id
            .as_deref()
            .is_some_and(|value| Uuid::parse_str(value).is_err())
        || query.state.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "queued"
                    | "running"
                    | "waiting_for_user"
                    | "succeeded"
                    | "failed"
                    | "cancelled"
                    | "interrupted"
            )
        })
    {
        return Err(AppError::BadRequest("activity.invalid_query".into()));
    }
    Ok(())
}

async fn count_jobs(
    state: &AppState,
    auth: &AuthUser,
    unrestricted: bool,
    category: &str,
    cutoff: Option<&str>,
) -> Result<i64, AppError> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM jobs j");
    if unrestricted {
        query.push(" WHERE 1 = 1");
    } else {
        query
            .push(" JOIN user_instance_grants g ON g.instance_id = j.instance_id WHERE g.user_id = ")
            .push_bind(&auth.id)
            .push(" AND (json_array_length(g.permissions) = 0 OR EXISTS (SELECT 1 FROM json_each(g.permissions) WHERE value IN ('*', 'job.read')))");
    }
    match category {
        "active" => query.push(" AND j.state IN ('queued', 'running', 'waiting_for_user')"),
        "waiting" => query.push(" AND j.state = 'waiting_for_user'"),
        "failed" => query.push(" AND j.state IN ('failed', 'interrupted')"),
        _ => return Err(AppError::Internal("invalid activity category".into())),
    };
    if let Some(cutoff) = cutoff {
        query.push(" AND j.created_at >= ").push_bind(cutoff);
    }
    query
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(Into::into)
}

async fn count_instances(
    state: &AppState,
    auth: &AuthUser,
    unrestricted: bool,
    crashed_only: bool,
) -> Result<i64, AppError> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM instances i");
    if unrestricted {
        query.push(" WHERE 1 = 1");
    } else {
        query
            .push(" JOIN user_instance_grants g ON g.instance_id = i.id WHERE g.user_id = ")
            .push_bind(&auth.id)
            .push(" AND (json_array_length(g.permissions) = 0 OR EXISTS (SELECT 1 FROM json_each(g.permissions) WHERE value IN ('*', 'server.read')))");
    }
    if crashed_only {
        query.push(" AND i.runtime_state = 'crashed'");
    }
    query
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(Into::into)
}

async fn count_config_conflicts(
    state: &AppState,
    auth: &AuthUser,
    unrestricted: bool,
) -> Result<i64, AppError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) FROM config_changes c JOIN instances i ON i.id = c.instance_id",
    );
    if unrestricted {
        query.push(" WHERE c.status = 'conflict'");
    } else {
        query
            .push(" JOIN user_instance_grants g ON g.instance_id = i.id WHERE c.status = 'conflict' AND g.user_id = ")
            .push_bind(&auth.id)
            .push(" AND (json_array_length(g.permissions) = 0 OR EXISTS (SELECT 1 FROM json_each(g.permissions) WHERE value IN ('*', 'server.read')))");
    }
    query
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_query_is_bounded_and_validated() {
        assert!(
            validate_jobs_query(&ActivityJobsQuery {
                cursor: None,
                limit: Some(50),
                state: Some("running".into()),
                instance_id: None,
            })
            .is_ok()
        );
        assert!(
            validate_jobs_query(&ActivityJobsQuery {
                cursor: Some("not-a-uuid".into()),
                limit: None,
                state: None,
                instance_id: None,
            })
            .is_err()
        );
    }
}
