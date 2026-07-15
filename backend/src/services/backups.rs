use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    future::Future,
    io::{self, Read, SeekFrom},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    core::{AppState, DbPool, Settings, database, error::AppError},
    domain::v1::{GameProfile, Job},
    services::{
        installers::{ArchiveLimits, declared_backup_paths_for_profile, extract_zip},
        instance_storage, jobs, notifications,
        secure_fs::{self, BackupFile},
    },
};

pub const MAX_BACKUP_SOURCE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_BACKUP_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_INSTANCE_BACKUP_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const BACKUP_RETENTION_COUNT: usize = 10;
const RESTORE_STATE_FILE: &str = "restore-state.json";
const RESTORE_STATE_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreSwapManifest {
    version: u8,
    entries: Vec<RestoreSwapEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreSwapEntry {
    relative: PathBuf,
    destination_existed: bool,
    staged_existed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Backup {
    pub id: String,
    pub instance_id: String,
    pub kind: String,
    pub status: String,
    pub checksum_sha256: Option<String>,
    pub size_bytes: Option<u64>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct BackupRow {
    id: String,
    instance_id: String,
    creation_job_id: Option<String>,
    kind: String,
    status: String,
    storage_name: String,
    checksum_sha256: Option<String>,
    size_bytes: Option<i64>,
    created_at: String,
    completed_at: Option<String>,
}

#[derive(Debug, FromRow)]
struct BackupInstance {
    profile_id: String,
    profile_revision: i64,
    settings: String,
    runtime_state: String,
}

pub async fn insert(
    pool: &DbPool,
    instance_id: &str,
    creation_job_id: Option<&str>,
    kind: &str,
    created_by: &str,
) -> Result<Backup, AppError> {
    let id = Uuid::new_v4().to_string();
    let storage_name = format!("{id}.zip");
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO backups
            (id, instance_id, creation_job_id, kind, status, storage_name, created_by, created_at)
        VALUES (?, ?, ?, ?, 'creating', ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(instance_id)
    .bind(creation_job_id)
    .bind(kind)
    .bind(storage_name)
    .bind(created_by)
    .bind(created_at)
    .execute(pool)
    .await?;
    get(pool, &id).await
}

pub async fn get(pool: &DbPool, id: &str) -> Result<Backup, AppError> {
    get_row(pool, id).await?.try_into()
}

pub async fn get_by_creation_job(pool: &DbPool, job_id: &str) -> Result<Backup, AppError> {
    let row: BackupRow = sqlx::query_as("SELECT * FROM backups WHERE creation_job_id = ?")
        .bind(job_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("backups.not_found".into()))?;
    row.try_into()
}

pub async fn list(pool: &DbPool, instance_id: &str) -> Result<Vec<Backup>, AppError> {
    let rows: Vec<BackupRow> =
        sqlx::query_as("SELECT * FROM backups WHERE instance_id = ? ORDER BY created_at DESC")
            .bind(instance_id)
            .fetch_all(pool)
            .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn instance_id_for(pool: &DbPool, id: &str) -> Result<String, AppError> {
    sqlx::query_scalar("SELECT instance_id FROM backups WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("backups.not_found".into()))
}

pub async fn open_verified_archive(
    settings: &Settings,
    pool: &DbPool,
    id: &str,
) -> Result<(tokio::fs::File, u64, Backup), AppError> {
    let row = get_row(pool, id).await?;
    if row.status != "ready" {
        return Err(AppError::Conflict("backups.not_ready".into()));
    }
    let expected = row
        .checksum_sha256
        .as_deref()
        .filter(|value| value.len() == 64)
        .ok_or_else(|| AppError::Internal("backup checksum is missing".into()))?;
    let directory = storage_directory(settings, &row.instance_id).await?;
    validate_storage_name(&row.storage_name)?;
    let (mut file, size) = secure_fs::open_regular_file(&directory, &row.storage_name).await?;
    if row.size_bytes.and_then(|value| u64::try_from(value).ok()) != Some(size) {
        return Err(AppError::Conflict("backups.size_mismatch".into()));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(AppError::Conflict("backups.checksum_mismatch".into()));
    }
    file.seek(SeekFrom::Start(0)).await?;
    Ok((file, size, row.try_into()?))
}

pub async fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    let row = get_row(&state.pool, id).await?;
    let active_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE instance_id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(&row.instance_id)
    .fetch_one(&state.pool)
    .await?;
    if active_jobs > 0 {
        return Err(AppError::Conflict("jobs.instance_busy".into()));
    }
    let directory = storage_directory(&state.settings, &row.instance_id).await?;
    validate_storage_name(&row.storage_name)?;
    let path = directory.join(&row.storage_name);
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    sqlx::query("DELETE FROM backups WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

pub async fn purge_instance_storage(
    settings: &Settings,
    instance_id: &str,
) -> Result<(), AppError> {
    let id = Uuid::parse_str(instance_id)
        .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    let base = settings.data_dir.join("backups");
    match tokio::fs::symlink_metadata(&base).await {
        Ok(_) => secure_fs::validate_directory_root(&base).await?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    remove_tree_if_exists(&base.join(id.to_string())).await
}

pub async fn recover_interrupted_restores(
    pool: &DbPool,
    settings: &Settings,
) -> Result<(), AppError> {
    let instance_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM instances ORDER BY id")
        .fetch_all(pool)
        .await?;
    for instance_id in instance_ids {
        let root = instance_storage::resolve(pool, settings, &instance_id)
            .await?
            .root;
        let mut entries = match tokio::fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with(".restore-") {
                continue;
            }
            let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(AppError::BadRequest("backups.restore_state_unsafe".into()));
            }
            recover_restore_work(&root, &entry.path()).await?;
        }
    }
    Ok(())
}

pub async fn create_pre_update(
    pool: &DbPool,
    settings: &Settings,
    instance_id: &str,
    requested_by: &str,
    parent_job_id: &str,
) -> Result<Option<String>, AppError> {
    Uuid::parse_str(instance_id).map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    Uuid::parse_str(parent_job_id).map_err(|_| AppError::BadRequest("jobs.invalid_id".into()))?;
    let instance = backup_instance(pool, instance_id).await?;
    if instance.runtime_state != "stopped" {
        return Err(AppError::Conflict("backups.server_must_be_stopped".into()));
    }
    let storage = instance_storage::resolve(pool, settings, instance_id).await?;
    match tokio::fs::symlink_metadata(storage.root.join("game")).await {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(AppError::BadRequest("backups.source_unsafe".into()));
        }
        Ok(_) => {}
    }
    let manifest: String =
        sqlx::query_scalar("SELECT manifest FROM game_profiles WHERE id = ? AND revision = ?")
            .bind(&instance.profile_id)
            .bind(instance.profile_revision)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    let profile: GameProfile = serde_json::from_str(&manifest)
        .map_err(|_| AppError::Internal("stored game profile is invalid".into()))?;
    let instance_settings: serde_json::Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let declared =
        declared_backup_paths_at_root(&profile, &instance_settings, &storage.root).await?;
    if declared.is_empty()
        || secure_fs::scan_backup_files(&storage.root, &declared, MAX_BACKUP_SOURCE_BYTES)
            .await?
            .is_empty()
    {
        return Ok(None);
    }

    let row: BackupRow = match sqlx::query_as::<_, BackupRow>(
        "SELECT * FROM backups WHERE creation_job_id = ?",
    )
    .bind(parent_job_id)
    .fetch_optional(pool)
    .await?
    {
        Some(row) if row.status == "ready" => return Ok(Some(row.id)),
        Some(row) => {
            cleanup_backup_files(settings, &row).await;
            sqlx::query(
                "UPDATE backups SET status = 'creating', checksum_sha256 = NULL, size_bytes = NULL, completed_at = NULL WHERE id = ?",
            )
            .bind(&row.id)
            .execute(pool)
            .await?;
            get_row(pool, &row.id).await?
        }
        None => {
            let backup = insert(
                pool,
                instance_id,
                Some(parent_job_id),
                "pre_update",
                requested_by,
            )
            .await?;
            get_row(pool, &backup.id).await?
        }
    };
    let archive =
        create_archive_payload_for(pool, settings, &profile, &instance_settings, &row, false).await;
    let (size, checksum) = match archive {
        Ok(archive) => archive,
        Err(error) => {
            mark_failed_and_cleanup_parts(pool, settings, &row.id).await;
            return Err(error);
        }
    };
    if let Err(error) = database::audit(
        pool,
        Some(requested_by),
        "backup.pre_update_created",
        "instance",
        Some(instance_id),
        "success",
        serde_json::json!({
            "backup_id": &row.id,
            "parent_job_id": parent_job_id,
        }),
    )
    .await
    {
        mark_failed_and_cleanup_parts(pool, settings, &row.id).await;
        return Err(error);
    }
    if let Err(error) = finalize_archive(pool, &row.id, size, &checksum).await {
        mark_failed_and_cleanup_parts(pool, settings, &row.id).await;
        return Err(error);
    }
    if let Err(error) =
        enforce_retention_parts(pool, settings, instance_id, std::slice::from_ref(&row.id)).await
    {
        tracing::warn!(%error, "failed to enforce retention after pre-update backup");
    }
    Ok(Some(row.id))
}

pub fn spawn_create(state: AppState, job: Job, backup_id: String, claim: jobs::JobClaim) {
    let worker = claimed_backup_worker(claim, move |claim| async move {
        let mut claim = Some(claim);
        match jobs::begin(&state.pool, &job.id).await {
            Ok(true) => {}
            Ok(false) => {
                disarm_terminal_claim(&mut claim, &job.id).await;
                return;
            }
            Err(error) => {
                tracing::error!(job_id = %job.id, %error, "failed to begin backup creation job");
                return;
            }
        }
        let result = create_archive(&state, &backup_id).await;
        match result {
            Ok(()) => {
                if let Some(instance_id) = job.instance_id.as_deref()
                    && let Err(error) =
                        enforce_retention(&state, instance_id, std::slice::from_ref(&backup_id))
                            .await
                {
                    tracing::warn!(%error, "failed to enforce backup retention");
                }
                if let Err(error) = jobs::succeed(&state.pool, &job.id).await {
                    tracing::error!(job_id = %job.id, %error, "failed to complete backup creation job");
                    return;
                }
                disarm_terminal_claim(&mut claim, &job.id).await;
                let _ = database::audit(
                    &state.pool,
                    Some(&job.requested_by),
                    "backup.created",
                    "instance",
                    job.instance_id.as_deref(),
                    "success",
                    serde_json::json!({"backup_id": backup_id}),
                )
                .await;
                if let Err(error) = notifications::create(
                    &state.pool,
                    &state.events,
                    &job.requested_by,
                    "backup.created",
                    "notifications.backup_created",
                    serde_json::json!({
                        "job_id": &job.id,
                        "backup_id": &backup_id,
                        "instance_id": job.instance_id.as_deref(),
                    }),
                )
                .await
                {
                    tracing::warn!(job_id = %job.id, %error, "failed to create backup notification");
                }
                if let Some(instance_id) = job.instance_id {
                    state.events.publish(
                        "backup.created",
                        Some(instance_id),
                        serde_json::json!({"backup_id": backup_id}),
                    );
                }
            }
            Err(error) => {
                tracing::warn!(job_id = %job.id, backup_id, %error, "backup creation failed");
                mark_failed_and_cleanup(&state, &backup_id).await;
                if let Err(update_error) = jobs::fail(
                    &state.pool,
                    &job.id,
                    "backup_failed",
                    "backups.creation_failed",
                )
                .await
                {
                    tracing::error!(job_id = %job.id, %update_error, "failed to persist backup creation failure");
                    return;
                }
                disarm_terminal_claim(&mut claim, &job.id).await;
                let _ = database::audit(
                    &state.pool,
                    Some(&job.requested_by),
                    "backup.created",
                    "instance",
                    job.instance_id.as_deref(),
                    "failure",
                    serde_json::json!({"backup_id": backup_id}),
                )
                .await;
                if let Err(notification_error) = notifications::create(
                    &state.pool,
                    &state.events,
                    &job.requested_by,
                    "backup.failed",
                    "notifications.backup_failed",
                    serde_json::json!({
                        "job_id": &job.id,
                        "backup_id": &backup_id,
                        "instance_id": job.instance_id.as_deref(),
                    }),
                )
                .await
                {
                    tracing::warn!(job_id = %job.id, %notification_error, "failed to create backup notification");
                }
            }
        }
    });
    tokio::spawn(worker);
}

pub fn spawn_restore(state: AppState, job: Job, backup_id: String, claim: jobs::JobClaim) {
    let worker = claimed_backup_worker(claim, move |claim| async move {
        let mut claim = Some(claim);
        match jobs::begin(&state.pool, &job.id).await {
            Ok(true) => {}
            Ok(false) => {
                disarm_terminal_claim(&mut claim, &job.id).await;
                return;
            }
            Err(error) => {
                tracing::error!(job_id = %job.id, %error, "failed to begin backup restore job");
                return;
            }
        }
        let result = restore_archive(&state, &job, &backup_id).await;
        match result {
            Ok(pre_restore_id) => {
                if let Err(error) = jobs::succeed(&state.pool, &job.id).await {
                    tracing::error!(job_id = %job.id, %error, "failed to complete backup restore job");
                    return;
                }
                disarm_terminal_claim(&mut claim, &job.id).await;
                let _ = database::audit(
                    &state.pool,
                    Some(&job.requested_by),
                    "backup.restored",
                    "instance",
                    job.instance_id.as_deref(),
                    "success",
                    serde_json::json!({
                        "backup_id": backup_id,
                        "pre_restore_backup_id": pre_restore_id,
                    }),
                )
                .await;
                if let Err(error) = notifications::create(
                    &state.pool,
                    &state.events,
                    &job.requested_by,
                    "backup.restored",
                    "notifications.backup_restored",
                    serde_json::json!({
                        "job_id": &job.id,
                        "backup_id": &backup_id,
                        "pre_restore_backup_id": &pre_restore_id,
                        "instance_id": job.instance_id.as_deref(),
                    }),
                )
                .await
                {
                    tracing::warn!(job_id = %job.id, %error, "failed to create restore notification");
                }
                if let Some(instance_id) = job.instance_id {
                    state.events.publish(
                        "backup.restored",
                        Some(instance_id),
                        serde_json::json!({"backup_id": backup_id}),
                    );
                }
            }
            Err(error) => {
                tracing::warn!(job_id = %job.id, backup_id, %error, "backup restore failed");
                if let Err(update_error) = jobs::fail(
                    &state.pool,
                    &job.id,
                    "restore_failed",
                    "backups.restore_failed",
                )
                .await
                {
                    tracing::error!(job_id = %job.id, %update_error, "failed to persist backup restore failure");
                    return;
                }
                disarm_terminal_claim(&mut claim, &job.id).await;
                let _ = database::audit(
                    &state.pool,
                    Some(&job.requested_by),
                    "backup.restored",
                    "instance",
                    job.instance_id.as_deref(),
                    "failure",
                    serde_json::json!({"backup_id": backup_id}),
                )
                .await;
                if let Err(notification_error) = notifications::create(
                    &state.pool,
                    &state.events,
                    &job.requested_by,
                    "backup.restore_failed",
                    "notifications.backup_restore_failed",
                    serde_json::json!({
                        "job_id": &job.id,
                        "backup_id": &backup_id,
                        "instance_id": job.instance_id.as_deref(),
                    }),
                )
                .await
                {
                    tracing::warn!(job_id = %job.id, %notification_error, "failed to create restore notification");
                }
            }
        }
    });
    tokio::spawn(worker);
}

async fn claimed_backup_worker<F, Fut>(claim: jobs::JobClaim, operation: F)
where
    F: FnOnce(jobs::JobClaim) -> Fut,
    Fut: Future<Output = ()>,
{
    operation(claim).await;
}

async fn disarm_terminal_claim(claim: &mut Option<jobs::JobClaim>, job_id: &str) {
    let Some(claim) = claim.take() else {
        return;
    };
    if let Err(error) = claim.disarm_terminal().await {
        tracing::error!(%job_id, %error, "failed to disarm terminal job claim");
    }
}

async fn declared_backup_paths_at_root(
    profile: &GameProfile,
    settings: &serde_json::Value,
    root: &Path,
) -> Result<Vec<PathBuf>, AppError> {
    let mut declared = declared_backup_paths_for_profile(profile, settings)?;
    if profile.id.starts_with("minecraft-java-") {
        let properties = root.join("game/server.properties");
        match tokio::fs::symlink_metadata(&properties).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                let (file, size) =
                    secure_fs::open_regular_file(root, "game/server.properties").await?;
                if size > 1024 * 1024 {
                    return Err(AppError::BadRequest(
                        "backups.server_properties_too_large".into(),
                    ));
                }
                let mut contents = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
                file.take(size.saturating_add(1))
                    .read_to_end(&mut contents)
                    .await?;
                if contents.len() as u64 != size {
                    return Err(AppError::BadRequest("backups.source_changed".into()));
                }
                let level_name = minecraft_level_name(&contents)?;
                declared.extend([
                    PathBuf::from("game").join(&level_name),
                    PathBuf::from("game").join(format!("{level_name}_nether")),
                    PathBuf::from("game").join(format!("{level_name}_the_end")),
                ]);
            }
        }
    }
    secure_fs::normalize_declared_paths(&declared)
}

fn minecraft_level_name(properties: &[u8]) -> Result<String, AppError> {
    let text = String::from_utf8_lossy(properties);
    let mut found: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(['#', '!']) {
            continue;
        }
        let Some(separator) = line.find(['=', ':']) else {
            continue;
        };
        if line[..separator].trim() != "level-name" {
            continue;
        }
        let value = line[separator + 1..].trim();
        validate_minecraft_level_name(value)?;
        if found.as_deref().is_some_and(|current| current != value) {
            return Err(AppError::BadRequest("backups.ambiguous_level_name".into()));
        }
        found = Some(value.to_string());
    }
    Ok(found.unwrap_or_else(|| "world".to_string()))
}

fn validate_minecraft_level_name(value: &str) -> Result<(), AppError> {
    let mut components = Path::new(value).components();
    let is_one_normal_component =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if value.is_empty()
        || value.len() > 128
        || !is_one_normal_component
        || value.ends_with(['.', ' '])
        || value.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest("backups.invalid_level_name".into()));
    }
    Ok(())
}

