use std::{collections::HashMap, path::PathBuf};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, database, error::AppError},
    domain::v1::{DesiredState, InstallationState, Instance, PortProtocol, RuntimeState},
    services::{
        backups,
        instance_storage::{self, StorageMode},
        secrets::{
            allowed_secret_names, required_secret_names, validate_profile_secret,
            validate_profile_secret_value,
        },
    },
};

mod actions;
mod imports;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInstanceRequest {
    pub name: String,
    pub profile_id: String,
    #[serde(default = "empty_object")]
    pub settings: Value,
    #[serde(default)]
    pub secrets: HashMap<String, String>,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default = "default_true")]
    pub watchdog_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateInstanceRequest {
    pub name: Option<String>,
    pub settings: Option<Value>,
    pub auto_start: Option<bool>,
    pub watchdog_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSecretRequest {
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetProfileRevisionRequest {
    pub revision: u32,
}

#[derive(Debug, Serialize)]
pub struct SecretStatusList {
    pub items: Vec<SecretStatus>,
}

#[derive(Debug, Serialize)]
pub struct SecretStatus {
    pub name: String,
    pub configured: bool,
}

#[derive(Debug, FromRow)]
struct InstanceRow {
    id: String,
    name: String,
    profile_id: String,
    profile_revision: i64,
    settings: String,
    config_version: i64,
    installation_state: String,
    installed_version: Option<String>,
    installed_build: Option<String>,
    desired_state: String,
    runtime_state: String,
    managed: bool,
    auto_start: bool,
    watchdog_enabled: bool,
    created_at: String,
    updated_at: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers", get(list).post(create))
        .route("/servers/{id}", get(get_one).patch(update).delete(remove))
        .route("/servers/{id}/profile-revision", put(set_profile_revision))
        .route("/servers/{id}/secrets", get(list_secrets))
        .route(
            "/servers/{id}/secrets/{name}",
            put(set_secret).delete(delete_secret),
        )
        .merge(actions::routes())
        .merge(imports::routes())
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Instance>>, AppError> {
    auth.require("server.read")?;
    let rows: Vec<InstanceRow> = if matches!(auth.role.as_str(), "owner" | "admin") {
        sqlx::query_as("SELECT * FROM instances ORDER BY created_at DESC")
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT i.* FROM instances i
            JOIN user_instance_grants g ON g.instance_id = i.id
            WHERE g.user_id = ? AND (
                json_array_length(g.permissions) = 0 OR EXISTS (
                    SELECT 1 FROM json_each(g.permissions)
                    WHERE value IN ('*', 'server.read')
                )
            )
            ORDER BY i.created_at DESC
            "#,
        )
        .bind(&auth.id)
        .fetch_all(&state.pool)
        .await?
    };
    rows.into_iter()
        .map(instance_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateInstanceRequest>,
) -> Result<(StatusCode, HeaderMap, Json<Instance>), AppError> {
    auth.require("server.create")?;
    validate_instance_name(&body.name)?;
    let profile = state
        .profiles
        .get(&body.profile_id)
        .ok_or_else(|| AppError::BadRequest("profiles.unknown".into()))?;
    state
        .profiles
        .validate_settings(&profile.id, &body.settings)?;
    for (name, value) in &body.secrets {
        validate_profile_secret_value(&profile, name, value)?;
    }
    for required in required_secret_names(&profile.id) {
        if !body
            .secrets
            .get(*required)
            .is_some_and(|value| !value.is_empty())
        {
            return Err(AppError::BadRequest(format!("secrets.required:{required}")));
        }
    }

    let id = Uuid::new_v4().to_string();
    let root = instance_root(&state, &id)?;
    tokio::fs::create_dir_all(&root).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let result = create_instance_record(&state, &auth, &id, &body, &profile, &now).await;
    if let Err(error) = result {
        let _ = remove_instance_root(&state, &id).await;
        return Err(error);
    }

    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.created",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({
            "profile_id": profile.id,
            "eula_accepted": body.settings.get("eula_accepted").and_then(Value::as_bool),
        }),
    )
    .await?;
    state.events.publish(
        "server.created",
        Some(id.clone()),
        serde_json::json!({"name": body.name}),
    );
    let instance = fetch_instance(&state, &id).await?;
    let headers = etag_headers(instance.config_version)?;
    Ok((StatusCode::CREATED, headers, Json(instance)))
}

