use std::{
    collections::HashMap,
    ffi::OsString,
    io::SeekFrom,
    net::{Ipv4Addr, SocketAddrV4},
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use futures::FutureExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot, watch},
};

use crate::{
    core::{DbPool, Settings, database, error::AppError, events::EventHub},
    domain::v1::{Job, JobState, LaunchSpec, SteamProfile, SteamStopStrategy, StopStrategy},
    services::{
        backups, config_files,
        installers::{self, InstallContext, InstallerExecutable},
        instance_storage, jobs, metrics, players, profiles,
        secrets::{SecretStore, allowed_secret_names},
    },
};

const ACTOR_QUEUE_SIZE: usize = 64;
const MAX_CONSOLE_COMMAND: usize = 4 * 1024;
const MAX_LOG_LINE: usize = 16 * 1024;
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;
const LOG_GENERATIONS: usize = 5;
const MAX_CONSOLE_LOG_HISTORY_LINES: usize = 1_000;
const MAX_INSTALL_LOG_HISTORY_LINES: usize = 10_000;
const MAX_CONSOLE_LOG_HISTORY_BYTES: u64 = 1024 * 1024;
const MAX_INSTALL_LOG_HISTORY_BYTES: u64 = MAX_LOG_SIZE;
const PARTIAL_LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const MAX_WATCHDOG_RESTARTS: u8 = 5;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
const READINESS_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEFAULT_READINESS_STABILITY_WINDOW: Duration = Duration::from_secs(5);
const MINECRAFT_SAVE_TIMEOUT: Duration = Duration::from_secs(30);
const HYTALE_UPDATE_EXIT_CODE: i32 = 8;
const HYTALE_UPDATE_STABILITY_WINDOW: Duration = Duration::from_secs(30);
const HYTALE_UPDATE_STATE_FILE: &str = ".hytale-update-state.json";
const HYTALE_UPDATE_STATE_TEMP_FILE: &str = ".hytale-update-state.tmp";
const HYTALE_UPDATE_CANDIDATE: &str = ".hytale-update-candidate";
const HYTALE_UPDATE_ROLLBACK: &str = ".hytale-update-rollback";
const HYTALE_UPDATE_FAILED: &str = ".hytale-update-failed";
const LEASE_RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
const LEASE_RELEASE_RETRY_DELAY: Duration = Duration::from_millis(100);
const GAME_UPDATE_CHECK_TTL: Duration = Duration::from_secs(10 * 60);
const GAME_UPDATE_CHECK_FAILURE_TTL: Duration = Duration::from_secs(60);
const GAME_UPDATE_PROCESS_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_GAME_UPDATE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAction {
    Install,
    Start,
    Stop,
    Restart,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLogSource {
    Install,
    Console,
}

impl RuntimeLogSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Console => "console",
        }
    }

    fn files(self) -> [(&'static str, &'static str); 2] {
        match self {
            Self::Install => [
                ("logs/install.log", "install"),
                ("logs/install.error.log", "install_error"),
            ],
            Self::Console => [
                ("logs/console.log", "console"),
                ("logs/console.error.log", "console_error"),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLogLine {
    pub stream: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GameUpdateState {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    CheckFailed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GameUpdateStatus {
    pub state: GameUpdateState,
    pub installed_version: Option<String>,
    pub installed_build: Option<String>,
    pub available_version: Option<String>,
    pub available_build: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone)]
struct CachedGameUpdateStatus {
    fingerprint: String,
    expires_at: Instant,
    status: GameUpdateStatus,
}

#[derive(Debug)]
pub struct BackupLease {
    sender: mpsc::Sender<ActorCommand>,
    token: Option<String>,
}

#[derive(Debug)]
pub struct FilesystemLease {
    sender: mpsc::Sender<ActorCommand>,
    token: Option<String>,
}

impl FilesystemLease {
    pub async fn release(mut self) -> Result<(), AppError> {
        let Some(token) = self.token.clone() else {
            return Ok(());
        };
        release_filesystem_lease_once(&self.sender, token).await?;
        self.token = None;
        Ok(())
    }
}

impl BackupLease {
    async fn release(mut self) -> Result<(), AppError> {
        let Some(token) = self.token.clone() else {
            return Ok(());
        };
        release_backup_lease_once(&self.sender, token).await?;
        self.token = None;
        Ok(())
    }
}

impl Drop for FilesystemLease {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        let sender = self.sender.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                release_filesystem_lease_retry(sender, token).await;
            });
        } else {
            tracing::error!("filesystem maintenance lease dropped outside a Tokio runtime");
        }
    }
}

impl Drop for BackupLease {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        let sender = self.sender.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                release_backup_lease_retry(sender, token).await;
            });
        } else {
            tracing::error!("backup lease dropped outside a Tokio runtime");
        }
    }
}

async fn release_filesystem_lease_once(
    sender: &mpsc::Sender<ActorCommand>,
    token: String,
) -> Result<(), AppError> {
    let (response_tx, response_rx) = oneshot::channel();
    tokio::time::timeout(
        LEASE_RELEASE_TIMEOUT,
        sender.send(ActorCommand::EndFilesystemMaintenance {
            token,
            response: response_tx,
        }),
    )
    .await
    .map_err(|_| AppError::Internal("runtime actor lease release timed out".into()))?
    .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
    tokio::time::timeout(LEASE_RELEASE_TIMEOUT, response_rx)
        .await
        .map_err(|_| AppError::Internal("runtime actor lease release timed out".into()))?
        .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
}

async fn release_backup_lease_once(
    sender: &mpsc::Sender<ActorCommand>,
    token: String,
) -> Result<(), AppError> {
    let (response_tx, response_rx) = oneshot::channel();
    tokio::time::timeout(
        LEASE_RELEASE_TIMEOUT,
        sender.send(ActorCommand::EndBackup {
            token,
            response: response_tx,
        }),
    )
    .await
    .map_err(|_| AppError::Internal("runtime actor backup release timed out".into()))?
    .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
    tokio::time::timeout(LEASE_RELEASE_TIMEOUT, response_rx)
        .await
        .map_err(|_| AppError::Internal("runtime actor backup release timed out".into()))?
        .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
}

async fn release_filesystem_lease_retry(sender: mpsc::Sender<ActorCommand>, token: String) {
    while !sender.is_closed() {
        match release_filesystem_lease_once(&sender, token.clone()).await {
            Ok(()) => return,
            Err(error) => {
                tracing::warn!(%error, "retrying filesystem maintenance lease release");
                tokio::time::sleep(LEASE_RELEASE_RETRY_DELAY).await;
            }
        }
    }
}

async fn release_backup_lease_retry(sender: mpsc::Sender<ActorCommand>, token: String) {
    while !sender.is_closed() {
        match release_backup_lease_once(&sender, token.clone()).await {
            Ok(()) => return,
            Err(error) => {
                tracing::warn!(%error, "retrying backup lease release");
                tokio::time::sleep(LEASE_RELEASE_RETRY_DELAY).await;
            }
        }
    }
}

impl RuntimeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Kill => "kill",
        }
    }
}