async fn create_archive(state: &AppState, backup_id: &str) -> Result<(), AppError> {
    let state = state.clone();
    let backup_id = backup_id.to_string();
    // The inner task owns the backup lease. Cancelling an HTTP/job waiter must not cancel the
    // thaw/restart sequence and leave the game server frozen after `save-off` or a full stop.
    tokio::spawn(async move { create_archive_inner(&state, &backup_id).await })
        .await
        .map_err(|error| AppError::Internal(format!("backup archive task failed: {error}")))?
}

async fn create_archive_inner(state: &AppState, backup_id: &str) -> Result<(), AppError> {
    let row = get_row(&state.pool, backup_id).await?;
    let lease = state.runtime.begin_backup(&row.instance_id).await?;
    let archive = create_archive_payload(state, &row).await;
    let thaw = state.runtime.end_backup(lease).await;
    let (size, checksum) = match (archive, thaw) {
        (Ok(archive), Ok(())) => archive,
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
        (Err(archive_error), Err(thaw_error)) => {
            tracing::error!(%archive_error, %thaw_error, "backup failed and Minecraft could not be thawed safely");
            return Err(thaw_error);
        }
    };
    finalize_archive(&state.pool, &row.id, size, &checksum).await
}

async fn create_archive_payload(
    state: &AppState,
    row: &BackupRow,
) -> Result<(u64, String), AppError> {
    let instance = backup_instance(&state.pool, &row.instance_id).await?;
    let settings: serde_json::Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let profile_revision = u32::try_from(instance.profile_revision)
        .map_err(|_| AppError::Internal("invalid instance profile revision".into()))?;
    let profile = state
        .profiles
        .get_revision(&instance.profile_id, profile_revision)
        .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    create_archive_payload_for(
        &state.pool,
        &state.settings,
        &profile,
        &settings,
        row,
        row.kind == "manual",
    )
    .await
}