async fn create_instance_record(
    state: &AppState,
    auth: &AuthUser,
    id: &str,
    body: &CreateInstanceRequest,
    profile: &crate::domain::v1::GameProfile,
    now: &str,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO instances
            (id, name, profile_id, profile_revision, settings, auto_start,
             watchdog_enabled, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&profile.id)
    .bind(profile.revision)
    .bind(body.settings.to_string())
    .bind(body.auto_start)
    .bind(body.watchdog_enabled)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    // Keep provisioning invisible until the instance, its encrypted secrets and its initial
    // assignment are all durable. Acquiring the runtime lease before this transaction is not
    // possible because the actor deliberately rejects unknown instance IDs.
    for (name, value) in &body.secrets {
        let associated_data = format!("{id}:{name}");
        let (nonce, ciphertext) = state.secrets.seal(&associated_data, value)?;
        sqlx::query(
            r#"
            INSERT INTO instance_secrets
                (instance_id, name, nonce, ciphertext, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(nonce)
        .bind(ciphertext)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    }

    for (protocol, port, purpose) in effective_ports(profile, &body.settings)? {
        let inserted = sqlx::query(
            "INSERT INTO port_reservations (instance_id, protocol, port, purpose) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(protocol)
        .bind(port)
        .bind(purpose)
        .execute(&mut *transaction)
        .await;
        if inserted.is_err() {
            return Err(AppError::Conflict("servers.port_conflict".into()));
        }
    }

    // Creator receives an explicit assignment too; Owner/Admin still bypass it.
    sqlx::query(
        "INSERT INTO user_instance_grants (user_id, instance_id, permissions, created_at) VALUES (?, ?, '[]', ?)",
    )
    .bind(&auth.id)
    .bind(id)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<(HeaderMap, Json<Instance>), AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.read").await?;
    let instance = fetch_instance(&state, &id).await?;
    Ok((etag_headers(instance.config_version)?, Json(instance)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateInstanceRequest>,
) -> Result<(HeaderMap, Json<Instance>), AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.update").await?;
    let expected_version = parse_if_match(&headers)?;
    if let Some(name) = &body.name {
        validate_instance_name(name)?;
    }

    // Frontends send the complete settings object even for an operational-only edit. Compare it
    // against an ETag-validated snapshot first so rename/auto-start/watchdog changes remain
    // available while a server is running. A real settings change is then serialized with every
    // filesystem mutation and revalidated from a second authoritative snapshot under the lease.
    let initial = fetch_instance(&state, &id).await?;
    if initial.config_version != expected_version {
        return Err(AppError::Conflict("servers.version_conflict".into()));
    }
    let filesystem_lease = if update_requires_filesystem_lease(&body, &initial.settings) {
        Some(state.runtime.begin_filesystem_maintenance(&id).await?)
    } else {
        None
    };
    let current = if filesystem_lease.is_some() {
        fetch_instance(&state, &id).await?
    } else {
        initial
    };
    if current.config_version != expected_version {
        return Err(AppError::Conflict("servers.version_conflict".into()));
    }
    if let Some(settings) = &body.settings {
        state.profiles.validate_settings_revision(
            &current.profile_id,
            current.profile_revision,
            settings,
        )?;
    }
    let settings_changed = body
        .settings
        .as_ref()
        .is_some_and(|settings| settings != &current.settings);
    if settings_changed
        && (current.runtime_state != RuntimeState::Stopped
            || current.desired_state != DesiredState::Stopped)
    {
        return Err(AppError::Conflict(
            "servers.must_be_stopped_before_configuration".into(),
        ));
    }
    if settings_changed {
        let active_jobs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE instance_id = ? \
             AND state IN ('queued', 'running', 'waiting_for_user')",
        )
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
        if active_jobs != 0 {
            return Err(AppError::Conflict("servers.has_active_jobs".into()));
        }
    }

    let name = body
        .name
        .as_deref()
        .unwrap_or(&current.name)
        .trim()
        .to_string();
    let settings = body.settings.as_ref().unwrap_or(&current.settings);
    let auto_start = body.auto_start.unwrap_or(current.auto_start);
    let watchdog_enabled = body.watchdog_enabled.unwrap_or(current.watchdog_enabled);
    let reinstall_required = settings_changed
        && reinstall_required_for_settings(&current.profile_id, &current.settings, settings);
    if reinstall_required {
        authorize_instance(&state, &auth, &id, "server.update_game").await?;
    }
    let (installation_state, installed_version, installed_build) = if reinstall_required {
        ("not_installed", None, None)
    } else {
        (
            installation_state_name(&current.installation_state),
            current.installed_version.as_deref(),
            current.installed_build.as_deref(),
        )
    };
    let profile = state
        .profiles
        .get_revision(&current.profile_id, current.profile_revision)
        .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut transaction = state.pool.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE instances SET name = ?, settings = ?, auto_start = ?, watchdog_enabled = ?,
            installation_state = ?, installed_version = ?, installed_build = ?,
            config_version = config_version + 1, updated_at = ?
        WHERE id = ? AND config_version = ?
        "#,
    )
    .bind(name)
    .bind(settings.to_string())
    .bind(auto_start)
    .bind(watchdog_enabled)
    .bind(installation_state)
    .bind(installed_version)
    .bind(installed_build)
    .bind(&now)
    .bind(&id)
    .bind(expected_version)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict("servers.version_conflict".into()));
    }
    if settings_changed {
        sqlx::query("DELETE FROM port_reservations WHERE instance_id = ?")
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        for (protocol, port, purpose) in effective_ports(&profile, settings)? {
            if sqlx::query(
                "INSERT INTO port_reservations (instance_id, protocol, port, purpose) VALUES (?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(protocol)
            .bind(port)
            .bind(purpose)
            .execute(&mut *transaction)
            .await
            .is_err()
            {
                return Err(AppError::Conflict("servers.port_conflict".into()));
            }
        }
    }
    transaction.commit().await?;
    if let Some(lease) = filesystem_lease {
        lease.release().await?;
    }

    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.updated",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({
            "previous_version": expected_version,
            "settings_changed": settings_changed,
            "reinstall_required": reinstall_required,
        }),
    )
    .await?;
    state
        .events
        .publish("server.updated", Some(id.clone()), serde_json::json!({}));
    let instance = fetch_instance(&state, &id).await?;
    Ok((etag_headers(instance.config_version)?, Json(instance)))
}