#[derive(Clone)]
pub struct RuntimeManager {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    pool: DbPool,
    settings: Arc<Settings>,
    events: EventHub,
    secrets: SecretStore,
    actors: Mutex<HashMap<String, mpsc::Sender<ActorCommand>>>,
    actor_crash_restarts: Mutex<HashMap<String, u8>>,
    install_cancellations: Mutex<HashMap<String, ActiveInstallCancellation>>,
    game_update_checks: Mutex<HashMap<String, CachedGameUpdateStatus>>,
    game_update_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

struct ActiveInstallCancellation {
    job_id: String,
    signal: watch::Sender<bool>,
    live: bool,
}

impl RuntimeManager {
    pub fn new(
        pool: DbPool,
        settings: Arc<Settings>,
        events: EventHub,
        secrets: SecretStore,
    ) -> Self {
        Self {
            inner: Arc::new(RuntimeInner {
                pool,
                settings,
                events,
                secrets,
                actors: Mutex::new(HashMap::new()),
                actor_crash_restarts: Mutex::new(HashMap::new()),
                install_cancellations: Mutex::new(HashMap::new()),
                game_update_checks: Mutex::new(HashMap::new()),
                game_update_locks: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn game_update_status(
        &self,
        instance_id: &str,
    ) -> Result<GameUpdateStatus, AppError> {
        let instance = load_runtime_instance(&self.inner.pool, instance_id)
            .await
            .map_err(operation_failure_to_app)?;
        let fingerprint = game_update_fingerprint(&instance);
        if let Some(cached) = self
            .inner
            .game_update_checks
            .lock()
            .await
            .get(instance_id)
            .filter(|cached| {
                cached.fingerprint == fingerprint && cached.expires_at > Instant::now()
            })
            .cloned()
        {
            return Ok(cached.status);
        }
        let check_lock = {
            let mut locks = self.inner.game_update_locks.lock().await;
            Arc::clone(
                locks
                    .entry(instance_id.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _check_guard = check_lock.lock().await;
        if let Some(cached) = self
            .inner
            .game_update_checks
            .lock()
            .await
            .get(instance_id)
            .filter(|cached| {
                cached.fingerprint == fingerprint && cached.expires_at > Instant::now()
            })
            .cloned()
        {
            return Ok(cached.status);
        }
        let status = if instance.installation_state != "installed" {
            game_update_status_from_target(&instance, None, None, GameUpdateState::NotInstalled)
        } else {
            match self.resolve_game_update_target(&instance).await {
                Ok((available_version, available_build)) => {
                    let update_available = has_game_update(
                        &instance,
                        available_version.as_deref(),
                        available_build.as_deref(),
                    );
                    game_update_status_from_target(
                        &instance,
                        available_version,
                        available_build,
                        if update_available {
                            GameUpdateState::UpdateAvailable
                        } else {
                            GameUpdateState::UpToDate
                        },
                    )
                }
                Err(error) => {
                    tracing::warn!(
                        instance_id,
                        code = error.code,
                        detail = ?error.internal,
                        "game update check failed"
                    );
                    game_update_status_from_target(
                        &instance,
                        None,
                        None,
                        GameUpdateState::CheckFailed,
                    )
                }
            }
        };
        let ttl = if status.state == GameUpdateState::CheckFailed {
            GAME_UPDATE_CHECK_FAILURE_TTL
        } else {
            GAME_UPDATE_CHECK_TTL
        };
        self.inner.game_update_checks.lock().await.insert(
            instance_id.to_string(),
            CachedGameUpdateStatus {
                fingerprint,
                expires_at: Instant::now() + ttl,
                status: status.clone(),
            },
        );
        Ok(status)
    }

    async fn resolve_game_update_target(
        &self,
        instance: &RuntimeInstance,
    ) -> Result<(Option<String>, Option<String>), OperationFailure> {
        if instance.profile_id == "hytale" {
            return self
                .resolve_hytale_update_version(&instance.id)
                .await
                .map(|version| (Some(version), None));
        }
        if installers::native_install_supported(&instance.profile_id) {
            let settings: Value =
                serde_json::from_str(&instance.settings).map_err(OperationFailure::internal)?;
            let context = InstallContext::official_with_bedrock(&self.inner.settings)
                .map_err(installer_failure)?;
            let target =
                installers::native_update_target(&instance.profile_id, &settings, &context)
                    .await
                    .map_err(installer_failure)?;
            return Ok((Some(target.version), target.build));
        }
        let steam_profile = steam_profile_for_instance(&self.inner.pool, instance).await?;
        let (app_id, branch) = steam_install_target(instance, steam_profile.as_ref())?;
        let build = resolve_steam_available_build(
            &self.inner.settings.steamcmd_path,
            app_id,
            branch.as_deref().unwrap_or("public"),
        )
        .await?;
        Ok((None, Some(build)))
    }

    async fn resolve_hytale_update_version(
        &self,
        instance_id: &str,
    ) -> Result<String, OperationFailure> {
        let checks_root = self
            .inner
            .settings
            .data_dir
            .join("runtime/update-checks")
            .join(instance_id);
        tokio::fs::create_dir_all(&checks_root)
            .await
            .map_err(OperationFailure::internal)?;
        let session = checks_root.join(format!(".hytale-{}", uuid::Uuid::new_v4().as_simple()));
        tokio::fs::create_dir(&session)
            .await
            .map_err(OperationFailure::internal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(OperationFailure::internal)?;
        }
        let result = async {
            let context = InstallContext::official().map_err(installer_failure)?;
            let plan = installers::hytale::prepare_hytale_downloader(&session, &context)
                .await
                .map_err(installer_failure)?;
            if let Some(credentials) = self
                .inner
                .secrets
                .get(
                    &self.inner.pool,
                    instance_id,
                    installers::hytale::DOWNLOADER_CREDENTIAL_SECRET,
                )
                .await
                .map_err(OperationFailure::internal)?
            {
                installers::hytale::write_plaintext_credentials(
                    &plan.credential_file,
                    &credentials,
                )
                .await
                .map_err(installer_failure)?;
            }
            let mut command = Command::new(&plan.executable);
            command
                .current_dir(&plan.cwd)
                .args(plan.version_args())
                .env_clear()
                .envs(filtered_tool_environment())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let output = run_contained_capture(&mut command, GAME_UPDATE_PROCESS_TIMEOUT).await?;
            installers::hytale::parse_printed_version(&output).ok_or_else(|| {
                OperationFailure::new(
                    "hytale_version_invalid",
                    "servers.provider_response_invalid",
                )
            })
        }
        .await;
        remove_dir_if_exists(&session).await.ok();
        result
    }

    pub async fn enqueue(&self, job: Job, action: RuntimeAction) -> Result<(), AppError> {
        self.enqueue_inner(job, action, None).await
    }

    pub async fn enqueue_claimed(
        &self,
        job: Job,
        action: RuntimeAction,
        claim: jobs::JobClaim,
    ) -> Result<(), AppError> {
        self.enqueue_inner(job, action, Some(claim)).await
    }

    async fn enqueue_inner(
        &self,
        job: Job,
        action: RuntimeAction,
        claim: Option<jobs::JobClaim>,
    ) -> Result<(), AppError> {
        let instance_id = job
            .instance_id
            .clone()
            .ok_or_else(|| AppError::BadRequest("jobs.instance_required".into()))?;
        if action == RuntimeAction::Kill
            && let Some(cancellation) = self
                .inner
                .install_cancellations
                .lock()
                .await
                .get(&instance_id)
                .map(|cancellation| cancellation.signal.clone())
        {
            cancellation.send_replace(true);
        }
        let sender = self.actor_for(&instance_id).await;
        sender
            .send(ActorCommand::Execute {
                job: Box::new(job),
                action,
                claim,
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))
    }

    pub async fn send_console(&self, instance_id: &str, command: String) -> Result<(), AppError> {
        validate_console_command(&command)?;
        let sender = self.actor_for(instance_id).await;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(ActorCommand::Console {
                command,
                response: response_tx,
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
        response_rx
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
    }

    pub async fn log_history(
        &self,
        instance_id: &str,
        source: RuntimeLogSource,
        limit: usize,
    ) -> Result<Vec<RuntimeLogLine>, AppError> {
        let storage =
            instance_storage::resolve(&self.inner.pool, &self.inner.settings, instance_id).await?;
        let max_lines = match source {
            RuntimeLogSource::Install => MAX_INSTALL_LOG_HISTORY_LINES,
            RuntimeLogSource::Console => MAX_CONSOLE_LOG_HISTORY_LINES,
        };
        let limit = limit.clamp(1, max_lines);
        if source == RuntimeLogSource::Install {
            let combined = storage.root.join("logs/install.combined.log");
            match tokio::fs::symlink_metadata(&combined).await {
                Ok(_) => {
                    return read_log_tail(
                        &combined,
                        "install",
                        limit,
                        MAX_INSTALL_LOG_HISTORY_BYTES,
                    )
                    .await;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let mut lines = Vec::with_capacity(limit.saturating_mul(2));
        for (relative, stream) in source.files() {
            lines.extend(
                read_log_tail(
                    &storage.root.join(relative),
                    stream,
                    limit,
                    MAX_CONSOLE_LOG_HISTORY_BYTES,
                )
                .await?,
            );
        }
        Ok(lines)
    }

    pub async fn begin_filesystem_maintenance(
        &self,
        instance_id: &str,
    ) -> Result<FilesystemLease, AppError> {
        self.begin_filesystem_maintenance_inner(instance_id, None)
            .await
    }

    pub async fn begin_job_filesystem_maintenance(
        &self,
        instance_id: &str,
        job_id: &str,
    ) -> Result<FilesystemLease, AppError> {
        self.begin_filesystem_maintenance_inner(instance_id, Some(job_id.to_string()))
            .await
    }

    async fn begin_filesystem_maintenance_inner(
        &self,
        instance_id: &str,
        allowed_job_id: Option<String>,
    ) -> Result<FilesystemLease, AppError> {
        let sender = self.actor_for(instance_id).await;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(ActorCommand::BeginFilesystemMaintenance {
                allowed_job_id,
                response: response_tx,
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
        response_rx
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
    }

    pub async fn begin_backup(&self, instance_id: &str) -> Result<BackupLease, AppError> {
        let sender = self.actor_for(instance_id).await;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(ActorCommand::BeginBackup {
                response: response_tx,
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
        response_rx
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
    }

    pub async fn end_backup(&self, lease: BackupLease) -> Result<(), AppError> {
        lease.release().await
    }

    pub async fn request_install_cancel(
        &self,
        instance_id: &str,
        job_id: &str,
    ) -> Result<bool, AppError> {
        let signalled = {
            let cancellations = self.inner.install_cancellations.lock().await;
            if let Some(cancellation) = cancellations
                .get(instance_id)
                .filter(|cancellation| cancellation.live && cancellation.job_id == job_id)
            {
                cancellation.signal.send_replace(true);
                true
            } else {
                false
            }
        };
        if !signalled {
            return Ok(false);
        }
        jobs::request_install_cancel(&self.inner.pool, job_id, instance_id).await
    }

    pub async fn cancel_waiting_install(
        &self,
        instance_id: &str,
        job_id: &str,
    ) -> Result<bool, AppError> {
        {
            let mut cancellations = self.inner.install_cancellations.lock().await;
            if let Some(cancellation) = cancellations.get(instance_id)
                && cancellation.job_id == job_id
                && cancellation.live
            {
                cancellation.signal.send_replace(true);
                drop(cancellations);
                return jobs::request_install_cancel(&self.inner.pool, job_id, instance_id).await;
            }
            if cancellations
                .get(instance_id)
                .is_some_and(|cancellation| cancellation.live && cancellation.job_id != job_id)
            {
                return Err(AppError::Conflict("jobs.instance_busy".into()));
            }
            let (signal, _) = watch::channel(true);
            cancellations.insert(
                instance_id.to_string(),
                ActiveInstallCancellation {
                    job_id: job_id.to_string(),
                    signal,
                    live: false,
                },
            );
        }

        let sender = self.actor_for(instance_id).await;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(ActorCommand::AbortWaitingInstall {
                job_id: job_id.to_string(),
                reason: WaitingInstallAbort::Cancelled,
                response: Some(response_tx),
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
        let aborted = response_rx
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))??;
        if aborted {
            return Ok(true);
        }
        Ok(jobs::get(&self.inner.pool, job_id)
            .await
            .is_ok_and(|job| matches!(job.state, crate::domain::v1::JobState::Cancelled)))
    }

    pub async fn resume_waiting_install(
        &self,
        instance_id: &str,
        job_id: &str,
    ) -> Result<Job, AppError> {
        let sender = self.actor_for(instance_id).await;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(ActorCommand::ResumeWaitingInstall {
                job_id: job_id.to_string(),
                response: response_tx,
            })
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
        response_rx
            .await
            .map_err(|_| AppError::Internal("runtime actor stopped".into()))?
    }

    pub async fn reconcile_boot(&self) -> Result<(), AppError> {
        cleanup_orphaned_hytale_sessions(&self.inner.settings).await?;
        self.reconcile_update_transactions().await?;
        let resumable_install_jobs: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE kind = 'install' AND state = 'queued' ORDER BY created_at",
        )
        .fetch_all(&self.inner.pool)
        .await?;
        for job_id in resumable_install_jobs {
            let job = jobs::get(&self.inner.pool, &job_id).await?;
            self.enqueue(job, RuntimeAction::Install).await?;
        }

        let ids: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT id FROM instances
            WHERE installation_state = 'installed' AND (desired_state = 'running' OR auto_start = 1)
            "#,
        )
        .fetch_all(&self.inner.pool)
        .await?;
        for id in ids {
            sqlx::query(
                "UPDATE instances SET desired_state = 'running', runtime_state = 'stopped', \
                 updated_at = ? WHERE id = ?",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&self.inner.pool)
            .await?;
            let sender = self.actor_for(&id).await;
            if sender.send(ActorCommand::AutoStart).await.is_err() {
                tracing::error!(instance_id = %id, "failed to queue automatic server start");
            }
        }
        Ok(())
    }

    async fn reconcile_update_transactions(&self) -> Result<(), AppError> {
        let transactions: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT t.instance_id, t.job_id, j.state \
             FROM instance_update_transactions t JOIN jobs j ON j.id = t.job_id \
             WHERE j.state IN ('succeeded', 'failed', 'cancelled', 'interrupted') \
             ORDER BY t.created_at",
        )
        .fetch_all(&self.inner.pool)
        .await?;
        for (instance_id, job_id, job_state) in transactions {
            let sender = self.actor_for(&instance_id).await;
            let (response_tx, response_rx) = oneshot::channel();
            sender
                .send(ActorCommand::ReconcileUpdateTransaction {
                    job_id,
                    job_state,
                    response: response_tx,
                })
                .await
                .map_err(|_| AppError::Internal("runtime actor stopped".into()))?;
            response_rx
                .await
                .map_err(|_| AppError::Internal("runtime actor stopped".into()))??;
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        let cancellations = {
            let cancellations = self.inner.install_cancellations.lock().await;
            cancellations
                .values()
                .map(|cancellation| cancellation.signal.clone())
                .collect::<Vec<_>>()
        };
        for cancellation in cancellations {
            cancellation.send_replace(true);
        }
        let senders = {
            let actors = self.inner.actors.lock().await;
            actors.values().cloned().collect::<Vec<_>>()
        };
        let mut responses = Vec::with_capacity(senders.len());
        for sender in senders {
            let (response, receiver) = oneshot::channel();
            if tokio::time::timeout(
                Duration::from_secs(5),
                sender.send(ActorCommand::Shutdown { response }),
            )
            .await
            .is_ok_and(|result| result.is_ok())
            {
                responses.push(receiver);
            }
        }
        futures::future::join_all(
            responses
                .into_iter()
                .map(|receiver| tokio::time::timeout(Duration::from_secs(90), receiver)),
        )
        .await;
    }

    async fn actor_for(&self, instance_id: &str) -> mpsc::Sender<ActorCommand> {
        // The map is held only while looking up or inserting a mailbox. Long-running
        // installs and process waits happen inside the per-instance actor.
        let mut actors = self.inner.actors.lock().await;
        if let Some(sender) = actors.get(instance_id)
            && !sender.is_closed()
        {
            return sender.clone();
        }
        let (sender, receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        actors.insert(instance_id.to_string(), sender.clone());
        let actor = InstanceActor {
            instance_id: instance_id.to_string(),
            inner: Arc::clone(&self.inner),
            sender: sender.clone(),
            process: None,
            generation: 0,
            watchdog_attempts: 0,
            hytale_update_restarts: 0,
            backup_token: None,
            backup_restart_after: false,
            backup_started_stopped: false,
            filesystem_maintenance_token: None,
            filesystem_autostart_pending: false,
            retain_install_rollback: false,
        };
        std::mem::drop(spawn_instance_actor(actor, receiver));
        sender
    }
}

fn spawn_instance_actor(
    actor: InstanceActor,
    receiver: mpsc::Receiver<ActorCommand>,
) -> tokio::task::JoinHandle<()> {
    let inner = Arc::clone(&actor.inner);
    let instance_id = actor.instance_id.clone();
    let sender = actor.sender.clone();
    let mut guard = ActorTaskGuard {
        inner: Arc::clone(&inner),
        instance_id: instance_id.clone(),
        sender: sender.clone(),
        abnormal: true,
        armed: true,
    };
    tokio::spawn(async move {
        let exit = AssertUnwindSafe(actor.run(receiver)).catch_unwind().await;
        let abnormal = match exit {
            Ok(ActorExit::Shutdown) => false,
            Ok(ActorExit::MailboxClosed) => {
                tracing::error!(%instance_id, "instance actor mailbox closed unexpectedly");
                true
            }
            Err(_) => {
                tracing::error!(%instance_id, "instance actor panicked");
                true
            }
        };
        guard.set_abnormal(abnormal);
        finalize_actor_exit(inner, instance_id, sender, abnormal).await;
        guard.disarm();
    })
}

struct ActorTaskGuard {
    inner: Arc<RuntimeInner>,
    instance_id: String,
    sender: mpsc::Sender<ActorCommand>,
    abnormal: bool,
    armed: bool,
}

impl ActorTaskGuard {
    fn set_abnormal(&mut self, abnormal: bool) {
        self.abnormal = abnormal;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ActorTaskGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let instance_id = self.instance_id.clone();
        let sender = self.sender.clone();
        let abnormal = self.abnormal;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                finalize_actor_exit(inner, instance_id, sender, abnormal).await;
            });
        } else {
            tracing::error!(%instance_id, "instance actor supervision dropped outside a Tokio runtime");
        }
    }
}

async fn finalize_actor_exit(
    inner: Arc<RuntimeInner>,
    instance_id: String,
    sender: mpsc::Sender<ActorCommand>,
    abnormal: bool,
) {
    {
        let mut actors = inner.actors.lock().await;
        if actors
            .get(&instance_id)
            .is_some_and(|current| current.same_channel(&sender))
        {
            actors.remove(&instance_id);
        }
    }
    if let Some(cancellation) = inner
        .install_cancellations
        .lock()
        .await
        .remove(&instance_id)
    {
        cancellation.signal.send_replace(true);
    }
    if abnormal {
        if publish_actor_crash(&inner, &instance_id).await {
            schedule_actor_crash_recovery(Arc::clone(&inner), instance_id).await;
        } else {
            inner.actor_crash_restarts.lock().await.remove(&instance_id);
        }
    }
}

async fn publish_actor_crash(inner: &RuntimeInner, instance_id: &str) -> bool {
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(error) =
        sqlx::query("UPDATE instances SET runtime_state = 'crashed', updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(instance_id)
            .execute(&inner.pool)
            .await
    {
        tracing::error!(%instance_id, %error, "failed to persist actor crash state");
        return false;
    }
    let state: Result<Option<(String, String, bool)>, sqlx::Error> = sqlx::query_as(
        "SELECT installation_state, desired_state, watchdog_enabled FROM instances WHERE id = ?",
    )
    .bind(instance_id)
    .fetch_optional(&inner.pool)
    .await;
    match state {
        Ok(Some((installation_state, desired_state, watchdog_enabled))) => {
            let should_recover = desired_state == "running" && watchdog_enabled;
            inner.events.publish(
                "server.state",
                Some(instance_id.to_string()),
                serde_json::json!({
                    "installation_state": installation_state,
                    "desired_state": desired_state,
                    "runtime_state": "crashed",
                }),
            );
            should_recover
        }
        Ok(None) => false,
        Err(error) => {
            tracing::error!(%instance_id, %error, "failed to publish actor crash state");
            false
        }
    }
}

async fn schedule_actor_crash_recovery(inner: Arc<RuntimeInner>, instance_id: String) {
    let attempt = {
        let mut attempts = inner.actor_crash_restarts.lock().await;
        let attempt = attempts.entry(instance_id.clone()).or_default();
        if *attempt >= MAX_WATCHDOG_RESTARTS {
            tracing::error!(%instance_id, attempts = *attempt, "instance actor crash restart limit reached");
            return;
        }
        *attempt += 1;
        *attempt
    };
    let delay = Duration::from_secs(2_u64.pow(u32::from(attempt)).min(60));
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let runtime = RuntimeManager { inner };
        let sender = runtime.actor_for(&instance_id).await;
        if sender.send(ActorCommand::AutoStart).await.is_err() {
            tracing::error!(%instance_id, attempt, "failed to queue actor crash recovery");
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorExit {
    Shutdown,
    MailboxClosed,
}

enum ActorCommand {
    Execute {
        job: Box<Job>,
        action: RuntimeAction,
        claim: Option<jobs::JobClaim>,
    },
    Console {
        command: String,
        response: oneshot::Sender<Result<(), AppError>>,
    },
    BeginBackup {
        response: oneshot::Sender<Result<BackupLease, AppError>>,
    },
    EndBackup {
        token: String,
        response: oneshot::Sender<Result<(), AppError>>,
    },
    BeginFilesystemMaintenance {
        allowed_job_id: Option<String>,
        response: oneshot::Sender<Result<FilesystemLease, AppError>>,
    },
    EndFilesystemMaintenance {
        token: String,
        response: oneshot::Sender<Result<(), AppError>>,
    },
    AbortWaitingInstall {
        job_id: String,
        reason: WaitingInstallAbort,
        response: Option<oneshot::Sender<Result<bool, AppError>>>,
    },
    ResumeWaitingInstall {
        job_id: String,
        response: oneshot::Sender<Result<Job, AppError>>,
    },
    ReconcileUpdateTransaction {
        job_id: String,
        job_state: String,
        response: oneshot::Sender<Result<(), AppError>>,
    },
    Exited {
        generation: u64,
        outcome: ExitOutcome,
    },
    WatchdogRestart,
    ConfirmHytaleUpdate {
        generation: u64,
    },
    ResumeAfterBackup {
        attempt: u8,
    },
    AutoStart,
    Shutdown {
        response: oneshot::Sender<()>,
    },
    #[cfg(test)]
    Panic,
}

struct InstanceActor {
    instance_id: String,
    inner: Arc<RuntimeInner>,
    sender: mpsc::Sender<ActorCommand>,
    process: Option<ManagedProcess>,
    generation: u64,
    watchdog_attempts: u8,
    hytale_update_restarts: u8,
    backup_token: Option<String>,
    backup_restart_after: bool,
    backup_started_stopped: bool,
    filesystem_maintenance_token: Option<String>,
    filesystem_autostart_pending: bool,
    retain_install_rollback: bool,
}

impl InstanceActor {
    async fn run(mut self, mut receiver: mpsc::Receiver<ActorCommand>) -> ActorExit {
        while let Some(command) = receiver.recv().await {
            match command {
                ActorCommand::Execute { job, action, claim } => {
                    self.execute(*job, action, claim).await
                }
                ActorCommand::Console { command, response } => {
                    let result = if self.backup_token.is_some() {
                        Err(AppError::Conflict("backups.server_frozen".into()))
                    } else {
                        self.console(&command).await
                    };
                    let _ = response.send(result);
                }
                ActorCommand::BeginBackup { response } => {
                    let result = self.begin_backup().await.map(|token| BackupLease {
                        sender: self.sender.clone(),
                        token,
                    });
                    let _ = response.send(result);
                }
                ActorCommand::EndBackup { token, response } => {
                    let result = self.end_backup(&token).await;
                    let _ = response.send(result);
                }
                ActorCommand::BeginFilesystemMaintenance {
                    allowed_job_id,
                    response,
                } => {
                    let result = self
                        .begin_filesystem_maintenance(allowed_job_id.as_deref())
                        .await
                        .map(|token| FilesystemLease {
                            sender: self.sender.clone(),
                            token: Some(token),
                        });
                    let _ = response.send(result);
                }
                ActorCommand::EndFilesystemMaintenance { token, response } => {
                    let result = self.end_filesystem_maintenance(&token);
                    let _ = response.send(result);
                }
                ActorCommand::AbortWaitingInstall {
                    job_id,
                    reason,
                    response,
                } => {
                    let result = self.abort_waiting_install(&job_id, reason).await;
                    if let Some(response) = response {
                        let _ = response.send(result);
                    }
                }
                ActorCommand::ResumeWaitingInstall { job_id, response } => {
                    let result = self.resume_waiting_install(&job_id).await;
                    let _ = response.send(result);
                }
                ActorCommand::ReconcileUpdateTransaction {
                    job_id,
                    job_state,
                    response,
                } => {
                    let result = self.reconcile_update_transaction(&job_id, &job_state).await;
                    let _ = response.send(result);
                }
                ActorCommand::Exited {
                    generation,
                    outcome,
                } => self.process_exited(generation, outcome).await,
                ActorCommand::WatchdogRestart => self.watchdog_restart().await,
                ActorCommand::ConfirmHytaleUpdate { generation } => {
                    self.confirm_hytale_update(generation).await
                }
                ActorCommand::ResumeAfterBackup { attempt } => {
                    self.resume_after_backup_with_retry(attempt).await
                }
                ActorCommand::AutoStart => self.auto_start().await,
                ActorCommand::Shutdown { response } => {
                    if self.backup_token.is_some() {
                        let _ = self.release_backup_lease(false).await;
                    }
                    self.filesystem_maintenance_token = None;
                    self.filesystem_autostart_pending = false;
                    if self.stop_process(true, false).await.is_err() {
                        let _ = self.stop_process(true, true).await;
                    }
                    let _ = response.send(());
                    return ActorExit::Shutdown;
                }
                #[cfg(test)]
                ActorCommand::Panic => panic!("injected instance actor panic"),
            }
        }
        ActorExit::MailboxClosed
    }

    async fn execute(&mut self, job: Job, action: RuntimeAction, claim: Option<jobs::JobClaim>) {
        if action == RuntimeAction::Install {
            self.register_install_cancellation(&job.id).await;
        }
        let job_id = job.id.clone();
        self.execute_inner(job, action).await;
        if action == RuntimeAction::Install {
            let _ = self.close_install_cancellation(&job_id).await;
        }
        if let Some(claim) = claim {
            self.settle_job_claim(&job_id, claim).await;
        }
    }

    async fn settle_job_claim(&self, job_id: &str, claim: jobs::JobClaim) {
        let job = match jobs::get(&self.inner.pool, job_id).await {
            Ok(job) => job,
            Err(error) => {
                tracing::error!(%job_id, %error, "failed to inspect runtime job before claim handoff");
                return;
            }
        };
        let result = match job.state {
            JobState::WaitingForUser => claim.disarm_waiting().await,
            JobState::Succeeded
            | JobState::Failed
            | JobState::Cancelled
            | JobState::Interrupted => claim.disarm_terminal().await,
            JobState::Queued | JobState::Running => {
                tracing::error!(%job_id, state = ?job.state, "runtime worker exited while its job remained active");
                return;
            }
        };
        if let Err(error) = result {
            tracing::error!(%job_id, %error, "failed to disarm runtime job claim");
        }
    }

    async fn execute_inner(&mut self, job: Job, action: RuntimeAction) {
        if self.backup_token.is_some() {
            let _ = jobs::fail(
                &self.inner.pool,
                &job.id,
                "server_frozen_for_backup",
                "backups.server_frozen",
            )
            .await;
            self.publish_job(&job.id).await;
            return;
        }
        if self.filesystem_maintenance_token.is_some()
            && matches!(
                action,
                RuntimeAction::Install | RuntimeAction::Start | RuntimeAction::Restart
            )
        {
            let _ = jobs::fail(
                &self.inner.pool,
                &job.id,
                "server_filesystem_maintenance",
                "files.server_must_be_stopped",
            )
            .await;
            self.publish_job(&job.id).await;
            return;
        }
        match jobs::begin(&self.inner.pool, &job.id).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::error!(job_id = %job.id, %error, "failed to begin runtime job");
                return;
            }
        }
        self.publish_job(&job.id).await;
        if matches!(action, RuntimeAction::Start | RuntimeAction::Restart) {
            self.watchdog_attempts = 0;
            self.hytale_update_restarts = 0;
        }

        let result = match action {
            RuntimeAction::Install => self.install_with_restart(&job).await,
            RuntimeAction::Start => {
                async {
                    self.set_runtime_state("starting", Some("running"))
                        .await
                        .map_err(OperationFailure::internal)?;
                    self.start_process().await
                }
                .await
            }
            RuntimeAction::Stop => self.stop_process(true, false).await,
            RuntimeAction::Restart => {
                async {
                    self.stop_process(false, false).await?;
                    self.set_runtime_state("starting", Some("running"))
                        .await
                        .map_err(OperationFailure::internal)?;
                    self.start_process().await
                }
                .await
            }
            RuntimeAction::Kill => self.stop_process(true, true).await,
        };

        match result {
            Ok(()) => {
                let completion = if action == RuntimeAction::Install {
                    self.complete_install_job(&job.id).await
                } else {
                    jobs::succeed(&self.inner.pool, &job.id).await
                };
                if let Err(error) = completion {
                    tracing::error!(job_id = %job.id, %error, "failed to complete job");
                    self.publish_job(&job.id).await;
                    return;
                }
                let _ = database::audit(
                    &self.inner.pool,
                    Some(&job.requested_by),
                    &format!("server.{}", action.as_str()),
                    "instance",
                    Some(&self.instance_id),
                    "success",
                    serde_json::json!({"job_id": job.id}),
                )
                .await;
            }
            Err(error) => {
                if error.deferred {
                    tracing::info!(
                        job_id = %job.id,
                        instance_id = %self.instance_id,
                        code = error.code,
                        "runtime operation is waiting for user input"
                    );
                    self.publish_job(&job.id).await;
                    return;
                }
                if action == RuntimeAction::Install
                    && let Ok(root) = self.instance_root().await
                {
                    let outcome = if error.cancelled {
                        format!("[DMX] Installation cancelled ({}).", error.code)
                    } else {
                        format!(
                            "[DMX] Installation failed ({}): {}.",
                            error.code, error.client_message
                        )
                    };
                    let _ = self.write_install_log(&root, &outcome).await;
                    if !error.cancelled
                        && let Some(detail) = public_install_failure_detail(&error, &root)
                    {
                        let diagnostic = format!("[DMX] Technical detail: {detail}");
                        let _ = self.write_install_log(&root, &diagnostic).await;
                    }
                }
                match action {
                    RuntimeAction::Install => {
                        if error.cancelled {
                            let _ = self.restore_cancelled_initial_install().await;
                        } else if self.instance().await.is_ok_and(|instance| {
                            matches!(
                                instance.installation_state.as_str(),
                                "installing" | "updating"
                            )
                        }) {
                            self.install_failed().await;
                        }
                    }
                    RuntimeAction::Start | RuntimeAction::Restart => {
                        let _ = self.set_runtime_state("crashed", Some("running")).await;
                    }
                    RuntimeAction::Stop | RuntimeAction::Kill => {
                        let state = if self.process.is_some() {
                            "running"
                        } else {
                            "stopped"
                        };
                        let _ = self.set_runtime_state(state, None).await;
                    }
                }
                tracing::warn!(
                    job_id = %job.id,
                    instance_id = %self.instance_id,
                    code = error.code,
                    detail = ?error.internal,
                    "runtime operation failed"
                );
                let update = if error.cancelled {
                    jobs::cancel(&self.inner.pool, &job.id, error.code).await
                } else {
                    jobs::fail(&self.inner.pool, &job.id, error.code, error.client_message).await
                };
                if let Err(update_error) = update {
                    tracing::error!(job_id = %job.id, %update_error, "failed to persist job failure");
                }
                let _ = database::audit(
                    &self.inner.pool,
                    Some(&job.requested_by),
                    &format!("server.{}", action.as_str()),
                    "instance",
                    Some(&self.instance_id),
                    "failure",
                    serde_json::json!({"job_id": job.id, "error_code": error.code}),
                )
                .await;
            }
        }
        self.publish_job(&job.id).await;
    }

    async fn register_install_cancellation(&self, job_id: &str) {
        let mut cancellations = self.inner.install_cancellations.lock().await;
        if let Some(cancellation) = cancellations.get_mut(&self.instance_id)
            && cancellation.job_id == job_id
        {
            cancellation.live = true;
            return;
        }
        let (signal, _) = watch::channel(false);
        cancellations.insert(
            self.instance_id.clone(),
            ActiveInstallCancellation {
                job_id: job_id.to_string(),
                signal,
                live: true,
            },
        );
    }

    async fn install_cancellation_receiver(
        &self,
        job_id: &str,
    ) -> Result<watch::Receiver<bool>, OperationFailure> {
        let cancellations = self.inner.install_cancellations.lock().await;
        let cancellation = cancellations
            .get(&self.instance_id)
            .filter(|cancellation| cancellation.job_id == job_id && cancellation.live)
            .ok_or_else(|| OperationFailure::new("installation_interrupted", "jobs.interrupted"))?;
        Ok(cancellation.signal.subscribe())
    }

    async fn close_install_cancellation(&self, job_id: &str) -> bool {
        let mut cancellations = self.inner.install_cancellations.lock().await;
        if cancellations
            .get(&self.instance_id)
            .is_some_and(|cancellation| cancellation.job_id == job_id)
        {
            let cancellation = cancellations
                .remove(&self.instance_id)
                .expect("matching cancellation remains present");
            return *cancellation.signal.borrow();
        }
        false
    }

    async fn install_cancellation_requested(&self, job_id: &str) -> bool {
        let in_memory = self
            .inner
            .install_cancellations
            .lock()
            .await
            .get(&self.instance_id)
            .is_some_and(|cancellation| {
                cancellation.job_id == job_id && *cancellation.signal.borrow()
            });
        if in_memory {
            return true;
        }
        match jobs::install_cancel_requested(&self.inner.pool, job_id).await {
            Ok(requested) => requested,
            Err(error) => {
                tracing::error!(job_id, %error, "failed to read persisted install cancellation");
                false
            }
        }
    }

    async fn restore_cancelled_initial_install(&self) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE instances SET installation_state = \
             CASE WHEN installed_version IS NULL THEN 'not_installed' ELSE 'installed' END, \
             runtime_state = 'stopped', updated_at = ? WHERE id = ? \
             AND installation_state IN ('installing', 'updating', 'failed')",
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await?;
        self.publish_state().await;
        Ok(())
    }

    async fn abort_waiting_install(
        &mut self,
        job_id: &str,
        reason: WaitingInstallAbort,
    ) -> Result<bool, AppError> {
        let job = jobs::get(&self.inner.pool, job_id).await?;
        if job.instance_id.as_deref() != Some(self.instance_id.as_str()) || job.kind != "install" {
            return Err(AppError::Conflict("jobs.cancellation_unavailable".into()));
        }
        let abortable = match reason {
            WaitingInstallAbort::Cancelled => {
                matches!(job.state, JobState::Queued | JobState::WaitingForUser)
            }
            WaitingInstallAbort::TimedOut => job.state == JobState::WaitingForUser,
        };
        if !abortable {
            let _ = self.close_install_cancellation(job_id).await;
            return Ok(matches!(job.state, JobState::Cancelled));
        }

        let instance = self.instance().await.map_err(operation_failure_to_app)?;
        let desired_state = instance.desired_state.clone();
        let root = self
            .instance_root()
            .await
            .map_err(operation_failure_to_app)?;
        let update = self
            .load_update_transaction()
            .await
            .map_err(operation_failure_to_app)?
            .filter(|transaction| transaction.job_id == job_id);
        if self.process.is_some() {
            self.stop_process(false, true)
                .await
                .map_err(operation_failure_to_app)?;
        }

        if let Some(transaction) = update.as_ref() {
            match transaction.phase.as_str() {
                "preparing" => self
                    .recover_preparing_install_switch(&root, job_id)
                    .await
                    .map_err(operation_failure_to_app)?,
                "committed" | "finalizing" | "rolling_back" => self
                    .restore_previous_game_tree(&root, job_id)
                    .await
                    .map_err(operation_failure_to_app)?,
                "rolled_back" => {}
                _ => {
                    return Err(AppError::Internal(
                        "servers.update_transaction_invalid".into(),
                    ));
                }
            }
            self.restore_update_snapshot_with_desired(
                transaction,
                Some("rolled_back"),
                Some(&desired_state),
            )
            .await
            .map_err(operation_failure_to_app)?;
        } else if job.state == JobState::WaitingForUser {
            if instance.installed_version.is_some() {
                return Err(AppError::Conflict(
                    "servers.update_transaction_incomplete".into(),
                ));
            }
            sqlx::query(
                "UPDATE instances SET installation_state = 'not_installed', \
                 runtime_state = 'stopped', updated_at = ? WHERE id = ? \
                 AND installation_state IN ('installing', 'updating', 'failed')",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&self.instance_id)
            .execute(&self.inner.pool)
            .await?;
            self.publish_state().await;
        }

        remove_dir_if_exists(&root.join(".staging").join(job_id)).await?;
        installers::remove_bedrock_upload(&root, job_id)
            .await
            .map_err(|error| {
                tracing::warn!(code = error.code, detail = ?error.internal, "failed to clean Bedrock upload");
                AppError::Internal("servers.bedrock_archive_cleanup_failed".into())
            })?;

        if update.is_some()
            && desired_state == "running"
            && let Err(error) = self.start_after_update().await
        {
            tracing::error!(
                instance_id = %self.instance_id,
                job_id,
                code = error.code,
                detail = ?error.internal,
                "cancelled update was restored but the previous server could not restart"
            );
            let _ = self.set_runtime_state("crashed", Some("running")).await;
            self.schedule_watchdog().await;
        }
        if update.is_some() {
            self.finish_update_transaction(&root, job_id)
                .await
                .map_err(operation_failure_to_app)?;
        }

        let transitioned = match reason {
            WaitingInstallAbort::Cancelled => {
                jobs::cancel_pending(&self.inner.pool, job_id, "cancelled_by_user").await?
            }
            WaitingInstallAbort::TimedOut => {
                jobs::expire_waiting(
                    &self.inner.pool,
                    job_id,
                    "bedrock_archive_upload_timeout",
                    "servers.bedrock_archive_upload_timeout",
                )
                .await?
            }
        };
        let _ = self.close_install_cancellation(job_id).await;
        if transitioned {
            self.publish_job(job_id).await;
            if reason == WaitingInstallAbort::TimedOut {
                let _ = database::audit(
                    &self.inner.pool,
                    None,
                    "server.install",
                    "instance",
                    Some(&self.instance_id),
                    "failure",
                    serde_json::json!({
                        "job_id": job_id,
                        "error_code": "bedrock_archive_upload_timeout"
                    }),
                )
                .await;
            }
        }
        Ok(transitioned)
    }

    async fn resume_waiting_install(&mut self, job_id: &str) -> Result<Job, AppError> {
        let job = jobs::get(&self.inner.pool, job_id).await?;
        if job.instance_id.as_deref() != Some(self.instance_id.as_str())
            || job.kind != "install"
            || job.state != JobState::WaitingForUser
        {
            return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
        }
        if self
            .inner
            .install_cancellations
            .lock()
            .await
            .get(&self.instance_id)
            .is_some_and(|cancellation| {
                cancellation.job_id == job_id && *cancellation.signal.borrow()
            })
        {
            return Err(AppError::Conflict("jobs.cancellation_in_progress".into()));
        }
        let permit = self
            .sender
            .clone()
            .try_reserve_owned()
            .map_err(|_| AppError::Conflict("jobs.instance_busy".into()))?;
        jobs::requeue_from_user(&self.inner.pool, job_id).await?;
        let job = jobs::get(&self.inner.pool, job_id).await?;
        permit.send(ActorCommand::Execute {
            job: Box::new(job.clone()),
            action: RuntimeAction::Install,
            claim: None,
        });
        Ok(job)
    }

    async fn reconcile_update_transaction(
        &mut self,
        job_id: &str,
        job_state: &str,
    ) -> Result<(), AppError> {
        let Some(transaction) = self
            .load_update_transaction()
            .await
            .map_err(operation_failure_to_app)?
        else {
            return Ok(());
        };
        if transaction.job_id != job_id {
            return Err(AppError::Conflict(
                "servers.update_transaction_incomplete".into(),
            ));
        }
        self.recover_terminal_update_transaction(&transaction, Some(job_state))
            .await
            .map_err(operation_failure_to_app)
    }

    async fn install(&mut self, job: &Job) -> Result<(), OperationFailure> {
        let job_id = job.id.as_str();
        uuid::Uuid::parse_str(job_id).map_err(|_| {
            OperationFailure::new("job_id_invalid", "servers.install_metadata_invalid")
        })?;
        if self.process.is_some() {
            return Err(OperationFailure::new(
                "server_running",
                "servers.must_be_stopped_before_install",
            ));
        }
        let instance = self.instance().await?;
        let root = self.instance_root().await?;
        reset_install_logs(&root).await?;
        self.write_install_log(
            &root,
            &format!(
                "[DMX] Installation job {job_id} started for profile {}.",
                instance.profile_id
            ),
        )
        .await?;
        jobs::progress(&self.inner.pool, job_id, 5)
            .await
            .map_err(OperationFailure::internal)?;
        self.write_install_log(&root, "[DMX] Preparing the pre-installation backup.")
            .await?;
        if let Err(error) = backups::create_pre_update(
            &self.inner.pool,
            &self.inner.settings,
            &self.instance_id,
            &job.requested_by,
            job_id,
        )
        .await
        {
            let _ = self
                .write_install_log(&root, "[DMX] The pre-installation backup failed.")
                .await;
            return Err(OperationFailure::with_internal(
                "pre_update_backup_failed",
                "backups.pre_update_failed",
                error,
            ));
        }
        self.write_install_log(&root, "[DMX] Pre-installation backup completed.")
            .await?;
        if instance.profile_id == "hytale" {
            return self.install_hytale(job_id, &instance).await;
        }
        if installers::native_install_supported(&instance.profile_id) {
            return self.install_native(job_id, &instance).await;
        }
        let steam_profile = steam_profile_for_instance(&self.inner.pool, &instance).await?;
        let (app_id, branch) = steam_install_target(&instance, steam_profile.as_ref())?;
        let installation_state = if matches!(
            instance.installation_state.as_str(),
            "installed" | "updating"
        ) {
            "updating"
        } else {
            "installing"
        };
        sqlx::query(
            "UPDATE instances SET installation_state = ?, runtime_state = 'stopped', \
             updated_at = ? WHERE id = ?",
        )
        .bind(installation_state)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        self.publish_state().await;
        jobs::progress(&self.inner.pool, job_id, 10)
            .await
            .map_err(OperationFailure::internal)?;

        let staging_parent = ensure_staging_parent(&root).await?;
        let staging = staging_parent.join(job_id);
        match tokio::fs::symlink_metadata(&staging).await {
            Ok(_) => {
                if let Err(error) = installers::validate_resumable_staging_tree(&staging).await {
                    tracing::warn!(
                        instance_id = %self.instance_id,
                        code = error.code,
                        "discarding unsafe SteamCMD staging tree"
                    );
                    remove_dir_if_exists(&staging)
                        .await
                        .map_err(OperationFailure::internal)?;
                    tokio::fs::create_dir(&staging)
                        .await
                        .map_err(OperationFailure::internal)?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&staging)
                    .await
                    .map_err(OperationFailure::internal)?;
            }
            Err(error) => return Err(OperationFailure::internal(error)),
        }

        let mut command = Command::new(&self.inner.settings.steamcmd_path);
        self.write_install_log(
            &root,
            &format!("[DMX] Starting SteamCMD anonymous installation for AppID {app_id}."),
        )
        .await?;
        command
            .arg("+force_install_dir")
            .arg(&staging)
            .arg("+login")
            .arg("anonymous")
            .arg("+app_update")
            .arg(app_id.to_string());
        if let Some(branch) = branch {
            command.arg("-beta").arg(branch);
        }
        command
            .arg("validate")
            .arg("+quit")
            .env_clear()
            .envs(filtered_tool_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = match spawn_contained(&mut command) {
            Ok(spawned) => spawned,
            Err(ContainedSpawnError::Spawn(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                self.install_failed().await;
                return Err(OperationFailure::with_internal(
                    "steamcmd_unavailable",
                    "servers.steamcmd_unavailable",
                    error,
                ));
            }
            Err(ContainedSpawnError::Spawn(error)) => {
                self.install_failed().await;
                return Err(OperationFailure::with_internal(
                    "steamcmd_start_failed",
                    "servers.steamcmd_start_failed",
                    error,
                ));
            }
            Err(ContainedSpawnError::Containment(error)) => {
                self.install_failed().await;
                return Err(OperationFailure::with_internal(
                    "process_containment_failed",
                    "servers.process_containment_failed",
                    error,
                ));
            }
        };
        let mut child = spawned.child;
        #[cfg(windows)]
        let windows_job = spawned.windows_job;
        let pid = child.id().ok_or_else(|| {
            OperationFailure::new("steamcmd_start_failed", "servers.steamcmd_start_failed")
        })?;

        #[cfg(windows)]
        let job_handle = Some(windows_job.handle);
        #[cfg(not(windows))]
        let job_handle = None;

        let mut cancellation_rx = self.install_cancellation_receiver(job_id).await?;

        let redactions = Vec::new();
        let combined_log = Arc::new(Mutex::new(
            RotatingLog::open(root.join("logs/install.combined.log"))
                .await
                .map_err(OperationFailure::internal)?,
        ));
        let stdout_task = child.stdout.take().map(|stdout| {
            tokio::spawn(pump_output_observed(
                stdout,
                OutputPumpConfig {
                    log_path: root.join("logs/install.log"),
                    combined_log: Some(combined_log.clone()),
                    stream: "install",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions: redactions.clone(),
                    observer: None,
                    player_observer: None,
                    public_log_policy: PublicLogPolicy::Normal,
                },
            ))
        });
        let stderr_task = child.stderr.take().map(|stderr| {
            tokio::spawn(pump_output_observed(
                stderr,
                OutputPumpConfig {
                    log_path: root.join("logs/install.error.log"),
                    combined_log: Some(combined_log.clone()),
                    stream: "install_error",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions,
                    observer: None,
                    player_observer: None,
                    public_log_policy: PublicLogPolicy::Normal,
                },
            ))
        });
        let (status, mut interrupted) = tokio::select! {
            status = child.wait() => (status, None),
            changed = cancellation_rx.changed() => {
                if changed.is_ok() && *cancellation_rx.borrow() {
                    (terminate_installer(&mut child, pid, job_handle).await, Some(InstallInterruption::Cancelled))
                } else {
                    (child.wait().await, None)
                }
            }
            () = tokio::time::sleep(INSTALL_TIMEOUT) => {
                (terminate_installer(&mut child, pid, job_handle).await, Some(InstallInterruption::TimedOut))
            }
        };
        let status = status.map_err(OperationFailure::internal)?;
        if let Some(task) = stdout_task {
            let _ = task.await;
        }
        if let Some(task) = stderr_task {
            let _ = task.await;
        }
        if self.close_install_cancellation(job_id).await {
            interrupted = Some(InstallInterruption::Cancelled);
        }
        if let Some(interrupted) = interrupted {
            remove_dir_if_exists(&staging).await.ok();
            if !matches!(interrupted, InstallInterruption::Cancelled) {
                self.install_failed().await;
            }
            return match interrupted {
                InstallInterruption::Cancelled => Err(OperationFailure::cancelled(
                    "installation_cancelled",
                    "jobs.cancelled",
                )),
                InstallInterruption::TimedOut => Err(OperationFailure::new(
                    "installation_timeout",
                    "servers.installation_timeout",
                )),
            };
        }
        if !status.success() {
            let exit = status.code().map_or_else(
                || "terminated by signal".to_string(),
                |code| code.to_string(),
            );
            self.write_install_log(
                &root,
                &format!(
                    "[DMX] SteamCMD exited unsuccessfully ({exit}). Checking whether the depot was nevertheless installed completely."
                ),
            )
            .await?;
            let installed_files_are_valid =
                validate_installed_files(&instance, steam_profile.as_ref(), &staging)
                    .await
                    .is_ok();
            let manifest_is_valid =
                read_steam_build_id(&staging, self.inner.settings.steamcmd_path.parent(), app_id)
                    .await
                    .is_ok_and(|build_id| build_id.is_some());
            if !installed_files_are_valid || !manifest_is_valid {
                self.install_failed().await;
                return Err(OperationFailure::with_internal(
                    "steamcmd_failed",
                    "servers.steamcmd_anonymous_install_failed",
                    format!("SteamCMD exit: {exit}"),
                ));
            }
            self.write_install_log(
                &root,
                "[DMX] The staged depot and Steam app manifest are complete; continuing after the non-zero SteamCMD exit.",
            )
            .await?;
        }
        self.write_install_log(
            &root,
            "[DMX] SteamCMD completed. Validating the installed files.",
        )
        .await?;
        let current_game = root.join("game");
        if let Err(error) =
            preserve_instance_data(&instance, steam_profile.as_ref(), &current_game, &staging).await
        {
            remove_dir_if_exists(&staging).await.ok();
            self.install_failed().await;
            return Err(error);
        }
        if let Err(error) =
            validate_installed_files(&instance, steam_profile.as_ref(), &staging).await
        {
            remove_dir_if_exists(&staging).await.ok();
            self.install_failed().await;
            return Err(error);
        }
        let installed_build =
            read_steam_build_id(&staging, self.inner.settings.steamcmd_path.parent(), app_id)
                .await?;
        jobs::progress(&self.inner.pool, job_id, 80)
            .await
            .map_err(OperationFailure::internal)?;

        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{job_id}"));
        remove_dir_if_exists(&rollback)
            .await
            .map_err(OperationFailure::internal)?;
        let had_previous = tokio::fs::try_exists(&game)
            .await
            .map_err(OperationFailure::internal)?;
        if had_previous {
            tokio::fs::rename(&game, &rollback)
                .await
                .map_err(OperationFailure::internal)?;
        }
        if let Err(error) = tokio::fs::rename(&staging, &game).await {
            if had_previous {
                self.restore_previous_game_tree(&root, job_id).await?;
            }
            self.install_failed().await;
            return Err(OperationFailure::with_internal(
                "install_switch_failed",
                "servers.install_switch_failed",
                error,
            ));
        }

        if let Err(error) = self
            .mark_install_committed(None, installed_build.as_deref(), None)
            .await
        {
            if had_previous {
                self.restore_previous_game_tree(&root, job_id).await?;
            } else {
                remove_dir_if_exists(&game)
                    .await
                    .map_err(OperationFailure::internal)?;
            }
            return Err(OperationFailure::internal(error));
        }
        if had_previous && !self.retain_install_rollback {
            remove_dir_if_exists(&rollback).await.ok();
        }
        remove_dir_if_exists(&staging_parent).await.ok();
        self.publish_state().await;
        self.write_install_log(&root, "[DMX] Steam installation completed successfully.")
            .await?;
        if !self.retain_install_rollback
            && (instance.auto_start || instance.desired_state == "running")
        {
            let _ = sqlx::query(
                "UPDATE instances SET desired_state = 'running', updated_at = ? WHERE id = ?",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&self.instance_id)
            .execute(&self.inner.pool)
            .await;
            let _ = self.sender.try_send(ActorCommand::AutoStart);
        }
        Ok(())
    }

    async fn install_with_restart(&mut self, job: &Job) -> Result<(), OperationFailure> {
        let current = self.instance().await?;
        let process_was_running = self.process.is_some();
        let requested_restart =
            process_was_running || current.desired_state == "running" || current.auto_start;
        let update = self
            .begin_or_load_update_transaction(job, &current, requested_restart)
            .await?;
        let restart_after = update
            .as_ref()
            .map_or(requested_restart, |transaction| transaction.restart_after);
        let root = if update.is_some() || restart_after {
            Some(self.instance_root().await?)
        } else {
            None
        };
        if process_was_running {
            self.stop_process(false, false).await?;
        }
        if let (Some(root), Some(transaction)) = (&root, &update)
            && transaction.phase == "preparing"
        {
            self.recover_preparing_install_switch(root, &job.id).await?;
        }
        if let (Some(root), Some(transaction)) = (&root, &update)
            && transaction.phase == "rolling_back"
        {
            self.recover_rolling_back_install(root, transaction).await?;
        }

        self.retain_install_rollback = update.is_some();
        let mut installation = if self.install_cancellation_requested(&job.id).await {
            Err(OperationFailure::cancelled(
                "installation_cancelled",
                "jobs.cancelled",
            ))
        } else {
            match update
                .as_ref()
                .map(|transaction| transaction.phase.as_str())
            {
                Some("committed" | "finalizing") => Ok(()),
                Some("rolling_back" | "rolled_back") => Err(OperationFailure::new(
                    "update_rolled_back",
                    "servers.update_rolled_back",
                )),
                _ => self.install(job).await,
            }
        };
        if self.close_install_cancellation(&job.id).await {
            installation = Err(OperationFailure::cancelled(
                "installation_cancelled",
                "jobs.cancelled",
            ));
        }
        self.retain_install_rollback = false;

        let Some(transaction) = update.as_ref() else {
            return installation;
        };
        let root = root.ok_or_else(|| {
            OperationFailure::new("instance_root_missing", "servers.instance_data_unsafe")
        })?;
        match installation {
            Err(installation_error) => {
                let current_transaction = self
                    .load_update_transaction()
                    .await?
                    .unwrap_or_else(|| transaction.clone());
                match current_transaction.phase.as_str() {
                    "committed" | "finalizing" => {
                        self.rollback_committed_install(&root, &current_transaction)
                            .await?;
                    }
                    "rolling_back" => {
                        self.recover_rolling_back_install(&root, &current_transaction)
                            .await?;
                    }
                    "rolled_back" => {}
                    _ => {
                        self.restore_update_snapshot(&current_transaction, None)
                            .await?;
                    }
                }
                if restart_after && let Err(restart_error) = self.start_after_update().await {
                    tracing::error!(
                        instance_id = %self.instance_id,
                        code = restart_error.code,
                        detail = ?restart_error.internal,
                        "game update failed and the previous server could not be restarted"
                    );
                    let _ = self.set_runtime_state("crashed", Some("running")).await;
                    self.schedule_watchdog().await;
                }
                if !installation_error.deferred
                    && let Err(cleanup_error) = self.finish_update_transaction(&root, &job.id).await
                {
                    tracing::error!(
                        instance_id = %self.instance_id,
                        job_id = %job.id,
                        detail = ?cleanup_error.internal,
                        "previous game was restored but update transaction cleanup failed"
                    );
                }
                Err(installation_error)
            }
            Ok(()) if !restart_after => Ok(()),
            Ok(()) => match self.start_after_update().await {
                Ok(()) => Ok(()),
                Err(updated_start_error) => {
                    self.rollback_committed_install(&root, transaction).await?;
                    if let Err(previous_start_error) = self.start_after_update().await {
                        tracing::error!(
                            instance_id = %self.instance_id,
                            code = previous_start_error.code,
                            detail = ?previous_start_error.internal,
                            "previous server was restored but could not be restarted"
                        );
                        let _ = self.set_runtime_state("crashed", Some("running")).await;
                        self.schedule_watchdog().await;
                    }
                    if let Err(cleanup_error) = self.finish_update_transaction(&root, &job.id).await
                    {
                        tracing::error!(
                            instance_id = %self.instance_id,
                            job_id = %job.id,
                            detail = ?cleanup_error.internal,
                            "rolled-back update transaction cleanup failed"
                        );
                    }
                    Err(updated_start_error)
                }
            },
        }
    }

    async fn load_update_transaction(&self) -> Result<Option<UpdateTransaction>, OperationFailure> {
        sqlx::query_as(
            "SELECT instance_id, job_id, previous_installation_state, previous_installed_version, \
             previous_installed_build, previous_settings, previous_config_version, \
             previous_desired_state, restart_after, phase \
             FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&self.instance_id)
        .fetch_optional(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)
    }

    async fn begin_or_load_update_transaction(
        &mut self,
        job: &Job,
        instance: &RuntimeInstance,
        restart_after: bool,
    ) -> Result<Option<UpdateTransaction>, OperationFailure> {
        let existing = self.load_update_transaction().await?;
        let mut recovered_terminal = false;
        if let Some(existing) = existing {
            if existing.job_id != job.id || existing.instance_id != self.instance_id {
                let state: Option<String> =
                    sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
                        .bind(&existing.job_id)
                        .fetch_optional(&self.inner.pool)
                        .await
                        .map_err(OperationFailure::internal)?;
                if state.as_deref().is_some_and(|state| {
                    matches!(state, "succeeded" | "failed" | "cancelled" | "interrupted")
                }) {
                    self.recover_terminal_update_transaction(&existing, state.as_deref())
                        .await?;
                    recovered_terminal = true;
                } else {
                    return Err(OperationFailure::new(
                        "update_transaction_conflict",
                        "jobs.instance_busy",
                    ));
                }
            } else {
                return Ok(Some(existing));
            }
        }
        let recovered_instance = if recovered_terminal {
            Some(self.instance().await?)
        } else {
            None
        };
        let instance = recovered_instance.as_ref().unwrap_or(instance);
        if !matches!(
            instance.installation_state.as_str(),
            "installed" | "updating"
        ) {
            return Ok(None);
        }
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO instance_update_transactions \
             (instance_id, job_id, previous_installation_state, previous_installed_version, \
              previous_installed_build, previous_settings, previous_config_version, \
              previous_desired_state, restart_after, phase, created_at, updated_at) \
             VALUES (?, ?, 'installed', ?, ?, ?, ?, ?, ?, 'preparing', ?, ?)",
        )
        .bind(&self.instance_id)
        .bind(&job.id)
        .bind(&instance.installed_version)
        .bind(&instance.installed_build)
        .bind(&instance.settings)
        .bind(instance.config_version)
        .bind(&instance.desired_state)
        .bind(restart_after)
        .bind(&now)
        .bind(&now)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        sqlx::query_as(
            "SELECT instance_id, job_id, previous_installation_state, previous_installed_version, \
             previous_installed_build, previous_settings, previous_config_version, \
             previous_desired_state, restart_after, phase \
             FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&self.instance_id)
        .fetch_optional(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)
    }

    async fn start_after_update(&mut self) -> Result<(), OperationFailure> {
        self.set_runtime_state("starting", Some("running"))
            .await
            .map_err(OperationFailure::internal)?;
        self.start_process().await
    }

    async fn rollback_committed_install(
        &mut self,
        root: &Path,
        transaction: &UpdateTransaction,
    ) -> Result<(), OperationFailure> {
        if self.process.is_some() {
            self.stop_process(false, true).await?;
        }
        self.set_update_phase("rolling_back").await?;
        self.restore_previous_game_tree(root, &transaction.job_id)
            .await?;
        self.restore_update_snapshot(transaction, Some("rolled_back"))
            .await?;
        self.inner.events.publish(
            "server.update_rolled_back",
            Some(self.instance_id.clone()),
            serde_json::json!({"reason": "readiness_failed", "job_id": transaction.job_id}),
        );
        Ok(())
    }

    async fn recover_preparing_install_switch(
        &self,
        root: &Path,
        job_id: &str,
    ) -> Result<(), OperationFailure> {
        if tokio::fs::symlink_metadata(root.join(format!(".rollback-{job_id}")))
            .await
            .is_ok()
        {
            self.restore_previous_game_tree(root, job_id).await?;
        }
        Ok(())
    }

    async fn recover_rolling_back_install(
        &self,
        root: &Path,
        transaction: &UpdateTransaction,
    ) -> Result<(), OperationFailure> {
        self.restore_previous_game_tree(root, &transaction.job_id)
            .await?;
        self.restore_update_snapshot(transaction, Some("rolled_back"))
            .await
    }

    async fn restore_previous_game_tree(
        &self,
        root: &Path,
        job_id: &str,
    ) -> Result<(), OperationFailure> {
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{job_id}"));
        let failed = root.join(format!(".failed-{job_id}"));
        if tokio::fs::symlink_metadata(&rollback).await.is_ok() {
            let metadata = tokio::fs::symlink_metadata(&rollback)
                .await
                .map_err(OperationFailure::internal)?;
            if !metadata.is_dir() || runtime_metadata_is_link_like(&metadata) {
                return Err(OperationFailure::new(
                    "update_rollback_unsafe",
                    "servers.update_rollback_failed",
                ));
            }
            remove_dir_if_exists(&failed)
                .await
                .map_err(OperationFailure::internal)?;
            if tokio::fs::symlink_metadata(&game).await.is_ok() {
                tokio::fs::rename(&game, &failed).await.map_err(|error| {
                    OperationFailure::with_internal(
                        "update_failed_tree_move_failed",
                        "servers.update_rollback_failed",
                        error,
                    )
                })?;
            }
            if let Err(error) = tokio::fs::rename(&rollback, &game).await {
                if tokio::fs::symlink_metadata(&failed).await.is_ok() {
                    let _ = tokio::fs::rename(&failed, &game).await;
                }
                return Err(OperationFailure::with_internal(
                    "update_rollback_switch_failed",
                    "servers.update_rollback_failed",
                    error,
                ));
            }
        } else if tokio::fs::symlink_metadata(&game).await.is_err()
            && tokio::fs::symlink_metadata(&failed).await.is_ok()
        {
            tokio::fs::rename(&failed, &game)
                .await
                .map_err(OperationFailure::internal)?;
        }
        if let Err(error) = remove_dir_if_exists(&failed).await {
            tracing::warn!(path = %failed.display(), %error, "could not remove failed update tree");
        }
        Ok(())
    }

    async fn restore_update_snapshot(
        &self,
        transaction: &UpdateTransaction,
        phase: Option<&str>,
    ) -> Result<(), OperationFailure> {
        self.restore_update_snapshot_with_desired(transaction, phase, None)
            .await
    }

    async fn restore_update_snapshot_with_desired(
        &self,
        transaction: &UpdateTransaction,
        phase: Option<&str>,
        desired_state: Option<&str>,
    ) -> Result<(), OperationFailure> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut sql_transaction = self
            .inner
            .pool
            .begin()
            .await
            .map_err(OperationFailure::internal)?;
        sqlx::query(
            "UPDATE instances SET installation_state = ?, installed_version = ?, \
             installed_build = ?, settings = COALESCE(?, settings), \
             config_version = COALESCE(?, config_version), runtime_state = 'stopped', \
             desired_state = ?, updated_at = ? \
             WHERE id = ?",
        )
        .bind(&transaction.previous_installation_state)
        .bind(&transaction.previous_installed_version)
        .bind(&transaction.previous_installed_build)
        .bind(&transaction.previous_settings)
        .bind(transaction.previous_config_version)
        .bind(desired_state.unwrap_or(&transaction.previous_desired_state))
        .bind(&now)
        .bind(&self.instance_id)
        .execute(&mut *sql_transaction)
        .await
        .map_err(OperationFailure::internal)?;
        if let Some(phase) = phase {
            sqlx::query(
                "UPDATE instance_update_transactions SET phase = ?, updated_at = ? WHERE instance_id = ?",
            )
            .bind(phase)
            .bind(&now)
            .bind(&self.instance_id)
            .execute(&mut *sql_transaction)
            .await
            .map_err(OperationFailure::internal)?;
        }
        sql_transaction
            .commit()
            .await
            .map_err(OperationFailure::internal)?;
        self.publish_state().await;
        Ok(())
    }

    async fn set_update_phase(&self, phase: &str) -> Result<(), OperationFailure> {
        sqlx::query(
            "UPDATE instance_update_transactions SET phase = ?, updated_at = ? WHERE instance_id = ?",
        )
        .bind(phase)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        Ok(())
    }

    async fn recover_terminal_update_transaction(
        &mut self,
        transaction: &UpdateTransaction,
        job_state: Option<&str>,
    ) -> Result<(), OperationFailure> {
        let root = self.instance_root().await?;
        if matches!(transaction.phase.as_str(), "committed" | "finalizing")
            && job_state == Some("succeeded")
        {
            self.finish_update_transaction(&root, &transaction.job_id)
                .await?;
            return Ok(());
        }
        let desired_state = self.instance().await?.desired_state;
        if self.process.is_some() {
            self.stop_process(false, true).await?;
        }
        match transaction.phase.as_str() {
            "preparing" => {
                self.recover_preparing_install_switch(&root, &transaction.job_id)
                    .await?;
            }
            "committed" | "finalizing" | "rolling_back" => {
                self.restore_previous_game_tree(&root, &transaction.job_id)
                    .await?;
            }
            "rolled_back" => {}
            _ => {
                return Err(OperationFailure::new(
                    "update_transaction_invalid",
                    "servers.update_rollback_failed",
                ));
            }
        }
        self.restore_update_snapshot_with_desired(
            transaction,
            Some("rolled_back"),
            Some(&desired_state),
        )
        .await?;
        self.finish_update_transaction(&root, &transaction.job_id)
            .await
    }

    async fn finish_update_transaction(
        &self,
        root: &Path,
        job_id: &str,
    ) -> Result<(), OperationFailure> {
        sqlx::query(
            "DELETE FROM instance_update_transactions WHERE instance_id = ? AND job_id = ?",
        )
        .bind(&self.instance_id)
        .bind(job_id)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        for path in [
            root.join(format!(".rollback-{job_id}")),
            root.join(format!(".failed-{job_id}")),
        ] {
            if let Err(error) = remove_dir_if_exists(&path).await {
                tracing::warn!(path = %path.display(), %error, "could not clean update transaction tree");
            }
        }
        Ok(())
    }

    async fn complete_install_job(&self, job_id: &str) -> Result<(), AppError> {
        let root = self
            .instance_root()
            .await
            .map_err(operation_failure_to_app)?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut transaction = self.inner.pool.begin().await?;
        let conflicting_transaction: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instance_update_transactions \
             WHERE instance_id = ? AND job_id <> ?",
        )
        .bind(&self.instance_id)
        .bind(job_id)
        .fetch_one(&mut *transaction)
        .await?;
        let update_phase: Option<String> = sqlx::query_scalar(
            "SELECT phase FROM instance_update_transactions \
             WHERE instance_id = ? AND job_id = ?",
        )
        .bind(&self.instance_id)
        .bind(job_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if conflicting_transaction != 0
            || update_phase
                .as_deref()
                .is_some_and(|phase| !matches!(phase, "committed" | "finalizing"))
        {
            return Err(AppError::Conflict(
                "servers.update_transaction_incomplete".into(),
            ));
        }
        if update_phase.is_some() {
            sqlx::query(
                "UPDATE instance_update_transactions SET phase = 'finalizing', updated_at = ? \
                 WHERE instance_id = ? AND job_id = ?",
            )
            .bind(&now)
            .bind(&self.instance_id)
            .bind(job_id)
            .execute(&mut *transaction)
            .await?;
        }
        let completed = sqlx::query(
            "UPDATE jobs SET state = 'succeeded', progress = 100, error_code = NULL, \
             error_message = NULL, finished_at = ? WHERE id = ? AND state = 'running' \
             AND cancel_requested_at IS NULL",
        )
        .bind(&now)
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
        if completed.rows_affected() != 1 {
            return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
        }
        sqlx::query(
            "INSERT INTO job_events (job_id, event_type, payload, created_at) \
             VALUES (?, 'job.succeeded', '{\"error_code\":null}', ?)",
        )
        .bind(job_id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM instance_update_transactions WHERE instance_id = ? AND job_id = ?",
        )
        .bind(&self.instance_id)
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.cleanup_update_trees(&root, job_id).await;
        Ok(())
    }

    async fn cleanup_update_trees(&self, root: &Path, job_id: &str) {
        for path in [
            root.join(format!(".rollback-{job_id}")),
            root.join(format!(".failed-{job_id}")),
        ] {
            if let Err(error) = remove_dir_if_exists(&path).await {
                tracing::warn!(path = %path.display(), %error, "could not clean update transaction tree");
            }
        }
    }

    async fn install_hytale(
        &mut self,
        job_id: &str,
        instance: &RuntimeInstance,
    ) -> Result<(), OperationFailure> {
        let installation_state = if matches!(
            instance.installation_state.as_str(),
            "installed" | "updating"
        ) {
            "updating"
        } else {
            "installing"
        };
        sqlx::query(
            "UPDATE instances SET installation_state = ?, runtime_state = 'stopped', \
             updated_at = ? WHERE id = ?",
        )
        .bind(installation_state)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        self.publish_state().await;
        jobs::progress(&self.inner.pool, job_id, 10)
            .await
            .map_err(OperationFailure::internal)?;

        let root = self.instance_root().await?;
        self.write_install_log(
            &root,
            "[DMX] Preparing the official Hytale downloader and managed Java 25.",
        )
        .await?;
        let staging_parent = ensure_staging_parent(&root).await?;
        let hytale_staging = staging_parent.join("hytale");
        match tokio::fs::symlink_metadata(&hytale_staging).await {
            Ok(metadata) if metadata.is_dir() && !runtime_metadata_is_link_like(&metadata) => {}
            Ok(_) => {
                return Err(OperationFailure::new(
                    "install_tree_unsafe",
                    "servers.instance_data_unsafe",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&hytale_staging)
                    .await
                    .map_err(OperationFailure::internal)?;
            }
            Err(error) => return Err(OperationFailure::internal(error)),
        }
        let payload = hytale_staging.join("payload");
        let settings: Value =
            serde_json::from_str(&instance.settings).map_err(OperationFailure::internal)?;
        let context = InstallContext::official()
            .map_err(installer_failure)?
            .with_toolchain_root(self.inner.settings.data_dir.join("toolchains/java"));
        if tokio::fs::try_exists(&payload)
            .await
            .map_err(OperationFailure::internal)?
        {
            match installers::resume_native_install("hytale", &settings, &payload).await {
                Ok(installed) => {
                    installers::ensure_managed_java(&context, 25)
                        .await
                        .map_err(installer_failure)?;
                    if self.close_install_cancellation(job_id).await {
                        return Err(OperationFailure::cancelled(
                            "installation_cancelled",
                            "jobs.cancelled",
                        ));
                    }
                    return self
                        .commit_native_install(job_id, instance, &root, &payload, installed, None)
                        .await;
                }
                Err(error) => {
                    tracing::warn!(
                        instance_id = %self.instance_id,
                        code = error.code,
                        "discarding incomplete Hytale staging payload"
                    );
                    remove_dir_if_exists(&payload)
                        .await
                        .map_err(OperationFailure::internal)?;
                }
            }
        }
        let session = hytale_staging.join(format!(".session-{}", uuid::Uuid::new_v4().as_simple()));
        tokio::fs::create_dir(&session)
            .await
            .map_err(OperationFailure::internal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&session, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(OperationFailure::internal)?;
        }

        let plan = installers::hytale::prepare_hytale_downloader(&session, &context)
            .await
            .map_err(installer_failure)?;
        self.write_install_log(
            &root,
            &format!(
                "[DMX] Official Hytale downloader ready (sha256={}, size={} bytes, isolated environment=yes).",
                plan.downloader_artifact.sha256, plan.downloader_artifact.size
            ),
        )
        .await?;
        let mut redactions = Vec::new();
        let mut restored_credentials = false;
        if let Some(document) = self
            .inner
            .secrets
            .get(
                &self.inner.pool,
                &self.instance_id,
                installers::hytale::DOWNLOADER_CREDENTIAL_SECRET,
            )
            .await
            .map_err(OperationFailure::internal)?
        {
            redactions =
                installers::hytale::credential_redactions(&document).map_err(installer_failure)?;
            installers::hytale::write_plaintext_credentials(&plan.credential_file, &document)
                .await
                .map_err(installer_failure)?;
            restored_credentials = true;
        }
        self.write_install_log(
            &root,
            if restored_credentials {
                "[DMX] Restored encrypted Hytale downloader credentials for this instance."
            } else {
                "[DMX] No stored Hytale downloader credentials; OAuth device authorization is expected."
            },
        )
        .await?;
        self.write_install_log(&root, "[DMX] Checking the available Hytale server version.")
            .await?;

        let mut cancellation_rx = self.install_cancellation_receiver(job_id).await?;
        let mut result: Result<installers::InstallResult, OperationFailure> = async {
            let version_output = self
                .run_hytale_downloader(
                    job_id,
                    &plan,
                    HytaleDownloaderPhase::VersionCheck,
                    redactions.clone(),
                    &root,
                    &mut cancellation_rx,
                )
                .await?;
            let installed_version = installers::hytale::parse_printed_version(&version_output)
                .ok_or_else(|| {
                    OperationFailure::new(
                        "hytale_version_invalid",
                        "servers.provider_response_invalid",
                    )
                })?;
            self.write_install_log(
                &root,
                &format!("[DMX] Hytale server version {installed_version} selected."),
            )
            .await?;
            match installers::hytale::read_plaintext_credentials(&plan.credential_file).await {
                Ok(document) => {
                    self.inner
                        .secrets
                        .set(
                            &self.inner.pool,
                            &self.instance_id,
                            installers::hytale::DOWNLOADER_CREDENTIAL_SECRET,
                            &document,
                        )
                        .await
                        .map_err(OperationFailure::internal)?;
                    redactions = installers::hytale::credential_redactions(&document)
                        .map_err(installer_failure)?;
                }
                Err(error) if error.code == "hytale_credentials_missing" => {
                    // `-print-version` may not require authentication. The actual
                    // download below owns the device flow and creates this file.
                }
                Err(error) => return Err(installer_failure(error)),
            }
            jobs::progress(&self.inner.pool, job_id, 30)
                .await
                .map_err(OperationFailure::internal)?;

            self.write_install_log(
                &root,
                "[DMX] Starting the official Hytale server download. Authentication instructions will appear here and as an action card if required.",
            )
            .await?;
            self.run_hytale_downloader(
                job_id,
                &plan,
                HytaleDownloaderPhase::ServerDownload,
                redactions,
                &root,
                &mut cancellation_rx,
            )
            .await?;
            let refreshed = installers::hytale::read_plaintext_credentials(&plan.credential_file)
                .await
                .map_err(installer_failure)?;
            self.inner
                .secrets
                .set(
                    &self.inner.pool,
                    &self.instance_id,
                    installers::hytale::DOWNLOADER_CREDENTIAL_SECRET,
                    &refreshed,
                )
                .await
                .map_err(OperationFailure::internal)?;
            jobs::progress(&self.inner.pool, job_id, 55)
                .await
                .map_err(OperationFailure::internal)?;
            self.write_install_log(
                &root,
                "[DMX] Download completed. Extracting and validating the Hytale server.",
            )
            .await?;
            remove_dir_if_exists(&payload)
                .await
                .map_err(OperationFailure::internal)?;
            let installed = installers::install_hytale_downloaded(
                &settings,
                &root,
                &plan.output_archive,
                &payload,
                &context,
                installed_version,
                plan.downloader_artifact.clone(),
            )
            .await;
            match installed {
                Ok(installed) => {
                    let aot_cache_enabled = installed
                        .plan
                        .args
                        .iter()
                        .any(|argument| argument == "-XX:AOTCache=HytaleServer.aot");
                    self.write_install_log(
                        &root,
                        if aot_cache_enabled {
                            "[DMX] Hytale archive layout validated (Assets.zip=present, HytaleServer.jar=present, optional AOT cache=present and enabled)."
                        } else {
                            "[DMX] Hytale archive layout validated (Assets.zip=present, HytaleServer.jar=present, optional AOT cache=absent; the server will start without it)."
                        },
                    )
                    .await?;
                    Ok(installed)
                }
                Err(error) => {
                    if error.code == "hytale_layout_invalid" {
                        let diagnostics =
                            installers::hytale::game_layout_diagnostics(&payload).await;
                        if let Err(log_error) = self
                            .write_install_log(
                                &root,
                                &format!(
                                    "[DMX] Hytale archive layout diagnostics: {diagnostics}"
                                ),
                            )
                            .await
                        {
                            tracing::warn!(
                                instance_id = %self.instance_id,
                                ?log_error,
                                "failed to append Hytale layout diagnostics"
                            );
                        }
                    }
                    Err(installer_failure(error))
                }
            }
        }
        .await;
        if self.close_install_cancellation(job_id).await {
            result = Err(OperationFailure::cancelled(
                "installation_cancelled",
                "jobs.cancelled",
            ));
        }
        let plaintext_cleanup =
            installers::hytale::remove_plaintext_credentials(&plan.credential_file).await;
        if let Err(error) = &plaintext_cleanup {
            tracing::error!(
                instance_id = %self.instance_id,
                code = error.code,
                detail = ?error.internal,
                "failed to remove Hytale downloader plaintext credentials"
            );
        }
        let session_cleanup = remove_dir_if_exists(&session).await;
        if let Err(error) = &session_cleanup {
            tracing::warn!(instance_id = %self.instance_id, %error, "failed to clean Hytale downloader session");
        }
        if session_cleanup.is_err() {
            self.install_failed().await;
            return Err(OperationFailure::new(
                if plaintext_cleanup.is_err() {
                    "hytale_credentials_cleanup_failed"
                } else {
                    "hytale_session_cleanup_failed"
                },
                "servers.installation_failed",
            ));
        }
        let installed = match result {
            Ok(installed) => installed,
            Err(error) => {
                if !error.cancelled {
                    self.install_failed().await;
                }
                return Err(error);
            }
        };
        self.commit_native_install(job_id, instance, &root, &payload, installed, None)
            .await
    }

    async fn run_hytale_downloader(
        &self,
        job_id: &str,
        plan: &installers::hytale::HytaleDownloaderPlan,
        phase: HytaleDownloaderPhase,
        redactions: Vec<String>,
        root: &Path,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<String, OperationFailure> {
        let combined_log = Arc::new(Mutex::new(
            RotatingLog::open(root.join("logs/install.combined.log"))
                .await
                .map_err(OperationFailure::internal)?,
        ));
        let started_at = Instant::now();
        let credentials_before = hytale_credential_file_state(&plan.credential_file).await;
        self.write_hytale_diagnostic(
            &combined_log,
            &format!(
                "[DMX] Starting Hytale downloader phase={} (arguments={}, credentials_before={}).",
                phase.label(),
                phase.safe_arguments(),
                credentials_before
            ),
        )
        .await;

        let mut command = Command::new(&plan.executable);
        command
            .current_dir(&plan.cwd)
            .args(phase.arguments(plan))
            .env_clear()
            .envs(filtered_tool_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = match spawn_contained(&mut command) {
            Ok(spawned) => spawned,
            Err(error) => {
                let category = match &error {
                    ContainedSpawnError::Spawn(_) => "spawn-error",
                    ContainedSpawnError::Containment(_) => "containment-error",
                };
                self.write_hytale_diagnostic(
                    &combined_log,
                    &format!(
                        "[DMX] Hytale downloader phase={} failed to start (category={category}).",
                        phase.label()
                    ),
                )
                .await;
                return Err(match error {
                    ContainedSpawnError::Spawn(error) => OperationFailure::with_internal(
                        "hytale_downloader_start_failed",
                        "servers.hytale_downloader_failed",
                        error,
                    ),
                    ContainedSpawnError::Containment(error) => OperationFailure::with_internal(
                        "process_containment_failed",
                        "servers.process_containment_failed",
                        error,
                    ),
                });
            }
        };
        let mut child = spawned.child;
        #[cfg(windows)]
        let windows_job = spawned.windows_job;
        let Some(pid) = child.id() else {
            self.write_hytale_diagnostic(
                &combined_log,
                &format!(
                    "[DMX] Hytale downloader phase={} started without a process identifier.",
                    phase.label()
                ),
            )
            .await;
            return Err(OperationFailure::new(
                "hytale_downloader_start_failed",
                "servers.hytale_downloader_failed",
            ));
        };
        #[cfg(windows)]
        let job_handle = Some(windows_job.handle);
        #[cfg(not(windows))]
        let job_handle = None;
        self.write_hytale_diagnostic(
            &combined_log,
            &format!(
                "[DMX] Hytale downloader phase={} process started (pid={pid}).",
                phase.label()
            ),
        )
        .await;
        let (line_tx, mut line_rx) = mpsc::channel(256);
        let stdout_task = child.stdout.take().map(|stdout| {
            tokio::spawn(pump_output_observed(
                stdout,
                OutputPumpConfig {
                    log_path: root.join("logs/install.log"),
                    combined_log: Some(combined_log.clone()),
                    stream: "install",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions: redactions.clone(),
                    observer: Some(line_tx.clone()),
                    player_observer: None,
                    public_log_policy: PublicLogPolicy::HytaleDeviceFlow,
                },
            ))
        });
        let stderr_task = child.stderr.take().map(|stderr| {
            tokio::spawn(pump_output_observed(
                stderr,
                OutputPumpConfig {
                    log_path: root.join("logs/install.error.log"),
                    combined_log: Some(combined_log.clone()),
                    stream: "install_error",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions,
                    observer: Some(line_tx.clone()),
                    player_observer: None,
                    public_log_policy: PublicLogPolicy::HytaleDeviceFlow,
                },
            ))
        });
        drop(line_tx);
        let timeout = tokio::time::sleep(INSTALL_TIMEOUT);
        tokio::pin!(timeout);
        let mut captured = String::new();
        let mut authorization_tail = String::new();
        let mut active_authorization: Option<installers::hytale::DeviceAuthorization> = None;
        let mut observed_lines = 0_u64;
        let mut authorization_requests = 0_u32;
        let (status, interrupted) = loop {
            tokio::select! {
                status = child.wait() => break (status, None),
                line = line_rx.recv() => {
                    if let Some(line) = line {
                        observed_lines = observed_lines.saturating_add(1);
                        append_bounded_output(&mut captured, &line, 64 * 1024);
                        append_bounded_tail(&mut authorization_tail, &line, 16 * 1024);
                        if let Some(authorization) =
                            installers::hytale::detect_device_authorization(&authorization_tail)
                            && active_authorization.as_ref() != Some(&authorization)
                        {
                            let payload = serde_json::json!({
                                "job_id": job_id,
                                "interaction": {
                                    "kind": "oauth_device",
                                    "verification_uri": authorization.verification_uri.clone(),
                                    "user_code": authorization.user_code.clone(),
                                }
                            });
                            if active_authorization.is_some() {
                                jobs::refresh_waiting_for_user(
                                    &self.inner.pool,
                                    job_id,
                                    payload.clone(),
                                )
                                .await
                                .map_err(OperationFailure::internal)?;
                            } else {
                                jobs::wait_for_user(&self.inner.pool, job_id, payload.clone())
                                    .await
                                    .map_err(OperationFailure::internal)?;
                            }
                            self.inner.events.publish(
                                "job.waiting_for_user",
                                Some(self.instance_id.clone()),
                                payload,
                            );
                            self.publish_job(job_id).await;
                            authorization_requests = authorization_requests.saturating_add(1);
                            self.write_hytale_diagnostic(
                                &combined_log,
                                &hytale_device_request_diagnostic(
                                    &authorization,
                                    authorization_requests,
                                ),
                            )
                            .await;
                            active_authorization = Some(authorization);
                        }
                    }
                }
                changed = cancellation.changed() => {
                    if changed.is_ok() && *cancellation.borrow() {
                        break (
                            terminate_installer(&mut child, pid, job_handle).await,
                            Some(InstallInterruption::Cancelled),
                        );
                    }
                }
                () = &mut timeout => {
                    break (
                        terminate_installer(&mut child, pid, job_handle).await,
                        Some(InstallInterruption::TimedOut),
                    );
                }
            }
        };
        if let Some(task) = stdout_task {
            let _ = task.await;
        }
        if let Some(task) = stderr_task {
            let _ = task.await;
        }
        while let Ok(line) = line_rx.try_recv() {
            observed_lines = observed_lines.saturating_add(1);
            append_bounded_output(&mut captured, &line, 64 * 1024);
            append_bounded_tail(&mut authorization_tail, &line, 16 * 1024);
        }
        let credentials_after = hytale_credential_file_state(&plan.credential_file).await;
        let outcome = match (interrupted, &status) {
            (Some(InstallInterruption::Cancelled), _) => "cancelled".to_string(),
            (Some(InstallInterruption::TimedOut), _) => "installation-timeout".to_string(),
            (None, Ok(status)) if status.success() => "success".to_string(),
            (None, Ok(status)) => status
                .code()
                .map_or_else(|| "signal".to_string(), |code| format!("exit-{code}")),
            (None, Err(_)) => "wait-error".to_string(),
        };
        self.write_hytale_diagnostic(
            &combined_log,
            &format!(
                "[DMX] Hytale downloader phase={} completed (outcome={}, elapsed_ms={}, output_lines={}, oauth_requests={}, credentials_after={}).",
                phase.label(),
                outcome,
                started_at.elapsed().as_millis(),
                observed_lines,
                authorization_requests,
                credentials_after
            ),
        )
        .await;
        if active_authorization.is_some() {
            jobs::resume_from_user(&self.inner.pool, job_id)
                .await
                .map_err(OperationFailure::internal)?;
            self.publish_job(job_id).await;
        }
        if let Some(interrupted) = interrupted {
            return match interrupted {
                InstallInterruption::Cancelled => Err(OperationFailure::cancelled(
                    "installation_cancelled",
                    "jobs.cancelled",
                )),
                InstallInterruption::TimedOut => Err(OperationFailure::new(
                    "installation_timeout",
                    "servers.installation_timeout",
                )),
            };
        }
        let status = status.map_err(OperationFailure::internal)?;
        if !status.success() {
            let exit = status.code().map_or_else(
                || "terminated by signal".to_string(),
                |code| code.to_string(),
            );
            if let Some(diagnostic) = hytale_downloader_failure_diagnostic(&authorization_tail) {
                self.write_hytale_diagnostic(&combined_log, diagnostic)
                    .await;
            }
            self.write_hytale_diagnostic(
                &combined_log,
                &format!("[DMX] Hytale downloader exited unsuccessfully ({exit})."),
            )
            .await;
            return Err(OperationFailure::with_internal(
                "hytale_downloader_failed",
                "servers.hytale_downloader_failed",
                format!("Hytale downloader exit: {exit}"),
            ));
        }
        Ok(captured)
    }

    async fn install_native(
        &mut self,
        job_id: &str,
        instance: &RuntimeInstance,
    ) -> Result<(), OperationFailure> {
        let installation_state = if matches!(
            instance.installation_state.as_str(),
            "installed" | "updating"
        ) {
            "updating"
        } else {
            "installing"
        };
        sqlx::query(
            "UPDATE instances SET installation_state = ?, runtime_state = 'stopped', \
             updated_at = ? WHERE id = ?",
        )
        .bind(installation_state)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await
        .map_err(OperationFailure::internal)?;
        self.publish_state().await;
        jobs::progress(&self.inner.pool, job_id, 10)
            .await
            .map_err(OperationFailure::internal)?;

        let root = self.instance_root().await?;
        self.write_install_log(
            &root,
            &format!(
                "[DMX] Preparing the native installer for profile {}.",
                instance.profile_id
            ),
        )
        .await?;
        let staging_parent = ensure_staging_parent(&root).await?;
        let staging = staging_parent.join(job_id);
        let configured_settings: Value =
            serde_json::from_str(&instance.settings).map_err(OperationFailure::internal)?;
        let mut context = InstallContext::official_with_bedrock(&self.inner.settings)
            .map_err(installer_failure)?
            .with_toolchain_root(self.inner.settings.data_dir.join("toolchains/java"));
        let settings = if instance.installed_version.is_some() || instance.installed_build.is_some()
        {
            installers::native_update_target(&instance.profile_id, &configured_settings, &context)
                .await
                .map_err(installer_failure)?
                .settings
        } else {
            configured_settings.clone()
        };
        let resolved_settings = (settings != configured_settings).then_some(settings.clone());
        match tokio::fs::symlink_metadata(&staging).await {
            Ok(_) => {
                match installers::resume_native_install(&instance.profile_id, &settings, &staging)
                    .await
                {
                    Ok(installed) => {
                        if let InstallerExecutable::ManagedJava { major } =
                            installed.plan.executable
                        {
                            installers::ensure_managed_java(&context, major)
                                .await
                                .map_err(installer_failure)?;
                        }
                        if self.close_install_cancellation(job_id).await {
                            return Err(OperationFailure::cancelled(
                                "installation_cancelled",
                                "jobs.cancelled",
                            ));
                        }
                        let result = self
                            .commit_native_install(
                                job_id,
                                instance,
                                &root,
                                &staging,
                                installed,
                                resolved_settings.as_ref(),
                            )
                            .await;
                        if instance.profile_id == "minecraft-bedrock" {
                            let _ = installers::remove_bedrock_upload(&root, job_id).await;
                        }
                        return result;
                    }
                    Err(error) => {
                        tracing::warn!(
                            instance_id = %self.instance_id,
                            code = error.code,
                            "discarding incomplete native installer staging tree"
                        );
                        remove_dir_if_exists(&staging)
                            .await
                            .map_err(OperationFailure::internal)?;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(OperationFailure::internal(error)),
        }
        tokio::fs::create_dir(&staging)
            .await
            .map_err(OperationFailure::internal)?;
        if instance.profile_id == "minecraft-bedrock" {
            let expected_version =
                settings
                    .get("version")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        OperationFailure::new(
                            "bedrock_version_unavailable",
                            "servers.bedrock_version_unavailable",
                        )
                    })?;
            context = context
                .with_bedrock_upload(&root, job_id, expected_version)
                .await
                .map_err(installer_failure)?;
        }
        let mut cancellation_rx = self.install_cancellation_receiver(job_id).await?;
        self.write_install_log(
            &root,
            "[DMX] Downloading and validating the required game files and toolchains.",
        )
        .await?;
        let mut installation = tokio::select! {
            result = installers::install_native(
                &instance.profile_id,
                &settings,
                &root,
                &staging,
                &context,
            ) => result.map_err(installer_failure),
            changed = cancellation_rx.changed() => {
                if changed.is_ok() && *cancellation_rx.borrow() {
                    Err(OperationFailure::cancelled("installation_cancelled", "jobs.cancelled"))
                } else {
                    Err(OperationFailure::new("installation_interrupted", "jobs.interrupted"))
                }
            }
            _ = tokio::time::sleep(INSTALL_TIMEOUT) => {
                Err(OperationFailure::new("installation_timeout", "servers.installation_timeout"))
            }
        };
        if self.close_install_cancellation(job_id).await {
            installation = Err(OperationFailure::cancelled(
                "installation_cancelled",
                "jobs.cancelled",
            ));
        }
        let installed = match installation {
            Ok(installed) => installed,
            Err(error) => {
                remove_dir_if_exists(&staging).await.ok();
                if instance.profile_id == "minecraft-bedrock"
                    && error.code == "bedrock_official_source_unavailable"
                {
                    self.write_install_log(
                        &root,
                        "[ACTION REQUIRED] Upload the official Minecraft Bedrock server archive from the action card above or from the Jobs page.",
                    )
                    .await?;
                    let payload = serde_json::json!({
                        "job_id": job_id,
                        "interaction": {
                            "kind": "bedrock_archive_upload",
                            "instance_id": &self.instance_id,
                            "version": settings.get("version").and_then(Value::as_str),
                            "method": "POST",
                            "path": format!("/api/v1/servers/{}/imports/zip", self.instance_id),
                            "required_sha256_header": "x-dmx-archive-sha256",
                            "max_bytes": 4_u64 * 1024 * 1024 * 1024,
                        }
                    });
                    jobs::wait_for_user(&self.inner.pool, job_id, payload.clone())
                        .await
                        .map_err(OperationFailure::internal)?;
                    self.inner.events.publish(
                        "job.waiting_for_user",
                        Some(self.instance_id.clone()),
                        payload,
                    );
                    self.publish_job(job_id).await;
                    tokio::spawn(expire_bedrock_upload_wait(
                        self.sender.clone(),
                        job_id.to_string(),
                    ));
                    return Err(OperationFailure::deferred(
                        "bedrock_archive_required",
                        "servers.bedrock_archive_required",
                    ));
                }
                if instance.profile_id == "minecraft-bedrock" {
                    let _ = installers::remove_bedrock_upload(&root, job_id).await;
                }
                if !error.cancelled {
                    self.install_failed().await;
                }
                return Err(error);
            }
        };
        self.write_install_log(
            &root,
            "[DMX] Native installer completed. Activating the staged game files.",
        )
        .await?;
        let result = self
            .commit_native_install(
                job_id,
                instance,
                &root,
                &staging,
                installed,
                resolved_settings.as_ref(),
            )
            .await;
        if instance.profile_id == "minecraft-bedrock" {
            let _ = installers::remove_bedrock_upload(&root, job_id).await;
        }
        result
    }

    async fn commit_native_install(
        &mut self,
        job_id: &str,
        instance: &RuntimeInstance,
        root: &Path,
        staging: &Path,
        installed: installers::InstallResult,
        resolved_settings: Option<&Value>,
    ) -> Result<(), OperationFailure> {
        self.write_install_log(
            root,
            "[DMX] Switching the validated staging directory into place.",
        )
        .await?;
        jobs::progress(&self.inner.pool, job_id, 80)
            .await
            .map_err(OperationFailure::internal)?;
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{job_id}"));
        remove_dir_if_exists(&rollback)
            .await
            .map_err(OperationFailure::internal)?;
        let had_previous = tokio::fs::try_exists(&game)
            .await
            .map_err(OperationFailure::internal)?;
        if had_previous {
            tokio::fs::rename(&game, &rollback)
                .await
                .map_err(OperationFailure::internal)?;
        }
        if let Err(error) = tokio::fs::rename(staging, &game).await {
            if had_previous {
                self.restore_previous_game_tree(root, job_id).await?;
            }
            self.install_failed().await;
            return Err(OperationFailure::with_internal(
                "install_switch_failed",
                "servers.install_switch_failed",
                error,
            ));
        }
        if let Err(error) = self
            .mark_install_committed(
                Some(&installed.installed_version),
                installed.installed_build.as_deref(),
                resolved_settings,
            )
            .await
        {
            if had_previous {
                self.restore_previous_game_tree(root, job_id).await?;
            } else {
                remove_dir_if_exists(&game)
                    .await
                    .map_err(OperationFailure::internal)?;
            }
            return Err(OperationFailure::internal(error));
        }
        if had_previous && !self.retain_install_rollback {
            remove_dir_if_exists(&rollback).await.ok();
        }
        if let Some(parent) = staging.parent() {
            remove_dir_if_exists(parent).await.ok();
        }
        self.publish_state().await;
        self.write_install_log(root, "[DMX] Installation completed successfully.")
            .await?;
        if !self.retain_install_rollback
            && (instance.auto_start || instance.desired_state == "running")
        {
            let _ = sqlx::query(
                "UPDATE instances SET desired_state = 'running', updated_at = ? WHERE id = ?",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&self.instance_id)
            .execute(&self.inner.pool)
            .await;
            let _ = self.sender.try_send(ActorCommand::AutoStart);
        }
        Ok(())
    }

    async fn mark_install_committed(
        &self,
        installed_version: Option<&str>,
        installed_build: Option<&str>,
        resolved_settings: Option<&Value>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut transaction = self.inner.pool.begin().await?;
        if let Some(settings) = resolved_settings {
            sqlx::query(
                "UPDATE instances SET installation_state = 'installed', installed_version = ?, \
                 installed_build = ?, settings = ?, config_version = config_version + 1, \
                 updated_at = ? WHERE id = ?",
            )
            .bind(installed_version)
            .bind(installed_build)
            .bind(settings.to_string())
            .bind(&now)
            .bind(&self.instance_id)
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query(
                "UPDATE instances SET installation_state = 'installed', installed_version = ?, \
                 installed_build = ?, updated_at = ? WHERE id = ?",
            )
            .bind(installed_version)
            .bind(installed_build)
            .bind(&now)
            .bind(&self.instance_id)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "UPDATE instance_update_transactions SET phase = 'committed', updated_at = ? \
             WHERE instance_id = ?",
        )
        .bind(&now)
        .bind(&self.instance_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }

    async fn install_failed(&self) {
        let _ = sqlx::query(
            "UPDATE instances SET installation_state = 'failed', updated_at = ? WHERE id = ?",
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await;
        self.publish_state().await;
    }

    async fn start_process(&mut self) -> Result<(), OperationFailure> {
        if self.process.is_some() {
            self.set_runtime_state("running", Some("running"))
                .await
                .map_err(OperationFailure::internal)?;
            return Ok(());
        }
        let instance = self.instance().await?;
        if instance.installation_state != "installed" {
            return Err(OperationFailure::new(
                "server_not_installed",
                "servers.not_installed",
            ));
        }
        let steam_profile = steam_profile_for_instance(&self.inner.pool, &instance).await?;
        if !installers::native_install_supported(&instance.profile_id)
            && !matches!(
                instance.profile_id.as_str(),
                "valheim"
                    | "palworld"
                    | "satisfactory"
                    | "seven-days-to-die"
                    | "project-zomboid"
                    | "rust"
            )
            && steam_profile.is_none()
        {
            return Err(OperationFailure::new(
                "runtime_not_implemented",
                "servers.runtime_not_implemented",
            ));
        }
        let root = self.instance_root().await?;
        let hytale_update_pending = if instance.profile_id == "hytale" {
            recover_hytale_update_state(&root).await?
        } else {
            false
        };
        preflight_ports(&self.inner.pool, &self.instance_id).await?;
        let readiness = readiness_pattern(&self.inner.pool, &instance).await?;
        let launch = self.build_launch_spec(&instance).await?;
        self.apply_pending_config_changes(&root).await?;
        self.set_runtime_state("starting", Some("running"))
            .await
            .map_err(OperationFailure::internal)?;
        let mut command = Command::new(&launch.spec.executable);
        command
            .current_dir(&launch.spec.cwd)
            .args(&launch.spec.args)
            .env_clear()
            .envs(launch.spec.env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = spawn_contained(&mut command).map_err(|error| match error {
            ContainedSpawnError::Spawn(error) => OperationFailure::with_internal(
                "server_start_failed",
                "servers.start_failed",
                error,
            ),
            ContainedSpawnError::Containment(error) => OperationFailure::with_internal(
                "process_containment_failed",
                "servers.process_containment_failed",
                error,
            ),
        })?;
        let mut child = spawned.child;
        #[cfg(windows)]
        let windows_job = spawned.windows_job;
        let pid = child
            .id()
            .ok_or_else(|| OperationFailure::new("server_start_failed", "servers.start_failed"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        let (output_tx, mut output_rx) = mpsc::channel(1_024);
        let player_log_tx = players::spawn_log_observer(
            self.inner.pool.clone(),
            self.inner.events.clone(),
            self.instance_id.clone(),
            instance.profile_id.clone(),
        );
        if let Some(stdout) = stdout {
            tokio::spawn(pump_output_observed(
                stdout,
                OutputPumpConfig {
                    log_path: root.join("logs/console.log"),
                    combined_log: None,
                    stream: "stdout",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions: launch.redactions.clone(),
                    observer: Some(output_tx.clone()),
                    player_observer: Some(player_log_tx.clone()),
                    public_log_policy: PublicLogPolicy::Normal,
                },
            ));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(pump_output_observed(
                stderr,
                OutputPumpConfig {
                    log_path: root.join("logs/console.error.log"),
                    combined_log: None,
                    stream: "stderr",
                    instance_id: self.instance_id.clone(),
                    events: self.inner.events.clone(),
                    redactions: launch.redactions,
                    observer: Some(output_tx.clone()),
                    player_observer: Some(player_log_tx.clone()),
                    public_log_policy: PublicLogPolicy::Normal,
                },
            ));
        }
        drop(output_tx);
        drop(player_log_tx);

        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let (exit_tx, exit_rx) = watch::channel(None);
        let readiness_exit_rx = exit_rx.clone();
        let sender = self.sender.clone();
        let started_at = Instant::now();
        tokio::spawn(async move {
            let outcome = match child.wait().await {
                Ok(status) => ExitOutcome {
                    success: status.success(),
                    code: status.code(),
                    elapsed: started_at.elapsed(),
                },
                Err(error) => {
                    tracing::error!(pid, %error, "failed to wait for game process");
                    ExitOutcome {
                        success: false,
                        code: None,
                        elapsed: started_at.elapsed(),
                    }
                }
            };
            let _ = exit_tx.send(Some(outcome.clone()));
            let _ = sender
                .send(ActorCommand::Exited {
                    generation,
                    outcome,
                })
                .await;
        });
        let metrics_stop = metrics::spawn_collector(
            self.inner.pool.clone(),
            self.inner.events.clone(),
            self.instance_id.clone(),
            pid,
            root,
        );
        self.process = Some(ManagedProcess {
            pid,
            stdin,
            exit_rx,
            generation,
            stop: launch.stop,
            output_rx: None,
            _metrics_stop: metrics_stop,
            #[cfg(windows)]
            _job: windows_job,
        });
        let readiness_result = if let Some(readiness) = readiness {
            wait_until_ready(
                readiness,
                &mut output_rx,
                readiness_exit_rx,
                READINESS_TIMEOUT,
            )
            .await
        } else {
            wait_for_process_stability(readiness_exit_rx, DEFAULT_READINESS_STABILITY_WINDOW).await
        };
        if let Err(error) = readiness_result {
            let mut process_to_kill = self
                .process
                .take()
                .filter(|process| process.exit_rx.borrow().is_none());
            if let Some(process) = process_to_kill.as_mut() {
                let _ = process.force_kill().await;
            }
            return Err(error);
        }
        if let Some(process) = self.process.as_mut() {
            process.output_rx = Some(output_rx);
        }
        self.set_runtime_state("running", Some("running"))
            .await
            .map_err(OperationFailure::internal)?;
        self.inner
            .actor_crash_restarts
            .lock()
            .await
            .remove(&self.instance_id);
        self.inner.events.publish(
            "server.started",
            Some(self.instance_id.clone()),
            serde_json::json!({"pid": pid}),
        );
        if hytale_update_pending {
            let sender = self.sender.clone();
            tokio::spawn(async move {
                tokio::time::sleep(HYTALE_UPDATE_STABILITY_WINDOW).await;
                let _ = sender
                    .send(ActorCommand::ConfirmHytaleUpdate { generation })
                    .await;
            });
        }
        Ok(())
    }

    async fn stop_process(
        &mut self,
        set_desired_stopped: bool,
        force: bool,
    ) -> Result<(), OperationFailure> {
        let desired = set_desired_stopped.then_some("stopped");
        self.set_runtime_state("stopping", desired)
            .await
            .map_err(OperationFailure::internal)?;
        let Some(mut process) = self.process.take() else {
            self.set_runtime_state("stopped", desired)
                .await
                .map_err(OperationFailure::internal)?;
            if !force {
                self.apply_pending_config_after_stop().await;
            }
            return Ok(());
        };

        let result = if force {
            process.force_kill().await
        } else {
            process.graceful_stop().await
        };
        if let Err(error) = result {
            self.process = Some(process);
            return Err(error);
        }
        self.set_runtime_state("stopped", desired)
            .await
            .map_err(OperationFailure::internal)?;
        if !force {
            self.apply_pending_config_after_stop().await;
        }
        self.watchdog_attempts = 0;
        self.inner.events.publish(
            "server.stopped",
            Some(self.instance_id.clone()),
            serde_json::json!({"forced": force}),
        );
        Ok(())
    }

    async fn apply_pending_config_after_stop(&self) {
        let root = match self.instance_root().await {
            Ok(root) => root,
            Err(error) => {
                tracing::warn!(
                    instance_id = %self.instance_id,
                    code = error.code,
                    detail = ?error.internal,
                    "server stopped but queued configuration root was unavailable"
                );
                return;
            }
        };
        if let Err(error) = self.apply_pending_config_changes(&root).await {
            tracing::warn!(
                instance_id = %self.instance_id,
                code = error.code,
                detail = ?error.internal,
                "server stopped with one or more queued configuration changes blocked"
            );
        }
    }

    async fn apply_pending_config_changes(&self, root: &Path) -> Result<(), OperationFailure> {
        config_files::apply_pending(
            &self.inner.pool,
            &self.inner.secrets,
            &self.inner.events,
            root,
            &self.instance_id,
        )
        .await
        .map(|_| ())
        .map_err(|error| {
            OperationFailure::with_internal(
                "config_apply_failed",
                "config_files.apply_failed",
                error,
            )
        })
    }

    async fn console(&mut self, command: &str) -> Result<(), AppError> {
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| AppError::Conflict("servers.not_running".into()))?;
        process.write_stdin(command).await.map_err(|error| {
            tracing::warn!(instance_id = %self.instance_id, %error, "console write failed");
            AppError::Conflict("servers.console_unavailable".into())
        })?;
        self.inner.events.publish(
            "server.console_command",
            Some(self.instance_id.clone()),
            serde_json::json!({"accepted": true}),
        );
        Ok(())
    }

    async fn begin_filesystem_maintenance(
        &mut self,
        allowed_job_id: Option<&str>,
    ) -> Result<String, AppError> {
        if self.filesystem_maintenance_token.is_some() || self.backup_token.is_some() {
            return Err(AppError::Conflict("files.server_must_be_stopped".into()));
        }
        let active_job: bool = if let Some(allowed_job_id) = allowed_job_id {
            let allowed_is_active: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE id = ? AND instance_id = ? \
                 AND state IN ('queued', 'running', 'waiting_for_user'))",
            )
            .bind(allowed_job_id)
            .bind(&self.instance_id)
            .fetch_one(&self.inner.pool)
            .await?;
            if !allowed_is_active {
                return Err(AppError::Conflict("files.server_must_be_stopped".into()));
            }
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE instance_id = ? AND id <> ? \
                 AND state IN ('queued', 'running', 'waiting_for_user'))",
            )
            .bind(&self.instance_id)
            .bind(allowed_job_id)
            .fetch_one(&self.inner.pool)
            .await?
        } else {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE instance_id = ? \
                 AND state IN ('queued', 'running', 'waiting_for_user'))",
            )
            .bind(&self.instance_id)
            .fetch_one(&self.inner.pool)
            .await?
        };
        if active_job {
            return Err(AppError::Conflict("files.server_must_be_stopped".into()));
        }
        let instance = self.instance().await.map_err(|error| {
            if error.code == "server_not_found" {
                AppError::NotFound("servers.not_found".into())
            } else {
                operation_failure_to_app(error)
            }
        })?;
        if self.process.is_some()
            || instance.runtime_state != "stopped"
            || instance.desired_state != "stopped"
        {
            return Err(AppError::Conflict("files.server_must_be_stopped".into()));
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.filesystem_maintenance_token = Some(token.clone());
        Ok(token)
    }

    fn end_filesystem_maintenance(&mut self, token: &str) -> Result<(), AppError> {
        if self.filesystem_maintenance_token.as_deref() == Some(token) {
            self.filesystem_maintenance_token = None;
            self.replay_pending_autostart();
        }
        Ok(())
    }

    fn replay_pending_autostart(&mut self) {
        if !self.filesystem_autostart_pending
            || self.filesystem_maintenance_token.is_some()
            || self.backup_token.is_some()
        {
            return;
        }
        self.filesystem_autostart_pending = false;
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let _ = sender.send(ActorCommand::AutoStart).await;
        });
    }

    async fn auto_start(&mut self) {
        if self.backup_token.is_some() || self.filesystem_maintenance_token.is_some() {
            self.filesystem_autostart_pending = true;
            return;
        }
        let instance = match self.instance().await {
            Ok(instance) => instance,
            Err(error) => {
                tracing::warn!(
                    instance_id = %self.instance_id,
                    code = error.code,
                    detail = ?error.internal,
                    "automatic server start could not load instance state"
                );
                return;
            }
        };
        if instance.desired_state != "running" || self.process.is_some() {
            return;
        }
        if let Err(error) = self.start_process().await {
            tracing::error!(
                instance_id = %self.instance_id,
                code = error.code,
                detail = ?error.internal,
                "automatic server start failed"
            );
            let _ = self.set_runtime_state("crashed", Some("running")).await;
            self.schedule_watchdog().await;
        }
    }

    async fn begin_backup(&mut self) -> Result<Option<String>, AppError> {
        if self.backup_token.is_some() || self.filesystem_maintenance_token.is_some() {
            return Err(AppError::Conflict("backups.server_frozen".into()));
        }
        let instance = self.instance().await.map_err(operation_failure_to_app)?;
        if instance.runtime_state == "stopped" && self.process.is_none() {
            let token = uuid::Uuid::new_v4().to_string();
            self.backup_token = Some(token.clone());
            self.backup_started_stopped = true;
            return Ok(Some(token));
        }
        if instance.runtime_state != "running" || self.process.is_none() {
            return Err(AppError::Conflict("backups.server_must_be_stopped".into()));
        }

        let token = uuid::Uuid::new_v4().to_string();
        self.backup_token = Some(token.clone());
        self.backup_restart_after = true;
        if instance.profile_id == "minecraft-java"
            || instance.profile_id.starts_with("minecraft-java-")
        {
            let freeze = async {
                let process = self
                    .process
                    .as_mut()
                    .ok_or_else(|| AppError::Conflict("servers.not_running".into()))?;
                drain_process_output(process)?;
                process.write_stdin("save-off").await.map_err(|error| {
                    tracing::warn!(instance_id = %self.instance_id, %error, "Minecraft save-off failed");
                    AppError::Conflict("backups.freeze_failed".into())
                })?;
                wait_for_minecraft_save_off(process).await?;
                drain_process_output(process)?;
                process.write_stdin("save-all flush").await.map_err(|error| {
                    tracing::warn!(instance_id = %self.instance_id, %error, "Minecraft save flush failed");
                    AppError::Conflict("backups.freeze_failed".into())
                })?;
                wait_for_minecraft_save(process).await
            }
            .await;
            if let Err(error) = freeze {
                tracing::warn!(
                    instance_id = %self.instance_id,
                    %error,
                    "Minecraft pre-backup flush failed; falling back to a graceful full stop"
                );
            }
        }
        if let Err(error) = self.stop_process(false, false).await {
            self.backup_token = None;
            self.backup_restart_after = false;
            self.backup_started_stopped = false;
            return Err(operation_failure_to_app(error));
        }
        Ok(Some(token))
    }

    async fn end_backup(&mut self, token: &str) -> Result<(), AppError> {
        if self.backup_token.as_deref() == Some(token) {
            self.release_backup_lease(true).await?;
        }
        Ok(())
    }

    async fn release_backup_lease(&mut self, resume: bool) -> Result<(), AppError> {
        if self.backup_token.take().is_none() {
            return Ok(());
        }
        let started_stopped = std::mem::take(&mut self.backup_started_stopped);
        let restart_after = std::mem::take(&mut self.backup_restart_after);
        if !started_stopped && restart_after && resume {
            let sender = self.sender.clone();
            tokio::spawn(async move {
                let _ = sender
                    .send(ActorCommand::ResumeAfterBackup { attempt: 0 })
                    .await;
            });
        }
        self.replay_pending_autostart();
        Ok(())
    }

    async fn resume_after_backup_with_retry(&mut self, attempt: u8) {
        if self.process.is_some() || self.backup_token.is_some() {
            return;
        }
        let instance = match self.instance().await {
            Ok(instance) => instance,
            Err(error) => {
                tracing::warn!(
                    instance_id = %self.instance_id,
                    attempt,
                    code = error.code,
                    detail = ?error.internal,
                    "could not read instance state while resuming after backup"
                );
                self.schedule_backup_resume_retry(attempt.saturating_add(1));
                return;
            }
        };
        if instance.desired_state != "running" {
            if let Err(error) = self.set_runtime_state("stopped", None).await {
                tracing::warn!(
                    instance_id = %self.instance_id,
                    %error,
                    "could not persist stopped state after backup"
                );
            }
            return;
        }
        if let Err(error) = self.set_runtime_state("starting", Some("running")).await {
            tracing::warn!(
                instance_id = %self.instance_id,
                attempt,
                %error,
                "could not persist resume state after backup"
            );
            self.schedule_backup_resume_retry(attempt.saturating_add(1));
            return;
        }
        if let Err(error) = self.start_process().await {
            tracing::error!(
                instance_id = %self.instance_id,
                code = error.code,
                detail = ?error.internal,
                "server could not resume after backup; watchdog will retry"
            );
            self.inner.events.publish(
                "backup.resume_failed",
                Some(self.instance_id.clone()),
                serde_json::json!({"error_code": error.code}),
            );
            if self
                .set_runtime_state("crashed", Some("running"))
                .await
                .is_ok()
            {
                self.schedule_watchdog().await;
            } else {
                self.schedule_backup_resume_retry(attempt.saturating_add(1));
            }
        }
    }

    fn schedule_backup_resume_retry(&self, attempt: u8) {
        let delay = Duration::from_secs(1_u64 << u32::from(attempt.min(6)));
        let sender = self.sender.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = sender
                .send(ActorCommand::ResumeAfterBackup { attempt })
                .await;
        });
    }

    async fn process_exited(&mut self, generation: u64, outcome: ExitOutcome) {
        if self
            .process
            .as_ref()
            .is_none_or(|process| process.generation != generation)
        {
            return;
        }
        let exited_process = self.process.take();
        #[cfg(unix)]
        if let Some(process) = exited_process.as_ref()
            && let Err(error) = hard_kill_group(process.pid, None)
            && error.raw_os_error() != Some(libc::ESRCH)
        {
            tracing::warn!(
                instance_id = %self.instance_id,
                pid = process.pid,
                %error,
                "failed to kill descendants left behind by the exited game process"
            );
        }
        drop(exited_process);
        if self.backup_token.is_some() {
            self.backup_restart_after = true;
            self.backup_started_stopped = false;
            let _ = self.set_runtime_state("crashed", None).await;
            self.inner.events.publish(
                "server.crashed_during_backup",
                Some(self.instance_id.clone()),
                serde_json::json!({"exit_code": outcome.code}),
            );
            return;
        }
        self.backup_token = None;
        self.backup_restart_after = false;
        self.backup_started_stopped = false;
        let instance = match self.instance().await {
            Ok(instance) => instance,
            Err(error) => {
                tracing::error!(instance_id = %self.instance_id, code = error.code, "cannot load exited instance");
                return;
            }
        };
        if instance.desired_state == "stopped" {
            let _ = self.set_runtime_state("stopped", None).await;
            return;
        }
        if instance.profile_id == "hytale" {
            let root = match self.instance_root().await {
                Ok(root) => root,
                Err(error) => {
                    tracing::error!(
                        instance_id = %self.instance_id,
                        code = error.code,
                        detail = ?error.internal,
                        "cannot resolve Hytale update storage"
                    );
                    let _ = self.set_runtime_state("crashed", None).await;
                    return;
                }
            };
            let pending_update = match read_hytale_update_state(&root).await {
                Ok(state) => state.is_some_and(|state| state.phase == HytaleUpdatePhase::Applied),
                Err(error) => {
                    tracing::error!(
                        instance_id = %self.instance_id,
                        code = error.code,
                        detail = ?error.internal,
                        "cannot safely inspect Hytale update state"
                    );
                    let _ = self.set_runtime_state("crashed", None).await;
                    return;
                }
            };
            if pending_update && outcome.elapsed < HYTALE_UPDATE_STABILITY_WINDOW {
                match rollback_hytale_update(&root).await {
                    Ok(state) => {
                        if let Err(error) = restore_hytale_version_metadata(
                            &self.inner.pool,
                            &self.instance_id,
                            &state,
                        )
                        .await
                        {
                            tracing::warn!(instance_id = %self.instance_id, %error, "could not restore Hytale version metadata after rollback");
                        }
                        self.hytale_update_restarts = 0;
                        self.inner.events.publish(
                            "server.update_rolled_back",
                            Some(self.instance_id.clone()),
                            serde_json::json!({
                                "reason": "early_crash",
                                "exit_code": outcome.code,
                            }),
                        );
                        let _ = self.set_runtime_state("starting", Some("running")).await;
                        if let Err(error) = self.start_process().await {
                            tracing::error!(
                                instance_id = %self.instance_id,
                                code = error.code,
                                detail = ?error.internal,
                                "failed to start Hytale after update rollback"
                            );
                            let _ = self.set_runtime_state("crashed", None).await;
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            instance_id = %self.instance_id,
                            code = error.code,
                            detail = ?error.internal,
                            "failed to roll back unstable Hytale update"
                        );
                        let _ = self.set_runtime_state("crashed", None).await;
                        self.inner.events.publish(
                            "server.update_failed",
                            Some(self.instance_id.clone()),
                            serde_json::json!({"code": error.code}),
                        );
                    }
                }
                return;
            }
            if pending_update {
                match finalize_hytale_update(&root).await {
                    Ok(true) => self.hytale_update_restarts = 0,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::error!(
                            instance_id = %self.instance_id,
                            code = error.code,
                            detail = ?error.internal,
                            "failed to finalize Hytale update after its stability window"
                        );
                        let _ = self.set_runtime_state("crashed", None).await;
                        return;
                    }
                }
            }
            if outcome.code == Some(HYTALE_UPDATE_EXIT_CODE) {
                self.handle_hytale_update_exit(&root, &instance).await;
                return;
            }
        }
        let _ = self.set_runtime_state("crashed", None).await;
        self.inner.events.publish(
            "server.crashed",
            Some(self.instance_id.clone()),
            serde_json::json!({"exit_code": outcome.code, "success": outcome.success}),
        );
        self.schedule_watchdog().await;
    }

    async fn handle_hytale_update_exit(&mut self, root: &Path, instance: &RuntimeInstance) {
        if self.hytale_update_restarts >= 2 {
            let _ = self.set_runtime_state("crashed", None).await;
            self.inner.events.publish(
                "server.update_failed",
                Some(self.instance_id.clone()),
                serde_json::json!({"code": "hytale_update_restart_limit"}),
            );
            return;
        }
        self.hytale_update_restarts += 1;
        if let Err(error) = apply_hytale_staged_update(
            root,
            instance.installed_version.clone(),
            instance.installed_build.clone(),
        )
        .await
        {
            tracing::error!(
                instance_id = %self.instance_id,
                code = error.code,
                detail = ?error.internal,
                "Hytale exited for an update but its staging tree is invalid"
            );
            let _ = self.set_runtime_state("crashed", None).await;
            self.inner.events.publish(
                "server.update_failed",
                Some(self.instance_id.clone()),
                serde_json::json!({"code": error.code}),
            );
            return;
        }
        if let Err(error) = sqlx::query(
            "UPDATE instances SET installed_version = NULL, installed_build = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&self.instance_id)
        .execute(&self.inner.pool)
        .await
        {
            tracing::warn!(
                instance_id = %self.instance_id,
                %error,
                "could not invalidate stale Hytale version metadata after self-update"
            );
        }
        self.inner.events.publish(
            "server.update_applied",
            Some(self.instance_id.clone()),
            serde_json::json!({"source": "hytale_exit_code_8"}),
        );
        let _ = self.set_runtime_state("starting", Some("running")).await;
        if let Err(start_error) = self.start_process().await {
            tracing::error!(
                instance_id = %self.instance_id,
                code = start_error.code,
                detail = ?start_error.internal,
                "updated Hytale server failed its initial start"
            );
            match rollback_hytale_update(root).await {
                Ok(state) => {
                    if let Err(error) =
                        restore_hytale_version_metadata(&self.inner.pool, &self.instance_id, &state)
                            .await
                    {
                        tracing::warn!(instance_id = %self.instance_id, %error, "could not restore Hytale version metadata after rollback");
                    }
                    self.inner.events.publish(
                        "server.update_rolled_back",
                        Some(self.instance_id.clone()),
                        serde_json::json!({"reason": "start_failed"}),
                    );
                    if let Err(rollback_start_error) = self.start_process().await {
                        tracing::error!(
                            instance_id = %self.instance_id,
                            code = rollback_start_error.code,
                            detail = ?rollback_start_error.internal,
                            "Hytale rollback was restored but could not be started"
                        );
                        let _ = self.set_runtime_state("crashed", None).await;
                    }
                }
                Err(rollback_error) => {
                    tracing::error!(
                        instance_id = %self.instance_id,
                        code = rollback_error.code,
                        detail = ?rollback_error.internal,
                        "Hytale update start failed and rollback also failed"
                    );
                    let _ = self.set_runtime_state("crashed", None).await;
                }
            }
        }
    }

    async fn confirm_hytale_update(&mut self, generation: u64) {
        if self
            .process
            .as_ref()
            .is_none_or(|process| process.generation != generation)
        {
            return;
        }
        let Ok(root) = self.instance_root().await else {
            return;
        };
        match finalize_hytale_update(&root).await {
            Ok(true) => {
                self.hytale_update_restarts = 0;
                self.inner.events.publish(
                    "server.update_confirmed",
                    Some(self.instance_id.clone()),
                    serde_json::json!({"stability_seconds": HYTALE_UPDATE_STABILITY_WINDOW.as_secs()}),
                );
            }
            Ok(false) => {}
            Err(error) => {
                tracing::error!(
                    instance_id = %self.instance_id,
                    code = error.code,
                    detail = ?error.internal,
                    "failed to finalize stable Hytale update"
                );
            }
        }
    }

    async fn schedule_watchdog(&mut self) {
        let Ok(instance) = self.instance().await else {
            return;
        };
        if !instance.watchdog_enabled
            || instance.desired_state != "running"
            || self.watchdog_attempts >= MAX_WATCHDOG_RESTARTS
        {
            return;
        }
        self.watchdog_attempts += 1;
        let delay = Duration::from_secs(2_u64.pow(u32::from(self.watchdog_attempts)).min(60));
        let sender = self.sender.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = sender.send(ActorCommand::WatchdogRestart).await;
        });
    }

    async fn watchdog_restart(&mut self) {
        if self.process.is_some()
            || self.backup_token.is_some()
            || self.filesystem_maintenance_token.is_some()
        {
            return;
        }
        let Ok(instance) = self.instance().await else {
            return;
        };
        if instance.desired_state != "running" || !instance.watchdog_enabled {
            return;
        }
        if let Err(error) = self.start_process().await {
            tracing::warn!(
                instance_id = %self.instance_id,
                attempt = self.watchdog_attempts,
                code = error.code,
                detail = ?error.internal,
                "watchdog restart failed"
            );
            let _ = self.set_runtime_state("crashed", None).await;
            self.schedule_watchdog().await;
        }
    }

    async fn build_launch_spec(
        &self,
        instance: &RuntimeInstance,
    ) -> Result<PreparedLaunch, OperationFailure> {
        let root = self.instance_root().await?;
        let settings: Value =
            serde_json::from_str(&instance.settings).map_err(OperationFailure::internal)?;
        if installers::native_install_supported(&instance.profile_id) {
            return self
                .build_native_launch_spec(instance, &root, &settings)
                .await;
        }
        if let Some(steam_profile) = steam_profile_for_instance(&self.inner.pool, instance).await? {
            return build_custom_steam_launch_spec(instance, &root, &settings, &steam_profile)
                .await;
        }
        let mut redactions = Vec::new();
        for name in allowed_secret_names(&instance.profile_id) {
            if let Some(secret) = self
                .inner
                .secrets
                .get(&self.inner.pool, &self.instance_id, name)
                .await
                .map_err(OperationFailure::internal)?
            {
                redactions.push(secret);
            }
        }

        let (executable, args, stop) = match instance.profile_id.as_str() {
            "valheim" => {
                let password = self
                    .inner
                    .secrets
                    .get(&self.inner.pool, &self.instance_id, "server_password")
                    .await
                    .map_err(OperationFailure::internal)?
                    .ok_or_else(|| {
                        OperationFailure::new("secret_missing", "servers.required_secret_missing")
                    })?;
                let server_name = required_string(&settings, "server_name")?;
                let world_name = required_string(&settings, "world_name")?;
                let port = integer_setting(&settings, "port", 2456)?;
                tokio::fs::create_dir_all(root.join("data"))
                    .await
                    .map_err(OperationFailure::internal)?;
                let mut args = vec![
                    "-nographics".to_string(),
                    "-batchmode".to_string(),
                    "-name".to_string(),
                    server_name,
                    "-port".to_string(),
                    port.to_string(),
                    "-world".to_string(),
                    world_name,
                    "-password".to_string(),
                    password,
                    "-savedir".to_string(),
                    root.join("data").to_string_lossy().into_owned(),
                ];
                if settings
                    .get("crossplay")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    args.push("-crossplay".into());
                }
                (
                    platform_executable("game/valheim_server.x86_64", "game/valheim_server.exe")?,
                    args,
                    StopStrategy::Interrupt {
                        timeout_seconds: 60,
                    },
                )
            }
            "palworld" => {
                let port = integer_setting(&settings, "port", 8211)?;
                self.write_palworld_settings(&root, &settings).await?;
                let mut args = vec![format!("-port={port}")];
                if settings
                    .get("public_server")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    args.push("-publiclobby".into());
                }
                (
                    platform_executable("game/PalServer.sh", "game/PalServer.exe")?,
                    args,
                    StopStrategy::Interrupt {
                        timeout_seconds: 60,
                    },
                )
            }
            "satisfactory" => {
                let port = integer_setting(&settings, "port", 7777)?;
                let reliable_port = integer_setting(&settings, "reliable_port", 8888)?;
                (
                    platform_executable("game/FactoryServer.sh", "game/FactoryServer.exe")?,
                    vec![
                        "-unattended".into(),
                        "-log".into(),
                        format!("-Port={port}"),
                        format!("-ReliablePort={reliable_port}"),
                        format!("-ExternalReliablePort={reliable_port}"),
                    ],
                    StopStrategy::Interrupt {
                        timeout_seconds: 90,
                    },
                )
            }
            "seven-days-to-die" => {
                self.write_seven_days_settings(&root, &settings).await?;
                (
                    platform_executable(
                        "game/7DaysToDieServer.x86_64",
                        "game/7DaysToDieServer.exe",
                    )?,
                    vec![
                        "-logfile".into(),
                        "-".into(),
                        "-quit".into(),
                        "-batchmode".into(),
                        "-nographics".into(),
                        "-configfile=dmx-serverconfig.xml".into(),
                        "-dedicated".into(),
                    ],
                    StopStrategy::Stdin {
                        command: "shutdown".into(),
                        timeout_seconds: 90,
                    },
                )
            }
            "project-zomboid" => {
                let admin_password = self
                    .inner
                    .secrets
                    .get(&self.inner.pool, &self.instance_id, "admin_password")
                    .await
                    .map_err(OperationFailure::internal)?
                    .ok_or_else(|| {
                        OperationFailure::new("secret_missing", "servers.required_secret_missing")
                    })?;
                self.write_project_zomboid_settings(&root, &settings)
                    .await?;
                (
                    platform_executable("game/start-server.sh", "game/ProjectZomboid64.exe")?,
                    vec![
                        "-servername".into(),
                        required_string(&settings, "server_name")?,
                        "-adminpassword".into(),
                        admin_password,
                    ],
                    StopStrategy::Stdin {
                        command: "quit".into(),
                        timeout_seconds: 90,
                    },
                )
            }
            "rust" => {
                let rcon_password = self
                    .inner
                    .secrets
                    .get(&self.inner.pool, &self.instance_id, "rcon_password")
                    .await
                    .map_err(OperationFailure::internal)?
                    .ok_or_else(|| {
                        OperationFailure::new("secret_missing", "servers.required_secret_missing")
                    })?;
                let port = integer_setting(&settings, "port", 28_015)?;
                let rcon_port = integer_setting(&settings, "rcon_port", 28_016)?;
                let query_port = integer_setting(&settings, "query_port", 28_017)?;
                let max_players = integer_setting(&settings, "max_players", 50)?;
                let world_size = integer_setting(&settings, "world_size", 3500)?;
                let seed = settings
                    .get("seed")
                    .and_then(Value::as_u64)
                    .unwrap_or(12_345);
                (
                    platform_executable("game/RustDedicated", "game/RustDedicated.exe")?,
                    vec![
                        "-batchmode".into(),
                        "+server.port".into(),
                        port.to_string(),
                        "+server.queryport".into(),
                        query_port.to_string(),
                        "+rcon.port".into(),
                        rcon_port.to_string(),
                        "+rcon.password".into(),
                        rcon_password,
                        "+rcon.web".into(),
                        "1".into(),
                        "+server.hostname".into(),
                        required_string(&settings, "server_name")?,
                        "+server.identity".into(),
                        required_string(&settings, "identity")?,
                        "+server.maxplayers".into(),
                        max_players.to_string(),
                        "+server.worldsize".into(),
                        world_size.to_string(),
                        "+server.seed".into(),
                        seed.to_string(),
                    ],
                    StopStrategy::Interrupt {
                        timeout_seconds: 90,
                    },
                )
            }
            _ => {
                return Err(OperationFailure::new(
                    "runtime_not_implemented",
                    "servers.runtime_not_implemented",
                ));
            }
        };
        let mut spec =
            LaunchSpec::for_instance(&root, &executable, "game", args).map_err(|_| {
                OperationFailure::new("invalid_launch_spec", "servers.invalid_launch_spec")
            })?;
        validate_launch_paths(&root, &mut spec).await?;
        spec.env = filtered_environment(&root, &instance.profile_id);
        Ok(PreparedLaunch {
            spec,
            stop,
            redactions,
        })
    }

    async fn build_native_launch_spec(
        &self,
        instance: &RuntimeInstance,
        root: &Path,
        settings: &Value,
    ) -> Result<PreparedLaunch, OperationFailure> {
        let hytale_native_workdir = if instance.profile_id == "hytale" {
            Some(prepare_hytale_native_workdir(root).await?)
        } else {
            None
        };
        let game = tokio::fs::canonicalize(root.join("game"))
            .await
            .map_err(OperationFailure::internal)?;
        installers::apply_runtime_configuration(&instance.profile_id, settings, &game)
            .await
            .map_err(installer_failure)?;
        let plan = installers::native_launch_plan(&instance.profile_id, settings, &game)
            .await
            .map_err(installer_failure)?;
        if !plan.restart_exit_codes.is_empty() && instance.profile_id != "hytale" {
            return Err(OperationFailure::new(
                "native_restart_protocol_not_supported",
                "servers.runtime_not_implemented",
            ));
        }
        ensure_no_symlink_components(&game, &plan.cwd_relative).await?;
        let cwd = crate::domain::v1::safe_join(&game, &plan.cwd_relative).map_err(|_| {
            OperationFailure::new("invalid_launch_spec", "servers.invalid_launch_spec")
        })?;
        let cwd = tokio::fs::canonicalize(cwd)
            .await
            .map_err(OperationFailure::internal)?;
        if !cwd.starts_with(&game) {
            return Err(OperationFailure::new(
                "launch_path_escape",
                "servers.invalid_launch_spec",
            ));
        }
        let executable = match plan.executable {
            InstallerExecutable::ManagedJava { major } => {
                let executable = managed_java_executable(&self.inner.settings, major).await?;
                validate_java_runtime(&executable, major).await?;
                executable
            }
            InstallerExecutable::InstanceRelative { path } => {
                ensure_no_symlink_components(&game, &path).await?;
                let executable = crate::domain::v1::safe_join(&game, &path).map_err(|_| {
                    OperationFailure::new("invalid_launch_spec", "servers.invalid_launch_spec")
                })?;
                let executable = tokio::fs::canonicalize(executable)
                    .await
                    .map_err(OperationFailure::internal)?;
                if !executable.starts_with(&game) {
                    return Err(OperationFailure::new(
                        "launch_path_escape",
                        "servers.invalid_launch_spec",
                    ));
                }
                executable
            }
        };
        let args = plan
            .args
            .into_iter()
            .map(|argument| native_launch_argument(argument, hytale_native_workdir.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut environment = filtered_environment(root, &instance.profile_id);
        environment.extend(plan.env);
        Ok(PreparedLaunch {
            spec: LaunchSpec {
                executable,
                cwd,
                args,
                env: environment,
            },
            stop: plan.stop,
            redactions: Vec::new(),
        })
    }

    async fn write_palworld_settings(
        &self,
        root: &Path,
        settings: &Value,
    ) -> Result<(), OperationFailure> {
        let server_name = ini_value(&required_string(settings, "server_name")?)?;
        let server_password = self
            .inner
            .secrets
            .get(&self.inner.pool, &self.instance_id, "server_password")
            .await
            .map_err(OperationFailure::internal)?
            .unwrap_or_default();
        let admin_password = self
            .inner
            .secrets
            .get(&self.inner.pool, &self.instance_id, "admin_password")
            .await
            .map_err(OperationFailure::internal)?
            .unwrap_or_default();
        let port = integer_setting(settings, "port", 8211)?;
        let platform_dir = if cfg!(windows) {
            "WindowsServer"
        } else {
            "LinuxServer"
        };
        let game = root.join("game");
        let relative_directory = format!("Pal/Saved/Config/{platform_dir}");
        ensure_no_symlink_components(&game, &relative_directory).await?;
        let directory = game.join(&relative_directory);
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(OperationFailure::internal)?;
        ensure_no_symlink_components(&game, &relative_directory).await?;
        let destination = directory.join("PalWorldSettings.ini");
        let existing = read_bounded_runtime_text(&destination, 4 * 1024 * 1024).await?;
        let contents = merge_palworld_settings(
            existing.as_deref().unwrap_or_default(),
            &server_name,
            &ini_value(&server_password)?,
            &ini_value(&admin_password)?,
            port,
        )?;
        let temporary = directory.join(format!(
            ".PalWorldSettings-{}.tmp",
            uuid::Uuid::new_v4().as_simple()
        ));
        write_private_runtime_file(&temporary, contents.as_bytes()).await?;
        replace_runtime_file(&temporary, &destination).await
    }

    async fn write_seven_days_settings(
        &self,
        root: &Path,
        settings: &Value,
    ) -> Result<(), OperationFailure> {
        let game = root.join("game");
        let data = root.join("data/7days-to-die");
        tokio::fs::create_dir_all(&data)
            .await
            .map_err(OperationFailure::internal)?;
        let password = self
            .inner
            .secrets
            .get(&self.inner.pool, &self.instance_id, "server_password")
            .await
            .map_err(OperationFailure::internal)?
            .unwrap_or_default();
        let values = [
            ("ServerName", required_string(settings, "server_name")?),
            ("ServerPassword", password),
            (
                "ServerMaxPlayerCount",
                integer_setting(settings, "max_players", 8)?.to_string(),
            ),
            (
                "ServerPort",
                integer_setting(settings, "port", 26_900)?.to_string(),
            ),
            (
                "ServerVisibility",
                if settings
                    .get("public_server")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "2".to_string()
                } else {
                    "0".to_string()
                },
            ),
            ("GameWorld", required_string(settings, "world_name")?),
            ("GameName", required_string(settings, "game_name")?),
            ("UserDataFolder", data.to_string_lossy().into_owned()),
            ("TelnetEnabled", "false".to_string()),
            ("WebDashboardEnabled", "false".to_string()),
        ];
        let destination = game.join("dmx-serverconfig.xml");
        let existing = read_bounded_runtime_text(&destination, 4 * 1024 * 1024).await?;
        let contents = merge_seven_days_settings(existing.as_deref().unwrap_or_default(), &values)?;
        write_runtime_configuration(&game, "dmx-serverconfig.xml", contents.as_bytes()).await
    }

    async fn write_project_zomboid_settings(
        &self,
        root: &Path,
        settings: &Value,
    ) -> Result<(), OperationFailure> {
        let name = required_string(settings, "server_name")?;
        let directory = root.join("data/Zomboid/Server");
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(OperationFailure::internal)?;
        let values = [
            (
                "DefaultPort",
                integer_setting(settings, "port", 16_261)?.to_string(),
            ),
            (
                "SteamPort1",
                integer_setting(settings, "steam_port", 8_766)?.to_string(),
            ),
            (
                "SteamPort2",
                integer_setting(settings, "steam_query_port", 8_767)?.to_string(),
            ),
            ("Public", "false".to_string()),
            ("PauseEmpty", "true".to_string()),
        ];
        let destination = directory.join(format!("{name}.ini"));
        let existing = read_bounded_runtime_text(&destination, 4 * 1024 * 1024).await?;
        let contents = merge_ini_settings(existing.as_deref().unwrap_or_default(), &values);
        write_runtime_configuration(&directory, &format!("{name}.ini"), contents.as_bytes()).await
    }

    async fn instance(&self) -> Result<RuntimeInstance, OperationFailure> {
        load_runtime_instance(&self.inner.pool, &self.instance_id).await
    }

    async fn instance_root(&self) -> Result<PathBuf, OperationFailure> {
        Ok(
            instance_storage::resolve(&self.inner.pool, &self.inner.settings, &self.instance_id)
                .await
                .map_err(OperationFailure::internal)?
                .root,
        )
    }

    async fn write_install_log(&self, root: &Path, message: &str) -> Result<(), OperationFailure> {
        let mut writer = RotatingLog::open(root.join("logs/install.combined.log"))
            .await
            .map_err(OperationFailure::internal)?;
        writer
            .write_line(message)
            .await
            .map_err(OperationFailure::internal)?;
        self.inner.events.publish(
            "server.log",
            Some(self.instance_id.clone()),
            serde_json::json!({"stream": "install", "message": message}),
        );
        Ok(())
    }

    async fn write_hytale_diagnostic(&self, combined_log: &Arc<Mutex<RotatingLog>>, message: &str) {
        let write_result = {
            let mut writer = combined_log.lock().await;
            writer.write_line(message).await
        };
        if let Err(error) = write_result {
            tracing::warn!(instance_id = %self.instance_id, %error, "Hytale diagnostic log write failed");
        }
        self.inner.events.publish(
            "server.log",
            Some(self.instance_id.clone()),
            serde_json::json!({"stream": "install", "message": message}),
        );
    }

    async fn set_runtime_state(
        &self,
        runtime_state: &str,
        desired_state: Option<&str>,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(desired_state) = desired_state {
            sqlx::query(
                "UPDATE instances SET runtime_state = ?, desired_state = ?, updated_at = ? WHERE id = ?",
            )
            .bind(runtime_state)
            .bind(desired_state)
            .bind(now)
            .bind(&self.instance_id)
            .execute(&self.inner.pool)
            .await?;
        } else {
            sqlx::query("UPDATE instances SET runtime_state = ?, updated_at = ? WHERE id = ?")
                .bind(runtime_state)
                .bind(now)
                .bind(&self.instance_id)
                .execute(&self.inner.pool)
                .await?;
        }
        self.publish_state().await;
        Ok(())
    }

    async fn publish_state(&self) {
        if let Ok(instance) = self.instance().await {
            self.inner.events.publish(
                "server.state",
                Some(self.instance_id.clone()),
                serde_json::json!({
                    "installation_state": instance.installation_state,
                    "desired_state": instance.desired_state,
                    "runtime_state": instance.runtime_state,
                }),
            );
        }
    }

    async fn publish_job(&self, job_id: &str) {
        if let Ok(job) = jobs::get(&self.inner.pool, job_id).await {
            self.inner.events.publish(
                "job.updated",
                Some(self.instance_id.clone()),
                serde_json::to_value(job).unwrap_or_default(),
            );
        }
    }
}

fn native_launch_argument(
    argument: String,
    hytale_native_workdir: Option<&Path>,
) -> Result<OsString, OperationFailure> {
    if argument.contains('\0') || argument.len() > 8_192 {
        return Err(OperationFailure::new(
            "invalid_launch_spec",
            "servers.invalid_launch_spec",
        ));
    }
    let Some(workdir) = hytale_native_workdir else {
        return Ok(OsString::from(argument));
    };
    if !workdir.is_absolute() {
        return Err(OperationFailure::new(
            "hytale_native_workdir_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    for property in [
        "-Djava.io.tmpdir",
        "-Djansi.tmpdir",
        "-Dio.netty.native.workdir",
    ] {
        if argument == format!("{property}={}", installers::hytale::NATIVE_WORKDIR_RELATIVE) {
            let mut resolved = OsString::from(format!("{property}="));
            resolved.push(workdir.as_os_str());
            return Ok(resolved);
        }
    }
    Ok(OsString::from(argument))
}

async fn expire_bedrock_upload_wait(sender: mpsc::Sender<ActorCommand>, job_id: String) {
    tokio::time::sleep(INSTALL_TIMEOUT).await;
    if sender
        .send(ActorCommand::AbortWaitingInstall {
            job_id: job_id.clone(),
            reason: WaitingInstallAbort::TimedOut,
            response: None,
        })
        .await
        .is_err()
    {
        tracing::error!(%job_id, "failed to queue Bedrock upload timeout cleanup");
    }
}

#[derive(Debug, FromRow)]
struct RuntimeInstance {
    #[allow(dead_code)]
    id: String,
    profile_id: String,
    profile_revision: i64,
    settings: String,
    config_version: i64,
    installation_state: String,
    installed_version: Option<String>,
    installed_build: Option<String>,
    desired_state: String,
    runtime_state: String,
    auto_start: bool,
    watchdog_enabled: bool,
}

async fn load_runtime_instance(
    pool: &DbPool,
    instance_id: &str,
) -> Result<RuntimeInstance, OperationFailure> {
    sqlx::query_as(
        "SELECT id, profile_id, profile_revision, settings, config_version, installation_state, installed_version, \
         installed_build, desired_state, runtime_state, auto_start, watchdog_enabled \
         FROM instances WHERE id = ?",
    )
    .bind(instance_id)
    .fetch_optional(pool)
    .await
    .map_err(OperationFailure::internal)?
    .ok_or_else(|| OperationFailure::new("server_not_found", "servers.not_found"))
}

fn game_update_fingerprint(instance: &RuntimeInstance) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}",
        instance.profile_id,
        instance.settings,
        instance.installation_state,
        instance.installed_version.as_deref().unwrap_or_default(),
        instance.installed_build.as_deref().unwrap_or_default(),
    )
}

fn game_update_status_from_target(
    instance: &RuntimeInstance,
    available_version: Option<String>,
    available_build: Option<String>,
    state: GameUpdateState,
) -> GameUpdateStatus {
    GameUpdateStatus {
        state,
        installed_version: instance.installed_version.clone(),
        installed_build: instance.installed_build.clone(),
        available_version,
        available_build,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn has_game_update(
    instance: &RuntimeInstance,
    available_version: Option<&str>,
    available_build: Option<&str>,
) -> bool {
    available_version
        .zip(instance.installed_version.as_deref())
        .is_some_and(|(available, installed)| is_newer_release(available, installed))
        || available_build
            .zip(instance.installed_build.as_deref())
            .is_some_and(|(available, installed)| {
                let Ok(available) = available.parse::<u128>() else {
                    return false;
                };
                let Ok(installed) = installed.parse::<u128>() else {
                    return false;
                };
                available > installed
            })
}

fn is_newer_release(available: &str, installed: &str) -> bool {
    if let (Ok(available), Ok(installed)) = (
        semver::Version::parse(available.trim_start_matches(['v', 'V'])),
        semver::Version::parse(installed.trim_start_matches(['v', 'V'])),
    ) {
        return available > installed;
    }
    let Some(mut available) = numeric_release_components(available) else {
        return false;
    };
    let Some(mut installed) = numeric_release_components(installed) else {
        return false;
    };
    let width = available.len().max(installed.len());
    available.resize(width, 0);
    installed.resize(width, 0);
    available > installed
}

fn numeric_release_components(value: &str) -> Option<Vec<u64>> {
    let value = value.trim_start_matches(['v', 'V']);
    let core = value.split(['-', '+']).next()?;
    if core.is_empty() {
        return None;
    }
    core.split('.')
        .map(|component| component.parse::<u64>().ok())
        .collect()
}

#[derive(Debug, Clone, FromRow)]
struct UpdateTransaction {
    instance_id: String,
    job_id: String,
    previous_installation_state: String,
    previous_installed_version: Option<String>,
    previous_installed_build: Option<String>,
    previous_settings: Option<String>,
    previous_config_version: Option<i64>,
    previous_desired_state: String,
    restart_after: bool,
    phase: String,
}

struct PreparedLaunch {
    spec: LaunchSpec,
    stop: StopStrategy,
    redactions: Vec<String>,
}

struct ManagedProcess {
    pid: u32,
    stdin: Option<ChildStdin>,
    exit_rx: watch::Receiver<Option<ExitOutcome>>,
    generation: u64,
    stop: StopStrategy,
    output_rx: Option<mpsc::Receiver<String>>,
    _metrics_stop: watch::Sender<bool>,
    #[cfg(windows)]
    _job: WindowsJob,
}

#[cfg(unix)]
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // The actor owns the only ManagedProcess handle. If its task panics or
        // is aborted, synchronously kill the whole process group before that
        // ownership disappears so no game process can outlive supervision.
        if self.pid == 0 {
            return;
        }
        if let Err(error) = hard_kill_group(self.pid, None)
            && error.raw_os_error() != Some(libc::ESRCH)
        {
            tracing::error!(pid = self.pid, %error, "failed to kill game process group on supervisor drop");
        }
    }
}

impl ManagedProcess {
    async fn write_stdin(&mut self, command: &str) -> std::io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin is unavailable")
        })?;
        stdin.write_all(command.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    }

    async fn graceful_stop(&mut self) -> Result<(), OperationFailure> {
        let (timeout_seconds, signal) = match self.stop.clone() {
            StopStrategy::Stdin {
                command,
                timeout_seconds,
            } => {
                if let Err(error) = self.write_stdin(&command).await {
                    tracing::warn!(pid = self.pid, %error, "stdin stop failed; falling back to termination signal");
                    (timeout_seconds, Some(StopSignal::Terminate))
                } else {
                    (timeout_seconds, None)
                }
            }
            StopStrategy::Interrupt { timeout_seconds } => {
                (timeout_seconds, Some(StopSignal::Interrupt))
            }
            StopStrategy::Terminate { timeout_seconds } => {
                (timeout_seconds, Some(StopSignal::Terminate))
            }
        };
        if let Some(signal) = signal {
            send_group_signal(self.pid, signal, self.windows_job_handle()).map_err(|error| {
                OperationFailure::with_internal(
                    "graceful_stop_failed",
                    "servers.graceful_stop_failed",
                    error,
                )
            })?;
        }
        if self
            .wait(Duration::from_secs(u64::from(timeout_seconds)))
            .await
        {
            return Ok(());
        }
        self.force_kill().await
    }

    async fn force_kill(&mut self) -> Result<(), OperationFailure> {
        hard_kill_group(self.pid, self.windows_job_handle()).map_err(|error| {
            OperationFailure::with_internal("server_kill_failed", "servers.kill_failed", error)
        })?;
        if self.wait(Duration::from_secs(15)).await {
            Ok(())
        } else {
            Err(OperationFailure::new(
                "server_kill_timeout",
                "servers.kill_timeout",
            ))
        }
    }

    async fn wait(&mut self, duration: Duration) -> bool {
        if self.exit_rx.borrow().is_some() {
            return true;
        }
        tokio::time::timeout(duration, async {
            loop {
                if self.exit_rx.changed().await.is_err() || self.exit_rx.borrow().is_some() {
                    return;
                }
            }
        })
        .await
        .is_ok()
    }

    #[cfg(windows)]
    fn windows_job_handle(&self) -> Option<isize> {
        Some(self._job.handle)
    }

    #[cfg(not(windows))]
    fn windows_job_handle(&self) -> Option<isize> {
        None
    }
}

async fn wait_for_minecraft_save(process: &mut ManagedProcess) -> Result<(), AppError> {
    wait_for_minecraft_output(process, minecraft_save_completed).await
}

async fn wait_for_minecraft_save_off(process: &mut ManagedProcess) -> Result<(), AppError> {
    wait_for_minecraft_output(process, minecraft_save_off_completed).await
}

async fn wait_for_minecraft_output(
    process: &mut ManagedProcess,
    completed: fn(&str) -> bool,
) -> Result<(), AppError> {
    let mut exit_rx = process.exit_rx.clone();
    let output = process
        .output_rx
        .as_mut()
        .ok_or_else(|| AppError::Conflict("backups.console_observer_unavailable".into()))?;
    tokio::time::timeout(MINECRAFT_SAVE_TIMEOUT, async {
        loop {
            tokio::select! {
                line = output.recv() => {
                    let line = line.ok_or_else(|| AppError::Conflict("backups.console_observer_unavailable".into()))?;
                    if completed(&line) {
                        return Ok(());
                    }
                }
                changed = exit_rx.changed() => {
                    if changed.is_err() || exit_rx.borrow().is_some() {
                        return Err(AppError::Conflict("backups.server_exited_during_freeze".into()));
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| AppError::Conflict("backups.freeze_timeout".into()))?
}

fn drain_process_output(process: &mut ManagedProcess) -> Result<(), AppError> {
    let output = process
        .output_rx
        .as_mut()
        .ok_or_else(|| AppError::Conflict("backups.console_observer_unavailable".into()))?;
    loop {
        match output.try_recv() {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(()),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Err(AppError::Conflict(
                    "backups.console_observer_unavailable".into(),
                ));
            }
        }
    }
}

fn minecraft_save_completed(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("saved the game")
        || line.contains("saved the world")
        || (line.contains("saved ") && line.contains(" chunks"))
}

fn minecraft_save_off_completed(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    (line.contains("saving") && line.contains("disabled"))
        || line.contains("saving is already turned off")
}

#[derive(Debug, Clone)]
struct ExitOutcome {
    success: bool,
    code: Option<i32>,
    elapsed: Duration,
}

#[derive(Debug)]
struct OperationFailure {
    code: &'static str,
    client_message: &'static str,
    internal: Option<String>,
    cancelled: bool,
    deferred: bool,
}

fn operation_failure_to_app(error: OperationFailure) -> AppError {
    tracing::warn!(code = error.code, detail = ?error.internal, "runtime state unavailable for backup");
    AppError::Internal(error.client_message.into())
}

impl OperationFailure {
    fn new(code: &'static str, client_message: &'static str) -> Self {
        Self {
            code,
            client_message,
            internal: None,
            cancelled: false,
            deferred: false,
        }
    }

    fn with_internal(
        code: &'static str,
        client_message: &'static str,
        error: impl std::fmt::Display,
    ) -> Self {
        Self {
            code,
            client_message,
            internal: Some(error.to_string()),
            cancelled: false,
            deferred: false,
        }
    }

    fn cancelled(code: &'static str, client_message: &'static str) -> Self {
        Self {
            code,
            client_message,
            internal: None,
            cancelled: true,
            deferred: false,
        }
    }

    fn deferred(code: &'static str, client_message: &'static str) -> Self {
        Self {
            code,
            client_message,
            internal: None,
            cancelled: false,
            deferred: true,
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self::with_internal("internal_error", "errors.internal", error)
    }
}

fn installer_failure(error: installers::InstallerError) -> OperationFailure {
    OperationFailure {
        code: error.code,
        client_message: error.client_message,
        internal: error.internal,
        cancelled: false,
        deferred: false,
    }
}

fn public_install_failure_detail(error: &OperationFailure, instance_root: &Path) -> Option<String> {
    if !matches!(
        error.code,
        "archive_extract_failed"
            | "archive_invalid"
            | "archive_open_failed"
            | "archive_worker_failed"
            | "hytale_archive_read_failed"
            | "hytale_layout_invalid"
            | "hytale_preserve_failed"
            | "install_artifact_invalid"
            | "install_artifact_missing"
            | "install_artifact_read_failed"
            | "install_artifact_worker_failed"
            | "install_metadata_failed"
            | "install_metadata_invalid"
            | "install_metadata_missing"
            | "install_metadata_worker_failed"
            | "install_tree_invalid"
            | "install_tree_worker_failed"
            | "staging_create_failed"
            | "staging_read_failed"
    ) {
        return None;
    }
    let detail = error.internal.as_deref()?;
    let instance_root = instance_root.to_string_lossy();
    let redacted = detail.replace(instance_root.as_ref(), "<instance>");
    let mut sanitized = redacted
        .chars()
        .map(|character| {
            if matches!(character, '\n' | '\r' | '\t') {
                ' '
            } else {
                character
            }
        })
        .filter(|character| !character.is_control())
        .take(512)
        .collect::<String>();
    sanitized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    (!sanitized.is_empty()).then_some(sanitized)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HytaleUpdatePhase {
    Prepared,
    Applied,
    RollingBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct HytaleUpdateState {
    schema: u8,
    phase: HytaleUpdatePhase,
    created_at: String,
    previous_version: Option<String>,
    previous_build: Option<String>,
}

impl HytaleUpdateState {
    fn new(
        phase: HytaleUpdatePhase,
        previous_version: Option<String>,
        previous_build: Option<String>,
    ) -> Self {
        Self {
            schema: 1,
            phase,
            created_at: chrono::Utc::now().to_rfc3339(),
            previous_version,
            previous_build,
        }
    }
}

async fn restore_hytale_version_metadata(
    pool: &DbPool,
    instance_id: &str,
    state: &HytaleUpdateState,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE instances SET installed_version = ?, installed_build = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&state.previous_version)
    .bind(&state.previous_build)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn apply_hytale_staged_update(
    root: &Path,
    previous_version: Option<String>,
    previous_build: Option<String>,
) -> Result<(), OperationFailure> {
    if read_hytale_update_state(root).await?.is_some() {
        return Err(OperationFailure::new(
            "hytale_update_already_pending",
            "servers.update_failed",
        ));
    }
    let game = root.join("game");
    let provider_staging = game.join("updater/staging");
    let candidate = root.join(HYTALE_UPDATE_CANDIDATE);
    let rollback = root.join(HYTALE_UPDATE_ROLLBACK);
    let failed = root.join(HYTALE_UPDATE_FAILED);
    remove_hytale_internal_path(&candidate).await?;
    remove_hytale_internal_path(&rollback).await?;
    remove_hytale_internal_path(&failed).await?;
    installers::hytale::prepare_runtime_update(&game, &provider_staging, &candidate)
        .await
        .map_err(installer_failure)?;
    let mut state = HytaleUpdateState::new(
        HytaleUpdatePhase::Prepared,
        previous_version,
        previous_build,
    );
    write_hytale_update_state(root, &state).await?;

    tokio::fs::rename(&game, &rollback).await.map_err(|error| {
        OperationFailure::with_internal(
            "hytale_update_switch_failed",
            "servers.update_failed",
            error,
        )
    })?;
    if let Err(error) = tokio::fs::rename(&candidate, &game).await {
        let _ = tokio::fs::rename(&rollback, &game).await;
        let _ = remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await;
        return Err(OperationFailure::with_internal(
            "hytale_update_switch_failed",
            "servers.update_failed",
            error,
        ));
    }
    if let Err(error) = installers::hytale::validate_game_layout(&game).await {
        let _ = tokio::fs::rename(&game, &failed).await;
        let _ = tokio::fs::rename(&rollback, &game).await;
        let _ = remove_hytale_internal_path(&failed).await;
        let _ = remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await;
        return Err(installer_failure(error));
    }
    state.phase = HytaleUpdatePhase::Applied;
    write_hytale_update_state(root, &state).await
}

async fn rollback_hytale_update(root: &Path) -> Result<HytaleUpdateState, OperationFailure> {
    let Some(mut state) = read_hytale_update_state(root).await? else {
        return Err(OperationFailure::new(
            "hytale_update_state_missing",
            "servers.update_rollback_failed",
        ));
    };
    if state.phase != HytaleUpdatePhase::Applied {
        return Err(OperationFailure::new(
            "hytale_update_state_invalid",
            "servers.update_rollback_failed",
        ));
    }
    let game = root.join("game");
    let rollback = root.join(HYTALE_UPDATE_ROLLBACK);
    let failed = root.join(HYTALE_UPDATE_FAILED);
    installers::hytale::preserve_runtime_data(&game, &rollback)
        .await
        .map_err(installer_failure)?;
    state.phase = HytaleUpdatePhase::RollingBack;
    write_hytale_update_state(root, &state).await?;
    remove_hytale_internal_path(&failed).await?;
    tokio::fs::rename(&game, &failed).await.map_err(|error| {
        OperationFailure::with_internal(
            "hytale_update_rollback_failed",
            "servers.update_rollback_failed",
            error,
        )
    })?;
    if let Err(error) = tokio::fs::rename(&rollback, &game).await {
        let _ = tokio::fs::rename(&failed, &game).await;
        return Err(OperationFailure::with_internal(
            "hytale_update_rollback_failed",
            "servers.update_rollback_failed",
            error,
        ));
    }
    installers::hytale::validate_game_layout(&game)
        .await
        .map_err(installer_failure)?;
    remove_hytale_internal_path(&failed).await?;
    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_CANDIDATE)).await?;
    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await?;
    Ok(state)
}

/// Repairs an interrupted rename protocol and returns whether the newly
/// applied version is still inside its stability/rollback window.
async fn recover_hytale_update_state(root: &Path) -> Result<bool, OperationFailure> {
    let Some(mut state) = read_hytale_update_state(root).await? else {
        return Ok(false);
    };
    let game = root.join("game");
    let candidate = root.join(HYTALE_UPDATE_CANDIDATE);
    let rollback = root.join(HYTALE_UPDATE_ROLLBACK);
    let failed = root.join(HYTALE_UPDATE_FAILED);
    match state.phase {
        HytaleUpdatePhase::Prepared => {
            let game_exists = operation_path_exists(&game).await?;
            let candidate_exists = operation_path_exists(&candidate).await?;
            let rollback_exists = operation_path_exists(&rollback).await?;
            match (game_exists, candidate_exists, rollback_exists) {
                (true, true, false) => {
                    tokio::fs::rename(&game, &rollback)
                        .await
                        .map_err(OperationFailure::internal)?;
                    if let Err(error) = tokio::fs::rename(&candidate, &game).await {
                        let _ = tokio::fs::rename(&rollback, &game).await;
                        return Err(OperationFailure::internal(error));
                    }
                }
                (false, true, true) => {
                    tokio::fs::rename(&candidate, &game)
                        .await
                        .map_err(OperationFailure::internal)?;
                }
                (true, false, true) => {}
                _ => {
                    return Err(OperationFailure::new(
                        "hytale_update_state_invalid",
                        "servers.update_failed",
                    ));
                }
            }
            installers::hytale::validate_game_layout(&game)
                .await
                .map_err(installer_failure)?;
            state.phase = HytaleUpdatePhase::Applied;
            write_hytale_update_state(root, &state).await?;
            Ok(true)
        }
        HytaleUpdatePhase::Applied => {
            let game_exists = operation_path_exists(&game).await?;
            let rollback_exists = operation_path_exists(&rollback).await?;
            match (game_exists, rollback_exists) {
                (true, true) => {
                    installers::hytale::validate_game_layout(&game)
                        .await
                        .map_err(installer_failure)?;
                    Ok(true)
                }
                (false, true) => {
                    tokio::fs::rename(&rollback, &game)
                        .await
                        .map_err(OperationFailure::internal)?;
                    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await?;
                    Ok(false)
                }
                (true, false) => {
                    installers::hytale::validate_game_layout(&game)
                        .await
                        .map_err(installer_failure)?;
                    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await?;
                    Ok(false)
                }
                (false, false) => Err(OperationFailure::new(
                    "hytale_update_state_invalid",
                    "servers.update_failed",
                )),
            }
        }
        HytaleUpdatePhase::RollingBack => {
            let game_exists = operation_path_exists(&game).await?;
            let rollback_exists = operation_path_exists(&rollback).await?;
            let failed_exists = operation_path_exists(&failed).await?;
            match (game_exists, rollback_exists, failed_exists) {
                (true, true, false) => {
                    tokio::fs::rename(&game, &failed)
                        .await
                        .map_err(OperationFailure::internal)?;
                    if let Err(error) = tokio::fs::rename(&rollback, &game).await {
                        let _ = tokio::fs::rename(&failed, &game).await;
                        return Err(OperationFailure::internal(error));
                    }
                }
                (false, true, true) => {
                    tokio::fs::rename(&rollback, &game)
                        .await
                        .map_err(OperationFailure::internal)?;
                }
                (true, false, true) => {}
                _ => {
                    return Err(OperationFailure::new(
                        "hytale_update_state_invalid",
                        "servers.update_rollback_failed",
                    ));
                }
            }
            installers::hytale::validate_game_layout(&game)
                .await
                .map_err(installer_failure)?;
            remove_hytale_internal_path(&failed).await?;
            remove_hytale_internal_path(&candidate).await?;
            remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await?;
            Ok(false)
        }
    }
}

async fn operation_path_exists(path: &Path) -> Result<bool, OperationFailure> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if runtime_metadata_is_link_like(&metadata) => Err(OperationFailure::new(
            "hytale_internal_path_unsafe",
            "servers.update_failed",
        )),
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => Ok(true),
        Ok(_) => Err(OperationFailure::new(
            "hytale_internal_path_unsafe",
            "servers.update_failed",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(OperationFailure::internal(error)),
    }
}

async fn remove_hytale_internal_path(path: &Path) -> Result<(), OperationFailure> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if runtime_metadata_is_link_like(&metadata) => Err(OperationFailure::new(
            "hytale_internal_path_unsafe",
            "servers.update_failed",
        )),
        Ok(metadata) if metadata.is_file() => tokio::fs::remove_file(path)
            .await
            .map_err(OperationFailure::internal),
        Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir_all(path)
            .await
            .map_err(OperationFailure::internal),
        Ok(_) => Err(OperationFailure::new(
            "hytale_internal_path_unsafe",
            "servers.update_failed",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(OperationFailure::internal(error)),
    }
}

async fn finalize_hytale_update(root: &Path) -> Result<bool, OperationFailure> {
    let Some(state) = read_hytale_update_state(root).await? else {
        return Ok(false);
    };
    if state.phase != HytaleUpdatePhase::Applied {
        return Ok(false);
    }
    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_ROLLBACK)).await?;
    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_CANDIDATE)).await?;
    remove_hytale_internal_path(&root.join(HYTALE_UPDATE_STATE_FILE)).await?;
    Ok(true)
}

async fn read_hytale_update_state(
    root: &Path,
) -> Result<Option<HytaleUpdateState>, OperationFailure> {
    let path = root.join(HYTALE_UPDATE_STATE_FILE);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(OperationFailure::internal(error)),
    };
    if !metadata.is_file() || runtime_metadata_is_link_like(&metadata) || metadata.len() > 4096 {
        return Err(OperationFailure::new(
            "hytale_update_state_invalid",
            "servers.update_failed",
        ));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(OperationFailure::internal)?;
    let state: HytaleUpdateState =
        serde_json::from_slice(&bytes).map_err(OperationFailure::internal)?;
    if state.schema != 1
        || state.created_at.len() > 64
        || state
            .previous_version
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        || state
            .previous_build
            .as_ref()
            .is_some_and(|value| value.len() > 256)
    {
        return Err(OperationFailure::new(
            "hytale_update_state_invalid",
            "servers.update_failed",
        ));
    }
    Ok(Some(state))
}

async fn write_hytale_update_state(
    root: &Path,
    state: &HytaleUpdateState,
) -> Result<(), OperationFailure> {
    let destination = root.join(HYTALE_UPDATE_STATE_FILE);
    let temporary = root.join(HYTALE_UPDATE_STATE_TEMP_FILE);
    remove_hytale_internal_path(&temporary).await?;
    let bytes = serde_json::to_vec(state).map_err(OperationFailure::internal)?;
    let temporary_for_write = temporary.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write as _;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let mut file = options.open(&temporary_for_write)?;
        file.write_all(&bytes)?;
        file.sync_all()
    })
    .await
    .map_err(OperationFailure::internal)?
    .map_err(OperationFailure::internal)?;
    replace_runtime_file(&temporary, &destination).await
}

async fn replace_runtime_file(
    temporary: &Path,
    destination: &Path,
) -> Result<(), OperationFailure> {
    #[cfg(not(windows))]
    {
        tokio::fs::rename(temporary, destination)
            .await
            .map_err(OperationFailure::internal)
    }
    #[cfg(windows)]
    {
        let rollback = destination.with_extension("previous");
        remove_dir_if_exists(&rollback)
            .await
            .map_err(OperationFailure::internal)?;
        let existed = tokio::fs::try_exists(destination)
            .await
            .map_err(OperationFailure::internal)?;
        if existed {
            tokio::fs::rename(destination, &rollback)
                .await
                .map_err(OperationFailure::internal)?;
        }
        if let Err(error) = tokio::fs::rename(temporary, destination).await {
            if existed {
                let _ = tokio::fs::rename(&rollback, destination).await;
            }
            return Err(OperationFailure::internal(error));
        }
        if existed {
            remove_dir_if_exists(&rollback)
                .await
                .map_err(OperationFailure::internal)?;
        }
        Ok(())
    }
}

async fn steam_profile_for_instance(
    pool: &DbPool,
    instance: &RuntimeInstance,
) -> Result<Option<SteamProfile>, OperationFailure> {
    let revision = u32::try_from(instance.profile_revision).map_err(|_| {
        OperationFailure::new("profile_revision_invalid", "servers.profile_invalid")
    })?;
    profiles::load_steam_profile_revision(pool, &instance.profile_id, revision)
        .await
        .map_err(OperationFailure::internal)
}

async fn readiness_pattern(
    pool: &DbPool,
    instance: &RuntimeInstance,
) -> Result<Option<Regex>, OperationFailure> {
    let pattern: Option<String> = sqlx::query_scalar(
        "SELECT json_extract(manifest, '$.lifecycle.ready_log_pattern') \
         FROM game_profiles WHERE id = ? AND revision = ?",
    )
    .bind(&instance.profile_id)
    .bind(instance.profile_revision)
    .fetch_one(pool)
    .await
    .map_err(OperationFailure::internal)?;
    pattern
        .map(|pattern| {
            Regex::new(&pattern).map_err(|error| {
                OperationFailure::with_internal(
                    "readiness_pattern_invalid",
                    "servers.invalid_launch_spec",
                    error,
                )
            })
        })
        .transpose()
}

async fn wait_until_ready(
    pattern: Regex,
    lines: &mut mpsc::Receiver<String>,
    mut exit: watch::Receiver<Option<ExitOutcome>>,
    timeout_duration: Duration,
) -> Result<(), OperationFailure> {
    let timeout = tokio::time::sleep(timeout_duration);
    tokio::pin!(timeout);
    let mut lines_open = true;
    loop {
        tokio::select! {
            line = lines.recv(), if lines_open => {
                match line {
                    Some(line) if pattern.is_match(&line) => return Ok(()),
                    Some(_) => {}
                    None => lines_open = false,
                }
            }
            changed = exit.changed() => {
                let outcome = exit.borrow().clone();
                let detail = outcome.map(|outcome| {
                    format!("process exited before readiness (code={:?}, success={})", outcome.code, outcome.success)
                }).unwrap_or_else(|| {
                    if changed.is_err() {
                        "process monitor closed before readiness".to_string()
                    } else {
                        "process state changed without an exit outcome".to_string()
                    }
                });
                return Err(OperationFailure::with_internal(
                    "server_exited_before_ready",
                    "servers.start_failed",
                    detail,
                ));
            }
            () = &mut timeout => {
                return Err(OperationFailure::new(
                    "server_readiness_timeout",
                    "servers.start_failed",
                ));
            }
        }
    }
}

async fn wait_for_process_stability(
    mut exit: watch::Receiver<Option<ExitOutcome>>,
    stability_window: Duration,
) -> Result<(), OperationFailure> {
    if let Some(outcome) = exit.borrow().clone() {
        return Err(OperationFailure::with_internal(
            "server_exited_before_ready",
            "servers.start_failed",
            format!(
                "process exited during stability window (code={:?}, success={})",
                outcome.code, outcome.success
            ),
        ));
    }
    tokio::select! {
        () = tokio::time::sleep(stability_window) => Ok(()),
        changed = exit.changed() => {
            let detail = exit.borrow().clone().map(|outcome| {
                format!(
                    "process exited during stability window (code={:?}, success={})",
                    outcome.code, outcome.success
                )
            }).unwrap_or_else(|| {
                if changed.is_err() {
                    "process monitor closed during stability window".to_string()
                } else {
                    "process state changed without an exit outcome".to_string()
                }
            });
            Err(OperationFailure::with_internal(
                "server_exited_before_ready",
                "servers.start_failed",
                detail,
            ))
        }
    }
}

fn steam_install_target(
    instance: &RuntimeInstance,
    steam_profile: Option<&SteamProfile>,
) -> Result<(u32, Option<String>), OperationFailure> {
    match instance.profile_id.as_str() {
        "valheim" => Ok((896_660, None)),
        "palworld" => Ok((2_394_010, None)),
        "satisfactory" => Ok((1_690_800, None)),
        "seven-days-to-die" => Ok((294_420, None)),
        "project-zomboid" => Ok((380_870, None)),
        "rust" => Ok((258_550, None)),
        _ => steam_profile
            .map(|profile| (profile.app_id, profile.branch.clone()))
            .ok_or_else(|| {
                OperationFailure::new(
                    "installer_not_implemented",
                    "servers.installer_not_implemented",
                )
            }),
    }
}

async fn resolve_steam_available_build(
    steamcmd_path: &Path,
    app_id: u32,
    branch: &str,
) -> Result<String, OperationFailure> {
    let mut command = Command::new(steamcmd_path);
    command
        .arg("+login")
        .arg("anonymous")
        .arg("+app_info_update")
        .arg("1")
        .arg("+app_info_print")
        .arg(app_id.to_string())
        .arg("+quit")
        .env_clear()
        .envs(filtered_tool_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_contained_capture(&mut command, GAME_UPDATE_PROCESS_TIMEOUT).await?;
    parse_steam_branch_build(&output, branch).ok_or_else(|| {
        OperationFailure::new(
            "steam_update_metadata_invalid",
            "servers.provider_response_invalid",
        )
    })
}

fn parse_steam_branch_build(output: &str, branch: &str) -> Option<String> {
    if branch.is_empty()
        || branch.len() > 64
        || !branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    let branches = vdf_object(output, "branches")?;
    let branch = vdf_object(branches, branch)?;
    Regex::new(r#"(?m)^\s*"buildid"\s+"([0-9]{1,20})"\s*$"#)
        .ok()?
        .captures(branch)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn vdf_object<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let key = format!("\"{key}\"");
    let key_start = input.find(&key)?;
    let object_start = input[key_start + key.len()..].find('{')? + key_start + key.len();
    let bytes = input.as_bytes();
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(object_start) {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if byte == b'{' {
            depth = depth.saturating_add(1);
        } else if byte == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return input.get(object_start + 1..index);
            }
        }
    }
    None
}

async fn run_contained_capture(
    command: &mut Command,
    timeout: Duration,
) -> Result<String, OperationFailure> {
    let spawned = spawn_contained(command).map_err(|error| match error {
        ContainedSpawnError::Spawn(error) => OperationFailure::with_internal(
            "update_check_start_failed",
            "servers.update_check_unavailable",
            error,
        ),
        ContainedSpawnError::Containment(error) => OperationFailure::with_internal(
            "process_containment_failed",
            "servers.process_containment_failed",
            error,
        ),
    })?;
    let mut child = spawned.child;
    #[cfg(windows)]
    let windows_job = spawned.windows_job;
    let pid = child.id().ok_or_else(|| {
        OperationFailure::new(
            "update_check_start_failed",
            "servers.update_check_unavailable",
        )
    })?;
    #[cfg(windows)]
    let job_handle = Some(windows_job.handle);
    #[cfg(not(windows))]
    let job_handle = None;
    let stdout = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(read_bounded_process_output(stdout)));
    let stderr = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(read_bounded_process_output(stderr)));
    let status = tokio::select! {
        status = child.wait() => status.map_err(OperationFailure::internal)?,
        () = tokio::time::sleep(timeout) => {
            let _ = terminate_installer(&mut child, pid, job_handle).await;
            return Err(OperationFailure::new(
                "update_check_timeout",
                "servers.update_check_unavailable",
            ));
        }
    };
    let stdout = match stdout {
        Some(task) => task
            .await
            .map_err(OperationFailure::internal)?
            .map_err(OperationFailure::internal)?,
        None => Vec::new(),
    };
    let stderr = match stderr {
        Some(task) => task
            .await
            .map_err(OperationFailure::internal)?
            .map_err(OperationFailure::internal)?,
        None => Vec::new(),
    };
    if !status.success() {
        return Err(OperationFailure::with_internal(
            "update_check_failed",
            "servers.update_check_unavailable",
            format!("provider checker exited with {:?}", status.code()),
        ));
    }
    let mut output = String::from_utf8_lossy(&stdout).into_owned();
    if !stderr.is_empty() {
        output.push('\n');
        output.push_str(&String::from_utf8_lossy(&stderr));
    }
    Ok(output)
}

async fn read_bounded_process_output<R>(mut reader: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_GAME_UPDATE_OUTPUT_BYTES.saturating_sub(captured.len());
        if remaining > 0 {
            captured.extend_from_slice(&chunk[..read.min(remaining)]);
        }
    }
    Ok(captured)
}

async fn read_steam_build_id(
    staging: &Path,
    steamcmd_directory: Option<&Path>,
    app_id: u32,
) -> Result<Option<String>, OperationFailure> {
    let manifest = format!("appmanifest_{app_id}.acf");
    let mut candidates = vec![
        staging.join("steamapps").join(&manifest),
        staging.join(&manifest),
    ];
    if let Some(directory) = steamcmd_directory {
        candidates.push(directory.join("steamapps").join(&manifest));
    }
    let pattern = Regex::new(r#"(?m)^\s*"buildid"\s+"([0-9]{1,20})"\s*$"#)
        .map_err(OperationFailure::internal)?;
    for path in candidates {
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(OperationFailure::internal(error)),
        };
        if !metadata.is_file()
            || runtime_metadata_is_link_like(&metadata)
            || metadata.len() > 1024 * 1024
        {
            return Err(OperationFailure::new(
                "steam_manifest_invalid",
                "servers.install_metadata_invalid",
            ));
        }
        let contents = tokio::fs::read_to_string(path)
            .await
            .map_err(OperationFailure::internal)?;
        return Ok(pattern
            .captures(&contents)
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_string()));
    }
    Ok(None)
}

async fn preserve_instance_data(
    instance: &RuntimeInstance,
    steam_profile: Option<&SteamProfile>,
    current_game: &Path,
    staging: &Path,
) -> Result<(), OperationFailure> {
    if !tokio::fs::try_exists(current_game)
        .await
        .map_err(OperationFailure::internal)?
    {
        return Ok(());
    }
    let save_paths = match instance.profile_id.as_str() {
        "palworld" => vec!["Pal/Saved".to_string()],
        "satisfactory" => vec!["FactoryGame/Saved".to_string()],
        "seven-days-to-die" => vec!["dmx-serverconfig.xml".to_string(), "Mods".to_string()],
        "rust" => vec!["server".to_string()],
        _ => steam_profile
            .map(|profile| profile.save_paths.clone())
            .unwrap_or_default(),
    };
    for relative in save_paths {
        ensure_no_symlink_components(current_game, &relative).await?;
        ensure_no_symlink_components(staging, &relative).await?;
        let source = crate::domain::v1::safe_join(current_game, &relative)
            .map_err(|_| OperationFailure::new("invalid_save_path", "servers.invalid_save_path"))?;
        if !tokio::fs::try_exists(&source)
            .await
            .map_err(OperationFailure::internal)?
        {
            continue;
        }
        let destination = crate::domain::v1::safe_join(staging, &relative)
            .map_err(|_| OperationFailure::new("invalid_save_path", "servers.invalid_save_path"))?;
        remove_dir_if_exists(&destination)
            .await
            .map_err(OperationFailure::internal)?;
        copy_tree_without_links(&source, &destination).await?;
    }
    Ok(())
}

async fn ensure_no_symlink_components(root: &Path, relative: &str) -> Result<(), OperationFailure> {
    let root_metadata = tokio::fs::symlink_metadata(root)
        .await
        .map_err(OperationFailure::internal)?;
    if runtime_metadata_is_link_like(&root_metadata) {
        return Err(OperationFailure::new(
            "unsafe_save_path",
            "servers.unsafe_save_path",
        ));
    }
    let relative = crate::domain::v1::safe_join(Path::new(""), relative)
        .map_err(|_| OperationFailure::new("invalid_save_path", "servers.invalid_save_path"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) if runtime_metadata_is_link_like(&metadata) => {
                return Err(OperationFailure::new(
                    "unsafe_save_path",
                    "servers.unsafe_save_path",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(OperationFailure::internal(error)),
        }
    }
    Ok(())
}

fn runtime_metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

async fn copy_tree_without_links(
    source: &Path,
    destination: &Path,
) -> Result<(), OperationFailure> {
    let metadata = tokio::fs::symlink_metadata(source)
        .await
        .map_err(OperationFailure::internal)?;
    if runtime_metadata_is_link_like(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(OperationFailure::new(
            "unsafe_save_path",
            "servers.unsafe_save_path",
        ));
    }
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(OperationFailure::internal)?;
        }
        tokio::fs::copy(source, destination)
            .await
            .map_err(OperationFailure::internal)?;
        return Ok(());
    }

    tokio::fs::create_dir_all(destination)
        .await
        .map_err(OperationFailure::internal)?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_dir, destination_dir)) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&source_dir)
            .await
            .map_err(OperationFailure::internal)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(OperationFailure::internal)?
        {
            let metadata = tokio::fs::symlink_metadata(entry.path())
                .await
                .map_err(OperationFailure::internal)?;
            if runtime_metadata_is_link_like(&metadata)
                || (!metadata.is_dir() && !metadata.is_file())
            {
                return Err(OperationFailure::new(
                    "unsafe_save_path",
                    "servers.unsafe_save_path",
                ));
            }
            let next_destination = destination_dir.join(entry.file_name());
            if metadata.is_dir() {
                tokio::fs::create_dir(&next_destination)
                    .await
                    .map_err(OperationFailure::internal)?;
                pending.push((entry.path(), next_destination));
            } else {
                tokio::fs::copy(entry.path(), next_destination)
                    .await
                    .map_err(OperationFailure::internal)?;
            }
        }
    }
    Ok(())
}