async fn create_archive_payload_for(
    pool: &DbPool,
    settings: &Settings,
    profile: &GameProfile,
    instance_settings: &serde_json::Value,
    row: &BackupRow,
    report_job_progress: bool,
) -> Result<(u64, String), AppError> {
    let root = instance_storage::resolve(pool, settings, &row.instance_id)
        .await?
        .root;
    let declared = declared_backup_paths_at_root(profile, instance_settings, &root).await?;
    if declared.is_empty() {
        return Err(AppError::BadRequest("backups.not_supported".into()));
    }
    let files = secure_fs::scan_backup_files(&root, &declared, MAX_BACKUP_SOURCE_BYTES).await?;
    let source_bytes = files.iter().try_fold(0_u64, |total, file| {
        total
            .checked_add(file.size_bytes)
            .ok_or_else(|| AppError::BadRequest("backups.quota_exceeded".into()))
    })?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM backups WHERE instance_id = ? AND status = 'ready' AND id <> ?",
    )
    .bind(&row.instance_id)
    .bind(&row.id)
    .fetch_one(pool)
    .await?;
    let existing = u64::try_from(existing)
        .map_err(|_| AppError::Internal("invalid backup quota value".into()))?;
    if existing.saturating_add(source_bytes) > MAX_INSTANCE_BACKUP_BYTES {
        return Err(AppError::BadRequest("backups.quota_exceeded".into()));
    }

    if report_job_progress && let Some(job_id) = &row.creation_job_id {
        jobs::progress(pool, job_id, 20).await?;
    }
    let directory = storage_directory(settings, &row.instance_id).await?;
    validate_storage_name(&row.storage_name)?;
    let final_path = directory.join(&row.storage_name);
    let temporary = directory.join(format!("{}.tmp", row.storage_name));
    let archive_result = write_archive(temporary.clone(), final_path.clone(), files).await;
    let (size, checksum) = match archive_result {
        Ok(result) => result,
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
    };
    if report_job_progress && let Some(job_id) = &row.creation_job_id {
        jobs::progress(pool, job_id, 90).await?;
    }
    Ok((size, checksum))
}