async fn set_profile_revision(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SetProfileRevisionRequest>,
) -> Result<(HeaderMap, Json<Instance>), AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.update_game").await?;
    let expected_version = parse_if_match(&headers)?;

    // A profile revision can change both the on-disk layout and launch contract. Acquire the
    // same exclusive lease as files/mods/imports before reading any validation context.
    let filesystem_lease = state.runtime.begin_filesystem_maintenance(&id).await?;
    let current = fetch_instance(&state, &id).await?;
    if current.config_version != expected_version {
        return Err(AppError::Conflict("servers.version_conflict".into()));
    }
    if current.runtime_state != RuntimeState::Stopped
        || current.desired_state != DesiredState::Stopped
    {
        return Err(AppError::Conflict(
            "servers.must_be_stopped_before_profile_upgrade".into(),
        ));
    }
    if body.revision <= current.profile_revision {
        return Err(AppError::BadRequest(
            "profiles.upgrade_requires_newer_revision".into(),
        ));
    }
    let target = state
        .profiles
        .get_revision(&current.profile_id, body.revision)
        .ok_or_else(|| AppError::NotFound("profiles.revision_not_found".into()))?;
    state.profiles.validate_settings_revision(
        &current.profile_id,
        body.revision,
        &current.settings,
    )?;
    let active_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE instance_id = ? \
         AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    if active_jobs != 0 {
        return Err(AppError::Conflict("servers.has_active_jobs".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut transaction = state.pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE instances SET profile_revision = ?, config_version = config_version + 1, \
         installation_state = 'not_installed', installed_version = NULL, installed_build = NULL, \
         updated_at = ? WHERE id = ? AND config_version = ? AND runtime_state = 'stopped' \
         AND desired_state = 'stopped'",
    )
    .bind(body.revision)
    .bind(&now)
    .bind(&id)
    .bind(expected_version)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict("servers.version_conflict".into()));
    }
    sqlx::query("DELETE FROM port_reservations WHERE instance_id = ?")
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
    for (protocol, port, purpose) in effective_ports(&target, &current.settings)? {
        if sqlx::query(
            "INSERT INTO port_reservations (instance_id, protocol, port, purpose) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(protocol)
        .bind(port)
        .bind(purpose)
        .execute(&mut *transaction)
        .await
        .is_err()
        {
            return Err(AppError::Conflict("servers.port_conflict".into()));
        }
    }
    transaction.commit().await?;
    filesystem_lease.release().await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.profile_revision_changed",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({
            "from_revision": current.profile_revision,
            "to_revision": body.revision,
        }),
    )
    .await?;
    state.events.publish(
        "server.profile_revision_changed",
        Some(id.clone()),
        serde_json::json!({"revision": body.revision}),
    );
    let instance = fetch_instance(&state, &id).await?;
    Ok((etag_headers(instance.config_version)?, Json(instance)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.delete").await?;

    // Deletion competes with file downloads/uploads, mods, imports and backup hooks. Keep the
    // exclusive lease from the authoritative re-read through every root/DB mutation.
    let filesystem_lease = state.runtime.begin_filesystem_maintenance(&id).await?;
    let instance = fetch_instance(&state, &id).await?;
    if instance.runtime_state != RuntimeState::Stopped
        || instance.desired_state != DesiredState::Stopped
    {
        return Err(AppError::Conflict(
            "servers.must_be_stopped_before_delete".into(),
        ));
    }
    let active_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE instance_id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    if active_jobs > 0 {
        return Err(AppError::Conflict("servers.has_active_jobs".into()));
    }
    let tombstone = stage_instance_root_for_deletion(&state, &id).await?;
    let deleted = sqlx::query("DELETE FROM instances WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await;
    if let Err(error) = deleted {
        if let Some(tombstone) = &tombstone {
            let _ = tokio::fs::rename(tombstone, instance_root(&state, &id)?).await;
        }
        return Err(error.into());
    }
    if let Some(tombstone) = tombstone
        && let Err(error) = remove_path(&tombstone).await
    {
        tracing::warn!(path = %tombstone.display(), %error, "failed to purge deleted instance tombstone");
    }
    if !instance.managed
        && let Err(error) = remove_instance_root(&state, &id).await
    {
        tracing::warn!(instance_id = %id, %error, "failed to purge detached instance control directory");
    }
    if let Err(error) = backups::purge_instance_storage(&state.settings, &id).await {
        tracing::warn!(instance_id = %id, %error, "failed to purge deleted instance backups");
    }
    filesystem_lease.release().await?;
    if let Err(error) = database::audit(
        &state.pool,
        Some(&auth.id),
        "server.deleted",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({"profile_id": instance.profile_id}),
    )
    .await
    {
        tracing::error!(%error, instance_id = %id, "failed to audit completed instance deletion");
    }
    state
        .events
        .publish("server.deleted", Some(id), serde_json::json!({}));
    Ok(SuccessResponse::with_message("servers.deleted"))
}

async fn list_secrets(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<SecretStatusList>, AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.update").await?;
    let instance = fetch_instance(&state, &id).await?;
    let configured: Vec<String> =
        sqlx::query_scalar("SELECT name FROM instance_secrets WHERE instance_id = ? ORDER BY name")
            .bind(&id)
            .fetch_all(&state.pool)
            .await?;
    let items = allowed_secret_names(&instance.profile_id)
        .iter()
        .map(|name| SecretStatus {
            name: (*name).to_string(),
            configured: configured
                .iter()
                .any(|configured_name| configured_name == name),
        })
        .collect();
    Ok(Json(SecretStatusList { items }))
}

async fn set_secret(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<SecretStatus>, AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.update").await?;
    let instance = fetch_instance(&state, &id).await?;
    validate_profile_secret(&instance.profile_id, &name)?;
    let profile = state
        .profiles
        .get_revision(&instance.profile_id, instance.profile_revision)
        .ok_or_else(|| AppError::Internal("stored profile revision is unavailable".into()))?;
    validate_profile_secret_value(&profile, &name, &body.value)?;
    state
        .secrets
        .set(&state.pool, &id, &name, &body.value)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.secret_set",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({"name": name}),
    )
    .await?;
    Ok(Json(SecretStatus {
        name,
        configured: true,
    }))
}

async fn delete_secret(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<SecretStatus>, AppError> {
    validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.update").await?;
    let instance = fetch_instance(&state, &id).await?;
    validate_profile_secret(&instance.profile_id, &name)?;
    sqlx::query("DELETE FROM instance_secrets WHERE instance_id = ? AND name = ?")
        .bind(&id)
        .bind(&name)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.secret_deleted",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({"name": name}),
    )
    .await?;
    Ok(Json(SecretStatus {
        name,
        configured: false,
    }))
}

async fn fetch_instance(state: &AppState, id: &str) -> Result<Instance, AppError> {
    let row: InstanceRow = sqlx::query_as("SELECT * FROM instances WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    instance_from_row(row)
}

fn instance_from_row(row: InstanceRow) -> Result<Instance, AppError> {
    Ok(Instance {
        id: row.id,
        name: row.name,
        profile_id: row.profile_id,
        profile_revision: u32::try_from(row.profile_revision)
            .map_err(|_| AppError::Internal("invalid profile revision".into()))?,
        settings: serde_json::from_str(&row.settings)
            .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?,
        config_version: u32::try_from(row.config_version)
            .map_err(|_| AppError::Internal("invalid config version".into()))?,
        installation_state: parse_installation_state(&row.installation_state)?,
        installed_version: row.installed_version,
        installed_build: row.installed_build,
        desired_state: parse_desired_state(&row.desired_state)?,
        runtime_state: parse_runtime_state(&row.runtime_state)?,
        managed: row.managed,
        auto_start: row.auto_start,
        watchdog_enabled: row.watchdog_enabled,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn effective_ports(
    profile: &crate::domain::v1::GameProfile,
    settings: &Value,
) -> Result<Vec<(String, u16, String)>, AppError> {
    let ports = profile
        .ports
        .iter()
        .map(|spec| {
            let configured = settings
                .get(&spec.name)
                .and_then(Value::as_u64)
                .unwrap_or(u64::from(spec.default));
            let port = u16::try_from(configured)
                .map_err(|_| AppError::BadRequest("servers.invalid_port".into()))?;
            let protocol = match spec.protocol {
                PortProtocol::Tcp => "tcp",
                PortProtocol::Udp => "udp",
            };
            Ok((protocol.to_string(), port, spec.name.clone()))
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(ports)
}

fn validate_instance_name(name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        Err(AppError::BadRequest("servers.invalid_name".into()))
    } else {
        Ok(())
    }
}

pub(super) fn validate_instance_id(id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(id).map_err(|_| AppError::BadRequest("servers.invalid_id".into()))
}

fn instance_root(state: &AppState, id: &str) -> Result<PathBuf, AppError> {
    instance_storage::managed_root(&state.settings, id)
}

async fn remove_instance_root(state: &AppState, id: &str) -> Result<(), AppError> {
    let root = instance_root(state, id)?;
    let base = state.settings.instances_dir();
    if !root.starts_with(&base) {
        return Err(AppError::Internal(
            "refusing unsafe instance deletion".into(),
        ));
    }
    remove_path(&root).await
}

async fn stage_instance_root_for_deletion(
    state: &AppState,
    id: &str,
) -> Result<Option<PathBuf>, AppError> {
    let storage = instance_storage::resolve(&state.pool, &state.settings, id).await?;
    if storage.mode == StorageMode::Attached {
        return Ok(None);
    }
    let root = storage.root;
    match tokio::fs::symlink_metadata(&root).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let tombstone = state
        .settings
        .instances_dir()
        .join(format!(".deleting-{id}-{}", Uuid::new_v4()));
    tokio::fs::rename(root, &tombstone).await?;
    Ok(Some(tombstone))
}

async fn remove_path(root: &std::path::Path) -> Result<(), AppError> {
    let metadata = match tokio::fs::symlink_metadata(root).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        tokio::fs::remove_file(root).await?;
    } else {
        tokio::fs::remove_dir_all(root).await?;
    }
    Ok(())
}

fn etag_headers(version: u32) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{version}\""))
            .map_err(|error| AppError::Internal(error.to_string()))?,
    );
    Ok(headers)
}

fn parse_if_match(headers: &HeaderMap) -> Result<u32, AppError> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().trim_start_matches("W/").trim_matches('"'))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| AppError::PreconditionRequired("servers.if_match_required".into()))
}

