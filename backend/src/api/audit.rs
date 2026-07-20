use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::DateTime;
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
    actor_user_id: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    from: Option<String>,
    to: Option<String>,
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
    if !matches!(auth.role.as_str(), "owner" | "admin") {
        return Err(AppError::Forbidden("auth.forbidden".into()));
    }
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
    if let Some(actor_user_id) = query.actor_user_id.as_deref() {
        statement
            .push(" AND e.actor_user_id = ")
            .push_bind(actor_user_id);
    }
    if let Some(action) = query.action.as_deref() {
        statement.push(" AND e.action = ").push_bind(action);
    }
    if let Some(outcome) = query.outcome.as_deref() {
        statement.push(" AND e.outcome = ").push_bind(outcome);
    }
    if let Some(from) = query.from.as_deref() {
        statement.push(" AND e.created_at >= ").push_bind(from);
    }
    if let Some(to) = query.to.as_deref() {
        statement.push(" AND e.created_at <= ").push_bind(to);
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
            .actor_user_id
            .as_deref()
            .is_some_and(|value| uuid::Uuid::parse_str(value).is_err())
        || query
            .action
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
        || query
            .outcome
            .as_deref()
            .is_some_and(|value| !matches!(value, "success" | "denied" | "failure"))
        || query
            .from
            .as_deref()
            .is_some_and(|value| DateTime::parse_from_rfc3339(value).is_err())
        || query
            .to
            .as_deref()
            .is_some_and(|value| DateTime::parse_from_rfc3339(value).is_err())
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
        metadata: redact_metadata(
            serde_json::from_str(&row.metadata)
                .map_err(|_| AppError::Internal("stored audit metadata is invalid".into()))?,
        ),
        created_at: row.created_at,
    })
}

fn redact_metadata(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let sensitive = [
                        "password",
                        "secret",
                        "token",
                        "authorization",
                        "cookie",
                        "content",
                        "command",
                        "user_code",
                        "verification_uri",
                    ]
                    .iter()
                    .any(|part| normalized.contains(part));
                    if sensitive {
                        (key, Value::String("[redacted]".into()))
                    } else {
                        (key, redact_metadata(value))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_metadata).collect()),
        other => other,
    }
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
                actor_user_id: None,
                action: None,
                outcome: None,
                from: None,
                to: None,
            })
            .is_err()
        );
        assert!(
            validate_query(&AuditQuery {
                before_id: None,
                limit: Some(10),
                resource_type: Some("instance".into()),
                resource_id: None,
                actor_user_id: None,
                action: None,
                outcome: Some("success".into()),
                from: None,
                to: None,
            })
            .is_ok()
        );
        assert!(
            validate_query(&AuditQuery {
                before_id: None,
                limit: Some(10),
                resource_type: None,
                resource_id: None,
                actor_user_id: None,
                action: None,
                outcome: Some("maybe".into()),
                from: None,
                to: None,
            })
            .is_err()
        );
    }

    #[test]
    fn audit_metadata_redacts_nested_sensitive_values() {
        let redacted = redact_metadata(serde_json::json!({
            "instance_id": "safe",
            "nested": {"access_token": "secret", "count": 2},
            "command": "op player"
        }));
        assert_eq!(redacted["instance_id"], "safe");
        assert_eq!(redacted["nested"]["access_token"], "[redacted]");
        assert_eq!(redacted["nested"]["count"], 2);
        assert_eq!(redacted["command"], "[redacted]");
    }
}