async fn validate_installed_files(
    instance: &RuntimeInstance,
    steam_profile: Option<&SteamProfile>,
    staging: &Path,
) -> Result<(), OperationFailure> {
    let relative = match instance.profile_id.as_str() {
        "valheim" => platform_executable("valheim_server.x86_64", "valheim_server.exe")?,
        "palworld" => platform_executable("PalServer.sh", "PalServer.exe")?,
        "satisfactory" => platform_executable("FactoryServer.sh", "FactoryServer.exe")?,
        "seven-days-to-die" => {
            platform_executable("7DaysToDieServer.x86_64", "7DaysToDieServer.exe")?
        }
        "project-zomboid" => platform_executable("start-server.sh", "ProjectZomboid64.exe")?,
        "rust" => platform_executable("RustDedicated", "RustDedicated.exe")?,
        _ => steam_profile
            .map(platform_steam_executable)
            .transpose()?
            .ok_or_else(|| {
                OperationFailure::new(
                    "installer_not_implemented",
                    "servers.installer_not_implemented",
                )
            })?,
    };
    let executable = crate::domain::v1::safe_join(staging, &relative)
        .map_err(|_| OperationFailure::new("invalid_launch_spec", "servers.invalid_launch_spec"))?;
    let metadata = tokio::fs::symlink_metadata(&executable)
        .await
        .map_err(|error| {
            OperationFailure::with_internal(
                "installed_executable_missing",
                "servers.installed_executable_missing",
                error,
            )
        })?;
    if !metadata.file_type().is_file() {
        return Err(OperationFailure::new(
            "installed_executable_invalid",
            "servers.installed_executable_invalid",
        ));
    }
    Ok(())
}

