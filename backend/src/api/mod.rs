use axum::{Json, Router, extract::State, http::StatusCode, middleware, routing::get};
use serde::Serialize;

use crate::core::AppState;

pub mod administration;
pub mod audit;
pub mod auth;
pub mod backups;
pub mod catalog;
pub mod chat;
pub mod config_files;
pub mod events;
pub mod files;
pub mod game_profiles;
pub mod jobs;
pub mod metrics;
pub mod mods;
pub mod notifications;
pub mod openapi;
pub mod players;
pub mod releases;
pub mod schedules;
pub mod servers;
pub mod webhooks;

#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessResponse {
    pub fn ok() -> Json<Self> {
        Json(Self {
            success: true,
            message: None,
        })
    }

    pub fn with_message(message: impl Into<String>) -> Json<Self> {
        Json(Self {
            success: true,
            message: Some(message.into()),
        })
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

pub fn routes(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/openapi.json", get(openapi::get_document))
        .merge(game_profiles::routes())
        .merge(administration::routes())
        .merge(audit::routes())
        .merge(backups::routes())
        .merge(catalog::routes())
        .merge(chat::routes())
        .merge(config_files::routes())
        .merge(files::routes())
        .merge(schedules::routes())
        .merge(servers::routes())
        .merge(jobs::routes())
        .merge(metrics::routes())
        .merge(mods::routes())
        .merge(notifications::routes())
        .merge(players::routes())
        .merge(releases::routes())
        .merge(webhooks::routes())
        .route("/events", get(events::stream_events))
        .route_layer(middleware::from_fn(auth::require_password_change_completed))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::csrf_middleware,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    Router::new()
        .route("/health", get(health))
        .nest("/auth", auth::routes(state))
        .merge(protected)
}

async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<HealthResponse>)> {
    if sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_err()
    {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "unavailable",
                service: "dmx-server-manager",
                version: env!("CARGO_PKG_VERSION"),
            }),
        ));
    }
    Ok(Json(HealthResponse {
        status: "ok",
        service: "dmx-server-manager",
        version: env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        Router,
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Request, StatusCode, header},
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;

    use crate::{
        core::{AppState, Settings, config::DeploymentMode, database, events::EventHub},
        services::{
            profiles::ProfileRegistry, releases::ReleaseMonitor, runtime::RuntimeManager,
            secrets::SecretStore,
        },
    };

    async fn test_state() -> AppState {
        let root = tempfile::tempdir().unwrap().keep();
        let database_url = format!("sqlite:{}/test.db?mode=rwc", root.display());
        let settings = Settings {
            config_file: root.join("config.toml"),
            data_dir: root.clone(),
            static_dir: root.join("static"),
            bind: SocketAddr::from(([127, 0, 0, 1], 5500)),
            database_url: database_url.clone(),
            master_key_file: root.join("master.key"),
            steamcmd_path: root.join("missing-steamcmd"),
            bedrock_linux_source: None,
            bedrock_windows_source: None,
            import_roots: Vec::new(),
            trusted_proxies: Vec::new(),
            reverse_proxy: false,
            log: "error".into(),
            dev_origin: None,
            setup_token: Some("test-setup-token".into()),
            session_ttl_hours: 24,
            deployment_mode: DeploymentMode::Native,
            release_check: None,
        };
        tokio::fs::create_dir_all(settings.instances_dir())
            .await
            .unwrap();
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let profiles = Arc::new(ProfileRegistry::builtins());
        profiles.persist_builtins(&pool).await.unwrap();
        let secrets = SecretStore::load_or_create(&settings.master_key_file).unwrap();
        let settings = Arc::new(settings);
        let events = EventHub::new(32);
        let runtime = RuntimeManager::new(
            pool.clone(),
            settings.clone(),
            events.clone(),
            secrets.clone(),
        );
        let releases = ReleaseMonitor::new(settings.clone()).unwrap();
        AppState {
            pool,
            settings,
            profiles,
            events,
            secrets,
            runtime,
            releases,
        }
    }

    async fn authenticated_state() -> (AppState, String, String) {
        let state = test_state().await;
        let user_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        let session_token = "test-session-token".to_string();
        let csrf_token = "test-csrf-token".to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) VALUES (?, 'owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(token_hash(&session_token))
        .bind(token_hash(&csrf_token))
        .bind((Utc::now() + Duration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        (state, session_token, csrf_token)
    }

    fn token_hash(value: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
    }

    fn api(state: AppState) -> Router {
        Router::new()
            .nest("/api/v1", super::routes(state.clone()))
            .with_state(state)
    }

    #[tokio::test]
    async fn protected_routes_require_a_session_and_csrf_for_mutations() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/game-profiles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/game-profiles")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let create_body = serde_json::json!({
            "name": "Hytale test",
            "profile_id": "hytale",
            "settings": {}
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/releases/panel")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/releases/panel/check")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = router
            .oneshot(
                Request::post("/api/v1/releases/panel/check")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn password_change_required_session_is_limited_to_auth_recovery_routes() {
        let (state, session, _) = authenticated_state().await;
        let temporary_password = "Temporary-Owner-2026!";
        sqlx::query(
            "UPDATE users SET password_hash = ?, must_change_password = 1 WHERE username = 'owner'",
        )
        .bind(super::auth::hash_password(temporary_password).unwrap())
        .execute(&state.pool)
        .await
        .unwrap();
        let router = api(state.clone());

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/game-profiles")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(problem["title"], "auth.password_change_required");
        assert_eq!(problem["code"], "AUTH_009");

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/auth/me")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let me: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(me["user"]["must_change_password"], true);
        let csrf = me["csrf_token"].as_str().unwrap();

        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "Forbidden while password is temporary",
                            "profile_id": "hytale",
                            "settings": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let unchanged = serde_json::json!({
            "current_password": temporary_password,
            "new_password": temporary_password
        });
        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/auth/password")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(unchanged.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let changed = serde_json::json!({
            "current_password": temporary_password,
            "new_password": "Permanent-Owner-2026!"
        });
        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/auth/password")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(changed.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        let must_change: bool =
            sqlx::query_scalar("SELECT must_change_password FROM users WHERE username = 'owner'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert!(!must_change);
        assert_eq!(sessions, 0);
    }

    #[tokio::test]
    async fn password_change_required_session_can_logout() {
        let (state, session, csrf) = authenticated_state().await;
        sqlx::query("UPDATE users SET must_change_password = 1 WHERE username = 'owner'")
            .execute(&state.pool)
            .await
            .unwrap();

        let response = api(state.clone())
            .oneshot(
                Request::post("/api/v1/auth/logout")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(sessions, 0);
    }

    #[tokio::test]
    async fn active_theme_is_authenticated_versioned_and_owner_managed() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());

        let response = router
            .clone()
            .oneshot(
                Request::get("/api/v1/catalog/theme")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::ETAG).unwrap(), "\"1\"");

        let selection = serde_json::json!({"kind": "default"}).to_string();
        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/catalog/theme")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header(header::IF_MATCH, "\"1\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(selection.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/catalog/theme")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(selection.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PRECONDITION_REQUIRED);

        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/catalog/theme")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::IF_MATCH, "\"1\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(selection.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::ETAG).unwrap(), "\"2\"");
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let theme: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(theme["selection"], serde_json::json!({"kind": "default"}));
        assert!(theme.get("css").is_none());

        sqlx::query("UPDATE users SET role_id = 'admin' WHERE username = 'owner'")
            .execute(&state.pool)
            .await
            .unwrap();
        let response = router
            .oneshot(
                Request::put("/api/v1/catalog/theme")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::IF_MATCH, "\"2\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(selection))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn every_private_api_family_rejects_an_anonymous_request() {
        let state = test_state().await;
        let router = api(state);
        let instance_id = uuid::Uuid::new_v4();
        let paths = [
            "/api/v1/openapi.json".to_string(),
            "/api/v1/game-profiles".to_string(),
            "/api/v1/permissions".to_string(),
            "/api/v1/roles".to_string(),
            "/api/v1/users".to_string(),
            "/api/v1/audit".to_string(),
            "/api/v1/backups".to_string(),
            "/api/v1/catalog".to_string(),
            "/api/v1/catalog/theme".to_string(),
            "/api/v1/chat".to_string(),
            "/api/v1/files".to_string(),
            "/api/v1/schedules".to_string(),
            "/api/v1/servers".to_string(),
            "/api/v1/jobs".to_string(),
            "/api/v1/notifications".to_string(),
            "/api/v1/releases/panel".to_string(),
            "/api/v1/webhooks".to_string(),
            "/api/v1/events".to_string(),
            format!("/api/v1/servers/{instance_id}/metrics"),
            format!("/api/v1/servers/{instance_id}/mods"),
            format!("/api/v1/servers/{instance_id}/secrets"),
        ];
        for path in paths {
            let response = router
                .clone()
                .oneshot(Request::get(&path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "private route {path} did not reject an anonymous request"
            );
        }
    }

    #[tokio::test]
    async fn panel_release_metadata_and_manual_checks_are_owner_only() {
        let (state, session, csrf) = authenticated_state().await;
        sqlx::query("UPDATE users SET role_id = 'admin' WHERE username = 'owner'")
            .execute(&state.pool)
            .await
            .unwrap();
        let router = api(state);

        let status = router
            .clone()
            .oneshot(
                Request::get("/api/v1/releases/panel")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::FORBIDDEN);

        let check = router
            .oneshot(
                Request::post("/api/v1/releases/panel/check")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(check.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn discord_webhook_urls_are_encrypted_and_never_returned() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let plaintext = "https://discord.com/api/webhooks/123456789012345678/abcdefghijklmnopqrstuvwxyzABCDEF0123456789";
        let body = serde_json::json!({
            "name": "Operations",
            "url": plaintext,
            "events": ["server.started", "server.crashed"],
            "enabled": true
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/webhooks")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let response_body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&response_body).contains(plaintext));

        let stored: (String, String) =
            sqlx::query_as("SELECT url_nonce, url_ciphertext FROM discord_webhooks LIMIT 1")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(!stored.0.contains(plaintext));
        assert!(!stored.1.contains(plaintext));

        let response = router
            .oneshot(
                Request::get("/api/v1/webhooks")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response_body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let response_text = String::from_utf8_lossy(&response_body);
        assert!(!response_text.contains(plaintext));
        assert!(response_text.contains("\"configured\":true"));
    }

    #[tokio::test]
    async fn restart_requeues_only_idempotent_install_jobs() {
        let (state, _, _) = authenticated_state().await;
        let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'owner'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        let now = Utc::now().to_rfc3339();
        let install_instance = uuid::Uuid::new_v4().to_string();
        let backup_instance = uuid::Uuid::new_v4().to_string();
        for (id, name) in [
            (&install_instance, "install-recovery"),
            (&backup_instance, "backup-recovery"),
        ] {
            sqlx::query(
                "INSERT INTO instances (id, name, profile_id, profile_revision, settings, created_at, updated_at) \
                 VALUES (?, ?, 'hytale', 2, '{}', ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(&now)
            .bind(&now)
            .execute(&state.pool)
            .await
            .unwrap();
        }
        let install_job = uuid::Uuid::new_v4().to_string();
        let backup_job = uuid::Uuid::new_v4().to_string();
        for (job_id, instance_id, kind, job_state) in [
            (
                &install_job,
                &install_instance,
                "install",
                "waiting_for_user",
            ),
            (&backup_job, &backup_instance, "backup", "running"),
        ] {
            sqlx::query(
                "INSERT INTO jobs (id, instance_id, kind, state, progress, requested_by, created_at, started_at) \
                 VALUES (?, ?, ?, ?, 50, ?, ?, ?)",
            )
            .bind(job_id)
            .bind(instance_id)
            .bind(kind)
            .bind(job_state)
            .bind(&user_id)
            .bind(&now)
            .bind(&now)
            .execute(&state.pool)
            .await
            .unwrap();
        }

        database::run_migrations(&state.pool).await.unwrap();
        let install: (String, i64, Option<String>) =
            sqlx::query_as("SELECT state, progress, started_at FROM jobs WHERE id = ?")
                .bind(install_job)
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(install, ("queued".into(), 0, None));
        let backup: (String, Option<String>) =
            sqlx::query_as("SELECT state, error_code FROM jobs WHERE id = ?")
                .bind(backup_job)
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(backup.0, "interrupted");
        assert_eq!(backup.1.as_deref(), Some("manager_restarted"));
    }

    #[tokio::test]
    async fn profile_secrets_are_encrypted_and_never_part_of_instance_settings() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let plaintext = "VerySecret42!";
        let body = serde_json::json!({
            "name": "Valheim test",
            "profile_id": "valheim",
            "settings": {"server_name": "Test", "world_name": "World"},
            "secrets": {"server_password": plaintext}
        });
        let response = router
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let (ciphertext, settings): (String, String) = sqlx::query_as(
            "SELECT s.ciphertext, i.settings FROM instance_secrets s JOIN instances i ON i.id = s.instance_id WHERE s.name = 'server_password'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert!(!ciphertext.contains(plaintext));
        assert!(!settings.contains(plaintext));
        assert!(!settings.contains("server_password"));
    }

    #[tokio::test]
    async fn profile_secret_schema_is_enforced_before_instance_creation() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let response = router
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "Invalid Valheim secret",
                            "profile_id": "valheim",
                            "settings": {"server_name": "Test", "world_name": "World"},
                            "secrets": {"server_password": "four"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM instances")
                .fetch_one(&state.pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn steam_profiles_are_versioned_and_instances_pin_the_selected_revision() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let definition = serde_json::json!({
            "name": "Fixture Dedicated Server",
            "description": "Anonymous SteamCMD fixture",
            "app_id": 90,
            "executable": {
                "linux_x86_64": "server",
                "windows_x86_64": "server.exe"
            },
            "arguments": ["--port", "{{port:game_port}}"],
            "ports": [{"name": "game_port", "protocol": "udp", "default": 27015}],
            "save_paths": ["saves"],
            "ready_log_pattern": "Ready",
            "stop_strategy": {"kind": "terminate", "timeout_seconds": 30}
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/game-profiles/steam")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "steam-fixture",
                            "definition": definition,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get(header::ETAG).unwrap(), "\"1\"");

        let create_instance = |name: &str| {
            Request::post("/api/v1/servers")
                .header(header::COOKIE, format!("dmx_session={session}"))
                .header("x-csrf-token", &csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": name,
                        "profile_id": "steam-fixture",
                        "settings": {"game_port": if name == "first" {27015} else {27016}}
                    })
                    .to_string(),
                ))
                .unwrap()
        };
        let response = router
            .clone()
            .oneshot(create_instance("first"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let mut revised = definition;
        revised["branch"] = serde_json::Value::String("public".into());
        let response = router
            .clone()
            .oneshot(
                Request::put("/api/v1/game-profiles/steam/steam-fixture")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::IF_MATCH, "\"1\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(revised.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get(header::ETAG).unwrap(), "\"2\"");

        let response = router.oneshot(create_instance("second")).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let rows: Vec<(String, i64, String)> = sqlx::query_as(
            "SELECT name, profile_revision, settings FROM instances \
             WHERE profile_id = 'steam-fixture' ORDER BY name",
        )
        .fetch_all(&state.pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[1].1, 2);
        assert!(!rows[0].2.contains("executable"));
        assert!(!rows[1].2.contains("app_id"));
    }

    #[tokio::test]
    async fn actions_return_persistent_idempotent_jobs_and_record_install_failures() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let create = serde_json::json!({
            "name": "Unavailable SteamCMD test",
            "profile_id": "valheim",
            "settings": {
                "server_name": "Test",
                "world_name": "World"
            },
            "secrets": {"server_password": "secret42"}
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let instance: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = instance["id"].as_str().unwrap();

        let submit = || {
            Request::post(format!("/api/v1/servers/{id}/actions/install"))
                .header(header::COOKIE, format!("dmx_session={session}"))
                .header("x-csrf-token", &csrf)
                .header("idempotency-key", "unsupported-install")
                .body(Body::empty())
                .unwrap()
        };
        let response = router.clone().oneshot(submit()).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = job["id"].as_str().unwrap().to_string();

        let duplicate = router.oneshot(submit()).await.unwrap();
        assert_eq!(duplicate.status(), StatusCode::ACCEPTED);
        let body = to_bytes(duplicate.into_body(), 64 * 1024).await.unwrap();
        let duplicate_job: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(duplicate_job["id"], job_id);

        for _ in 0..100 {
            let row: (String, Option<String>) =
                sqlx::query_as("SELECT state, error_code FROM jobs WHERE id = ?")
                    .bind(&job_id)
                    .fetch_one(&state.pool)
                    .await
                    .unwrap();
            if row.0 == "failed" {
                assert_eq!(row.1.as_deref(), Some("steamcmd_unavailable"));
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("failed installation job did not finish");
    }

    #[tokio::test]
    async fn explicit_minecraft_version_change_forces_a_reinstall() {
        let (state, session, csrf) = authenticated_state().await;
        let router = api(state.clone());
        let create = serde_json::json!({
            "name": "Pinned Minecraft",
            "profile_id": "minecraft-java-vanilla",
            "settings": {"version": "1.21.4", "eula_accepted": true}
        });
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/servers")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let instance: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = instance["id"].as_str().unwrap();
        sqlx::query(
            "UPDATE instances SET installation_state = 'installed', installed_version = '1.21.4' WHERE id = ?",
        )
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();

        let update = serde_json::json!({
            "settings": {"version": "1.21.5", "eula_accepted": true}
        });
        let response = router
            .oneshot(
                Request::patch(format!("/api/v1/servers/{id}"))
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::IF_MATCH, "\"1\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["installation_state"], "not_installed");
        assert!(updated["installed_version"].is_null());
        assert_eq!(updated["settings"]["version"], "1.21.5");
    }

    #[tokio::test]
    async fn additive_hytale_settings_upgrade_preserves_the_installed_game() {
        let (state, session, csrf) = authenticated_state().await;
        let mut previous = state.profiles.get("hytale").unwrap();
        assert_eq!(previous.revision, 2);
        previous.revision = 1;
        previous.ui_schema = serde_json::json!({"layout": "sections"});
        let properties = previous
            .settings_schema
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        for setting in [
            "allow_op",
            "disable_sentry",
            "accept_early_plugins",
            "automatic_backups",
            "backup_frequency_minutes",
        ] {
            properties.remove(setting);
        }
        let previous_manifest = serde_json::to_string(&previous).unwrap();
        state.profiles.register(previous);

        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO game_profiles (id, revision, kind, manifest, created_at) \
             VALUES ('hytale', 1, 'builtin', ?, ?)",
        )
        .bind(previous_manifest)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, installation_state, \
              installed_version, runtime_state, desired_state, created_at, updated_at) \
             VALUES (?, 'Hytale revision upgrade', 'hytale', 1, ?, 'installed', \
                     '2026.07', 'stopped', 'stopped', ?, ?)",
        )
        .bind(&instance_id)
        .bind(
            serde_json::json!({
                "port": 5520,
                "max_memory_mb": 8192,
                "auth_mode": "authenticated"
            })
            .to_string(),
        )
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        tokio::fs::create_dir_all(state.settings.instances_dir().join(&instance_id))
            .await
            .unwrap();

        let update = serde_json::json!({
            "settings": {
                "port": 5520,
                "max_memory_mb": 8192,
                "auth_mode": "authenticated",
                "allow_op": true,
                "automatic_backups": true,
                "backup_frequency_minutes": 30
            }
        });
        let response = api(state.clone())
            .oneshot(
                Request::patch(format!("/api/v1/servers/{instance_id}"))
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header(header::IF_MATCH, "\"1\"")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["profile_revision"], 2);
        assert_eq!(updated["installation_state"], "installed");
        assert_eq!(updated["installed_version"], "2026.07");
        assert_eq!(updated["settings"]["allow_op"], true);
        assert_eq!(updated["settings"]["automatic_backups"], true);
    }

    #[tokio::test]
    async fn file_access_is_limited_to_assigned_instances() {
        let state = test_state().await;
        let user_id = uuid::Uuid::new_v4().to_string();
        let allowed_instance = uuid::Uuid::new_v4().to_string();
        let denied_instance = uuid::Uuid::new_v4().to_string();
        let session_token = "operator-session-token";
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) VALUES (?, 'operator', 'unused', 'operator', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        for (id, name) in [
            (&allowed_instance, "assigned"),
            (&denied_instance, "not-assigned"),
        ] {
            sqlx::query(
                "INSERT INTO instances (id, name, profile_id, profile_revision, settings, created_at, updated_at) VALUES (?, ?, 'hytale', 2, '{}', ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(&now)
            .bind(&now)
            .execute(&state.pool)
            .await
            .unwrap();
            tokio::fs::create_dir_all(state.settings.instances_dir().join(id))
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO user_instance_grants (user_id, instance_id, permissions, created_at) VALUES (?, ?, '[]', ?)",
        )
        .bind(&user_id)
        .bind(&allowed_instance)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE instances SET runtime_state = 'crashed', desired_state = 'running' WHERE id = ?",
        )
        .bind(&allowed_instance)
        .execute(&state.pool)
        .await
        .unwrap();
        tokio::fs::write(
            state
                .settings
                .instances_dir()
                .join(&allowed_instance)
                .join("diagnostic.log"),
            b"native library load failed",
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .bind(token_hash(session_token))
        .bind(token_hash("csrf"))
        .bind((Utc::now() + Duration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();

        let router = api(state);
        let request = |instance_id: &str| {
            Request::get(format!("/api/v1/files?instance_id={instance_id}"))
                .header(header::COOKIE, format!("dmx_session={session_token}"))
                .body(Body::empty())
                .unwrap()
        };
        let allowed = router
            .clone()
            .oneshot(request(&allowed_instance))
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        let body = to_bytes(allowed.into_body(), 64 * 1024).await.unwrap();
        let files: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            files["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| { item["name"] == "diagnostic.log" })
        );

        let readable_after_crash = router
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/files/text?instance_id={allowed_instance}&path=diagnostic.log"
                ))
                .header(header::COOKIE, format!("dmx_session={session_token}"))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(readable_after_crash.status(), StatusCode::OK);
        let body = to_bytes(readable_after_crash.into_body(), 64 * 1024)
            .await
            .unwrap();
        let diagnostic: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(diagnostic["content"], "native library load failed");

        let denied = router.oneshot(request(&denied_instance)).await.unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn backup_jobs_stream_verify_and_restore_instance_data() {
        let (state, session, csrf) = authenticated_state().await;
        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO instances (id, name, profile_id, settings, installation_state, created_at, updated_at) \
             VALUES (?, 'backup-test', 'minecraft-java-vanilla', ?, 'installed', ?, ?)",
        )
        .bind(&instance_id)
        .bind(
            serde_json::json!({"version": "1.21.11", "eula_accepted": true}).to_string(),
        )
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        let world = state
            .settings
            .instances_dir()
            .join(&instance_id)
            .join("game/world");
        tokio::fs::create_dir_all(&world).await.unwrap();
        tokio::fs::write(world.join("level.dat"), b"before-backup")
            .await
            .unwrap();

        let router = api(state.clone());
        let response = router
            .clone()
            .oneshot(
                Request::post("/api/v1/backups")
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header("idempotency-key", "backup-e2e")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"instance_id": instance_id.clone()}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let job_id = job["id"].as_str().unwrap();
        wait_for_job(&state, job_id, "succeeded").await;
        let (backup_id, checksum): (String, String) = sqlx::query_as(
            "SELECT id, checksum_sha256 FROM backups WHERE creation_job_id = ? AND status = 'ready'",
        )
        .bind(job_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(checksum.len(), 64);

        tokio::fs::write(world.join("level.dat"), b"after-backup")
            .await
            .unwrap();
        tokio::fs::write(world.join("new.dat"), b"must-disappear")
            .await
            .unwrap();
        let response = router
            .oneshot(
                Request::post(format!("/api/v1/backups/{backup_id}/restore"))
                    .header(header::COOKIE, format!("dmx_session={session}"))
                    .header("x-csrf-token", &csrf)
                    .header("idempotency-key", "restore-e2e")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: serde_json::Value = serde_json::from_slice(&body).unwrap();
        wait_for_job(&state, job["id"].as_str().unwrap(), "succeeded").await;

        assert_eq!(
            tokio::fs::read(world.join("level.dat")).await.unwrap(),
            b"before-backup"
        );
        assert!(!tokio::fs::try_exists(world.join("new.dat")).await.unwrap());
        let pre_restore_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM backups WHERE instance_id = ? AND kind = 'pre_restore' AND status = 'ready'",
        )
        .bind(&instance_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(pre_restore_count, 1);
    }

    async fn wait_for_job(state: &AppState, job_id: &str, expected: &str) {
        for _ in 0..200 {
            let status: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(&state.pool)
                .await
                .unwrap();
            if status == expected {
                return;
            }
            assert_ne!(status, "failed", "job {job_id} failed unexpectedly");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("job {job_id} did not reach {expected}");
    }

    #[tokio::test]
    async fn setup_uses_the_real_forwarded_client_only_from_a_trusted_proxy() {
        let mut state = test_state().await;
        let settings = Arc::make_mut(&mut state.settings);
        settings.reverse_proxy = true;
        settings.trusted_proxies = vec!["127.0.0.1".parse().unwrap()];
        settings.setup_token = None;
        let router = api(state);
        let mut request = Request::post("/api/v1/auth/setup")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-forwarded-for", "203.0.113.10")
            .body(Body::from(
                serde_json::json!({
                    "username": "owner",
                    "password": "A-secure-password-42!"
                })
                .to_string(),
            ))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 41234))));
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn only_one_concurrent_request_can_claim_initial_setup() {
        let state = test_state().await;
        let claim = |pool: crate::core::DbPool| async move {
            let mut transaction = pool.begin().await.unwrap();
            let affected = sqlx::query(
                "UPDATE setup_state SET completed = 1 WHERE singleton = 1 AND completed = 0",
            )
            .execute(&mut *transaction)
            .await
            .unwrap()
            .rows_affected();
            transaction.commit().await.unwrap();
            affected
        };
        let (first, second) = tokio::join!(claim(state.pool.clone()), claim(state.pool.clone()));
        assert_eq!(first + second, 1);
    }
}
