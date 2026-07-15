use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header},
    routing::post,
};
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    api::auth::{AuthUser, authorize_instance},
    core::{AppState, database, error::AppError},
    domain::v1::{GameProfile, Job, JobState, ProfileKind},
    services::{
        installers::{self, ArchiveLimits, InstallContext, extract_zip},
        instance_storage::{self, StorageMode},
        jobs,
    },
};

const MAX_IMPORT_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_IMPORT_ENTRIES: usize = 100_000;
const MAX_IMPORT_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const IMPORT_UPLOAD_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const IMPORT_UPLOAD_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 60);
static ATTACH_ROOT_LOCK: Mutex<()> = Mutex::const_new(());

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers/{id}/imports/zip", post(import_zip))
        .route("/servers/{id}/imports/copy", post(import_copy))
        .route("/servers/{id}/imports/attach", post(import_attach))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRequest {
    source_path: PathBuf,
}

#[derive(Debug, sqlx::FromRow)]
struct ImportableInstance {
    profile_id: String,
    profile_revision: i64,
    settings: String,
    installation_state: String,
    desired_state: String,
    runtime_state: String,
    storage_mode: String,
}

#[derive(Debug, sqlx::FromRow)]
struct BedrockWaitingInstall {
    job_id: String,
    settings: String,
}