async fn validate_launch_paths(root: &Path, spec: &mut LaunchSpec) -> Result<(), OperationFailure> {
    let game = tokio::fs::canonicalize(root.join("game"))
        .await
        .map_err(OperationFailure::internal)?;
    let executable = tokio::fs::canonicalize(&spec.executable)
        .await
        .map_err(|error| {
            OperationFailure::with_internal(
                "installed_executable_missing",
                "servers.installed_executable_missing",
                error,
            )
        })?;
    let cwd = tokio::fs::canonicalize(&spec.cwd)
        .await
        .map_err(OperationFailure::internal)?;
    if !executable.starts_with(&game) || !cwd.starts_with(&game) {
        return Err(OperationFailure::new(
            "launch_path_escape",
            "servers.invalid_launch_spec",
        ));
    }
    spec.executable = executable;
    spec.cwd = cwd;
    Ok(())
}

fn filtered_environment(root: &Path, profile_id: &str) -> Vec<(OsString, OsString)> {
    let mut environment = Vec::new();
    for name in ["PATH", "HOME", "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL"] {
        if profile_id == "project-zomboid" && name == "HOME" {
            continue;
        }
        if let Some(value) = std::env::var_os(name) {
            environment.push((OsString::from(name), value));
        }
    }
    if profile_id == "project-zomboid" {
        environment.push((OsString::from("HOME"), root.join("data").into_os_string()));
    }
    if profile_id == "valheim" {
        environment.push((OsString::from("SteamAppId"), OsString::from("892970")));
        let linux64 = root.join("game/linux64");
        let mut library_path = linux64.into_os_string();
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            library_path.push(":");
            library_path.push(existing);
        }
        environment.push((OsString::from("LD_LIBRARY_PATH"), library_path));
    }
    environment
}

