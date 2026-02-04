use axum::{
    routing::get,
    extract::{Path, Query, State},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

use crate::core::AppState;
use crate::core::error::AppError;

/// A single metric data point
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MetricDataPoint {
    pub id: String,
    pub server_id: String,
    pub cpu_usage: f64,
    pub memory_bytes: i64,
    pub disk_bytes: i64,
    pub player_count: i32,
    pub recorded_at: String,
}

/// Response for metrics history
#[derive(Debug, Serialize)]
pub struct MetricsHistoryResponse {
    pub server_id: String,
    pub period: String,
    pub data: Vec<MetricDataPoint>,
}

/// Query parameters for metrics endpoint
#[derive(Debug, Deserialize)]
pub struct MetricsQuery {
    /// Period: 1h, 6h, 1d, 7d (default: 1d)
    pub period: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/:id/metrics", get(get_server_metrics))
}

/// GET /api/v1/servers/:id/metrics?period=1d
/// Returns historical metrics for a server
async fn get_server_metrics(
    State(state): State<AppState>,
    Path(server_id): Path<String>,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<MetricsHistoryResponse>, AppError> {
    let period = query.period.unwrap_or_else(|| "1d".to_string());
    
    // Calculate time threshold based on period
    let hours = match period.as_str() {
        "1h" => 1,
        "6h" => 6,
        "1d" => 24,
        "7d" => 24 * 7,
        _ => 24, // Default to 1 day
    };
    
    let threshold = Utc::now() - Duration::hours(hours);
    let threshold_str = threshold.to_rfc3339();
    
    debug!(server_id = %server_id, period = %period, threshold = %threshold_str, "Fetching metrics history");
    
    // Fetch metrics from database
    let metrics: Vec<MetricDataPoint> = sqlx::query_as(
        r#"
        SELECT id, server_id, cpu_usage, memory_bytes, disk_bytes, player_count, recorded_at
        FROM server_metrics
        WHERE server_id = ? AND recorded_at >= ?
        ORDER BY recorded_at ASC
        "#
    )
    .bind(&server_id)
    .bind(&threshold_str)
    .fetch_all(&state.pool)
    .await?;
    
    Ok(Json(MetricsHistoryResponse {
        server_id,
        period,
        data: metrics,
    }))
}

/// Insert a new metric data point (called from process manager)
pub async fn insert_metric(
    pool: &crate::core::database::DbPool,
    server_id: &str,
    cpu_usage: f64,
    memory_bytes: i64,
    disk_bytes: i64,
    player_count: i32,
) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    
    sqlx::query(
        r#"
        INSERT INTO server_metrics (id, server_id, cpu_usage, memory_bytes, disk_bytes, player_count, recorded_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&id)
    .bind(server_id)
    .bind(cpu_usage)
    .bind(memory_bytes)
    .bind(disk_bytes)
    .bind(player_count)
    .bind(&now)
    .execute(pool)
    .await?;
    
    Ok(())
}

/// Cleanup old metrics (called periodically)
pub async fn cleanup_old_metrics(pool: &crate::core::database::DbPool, retention_days: i64) -> Result<u64, sqlx::Error> {
    let threshold = Utc::now() - Duration::days(retention_days);
    let threshold_str = threshold.to_rfc3339();
    
    let result = sqlx::query("DELETE FROM server_metrics WHERE recorded_at < ?")
        .bind(&threshold_str)
        .execute(pool)
        .await?;
    
    Ok(result.rows_affected())
}
