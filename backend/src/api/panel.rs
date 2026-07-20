use std::net::IpAddr;

use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    api::auth::AuthUser,
    core::{AppState, DbPool, database, error::AppError},
};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct NetworkSettings {
    pub advertised_game_host: Option<String>,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateNetworkSettings {
    pub advertised_game_host: Option<String>,
    pub expected_version: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/panel/network", get(get_network).put(update_network))
}

async fn get_network(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<NetworkSettings>, AppError> {
    require_network_manager(&auth)?;
    fetch_network_settings(&state.pool).await.map(Json)
}

async fn update_network(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<UpdateNetworkSettings>,
) -> Result<Json<NetworkSettings>, AppError> {
    require_network_manager(&auth)?;
    if body.expected_version <= 0 {
        return Err(AppError::BadRequest("panel.invalid_version".into()));
    }
    let host = normalize_advertised_host(body.advertised_game_host.as_deref())?;
    let now = chrono::Utc::now().to_rfc3339();
    let updated = sqlx::query(
        "UPDATE panel_settings SET advertised_game_host = ?, version = version + 1, \
         updated_by = ?, updated_at = ? WHERE singleton = 1 AND version = ?",
    )
    .bind(host.as_deref())
    .bind(&auth.id)
    .bind(&now)
    .bind(body.expected_version)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict("panel.version_conflict".into()));
    }
    database::audit(
        &state.pool,
        Some(&auth.id),
        "panel.network_updated",
        "panel_settings",
        Some("network"),
        "success",
        serde_json::json!({"configured": host.is_some()}),
    )
    .await?;
    fetch_network_settings(&state.pool).await.map(Json)
}

fn require_network_manager(auth: &AuthUser) -> Result<(), AppError> {
    auth.require("panel.network.manage")?;
    if matches!(auth.role.as_str(), "owner" | "admin") {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.forbidden".into()))
    }
}

pub(crate) async fn fetch_network_settings(pool: &DbPool) -> Result<NetworkSettings, AppError> {
    sqlx::query_as(
        "SELECT advertised_game_host, version, updated_at FROM panel_settings WHERE singleton = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub(crate) fn normalize_advertised_host(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 253
        || value.contains("://")
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || value.contains(['/', '?', '#', '@', '[', ']', '%'])
    {
        return Err(AppError::BadRequest("panel.invalid_advertised_host".into()));
    }
    if let Ok(address) = value.parse::<IpAddr>() {
        return Ok(Some(address.to_string()));
    }

    let dns = value.trim_end_matches('.').to_ascii_lowercase();
    let valid = !dns.is_empty()
        && dns.len() <= 253
        && dns.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    if !valid {
        return Err(AppError::BadRequest("panel.invalid_advertised_host".into()));
    }
    Ok(Some(dns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_host_accepts_dns_ipv4_and_ipv6() {
        assert_eq!(
            normalize_advertised_host(Some(" Game.EXAMPLE.com. ")).unwrap(),
            Some("game.example.com".into())
        );
        assert_eq!(
            normalize_advertised_host(Some("192.0.2.4")).unwrap(),
            Some("192.0.2.4".into())
        );
        assert_eq!(
            normalize_advertised_host(Some("2001:db8::1")).unwrap(),
            Some("2001:db8::1".into())
        );
        assert_eq!(normalize_advertised_host(Some(" ")).unwrap(), None);
    }

    #[test]
    fn advertised_host_rejects_urls_ports_and_malformed_names() {
        for value in [
            "https://game.example.com",
            "game.example.com:5520",
            "bad label.example",
            "-bad.example",
            "[2001:db8::1]",
        ] {
            assert!(normalize_advertised_host(Some(value)).is_err(), "{value}");
        }
    }
}