fn filtered_tool_environment() -> Vec<(OsString, OsString)> {
    [
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ]
    .into_iter()
    .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(name), value)))
    .collect()
}

async fn managed_java_executable(
    settings: &Settings,
    major: u16,
) -> Result<PathBuf, OperationFailure> {
    let variable = match major {
        8 => "DMX_JAVA_8_PATH",
        16 => "DMX_JAVA_16_PATH",
        17 => "DMX_JAVA_17_PATH",
        21 => "DMX_JAVA_21_PATH",
        25 => "DMX_JAVA_25_PATH",
        _ => {
            return Err(OperationFailure::new(
                "java_version_unsupported",
                "servers.java_runtime_unavailable",
            ));
        }
    };
    match std::env::var_os(variable) {
        Some(value) => {
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                return Err(OperationFailure::new(
                    "java_path_invalid",
                    "servers.java_runtime_unavailable",
                ));
            }
            Ok(path)
        }
        None => installers::managed_java_path(&settings.data_dir.join("toolchains/java"), major)
            .await
            .map_err(installer_failure),
    }
}

async fn validate_java_runtime(
    executable: &Path,
    expected_major: u16,
) -> Result<(), OperationFailure> {
    let mut command = Command::new(executable);
    command
        .arg("-version")
        .env_clear()
        .envs(filtered_tool_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| {
            OperationFailure::new("java_probe_timeout", "servers.java_runtime_unavailable")
        })?
        .map_err(|error| {
            OperationFailure::with_internal(
                "java_probe_failed",
                "servers.java_runtime_unavailable",
                error,
            )
        })?;
    if !output.status.success() {
        return Err(OperationFailure::new(
            "java_probe_failed",
            "servers.java_runtime_unavailable",
        ));
    }
    let version_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let detected = parse_java_major(&version_text).ok_or_else(|| {
        OperationFailure::new("java_version_unknown", "servers.java_runtime_unavailable")
    })?;
    if detected != expected_major {
        return Err(OperationFailure::new(
            "java_version_mismatch",
            "servers.java_runtime_version_mismatch",
        ));
    }
    Ok(())
}

