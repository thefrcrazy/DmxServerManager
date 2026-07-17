use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    api::auth::{AuthUser, authorize_instance},
    core::{AppState, error::AppError},
};

const MAX_POINTS: i64 = 10_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsQuery {
    #[serde(default = "default_period")]
    period: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MetricPoint {
    pub id: String,
    pub cpu_usage: f64,
    pub memory_bytes: i64,
    pub disk_bytes: i64,
    pub uptime_seconds: i64,
    pub player_count: Option<i64>,
    pub recorded_at: String,
}

#[derive(Debug, Serialize)]
struct MetricsHistory {
    server_id: String,
    period: String,
    points: Vec<MetricPoint>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/servers/{id}/metrics", get(history))
}

async fn history(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(server_id): Path<String>,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<MetricsHistory>, AppError> {
    uuid::Uuid::parse_str(&server_id)
        .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    authorize_instance(&state, &auth, &server_id, "server.read").await?;
    let hours = match query.period.as_str() {
        "1h" => 1,
        "6h" => 6,
        "1d" => 24,
        "7d" => 24 * 7,
        _ => return Err(AppError::BadRequest("metrics.invalid_period".into())),
    };
    let threshold = (Utc::now() - Duration::hours(hours)).to_rfc3339();
    let mut points: Vec<MetricPoint> = sqlx::query_as(
        r#"
        SELECT id, cpu_usage, memory_bytes, disk_bytes, uptime_seconds, player_count, recorded_at
        FROM server_metrics
        WHERE server_id = ? AND recorded_at >= ?
        ORDER BY recorded_at DESC
        LIMIT ?
        "#,
    )
    .bind(&server_id)
    .bind(threshold)
    .bind(MAX_POINTS)
    .fetch_all(&state.pool)
    .await?;
    points.reverse();
    Ok(Json(MetricsHistory {
        server_id,
        period: query.period,
        points,
    }))
}

fn default_period() -> String {
    "1d".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metrics_period_is_bounded() {
        assert_eq!(default_period(), "1d");
        assert_eq!(MAX_POINTS, 10_000);
    }
}
