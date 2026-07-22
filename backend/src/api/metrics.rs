use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    api::auth::{AuthUser, authorize_instance, instance_grant_scope},
    core::{AppState, error::AppError},
    services::metrics::SystemMetricsSnapshot,
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

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CurrentServerMetric {
    pub server_id: String,
    pub cpu_usage: f64,
    pub memory_bytes: i64,
    pub disk_bytes: i64,
    pub uptime_seconds: i64,
    pub player_count: Option<i64>,
    pub recorded_at: String,
}

#[derive(Debug, Serialize)]
pub struct CurrentServerMetrics {
    pub items: Vec<CurrentServerMetric>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/metrics", get(history))
        .route("/metrics/current", get(current))
        .route("/metrics/system", get(system))
}

async fn system(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<SystemMetricsSnapshot>, AppError> {
    auth.require("server.read")?;
    Ok(Json(state.system_metrics.current().await))
}

async fn current(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<CurrentServerMetrics>, AppError> {
    auth.require("server.read")?;
    let scope = instance_grant_scope(&state, &auth).await?;
    let rows: Vec<CurrentServerMetric> = sqlx::query_as(
        r#"
        SELECT metric.server_id, metric.cpu_usage, metric.memory_bytes,
               metric.disk_bytes, metric.uptime_seconds, metric.player_count,
               metric.recorded_at
        FROM instances AS instance
        JOIN server_metrics AS metric ON metric.id = (
            SELECT candidate.id
            FROM server_metrics AS candidate
            WHERE candidate.server_id = instance.id
            ORDER BY candidate.recorded_at DESC, candidate.id DESC
            LIMIT 1
        )
        ORDER BY metric.server_id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(CurrentServerMetrics {
        items: rows
            .into_iter()
            .filter(|metric| scope.allows(&auth, &metric.server_id, "server.read"))
            .collect(),
    }))
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