fn reinstall_required_for_settings(profile_id: &str, current: &Value, next: &Value) -> bool {
    if profile_id == "minecraft-bedrock" {
        return current != next;
    }
    if profile_id.starts_with("minecraft-java-") {
        return ["version", "loader_version", "port"]
            .iter()
            .any(|key| current.get(*key) != next.get(*key));
    }
    false
}

fn update_requires_filesystem_lease(
    body: &UpdateInstanceRequest,
    current_settings: &Value,
) -> bool {
    body.settings
        .as_ref()
        .is_some_and(|settings| settings != current_settings)
}

const fn installation_state_name(state: &InstallationState) -> &'static str {
    match state {
        InstallationState::NotInstalled => "not_installed",
        InstallationState::Installing => "installing",
        InstallationState::Installed => "installed",
        InstallationState::Updating => "updating",
        InstallationState::Failed => "failed",
    }
}

fn parse_installation_state(value: &str) -> Result<InstallationState, AppError> {
    match value {
        "not_installed" => Ok(InstallationState::NotInstalled),
        "installing" => Ok(InstallationState::Installing),
        "installed" => Ok(InstallationState::Installed),
        "updating" => Ok(InstallationState::Updating),
        "failed" => Ok(InstallationState::Failed),
        _ => Err(AppError::Internal("invalid installation state".into())),
    }
}