fn parse_java_major(value: &str) -> Option<u16> {
    let version = Regex::new(r#"(?i)(?:java|openjdk)(?:\s+version)?\s+"?(?:1\.)?(\d+)"#)
        .ok()?
        .captures(value)?
        .get(1)?
        .as_str()
        .parse()
        .ok()?;
    Some(version)
}

async fn preflight_ports(pool: &DbPool, instance_id: &str) -> Result<(), OperationFailure> {
    let ports: Vec<(String, i64)> = sqlx::query_as(
        "SELECT protocol, port FROM port_reservations WHERE instance_id = ? ORDER BY port",
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await
    .map_err(OperationFailure::internal)?;
    for (protocol, port) in ports {
        let port = u16::try_from(port).map_err(OperationFailure::internal)?;
        let address = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
        let available = match protocol.as_str() {
            "tcp" => std::net::TcpListener::bind(address).map(drop),
            "udp" => std::net::UdpSocket::bind(address).map(drop),
            _ => {
                return Err(OperationFailure::new(
                    "invalid_port_protocol",
                    "servers.invalid_port_protocol",
                ));
            }
        };
        if let Err(error) = available {
            return Err(OperationFailure::with_internal(
                "port_unavailable",
                "servers.port_unavailable",
                error,
            ));
        }
    }
    Ok(())
}

async fn cleanup_orphaned_hytale_sessions(settings: &Settings) -> Result<(), AppError> {
    let mut instances = match tokio::fs::read_dir(settings.instances_dir()).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    while let Some(instance) = instances.next_entry().await? {
        let metadata = tokio::fs::symlink_metadata(instance.path()).await?;
        if !metadata.is_dir() || runtime_metadata_is_link_like(&metadata) {
            continue;
        }
        let mut sessions = match tokio::fs::read_dir(instance.path().join(".staging/hytale")).await
        {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(session) = sessions.next_entry().await? {
            if session
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".session-"))
            {
                remove_dir_if_exists(&session.path()).await?;
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_console_command(command: &str) -> Result<(), AppError> {
    if command.is_empty()
        || command.len() > MAX_CONSOLE_COMMAND
        || command.contains(['\0', '\r', '\n'])
    {
        Err(AppError::BadRequest(
            "servers.invalid_console_command".into(),
        ))
    } else {
        Ok(())
    }
}

fn required_string(settings: &Value, name: &str) -> Result<String, OperationFailure> {
    settings
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains(['\0', '\r', '\n']))
        .map(str::to_string)
        .ok_or_else(|| OperationFailure::new("invalid_setting", "servers.invalid_settings"))
}

fn integer_setting(settings: &Value, name: &str, default: u16) -> Result<u16, OperationFailure> {
    settings
        .get(name)
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(default))
        .try_into()
        .map_err(|_| OperationFailure::new("invalid_setting", "servers.invalid_settings"))
}

fn platform_executable(linux: &str, windows: &str) -> Result<String, OperationFailure> {
    if cfg!(target_os = "linux") {
        Ok(linux.to_string())
    } else if cfg!(windows) {
        Ok(windows.to_string())
    } else {
        Err(OperationFailure::new(
            "platform_not_supported",
            "servers.platform_not_supported",
        ))
    }
}

fn platform_steam_executable(profile: &SteamProfile) -> Result<String, OperationFailure> {
    let executable = if cfg!(target_os = "linux") {
        profile.executable.linux_x86_64.as_deref()
    } else if cfg!(windows) {
        profile.executable.windows_x86_64.as_deref()
    } else {
        return Err(OperationFailure::new(
            "platform_not_supported",
            "servers.platform_not_supported",
        ));
    };
    executable
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            OperationFailure::new(
                "platform_executable_missing",
                "servers.platform_executable_missing",
            )
        })
}

async fn build_custom_steam_launch_spec(
    instance: &RuntimeInstance,
    root: &Path,
    settings: &Value,
    profile: &SteamProfile,
) -> Result<PreparedLaunch, OperationFailure> {
    let executable = format!("game/{}", platform_steam_executable(profile)?);
    let args = profile
        .arguments
        .iter()
        .map(|argument| expand_steam_argument(argument, settings, profile, root))
        .collect::<Result<Vec<_>, _>>()?;
    let stop = match &profile.stop_strategy {
        SteamStopStrategy::Stdin {
            command,
            timeout_seconds,
        } => StopStrategy::Stdin {
            command: command.clone(),
            timeout_seconds: *timeout_seconds,
        },
        SteamStopStrategy::Interrupt { timeout_seconds } => StopStrategy::Interrupt {
            timeout_seconds: *timeout_seconds,
        },
        SteamStopStrategy::Terminate { timeout_seconds } => StopStrategy::Terminate {
            timeout_seconds: *timeout_seconds,
        },
    };
    let mut spec = LaunchSpec::for_instance(root, &executable, "game", args)
        .map_err(|_| OperationFailure::new("invalid_launch_spec", "servers.invalid_launch_spec"))?;
    validate_launch_paths(root, &mut spec).await?;
    spec.env = filtered_environment(root, &instance.profile_id);
    Ok(PreparedLaunch {
        spec,
        stop,
        redactions: Vec::new(),
    })
}

fn expand_steam_argument(
    argument: &str,
    settings: &Value,
    profile: &SteamProfile,
    root: &Path,
) -> Result<String, OperationFailure> {
    if argument.contains('\0') || argument.len() > 8_192 {
        return Err(OperationFailure::new(
            "invalid_launch_spec",
            "servers.invalid_launch_spec",
        ));
    }
    if argument == "{{instance_dir}}" {
        return Ok(root.to_string_lossy().into_owned());
    }
    if let Some(name) = argument
        .strip_prefix("{{port:")
        .and_then(|value| value.strip_suffix("}}"))
    {
        let declaration = profile
            .ports
            .iter()
            .find(|port| port.name == name)
            .ok_or_else(|| {
                OperationFailure::new("invalid_placeholder", "servers.invalid_launch_spec")
            })?;
        let port = settings
            .get(name)
            .and_then(Value::as_u64)
            .unwrap_or(u64::from(declaration.default));
        let port = u16::try_from(port)
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| {
                OperationFailure::new("invalid_placeholder", "servers.invalid_launch_spec")
            })?;
        return Ok(port.to_string());
    }
    if argument.contains("{{") || argument.contains("}}") {
        return Err(OperationFailure::new(
            "invalid_placeholder",
            "servers.invalid_launch_spec",
        ));
    }
    Ok(argument.to_string())
}

fn ini_value(value: &str) -> Result<String, OperationFailure> {
    if value.contains(['\0', '\r', '\n']) {
        return Err(OperationFailure::new(
            "invalid_ini_value",
            "servers.invalid_settings",
        ));
    }
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn merge_palworld_settings(
    existing: &str,
    server_name: &str,
    server_password: &str,
    admin_password: &str,
    port: u16,
) -> Result<String, OperationFailure> {
    const SECTION: &str = "[/Script/Pal.PalGameWorldSettings]";
    const OPTION_PREFIX: &str = "OptionSettings=(";
    let replacements = [
        ("ServerName", format!("\"{server_name}\"")),
        ("ServerPassword", format!("\"{server_password}\"")),
        ("AdminPassword", format!("\"{admin_password}\"")),
        ("PublicPort", port.to_string()),
    ];
    let option_count = existing
        .lines()
        .filter(|line| line.trim().starts_with(OPTION_PREFIX))
        .count();
    if option_count > 1 {
        return Err(OperationFailure::new(
            "palworld_configuration_invalid",
            "servers.settings_invalid",
        ));
    }
    if option_count == 1 {
        let mut merged = String::with_capacity(existing.len() + 128);
        for line in existing.split_inclusive('\n') {
            if line.trim().starts_with(OPTION_PREFIX) {
                merged.push_str(&merge_palworld_option_line(line, &replacements)?);
            } else {
                merged.push_str(line);
            }
        }
        return Ok(merged);
    }

    let option_line = format_palworld_option_line(&replacements);
    if existing.trim().is_empty() {
        return Ok(format!("{SECTION}\n{option_line}\n"));
    }
    let mut offset = 0;
    for line in existing.split_inclusive('\n') {
        offset += line.len();
        if line.trim() == SECTION {
            let mut merged = existing.to_string();
            merged.insert_str(offset, &format!("{option_line}\n"));
            return Ok(merged);
        }
    }
    let mut merged = existing.to_string();
    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(SECTION);
    merged.push('\n');
    merged.push_str(&option_line);
    merged.push('\n');
    Ok(merged)
}

fn merge_palworld_option_line(
    line: &str,
    replacements: &[(&str, String)],
) -> Result<String, OperationFailure> {
    const OPTION_PREFIX: &str = "OptionSettings=(";
    let (line, newline) = if let Some(line) = line.strip_suffix("\r\n") {
        (line, "\r\n")
    } else if let Some(line) = line.strip_suffix('\n') {
        (line, "\n")
    } else {
        (line, "")
    };
    let trimmed = line.trim();
    let body = trimmed
        .strip_prefix(OPTION_PREFIX)
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| {
            OperationFailure::new("palworld_configuration_invalid", "servers.settings_invalid")
        })?;
    let indent = &line[..line.find(OPTION_PREFIX).ok_or_else(|| {
        OperationFailure::new("palworld_configuration_invalid", "servers.settings_invalid")
    })?];
    let mut fields = Vec::new();
    let mut replaced = vec![false; replacements.len()];
    for raw in split_palworld_option_fields(body)? {
        let (key, _) = raw.split_once('=').ok_or_else(|| {
            OperationFailure::new("palworld_configuration_invalid", "servers.settings_invalid")
        })?;
        let key = key.trim();
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(OperationFailure::new(
                "palworld_configuration_invalid",
                "servers.settings_invalid",
            ));
        }
        if let Some((index, (_, value))) = replacements
            .iter()
            .enumerate()
            .find(|(_, (candidate, _))| *candidate == key)
        {
            if !replaced[index] {
                fields.push(format!("{key}={value}"));
                replaced[index] = true;
            }
        } else {
            fields.push(raw.trim().to_string());
        }
    }
    for (index, (key, value)) in replacements.iter().enumerate() {
        if !replaced[index] {
            fields.push(format!("{key}={value}"));
        }
    }
    Ok(format!(
        "{indent}OptionSettings=({}){newline}",
        fields.join(",")
    ))
}

fn split_palworld_option_fields(value: &str) -> Result<Vec<&str>, OperationFailure> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == ',' && !quoted {
            fields.push(&value[start..index]);
            start = index + 1;
        }
    }
    if quoted || escaped {
        return Err(OperationFailure::new(
            "palworld_configuration_invalid",
            "servers.settings_invalid",
        ));
    }
    if start < value.len() {
        fields.push(&value[start..]);
    }
    if fields.iter().any(|field| field.trim().is_empty()) {
        return Err(OperationFailure::new(
            "palworld_configuration_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(fields)
}

fn format_palworld_option_line(replacements: &[(&str, String)]) -> String {
    format!(
        "OptionSettings=({})",
        replacements
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn merge_ini_settings(existing: &str, updates: &[(&str, String)]) -> String {
    let mut merged = String::with_capacity(existing.len() + updates.len() * 32);
    let mut written = vec![false; updates.len()];
    for line in existing.split_inclusive('\n') {
        let key = line.trim().split_once('=').map(|(key, _)| key.trim());
        if let Some((index, (name, value))) = updates
            .iter()
            .enumerate()
            .find(|(_, (name, _))| Some(*name) == key)
        {
            if !written[index] {
                merged.push_str(name);
                merged.push('=');
                merged.push_str(value);
                merged.push('\n');
                written[index] = true;
            }
        } else {
            merged.push_str(line);
        }
    }
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    for ((name, value), already_written) in updates.iter().zip(written) {
        if !already_written {
            merged.push_str(name);
            merged.push('=');
            merged.push_str(value);
            merged.push('\n');
        }
    }
    merged
}

fn merge_seven_days_settings(
    existing: &str,
    updates: &[(&str, String)],
) -> Result<String, OperationFailure> {
    if existing.trim().is_empty() {
        let mut generated = String::from("<?xml version=\"1.0\"?>\n<ServerSettings>\n");
        append_seven_days_properties(&mut generated, updates.iter());
        generated.push_str("</ServerSettings>\n");
        return Ok(generated);
    }
    if !existing.contains("<ServerSettings") || !existing.contains("</ServerSettings>") {
        return Err(OperationFailure::new(
            "seven_days_configuration_invalid",
            "servers.settings_invalid",
        ));
    }
    let property =
        Regex::new(r#"^\s*<property\b[^>]*\bname\s*=\s*\"(?P<name>[^\"]+)\"[^>]*/>\s*$"#)
            .map_err(OperationFailure::internal)?;
    let mut merged = String::with_capacity(existing.len() + updates.len() * 64);
    let mut written = vec![false; updates.len()];
    let mut closed = false;
    for line in existing.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "</ServerSettings>" {
            if closed {
                return Err(OperationFailure::new(
                    "seven_days_configuration_invalid",
                    "servers.settings_invalid",
                ));
            }
            append_seven_days_properties(
                &mut merged,
                updates
                    .iter()
                    .zip(&written)
                    .filter_map(|(update, already_written)| (!already_written).then_some(update)),
            );
            written.fill(true);
            merged.push_str(line);
            closed = true;
            continue;
        }
        if let Some(captures) = property.captures(trimmed) {
            let name = captures.name("name").map(|value| value.as_str());
            if let Some((index, update)) = updates
                .iter()
                .enumerate()
                .find(|(_, (candidate, _))| Some(*candidate) == name)
            {
                if !written[index] {
                    append_seven_days_properties(&mut merged, std::iter::once(update));
                    written[index] = true;
                }
                continue;
            }
        }
        merged.push_str(line);
    }
    if !closed {
        return Err(OperationFailure::new(
            "seven_days_configuration_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(merged)
}

fn append_seven_days_properties<'a>(
    output: &mut String,
    values: impl Iterator<Item = &'a (&'a str, String)>,
) {
    for (name, value) in values {
        output.push_str("  <property name=\"");
        output.push_str(name);
        output.push_str("\" value=\"");
        output.push_str(&xml_attribute(value));
        output.push_str("\" />\n");
    }
}

async fn read_bounded_runtime_text(
    path: &Path,
    max_bytes: u64,
) -> Result<Option<String>, OperationFailure> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(OperationFailure::internal(error)),
    };
    if !metadata.is_file() || runtime_metadata_is_link_like(&metadata) || metadata.len() > max_bytes
    {
        return Err(OperationFailure::new(
            "configuration_path_unsafe",
            "servers.settings_invalid",
        ));
    }
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read as _;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || runtime_metadata_is_link_like(&opened) || opened.len() > max_bytes {
            return Err(std::io::Error::other("unsafe runtime configuration file"));
        }
        let mut bytes = Vec::with_capacity(opened.len() as usize);
        file.take(max_bytes + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(std::io::Error::other(
                "runtime configuration file is too large",
            ));
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| std::io::Error::other("runtime configuration file is not UTF-8"))
    })
    .await
    .map_err(OperationFailure::internal)?
    .map_err(OperationFailure::internal)
}

async fn write_private_runtime_file(path: &Path, contents: &[u8]) -> Result<(), OperationFailure> {
    let path = path.to_path_buf();
    let contents = contents.to_vec();
    tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let mut file = options.open(path)?;
        file.write_all(&contents)?;
        file.sync_all()
    })
    .await
    .map_err(OperationFailure::internal)?
    .map_err(OperationFailure::internal)
}

async fn write_runtime_configuration(
    directory: &Path,
    name: &str,
    contents: &[u8],
) -> Result<(), OperationFailure> {
    if name.is_empty()
        || name.chars().any(char::is_control)
        || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(OperationFailure::new(
            "configuration_path_invalid",
            "servers.invalid_settings",
        ));
    }
    let destination = directory.join(name);
    let temporary = directory.join(format!(".{name}-{}.tmp", uuid::Uuid::new_v4().as_simple()));
    write_private_runtime_file(&temporary, contents).await?;
    if let Err(error) = replace_runtime_file(&temporary, &destination).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

fn xml_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

async fn ensure_staging_parent(root: &Path) -> Result<PathBuf, OperationFailure> {
    let staging = root.join(".staging");
    match tokio::fs::symlink_metadata(&staging).await {
        Ok(metadata) if metadata.is_dir() && !runtime_metadata_is_link_like(&metadata) => {}
        Ok(_) => {
            return Err(OperationFailure::new(
                "install_tree_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(&staging)
                .await
                .map_err(OperationFailure::internal)?;
            #[cfg(unix)]
            tokio::fs::set_permissions(
                &staging,
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .await
            .map_err(OperationFailure::internal)?;
        }
        Err(error) => return Err(OperationFailure::internal(error)),
    }
    Ok(staging)
}

/// Hytale extracts JLine, Jansi and Netty QUIC shared libraries at startup.
/// Hardened container deployments mount `/tmp` as `noexec`, so Java must use
/// an instance-owned directory on the executable game-data filesystem.
async fn prepare_hytale_native_workdir(root: &Path) -> Result<PathBuf, OperationFailure> {
    let runtime = root.join(".dmx-runtime");
    match tokio::fs::symlink_metadata(&runtime).await {
        Ok(metadata) if metadata.is_dir() && !runtime_metadata_is_link_like(&metadata) => {}
        Ok(_) => {
            return Err(OperationFailure::new(
                "hytale_native_workdir_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(&runtime)
                .await
                .map_err(OperationFailure::internal)?;
        }
        Err(error) => return Err(OperationFailure::internal(error)),
    }
    set_private_directory_permissions(&runtime).await?;

    let native = runtime.join("hytale-native");
    match tokio::fs::symlink_metadata(&native).await {
        Ok(metadata) if metadata.is_dir() && !runtime_metadata_is_link_like(&metadata) => {
            tokio::fs::remove_dir_all(&native)
                .await
                .map_err(OperationFailure::internal)?;
        }
        Ok(_) => {
            return Err(OperationFailure::new(
                "hytale_native_workdir_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(OperationFailure::internal(error)),
    }
    tokio::fs::create_dir(&native)
        .await
        .map_err(OperationFailure::internal)?;
    set_private_directory_permissions(&native).await?;
    Ok(native)
}

async fn set_private_directory_permissions(path: &Path) -> Result<(), OperationFailure> {
    #[cfg(unix)]
    tokio::fs::set_permissions(
        path,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
    )
    .await
    .map_err(OperationFailure::internal)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

async fn remove_dir_if_exists(path: &Path) -> Result<(), AppError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            tokio::fs::remove_file(path).await?;
        }
        Ok(_) => tokio::fs::remove_dir_all(path).await?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn append_bounded_output(output: &mut String, line: &str, limit: usize) {
    if output.len() >= limit {
        return;
    }
    let remaining = limit - output.len();
    let mut end = line.len().min(remaining);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&line[..end]);
    if output.len() < limit {
        output.push('\n');
    }
}

fn append_bounded_tail(output: &mut String, line: &str, limit: usize) {
    output.push_str(line);
    output.push('\n');
    if output.len() <= limit {
        return;
    }
    let mut start = output.len() - limit;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    output.drain(..start);
}

async fn reset_install_logs(root: &Path) -> Result<(), OperationFailure> {
    let logs = root.join("logs");
    match tokio::fs::symlink_metadata(&logs).await {
        Ok(metadata) if metadata.is_dir() && !runtime_metadata_is_link_like(&metadata) => {}
        Ok(_) => {
            return Err(OperationFailure::new(
                "install_log_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(&logs)
                .await
                .map_err(OperationFailure::internal)?;
        }
        Err(error) => return Err(OperationFailure::internal(error)),
    }
    for name in ["install.log", "install.error.log", "install.combined.log"] {
        for generation in 0..LOG_GENERATIONS {
            let path = if generation == 0 {
                logs.join(name)
            } else {
                logs.join(format!("{name}.{generation}"))
            };
            match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) if metadata.is_file() || runtime_metadata_is_link_like(&metadata) => {
                    tokio::fs::remove_file(&path)
                        .await
                        .map_err(OperationFailure::internal)?;
                }
                Ok(_) => {
                    return Err(OperationFailure::new(
                        "install_log_unsafe",
                        "servers.instance_data_unsafe",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(OperationFailure::internal(error)),
            }
        }
    }
    Ok(())
}

async fn read_log_tail(
    path: &Path,
    stream: &str,
    limit: usize,
    max_bytes: u64,
) -> Result<Vec<RuntimeLogLine>, AppError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || runtime_metadata_is_link_like(&metadata) {
        return Err(AppError::Conflict("servers.instance_data_unsafe".into()));
    }
    let start = metadata.len().saturating_sub(max_bytes);
    let mut file = tokio::fs::File::open(path).await?;
    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }
    let mut bytes = Vec::with_capacity((metadata.len() - start).min(max_bytes) as usize);
    file.read_to_end(&mut bytes).await?;
    let contents = String::from_utf8_lossy(&bytes);
    let contents = if start > 0 {
        contents
            .split_once('\n')
            .map_or("", |(_, complete_lines)| complete_lines)
    } else {
        contents.as_ref()
    };
    let mut messages: Vec<_> = contents
        .lines()
        .rev()
        .take(limit)
        .map(|message| RuntimeLogLine {
            stream: stream.to_string(),
            message: message.trim_end_matches('\r').to_string(),
        })
        .collect();
    messages.reverse();
    Ok(messages)
}

struct OutputPumpConfig {
    log_path: PathBuf,
    combined_log: Option<Arc<Mutex<RotatingLog>>>,
    stream: &'static str,
    instance_id: String,
    events: EventHub,
    redactions: Vec<String>,
    observer: Option<mpsc::Sender<String>>,
    player_observer: Option<mpsc::Sender<String>>,
    public_log_policy: PublicLogPolicy,
}

async fn pump_output_observed<R>(mut reader: R, config: OutputPumpConfig)
where
    R: AsyncRead + Unpin,
{
    let mut writer = match RotatingLog::open(config.log_path).await {
        Ok(writer) => writer,
        Err(error) => {
            tracing::error!(instance_id = %config.instance_id, %error, "cannot open server log");
            return;
        }
    };
    let mut chunk = [0_u8; 4096];
    let mut line = Vec::with_capacity(4096);
    let mut truncated = false;
    let context = LogLineContext {
        stream: config.stream,
        instance_id: &config.instance_id,
        events: &config.events,
        redactions: &config.redactions,
        observer: config.observer.as_ref(),
        player_observer: config.player_observer.as_ref(),
        combined_log: config.combined_log.as_ref(),
    };
    let mut public_log_sanitizer = PublicLogSanitizer::new(config.public_log_policy);
    loop {
        let read = match tokio::time::timeout(PARTIAL_LOG_FLUSH_INTERVAL, reader.read(&mut chunk))
            .await
        {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => read,
            Ok(Err(error)) => {
                tracing::warn!(instance_id = %config.instance_id, %error, "server log read failed");
                break;
            }
            Err(_) => {
                if !line.is_empty() || truncated {
                    emit_log_line(
                        &mut writer,
                        &mut line,
                        &mut truncated,
                        &context,
                        &mut public_log_sanitizer,
                    )
                    .await;
                }
                continue;
            }
        };
        for byte in &chunk[..read] {
            if matches!(*byte, b'\n' | b'\r') {
                if !line.is_empty() || truncated {
                    emit_log_line(
                        &mut writer,
                        &mut line,
                        &mut truncated,
                        &context,
                        &mut public_log_sanitizer,
                    )
                    .await;
                }
            } else if line.len() < MAX_LOG_LINE {
                line.push(*byte);
            } else {
                truncated = true;
            }
        }
    }
    if !line.is_empty() || truncated {
        emit_log_line(
            &mut writer,
            &mut line,
            &mut truncated,
            &context,
            &mut public_log_sanitizer,
        )
        .await;
    }
}

struct LogLineContext<'a> {
    stream: &'static str,
    instance_id: &'a str,
    events: &'a EventHub,
    redactions: &'a [String],
    observer: Option<&'a mpsc::Sender<String>>,
    player_observer: Option<&'a mpsc::Sender<String>>,
    combined_log: Option<&'a Arc<Mutex<RotatingLog>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicLogPolicy {
    Normal,
    HytaleDeviceFlow,
}

enum PublicLogSanitizer {
    Normal,
    HytaleDeviceFlow(HytaleDeviceLogSanitizer),
}

impl PublicLogSanitizer {
    fn new(policy: PublicLogPolicy) -> Self {
        match policy {
            PublicLogPolicy::Normal => Self::Normal,
            PublicLogPolicy::HytaleDeviceFlow => {
                Self::HytaleDeviceFlow(HytaleDeviceLogSanitizer::default())
            }
        }
    }

    fn sanitize(&mut self, line: &str) -> String {
        match self {
            Self::Normal => line.to_string(),
            Self::HytaleDeviceFlow(sanitizer) => sanitizer.sanitize(line),
        }
    }
}

#[derive(Default)]
struct HytaleDeviceLogSanitizer {
    authorization_tail: String,
    user_codes: Vec<String>,
}

impl HytaleDeviceLogSanitizer {
    fn sanitize(&mut self, line: &str) -> String {
        append_bounded_tail(&mut self.authorization_tail, line, 16 * 1024);
        if let Some(code) =
            installers::hytale::detect_device_authorization(&self.authorization_tail)
                .and_then(|authorization| authorization.user_code)
            && !self.user_codes.iter().any(|known| known == &code)
        {
            self.user_codes.push(code);
            if self.user_codes.len() > 4 {
                self.user_codes.remove(0);
            }
        }
        redact_hytale_device_authorization(line, &self.user_codes)
    }
}

async fn emit_log_line(
    writer: &mut RotatingLog,
    bytes: &mut Vec<u8>,
    truncated: &mut bool,
    context: &LogLineContext<'_>,
    public_log_sanitizer: &mut PublicLogSanitizer,
) {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let mut line = String::from_utf8_lossy(bytes).replace('\0', "�");
    for secret in context.redactions.iter().filter(|value| !value.is_empty()) {
        line = line.replace(secret, "[REDACTED]");
    }
    if let Some(observer) = context.observer {
        let _ = observer.try_send(line.clone());
    }
    if let Some(observer) = context.player_observer {
        let _ = observer.try_send(line.clone());
    }
    line = public_log_sanitizer.sanitize(&line);
    if *truncated {
        line.push_str(" …[truncated]");
    }
    if let Err(error) = writer.write_line(&line).await {
        tracing::warn!(instance_id = %context.instance_id, %error, "server log write failed");
    }
    if let Some(combined_log) = context.combined_log {
        let combined_line = if context.stream.ends_with("_error") {
            format!("[stderr] {line}")
        } else {
            line.clone()
        };
        let mut combined_log = combined_log.lock().await;
        if let Err(error) = combined_log.write_line(&combined_line).await {
            tracing::warn!(instance_id = %context.instance_id, %error, "combined server log write failed");
        }
    }
    context.events.publish(
        "server.log",
        Some(context.instance_id.to_string()),
        serde_json::json!({"stream": context.stream, "message": line}),
    );
    bytes.clear();
    *truncated = false;
}

static HYTALE_DEVICE_LOG_URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"https://(?:accounts\.hytale\.com/device|oauth\.accounts\.hytale\.com/oauth2/device/verify)[^\s\x00-\x1f]*",
    )
    .expect("constant Hytale authorization URL regex is valid")
});
static HYTALE_LABELLED_CODE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)((?:authorization|user[_ ]?|device|verification|enter)\s*code\s*[:=]\s*)([A-Z0-9-]{4,32})",
    )
    .expect("constant labelled Hytale authorization code regex is valid")
});
static HYTALE_GENERIC_CODE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\s*code\s*[:=]\s*)([A-Z0-9-]{4,32})")
        .expect("constant generic Hytale authorization code regex is valid")
});
static HYTALE_STANDALONE_CODE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:[A-Z0-9]{8}|[A-Z0-9]{4}-[A-Z0-9]{4})\s*$")
        .expect("constant standalone Hytale authorization code regex is valid")
});