async fn finalize_archive(
    pool: &DbPool,
    backup_id: &str,
    size: u64,
    checksum: &str,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE backups SET status = 'ready', checksum_sha256 = ?, size_bytes = ?, completed_at = ? WHERE id = ? AND status = 'creating'",
    )
    .bind(checksum)
    .bind(i64::try_from(size).map_err(|_| AppError::Internal("backup too large".into()))?)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(backup_id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict(
            "backups.invalid_state_transition".into(),
        ));
    }
    Ok(())
}

async fn restore_archive(state: &AppState, job: &Job, backup_id: &str) -> Result<String, AppError> {
    let target = get_row(&state.pool, backup_id).await?;
    let instance_id = job
        .instance_id
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?;
    if target.instance_id != instance_id || target.status != "ready" {
        return Err(AppError::BadRequest(
            "backups.invalid_restore_target".into(),
        ));
    }
    let instance = backup_instance(&state.pool, instance_id).await?;
    if instance.runtime_state != "stopped" {
        return Err(AppError::Conflict("backups.server_must_be_stopped".into()));
    }
    let instance_settings: serde_json::Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let profile_revision = u32::try_from(instance.profile_revision)
        .map_err(|_| AppError::Internal("invalid instance profile revision".into()))?;
    let profile = state
        .profiles
        .get_revision(&instance.profile_id, profile_revision)
        .ok_or_else(|| AppError::Internal("instance references unknown profile".into()))?;
    let root = instance_storage::resolve(&state.pool, &state.settings, instance_id)
        .await?
        .root;
    let current_declared =
        declared_backup_paths_at_root(&profile, &instance_settings, &root).await?;
    if current_declared.is_empty() {
        return Err(AppError::BadRequest("backups.not_supported".into()));
    }

    let pre_restore = insert(
        &state.pool,
        instance_id,
        None,
        "pre_restore",
        &job.requested_by,
    )
    .await?;
    if let Err(error) = create_archive(state, &pre_restore.id).await {
        mark_failed_and_cleanup(state, &pre_restore.id).await;
        return Err(error);
    }
    jobs::progress(&state.pool, &job.id, 35).await?;

    let (verified, _, _) = open_verified_archive(&state.settings, &state.pool, backup_id).await?;
    drop(verified);
    let storage = storage_directory(&state.settings, instance_id).await?;
    validate_storage_name(&target.storage_name)?;
    let archive_path = storage.join(&target.storage_name);
    let work = root.join(format!(".restore-{}", job.id));
    let staging = work.join("staging");
    let rollback = work.join("rollback");
    remove_tree_if_exists(&work).await?;
    tokio::fs::create_dir_all(&staging).await?;
    let restore_result: Result<(), AppError> = async {
        let extracted = extract_zip(
            &archive_path,
            &staging,
            ArchiveLimits {
                max_entries: secure_fs::MAX_BACKUP_ENTRIES,
                max_file_bytes: MAX_BACKUP_SOURCE_BYTES,
                max_total_bytes: MAX_BACKUP_SOURCE_BYTES,
                max_compression_ratio: 250,
            },
            None,
        )
        .await
        .map_err(|error| {
            tracing::warn!(code = error.code, detail = ?error.internal, "backup archive rejected");
            AppError::BadRequest("backups.archive_invalid".into())
        })?;
        let restore_declared =
            declared_backup_paths_at_root(&profile, &instance_settings, &staging).await?;
        for path in extracted {
            let relative = path
                .strip_prefix(&staging)
                .map_err(|_| AppError::BadRequest("backups.archive_invalid".into()))?;
            secure_fs::validate_restore_entry(relative, &restore_declared)?;
        }
        validate_staging_tree(&staging, &restore_declared).await?;
        jobs::progress(&state.pool, &job.id, 65).await?;
        swap_restore_paths(&root, &staging, &rollback, &restore_declared).await
    }
    .await;
    if let Err(error) = remove_tree_if_exists(&work).await {
        tracing::warn!(path = %work.display(), %error, "failed to clean restore staging");
    }
    restore_result?;
    jobs::progress(&state.pool, &job.id, 95).await?;
    if let Err(error) = enforce_retention(
        state,
        instance_id,
        &[target.id.clone(), pre_restore.id.clone()],
    )
    .await
    {
        tracing::warn!(%error, "failed to enforce backup retention after restore");
    }
    Ok(pre_restore.id)
}