fn parse_desired_state(value: &str) -> Result<DesiredState, AppError> {
    match value {
        "running" => Ok(DesiredState::Running),
        "stopped" => Ok(DesiredState::Stopped),
        _ => Err(AppError::Internal("invalid desired state".into())),
    }
}

fn parse_runtime_state(value: &str) -> Result<RuntimeState, AppError> {
    match value {
        "stopped" => Ok(RuntimeState::Stopped),
        "starting" => Ok(RuntimeState::Starting),
        "running" => Ok(RuntimeState::Running),
        "stopping" => Ok(RuntimeState::Stopping),
        "crashed" => Ok(RuntimeState::Crashed),
        "unknown" => Ok(RuntimeState::Unknown),
        _ => Err(AppError::Internal("invalid runtime state".into())),
    }
}

fn empty_object() -> Value {
    serde_json::json!({})
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_ids_cannot_be_paths() {
        assert!(validate_instance_id("../../etc").is_err());
        assert!(validate_instance_id(&Uuid::new_v4().to_string()).is_ok());
    }

    #[test]
    fn if_match_requires_a_numeric_entity_tag() {
        let headers = HeaderMap::new();
        assert!(parse_if_match(&headers).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"7\""));
        assert_eq!(parse_if_match(&headers).unwrap(), 7);
    }

    #[test]
    fn game_or_loader_version_changes_require_reinstallation() {
        let current = serde_json::json!({
            "version": "1.21.4",
            "loader_version": "0.16.10",
            "motd": "before"
        });
        let cosmetic = serde_json::json!({
            "version": "1.21.4",
            "loader_version": "0.16.10",
            "motd": "after"
        });
        let upgraded = serde_json::json!({
            "version": "1.21.5",
            "loader_version": "0.16.10",
            "motd": "before"
        });
        assert!(!reinstall_required_for_settings(
            "minecraft-java-fabric",
            &current,
            &cosmetic,
        ));
        assert!(reinstall_required_for_settings(
            "minecraft-java-fabric",
            &current,
            &upgraded,
        ));
        assert!(reinstall_required_for_settings(
            "minecraft-bedrock",
            &current,
            &cosmetic,
        ));
    }

    #[test]
    fn only_real_settings_changes_require_the_filesystem_lease() {
        let current_settings = serde_json::json!({"version": "1.21.4"});
        let operational_only = UpdateInstanceRequest {
            name: Some("Renamed".into()),
            settings: None,
            auto_start: Some(true),
            watchdog_enabled: Some(false),
        };
        assert!(!update_requires_filesystem_lease(
            &operational_only,
            &current_settings
        ));

        let complete_but_unchanged = UpdateInstanceRequest {
            name: Some("Renamed".into()),
            settings: Some(current_settings.clone()),
            auto_start: Some(true),
            watchdog_enabled: Some(false),
        };
        assert!(!update_requires_filesystem_lease(
            &complete_but_unchanged,
            &current_settings
        ));

        let settings_update = UpdateInstanceRequest {
            name: None,
            settings: Some(serde_json::json!({"version": "1.21.5"})),
            auto_start: None,
            watchdog_enabled: None,
        };
        assert!(update_requires_filesystem_lease(
            &settings_update,
            &current_settings
        ));
    }
}