fn redact_hytale_device_authorization(line: &str, user_codes: &[String]) -> String {
    if HYTALE_STANDALONE_CODE_PATTERN.is_match(line) {
        return "[REDACTED — use the secure action card]".to_string();
    }
    let mut redacted = HYTALE_DEVICE_LOG_URL_PATTERN
        .replace_all(line, |captures: &regex::Captures<'_>| {
            let matched = captures.get(0).expect("full URL match").as_str();
            let Some((base, _)) = matched.split_once('?') else {
                return matched.to_string();
            };
            format!("{base}?[REDACTED]")
        })
        .into_owned();
    for code in user_codes.iter().filter(|code| !code.is_empty()) {
        redacted = redacted.replace(code, "[REDACTED — use the secure action card]");
    }
    redacted = HYTALE_LABELLED_CODE_PATTERN
        .replace_all(&redacted, "${1}[REDACTED — use the secure action card]")
        .into_owned();
    HYTALE_GENERIC_CODE_PATTERN
        .replace_all(&redacted, "${1}[REDACTED — use the secure action card]")
        .into_owned()
}

struct RotatingLog {
    path: PathBuf,
    file: Option<tokio::fs::File>,
    size: u64,
}

impl RotatingLog {
    async fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let size = tokio::fs::metadata(&path)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        Ok(Self {
            path,
            file: Some(file),
            size,
        })
    }

    async fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let required = line.len() as u64 + 1;
        if self.size.saturating_add(required) > MAX_LOG_SIZE {
            self.rotate().await?;
        }
        let file = self.file.as_mut().expect("log file remains open");
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        self.size = self.size.saturating_add(required);
        Ok(())
    }

    async fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush().await?;
        }
        for generation in (1..LOG_GENERATIONS).rev() {
            let source = numbered_log(&self.path, generation);
            let destination = numbered_log(&self.path, generation + 1);
            if tokio::fs::try_exists(&source).await.unwrap_or(false) {
                let _ = tokio::fs::remove_file(&destination).await;
                tokio::fs::rename(source, destination).await?;
            }
        }
        if tokio::fs::try_exists(&self.path).await.unwrap_or(false) {
            tokio::fs::rename(&self.path, numbered_log(&self.path, 1)).await?;
        }
        self.file = Some(
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?,
        );
        self.size = 0;
        Ok(())
    }
}

fn numbered_log(path: &Path, generation: usize) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{generation}"));
    PathBuf::from(value)
}

#[derive(Clone, Copy)]
enum HytaleDownloaderPhase {
    VersionCheck,
    ServerDownload,
}

impl HytaleDownloaderPhase {
    fn arguments(self, plan: &installers::hytale::HytaleDownloaderPlan) -> Vec<OsString> {
        match self {
            Self::VersionCheck => plan.version_args(),
            Self::ServerDownload => plan.args.clone(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::VersionCheck => "version-check",
            Self::ServerDownload => "server-download",
        }
    }

    fn safe_arguments(self) -> &'static str {
        match self {
            Self::VersionCheck => "-print-version -skip-update-check",
            Self::ServerDownload => {
                "-download-path <ephemeral-session>/hytale-game.zip -skip-update-check"
            }
        }
    }
}

async fn hytale_credential_file_state(path: &Path) -> String {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_file() && !runtime_metadata_is_link_like(&metadata) => {
            format!("present-{}-bytes", metadata.len())
        }
        Ok(_) => "unsafe-or-non-file".to_string(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
        Err(error) => format!("metadata-error-{:?}", error.kind()),
    }
}

fn hytale_device_request_diagnostic(
    authorization: &installers::hytale::DeviceAuthorization,
    request_number: u32,
) -> String {
    let parsed = reqwest::Url::parse(&authorization.verification_uri).ok();
    let flow =
        parsed
            .as_ref()
            .and_then(reqwest::Url::host_str)
            .map_or("unknown", |host| match host {
                "oauth.accounts.hytale.com" => "downloader",
                "accounts.hytale.com" => "game-server",
                _ => "unknown",
            });
    let complete_uri = parsed
        .as_ref()
        .is_some_and(|uri| uri.query_pairs().any(|(key, _)| key == "user_code"));
    let code_length = authorization.user_code.as_deref().map_or(0, str::len);
    format!(
        "[DMX] Hytale OAuth request #{request_number} published (flow={flow}, verification_uri_complete={}, code_length={code_length}).",
        if complete_uri { "yes" } else { "no" }
    )
}

fn hytale_downloader_failure_diagnostic(output: &str) -> Option<&'static str> {
    let normalized = output.to_ascii_lowercase();
    if normalized.contains("context deadline exceeded") || normalized.contains("expired_token") {
        return Some(
            "[DMX] Diagnostic classification=oauth-device-timeout: the downloader did not receive a completed browser approval before its OAuth session expired.",
        );
    }
    if normalized.contains("access_denied") || normalized.contains("authorization denied") {
        return Some(
            "[DMX] Diagnostic classification=oauth-access-denied: the Hytale authorization request was denied in the browser.",
        );
    }
    if normalized.contains("invalid_grant")
        || normalized.contains("user_code session could not be found")
    {
        return Some(
            "[DMX] Diagnostic classification=oauth-session-invalid: the Hytale authorization code and downloader session no longer match.",
        );
    }
    if normalized.contains("certificate")
        || normalized.contains("tls")
        || normalized.contains("no such host")
        || normalized.contains("connection refused")
    {
        return Some(
            "[DMX] Diagnostic classification=network-or-tls: the downloader could not establish a valid connection to the Hytale service.",
        );
    }
    None
}

#[derive(Clone, Copy)]
enum InstallInterruption {
    Cancelled,
    TimedOut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WaitingInstallAbort {
    Cancelled,
    TimedOut,
}

async fn terminate_installer(
    child: &mut Child,
    pid: u32,
    windows_job: Option<isize>,
) -> std::io::Result<std::process::ExitStatus> {
    let _ = hard_kill_group(pid, windows_job);
    match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            child.kill().await?;
            child.wait().await
        }
    }
}

struct ContainedChild {
    child: Child,
    #[cfg(windows)]
    windows_job: WindowsJob,
}

#[derive(Debug)]
enum ContainedSpawnError {
    Spawn(std::io::Error),
    #[cfg_attr(not(windows), allow(dead_code))]
    Containment(std::io::Error),
}