async fn write_archive(
    temporary: PathBuf,
    final_path: PathBuf,
    files: Vec<BackupFile>,
) -> Result<(u64, String), AppError> {
    tokio::task::spawn_blocking(move || {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let output = options.open(&temporary)?;
        let mut writer = ZipWriter::new(output);
        for file in files {
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .large_file(file.size_bytes > u32::MAX as u64)
                .unix_permissions(0o600);
            writer
                .start_file(&file.archive_name, options)
                .map_err(|error| AppError::Internal(error.to_string()))?;
            let source = secure_fs::open_backup_source(&file)?;
            let copied = io::copy(&mut source.take(file.size_bytes), &mut writer)?;
            if copied != file.size_bytes {
                return Err(AppError::BadRequest("backups.source_changed".into()));
            }
        }
        let output = writer
            .finish()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        output.sync_all()?;
        let size = output.metadata()?.len();
        if size > MAX_BACKUP_ARCHIVE_BYTES {
            return Err(AppError::BadRequest("backups.quota_exceeded".into()));
        }
        drop(output);
        let checksum = checksum_file(&temporary)?;
        fs::rename(&temporary, &final_path)?;
        Ok((size, checksum))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

fn checksum_file(path: &Path) -> Result<String, AppError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn validate_staging_tree(staging: &Path, declared: &[PathBuf]) -> Result<(), AppError> {
    let staging = staging.to_path_buf();
    let declared = declared.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut pending = vec![staging.clone()];
        let mut count = 0_usize;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                count += 1;
                if count > secure_fs::MAX_BACKUP_ENTRIES {
                    return Err(AppError::BadRequest("backups.archive_invalid".into()));
                }
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir())
                {
                    return Err(AppError::BadRequest("backups.archive_invalid".into()));
                }
                let relative = entry
                    .path()
                    .strip_prefix(&staging)
                    .map_err(|_| AppError::BadRequest("backups.archive_invalid".into()))?
                    .to_path_buf();
                validate_restore_tree_entry(&relative, metadata.is_dir(), &declared)?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

fn validate_restore_tree_entry(
    relative: &Path,
    is_directory: bool,
    declared: &[PathBuf],
) -> Result<(), AppError> {
    if secure_fs::validate_restore_entry(relative, declared).is_ok()
        || (is_directory
            && declared
                .iter()
                .any(|allowed| allowed != relative && allowed.starts_with(relative)))
    {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "backups.archive_path_not_allowed".into(),
        ))
    }
}

async fn swap_restore_paths(
    root: &Path,
    staging: &Path,
    rollback: &Path,
    declared: &[PathBuf],
) -> Result<(), AppError> {
    let work = staging
        .parent()
        .ok_or_else(|| AppError::Internal("restore staging has no work directory".into()))?;
    let mut manifest_entries = Vec::with_capacity(declared.len());
    for relative in declared {
        let destination = safe_restore_join(root, relative)?;
        let staged = safe_restore_join(staging, relative)?;
        manifest_entries.push(RestoreSwapEntry {
            relative: relative.clone(),
            destination_existed: path_exists_no_link_error(&destination).await?,
            staged_existed: path_exists_no_link_error(&staged).await?,
        });
    }
    write_restore_manifest(
        work,
        &RestoreSwapManifest {
            version: RESTORE_STATE_VERSION,
            entries: manifest_entries,
        },
    )
    .await?;
    tokio::fs::create_dir_all(rollback).await?;
    let mut processed = Vec::<(PathBuf, PathBuf, bool)>::new();
    for relative in declared {
        let destination = root.join(relative);
        let staged = staging.join(relative);
        let previous = rollback.join(relative);
        let destination_exists = match tokio::fs::try_exists(&destination).await {
            Ok(value) => value,
            Err(error) => {
                rollback_paths(&processed).await;
                return Err(error.into());
            }
        };
        let staged_exists = match tokio::fs::try_exists(&staged).await {
            Ok(value) => value,
            Err(error) => {
                rollback_paths(&processed).await;
                return Err(error.into());
            }
        };
        if destination_exists
            && let Some(parent) = previous.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            rollback_paths(&processed).await;
            return Err(error.into());
        }
        if staged_exists
            && let Some(parent) = destination.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            rollback_paths(&processed).await;
            return Err(error.into());
        }
        if destination_exists && let Err(error) = tokio::fs::rename(&destination, &previous).await {
            rollback_paths(&processed).await;
            return Err(error.into());
        }
        if staged_exists && let Err(error) = tokio::fs::rename(&staged, &destination).await {
            if destination_exists {
                let _ = tokio::fs::rename(&previous, &destination).await;
            }
            rollback_paths(&processed).await;
            return Err(error.into());
        }
        processed.push((destination, previous, staged_exists));
    }
    Ok(())
}

