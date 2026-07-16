use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, post, put},
};
use serde::Deserialize;

use crate::{
    api::{SuccessResponse, auth::AuthUser},
    core::{AppState, database, error::AppError},
    domain::v1::{GameProfile, SteamExecutable, SteamProfile, SteamStopStrategy},
    services::{catalog, installers, profiles::build_local_steam_profile},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/game-profiles", get(list))
        .route("/game-profiles/{id}/revisions", get(list_revisions))
        .route("/game-profiles/{id}/version-catalog", get(version_catalog))
        .route("/game-profiles/steam", post(create_steam))
        .route(
            "/game-profiles/steam/{id}",
            put(revise_steam).delete(remove_steam),
        )
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionCatalogQuery {
    game_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSteamProfileRequest {
    id: String,
    definition: SteamProfileDefinition,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SteamProfileDefinition {
    name: String,
    description: String,
    app_id: u32,
    #[serde(default)]
    branch: Option<String>,
    executable: SteamExecutable,
    #[serde(default)]
    arguments: Vec<String>,
    ports: Vec<crate::domain::v1::PortSpec>,
    save_paths: Vec<String>,
    #[serde(default)]
    ready_log_pattern: Option<String>,
    stop_strategy: SteamStopStrategy,
}

impl SteamProfileDefinition {
    fn build(self, id: String, revision: u32) -> Result<GameProfile, AppError> {
        build_local_steam_profile(
            id,
            revision,
            self.name,
            self.description,
            SteamProfile {
                app_id: self.app_id,
                branch: self.branch,
                executable: self.executable,
                arguments: self.arguments,
                ports: self.ports,
                save_paths: self.save_paths,
                ready_log_pattern: self.ready_log_pattern,
                stop_strategy: self.stop_strategy,
            },
        )
    }
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GameProfile>>, AppError> {
    require_profile_read(&auth)?;
    Ok(Json(state.profiles.all()))
}

async fn list_revisions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<GameProfile>>, AppError> {
    require_profile_read(&auth)?;
    let revisions = state.profiles.revisions(&id);
    if revisions.is_empty() {
        return Err(AppError::NotFound("profiles.not_found".into()));
    }
    Ok(Json(revisions))
}

async fn version_catalog(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(query): Query<VersionCatalogQuery>,
) -> Result<Json<installers::ProfileVersionCatalog>, AppError> {
    require_profile_read(&auth)?;
    if state.profiles.get(&id).is_none() {
        return Err(AppError::NotFound("profiles.not_found".into()));
    }
    let catalog = installers::profile_version_catalog(&id, query.game_version.as_deref())
        .await
        .map_err(|error| {
            tracing::warn!(
                profile_id = %id,
                code = error.code,
                detail = ?error.internal,
                "game profile version catalog failed"
            );
            AppError::Internal(error.client_message.into())
        })?;
    Ok(Json(catalog))
}

async fn create_steam(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateSteamProfileRequest>,
) -> Result<(StatusCode, HeaderMap, Json<GameProfile>), AppError> {
    auth.require("profile.manage")?;
    if catalog::profile_is_catalog_managed(&state.pool, &body.id).await? {
        return Err(AppError::Conflict("profiles.catalog_managed".into()));
    }
    if state.profiles.get(&body.id).is_some() {
        return Err(AppError::Conflict("profiles.id_exists".into()));
    }
    let profile = body.definition.build(body.id, 1)?;
    persist_custom_revision(&state, &auth, &profile).await?;
    Ok((
        StatusCode::CREATED,
        revision_headers(profile.revision)?,
        Json(profile),
    ))
}

async fn revise_steam(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SteamProfileDefinition>,
) -> Result<(StatusCode, HeaderMap, Json<GameProfile>), AppError> {
    auth.require("profile.manage")?;
    if catalog::profile_is_catalog_managed(&state.pool, &id).await? {
        return Err(AppError::Conflict("profiles.catalog_managed".into()));
    }
    let expected = parse_if_match(&headers)?;
    let current = state
        .profiles
        .get(&id)
        .ok_or_else(|| AppError::NotFound("profiles.not_found".into()))?;
    if current.kind != crate::domain::v1::ProfileKind::SteamCustom {
        return Err(AppError::Forbidden("profiles.builtin_immutable".into()));
    }
    if current.revision != expected {
        return Err(AppError::Conflict("profiles.version_conflict".into()));
    }
    let revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("profiles.revision_exhausted".into()))?;
    let profile = body.build(id, revision)?;
    if profile
        .steam_profile
        .as_ref()
        .zip(current.steam_profile.as_ref())
        .is_none_or(|(next, previous)| next.app_id != previous.app_id)
    {
        return Err(AppError::BadRequest(
            "profiles.steam.app_id_is_immutable".into(),
        ));
    }
    persist_custom_revision(&state, &auth, &profile).await?;
    Ok((
        StatusCode::CREATED,
        revision_headers(profile.revision)?,
        Json(profile),
    ))
}

async fn remove_steam(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    auth.require("profile.manage")?;
    if catalog::profile_is_catalog_managed(&state.pool, &id).await? {
        return Err(AppError::Conflict("profiles.catalog_managed".into()));
    }
    let profile = state
        .profiles
        .get(&id)
        .ok_or_else(|| AppError::NotFound("profiles.not_found".into()))?;
    if profile.kind != crate::domain::v1::ProfileKind::SteamCustom {
        return Err(AppError::Forbidden("profiles.builtin_immutable".into()));
    }
    let references: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE profile_id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    if references != 0 {
        return Err(AppError::Conflict("profiles.in_use".into()));
    }
    let removed = sqlx::query("DELETE FROM game_profiles WHERE id = ? AND kind = 'steam_custom'")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if removed.rows_affected() == 0 {
        return Err(AppError::NotFound("profiles.not_found".into()));
    }
    state.profiles.unregister_custom(&id);
    database::audit(
        &state.pool,
        Some(&auth.id),
        "profile.deleted",
        "game_profile",
        Some(&id),
        "success",
        serde_json::json!({"revision_count": removed.rows_affected()}),
    )
    .await?;
    state.events.publish(
        "profile.deleted",
        None,
        serde_json::json!({"profile_id": id}),
    );
    Ok(SuccessResponse::with_message("profiles.deleted"))
}

async fn persist_custom_revision(
    state: &AppState,
    auth: &AuthUser,
    profile: &GameProfile,
) -> Result<(), AppError> {
    let manifest =
        serde_json::to_string(profile).map_err(|error| AppError::Internal(error.to_string()))?;
    let inserted = sqlx::query(
        "INSERT INTO game_profiles (id, revision, kind, manifest, created_by, created_at) \
         VALUES (?, ?, 'steam_custom', ?, ?, ?)",
    )
    .bind(&profile.id)
    .bind(profile.revision)
    .bind(manifest)
    .bind(&auth.id)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&state.pool)
    .await;
    if let Err(error) = inserted {
        if matches!(error, sqlx::Error::Database(ref database) if database.is_unique_violation()) {
            return Err(AppError::Conflict("profiles.version_conflict".into()));
        }
        return Err(error.into());
    }
    state.profiles.register(profile.clone());
    database::audit(
        &state.pool,
        Some(&auth.id),
        "profile.revision_created",
        "game_profile",
        Some(&profile.id),
        "success",
        serde_json::json!({"revision": profile.revision, "app_id": profile.steam_profile.as_ref().map(|steam| steam.app_id)}),
    )
    .await?;
    state.events.publish(
        "profile.revision_created",
        None,
        serde_json::json!({"profile_id": profile.id, "revision": profile.revision}),
    );
    Ok(())
}

fn parse_if_match(headers: &HeaderMap) -> Result<u32, AppError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().trim_start_matches("W/").trim_matches('"'))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| AppError::PreconditionRequired("profiles.if_match_required".into()))
}

fn revision_headers(revision: u32) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{revision}\""))
            .map_err(|error| AppError::Internal(error.to_string()))?,
    );
    Ok(headers)
}

fn require_profile_read(auth: &AuthUser) -> Result<(), AppError> {
    if auth.has_permission("profile.read") || auth.has_permission("profile.manage") {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.permission_denied".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_revisions_require_a_numeric_entity_tag() {
        assert!(parse_if_match(&HeaderMap::new()).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"4\""));
        assert_eq!(parse_if_match(&headers).unwrap(), 4);
    }
}