fn spawn_contained(command: &mut Command) -> Result<ContainedChild, ContainedSpawnError> {
    configure_process_group(command).map_err(ContainedSpawnError::Spawn)?;
    let child = command.spawn().map_err(ContainedSpawnError::Spawn)?;

    #[cfg(windows)]
    {
        let mut child = child;
        let windows_job = match WindowsJob::assign_and_resume(&child) {
            Ok(job) => job,
            Err(error) => {
                // CREATE_SUSPENDED guarantees that no uncontained child code has
                // executed. If containment or resumption fails, terminate the
                // suspended root before dropping its process handle.
                let _ = child.start_kill();
                return Err(ContainedSpawnError::Containment(error));
            }
        };
        Ok(ContainedChild { child, windows_job })
    }

    #[cfg(not(windows))]
    {
        Ok(ContainedChild { child })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopSignal {
    Interrupt,
    Terminate,
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            #[cfg(target_os = "linux")]
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};
    ensure_windows_console()?;
    command
        .as_std_mut()
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    Ok(())
}

#[cfg(windows)]
static WINDOWS_CONSOLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
fn ensure_windows_console() -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{AllocConsole, GetConsoleProcessList};

    let _guard = WINDOWS_CONSOLE_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("Windows console lock is poisoned"))?;
    let mut process_id = 0_u32;
    if unsafe { GetConsoleProcessList(&mut process_id, 1) } != 0 {
        return Ok(());
    }
    if unsafe { AllocConsole() } == 0 {
        // Another native thread or host may have attached a console between
        // the probe and allocation. Accept that race only after re-verifying.
        if unsafe { GetConsoleProcessList(&mut process_id, 1) } != 0 {
            return Ok(());
        }
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { GetConsoleProcessList(&mut process_id, 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn send_group_signal(
    pid: u32,
    signal: StopSignal,
    _windows_job: Option<isize>,
) -> std::io::Result<()> {
    let signal = match signal {
        StopSignal::Interrupt => libc::SIGINT,
        StopSignal::Terminate => libc::SIGTERM,
    };
    let result = unsafe { libc::kill(-(pid as i32), signal) };
    if result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn send_group_signal(
    pid: u32,
    signal: StopSignal,
    windows_job: Option<isize>,
) -> std::io::Result<()> {
    match windows_stop_action(signal) {
        WindowsStopAction::CtrlBreak => send_ctrl_break(pid),
        WindowsStopAction::TerminateJob => {
            terminate_windows_job(windows_job, WINDOWS_REQUESTED_TERMINATE_EXIT_CODE)
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn send_group_signal(
    _pid: u32,
    _signal: StopSignal,
    _windows_job: Option<isize>,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn hard_kill_group(pid: u32, _windows_job: Option<isize>) -> std::io::Result<()> {
    let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    if result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(windows)]
const WINDOWS_REQUESTED_TERMINATE_EXIT_CODE: u32 = 1;
#[cfg(windows)]
const WINDOWS_FORCE_KILL_EXIT_CODE: u32 = 137;

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsStopAction {
    CtrlBreak,
    TerminateJob,
}

#[cfg(windows)]
fn windows_stop_action(signal: StopSignal) -> WindowsStopAction {
    match signal {
        StopSignal::Interrupt => WindowsStopAction::CtrlBreak,
        StopSignal::Terminate => WindowsStopAction::TerminateJob,
    }
}

#[cfg(windows)]
fn send_ctrl_break(pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_windows_job(windows_job: Option<isize>, exit_code: u32) -> std::io::Result<()> {
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;
    let handle = windows_job.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Windows Job Object is unavailable",
        )
    })?;
    if unsafe { TerminateJobObject(handle as _, exit_code) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn hard_kill_group(_pid: u32, windows_job: Option<isize>) -> std::io::Result<()> {
    terminate_windows_job(windows_job, WINDOWS_FORCE_KILL_EXIT_CODE)
}

#[cfg(not(any(unix, windows)))]
fn hard_kill_group(_pid: u32, _windows_job: Option<isize>) -> std::io::Result<()> {
    Err(std::io::Error::other("process groups are unsupported"))
}

#[cfg(windows)]
struct WindowsJob {
    handle: isize,
}

#[cfg(windows)]
impl WindowsJob {
    fn assign_and_resume(child: &Child) -> std::io::Result<Self> {
        use std::mem::{size_of, zeroed};
        use windows_sys::Win32::{
            Foundation::HANDLE,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };
        let pid = child
            .id()
            .ok_or_else(|| std::io::Error::other("process has no identifier"))?;
        let process = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("process has no Windows handle"))?
            as HANDLE;
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Self {
            handle: job as isize,
        };
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job.handle as _,
                JobObjectExtendedLimitInformation,
                &information as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { AssignProcessToJobObject(job.handle as _, process) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if let Err(error) = resume_suspended_primary_thread(pid) {
            let _ = terminate_windows_job(Some(job.handle), WINDOWS_FORCE_KILL_EXIT_CODE);
            return Err(error);
        }
        Ok(job)
    }

    #[cfg(test)]
    fn contains_child(&self, child: &Child) -> std::io::Result<bool> {
        use windows_sys::Win32::{Foundation::HANDLE, System::JobObjects::IsProcessInJob};
        let process = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("process has no Windows handle"))?
            as HANDLE;
        let mut contained = 0;
        if unsafe { IsProcessInJob(process, self.handle as _, &mut contained) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(contained != 0)
    }
}

#[cfg(windows)]
struct OwnedWindowsHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for OwnedWindowsHandle {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn resume_suspended_primary_thread(pid: u32) -> std::io::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = OwnedWindowsHandle(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "suspended process has no primary thread",
            ))
        } else {
            Err(error)
        };
    }

    let mut primary_thread_id = None;
    loop {
        if entry.th32OwnerProcessID == pid
            && primary_thread_id.replace(entry.th32ThreadID).is_some()
        {
            return Err(std::io::Error::other(
                "suspended process has multiple threads before containment",
            ));
        }
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }

    let thread_id = primary_thread_id.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "suspended process has no primary thread",
        )
    })?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let thread = OwnedWindowsHandle(thread);
    let previous_suspend_count = unsafe { ResumeThread(thread.0) };
    if previous_suspend_count == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    if previous_suspend_count != 1 {
        return Err(std::io::Error::other(format!(
            "unexpected primary thread suspend count {previous_suspend_count}"
        )));
    }
    Ok(())
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.handle as _) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_update_parser_selects_the_requested_branch_build() {
        let output = r#"
            "branches"
            {
                "public"
                {
                    "buildid" "123456"
                }
                "preview"
                {
                    "buildid" "987654"
                }
            }
        "#;
        assert_eq!(
            parse_steam_branch_build(output, "public").as_deref(),
            Some("123456")
        );
        assert_eq!(
            parse_steam_branch_build(output, "preview").as_deref(),
            Some("987654")
        );
        assert!(parse_steam_branch_build(output, "../public").is_none());
        assert!(parse_steam_branch_build(output, "missing").is_none());
    }

    #[test]
    fn game_update_status_reports_only_a_real_target_difference() {
        let instance = RuntimeInstance {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: "hytale".to_string(),
            profile_revision: 1,
            settings: "{}".to_string(),
            config_version: 1,
            installation_state: "installed".to_string(),
            installed_version: Some("0.5.7".to_string()),
            installed_build: None,
            desired_state: "stopped".to_string(),
            runtime_state: "stopped".to_string(),
            auto_start: false,
            watchdog_enabled: true,
        };
        assert!(!has_game_update(&instance, Some("0.5.7"), None));
        assert!(has_game_update(&instance, Some("0.5.8"), None));
        assert!(!has_game_update(&instance, Some("0.5.6"), None));
        assert!(!has_game_update(&instance, Some("unknown"), None));
        assert!(!has_game_update(&instance, None, None));

        let date_release = RuntimeInstance {
            installed_version: Some("2026.06.15-abcd".to_string()),
            ..instance
        };
        assert!(has_game_update(
            &date_release,
            Some("2026.07.01-efgh"),
            None
        ));
        assert!(!has_game_update(
            &date_release,
            Some("2026.06.14-efgh"),
            None
        ));
    }

    #[tokio::test]
    async fn committed_native_update_persists_the_resolved_version_settings() {
        let (_root, actor, _user_id) =
            runtime_actor_fixture("minecraft-java-vanilla", "stopped").await;
        let resolved = serde_json::json!({
            "version": "1.22.0",
            "eula_accepted": true,
            "max_memory_mb": 8192
        });

        actor
            .mark_install_committed(Some("1.22.0"), None, Some(&resolved))
            .await
            .unwrap();

        let (stored, config_version): (String, i64) =
            sqlx::query_as("SELECT settings, config_version FROM instances WHERE id = ?")
                .bind(&actor.instance_id)
                .fetch_one(&actor.inner.pool)
                .await
                .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), resolved);
        assert_eq!(config_version, 2);
        assert_eq!(
            actor.instance().await.unwrap().installed_version.as_deref(),
            Some("1.22.0")
        );
    }

    #[test]
    fn public_install_failure_detail_is_bounded_and_redacts_the_instance_path() {
        let error = OperationFailure::with_internal(
            "install_metadata_failed",
            "servers.installation_failed",
            "/data/instances/example/game/.dmx-install.json\nFile exists (os error 17)",
        );
        assert_eq!(
            public_install_failure_detail(&error, Path::new("/data/instances/example")).as_deref(),
            Some("<instance>/game/.dmx-install.json File exists (os error 17)")
        );
        let private_error = OperationFailure::with_internal(
            "hytale_credentials_failed",
            "servers.installation_failed",
            "secret-bearing provider detail",
        );
        assert!(
            public_install_failure_detail(&private_error, Path::new("/data/instances/example"))
                .is_none()
        );
    }

    async fn runtime_actor_fixture(
        profile_id: &str,
        desired_state: &str,
    ) -> (tempfile::TempDir, InstanceActor, String) {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/runtime.db?mode=rwc", root.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let registry = profiles::ProfileRegistry::builtins();
        registry.persist_builtins(&pool).await.unwrap();
        let profile_revision = registry.get(profile_id).unwrap().revision;

        let instance_id = uuid::Uuid::new_v4().to_string();
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'runtime-test-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, installation_state, \
              installed_version, installed_build, desired_state, runtime_state, created_at, updated_at) \
             VALUES (?, 'runtime-fixture', ?, ?, '{}', 'installed', '1.0.0', '100', ?, 'stopped', ?, ?)",
        )
        .bind(&instance_id)
        .bind(profile_id)
        .bind(profile_revision)
        .bind(desired_state)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let settings = Arc::new(Settings {
            config_file: root.path().join("config.toml"),
            data_dir: root.path().to_path_buf(),
            static_dir: root.path().join("static"),
            bind: "127.0.0.1:5500".parse().unwrap(),
            database_url,
            master_key_file: root.path().join("master.key"),
            steamcmd_path: root.path().join("missing-steamcmd"),
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
        });
        let events = EventHub::new(32);
        let secrets = SecretStore::load_or_create(&settings.master_key_file).unwrap();
        let inner = Arc::new(RuntimeInner {
            pool,
            settings,
            events,
            secrets,
            actors: Mutex::new(HashMap::new()),
            actor_crash_restarts: Mutex::new(HashMap::new()),
            install_cancellations: Mutex::new(HashMap::new()),
            game_update_checks: Mutex::new(HashMap::new()),
            game_update_locks: Mutex::new(HashMap::new()),
        });
        let (sender, _receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        let actor = InstanceActor {
            instance_id,
            inner,
            sender,
            process: None,
            generation: 0,
            watchdog_attempts: 0,
            hytale_update_restarts: 0,
            backup_token: None,
            backup_restart_after: false,
            backup_started_stopped: false,
            filesystem_maintenance_token: None,
            filesystem_autostart_pending: false,
            retain_install_rollback: false,
        };
        (root, actor, user_id)
    }

    async fn update_job(actor: &InstanceActor, user_id: &str) -> Job {
        jobs::create(
            &actor.inner.pool,
            &actor.instance_id,
            "install",
            user_id,
            None,
        )
        .await
        .unwrap()
        .0
    }

    async fn wait_for_abnormal_actor_cleanup(
        inner: &RuntimeInner,
        instance_id: &str,
        job_id: &str,
        sender: &mpsc::Sender<ActorCommand>,
    ) {
        const ATTEMPTS: usize = 500;
        const RETRY_DELAY: Duration = Duration::from_millis(20);

        for _ in 0..ATTEMPTS {
            let current = jobs::get(&inner.pool, job_id).await.unwrap();
            let cancellation_removed = inner
                .install_cancellations
                .lock()
                .await
                .get(instance_id)
                .is_none();
            let runtime_state: String =
                sqlx::query_scalar("SELECT runtime_state FROM instances WHERE id = ?")
                    .bind(instance_id)
                    .fetch_one(&inner.pool)
                    .await
                    .unwrap();
            if current.state == JobState::Interrupted
                && cancellation_removed
                && runtime_state == "crashed"
                && sender.is_closed()
            {
                return;
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }

        let current = jobs::get(&inner.pool, job_id).await.unwrap();
        let cancellation_present = inner
            .install_cancellations
            .lock()
            .await
            .contains_key(instance_id);
        let runtime_state: String =
            sqlx::query_scalar("SELECT runtime_state FROM instances WHERE id = ?")
                .bind(instance_id)
                .fetch_one(&inner.pool)
                .await
                .unwrap();
        panic!(
            "actor cleanup timed out: job_state={:?}, cancellation_present={}, runtime_state={}, mailbox_closed={}",
            current.state,
            cancellation_present,
            runtime_state,
            sender.is_closed()
        );
    }

    #[test]
    fn console_is_one_bounded_command() {
        assert!(validate_console_command("save-all flush").is_ok());
        assert!(validate_console_command("stop\nstatus").is_err());
        assert!(validate_console_command(&"x".repeat(MAX_CONSOLE_COMMAND + 1)).is_err());
    }

    #[tokio::test]
    async fn reports_actor_future_size() {
        let (_root, mut actor, user_id) =
            runtime_actor_fixture("minecraft-bedrock", "stopped").await;
        let job = update_job(&actor, &user_id).await;
        let execute = actor.execute_inner(job, RuntimeAction::Install);
        eprintln!("execute future size: {}", std::mem::size_of_val(&execute));
        std::mem::drop(execute);
        let (_sender, receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        let future = actor.run(receiver);
        eprintln!("actor future size: {}", std::mem::size_of_val(&future));
    }

    #[tokio::test]
    async fn actor_panic_interrupts_queued_claim_and_cleans_runtime_state() {
        let (_root, mut actor, user_id) = runtime_actor_fixture("hytale", "running").await;
        let instance_id = actor.instance_id.clone();
        let inner = Arc::clone(&actor.inner);
        sqlx::query("UPDATE instances SET watchdog_enabled = 0 WHERE id = ?")
            .bind(&instance_id)
            .execute(&inner.pool)
            .await
            .unwrap();
        let (job, created, claim) =
            jobs::create_claimed(&inner.pool, &instance_id, "start", &user_id, None)
                .await
                .unwrap();
        assert!(created);
        actor.register_install_cancellation(&job.id).await;

        let (sender, receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        actor.sender = sender.clone();
        inner
            .actors
            .lock()
            .await
            .insert(instance_id.clone(), sender.clone());
        sender.send(ActorCommand::Panic).await.unwrap();
        sender
            .send(ActorCommand::Execute {
                job: Box::new(job.clone()),
                action: RuntimeAction::Start,
                claim,
            })
            .await
            .unwrap();
        let task = spawn_instance_actor(actor, receiver);
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("actor supervisor timed out after the injected panic")
            .expect("actor supervisor task failed after catching the injected panic");

        wait_for_abnormal_actor_cleanup(&inner, &instance_id, &job.id, &sender).await;
    }

    #[tokio::test]
    async fn aborting_an_unpolled_actor_interrupts_queued_claim_and_cleans_cancellation() {
        let (_root, mut actor, user_id) = runtime_actor_fixture("hytale", "running").await;
        let instance_id = actor.instance_id.clone();
        let inner = Arc::clone(&actor.inner);
        sqlx::query("UPDATE instances SET watchdog_enabled = 0 WHERE id = ?")
            .bind(&instance_id)
            .execute(&inner.pool)
            .await
            .unwrap();
        let (job, created, claim) =
            jobs::create_claimed(&inner.pool, &instance_id, "start", &user_id, None)
                .await
                .unwrap();
        assert!(created);
        actor.register_install_cancellation(&job.id).await;

        let (sender, receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        actor.sender = sender.clone();
        inner
            .actors
            .lock()
            .await
            .insert(instance_id.clone(), sender.clone());
        sender
            .send(ActorCommand::Execute {
                job: Box::new(job.clone()),
                action: RuntimeAction::Start,
                claim,
            })
            .await
            .unwrap();
        let task = spawn_instance_actor(actor, receiver);
        task.abort();
        let _ = task.await;

        wait_for_abnormal_actor_cleanup(&inner, &instance_id, &job.id, &sender).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_managed_process_kills_its_process_group() {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "trap '' TERM; (trap '' TERM; sleep 60) & printf ready; wait",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = spawn_contained(&mut command).unwrap().child;
        let pid = child.id().unwrap();
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let mut ready = [0_u8; 5];
        tokio::time::timeout(Duration::from_secs(2), stdout.read_exact(&mut ready))
            .await
            .expect("the process-group fixture must become ready")
            .unwrap();
        assert_eq!(&ready, b"ready");
        let (exit_tx, exit_rx) = watch::channel(None);
        let mut observed_exit = exit_rx.clone();
        tokio::spawn(async move {
            let status = child.wait().await.unwrap();
            let _ = exit_tx.send(Some(ExitOutcome {
                success: status.success(),
                code: status.code(),
                elapsed: Duration::from_millis(1),
            }));
        });
        let (metrics_stop, _metrics_rx) = watch::channel(false);
        let process = ManagedProcess {
            pid,
            stdin,
            exit_rx,
            generation: 1,
            stop: StopStrategy::Terminate { timeout_seconds: 1 },
            output_rx: None,
            _metrics_stop: metrics_stop,
        };

        drop(process);

        tokio::time::timeout(Duration::from_secs(2), async {
            while observed_exit.borrow().is_none() {
                observed_exit.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(!observed_exit.borrow().as_ref().unwrap().success);

        // The background descendant inherits stdout. EOF therefore proves
        // that the entire group was killed; probing the numeric PGID after the
        // root is reaped is racy because the kernel may immediately reuse it
        // for an unrelated process group and return EPERM instead of ESRCH.
        let mut trailing_output = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            stdout.read_to_end(&mut trailing_output),
        )
        .await
        .expect("a descendant kept the process-group pipe open")
        .unwrap();
        assert!(trailing_output.is_empty());
    }

    #[tokio::test]
    async fn stopped_backup_lease_blocks_a_stale_watchdog_restart() {
        let (_root, mut actor, _user_id) = runtime_actor_fixture("hytale", "running").await;
        let token = actor.begin_backup().await.unwrap().unwrap();

        actor.watchdog_restart().await;

        assert_eq!(actor.backup_token.as_deref(), Some(token.as_str()));
        assert!(actor.process.is_none());
        assert_eq!(actor.instance().await.unwrap().runtime_state, "stopped");
        actor.release_backup_lease(false).await.unwrap();
        assert!(actor.backup_token.is_none());
    }

    #[tokio::test]
    async fn start_job_cannot_race_a_filesystem_maintenance_lease() {
        let (_root, actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let instance_id = actor.instance_id.clone();
        let runtime = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        drop(actor);

        let lease = runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap();
        let (job, _) = jobs::create(&runtime.inner.pool, &instance_id, "start", &user_id, None)
            .await
            .unwrap();
        runtime
            .enqueue(job.clone(), RuntimeAction::Start)
            .await
            .unwrap();

        let failed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let current = jobs::get(&runtime.inner.pool, &job.id).await.unwrap();
                if current.state == JobState::Failed {
                    break current;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            failed.error_code.as_deref(),
            Some("server_filesystem_maintenance")
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT runtime_state FROM instances WHERE id = ?")
                .bind(&instance_id)
                .fetch_one(&runtime.inner.pool)
                .await
                .unwrap(),
            "stopped"
        );

        lease.release().await.unwrap();
        runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap()
            .release()
            .await
            .unwrap();
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn maintenance_lease_blocks_a_stale_watchdog_restart() {
        let (_root, mut actor, _user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let token = actor.begin_filesystem_maintenance(None).await.unwrap();
        sqlx::query("UPDATE instances SET desired_state = 'running' WHERE id = ?")
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();

        actor.watchdog_restart().await;

        assert!(actor.process.is_none());
        assert_eq!(
            actor.filesystem_maintenance_token.as_deref(),
            Some(token.as_str())
        );
        actor.end_filesystem_maintenance(&token).unwrap();
    }

    #[tokio::test]
    async fn dropped_filesystem_lease_is_released_by_raii_fallback() {
        let (_root, actor, _user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let instance_id = actor.instance_id.clone();
        let runtime = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        drop(actor);
        let lease = runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap();

        drop(lease);

        let replacement = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match runtime.begin_filesystem_maintenance(&instance_id).await {
                    Ok(lease) => break lease,
                    Err(AppError::Conflict(_)) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected lease error: {error}"),
                }
            }
        })
        .await
        .unwrap();
        replacement.release().await.unwrap();
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn cancelling_explicit_filesystem_release_retries_the_same_token() {
        let (sender, mut receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        let lease = FilesystemLease {
            sender,
            token: Some("lease-token".to_string()),
        };
        let release = tokio::spawn(lease.release());
        let first_response = match receiver.recv().await.unwrap() {
            ActorCommand::EndFilesystemMaintenance { token, response } => {
                assert_eq!(token, "lease-token");
                response
            }
            _ => panic!("unexpected actor command"),
        };
        release.abort();
        let _ = release.await;
        drop(first_response);
        let retry = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        match retry {
            ActorCommand::EndFilesystemMaintenance { token, response } => {
                assert_eq!(token, "lease-token");
                response.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected actor command"),
        }
    }

    #[tokio::test]
    async fn cancelling_explicit_backup_release_retries_the_same_token() {
        let (sender, mut receiver) = mpsc::channel(ACTOR_QUEUE_SIZE);
        let lease = BackupLease {
            sender,
            token: Some("backup-token".to_string()),
        };
        let release = tokio::spawn(lease.release());
        let first_response = match receiver.recv().await.unwrap() {
            ActorCommand::EndBackup { token, response } => {
                assert_eq!(token, "backup-token");
                response
            }
            _ => panic!("unexpected actor command"),
        };
        release.abort();
        let _ = release.await;
        drop(first_response);
        let retry = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        match retry {
            ActorCommand::EndBackup { token, response } => {
                assert_eq!(token, "backup-token");
                response.send(Ok(())).unwrap();
            }
            _ => panic!("unexpected actor command"),
        }
    }

    #[tokio::test]
    async fn lease_queued_in_abandoned_oneshot_is_released() {
        let (_root, actor, _user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let instance_id = actor.instance_id.clone();
        let runtime = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        drop(actor);
        let lease = runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap();
        let (response, receiver) = oneshot::channel();
        response.send(lease).unwrap();
        drop(receiver);

        let replacement = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match runtime.begin_filesystem_maintenance(&instance_id).await {
                    Ok(lease) => break lease,
                    Err(AppError::Conflict(_)) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected lease error: {error}"),
                }
            }
        })
        .await
        .unwrap();
        replacement.release().await.unwrap();
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn backup_lease_queued_in_abandoned_oneshot_is_released() {
        let (_root, actor, _user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let instance_id = actor.instance_id.clone();
        let runtime = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        drop(actor);
        let lease = runtime.begin_backup(&instance_id).await.unwrap();
        let (response, receiver) = oneshot::channel();
        response.send(lease).unwrap();
        drop(receiver);

        let replacement = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match runtime.begin_filesystem_maintenance(&instance_id).await {
                    Ok(lease) => break lease,
                    Err(AppError::Conflict(_)) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected lease error: {error}"),
                }
            }
        })
        .await
        .unwrap();
        replacement.release().await.unwrap();
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn import_job_lease_is_refused_while_file_maintenance_is_active() {
        let (_root, actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let instance_id = actor.instance_id.clone();
        let runtime = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        drop(actor);
        let file_lease = runtime
            .begin_filesystem_maintenance(&instance_id)
            .await
            .unwrap();
        let (job, _) = jobs::create(
            &runtime.inner.pool,
            &instance_id,
            "import_zip",
            &user_id,
            None,
        )
        .await
        .unwrap();
        assert!(
            matches!(
                runtime
                    .begin_job_filesystem_maintenance(&instance_id, &job.id)
                    .await,
                Err(AppError::Conflict(_))
            ),
            "an import must not overlap an active file/delete lease"
        );
        jobs::fail(
            &runtime.inner.pool,
            &job.id,
            "import_conflict",
            "imports.instance_changed",
        )
        .await
        .unwrap();
        file_lease.release().await.unwrap();
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn autostart_received_during_maintenance_is_replayed_on_release() {
        let (_root, mut actor, _user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let token = actor.begin_filesystem_maintenance(None).await.unwrap();
        sqlx::query("UPDATE instances SET desired_state = 'running' WHERE id = ?")
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();
        actor.auto_start().await;
        assert!(actor.filesystem_autostart_pending);
        actor.end_filesystem_maintenance(&token).unwrap();
        assert!(!actor.filesystem_autostart_pending);
        assert!(actor.filesystem_maintenance_token.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn minecraft_backup_stops_the_process_before_returning_its_lease() {
        let (_root, mut actor, _user_id) =
            runtime_actor_fixture("minecraft-java-paper", "running").await;
        sqlx::query("UPDATE instances SET runtime_state = 'running' WHERE id = ?")
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "read save_off; read save_all; read stop"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let stdin = child.stdin.take();
        let (exit_tx, exit_rx) = watch::channel(None);
        tokio::spawn(async move {
            let status = child.wait().await.unwrap();
            let _ = exit_tx.send(Some(ExitOutcome {
                success: status.success(),
                code: status.code(),
                elapsed: Duration::from_millis(1),
            }));
        });
        let (metrics_stop, _metrics_rx) = watch::channel(false);
        let (output_tx, output_rx) = mpsc::channel(4);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            output_tx
                .send("Automatic saving is now disabled".into())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            output_tx.send("Saved the game".into()).await.unwrap();
        });
        actor.process = Some(ManagedProcess {
            pid,
            stdin,
            exit_rx,
            generation: 1,
            stop: StopStrategy::Stdin {
                command: "stop".into(),
                timeout_seconds: 2,
            },
            output_rx: Some(output_rx),
            _metrics_stop: metrics_stop,
        });

        let token = actor.begin_backup().await.unwrap().unwrap();

        assert!(actor.process.is_none());
        assert!(actor.backup_restart_after);
        assert_eq!(actor.instance().await.unwrap().runtime_state, "stopped");
        sqlx::query("UPDATE instances SET desired_state = 'stopped' WHERE id = ?")
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();
        actor.end_backup(&token).await.unwrap();
        assert!(actor.process.is_none());
        assert_eq!(actor.instance().await.unwrap().runtime_state, "stopped");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn minecraft_backup_waits_for_a_completed_flush_line() {
        let (output_tx, output_rx) = mpsc::channel(4);
        let (_exit_tx, exit_rx) = watch::channel(None);
        let (metrics_stop, _metrics_rx) = watch::channel(false);
        output_tx
            .send("[Server thread/INFO]: Saving the game".into())
            .await
            .unwrap();
        output_tx
            .send("[Server thread/INFO]: Saved the game".into())
            .await
            .unwrap();
        let mut process = ManagedProcess {
            pid: 0,
            stdin: None,
            exit_rx,
            generation: 1,
            stop: StopStrategy::Terminate { timeout_seconds: 1 },
            output_rx: Some(output_rx),
            _metrics_stop: metrics_stop,
        };

        wait_for_minecraft_save(&mut process).await.unwrap();
        assert!(!minecraft_save_completed("Saving the game"));
        assert!(minecraft_save_completed("Saved 42 chunks"));
        assert!(minecraft_save_off_completed(
            "Automatic saving is now disabled"
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_crash_during_frozen_backup_preserves_the_lease() {
        let (_root, mut actor, _user_id) = runtime_actor_fixture("hytale", "running").await;
        let (exit_tx, exit_rx) = watch::channel(None);
        let (metrics_stop, _metrics_rx) = watch::channel(false);
        actor.backup_token = Some("backup-token".into());
        actor.process = Some(ManagedProcess {
            pid: 42,
            stdin: None,
            exit_rx,
            generation: 7,
            stop: StopStrategy::Terminate { timeout_seconds: 1 },
            output_rx: None,
            _metrics_stop: metrics_stop,
        });
        drop(exit_tx);

        actor
            .process_exited(
                7,
                ExitOutcome {
                    success: false,
                    code: Some(1),
                    elapsed: Duration::from_secs(1),
                },
            )
            .await;

        assert_eq!(actor.backup_token.as_deref(), Some("backup-token"));
        assert!(actor.backup_restart_after);
        assert!(actor.process.is_none());
        assert_eq!(actor.instance().await.unwrap().runtime_state, "crashed");
        actor.release_backup_lease(false).await.unwrap();
    }

    #[tokio::test]
    async fn preparing_update_recovers_an_interrupted_filesystem_switch() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "running").await;
        let job = update_job(&actor, &user_id).await;
        let instance = actor.instance().await.unwrap();
        let transaction = actor
            .begin_or_load_update_transaction(&job, &instance, true)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n-plus-one")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE instances SET installation_state = 'updating', installed_version = '2.0.0', installed_build = '200' WHERE id = ?",
        )
        .bind(&actor.instance_id)
        .execute(&actor.inner.pool)
        .await
        .unwrap();

        actor
            .recover_preparing_install_switch(&root, &job.id)
            .await
            .unwrap();
        actor
            .restore_update_snapshot(&transaction, None)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n"
        );
        assert!(tokio::fs::symlink_metadata(&rollback).await.is_err());
        let recovered = actor.instance().await.unwrap();
        assert_eq!(recovered.installation_state, "installed");
        assert_eq!(recovered.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(recovered.installed_build.as_deref(), Some("100"));
        assert_eq!(recovered.desired_state, "running");
    }

    #[tokio::test]
    async fn committed_update_rolls_back_tree_and_metadata_after_readiness_failure() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "running").await;
        let job = update_job(&actor, &user_id).await;
        sqlx::query("UPDATE instances SET settings = ?, config_version = 4 WHERE id = ?")
            .bind(serde_json::json!({"version": "1.0.0"}).to_string())
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, true)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"broken-version-n-plus-one")
            .await
            .unwrap();
        actor
            .mark_install_committed(
                Some("2.0.0"),
                Some("200"),
                Some(&serde_json::json!({"version": "2.0.0"})),
            )
            .await
            .unwrap();
        let transaction: UpdateTransaction = sqlx::query_as(
            "SELECT instance_id, job_id, previous_installation_state, previous_installed_version, \
             previous_installed_build, previous_settings, previous_config_version, \
             previous_desired_state, restart_after, phase \
             FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();

        actor
            .rollback_committed_install(&root, &transaction)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n"
        );
        let recovered = actor.instance().await.unwrap();
        assert_eq!(recovered.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(recovered.installed_build.as_deref(), Some("100"));
        let (settings, config_version): (String, i64) =
            sqlx::query_as("SELECT settings, config_version FROM instances WHERE id = ?")
                .bind(&actor.instance_id)
                .fetch_one(&actor.inner.pool)
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&settings).unwrap(),
            serde_json::json!({"version": "1.0.0"})
        );
        assert_eq!(config_version, 4);
        let phase: String = sqlx::query_scalar(
            "SELECT phase FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(phase, "rolled_back");
    }

    #[tokio::test]
    async fn install_success_and_update_transaction_finalize_in_one_database_commit() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, false)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n-plus-one")
            .await
            .unwrap();
        actor
            .mark_install_committed(Some("2.0.0"), Some("200"), None)
            .await
            .unwrap();

        actor.complete_install_job(&job.id).await.unwrap();

        assert_eq!(
            jobs::get(&actor.inner.pool, &job.id).await.unwrap().state,
            JobState::Succeeded
        );
        let transactions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(transactions, 0);
        assert!(tokio::fs::symlink_metadata(&rollback).await.is_err());
        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n-plus-one"
        );
        let succeeded_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ? AND event_type = 'job.succeeded'",
        )
        .bind(&job.id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(succeeded_events, 1);
    }

    #[tokio::test]
    async fn install_completion_cannot_overwrite_a_persisted_cancellation_intent() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, false)
            .await
            .unwrap()
            .unwrap();
        actor
            .mark_install_committed(Some("2.0.0"), Some("200"), None)
            .await
            .unwrap();
        assert!(
            jobs::request_install_cancel(&actor.inner.pool, &job.id, &actor.instance_id)
                .await
                .unwrap()
        );

        assert!(actor.complete_install_job(&job.id).await.is_err());
        assert_eq!(
            jobs::get(&actor.inner.pool, &job.id).await.unwrap().state,
            JobState::Running
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM instance_update_transactions WHERE instance_id = ?",
            )
            .bind(&actor.instance_id)
            .fetch_one(&actor.inner.pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn cancelling_waiting_update_restores_tree_and_preserves_current_stop_intent() {
        let (_temporary, mut actor, user_id) =
            runtime_actor_fixture("minecraft-bedrock", "running").await;
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, true)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n-plus-one")
            .await
            .unwrap();
        actor
            .mark_install_committed(Some("2.0.0"), Some("200"), None)
            .await
            .unwrap();
        sqlx::query("UPDATE instances SET desired_state = 'stopped' WHERE id = ?")
            .bind(&actor.instance_id)
            .execute(&actor.inner.pool)
            .await
            .unwrap();
        jobs::wait_for_user(
            &actor.inner.pool,
            &job.id,
            serde_json::json!({"job_id": job.id, "interaction": {"kind": "test"}}),
        )
        .await
        .unwrap();

        assert!(
            actor
                .abort_waiting_install(&job.id, WaitingInstallAbort::Cancelled)
                .await
                .unwrap()
        );

        let recovered = actor.instance().await.unwrap();
        assert_eq!(recovered.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(recovered.installed_build.as_deref(), Some("100"));
        assert_eq!(recovered.desired_state, "stopped");
        assert_eq!(recovered.runtime_state, "stopped");
        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n"
        );
        assert_eq!(
            jobs::get(&actor.inner.pool, &job.id).await.unwrap().state,
            JobState::Cancelled
        );
        let transactions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(transactions, 0);
    }

    #[tokio::test]
    async fn cancelling_waiting_initial_install_returns_to_not_installed() {
        let (_temporary, mut actor, user_id) =
            runtime_actor_fixture("minecraft-bedrock", "stopped").await;
        sqlx::query(
            "UPDATE instances SET installation_state = 'installing', installed_version = NULL, \
             installed_build = NULL WHERE id = ?",
        )
        .bind(&actor.instance_id)
        .execute(&actor.inner.pool)
        .await
        .unwrap();
        let root = actor.instance_root().await.unwrap();
        tokio::fs::create_dir_all(&root).await.unwrap();
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        jobs::wait_for_user(
            &actor.inner.pool,
            &job.id,
            serde_json::json!({"job_id": job.id, "interaction": {"kind": "test"}}),
        )
        .await
        .unwrap();

        assert!(
            actor
                .abort_waiting_install(&job.id, WaitingInstallAbort::Cancelled)
                .await
                .unwrap()
        );

        assert_eq!(
            actor.instance().await.unwrap().installation_state,
            "not_installed"
        );
        assert_eq!(
            jobs::get(&actor.inner.pool, &job.id).await.unwrap().state,
            JobState::Cancelled
        );
    }

    #[tokio::test]
    async fn cancellation_signal_is_bound_to_the_exact_install_job() {
        let (_temporary, actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let manager = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        actor.register_install_cancellation(&job.id).await;

        assert!(
            !manager
                .request_install_cancel(&actor.instance_id, &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap()
        );
        assert!(
            manager
                .request_install_cancel(&actor.instance_id, &job.id)
                .await
                .unwrap()
        );
        assert!(
            jobs::install_cancel_requested(&actor.inner.pool, &job.id)
                .await
                .unwrap()
        );
        assert!(actor.close_install_cancellation(&job.id).await);
    }

    #[tokio::test]
    async fn persisted_cancellation_survives_restart_and_rolls_back_a_committed_update() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, false)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n-plus-one")
            .await
            .unwrap();
        actor
            .mark_install_committed(Some("2.0.0"), Some("200"), None)
            .await
            .unwrap();
        actor.register_install_cancellation(&job.id).await;
        let manager = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };
        assert!(
            manager
                .request_install_cancel(&actor.instance_id, &job.id)
                .await
                .unwrap()
        );
        actor.inner.install_cancellations.lock().await.clear();

        database::run_migrations(&actor.inner.pool).await.unwrap();
        assert_eq!(
            jobs::get(&actor.inner.pool, &job.id).await.unwrap().state,
            JobState::Queued
        );
        manager.reconcile_boot().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if jobs::get(&actor.inner.pool, &job.id).await.unwrap().state == JobState::Cancelled
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n"
        );
        let recovered = actor.instance().await.unwrap();
        assert_eq!(recovered.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(recovered.installed_build.as_deref(), Some("100"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM instance_update_transactions WHERE instance_id = ?",
            )
            .bind(&actor.instance_id)
            .fetch_one(&actor.inner.pool)
            .await
            .unwrap(),
            0
        );
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn boot_reconciliation_rolls_back_terminal_update_before_any_autostart() {
        let (_temporary, mut actor, user_id) = runtime_actor_fixture("hytale", "stopped").await;
        let job = update_job(&actor, &user_id).await;
        assert!(jobs::begin(&actor.inner.pool, &job.id).await.unwrap());
        let instance = actor.instance().await.unwrap();
        actor
            .begin_or_load_update_transaction(&job, &instance, false)
            .await
            .unwrap()
            .unwrap();
        let root = actor.instance_root().await.unwrap();
        let game = root.join("game");
        let rollback = root.join(format!(".rollback-{}", job.id));
        tokio::fs::create_dir_all(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n")
            .await
            .unwrap();
        tokio::fs::rename(&game, &rollback).await.unwrap();
        tokio::fs::create_dir(&game).await.unwrap();
        tokio::fs::write(game.join("version.txt"), b"version-n-plus-one")
            .await
            .unwrap();
        actor
            .mark_install_committed(Some("2.0.0"), Some("200"), None)
            .await
            .unwrap();
        jobs::cancel(&actor.inner.pool, &job.id, "manager_restarted")
            .await
            .unwrap();
        let manager = RuntimeManager {
            inner: Arc::clone(&actor.inner),
        };

        manager.reconcile_update_transactions().await.unwrap();

        assert_eq!(
            tokio::fs::read(game.join("version.txt")).await.unwrap(),
            b"version-n"
        );
        let recovered: RuntimeInstance = sqlx::query_as(
            "SELECT id, profile_id, profile_revision, settings, config_version, installation_state, \
             installed_version, installed_build, desired_state, runtime_state, auto_start, \
             watchdog_enabled FROM instances WHERE id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(recovered.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(recovered.installed_build.as_deref(), Some("100"));
        let transactions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instance_update_transactions WHERE instance_id = ?",
        )
        .bind(&actor.instance_id)
        .fetch_one(&actor.inner.pool)
        .await
        .unwrap();
        assert_eq!(transactions, 0);
    }

    #[test]
    fn typed_arguments_only_expand_declared_values() {
        let settings = serde_json::json!({"game": 8211});
        let profile = SteamProfile {
            app_id: 2_394_010,
            branch: None,
            executable: crate::domain::v1::SteamExecutable {
                linux_x86_64: Some("server".into()),
                windows_x86_64: Some("server.exe".into()),
            },
            arguments: vec![],
            ports: vec![crate::domain::v1::PortSpec {
                name: "game".into(),
                protocol: crate::domain::v1::PortProtocol::Udp,
                default: 8_211,
                adjacent_to: None,
            }],
            save_paths: vec!["saves".into()],
            ready_log_pattern: Some("ready".into()),
            stop_strategy: SteamStopStrategy::Stdin {
                command: "quit".into(),
                timeout_seconds: 30,
            },
        };
        let root = Path::new("/managed/instance");
        assert_eq!(
            expand_steam_argument("{{port:game}}", &settings, &profile, root).unwrap(),
            "8211"
        );
        assert!(expand_steam_argument("{{env:PATH}}", &settings, &profile, root).is_err());
    }

    #[tokio::test]
    async fn readiness_waits_for_the_declared_log_pattern() {
        let (line_tx, mut line_rx) = mpsc::channel(4);
        let (_exit_tx, exit_rx) = watch::channel(None);
        line_tx.send("still loading".into()).await.unwrap();
        line_tx
            .send("Dedicated server Ready on port 27015".into())
            .await
            .unwrap();
        wait_until_ready(
            Regex::new(r"Ready on port [0-9]+").unwrap(),
            &mut line_rx,
            exit_rx,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn readiness_fails_if_the_process_exits_first() {
        let (_line_tx, mut line_rx) = mpsc::channel(1);
        let (exit_tx, exit_rx) = watch::channel(None);
        exit_tx
            .send(Some(ExitOutcome {
                success: false,
                code: Some(1),
                elapsed: Duration::from_millis(10),
            }))
            .unwrap();
        let error = wait_until_ready(
            Regex::new("Ready").unwrap(),
            &mut line_rx,
            exit_rx,
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "server_exited_before_ready");
    }

    #[tokio::test]
    async fn profiles_without_a_ready_pattern_must_survive_a_stability_window() {
        let (_exit_tx, exit_rx) = watch::channel(None);
        wait_for_process_stability(exit_rx, Duration::from_millis(10))
            .await
            .unwrap();

        let (exit_tx, exit_rx) = watch::channel(None);
        exit_tx
            .send(Some(ExitOutcome {
                success: false,
                code: Some(1),
                elapsed: Duration::from_millis(1),
            }))
            .unwrap();
        let error = wait_for_process_stability(exit_rx, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.code, "server_exited_before_ready");
    }

    #[test]
    fn ini_values_reject_line_injection() {
        assert!(ini_value("safe name").is_ok());
        assert!(ini_value("bad\nAdminPassword=oops").is_err());
    }

    #[test]
    fn palworld_settings_preserve_unknown_options_and_other_sections() {
        let existing = concat!(
            "[/Script/Pal.PalGameWorldSettings]\n",
            "OptionSettings=(Difficulty=Hard,ServerName=\"Old\",CustomText=\"a,b\",PublicPort=8000,ServerName=\"Duplicate\")\n",
            "[Other.Section]\nKeep=true\n",
        );
        let merged = merge_palworld_settings(existing, "New", "player", "admin", 8211).unwrap();
        assert!(merged.contains("Difficulty=Hard"));
        assert!(merged.contains("CustomText=\"a,b\""));
        assert!(merged.contains("[Other.Section]\nKeep=true"));
        assert!(merged.contains("ServerName=\"New\""));
        assert!(merged.contains("ServerPassword=\"player\""));
        assert!(merged.contains("AdminPassword=\"admin\""));
        assert!(merged.contains("PublicPort=8211"));
        assert_eq!(merged.matches("ServerName=").count(), 1);
        assert!(!merged.contains("ServerName=\"Old\""));
    }

    #[test]
    fn palworld_settings_reject_malformed_existing_options() {
        assert!(
            merge_palworld_settings(
                "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(Name=\"unterminated)\n",
                "Server",
                "player",
                "admin",
                8211,
            )
            .is_err()
        );
        let generated = merge_palworld_settings("", "Server", "", "", 8211).unwrap();
        assert!(generated.starts_with("[/Script/Pal.PalGameWorldSettings]\nOptionSettings=("));
    }

    #[test]
    fn project_zomboid_settings_preserve_native_options() {
        let updates = [
            ("DefaultPort", "16261".to_string()),
            ("PauseEmpty", "true".to_string()),
        ];
        let merged = merge_ini_settings(
            "# native comment\nDefaultPort=1\nPVP=false\nDefaultPort=2\n",
            &updates,
        );
        assert!(merged.contains("# native comment\n"));
        assert!(merged.contains("PVP=false\n"));
        assert!(merged.contains("DefaultPort=16261\n"));
        assert!(merged.contains("PauseEmpty=true\n"));
        assert_eq!(merged.matches("DefaultPort=").count(), 1);
    }

    #[test]
    fn seven_days_settings_preserve_unknown_properties() {
        let updates = [
            ("ServerPort", "26900".to_string()),
            ("TelnetEnabled", "false".to_string()),
        ];
        let existing = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<ServerSettings>\n",
            "  <!-- preserve me -->\n",
            "  <property name=\"ServerPort\" value=\"1\" />\n",
            "  <property name=\"CustomNativeOption\" value=\"yes\" />\n",
            "  <property name=\"ServerPort\" value=\"2\" />\n",
            "</ServerSettings>\n",
        );
        let merged = merge_seven_days_settings(existing, &updates).unwrap();
        assert!(merged.contains("<!-- preserve me -->"));
        assert!(merged.contains("name=\"CustomNativeOption\" value=\"yes\""));
        assert!(merged.contains("name=\"ServerPort\" value=\"26900\""));
        assert!(merged.contains("name=\"TelnetEnabled\" value=\"false\""));
        assert_eq!(merged.matches("name=\"ServerPort\"").count(), 1);
        assert!(merge_seven_days_settings("<broken>", &updates).is_err());
    }

    #[test]
    fn java_probe_parses_legacy_and_modern_versions() {
        assert_eq!(parse_java_major("java version \"1.8.0_402\""), Some(8));
        assert_eq!(
            parse_java_major("openjdk version \"21.0.7\" 2025-04-15"),
            Some(21)
        );
        assert_eq!(parse_java_major("openjdk 25.0.1 2025-10-21 LTS"), Some(25));
        assert_eq!(parse_java_major("untrusted output"), None);
    }

    #[tokio::test]
    async fn steam_build_manifest_is_bounded_and_parsed_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(root.path().join("steamapps"))
            .await
            .unwrap();
        tokio::fs::write(
            root.path().join("steamapps/appmanifest_896660.acf"),
            b"\"AppState\"\n{\n  \"buildid\"  \"123456\"\n}\n",
        )
        .await
        .unwrap();
        assert_eq!(
            read_steam_build_id(root.path(), None, 896_660)
                .await
                .unwrap()
                .as_deref(),
            Some("123456")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_process_accepts_console_and_stops_its_process_group() {
        let mut command = Command::new("/bin/cat");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_process_group(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take();
        let (exit_tx, exit_rx) = watch::channel(None);
        tokio::spawn(async move {
            let status = child.wait().await.unwrap();
            let _ = exit_tx.send(Some(ExitOutcome {
                success: status.success(),
                code: status.code(),
                elapsed: Duration::ZERO,
            }));
        });
        let (metrics_stop, _metrics_rx) = watch::channel(false);
        let mut process = ManagedProcess {
            pid,
            stdin,
            exit_rx,
            generation: 1,
            stop: StopStrategy::Terminate { timeout_seconds: 2 },
            output_rx: None,
            _metrics_stop: metrics_stop,
        };
        process.write_stdin("status").await.unwrap();
        let mut echoed = [0_u8; 7];
        tokio::time::timeout(Duration::from_secs(2), stdout.read_exact(&mut echoed))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&echoed, b"status\n");
        process.graceful_stop().await.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_stop_strategies_map_to_explicit_os_actions() {
        assert_eq!(
            windows_stop_action(StopSignal::Interrupt),
            WindowsStopAction::CtrlBreak
        );
        assert_eq!(
            windows_stop_action(StopSignal::Terminate),
            WindowsStopAction::TerminateJob
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_child_is_contained_before_spawn_returns() {
        let mut command = Command::new("ping.exe");
        command
            .args(["-n", "5", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let spawned = spawn_contained(&mut command).unwrap();
        let mut child = spawned.child;
        let windows_job = spawned.windows_job;
        let pid = child.id().unwrap();
        assert!(windows_job.contains_child(&child).unwrap());

        send_group_signal(pid, StopSignal::Terminate, Some(windows_job.handle)).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!status.success());
    }

    #[cfg(windows)]
    unsafe extern "system" fn windows_ctrl_break_test_handler(ctrl_type: u32) -> i32 {
        use windows_sys::Win32::System::{Console::CTRL_BREAK_EVENT, Threading::ExitProcess};

        if ctrl_type == CTRL_BREAK_EVENT {
            // Avoid Rust allocation or stdio from the control-handler thread;
            // exit code 99 proves to the parent that CTRL_BREAK arrived.
            unsafe { ExitProcess(99) }
        }
        0
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "helper process launched by windows_ctrl_break_works_without_an_inherited_console"]
    fn windows_ctrl_break_server_child() {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

        if !matches!(
            std::env::var("DMX_WINDOWS_TEST_ROLE"),
            Ok(role) if role == "server"
        ) {
            return;
        }
        let marker = std::env::var_os("DMX_WINDOWS_TEST_MARKER")
            .map(PathBuf::from)
            .expect("server helper requires its readiness marker");
        assert_ne!(
            unsafe { SetConsoleCtrlHandler(Some(windows_ctrl_break_test_handler), 1) },
            0,
            "failed to install CTRL_BREAK test handler: {}",
            std::io::Error::last_os_error()
        );
        std::fs::write(marker, b"ready").unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[cfg(windows)]
    async fn wait_for_windows_test_marker(path: &Path) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if tokio::fs::try_exists(path).await.unwrap_or(false) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("Windows test server did not become ready");
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "helper process launched by windows_ctrl_break_works_without_an_inherited_console"]
    async fn windows_detached_console_manager_child() {
        use windows_sys::Win32::System::Console::GetConsoleProcessList;

        if !matches!(
            std::env::var("DMX_WINDOWS_TEST_ROLE"),
            Ok(role) if role == "manager"
        ) {
            return;
        }

        // DETACHED_PROCESS models the relevant Windows Service Control Manager
        // property: the service starts without an inherited console.
        let mut console_process_id = 0_u32;
        assert_eq!(
            unsafe { GetConsoleProcessList(&mut console_process_id, 1) },
            0,
            "detached manager unexpectedly inherited a console"
        );

        let marker = std::env::var_os("DMX_WINDOWS_TEST_MARKER")
            .map(PathBuf::from)
            .expect("manager helper requires its readiness marker");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("windows_ctrl_break_server_child")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("DMX_WINDOWS_TEST_ROLE", "server")
            .env("DMX_WINDOWS_TEST_MARKER", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let spawned = spawn_contained(&mut command).unwrap();
        let mut child = spawned.child;
        let windows_job = spawned.windows_job;
        let pid = child.id().unwrap();
        assert!(windows_job.contains_child(&child).unwrap());
        wait_for_windows_test_marker(&marker).await;

        send_group_signal(pid, StopSignal::Interrupt, Some(windows_job.handle)).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("CTRL_BREAK did not stop the test server")
            .unwrap();
        assert_eq!(
            status.code(),
            Some(99),
            "test server did not exit through its CTRL_BREAK handler"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_ctrl_break_works_without_an_inherited_console() {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::DETACHED_PROCESS;

        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("server-ready");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("windows_detached_console_manager_child")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("DMX_WINDOWS_TEST_ROLE", "manager")
            .env("DMX_WINDOWS_TEST_MARKER", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .as_std_mut()
            .creation_flags(DETACHED_PROCESS);
        let mut manager = command.spawn().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(20), manager.wait())
            .await
            .expect("detached Windows manager helper timed out")
            .unwrap();
        assert!(status.success(), "detached manager helper failed: {status}");
    }

    #[tokio::test]
    async fn log_pipeline_bounds_lines_and_redacts_secrets() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("console.log");
        let (mut writer, reader) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(pump_output_observed(
            reader,
            OutputPumpConfig {
                log_path: log.clone(),
                combined_log: None,
                stream: "stdout",
                instance_id: uuid::Uuid::new_v4().to_string(),
                events: EventHub::new(8),
                redactions: vec!["top-secret".into()],
                observer: None,
                player_observer: None,
                public_log_policy: PublicLogPolicy::Normal,
            },
        ));
        writer
            .write_all(format!("password=top-secret {}\n", "x".repeat(20_000)).as_bytes())
            .await
            .unwrap();
        drop(writer);
        task.await.unwrap();
        let contents = tokio::fs::read_to_string(log).await.unwrap();
        assert!(!contents.contains("top-secret"));
        assert!(contents.contains("[REDACTED]"));
        assert!(contents.contains("[truncated]"));
        assert!(contents.len() < MAX_LOG_LINE + 128);
    }

    #[tokio::test]
    async fn hytale_device_authorization_keeps_real_log_context_without_exposing_secrets() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("install.log");
        let combined = root.path().join("install.combined.log");
        let events = EventHub::new(8);
        let mut receiver = events.subscribe();
        let (line_tx, mut line_rx) = mpsc::channel(4);
        let (mut writer, reader) = tokio::io::duplex(4 * 1024);
        let task = tokio::spawn(pump_output_observed(
            reader,
            OutputPumpConfig {
                log_path: log.clone(),
                combined_log: Some(Arc::new(Mutex::new(
                    RotatingLog::open(combined.clone()).await.unwrap(),
                ))),
                stream: "install",
                instance_id: uuid::Uuid::new_v4().to_string(),
                events,
                redactions: Vec::new(),
                observer: Some(line_tx),
                player_observer: None,
                public_log_policy: PublicLogPolicy::HytaleDeviceFlow,
            },
        ));
        let sensitive = "Please visit the following URL to authenticate:\n\
             https://accounts.hytale.com/device?device_challenge=challenge_123&user_code=ABCD-1234\n\
             Authorization code: ABCD-1234\n\
             Waiting for authorization...\n";
        writer.write_all(sensitive.as_bytes()).await.unwrap();
        drop(writer);
        task.await.unwrap();

        let internal = [
            line_rx.recv().await.unwrap(),
            line_rx.recv().await.unwrap(),
            line_rx.recv().await.unwrap(),
            line_rx.recv().await.unwrap(),
        ]
        .join("\n");
        assert!(internal.contains("ABCD-1234"));
        let contents = tokio::fs::read_to_string(log).await.unwrap();
        assert!(!contents.contains("ABCD-1234"));
        assert!(!contents.contains("challenge_123"));
        assert!(contents.contains("Please visit the following URL to authenticate:"));
        assert!(contents.contains("https://accounts.hytale.com/device?[REDACTED]"));
        assert!(contents.contains("Authorization code: [REDACTED — use the secure action card]"));
        assert!(contents.contains("Waiting for authorization..."));
        let combined_contents = tokio::fs::read_to_string(combined).await.unwrap();
        assert!(!combined_contents.contains("ABCD-1234"));
        assert!(!combined_contents.contains("challenge_123"));
        assert!(combined_contents.contains("Please visit the following URL to authenticate:"));
        assert!(combined_contents.contains("Waiting for authorization..."));
        for _ in 0..4 {
            let event = receiver.recv().await.unwrap();
            let message = event.payload["message"].as_str().unwrap();
            assert!(!message.contains("ABCD-1234"));
            assert!(!message.contains("challenge_123"));
        }
    }

    #[test]
    fn hytale_log_sanitizer_redacts_a_standalone_code_after_the_downloader_url() {
        let mut sanitizer = HytaleDeviceLogSanitizer::default();
        assert_eq!(
            sanitizer.sanitize("https://oauth.accounts.hytale.com/oauth2/device/verify"),
            "https://oauth.accounts.hytale.com/oauth2/device/verify"
        );
        assert_eq!(
            sanitizer.sanitize("CIGWERCQ"),
            "[REDACTED — use the secure action card]"
        );
        assert_eq!(
            sanitizer.sanitize("Token request failed: context deadline exceeded"),
            "Token request failed: context deadline exceeded"
        );
        assert_eq!(
            sanitizer.sanitize("Process exit code: DEAD"),
            "Process exit code: DEAD"
        );
        assert_eq!(
            HytaleDeviceLogSanitizer::default().sanitize("Code: CIGWERCQ"),
            "Code: [REDACTED — use the secure action card]"
        );
    }

    #[test]
    fn hytale_diagnostics_explain_the_flow_without_exposing_the_code() {
        let authorization = installers::hytale::DeviceAuthorization {
            verification_uri:
                "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=x6nimECK"
                    .to_string(),
            user_code: Some("x6nimECK".to_string()),
        };
        let diagnostic = hytale_device_request_diagnostic(&authorization, 2);
        assert!(diagnostic.contains("request #2"));
        assert!(diagnostic.contains("flow=downloader"));
        assert!(diagnostic.contains("verification_uri_complete=yes"));
        assert!(diagnostic.contains("code_length=8"));
        assert!(!diagnostic.contains("x6nimECK"));

        let timeout = hytale_downloader_failure_diagnostic(
            "error obtaining token: context deadline exceeded",
        )
        .unwrap();
        assert!(timeout.contains("classification=oauth-device-timeout"));
        assert!(hytale_downloader_failure_diagnostic("unknown provider failure").is_none());
    }

    #[tokio::test]
    async fn hytale_credential_diagnostic_reports_metadata_only() {
        let root = tempfile::tempdir().unwrap();
        let credentials = root.path().join("credentials.json");
        assert_eq!(hytale_credential_file_state(&credentials).await, "absent");
        tokio::fs::write(&credentials, b"secret-value")
            .await
            .unwrap();
        assert_eq!(
            hytale_credential_file_state(&credentials).await,
            "present-12-bytes"
        );
    }

    #[tokio::test]
    async fn hytale_native_workdir_is_private_and_cleared_between_starts() {
        let root = tempfile::tempdir().unwrap();
        let native = prepare_hytale_native_workdir(root.path()).await.unwrap();
        assert_eq!(native, root.path().join(".dmx-runtime/hytale-native"));
        tokio::fs::write(native.join("stale-library.so"), b"stale")
            .await
            .unwrap();

        let recreated = prepare_hytale_native_workdir(root.path()).await.unwrap();
        assert_eq!(recreated, native);
        assert!(
            !tokio::fs::try_exists(recreated.join("stale-library.so"))
                .await
                .unwrap()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                tokio::fs::metadata(&recreated)
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn hytale_native_java_properties_resolve_to_the_absolute_private_directory() {
        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join(".dmx-runtime/hytale-native");
        for property in [
            "-Djava.io.tmpdir",
            "-Djansi.tmpdir",
            "-Dio.netty.native.workdir",
        ] {
            let argument = format!("{property}={}", installers::hytale::NATIVE_WORKDIR_RELATIVE);
            let resolved = native_launch_argument(argument, Some(&workdir)).unwrap();
            let mut expected = OsString::from(format!("{property}="));
            expected.push(workdir.as_os_str());
            assert_eq!(resolved, expected);
        }
        assert_eq!(
            native_launch_argument("--bind".into(), Some(&workdir)).unwrap(),
            OsString::from("--bind")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hytale_native_workdir_rejects_a_linked_runtime_parent() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(".dmx-runtime")).unwrap();

        let error = prepare_hytale_native_workdir(root.path())
            .await
            .unwrap_err();
        assert_eq!(error.code, "hytale_native_workdir_unsafe");
    }

    #[tokio::test]
    async fn installer_prompt_without_newline_is_flushed_while_process_keeps_running() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("install.log");
        let (line_tx, mut line_rx) = mpsc::channel(4);
        let (mut writer, reader) = tokio::io::duplex(4 * 1024);
        let task = tokio::spawn(pump_output_observed(
            reader,
            OutputPumpConfig {
                log_path: log,
                combined_log: None,
                stream: "install",
                instance_id: uuid::Uuid::new_v4().to_string(),
                events: EventHub::new(8),
                redactions: Vec::new(),
                observer: Some(line_tx),
                player_observer: None,
                public_log_policy: PublicLogPolicy::HytaleDeviceFlow,
            },
        ));
        writer
            .write_all(b"Authorization code: ABCD-1234")
            .await
            .unwrap();
        let observed = tokio::time::timeout(Duration::from_secs(2), line_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed, "Authorization code: ABCD-1234");
        drop(writer);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn palworld_updates_preserve_saved_worlds() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let staging = root.path().join("staging");
        tokio::fs::create_dir_all(game.join("Pal/Saved/SaveGames"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(staging.join("Pal/Saved"))
            .await
            .unwrap();
        tokio::fs::write(game.join("Pal/Saved/SaveGames/world.sav"), b"world")
            .await
            .unwrap();
        tokio::fs::write(staging.join("Pal/Saved/default.txt"), b"replace")
            .await
            .unwrap();
        let instance = RuntimeInstance {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: "palworld".into(),
            profile_revision: 1,
            settings: "{}".into(),
            config_version: 1,
            installation_state: "installed".into(),
            installed_version: None,
            installed_build: None,
            desired_state: "stopped".into(),
            runtime_state: "stopped".into(),
            auto_start: false,
            watchdog_enabled: true,
        };
        preserve_instance_data(&instance, None, &game, &staging)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(staging.join("Pal/Saved/SaveGames/world.sav"))
                .await
                .unwrap(),
            b"world"
        );
        assert!(
            !tokio::fs::try_exists(staging.join("Pal/Saved/default.txt"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn custom_steam_updates_preserve_only_profile_declared_saves() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let staging = root.path().join("staging");
        tokio::fs::create_dir_all(game.join("worlds/live"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(game.join("private"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(&staging).await.unwrap();
        tokio::fs::write(game.join("worlds/live/world.db"), b"world")
            .await
            .unwrap();
        tokio::fs::write(game.join("private/secret.txt"), b"secret")
            .await
            .unwrap();
        let profile = SteamProfile {
            app_id: 42,
            branch: None,
            executable: crate::domain::v1::SteamExecutable {
                linux_x86_64: Some("server".into()),
                windows_x86_64: Some("server.exe".into()),
            },
            arguments: vec![],
            ports: vec![],
            save_paths: vec!["worlds".into()],
            ready_log_pattern: None,
            stop_strategy: SteamStopStrategy::Terminate {
                timeout_seconds: 30,
            },
        };
        let instance = RuntimeInstance {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: "steam-fixture".into(),
            profile_revision: 1,
            settings: "{}".into(),
            config_version: 1,
            installation_state: "installed".into(),
            installed_version: None,
            installed_build: None,
            desired_state: "stopped".into(),
            runtime_state: "stopped".into(),
            auto_start: false,
            watchdog_enabled: true,
        };
        preserve_instance_data(&instance, Some(&profile), &game, &staging)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(staging.join("worlds/live/world.db"))
                .await
                .unwrap(),
            b"world"
        );
        assert!(
            !tokio::fs::try_exists(staging.join("private"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn hytale_exit_code_eight_update_is_atomic_and_rollback_preserves_world_changes() {
        assert_eq!(HYTALE_UPDATE_EXIT_CODE, 8);
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let server = game.join("Server");
        let provider = game.join("updater/staging");
        tokio::fs::create_dir_all(server.join("universe"))
            .await
            .unwrap();
        tokio::fs::write(game.join("Assets.zip"), b"old-assets")
            .await
            .unwrap();
        tokio::fs::write(server.join("HytaleServer.jar"), b"old-jar")
            .await
            .unwrap();
        tokio::fs::write(server.join("HytaleServer.aot"), b"old-aot")
            .await
            .unwrap();
        tokio::fs::write(server.join("universe/world.bin"), b"world-before")
            .await
            .unwrap();
        tokio::fs::write(game.join(".dmx-install.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::create_dir_all(provider.join("Server"))
            .await
            .unwrap();
        tokio::fs::write(provider.join("Assets.zip"), b"new-assets")
            .await
            .unwrap();
        tokio::fs::write(provider.join("Server/HytaleServer.jar"), b"new-jar")
            .await
            .unwrap();
        tokio::fs::write(provider.join("Server/HytaleServer.aot"), b"new-aot")
            .await
            .unwrap();

        apply_hytale_staged_update(root.path(), Some("old-version".into()), None)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(game.join("Server/HytaleServer.jar"))
                .await
                .unwrap(),
            b"new-jar"
        );
        assert_eq!(
            read_hytale_update_state(root.path())
                .await
                .unwrap()
                .unwrap()
                .phase,
            HytaleUpdatePhase::Applied
        );
        tokio::fs::write(game.join("Server/universe/world.bin"), b"world-after")
            .await
            .unwrap();

        rollback_hytale_update(root.path()).await.unwrap();
        assert_eq!(
            tokio::fs::read(game.join("Server/HytaleServer.jar"))
                .await
                .unwrap(),
            b"old-jar"
        );
        assert_eq!(
            tokio::fs::read(game.join("Server/universe/world.bin"))
                .await
                .unwrap(),
            b"world-after"
        );
        assert!(
            read_hytale_update_state(root.path())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn hytale_update_recovers_a_switch_interrupted_between_renames() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let candidate = root.path().join(HYTALE_UPDATE_CANDIDATE);
        for (directory, marker) in [(&game, b"old".as_slice()), (&candidate, b"new".as_slice())] {
            tokio::fs::create_dir_all(directory.join("Server"))
                .await
                .unwrap();
            tokio::fs::write(directory.join("Assets.zip"), marker)
                .await
                .unwrap();
            tokio::fs::write(directory.join("Server/HytaleServer.jar"), marker)
                .await
                .unwrap();
            tokio::fs::write(directory.join("Server/HytaleServer.aot"), marker)
                .await
                .unwrap();
        }
        write_hytale_update_state(
            root.path(),
            &HytaleUpdateState::new(
                HytaleUpdatePhase::Prepared,
                Some("old-version".into()),
                None,
            ),
        )
        .await
        .unwrap();
        tokio::fs::rename(&game, root.path().join(HYTALE_UPDATE_ROLLBACK))
            .await
            .unwrap();

        assert!(recover_hytale_update_state(root.path()).await.unwrap());
        assert_eq!(
            tokio::fs::read(game.join("Server/HytaleServer.jar"))
                .await
                .unwrap(),
            b"new"
        );
        assert_eq!(
            read_hytale_update_state(root.path())
                .await
                .unwrap()
                .unwrap()
                .phase,
            HytaleUpdatePhase::Applied
        );
    }
}