async fn rollback_paths(processed: &[(PathBuf, PathBuf, bool)]) {
    for (destination, previous, installed) in processed.iter().rev() {
        if *installed {
            let _ = remove_tree_if_exists(destination).await;
        }
        if tokio::fs::try_exists(previous).await.unwrap_or(false) {
            let _ = tokio::fs::rename(previous, destination).await;
        }
    }
}

async fn path_exists_no_link_error(path: &Path) -> Result<bool, AppError> {
    tokio::fs::try_exists(path).await.map_err(Into::into)
}

fn safe_restore_join(root: &Path, relative: &Path) -> Result<PathBuf, AppError> {
    let relative = relative
        .to_str()
        .ok_or_else(|| AppError::BadRequest("backups.restore_state_unsafe".into()))?;
    crate::domain::v1::safe_join(root, relative)
        .map_err(|_| AppError::BadRequest("backups.restore_state_unsafe".into()))
}

async fn write_restore_manifest(
    work: &Path,
    manifest: &RestoreSwapManifest,
) -> Result<(), AppError> {
    let encoded =
        serde_json::to_vec(manifest).map_err(|error| AppError::Internal(error.to_string()))?;
    let temporary = work.join(format!(".{RESTORE_STATE_FILE}.tmp"));
    let destination = work.join(RESTORE_STATE_FILE);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await?;
    file.write_all(&encoded).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, &destination).await?;
    if let Ok(directory) = tokio::fs::File::open(work).await {
        let _ = directory.sync_all().await;
    }
    Ok(())
}

