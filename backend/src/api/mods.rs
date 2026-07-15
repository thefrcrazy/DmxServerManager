use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::{delete, get, post},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    api::{
        SuccessResponse,
        auth::{AuthUser, authorize_instance},
    },
    core::{AppState, database, error::AppError},
    services::{
        instance_storage, mod_providers, mod_providers::Compatibility, runtime::FilesystemLease,
        secure_fs,
    },
};

const MAX_MOD_BYTES: u64 = 512 * 1024 * 1024;
const MOD_UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MOD_UPLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
type ManualUploadStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<axum::body::Bytes, String>> + Send>>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/mods/providers", get(provider_status))
        .route(
            "/mods/providers/curseforge",
            axum::routing::put(configure_curseforge).delete(clear_curseforge),
        )
        .route("/servers/{id}/mods", get(list))
        .route("/servers/{id}/mods/manual", post(upload_manual))
        .route("/servers/{id}/mods/provider", post(install_provider))
        .route("/servers/{id}/mods/{mod_id}", delete(remove))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadQuery {
    filename: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct InstalledMod {
    id: String,
    instance_id: String,
    source: String,
    display_name: String,
    checksum_sha256: String,
    size_bytes: i64,
    provider_project_id: Option<String>,
    provider_version_id: Option<String>,
    enabled: bool,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct InstalledModList {
    items: Vec<InstalledMod>,
}

#[derive(Debug, FromRow)]
struct InstanceModContext {
    profile_id: String,
    profile_revision: i64,
    settings: String,
    installation_state: String,
    runtime_state: String,
}

struct AuthorizedMods {
    context: InstanceModContext,
    lease: Option<FilesystemLease>,
}

#[derive(Debug, FromRow)]
struct StoredMod {
    id: String,
    instance_id: String,
    relative_path: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderRequestKind {
    Modrinth,
    Curseforge,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderInstallRequest {
    provider: ProviderRequestKind,
    project_id: String,
    version_id: String,
}

#[derive(Debug, Serialize)]
struct ProviderConfiguration {
    configured: bool,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    modrinth: ProviderConfiguration,
    curseforge: ProviderConfiguration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigureCurseForgeRequest {
    api_key: String,
}

#[derive(Debug, FromRow)]
struct InstalledProviderDependency {
    provider_project_id: Option<String>,
    provider_version_id: Option<String>,
}

async fn provider_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ProviderStatus>, AppError> {
    require_owner(&auth)?;
    Ok(Json(ProviderStatus {
        modrinth: ProviderConfiguration { configured: true },
        curseforge: ProviderConfiguration {
            configured: mod_providers::curseforge_configured(&state.pool).await?,
        },
    }))
}

async fn configure_curseforge(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ConfigureCurseForgeRequest>,
) -> Result<Json<ProviderConfiguration>, AppError> {
    require_owner(&auth)?;
    mod_providers::set_curseforge_api_key(&state.pool, &state.secrets, &body.api_key).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "mod_provider.configure",
        "global_setting",
        Some("curseforge"),
        "success",
        serde_json::json!({"configured": true}),
    )
    .await?;
    Ok(Json(ProviderConfiguration { configured: true }))
}

async fn clear_curseforge(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ProviderConfiguration>, AppError> {
    require_owner(&auth)?;
    mod_providers::clear_curseforge_api_key(&state.pool).await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "mod_provider.configure",
        "global_setting",
        Some("curseforge"),
        "success",
        serde_json::json!({"configured": false}),
    )
    .await?;
    Ok(Json(ProviderConfiguration { configured: false }))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<InstalledModList>, AppError> {
    let authorized = authorize_mods(&state, &auth, &id, false).await?;
    require_mod_capability(
        &state,
        &authorized.context.profile_id,
        authorized.context.profile_revision,
    )?;
    let items = sqlx::query_as::<_, InstalledMod>(
        "SELECT id, instance_id, source, display_name, checksum_sha256, size_bytes, \
         provider_project_id, provider_version_id, enabled, created_at \
         FROM instance_mods WHERE instance_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(InstalledModList { items }))
}

async fn upload_manual(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<UploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<InstalledMod>), AppError> {
    let authorized = authorize_mods(&state, &auth, &id, true).await?;
    let context = &authorized.context;
    let directory = require_mod_capability(&state, &context.profile_id, context.profile_revision)?;
    let extension = validate_filename(&query.filename)?;
    validate_upload_headers(&headers)?;

    let root = instance_storage::resolve(&state.pool, &state.settings, &id)
        .await?
        .root;
    ensure_directory(&root, directory).await?;
    let mod_id = Uuid::new_v4().to_string();
    let relative_path = format!("{directory}/{mod_id}.{extension}");
    let size_bytes = write_manual_upload(
        &root,
        &relative_path,
        body,
        MOD_UPLOAD_IDLE_TIMEOUT,
        MOD_UPLOAD_TOTAL_TIMEOUT,
    )
    .await?;
    if size_bytes == 0 {
        let _ = secure_fs::delete_entry(&root, &relative_path).await;
        return Err(AppError::BadRequest("mods.empty_archive".into()));
    }

    let checksum = match validate_and_hash_archive(&root, &relative_path).await {
        Ok(checksum) => checksum,
        Err(error) => {
            let _ = secure_fs::delete_entry(&root, &relative_path).await;
            return Err(error);
        }
    };
    let now = chrono::Utc::now().to_rfc3339();
    let insert = sqlx::query(
        "INSERT INTO instance_mods \
         (id, instance_id, source, display_name, relative_path, checksum_sha256, size_bytes, \
          enabled, metadata, created_by, created_at) \
         VALUES (?, ?, 'manual', ?, ?, ?, ?, 1, '{}', ?, ?)",
    )
    .bind(&mod_id)
    .bind(&id)
    .bind(&query.filename)
    .bind(&relative_path)
    .bind(&checksum)
    .bind(i64::try_from(size_bytes).map_err(|_| AppError::BadRequest("mods.too_large".into()))?)
    .bind(&auth.id)
    .bind(&now)
    .execute(&state.pool)
    .await;
    if let Err(error) = insert {
        let _ = secure_fs::delete_entry(&root, &relative_path).await;
        return Err(error.into());
    }

    database::audit(
        &state.pool,
        Some(&auth.id),
        "mod.installed",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({
            "mod_id": mod_id,
            "source": "manual",
            "display_name": query.filename,
            "checksum_sha256": checksum,
            "size_bytes": size_bytes,
        }),
    )
    .await?;
    state.events.publish(
        "mod.installed",
        Some(id.clone()),
        serde_json::json!({"mod_id": mod_id}),
    );
    let response = (
        StatusCode::CREATED,
        Json(InstalledMod {
            id: mod_id,
            instance_id: id,
            source: "manual".into(),
            display_name: query.filename,
            checksum_sha256: checksum,
            size_bytes: i64::try_from(size_bytes)
                .map_err(|_| AppError::BadRequest("mods.too_large".into()))?,
            provider_project_id: None,
            provider_version_id: None,
            enabled: true,
            created_at: now,
        }),
    );
    release_mod_lease(authorized.lease).await?;
    Ok(response)
}

async fn install_provider(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<ProviderInstallRequest>,
) -> Result<(StatusCode, Json<InstalledMod>), AppError> {
    let authorized = authorize_mods(&state, &auth, &id, true).await?;
    let context = &authorized.context;
    let directory = require_mod_capability(&state, &context.profile_id, context.profile_revision)?;
    let compatibility = provider_compatibility(context)?;
    let artifact = match body.provider {
        ProviderRequestKind::Modrinth => {
            mod_providers::resolve_modrinth(&body.project_id, &body.version_id, &compatibility)
                .await?
        }
        ProviderRequestKind::Curseforge => {
            mod_providers::resolve_curseforge(
                &state.pool,
                &state.secrets,
                &body.project_id,
                &body.version_id,
                &compatibility,
            )
            .await?
        }
    };
    ensure_provider_dependencies(&state, &id, &artifact).await?;
    let duplicate: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instance_mods WHERE instance_id = ? AND source = ? \
         AND provider_project_id = ?",
    )
    .bind(&id)
    .bind(artifact.provider.as_str())
    .bind(&artifact.project_id)
    .fetch_one(&state.pool)
    .await?;
    if duplicate != 0 {
        return Err(AppError::Conflict(
            "mods.provider_project_already_installed".into(),
        ));
    }

    let root = instance_storage::resolve(&state.pool, &state.settings, &id)
        .await?
        .root;
    ensure_directory(&root, directory).await?;
    let mod_id = Uuid::new_v4().to_string();
    let relative_path = format!("{directory}/{mod_id}.jar");
    let downloaded = mod_providers::download(&artifact, &root, &relative_path).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let metadata = serde_json::to_string(&artifact.metadata)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let insert = sqlx::query(
        "INSERT INTO instance_mods \
         (id, instance_id, source, display_name, relative_path, checksum_sha256, size_bytes, \
          provider_project_id, provider_version_id, enabled, metadata, created_by, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind(&mod_id)
    .bind(&id)
    .bind(artifact.provider.as_str())
    .bind(&artifact.display_name)
    .bind(&relative_path)
    .bind(&downloaded.sha256)
    .bind(
        i64::try_from(downloaded.size_bytes)
            .map_err(|_| AppError::BadRequest("mods.too_large".into()))?,
    )
    .bind(&artifact.project_id)
    .bind(&artifact.version_id)
    .bind(metadata)
    .bind(&auth.id)
    .bind(&now)
    .execute(&state.pool)
    .await;
    if let Err(error) = insert {
        let _ = secure_fs::delete_entry(&root, &relative_path).await;
        return Err(error.into());
    }

    database::audit(
        &state.pool,
        Some(&auth.id),
        "mod.installed",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({
            "mod_id": mod_id,
            "source": artifact.provider.as_str(),
            "provider_project_id": artifact.project_id,
            "provider_version_id": artifact.version_id,
            "checksum_sha256": downloaded.sha256,
            "size_bytes": downloaded.size_bytes,
        }),
    )
    .await?;
    state.events.publish(
        "mod.installed",
        Some(id.clone()),
        serde_json::json!({"mod_id": mod_id, "source": artifact.provider.as_str()}),
    );
    let response = (
        StatusCode::CREATED,
        Json(InstalledMod {
            id: mod_id,
            instance_id: id,
            source: artifact.provider.as_str().into(),
            display_name: artifact.display_name,
            checksum_sha256: downloaded.sha256,
            size_bytes: i64::try_from(downloaded.size_bytes)
                .map_err(|_| AppError::BadRequest("mods.too_large".into()))?,
            provider_project_id: Some(artifact.project_id),
            provider_version_id: Some(artifact.version_id),
            enabled: true,
            created_at: now,
        }),
    );
    release_mod_lease(authorized.lease).await?;
    Ok(response)
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath((id, mod_id)): AxumPath<(String, String)>,
) -> Result<Json<SuccessResponse>, AppError> {
    let authorized = authorize_mods(&state, &auth, &id, true).await?;
    let parsed =
        Uuid::parse_str(&mod_id).map_err(|_| AppError::BadRequest("mods.invalid_id".into()))?;
    let stored = sqlx::query_as::<_, StoredMod>(
        "SELECT id, instance_id, relative_path, display_name FROM instance_mods \
         WHERE id = ? AND instance_id = ?",
    )
    .bind(parsed.to_string())
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("mods.not_found".into()))?;
    let root = instance_storage::resolve(&state.pool, &state.settings, &id)
        .await?
        .root;
    match secure_fs::delete_entry(&root, &stored.relative_path).await {
        Ok(()) | Err(AppError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }
    sqlx::query("DELETE FROM instance_mods WHERE id = ? AND instance_id = ?")
        .bind(&stored.id)
        .bind(&stored.instance_id)
        .execute(&state.pool)
        .await?;
    database::audit(
        &state.pool,
        Some(&auth.id),
        "mod.deleted",
        "instance",
        Some(&id),
        "success",
        serde_json::json!({"mod_id": stored.id, "display_name": stored.display_name}),
    )
    .await?;
    state.events.publish(
        "mod.deleted",
        Some(id),
        serde_json::json!({"mod_id": stored.id}),
    );
    release_mod_lease(authorized.lease).await?;
    Ok(SuccessResponse::with_message("mods.deleted"))
}

async fn authorize_mods(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
    mutation: bool,
) -> Result<AuthorizedMods, AppError> {
    super::servers::validate_instance_id(instance_id)?;
    authorize_instance(state, auth, instance_id, "mods.manage").await?;
    let lease = if mutation {
        Some(
            state
                .runtime
                .begin_filesystem_maintenance(instance_id)
                .await?,
        )
    } else {
        None
    };
    let context = sqlx::query_as::<_, InstanceModContext>(
        "SELECT profile_id, profile_revision, settings, installation_state, runtime_state \
         FROM instances WHERE id = ?",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    if mutation && (context.installation_state != "installed" || context.runtime_state != "stopped")
    {
        return Err(AppError::Conflict(
            "mods.server_must_be_installed_and_stopped".into(),
        ));
    }
    Ok(AuthorizedMods { context, lease })
}

async fn release_mod_lease(lease: Option<FilesystemLease>) -> Result<(), AppError> {
    lease
        .ok_or_else(|| AppError::Internal("missing filesystem maintenance lease".into()))?
        .release()
        .await
}

fn provider_compatibility(context: &InstanceModContext) -> Result<Compatibility, AppError> {
    let settings: serde_json::Value = serde_json::from_str(&context.settings)
        .map_err(|_| AppError::Internal("instance settings are invalid".into()))?;
    let game_version = if context.profile_id.starts_with("minecraft-java-") {
        Some(
            settings
                .get("version")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 64)
                .ok_or_else(|| AppError::BadRequest("mods.game_version_required".into()))?
                .to_string(),
        )
    } else {
        None
    };
    Ok(Compatibility {
        profile_id: context.profile_id.clone(),
        game_version,
    })
}

async fn ensure_provider_dependencies(
    state: &AppState,
    instance_id: &str,
    artifact: &mod_providers::ProviderArtifact,
) -> Result<(), AppError> {
    if artifact.required_dependencies.is_empty() {
        return Ok(());
    }
    let installed = sqlx::query_as::<_, InstalledProviderDependency>(
        "SELECT provider_project_id, provider_version_id FROM instance_mods \
         WHERE instance_id = ? AND source = ? AND enabled = 1",
    )
    .bind(instance_id)
    .bind(artifact.provider.as_str())
    .fetch_all(&state.pool)
    .await?;
    let missing =
        artifact.required_dependencies.iter().any(|dependency| {
            !installed.iter().any(|candidate| {
                let project_matches = dependency.project_id.as_ref().is_none_or(|required| {
                    candidate.provider_project_id.as_ref() == Some(required)
                });
                let version_matches = dependency.version_id.as_ref().is_none_or(|required| {
                    candidate.provider_version_id.as_ref() == Some(required)
                });
                project_matches && version_matches
            })
        });
    if missing {
        return Err(AppError::Conflict(
            "mods.provider_dependencies_missing".into(),
        ));
    }
    Ok(())
}

fn require_mod_capability(
    state: &AppState,
    profile_id: &str,
    profile_revision: i64,
) -> Result<&'static str, AppError> {
    let revision = u32::try_from(profile_revision)
        .map_err(|_| AppError::Internal("instance profile revision is invalid".into()))?;
    let profile = state
        .profiles
        .get_revision(profile_id, revision)
        .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    if !profile
        .capabilities
        .iter()
        .any(|capability| capability == "mods")
    {
        return Err(AppError::BadRequest("mods.profile_not_supported".into()));
    }
    mod_directory(profile_id)
        .ok_or_else(|| AppError::Internal("profile declares an invalid mods capability".into()))
}

fn mod_directory(profile_id: &str) -> Option<&'static str> {
    match profile_id {
        "hytale" => Some("game/Server/mods"),
        "minecraft-java-paper" | "minecraft-java-purpur" | "minecraft-java-spigot" => {
            Some("game/plugins")
        }
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-quilt" => Some("game/mods"),
        _ => None,
    }
}

fn require_owner(auth: &AuthUser) -> Result<(), AppError> {
    if auth.role == "owner" {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.owner_required".into()))
    }
}

fn validate_filename(filename: &str) -> Result<&'static str, AppError> {
    if filename.is_empty()
        || filename.chars().count() > 255
        || filename.chars().any(char::is_control)
        || Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(filename)
    {
        return Err(AppError::BadRequest("mods.invalid_filename".into()));
    }
    if filename.to_ascii_lowercase().ends_with(".jar") {
        Ok("jar")
    } else {
        Err(AppError::BadRequest("mods.unsupported_file_type".into()))
    }
}

fn validate_upload_headers(headers: &HeaderMap) -> Result<(), AppError> {
    if let Some(length) = headers.get(header::CONTENT_LENGTH) {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0 && *value <= MAX_MOD_BYTES)
            .ok_or_else(|| AppError::BadRequest("mods.invalid_content_length".into()))?;
        let _ = length;
    }
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        let content_type = content_type
            .to_str()
            .map_err(|_| AppError::BadRequest("mods.invalid_content_type".into()))?
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        if !matches!(
            content_type,
            "application/java-archive" | "application/zip" | "application/octet-stream"
        ) {
            return Err(AppError::BadRequest("mods.invalid_content_type".into()));
        }
    }
    Ok(())
}

async fn ensure_directory(root: &Path, relative: &str) -> Result<(), AppError> {
    match secure_fs::list_directory(root, relative).await {
        Ok(_) => Ok(()),
        Err(AppError::NotFound(_)) => secure_fs::create_directory(root, relative).await,
        Err(error) => Err(error),
    }
}

async fn validate_and_hash_archive(root: &Path, relative: &str) -> Result<String, AppError> {
    let (mut file, expected_size) = secure_fs::open_regular_file(root, relative).await?;
    let mut digest = Sha256::new();
    let mut first = [0_u8; 4];
    let mut first_len = 0_usize;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if first_len < first.len() {
            let copy = (first.len() - first_len).min(read);
            first[first_len..first_len + copy].copy_from_slice(&buffer[..copy]);
            first_len += copy;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::BadRequest("mods.too_large".into()))?;
        if total > MAX_MOD_BYTES {
            return Err(AppError::BadRequest("mods.too_large".into()));
        }
        digest.update(&buffer[..read]);
    }
    if total != expected_size || first_len < 4 || first != *b"PK\x03\x04" {
        return Err(AppError::BadRequest("mods.invalid_archive".into()));
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn idle_bounded_body_stream(
    body: Body,
    idle_timeout: Duration,
) -> (ManualUploadStream, Arc<AtomicBool>) {
    let idle_expired = Arc::new(AtomicBool::new(false));
    let stream_idle_expired = Arc::clone(&idle_expired);
    let stream = Box::pin(futures::stream::unfold(
        body.into_data_stream(),
        move |mut stream| {
            let stream_idle_expired = Arc::clone(&stream_idle_expired);
            async move {
                match tokio::time::timeout(idle_timeout, stream.next()).await {
                    Ok(Some(Ok(chunk))) => Some((Ok(chunk), stream)),
                    Ok(Some(Err(error))) => Some((Err(error.to_string()), stream)),
                    Ok(None) => None,
                    Err(_) => {
                        stream_idle_expired.store(true, Ordering::Release);
                        Some((Err("upload idle timeout".to_string()), stream))
                    }
                }
            }
        },
    ));
    (stream, idle_expired)
}

async fn write_manual_upload(
    root: &Path,
    relative_path: &str,
    body: Body,
    idle_timeout: Duration,
    total_timeout: Duration,
) -> Result<u64, AppError> {
    let (stream, idle_expired) = idle_bounded_body_stream(body, idle_timeout);
    let result = tokio::time::timeout(
        total_timeout,
        secure_fs::write_stream(root, relative_path, stream, MAX_MOD_BYTES),
    )
    .await;
    match result {
        Err(_) => Err(AppError::BadRequest("mods.upload_timeout".into())),
        Ok(Err(_)) if idle_expired.load(Ordering::Acquire) => {
            Err(AppError::BadRequest("mods.upload_timeout".into()))
        }
        Ok(result) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use futures::stream;

    #[test]
    fn manual_mod_names_are_single_safe_jar_components() {
        assert_eq!(validate_filename("example.jar").unwrap(), "jar");
        for filename in ["../escape.jar", "folder/mod.jar", "mod.zip", "mod.jar\n"] {
            assert!(
                validate_filename(filename).is_err(),
                "accepted {filename:?}"
            );
        }
    }

    #[test]
    fn upload_metadata_is_bounded_and_has_a_closed_mime_set() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_static("536870913"),
        );
        assert!(validate_upload_headers(&headers).is_err());
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("42"));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
        assert!(validate_upload_headers(&headers).is_err());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/java-archive"),
        );
        assert!(validate_upload_headers(&headers).is_ok());
    }

    #[tokio::test]
    async fn archive_validation_rejects_non_zip_content() {
        let temporary = tempfile::tempdir().unwrap();
        tokio::fs::write(temporary.path().join("fake.jar"), b"not a zip")
            .await
            .unwrap();
        assert!(
            validate_and_hash_archive(temporary.path(), "fake.jar")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn stalled_manual_upload_hits_total_timeout_and_removes_staging_file() {
        let temporary = tempfile::tempdir().unwrap();
        let body =
            Body::from_stream(stream::pending::<Result<axum::body::Bytes, std::io::Error>>());
        let error = write_manual_upload(
            temporary.path(),
            "stalled.jar",
            body,
            Duration::from_secs(1),
            Duration::from_millis(10),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(message) if message == "mods.upload_timeout"));
        assert!(
            std::fs::read_dir(temporary.path())
                .unwrap()
                .next()
                .is_none(),
            "a cancelled atomic upload left a temporary file behind"
        );
    }

    #[tokio::test]
    async fn stalled_manual_upload_hits_idle_timeout_and_removes_staging_file() {
        let temporary = tempfile::tempdir().unwrap();
        let body =
            Body::from_stream(stream::pending::<Result<axum::body::Bytes, std::io::Error>>());
        let error = write_manual_upload(
            temporary.path(),
            "idle.jar",
            body,
            Duration::from_millis(10),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(message) if message == "mods.upload_timeout"));
        assert!(
            std::fs::read_dir(temporary.path())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[tokio::test]
    async fn manual_upload_success_is_atomically_published_with_its_quota() {
        let temporary = tempfile::tempdir().unwrap();
        let payload = axum::body::Bytes::from_static(b"PK\x03\x04fixture");
        let written = write_manual_upload(
            temporary.path(),
            "mod.jar",
            Body::from(payload.clone()),
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(written, payload.len() as u64);
        assert_eq!(
            tokio::fs::read(temporary.path().join("mod.jar"))
                .await
                .unwrap(),
            payload
        );
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    }
}