async fn import_zip(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<Job>), AppError> {
    validate_zip_headers(&headers)?;
    super::validate_instance_id(&id)?;
    authorize_instance(&state, &auth, &id, "server.files.write").await?;
    if let Some(waiting) = waiting_bedrock_install(&state, &id).await? {
        return upload_bedrock_archive(state, auth, id, headers, body, waiting).await;
    }
    let key = idempotency_key(&headers)?;
    if let Some(job) =
        replay_or_prepare_import(&state, &auth, &id, false, "import_zip", key.as_deref()).await?
    {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let (job, created, claim) =
        jobs::create_claimed(&state.pool, &id, "import_zip", &auth.id, key.as_deref()).await?;
    if !created {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let submission = ImportSubmissionGuard::new(
        state.clone(),
        job.clone(),
        claim.expect("newly-created jobs always carry a claim"),
    );

    let work = import_work_directory(&state, &job.id)?;
    let archive = work.join("upload.zip");
    let upload = async {
        create_private_work_directory(&work).await?;
        write_upload(&archive, body, MAX_IMPORT_ARCHIVE_BYTES).await
    }
    .await;
    upload?;
    let claim = submission.into_claim();

    let spawned_state = state.clone();
    let spawned_job = job.clone();
    let worker = run_job(
        spawned_state,
        spawned_job,
        claim,
        move |state, job| async move {
            let work = import_work_directory(&state, &job.id)?;
            let archive = work.join("upload.zip");
            let candidate = work.join("candidate");
            let game = candidate.join("game");
            create_private_work_directory(&work).await?;
            tokio::fs::create_dir_all(&game).await?;
            extract_zip(
                &archive,
                &game,
                ArchiveLimits {
                    max_entries: MAX_IMPORT_ENTRIES,
                    max_file_bytes: MAX_IMPORT_FILE_BYTES,
                    max_total_bytes: MAX_IMPORT_BYTES,
                    max_compression_ratio: 250,
                },
                None,
            )
            .await
            .map_err(|error| {
                tracing::warn!(job_id = %job.id, code = error.code, detail = ?error.internal, "ZIP import rejected");
                AppError::BadRequest("imports.archive_invalid".into())
            })?;
            jobs::progress(&state.pool, &job.id, 55).await?;
            validate_candidate(&state, &job, &candidate).await?;
            commit_managed(&state, &job, &candidate).await
        },
    );
    tokio::spawn(worker);
    publish_queued(&state, &job);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn waiting_bedrock_install(
    state: &AppState,
    instance_id: &str,
) -> Result<Option<BedrockWaitingInstall>, AppError> {
    super::validate_instance_id(instance_id)?;
    let waiting: Option<BedrockWaitingInstall> = sqlx::query_as(
        "SELECT j.id AS job_id, i.settings \
         FROM instances i JOIN jobs j ON j.instance_id = i.id \
         WHERE i.id = ? AND i.profile_id = 'minecraft-bedrock' \
         AND j.kind = 'install' AND j.state = 'waiting_for_user'",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(waiting) = waiting else {
        return Ok(None);
    };
    let payload: Option<String> = sqlx::query_scalar(
        "SELECT payload FROM job_events WHERE job_id = ? AND event_type = 'job.waiting_for_user' \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&waiting.job_id)
    .fetch_optional(&state.pool)
    .await?;
    let is_archive_wait = payload
        .as_deref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|payload| {
            payload
                .pointer("/interaction/kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|kind| kind == "bedrock_archive_upload");
    if !is_archive_wait {
        return Err(AppError::Conflict(
            "imports.bedrock_archive_not_requested".into(),
        ));
    }
    Ok(Some(waiting))
}

async fn upload_bedrock_archive(
    state: AppState,
    auth: AuthUser,
    instance_id: String,
    headers: HeaderMap,
    body: Body,
    waiting: BedrockWaitingInstall,
) -> Result<(StatusCode, Json<Job>), AppError> {
    if auth.role != "owner" {
        return Err(AppError::Forbidden(
            "imports.bedrock_archive_owner_only".into(),
        ));
    }
    authorize_instance(&state, &auth, &instance_id, "server.files.write").await?;
    let expected_sha256 = required_bedrock_sha256(&headers)?;
    let settings: serde_json::Value = serde_json::from_str(&waiting.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    if settings
        .get("eula_accepted")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(AppError::Conflict("servers.minecraft_eula_required".into()));
    }
    let expected_version = settings
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| AppError::Conflict("servers.bedrock_version_unavailable".into()))?;
    let lease = state
        .runtime
        .begin_job_filesystem_maintenance(&instance_id, &waiting.job_id)
        .await?;
    let storage = instance_storage::resolve(&state.pool, &state.settings, &instance_id).await?;
    if storage.mode != StorageMode::Managed {
        return Err(AppError::Conflict(
            "imports.bedrock_managed_storage_required".into(),
        ));
    }

    let upload = tokio::time::timeout(
        IMPORT_UPLOAD_TOTAL_TIMEOUT,
        installers::store_bedrock_upload(
            &storage.root,
            &waiting.job_id,
            expected_version,
            &expected_sha256,
            idle_bounded_body_stream(body, IMPORT_UPLOAD_IDLE_TIMEOUT),
        ),
    )
    .await;
    let artifact = match upload {
        Ok(result) => result.map_err(map_bedrock_upload_error),
        Err(_) => {
            let _ = installers::remove_bedrock_upload(&storage.root, &waiting.job_id).await;
            Err(AppError::BadRequest("imports.upload_timeout".into()))
        }
    };
    lease.release().await?;
    let artifact = artifact?;
    let job = match state
        .runtime
        .resume_waiting_install(&instance_id, &waiting.job_id)
        .await
    {
        Ok(job) => job,
        Err(error) => {
            let _ = installers::remove_bedrock_upload(&storage.root, &waiting.job_id).await;
            return Err(error);
        }
    };
    database::audit(
        &state.pool,
        Some(&auth.id),
        "server.bedrock_archive_uploaded",
        "instance",
        Some(&instance_id),
        "success",
        serde_json::json!({
            "job_id": &job.id,
            "sha256": artifact.sha256,
            "size_bytes": artifact.size,
            "version": expected_version,
            "eula_accepted": true,
        }),
    )
    .await?;
    state.events.publish(
        "job.updated",
        Some(instance_id.clone()),
        serde_json::to_value(&job).unwrap_or_default(),
    );
    Ok((StatusCode::ACCEPTED, Json(job)))
}

fn required_bedrock_sha256(headers: &HeaderMap) -> Result<String, AppError> {
    let value = headers
        .get("x-dmx-archive-sha256")
        .ok_or_else(|| AppError::BadRequest("imports.sha256_required".into()))?
        .to_str()
        .map_err(|_| AppError::BadRequest("imports.sha256_invalid".into()))?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest("imports.sha256_invalid".into()));
    }
    Ok(value.to_ascii_lowercase())
}

fn map_bedrock_upload_error(error: installers::InstallerError) -> AppError {
    tracing::warn!(code = error.code, detail = ?error.internal, "Bedrock archive upload rejected");
    match error.code {
        "bedrock_upload_conflict" => AppError::Conflict("imports.bedrock_archive_conflict".into()),
        "bedrock_upload_checksum_mismatch" => {
            AppError::BadRequest("imports.checksum_mismatch".into())
        }
        "bedrock_upload_too_large" => AppError::BadRequest("imports.archive_too_large".into()),
        _ if error.internal.is_some() => AppError::Internal("bedrock upload failed".into()),
        _ => AppError::BadRequest("imports.bedrock_archive_invalid".into()),
    }
}

async fn record_submission_failure(state: &AppState, job: &Job) {
    let _ = jobs::fail(
        &state.pool,
        &job.id,
        "import_upload_failed",
        "imports.upload_failed",
    )
    .await;
    let _ = database::audit(
        &state.pool,
        Some(&job.requested_by),
        "server.import_zip",
        "instance",
        job.instance_id.as_deref(),
        "failure",
        serde_json::json!({"job_id": job.id, "error_code": "import_upload_failed"}),
    )
    .await;
    if let Ok(updated) = jobs::get(&state.pool, &job.id).await {
        state.events.publish(
            "job.updated",
            updated.instance_id.clone(),
            serde_json::to_value(updated).unwrap_or_default(),
        );
    }
}

struct ImportSubmissionGuard {
    state: AppState,
    job: Option<Job>,
    claim: Option<jobs::JobClaim>,
}

impl ImportSubmissionGuard {
    fn new(state: AppState, job: Job, claim: jobs::JobClaim) -> Self {
        Self {
            state,
            job: Some(job),
            claim: Some(claim),
        }
    }

    fn into_claim(mut self) -> jobs::JobClaim {
        self.job = None;
        self.claim
            .take()
            .expect("an armed import submission owns its job claim")
    }
}

impl Drop for ImportSubmissionGuard {
    fn drop(&mut self) {
        let Some(job) = self.job.take() else {
            return;
        };
        let claim = self.claim.take();
        let state = self.state.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                record_submission_failure(&state, &job).await;
                if let Some(claim) = claim
                    && let Err(error) = claim.disarm_terminal().await
                {
                    tracing::error!(job_id = %job.id, %error, "failed to disarm abandoned import submission claim");
                }
                if let Ok(work) = import_work_directory(&state, &job.id)
                    && let Err(error) = remove_tree(&work).await
                {
                    tracing::warn!(path = %work.display(), %error, "failed to clean abandoned import upload");
                }
            });
        } else {
            tracing::error!(job_id = %job.id, "import submission abandoned outside a Tokio runtime");
        }
    }
}

async fn import_copy(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<SourceRequest>,
) -> Result<(StatusCode, Json<Job>), AppError> {
    let key = idempotency_key(&headers)?;
    if let Some(job) =
        replay_or_prepare_import(&state, &auth, &id, false, "import_copy", key.as_deref()).await?
    {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let source =
        instance_storage::validate_import_source(&state.settings, &body.source_path).await?;
    reject_protected_attach_root(&state, &source).await?;
    let (job, created, claim) =
        jobs::create_claimed(&state.pool, &id, "import_copy", &auth.id, key.as_deref()).await?;
    if !created {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }

    let spawned_state = state.clone();
    let spawned_job = job.clone();
    let worker = run_job(
        spawned_state,
        spawned_job,
        claim.expect("newly-created jobs always carry a claim"),
        move |state, job| async move {
            let work = import_work_directory(&state, &job.id)?;
            let candidate = work.join("candidate");
            let game = candidate.join("game");
            create_private_work_directory(&work).await?;
            tokio::fs::create_dir_all(&game).await?;
            instance_storage::validate_instance_tree(&source, false).await?;
            copy_directory(&source, &game).await?;
            jobs::progress(&state.pool, &job.id, 55).await?;
            validate_candidate(&state, &job, &candidate).await?;
            commit_managed(&state, &job, &candidate).await
        },
    );
    tokio::spawn(worker);
    publish_queued(&state, &job);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn import_attach(
    State(state): State<AppState>,
    auth: AuthUser,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<SourceRequest>,
) -> Result<(StatusCode, Json<Job>), AppError> {
    let key = idempotency_key(&headers)?;
    if let Some(job) =
        replay_or_prepare_import(&state, &auth, &id, true, "import_attach", key.as_deref()).await?
    {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }
    let source =
        instance_storage::validate_import_source(&state.settings, &body.source_path).await?;
    reject_protected_attach_root(&state, &source).await?;
    let (job, created, claim) =
        jobs::create_claimed(&state.pool, &id, "import_attach", &auth.id, key.as_deref()).await?;
    if !created {
        return Ok((StatusCode::ACCEPTED, Json(job)));
    }

    let spawned_state = state.clone();
    let spawned_job = job.clone();
    let worker = run_job(
        spawned_state,
        spawned_job,
        claim.expect("newly-created jobs always carry a claim"),
        move |state, job| async move {
            // The lock covers external-tree validation and the database reservation. Two attach
            // jobs must never inspect or normalize the same (or nested) tree concurrently.
            let _attach_guard = ATTACH_ROOT_LOCK.lock().await;
            instance_storage::validate_instance_tree(&source, true).await?;
            jobs::progress(&state.pool, &job.id, 60).await?;
            let marker_created = validate_profile_layout(&state, &job, &source, false).await?;
            let result = reserve_attached_root(
                &state,
                job.instance_id
                    .as_deref()
                    .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?,
                &source,
            )
            .await;
            if result.is_err() {
                if marker_created {
                    let _ = tokio::fs::remove_file(source.join("game/.dmx-install.json")).await;
                }
                return result;
            }
            Ok(())
        },
    );
    tokio::spawn(worker);
    publish_queued(&state, &job);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn replay_or_prepare_import(
    state: &AppState,
    auth: &AuthUser,
    instance_id: &str,
    attach: bool,
    kind: &str,
    idempotency_key: Option<&str>,
) -> Result<Option<Job>, AppError> {
    super::validate_instance_id(instance_id)?;
    authorize_instance(state, auth, instance_id, "server.files.write").await?;
    if attach && auth.role != "owner" {
        return Err(AppError::Forbidden("imports.attach_owner_only".into()));
    }
    if let Some(key) = idempotency_key
        && let Some(existing) =
            jobs::find_idempotent(&state.pool, key, instance_id, kind, &auth.id).await?
    {
        return Ok(Some(existing));
    }
    let instance: ImportableInstance = sqlx::query_as(
        "SELECT profile_id, profile_revision, settings, installation_state, desired_state, runtime_state, storage_mode \
         FROM instances WHERE id = ?",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    if instance.profile_id == "minecraft-bedrock" {
        return Err(AppError::Conflict(
            "imports.bedrock_install_job_required".into(),
        ));
    }
    if instance.storage_mode != "managed"
        || instance.installation_state != "not_installed"
        || instance.runtime_state != "stopped"
        || instance.desired_state != "stopped"
    {
        return Err(AppError::Conflict(
            "imports.requires_empty_stopped_instance".into(),
        ));
    }
    let settings: serde_json::Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let revision = u32::try_from(instance.profile_revision)
        .map_err(|_| AppError::Internal("stored profile revision is invalid".into()))?;
    state
        .profiles
        .validate_settings_revision(&instance.profile_id, revision, &settings)?;
    Ok(None)
}

async fn reserve_attached_root(
    state: &AppState,
    instance_id: &str,
    source: &Path,
) -> Result<(), AppError> {
    let source_string = source
        .to_str()
        .ok_or_else(|| AppError::BadRequest("imports.non_utf8_path".into()))?;
    let mut connection = state.pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *connection)
        .await?;
    let result = async {
        let attached: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, data_path FROM instances \
             WHERE storage_mode = 'attached' AND data_path IS NOT NULL AND id <> ?",
        )
        .bind(instance_id)
        .fetch_all(&mut *connection)
        .await?;
        for (_, existing) in attached {
            let existing = PathBuf::from(existing);
            if source == existing || source.starts_with(&existing) || existing.starts_with(source) {
                return Err(AppError::Conflict("imports.attach_root_conflict".into()));
            }
        }
        let update = sqlx::query(
            "UPDATE instances SET storage_mode = 'attached', managed = 0, data_path = ?, \
             installation_state = 'installed', \
             installed_version = COALESCE(json_extract(settings, '$.version'), installed_version), \
             updated_at = ? WHERE id = ? AND storage_mode = 'managed' AND managed = 1 \
             AND installation_state = 'not_installed' AND runtime_state = 'stopped' \
             AND desired_state = 'stopped'",
        )
        .bind(source_string)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(instance_id)
        .execute(&mut *connection)
        .await?;
        if update.rows_affected() != 1 {
            return Err(AppError::Conflict("imports.instance_changed".into()));
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            sqlx::query("COMMIT").execute(&mut *connection).await?;
            Ok(())
        }
        Err(error) => {
            if let Err(rollback_error) = sqlx::query("ROLLBACK").execute(&mut *connection).await {
                tracing::error!(%rollback_error, "failed to roll back attached root reservation");
            }
            Err(error)
        }
    }
}

async fn reject_protected_attach_root(state: &AppState, source: &Path) -> Result<(), AppError> {
    let mut protected = vec![
        state.settings.data_dir.clone(),
        state.settings.config_file.clone(),
        state.settings.master_key_file.clone(),
    ];
    if let Some(parent) = state.settings.master_key_file.parent() {
        protected.push(parent.to_path_buf());
    }
    if let Some(database) = sqlite_database_path(&state.settings.database_url) {
        protected.push(database);
    }
    for path in protected {
        let path = normalize_overlap_path(&path).await?;
        if source == path || source.starts_with(&path) || path.starts_with(source) {
            return Err(AppError::Forbidden("imports.attach_protected_path".into()));
        }
    }
    Ok(())
}

fn sqlite_database_path(url: &str) -> Option<PathBuf> {
    let value = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))?
        .split('?')
        .next()?;
    (!value.is_empty() && value != ":memory:").then(|| PathBuf::from(value))
}

async fn normalize_overlap_path(path: &Path) -> Result<PathBuf, AppError> {
    if let Ok(canonical) = tokio::fs::canonicalize(path).await {
        return Ok(canonical);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

async fn validate_candidate(state: &AppState, job: &Job, candidate: &Path) -> Result<(), AppError> {
    instance_storage::validate_instance_tree(candidate, true).await?;
    validate_profile_layout(state, job, candidate, true)
        .await
        .map(|_| ())
}

async fn validate_profile_layout(
    state: &AppState,
    job: &Job,
    root: &Path,
    normalize_permissions: bool,
) -> Result<bool, AppError> {
    let instance_id = job
        .instance_id
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?;
    let instance: ImportableInstance = sqlx::query_as(
        "SELECT profile_id, profile_revision, settings, installation_state, desired_state, runtime_state, storage_mode \
         FROM instances WHERE id = ?",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    let settings: serde_json::Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let revision = u32::try_from(instance.profile_revision)
        .map_err(|_| AppError::Internal("stored profile revision is invalid".into()))?;
    state
        .profiles
        .validate_settings_revision(&instance.profile_id, revision, &settings)?;
    let profile = state
        .profiles
        .get_revision(&instance.profile_id, revision)
        .ok_or_else(|| AppError::BadRequest("profiles.unknown_revision".into()))?;
    let (relative, requires_execute) = required_executable(&profile, &settings)?;
    let game = root.join("game");
    validate_regular_file(&game, &relative, requires_execute, normalize_permissions).await?;
    let context = InstallContext::official()
        .map_err(|_| AppError::Internal("failed to initialize installer policy".into()))?
        .with_toolchain_root(state.settings.data_dir.join("toolchains/java"));
    let adopted = installers::adopt_imported(&instance.profile_id, &settings, &game, &context)
        .await
        .map_err(|error| {
            tracing::warn!(job_id = %job.id, code = error.code, detail = ?error.internal, "import runtime preparation failed");
            AppError::BadRequest(error.client_message.to_string())
        })?;
    Ok(adopted.is_some())
}

fn required_executable(
    profile: &GameProfile,
    settings: &serde_json::Value,
) -> Result<(PathBuf, bool), AppError> {
    if profile.kind == ProfileKind::SteamCustom {
        return Err(AppError::BadRequest(
            "imports.steam_profile_not_supported".into(),
        ));
    }
    let (path, requires_execute) = match profile.id.as_str() {
        "minecraft-java-vanilla"
        | "minecraft-java-paper"
        | "minecraft-java-spigot"
        | "minecraft-java-purpur" => ("server.jar".to_string(), false),
        "minecraft-java-fabric" => ("fabric-server-launch.jar".to_string(), false),
        "minecraft-java-quilt" => ("quilt-server-launch.jar".to_string(), false),
        "minecraft-java-forge" | "minecraft-java-neoforge" => {
            let loader_version = import_loader_version(settings)?;
            let coordinate = if profile.id == "minecraft-java-forge" {
                "net/minecraftforge/forge"
            } else {
                "net/neoforged/neoforge"
            };
            let argument_file = if cfg!(windows) {
                "win_args.txt"
            } else {
                "unix_args.txt"
            };
            (
                format!("libraries/{coordinate}/{loader_version}/{argument_file}"),
                false,
            )
        }
        "minecraft-bedrock" if cfg!(windows) => ("bedrock_server.exe".to_string(), false),
        "minecraft-bedrock" => ("bedrock_server".to_string(), true),
        "hytale" => ("Server/HytaleServer.jar".to_string(), false),
        "valheim" if cfg!(windows) => ("valheim_server.exe".to_string(), false),
        "valheim" => ("valheim_server.x86_64".to_string(), true),
        "palworld" if cfg!(windows) => ("PalServer.exe".to_string(), false),
        "palworld" => ("PalServer.sh".to_string(), true),
        _ => {
            return Err(AppError::BadRequest(
                "imports.profile_not_importable".into(),
            ));
        }
    };
    let path = crate::domain::v1::safe_join(Path::new(""), &path)
        .map_err(|_| AppError::BadRequest("imports.invalid_executable".into()))?;
    Ok((path, requires_execute))
}

fn import_loader_version(settings: &serde_json::Value) -> Result<&str, AppError> {
    let value = settings
        .get("loader_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| AppError::BadRequest("imports.loader_version_required".into()))?;
    let normalized = value.to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 96
        || matches!(normalized.as_str(), "latest" | "recommended" | "stable")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
    {
        return Err(AppError::BadRequest(
            "imports.invalid_loader_version".into(),
        ));
    }
    Ok(value)
}

async fn validate_regular_file(
    root: &Path,
    relative: &Path,
    requires_execute: bool,
    normalize_permissions: bool,
) -> Result<(), AppError> {
    let root = root.to_path_buf();
    let relative = relative.to_path_buf();
    tokio::task::spawn_blocking(move || {
        #[cfg(not(unix))]
        let _ = (requires_execute, normalize_permissions);
        let mut current = root;
        for component in relative.components() {
            current.push(component.as_os_str());
            let metadata = fs::symlink_metadata(&current)
                .map_err(|_| AppError::BadRequest("imports.executable_missing".into()))?;
            if metadata_is_link_like(&metadata) {
                return Err(AppError::BadRequest("imports.links_forbidden".into()));
            }
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| AppError::BadRequest("imports.executable_missing".into()))?;
        if !metadata.is_file() {
            return Err(AppError::BadRequest("imports.executable_invalid".into()));
        }
        #[cfg(unix)]
        if requires_execute {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if normalize_permissions {
                fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
            } else if metadata.mode() & 0o111 == 0 {
                return Err(AppError::BadRequest(
                    "imports.executable_not_executable".into(),
                ));
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

async fn run_job<F, Fut>(state: AppState, job: Job, claim: jobs::JobClaim, operation: F)
where
    F: FnOnce(AppState, Job) -> Fut,
    Fut: std::future::Future<Output = Result<(), AppError>>,
{
    let mut claim = Some(claim);
    let began = jobs::begin(&state.pool, &job.id).await;
    match began {
        Ok(true) => {}
        Ok(false) => {
            disarm_import_claim(&mut claim, &job.id).await;
            return;
        }
        Err(error) => {
            tracing::error!(job_id = %job.id, %error, "failed to begin import job");
            return;
        }
    }
    let instance_id = match job.instance_id.as_deref() {
        Some(instance_id) => instance_id,
        None => {
            if jobs::fail(
                &state.pool,
                &job.id,
                "import_failed",
                "jobs.instance_required",
            )
            .await
            .is_ok()
            {
                disarm_import_claim(&mut claim, &job.id).await;
            }
            return;
        }
    };
    let lease = state
        .runtime
        .begin_job_filesystem_maintenance(instance_id, &job.id)
        .await;
    let result = match lease {
        Ok(lease) => {
            let operation_result = operation(state.clone(), job.clone()).await;
            match lease.release().await {
                Ok(()) => operation_result,
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    };
    let work = import_work_directory(&state, &job.id).ok();
    match result {
        Ok(()) => {
            let completed = if let Err(error) = jobs::succeed(&state.pool, &job.id).await {
                tracing::error!(job_id = %job.id, %error, "failed to complete import job");
                false
            } else {
                disarm_import_claim(&mut claim, &job.id).await;
                true
            };
            if completed {
                let _ = database::audit(
                    &state.pool,
                    Some(&job.requested_by),
                    &format!("server.{}", job.kind),
                    "instance",
                    job.instance_id.as_deref(),
                    "success",
                    serde_json::json!({"job_id": job.id}),
                )
                .await;
                if let Some(instance_id) = &job.instance_id {
                    state.events.publish(
                        "server.imported",
                        Some(instance_id.clone()),
                        serde_json::json!({"job_id": job.id, "kind": job.kind}),
                    );
                }
            }
        }
        Err(error) => {
            tracing::warn!(job_id = %job.id, %error, "instance import failed");
            let (error_code, message) = public_job_error(&error);
            if let Err(update_error) = jobs::fail(&state.pool, &job.id, error_code, message).await {
                tracing::error!(job_id = %job.id, %update_error, "failed to persist import failure");
            } else {
                disarm_import_claim(&mut claim, &job.id).await;
            }
            let _ = database::audit(
                &state.pool,
                Some(&job.requested_by),
                &format!("server.{}", job.kind),
                "instance",
                job.instance_id.as_deref(),
                "failure",
                serde_json::json!({"job_id": job.id, "error_code": error_code}),
            )
            .await;
        }
    }
    if let Ok(updated) = jobs::get(&state.pool, &job.id).await {
        state.events.publish(
            "job.updated",
            updated.instance_id.clone(),
            serde_json::to_value(updated).unwrap_or_default(),
        );
    }
    if let Some(work) = work
        && let Err(error) = remove_tree(&work).await
    {
        tracing::warn!(path = %work.display(), %error, "failed to clean import staging");
    }
}

async fn disarm_import_claim(claim: &mut Option<jobs::JobClaim>, job_id: &str) {
    let Some(claim) = claim.take() else {
        return;
    };
    if let Err(error) = claim.disarm_terminal().await {
        tracing::error!(%job_id, %error, "failed to disarm terminal import job claim");
    }
}

fn public_job_error(error: &AppError) -> (&'static str, &str) {
    match error {
        AppError::BadRequest(message) | AppError::Forbidden(message) => {
            ("import_rejected", message)
        }
        AppError::Conflict(message) | AppError::PreconditionRequired(message) => {
            ("import_conflict", message)
        }
        _ => ("import_failed", "imports.failed"),
    }
}

async fn commit_managed(state: &AppState, job: &Job, candidate: &Path) -> Result<(), AppError> {
    let instance_id = job
        .instance_id
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?;
    let storage = instance_storage::resolve(&state.pool, &state.settings, instance_id).await?;
    if storage.mode != StorageMode::Managed {
        return Err(AppError::Conflict("imports.instance_changed".into()));
    }
    jobs::progress(&state.pool, &job.id, 75).await?;
    let work = import_work_directory(state, &job.id)?;
    let rollback = work.join("rollback");
    let existed = tokio::fs::try_exists(&storage.root).await?;
    if existed {
        let metadata = tokio::fs::symlink_metadata(&storage.root).await?;
        if !metadata.is_dir() || metadata_is_link_like(&metadata) {
            return Err(AppError::Conflict("imports.unsafe_managed_root".into()));
        }
        tokio::fs::rename(&storage.root, &rollback).await?;
    }
    if let Err(error) = tokio::fs::rename(candidate, &storage.root).await {
        if existed {
            let _ = tokio::fs::rename(&rollback, &storage.root).await;
        }
        return Err(error.into());
    }
    let updated = sqlx::query(
        "UPDATE instances SET installation_state = 'installed', \
         installed_version = COALESCE(json_extract(settings, '$.version'), installed_version), updated_at = ? \
         WHERE id = ? AND storage_mode = 'managed' AND managed = 1 \
         AND installation_state = 'not_installed' AND runtime_state = 'stopped' AND desired_state = 'stopped'",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(instance_id)
    .execute(&state.pool)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {
            if existed && let Err(error) = remove_tree(&rollback).await {
                tracing::warn!(path = %rollback.display(), %error, "failed to purge import rollback");
            }
            let _ = jobs::progress(&state.pool, &job.id, 95).await;
            Ok(())
        }
        result => {
            let failed = work.join("failed-candidate");
            if let Err(error) = tokio::fs::rename(&storage.root, &failed).await {
                tracing::error!(path = %storage.root.display(), %error, "failed to stage rejected import for rollback");
                return Err(AppError::Internal(
                    "import rollback could not be started".into(),
                ));
            }
            if existed {
                tokio::fs::rename(&rollback, &storage.root).await.map_err(|error| {
                    tracing::error!(path = %storage.root.display(), %error, "failed to restore import rollback");
                    AppError::Internal("import rollback failed".into())
                })?;
            }
            match result {
                Ok(_) => Err(AppError::Conflict("imports.instance_changed".into())),
                Err(error) => Err(error.into()),
            }
        }
    }
}

async fn copy_directory(source: &Path, destination: &Path) -> Result<(), AppError> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || copy_directory_blocking(&source, &destination))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

fn copy_directory_blocking(source: &Path, destination: &Path) -> Result<(), AppError> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut count = 0_usize;
    let mut total = 0_u64;
    while let Some((source_directory, destination_directory)) = pending.pop() {
        for entry in fs::read_dir(source_directory)? {
            let entry = entry?;
            count += 1;
            if count > MAX_IMPORT_ENTRIES {
                return Err(AppError::BadRequest("imports.too_many_entries".into()));
            }
            let source_path = entry.path();
            let destination_path = destination_directory.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path)?;
            if metadata_is_link_like(&metadata) {
                return Err(AppError::BadRequest("imports.links_forbidden".into()));
            }
            if metadata.is_dir() {
                create_private_directory(&destination_path)?;
                pending.push((source_path, destination_path));
            } else if metadata.is_file() {
                reject_hardlink(&source_path, &metadata)?;
                total = total
                    .checked_add(metadata.len())
                    .ok_or_else(|| AppError::BadRequest("imports.quota_exceeded".into()))?;
                if metadata.len() > MAX_IMPORT_FILE_BYTES || total > MAX_IMPORT_BYTES {
                    return Err(AppError::BadRequest("imports.quota_exceeded".into()));
                }
                copy_regular_file(&source_path, &destination_path, metadata.len())?;
            } else {
                return Err(AppError::BadRequest(
                    "imports.special_files_forbidden".into(),
                ));
            }
        }
    }
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path, expected: u64) -> Result<(), AppError> {
    let mut source_options = OpenOptions::new();
    source_options.read(true);
    let mut destination_options = OpenOptions::new();
    destination_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        source_options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        destination_options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        source_options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        destination_options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut input = source_options.open(source)?;
    let input_metadata = input.metadata()?;
    if !input_metadata.is_file() || input_metadata.len() != expected {
        return Err(AppError::BadRequest("imports.source_changed".into()));
    }
    reject_hardlink(source, &input_metadata)?;
    let mut output = destination_options.open(destination)?;
    let copied = io::copy(&mut input, &mut output)?;
    if copied != expected {
        return Err(AppError::BadRequest("imports.source_changed".into()));
    }
    output.sync_all()?;
    Ok(())
}

async fn write_upload(path: &Path, body: Body, maximum: u64) -> Result<u64, AppError> {
    let result = tokio::time::timeout(
        IMPORT_UPLOAD_TOTAL_TIMEOUT,
        write_upload_inner(path, body, maximum),
    )
    .await;
    match result {
        Ok(Ok(written)) => Ok(written),
        Ok(Err(error)) => {
            let _ = tokio::fs::remove_file(path).await;
            Err(error)
        }
        Err(_) => {
            let _ = tokio::fs::remove_file(path).await;
            Err(AppError::BadRequest("imports.upload_timeout".into()))
        }
    }
}

async fn write_upload_inner(path: &Path, body: Body, maximum: u64) -> Result<u64, AppError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(path).await?;
    let mut stream = body.into_data_stream();
    let mut written = 0_u64;
    loop {
        let next = tokio::time::timeout(IMPORT_UPLOAD_IDLE_TIMEOUT, stream.next())
            .await
            .map_err(|_| AppError::BadRequest("imports.upload_timeout".into()))?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk.map_err(|_| AppError::BadRequest("imports.upload_invalid".into()))?;
        written = written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| AppError::BadRequest("imports.archive_too_large".into()))?;
        if written > maximum {
            drop(file);
            let _ = tokio::fs::remove_file(path).await;
            return Err(AppError::BadRequest("imports.archive_too_large".into()));
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
    }
    if written == 0 {
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
        return Err(AppError::BadRequest("imports.archive_empty".into()));
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    file.sync_all().await?;
    Ok(written)
}

fn idle_bounded_body_stream(
    body: Body,
    idle_timeout: std::time::Duration,
) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<axum::body::Bytes, String>> + Send>> {
    Box::pin(futures::stream::unfold(
        body.into_data_stream(),
        move |mut stream| async move {
            match tokio::time::timeout(idle_timeout, stream.next()).await {
                Ok(Some(Ok(chunk))) => Some((Ok(chunk), stream)),
                Ok(Some(Err(error))) => Some((Err(error.to_string()), stream)),
                Ok(None) => None,
                Err(_) => Some((Err("upload idle timeout".to_string()), stream)),
            }
        },
    ))
}

async fn create_private_work_directory(path: &Path) -> Result<(), AppError> {
    let mut builder = tokio::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path).await?;
    Ok(())
}

fn validate_zip_headers(headers: &HeaderMap) -> Result<(), AppError> {
    if let Some(length) = headers.get(header::CONTENT_LENGTH) {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| AppError::BadRequest("imports.invalid_content_length".into()))?;
        if length == 0 || length > MAX_IMPORT_ARCHIVE_BYTES {
            return Err(AppError::BadRequest("imports.archive_too_large".into()));
        }
    }
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        let content_type = content_type
            .to_str()
            .map_err(|_| AppError::BadRequest("imports.invalid_content_type".into()))?
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        if !matches!(content_type, "application/zip" | "application/octet-stream") {
            return Err(AppError::BadRequest("imports.invalid_content_type".into()));
        }
    }
    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| AppError::BadRequest("jobs.invalid_idempotency_key".into()))?;
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(|character| character.is_control())
    {
        return Err(AppError::BadRequest("jobs.invalid_idempotency_key".into()));
    }
    Ok(Some(value.to_string()))
}

fn import_work_directory(state: &AppState, job_id: &str) -> Result<PathBuf, AppError> {
    let id = uuid::Uuid::parse_str(job_id)
        .map_err(|_| AppError::Internal("invalid persisted job id".into()))?;
    Ok(state
        .settings
        .data_dir
        .join("import-staging")
        .join(id.to_string()))
}

fn publish_queued(state: &AppState, job: &Job) {
    if matches!(job.state, JobState::Queued) {
        state.events.publish(
            "job.queued",
            job.instance_id.clone(),
            serde_json::to_value(job).unwrap_or_default(),
        );
    }
}

async fn remove_tree(path: &Path) -> Result<(), AppError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Err(AppError::Internal("unsafe import staging path".into()));
    }
    tokio::fs::remove_dir_all(path).await?;
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

#[cfg(unix)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn reject_hardlink(path: &Path, metadata: &fs::Metadata) -> Result<(), AppError> {
    if crate::services::secure_fs::file_has_multiple_links(path, metadata)? {
        Err(AppError::BadRequest("imports.hardlinks_forbidden".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use std::{io::Write, net::SocketAddr, sync::Arc};
    use tower::ServiceExt;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use crate::{
        core::{Settings, config::DeploymentMode, database, events::EventHub},
        services::{
            profiles::ProfileRegistry, releases::ReleaseMonitor, runtime::RuntimeManager,
            secrets::SecretStore,
        },
    };

    #[test]
    fn executable_paths_are_profile_owned_and_custom_steam_import_is_closed() {
        let profiles = ProfileRegistry::builtins();
        assert_eq!(
            required_executable(
                &profiles.get("minecraft-java-vanilla").unwrap(),
                &serde_json::json!({})
            )
            .unwrap(),
            (PathBuf::from("server.jar"), false)
        );
        assert_eq!(
            required_executable(
                &profiles.get("minecraft-java-fabric").unwrap(),
                &serde_json::json!({"loader_version": "0.18.4"})
            )
            .unwrap(),
            (PathBuf::from("fabric-server-launch.jar"), false)
        );
        assert_eq!(
            required_executable(
                &profiles.get("minecraft-java-forge").unwrap(),
                &serde_json::json!({"loader_version": "1.21.1-52.1.0"})
            )
            .unwrap(),
            (
                PathBuf::from(format!(
                    "libraries/net/minecraftforge/forge/1.21.1-52.1.0/{}",
                    if cfg!(windows) {
                        "win_args.txt"
                    } else {
                        "unix_args.txt"
                    }
                )),
                false
            )
        );
        assert!(
            required_executable(
                &profiles.get("minecraft-java-forge").unwrap(),
                &serde_json::json!({"loader_version": "../../bin/sh"})
            )
            .is_err()
        );
        let mut custom = profiles.get("valheim").unwrap();
        custom.id = "local-steam-profile".into();
        custom.kind = ProfileKind::SteamCustom;
        assert!(required_executable(&custom, &serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn upload_stream_is_bounded_and_removed_on_overflow() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("upload.zip");
        assert!(write_upload(&path, Body::from("12345"), 4).await.is_err());
        assert!(!tokio::fs::try_exists(path).await.unwrap());
    }

    #[tokio::test]
    async fn protected_paths_and_attached_root_overlaps_are_rejected() {
        let (state, import_root, _session, _csrf) = test_state().await;
        let protected = tokio::fs::canonicalize(&state.settings.data_dir)
            .await
            .unwrap();
        assert!(
            reject_protected_attach_root(&state, &protected)
                .await
                .is_err()
        );

        let first = insert_valheim_instance(&state, "attach-parent").await;
        let nested = insert_valheim_instance(&state, "attach-nested").await;
        let exact = insert_valheim_instance(&state, "attach-exact").await;
        let parent = import_root.join("reserved");
        let child = parent.join("child");
        tokio::fs::create_dir_all(&child).await.unwrap();
        let parent = tokio::fs::canonicalize(parent).await.unwrap();
        let child = tokio::fs::canonicalize(child).await.unwrap();

        reserve_attached_root(&state, &first, &parent)
            .await
            .unwrap();
        assert!(matches!(
            reserve_attached_root(&state, &nested, &child).await,
            Err(AppError::Conflict(message)) if message == "imports.attach_root_conflict"
        ));
        assert!(matches!(
            reserve_attached_root(&state, &exact, &parent).await,
            Err(AppError::Conflict(message)) if message == "imports.attach_root_conflict"
        ));
    }

    #[tokio::test]
    async fn concurrent_exact_attached_root_reservation_has_one_winner() {
        let (state, import_root, _session, _csrf) = test_state().await;
        let first = insert_valheim_instance(&state, "attach-race-a").await;
        let second = insert_valheim_instance(&state, "attach-race-b").await;
        let source = import_root.join("race-root");
        tokio::fs::create_dir_all(&source).await.unwrap();
        let source = tokio::fs::canonicalize(source).await.unwrap();
        let (left, right) = tokio::join!(
            reserve_attached_root(&state, &first, &source),
            reserve_attached_root(&state, &second, &source),
        );
        assert_ne!(left.is_ok(), right.is_ok());
    }

    #[tokio::test]
    async fn aborting_zip_submission_terminalizes_job_and_releases_instance() {
        let (state, _import_root, session, csrf) = test_state().await;
        let instance_id = insert_valheim_instance(&state, "aborted-upload").await;
        let body_stream = futures::stream::once(async {
            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"PK"))
        })
        .chain(futures::stream::pending());
        let request = Request::post(format!("/api/v1/servers/{instance_id}/imports/zip"))
            .header(header::COOKIE, format!("dmx_session={session}"))
            .header("x-csrf-token", csrf)
            .header(header::CONTENT_TYPE, "application/zip")
            .body(Body::from_stream(body_stream))
            .unwrap();
        let task = tokio::spawn(router(state.clone()).oneshot(request));
        let job_id = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(id) = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM jobs WHERE instance_id = ? AND kind = 'import_zip'",
                )
                .bind(&instance_id)
                .fetch_optional(&state.pool)
                .await
                .unwrap()
                {
                    break id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let upload = import_work_directory(&state, &job_id)
            .unwrap()
            .join("upload.zip");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if tokio::fs::metadata(&upload)
                    .await
                    .is_ok_and(|metadata| metadata.len() == 2)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        assert_eq!(wait_for_job(&state, &job_id).await, "failed");
        state
            .runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap()
            .release()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn zip_extraction_rejects_traversal_before_candidate_commit() {
        let temporary = tempfile::tempdir().unwrap();
        let archive = temporary.path().join("hostile.zip");
        let file = fs::File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../escape", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"escape").unwrap();
        writer.finish().unwrap();
        let result = extract_zip(
            &archive,
            &temporary.path().join("candidate/game"),
            ArchiveLimits {
                max_entries: 10,
                max_file_bytes: 1024,
                max_total_bytes: 1024,
                max_compression_ratio: 10,
            },
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(!temporary.path().join("escape").exists());
    }

    async fn test_state() -> (AppState, PathBuf, String, String) {
        let root = tempfile::tempdir().unwrap().keep();
        let data_dir = root.join("data");
        let import_root = root.join("imports");
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        tokio::fs::create_dir_all(&import_root).await.unwrap();
        let database_url = format!("sqlite:{}/test.db?mode=rwc", data_dir.display());
        let settings = Settings {
            config_file: root.join("config.toml"),
            data_dir: data_dir.clone(),
            static_dir: root.join("static"),
            bind: SocketAddr::from(([127, 0, 0, 1], 5500)),
            database_url: database_url.clone(),
            master_key_file: data_dir.join("master.key"),
            steamcmd_path: PathBuf::from("steamcmd"),
            bedrock_linux_source: None,
            bedrock_windows_source: None,
            import_roots: vec![import_root.clone()],
            trusted_proxies: Vec::new(),
            reverse_proxy: false,
            log: "error".into(),
            dev_origin: None,
            setup_token: None,
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
        let events = EventHub::new(64);
        let runtime = RuntimeManager::new(
            pool.clone(),
            settings.clone(),
            events.clone(),
            secrets.clone(),
        );
        let releases = ReleaseMonitor::new(settings.clone()).unwrap();
        let state = AppState {
            pool,
            settings,
            profiles,
            events,
            secrets,
            runtime,
            releases,
        };
        let user_id = uuid::Uuid::new_v4().to_string();
        let session = "import-owner-session".to_string();
        let csrf = "import-owner-csrf".to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'import-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&user_id)
        .bind(token_hash(&session))
        .bind(token_hash(&csrf))
        .bind((Utc::now() + Duration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        (state, import_root, session, csrf)
    }

    async fn insert_valheim_instance(state: &AppState, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, created_at, updated_at) \
             VALUES (?, ?, 'valheim', 1, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(
            serde_json::json!({
                "server_name": name,
                "world_name": "World",
                "port": 2456,
                "query_port": 2457,
                "crossplay": false
            })
            .to_string(),
        )
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        tokio::fs::create_dir_all(state.settings.instances_dir().join(&id))
            .await
            .unwrap();
        id
    }

    async fn insert_waiting_bedrock_install(state: &AppState, owner_id: &str) -> (String, Job) {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, installation_state, created_at, updated_at) \
             VALUES (?, 'Bedrock fixture', 'minecraft-bedrock', 1, ?, 'installing', ?, ?)",
        )
        .bind(&id)
        .bind(
            serde_json::json!({
                "version": "1.2.3.4",
                "port": 19132,
                "port_v6": 19133,
                "eula_accepted": true
            })
            .to_string(),
        )
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        tokio::fs::create_dir_all(state.settings.instances_dir().join(&id))
            .await
            .unwrap();
        let (job, created) = jobs::create(&state.pool, &id, "install", owner_id, None)
            .await
            .unwrap();
        assert!(created);
        assert!(jobs::begin(&state.pool, &job.id).await.unwrap());
        jobs::wait_for_user(
            &state.pool,
            &job.id,
            serde_json::json!({
                "job_id": &job.id,
                "interaction": {"kind": "bedrock_archive_upload"}
            }),
        )
        .await
        .unwrap();
        (id, jobs::get(&state.pool, &job.id).await.unwrap())
    }

    async fn insert_admin_session(state: &AppState) -> (String, String) {
        let user_id = uuid::Uuid::new_v4().to_string();
        let session = format!("admin-session-{}", uuid::Uuid::new_v4());
        let csrf = format!("admin-csrf-{}", uuid::Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, ?, 'unused', 'admin', ?, ?)",
        )
        .bind(&user_id)
        .bind(format!("admin-{user_id}"))
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(user_id)
        .bind(token_hash(&session))
        .bind(token_hash(&csrf))
        .bind((Utc::now() + Duration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        (session, csrf)
    }

    fn token_hash(value: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
    }

    fn router(state: AppState) -> Router {
        Router::new()
            .nest("/api/v1", crate::api::routes(state.clone()))
            .with_state(state)
    }

    fn authenticated_request(
        method: &str,
        uri: String,
        session: &str,
        csrf: &str,
        body: Body,
    ) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, format!("dmx_session={session}"))
            .header("x-csrf-token", csrf)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .unwrap()
    }

    async fn wait_for_job(state: &AppState, job_id: &str) -> String {
        for _ in 0..400 {
            let status: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(&state.pool)
                .await
                .unwrap();
            if matches!(
                status.as_str(),
                "succeeded" | "failed" | "cancelled" | "interrupted"
            ) {
                return status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("import job timed out")
    }

    #[tokio::test]
    async fn copy_is_atomic_and_cannot_modify_another_instance() {
        use axum::body::to_bytes;

        let (state, import_root, session, csrf) = test_state().await;
        let imported = insert_valheim_instance(&state, "imported").await;
        let untouched = insert_valheim_instance(&state, "untouched").await;
        let source = import_root.join("legacy");
        tokio::fs::create_dir_all(&source).await.unwrap();
        let executable = if cfg!(windows) {
            "valheim_server.exe"
        } else {
            "valheim_server.x86_64"
        };
        tokio::fs::write(source.join(executable), b"fixture")
            .await
            .unwrap();
        tokio::fs::write(source.join("world.db"), b"world")
            .await
            .unwrap();

        let response = router(state.clone())
            .oneshot(authenticated_request(
                "POST",
                format!("/api/v1/servers/{imported}/imports/copy"),
                &session,
                &csrf,
                Body::from(serde_json::json!({"source_path": source}).to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: Job = serde_json::from_slice(&body).unwrap();
        assert_eq!(wait_for_job(&state, &job.id).await, "succeeded");

        let states: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, installation_state FROM instances WHERE id IN (?, ?) ORDER BY id",
        )
        .bind(&imported)
        .bind(&untouched)
        .fetch_all(&state.pool)
        .await
        .unwrap();
        assert_eq!(
            states.iter().find(|(id, _)| id == &imported).unwrap().1,
            "installed"
        );
        assert_eq!(
            states.iter().find(|(id, _)| id == &untouched).unwrap().1,
            "not_installed"
        );
        assert_eq!(
            tokio::fs::read(
                state
                    .settings
                    .instances_dir()
                    .join(&imported)
                    .join("game/world.db")
            )
            .await
            .unwrap(),
            b"world"
        );
        assert!(
            !tokio::fs::try_exists(
                state
                    .settings
                    .instances_dir()
                    .join(untouched)
                    .join("game/world.db")
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn zip_upload_returns_an_idempotent_job_and_installs_from_staging() {
        use axum::body::to_bytes;

        let (state, _import_root, session, csrf) = test_state().await;
        let instance_id = insert_valheim_instance(&state, "zip-import").await;
        let executable = if cfg!(windows) {
            "valheim_server.exe"
        } else {
            "valheim_server.x86_64"
        };
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        writer
            .start_file(executable, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer
            .start_file("world.db", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"zip-world").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let request = || {
            Request::post(format!("/api/v1/servers/{instance_id}/imports/zip"))
                .header(header::COOKIE, format!("dmx_session={session}"))
                .header("x-csrf-token", &csrf)
                .header("idempotency-key", "zip-import-fixture")
                .header(header::CONTENT_TYPE, "application/zip")
                .body(Body::from(archive.clone()))
                .unwrap()
        };

        let application = router(state.clone());
        let response = application.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: Job = serde_json::from_slice(&body).unwrap();
        let duplicate = application.clone().oneshot(request()).await.unwrap();
        assert_eq!(duplicate.status(), StatusCode::ACCEPTED);
        let duplicate = to_bytes(duplicate.into_body(), 64 * 1024).await.unwrap();
        let duplicate: Job = serde_json::from_slice(&duplicate).unwrap();
        assert_eq!(duplicate.id, job.id);
        assert_eq!(wait_for_job(&state, &job.id).await, "succeeded");
        let completed_replay = application.oneshot(request()).await.unwrap();
        assert_eq!(completed_replay.status(), StatusCode::ACCEPTED);
        let completed_replay = to_bytes(completed_replay.into_body(), 64 * 1024)
            .await
            .unwrap();
        let completed_replay: Job = serde_json::from_slice(&completed_replay).unwrap();
        assert_eq!(completed_replay.id, job.id);
        assert_eq!(
            tokio::fs::read(
                state
                    .settings
                    .instances_dir()
                    .join(instance_id)
                    .join("game/world.db")
            )
            .await
            .unwrap(),
            b"zip-world"
        );
    }

    #[tokio::test]
    async fn bedrock_archive_requires_owner_and_exact_sha256_on_the_waiting_install_job() {
        let (state, _import_root, owner_session, owner_csrf) = test_state().await;
        let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE role_id = 'owner'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        let (instance_id, job) = insert_waiting_bedrock_install(&state, &owner_id).await;
        let archive = b"fixture archive".to_vec();
        let sha256 = format!("{:x}", Sha256::digest(&archive));
        let (admin_session, admin_csrf) = insert_admin_session(&state).await;
        let application = router(state.clone());

        let denied = application
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/servers/{instance_id}/imports/zip"))
                    .header(header::COOKIE, format!("dmx_session={admin_session}"))
                    .header("x-csrf-token", admin_csrf)
                    .header("x-dmx-archive-sha256", &sha256)
                    .header(header::CONTENT_TYPE, "application/zip")
                    .body(Body::from(archive.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let missing_digest = application
            .oneshot(
                Request::post(format!("/api/v1/servers/{instance_id}/imports/zip"))
                    .header(header::COOKIE, format!("dmx_session={owner_session}"))
                    .header("x-csrf-token", owner_csrf)
                    .header(header::CONTENT_TYPE, "application/zip")
                    .body(Body::from(archive))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_digest.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            jobs::get(&state.pool, &job.id).await.unwrap().state,
            JobState::WaitingForUser
        );
    }

    #[test]
    fn valid_bedrock_archive_requeues_the_same_job_and_audits_only_verified_metadata() {
        use axum::body::to_bytes;

        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_stack_size(crate::TOKIO_WORKER_STACK_BYTES)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (state, _import_root, owner_session, owner_csrf) = test_state().await;
                let owner_id: String =
                    sqlx::query_scalar("SELECT id FROM users WHERE role_id = 'owner'")
                        .fetch_one(&state.pool)
                        .await
                        .unwrap();
                let (instance_id, waiting_job) =
                    insert_waiting_bedrock_install(&state, &owner_id).await;
                let cursor = std::io::Cursor::new(Vec::new());
                let mut writer = ZipWriter::new(cursor);
                writer
                    .start_file("bedrock_server", SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(b"fixture").unwrap();
                let archive = writer.finish().unwrap().into_inner();
                let sha256 = format!("{:x}", Sha256::digest(&archive));
                let response = router(state.clone())
                    .oneshot(
                        Request::post(format!("/api/v1/servers/{instance_id}/imports/zip"))
                            .header(header::COOKIE, format!("dmx_session={owner_session}"))
                            .header("x-csrf-token", owner_csrf)
                            .header("x-dmx-archive-sha256", &sha256)
                            .header(header::CONTENT_TYPE, "application/zip")
                            .body(Body::from(archive.clone()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::ACCEPTED);
                let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
                let resumed: Job = serde_json::from_slice(&body).unwrap();
                assert_eq!(resumed.id, waiting_job.id);

                let audit_metadata: String = sqlx::query_scalar(
            "SELECT metadata FROM audit_events WHERE action = 'server.bedrock_archive_uploaded' \
             AND resource_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(&instance_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
                let audit_metadata: serde_json::Value =
                    serde_json::from_str(&audit_metadata).unwrap();
                assert_eq!(
                    audit_metadata
                        .get("sha256")
                        .and_then(serde_json::Value::as_str),
                    Some(sha256.as_str())
                );
                assert_eq!(
                    audit_metadata
                        .get("size_bytes")
                        .and_then(serde_json::Value::as_u64),
                    Some(archive.len() as u64)
                );
                assert_eq!(
                    audit_metadata.get("eula_accepted"),
                    Some(&serde_json::Value::Bool(true))
                );
                assert!(audit_metadata.get("url").is_none());

                // The endpoint only requeues the persistent install job. Wait for the actor to
                // leave its install future, then shut it down explicitly: dropping a Tokio runtime
                // while that large future is being polled makes this test teardown timing-dependent.
                assert!(matches!(
                    wait_for_job(&state, &waiting_job.id).await.as_str(),
                    "succeeded" | "failed"
                ));
                state.runtime.shutdown().await;
            });
    }

    #[tokio::test]
    async fn a_waiting_bedrock_install_can_be_cancelled_without_leaving_install_state() {
        use axum::body::to_bytes;

        let (state, _import_root, owner_session, owner_csrf) = test_state().await;
        let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE role_id = 'owner'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        let (instance_id, waiting_job) = insert_waiting_bedrock_install(&state, &owner_id).await;
        let response = router(state.clone())
            .oneshot(authenticated_request(
                "POST",
                format!("/api/v1/jobs/{}/cancel", waiting_job.id),
                &owner_session,
                &owner_csrf,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let cancelled: Job = serde_json::from_slice(&body).unwrap();
        assert_eq!(cancelled.id, waiting_job.id);
        assert_eq!(cancelled.state, JobState::Cancelled);
        let installation_state: String =
            sqlx::query_scalar("SELECT installation_state FROM instances WHERE id = ?")
                .bind(instance_id)
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(installation_state, "not_installed");
    }

    #[tokio::test]
    async fn attached_data_is_detached_without_deletion() {
        use axum::body::to_bytes;

        let (state, import_root, session, csrf) = test_state().await;
        let instance_id = insert_valheim_instance(&state, "attached").await;
        let source = import_root.join("attached-root");
        tokio::fs::create_dir_all(source.join("game"))
            .await
            .unwrap();
        let executable = if cfg!(windows) {
            "valheim_server.exe"
        } else {
            "valheim_server.x86_64"
        };
        tokio::fs::write(source.join("game").join(executable), b"fixture")
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(
                source.join("game").join(executable),
                fs::Permissions::from_mode(0o700),
            )
            .await
            .unwrap();
        }
        tokio::fs::write(source.join("game/world.db"), b"keep")
            .await
            .unwrap();

        let admin_id = uuid::Uuid::new_v4().to_string();
        let admin_session = "import-admin-session";
        let admin_csrf = "import-admin-csrf";
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'import-admin', 'unused', 'admin', ?, ?)",
        )
        .bind(&admin_id)
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, csrf_hash, expires_at, created_at, last_seen_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(admin_id)
        .bind(token_hash(admin_session))
        .bind(token_hash(admin_csrf))
        .bind((Utc::now() + Duration::hours(1)).to_rfc3339())
        .bind(&now)
        .bind(&now)
        .execute(&state.pool)
        .await
        .unwrap();
        let denied = router(state.clone())
            .oneshot(authenticated_request(
                "POST",
                format!("/api/v1/servers/{instance_id}/imports/attach"),
                admin_session,
                admin_csrf,
                Body::from(serde_json::json!({"source_path": source}).to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let job_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE instance_id = ?")
            .bind(&instance_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(job_count, 0);

        let response = router(state.clone())
            .oneshot(authenticated_request(
                "POST",
                format!("/api/v1/servers/{instance_id}/imports/attach"),
                &session,
                &csrf,
                Body::from(serde_json::json!({"source_path": source}).to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let job: Job = serde_json::from_slice(&body).unwrap();
        assert_eq!(wait_for_job(&state, &job.id).await, "succeeded");
        let storage = instance_storage::resolve(&state.pool, &state.settings, &instance_id)
            .await
            .unwrap();
        assert_eq!(storage.mode, StorageMode::Attached);
        assert_eq!(storage.root, fs::canonicalize(&source).unwrap());

        let response = router(state.clone())
            .oneshot(authenticated_request(
                "DELETE",
                format!("/api/v1/servers/{instance_id}"),
                &session,
                &csrf,
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            tokio::fs::read(source.join("game/world.db")).await.unwrap(),
            b"keep"
        );
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id = ?)")
                .bind(instance_id)
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn dropping_an_unpolled_import_worker_interrupts_its_job() {
        let (state, _import_root, _session, _csrf) = test_state().await;
        let instance_id = insert_valheim_instance(&state, "unpolled-import").await;
        let owner_id: String = sqlx::query_scalar("SELECT id FROM users WHERE role_id = 'owner'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        let (job, created, claim) =
            jobs::create_claimed(&state.pool, &instance_id, "import_copy", &owner_id, None)
                .await
                .unwrap();
        assert!(created);

        let worker = run_job(
            state.clone(),
            job.clone(),
            claim.unwrap(),
            |_state, _job| async { Ok(()) },
        );
        drop(worker);

        assert_eq!(wait_for_job(&state, &job.id).await, "interrupted");
    }
}