async fn recover_restore_work(root: &Path, work: &Path) -> Result<(), AppError> {
    let manifest_path = work.join(RESTORE_STATE_FILE);
    let encoded = match tokio::fs::read(&manifest_path).await {
        Ok(encoded) if encoded.len() <= 1024 * 1024 => encoded,
        Ok(_) => return Err(AppError::BadRequest("backups.restore_state_unsafe".into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            remove_tree_if_exists(work).await?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let manifest: RestoreSwapManifest = serde_json::from_slice(&encoded)
        .map_err(|_| AppError::BadRequest("backups.restore_state_unsafe".into()))?;
    if manifest.version != RESTORE_STATE_VERSION
        || manifest.entries.len() > secure_fs::MAX_BACKUP_ENTRIES
    {
        return Err(AppError::BadRequest("backups.restore_state_unsafe".into()));
    }
    let staging = work.join("staging");
    let rollback = work.join("rollback");
    let normalized = secure_fs::normalize_declared_paths(
        &manifest
            .entries
            .iter()
            .map(|entry| entry.relative.clone())
            .collect::<Vec<_>>(),
    )?;
    if normalized.len() != manifest.entries.len() {
        return Err(AppError::BadRequest("backups.restore_state_unsafe".into()));
    }
    for entry in manifest.entries.iter().rev() {
        if !normalized.contains(&entry.relative) {
            return Err(AppError::BadRequest("backups.restore_state_unsafe".into()));
        }
        let destination = safe_restore_join(root, &entry.relative)?;
        let previous = safe_restore_join(&rollback, &entry.relative)?;
        let staged = safe_restore_join(&staging, &entry.relative)?;
        let previous_exists = tokio::fs::try_exists(&previous).await?;
        if entry.destination_existed {
            if previous_exists {
                remove_tree_if_exists(&destination).await?;
                if let Some(parent) = destination.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::rename(&previous, &destination).await?;
            }
        } else if entry.staged_existed
            && !tokio::fs::try_exists(&staged).await?
            && tokio::fs::try_exists(&destination).await?
        {
            remove_tree_if_exists(&destination).await?;
        }
    }
    remove_tree_if_exists(work).await
}

async fn enforce_retention(
    state: &AppState,
    instance_id: &str,
    keep: &[String],
) -> Result<(), AppError> {
    enforce_retention_parts(&state.pool, &state.settings, instance_id, keep).await
}

async fn enforce_retention_parts(
    pool: &DbPool,
    settings: &Settings,
    instance_id: &str,
    keep: &[String],
) -> Result<(), AppError> {
    let rows: Vec<BackupRow> = sqlx::query_as(
        "SELECT * FROM backups WHERE instance_id = ? AND status = 'ready' ORDER BY created_at DESC",
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await?;
    let keep = keep.iter().collect::<BTreeSet<_>>();
    let directory = storage_directory(settings, instance_id).await?;
    for row in rows.into_iter().skip(BACKUP_RETENTION_COUNT) {
        if keep.contains(&row.id) {
            continue;
        }
        validate_storage_name(&row.storage_name)?;
        match tokio::fs::remove_file(directory.join(&row.storage_name)).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(backup_id = %row.id, %error, "failed to enforce backup retention");
                continue;
            }
        }
        sqlx::query("DELETE FROM backups WHERE id = ?")
            .bind(row.id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn storage_directory(settings: &Settings, instance_id: &str) -> Result<PathBuf, AppError> {
    let id = Uuid::parse_str(instance_id)
        .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    let base = settings.data_dir.join("backups");
    tokio::fs::create_dir_all(&base).await?;
    secure_fs::validate_directory_root(&base).await?;
    let directory = base.join(id.to_string());
    tokio::fs::create_dir_all(&directory).await?;
    secure_fs::validate_directory_root(&directory).await?;
    #[cfg(unix)]
    tokio::fs::set_permissions(
        &directory,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
    )
    .await?;
    Ok(directory)
}

fn validate_storage_name(name: &str) -> Result<(), AppError> {
    let Some(stem) = name.strip_suffix(".zip") else {
        return Err(AppError::Internal("invalid backup storage name".into()));
    };
    Uuid::parse_str(stem)
        .map(|_| ())
        .map_err(|_| AppError::Internal("invalid backup storage name".into()))
}

async fn backup_instance(pool: &DbPool, id: &str) -> Result<BackupInstance, AppError> {
    sqlx::query_as(
        "SELECT profile_id, profile_revision, settings, runtime_state FROM instances WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))
}

async fn get_row(pool: &DbPool, id: &str) -> Result<BackupRow, AppError> {
    Uuid::parse_str(id).map_err(|_| AppError::BadRequest("backups.invalid_id".into()))?;
    sqlx::query_as("SELECT * FROM backups WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("backups.not_found".into()))
}

async fn mark_failed(pool: &DbPool, id: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE backups SET status = 'failed', completed_at = ? WHERE id = ? AND status = 'creating'",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_failed_and_cleanup(state: &AppState, id: &str) {
    mark_failed_and_cleanup_parts(&state.pool, &state.settings, id).await;
}

async fn mark_failed_and_cleanup_parts(pool: &DbPool, settings: &Settings, id: &str) {
    if let Ok(row) = get_row(pool, id).await {
        cleanup_backup_files(settings, &row).await;
    }
    if let Err(error) = mark_failed(pool, id).await {
        tracing::warn!(backup_id = id, %error, "failed to mark backup as failed");
    }
}

async fn cleanup_backup_files(settings: &Settings, row: &BackupRow) {
    if validate_storage_name(&row.storage_name).is_err() {
        return;
    }
    let Ok(directory) = storage_directory(settings, &row.instance_id).await else {
        return;
    };
    for name in [&row.storage_name, &format!("{}.tmp", row.storage_name)] {
        match tokio::fs::remove_file(directory.join(name)).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(backup_id = %row.id, %error, "failed to clean failed backup file");
            }
        }
    }
}

async fn remove_tree_if_exists(path: &Path) -> Result<(), AppError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            tokio::fs::remove_file(path).await?;
        }
        Ok(metadata) if metadata.is_dir() => {
            tokio::fs::remove_dir_all(path).await?;
        }
        Ok(_) => {
            tokio::fs::remove_file(path).await?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

impl TryFrom<BackupRow> for Backup {
    type Error = AppError;

    fn try_from(row: BackupRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            instance_id: row.instance_id,
            kind: row.kind,
            status: row.status,
            checksum_sha256: row.checksum_sha256,
            size_bytes: row
                .size_bytes
                .map(u64::try_from)
                .transpose()
                .map_err(|_| AppError::Internal("invalid backup size".into()))?,
            created_at: row.created_at,
            completed_at: row.completed_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write as _, net::SocketAddr};

    #[test]
    fn storage_names_are_closed_to_generated_uuid_archives() {
        assert!(validate_storage_name(&format!("{}.zip", Uuid::new_v4())).is_ok());
        for name in ["../secret.zip", "backup.tar", "id.zip/other"] {
            assert!(validate_storage_name(name).is_err());
        }
    }

    #[test]
    fn restore_tree_allows_only_directories_leading_to_declared_paths() {
        let declared = vec![PathBuf::from("game/world")];
        assert!(validate_restore_tree_entry(Path::new("game"), true, &declared).is_ok());
        assert!(validate_restore_tree_entry(Path::new("game/world"), true, &declared).is_ok());
        assert!(
            validate_restore_tree_entry(Path::new("game/world/level.dat"), false, &declared)
                .is_ok()
        );
        assert!(validate_restore_tree_entry(Path::new("game"), false, &declared).is_err());
        assert!(validate_restore_tree_entry(Path::new("other"), true, &declared).is_err());
    }

    #[test]
    fn minecraft_level_name_defaults_to_world_and_accepts_one_safe_component() {
        assert_eq!(minecraft_level_name(b"motd=Hello\n").unwrap(), "world");
        assert_eq!(
            minecraft_level_name(b"# generated\nlevel-name = custom_world\n").unwrap(),
            "custom_world"
        );
    }

    #[test]
    fn minecraft_level_name_rejects_traversal_and_ambiguous_values() {
        for properties in [
            b"level-name=../escape\n".as_slice(),
            b"level-name=world\\escape\n".as_slice(),
            b"level-name=world\nlevel-name=other\n".as_slice(),
        ] {
            assert!(minecraft_level_name(properties).is_err());
        }
    }

    #[tokio::test]
    async fn dropping_an_unpolled_backup_worker_interrupts_its_job() {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/claims.db?mode=rwc", directory.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        crate::services::profiles::ProfileRegistry::builtins()
            .persist_builtins(&pool)
            .await
            .unwrap();
        let owner_id = Uuid::new_v4().to_string();
        let instance_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'backup-claim-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&owner_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances (id, name, profile_id, settings, created_at, updated_at) \
             VALUES (?, 'backup-claim-instance', 'hytale', '{}', ?, ?)",
        )
        .bind(&instance_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        let (job, created, claim) =
            jobs::create_claimed(&pool, &instance_id, "backup.create", &owner_id, None)
                .await
                .unwrap();
        assert!(created);

        let worker = claimed_backup_worker(claim.unwrap(), |_claim| async {});
        drop(worker);

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if jobs::get(&pool, &job.id).await.unwrap().state
                    == crate::domain::v1::JobState::Interrupted
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn minecraft_backup_paths_follow_the_persisted_level_name() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("game")).unwrap();
        fs::write(
            root.join("game/server.properties"),
            b"level-name=custom_world\n",
        )
        .unwrap();
        let profile = crate::services::profiles::ProfileRegistry::builtins()
            .get("minecraft-java-vanilla")
            .unwrap();
        let paths = declared_backup_paths_at_root(
            &profile,
            &serde_json::json!({"version": "1.21.8"}),
            root,
        )
        .await
        .unwrap();

        assert!(paths.contains(&PathBuf::from("game/custom_world")));
        assert!(paths.contains(&PathBuf::from("game/custom_world_nether")));
        assert!(paths.contains(&PathBuf::from("game/custom_world_the_end")));
    }

    #[tokio::test]
    async fn pre_update_backup_is_persistent_and_idempotent_for_its_parent_job() {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/test.db?mode=rwc", directory.path().display());
        let settings = Settings {
            config_file: directory.path().join("config.toml"),
            data_dir: directory.path().to_path_buf(),
            static_dir: directory.path().join("static"),
            bind: SocketAddr::from(([127, 0, 0, 1], 5500)),
            database_url: database_url.clone(),
            master_key_file: directory.path().join("master.key"),
            steamcmd_path: directory.path().join("steamcmd"),
            bedrock_linux_source: None,
            bedrock_windows_source: None,
            import_roots: Vec::new(),
            trusted_proxies: Vec::new(),
            reverse_proxy: false,
            log: "error".into(),
            dev_origin: None,
            setup_token: None,
            session_ttl_hours: 24,
            deployment_mode: crate::core::config::DeploymentMode::Native,
            release_check: None,
        };
        tokio::fs::create_dir_all(settings.instances_dir())
            .await
            .unwrap();
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        crate::services::profiles::ProfileRegistry::builtins()
            .persist_builtins(&pool)
            .await
            .unwrap();
        let owner_id = Uuid::new_v4().to_string();
        let role_id: String = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'Owner'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) VALUES (?, 'owner', 'test', ?, ?, ?)",
        )
        .bind(&owner_id)
        .bind(role_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        let instance_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO instances (id, name, profile_id, settings, installation_state, created_at, updated_at) VALUES (?, 'server', 'minecraft-java-vanilla', ?, 'installed', ?, ?)",
        )
        .bind(&instance_id)
        .bind(serde_json::json!({"version": "1.21.11", "eula_accepted": true}).to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        let world = settings
            .instances_dir()
            .join(&instance_id)
            .join("game/world");
        tokio::fs::create_dir_all(&world).await.unwrap();
        tokio::fs::write(world.join("level.dat"), b"world-before-update")
            .await
            .unwrap();
        let (job, _) = jobs::create(&pool, &instance_id, "install", &owner_id, None)
            .await
            .unwrap();

        let first = create_pre_update(&pool, &settings, &instance_id, &owner_id, &job.id)
            .await
            .unwrap()
            .unwrap();
        let second = create_pre_update(&pool, &settings, &instance_id, &owner_id, &job.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        let row = get_row(&pool, &first).await.unwrap();
        assert_eq!(row.status, "ready");
        assert_eq!(row.kind, "pre_update");
        assert_eq!(row.creation_job_id.as_deref(), Some(job.id.as_str()));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM backups WHERE creation_job_id = ?",)
                .bind(&job.id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn archive_writer_streams_files_and_records_a_checksum() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("world.dat");
        fs::File::create(&source)
            .unwrap()
            .write_all(b"world")
            .unwrap();
        let temporary = directory.path().join("backup.zip.tmp");
        let destination = directory.path().join("backup.zip");
        let (size, checksum) = write_archive(
            temporary,
            destination.clone(),
            vec![BackupFile {
                source,
                archive_name: "game/world/world.dat".into(),
                size_bytes: 5,
            }],
        )
        .await
        .unwrap();
        assert_eq!(size, fs::metadata(destination).unwrap().len());
        assert_eq!(checksum.len(), 64);
    }

    #[tokio::test]
    async fn restore_swap_rolls_back_every_previously_replaced_path_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("instance");
        let staging = directory.path().join("staging");
        let rollback = directory.path().join("rollback");
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two/sub")).unwrap();
        fs::create_dir_all(staging.join("one")).unwrap();
        fs::create_dir_all(staging.join("two/sub")).unwrap();
        fs::write(root.join("one/value"), b"old-one").unwrap();
        fs::write(root.join("two/sub/value"), b"old-two").unwrap();
        fs::write(staging.join("one/value"), b"new-one").unwrap();
        fs::write(staging.join("two/sub/value"), b"new-two").unwrap();
        fs::create_dir_all(&rollback).unwrap();
        fs::write(rollback.join("two"), b"blocks-directory-creation").unwrap();

        let result = swap_restore_paths(
            &root,
            &staging,
            &rollback,
            &[PathBuf::from("one"), PathBuf::from("two/sub")],
        )
        .await;
        assert!(result.is_err());
        assert_eq!(fs::read(root.join("one/value")).unwrap(), b"old-one");
        assert_eq!(fs::read(root.join("two/sub/value")).unwrap(), b"old-two");
    }

    #[tokio::test]
    async fn interrupted_restore_manifest_recovers_before_and_after_staged_rename() {
        for install_staged in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("instance");
            let work = root.join(".restore-job");
            let staging = work.join("staging");
            let rollback = work.join("rollback");
            let relative = PathBuf::from("game/world");
            fs::create_dir_all(root.join(&relative)).unwrap();
            fs::create_dir_all(staging.join(&relative)).unwrap();
            fs::write(root.join(&relative).join("value"), b"old").unwrap();
            fs::write(staging.join(&relative).join("value"), b"new").unwrap();
            write_restore_manifest(
                &work,
                &RestoreSwapManifest {
                    version: RESTORE_STATE_VERSION,
                    entries: vec![RestoreSwapEntry {
                        relative: relative.clone(),
                        destination_existed: true,
                        staged_existed: true,
                    }],
                },
            )
            .await
            .unwrap();
            fs::create_dir_all(rollback.join("game")).unwrap();
            fs::rename(root.join(&relative), rollback.join(&relative)).unwrap();
            if install_staged {
                fs::rename(staging.join(&relative), root.join(&relative)).unwrap();
            }

            recover_restore_work(&root, &work).await.unwrap();

            assert_eq!(
                fs::read(root.join(&relative).join("value")).unwrap(),
                b"old"
            );
            assert!(!work.exists());
        }
    }

    #[tokio::test]
    async fn interrupted_restore_removes_a_new_path_that_did_not_exist_before() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("instance");
        let work = root.join(".restore-job");
        let staging = work.join("staging");
        let relative = PathBuf::from("game/new-world");
        fs::create_dir_all(staging.join(&relative)).unwrap();
        fs::write(staging.join(&relative).join("value"), b"new").unwrap();
        write_restore_manifest(
            &work,
            &RestoreSwapManifest {
                version: RESTORE_STATE_VERSION,
                entries: vec![RestoreSwapEntry {
                    relative: relative.clone(),
                    destination_existed: false,
                    staged_existed: true,
                }],
            },
        )
        .await
        .unwrap();
        fs::create_dir_all(root.join("game")).unwrap();
        fs::rename(staging.join(&relative), root.join(&relative)).unwrap();

        recover_restore_work(&root, &work).await.unwrap();

        assert!(!root.join(relative).exists());
        assert!(!work.exists());
    }
}
