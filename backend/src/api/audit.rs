use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{
    api::auth::AuthUser,
    core::{AppState, error::AppError},
};

const DEFAULT_PAGE_SIZE: u16 = 100;
const MAX_PAGE_SIZE: u16 = 200;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditQuery {
    before_id: Option<i64>,
    limit: Option<u16>,
    resource_type: Option<String>,
    resource_id: Option<String>,
    outcome: Option<String>,
}

#[derive(Debug, FromRow)]
struct AuditRow {
    id: i64,
    actor_user_id: Option<String>,
    actor_username: Option<String>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    outcome: String,
    metadata: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct AuditEvent {
    id: i64,
    actor_user_id: Option<String>,
    actor_username: Option<String>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    outcome: String,
    metadata: Value,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct AuditPage {
    items: Vec<AuditEvent>,
    next_before_id: Option<i64>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/audit", get(list))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditPage>, AppError> {
    auth.require("audit.read")?;
    validate_query(&query)?;
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);

    let mut statement = QueryBuilder::<Sqlite>::new(
        "SELECT e.id, e.actor_user_id, u.username AS actor_username, e.action, \
         e.resource_type, e.resource_id, e.outcome, e.metadata, e.created_at \
         FROM audit_events e LEFT JOIN users u ON u.id = e.actor_user_id WHERE 1 = 1",
    );
    if let Some(before_id) = query.before_id {
        statement.push(" AND e.id < ").push_bind(before_id);
    }
    if let Some(resource_type) = query.resource_type.as_deref() {
        statement
            .push(" AND e.resource_type = ")
            .push_bind(resource_type);
    }
    if let Some(resource_id) = query.resource_id.as_deref() {
        statement
            .push(" AND e.resource_id = ")
            .push_bind(resource_id);
    }
    if let Some(outcome) = query.outcome.as_deref() {
        statement.push(" AND e.outcome = ").push_bind(outcome);
    }
    statement
        .push(" ORDER BY e.id DESC LIMIT ")
        .push_bind(i64::from(limit) + 1);

    let mut rows = statement
        .build_query_as::<AuditRow>()
        .fetch_all(&state.pool)
        .await?;
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(event_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let next_before_id = if has_more {
        items.last().map(|event| event.id)
    } else {
        None
    };

    Ok(Json(AuditPage {
        items,
        next_before_id,
    }))
}

fn validate_query(query: &AuditQuery) -> Result<(), AppError> {
    if query.before_id.is_some_and(|value| value <= 0)
        || query.limit == Some(0)
        || query
            .resource_type
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 64)
        || query
            .resource_id
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
        || query
            .outcome
            .as_deref()
            .is_some_and(|value| !matches!(value, "success" | "denied" | "failure"))
    {
        return Err(AppError::BadRequest("audit.invalid_query".into()));
    }
    Ok(())
}

fn event_from_row(row: AuditRow) -> Result<AuditEvent, AppError> {
    Ok(AuditEvent {
        id: row.id,
        actor_user_id: row.actor_user_id,
        actor_username: row.actor_username,
        action: row.action,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        outcome: row.outcome,
        metadata: serde_json::from_str(&row.metadata)
            .map_err(|_| AppError::Internal("stored audit metadata is invalid".into()))?,
        created_at: row.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_validation_rejects_unbounded_or_unknown_outcomes() {
        assert!(
            validate_query(&AuditQuery {
                before_id: Some(0),
                limit: None,
                resource_type: None,
                resource_id: None,
                outcome: None,
            })
            .is_err()
        );
        assert!(
            validate_query(&AuditQuery {
                before_id: None,
                limit: Some(10),
                resource_type: Some("instance".into()),
                resource_id: None,
                outcome: Some("success".into()),
            })
            .is_ok()
        );
        assert!(
            validate_query(&AuditQuery {
                before_id: None,
                limit: Some(10),
                resource_type: None,
                resource_id: None,
                outcome: Some("maybe".into()),
            })
            .is_err()
        );
    }
}
