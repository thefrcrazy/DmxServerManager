use std::{str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

use crate::{
    core::{AppState, DbPool, database, error::AppError},
    domain::v1::Job,
    services::{
        backups, jobs,
        runtime::{RuntimeAction, validate_console_command},
    },
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_DUE_PER_TICK: usize = 32;
const MIN_INTERVAL_SECONDS: u64 = 60;
const MAX_INTERVAL_SECONDS: u64 = 31_536_000;
const RUN_RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleTrigger {
    Cron {
        expression: String,
        timezone: String,
    },
    Interval {
        seconds: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleAction {
    Start {},
    Stop {},
    Restart {},
    Backup {},
    Update {},
    Console { command: String },
}

impl ScheduleAction {
    pub fn required_permissions(&self) -> &'static [&'static str] {
        match self {
            Self::Start {} => &["server.start"],
            Self::Stop {} => &["server.stop"],
            Self::Restart {} => &["server.start", "server.stop"],
            Self::Backup {} => &["server.backup"],
            Self::Update {} => &["server.update_game"],
            Self::Console { .. } => &["server.console.write"],
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Start {} => "start",
            Self::Stop {} => "stop",
            Self::Restart {} => "restart",
            Self::Backup {} => "backup",
            Self::Update {} => "update",
            Self::Console { .. } => "console",
        }
    }

    fn payload(&self) -> serde_json::Value {
        match self {
            Self::Console { command } => serde_json::json!({"command": command}),
            _ => serde_json::json!({}),
        }
    }

    fn validate(&self) -> Result<(), AppError> {
        if let Self::Console { command } = self {
            validate_console_command(command)?;
        }
        Ok(())
    }

    fn from_storage(kind: &str, payload: &str) -> Result<Self, AppError> {
        let payload: serde_json::Value = serde_json::from_str(payload)
            .map_err(|_| AppError::Internal("invalid stored schedule action payload".into()))?;
        if !payload.is_object() {
            return Err(AppError::Internal(
                "invalid stored schedule action payload".into(),
            ));
        }
        match kind {
            "start" if payload.as_object().is_some_and(serde_json::Map::is_empty) => {
                Ok(Self::Start {})
            }
            "stop" if payload.as_object().is_some_and(serde_json::Map::is_empty) => {
                Ok(Self::Stop {})
            }
            "restart" if payload.as_object().is_some_and(serde_json::Map::is_empty) => {
                Ok(Self::Restart {})
            }
            "backup" if payload.as_object().is_some_and(serde_json::Map::is_empty) => {
                Ok(Self::Backup {})
            }
            "update" if payload.as_object().is_some_and(serde_json::Map::is_empty) => {
                Ok(Self::Update {})
            }
            "console" => {
                let object = payload.as_object().expect("checked above");
                if object.len() != 1 {
                    return Err(AppError::Internal(
                        "invalid stored schedule console payload".into(),
                    ));
                }
                let command = object
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        AppError::Internal("invalid stored schedule console payload".into())
                    })?
                    .to_string();
                let action = Self::Console { command };
                action.validate()?;
                Ok(action)
            }
            _ => Err(AppError::Internal("invalid stored schedule action".into())),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Schedule {
    pub id: String,
    pub instance_id: String,
    pub name: String,
    pub trigger: ScheduleTrigger,
    pub action: ScheduleAction,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_job_id: Option<String>,
    pub version: u32,
    pub created_by: String,
    pub requested_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ScheduleSpec {
    pub name: String,
    pub trigger: ScheduleTrigger,
    pub action: ScheduleAction,
    pub enabled: bool,
}

#[derive(Debug, FromRow)]
struct ScheduleRow {
    id: String,
    instance_id: String,
    name: String,
    trigger_kind: String,
    cron_expression: Option<String>,
    interval_seconds: Option<i64>,
    timezone: String,
    action_kind: String,
    action_payload: String,
    enabled: bool,
    next_run_at: Option<String>,
    last_run_at: Option<String>,
    last_job_id: Option<String>,
    version: i64,
    created_by: String,
    requested_by: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, FromRow)]
struct ClaimedRunRow {
    id: String,
    schedule_id: String,
    instance_id: String,
    scheduled_for: String,
    action_kind: String,
    action_payload: String,
    requested_by: String,
}

#[derive(Debug, Clone)]
struct ClaimedRun {
    id: String,
    schedule_id: String,
    instance_id: String,
    scheduled_for: String,
    action: ScheduleAction,
    requested_by: String,
}

#[derive(Debug)]
struct NormalizedTrigger {
    kind: &'static str,
    cron_expression: Option<String>,
    interval_seconds: Option<i64>,
    timezone: String,
    value: ScheduleTrigger,
}

pub async fn list(pool: &DbPool, instance_id: &str) -> Result<Vec<Schedule>, AppError> {
    let rows: Vec<ScheduleRow> =
        sqlx::query_as("SELECT * FROM schedules WHERE instance_id = ? ORDER BY created_at DESC")
            .bind(instance_id)
            .fetch_all(pool)
            .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn get(pool: &DbPool, id: &str) -> Result<Schedule, AppError> {
    schedule_row(pool, id).await?.try_into()
}

pub async fn create(
    pool: &DbPool,
    instance_id: &str,
    spec: ScheduleSpec,
    actor_id: &str,
) -> Result<Schedule, AppError> {
    let name = normalize_name(&spec.name)?;
    let trigger = normalize_trigger(spec.trigger)?;
    spec.action.validate()?;
    let now = Utc::now();
    let next_run_at = if spec.enabled {
        Some(next_run(&trigger.value, now)?.to_rfc3339())
    } else {
        None
    };
    let id = Uuid::new_v4().to_string();
    let now = now.to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO schedules
            (id, instance_id, name, trigger_kind, cron_expression, interval_seconds,
             timezone, action_kind, action_payload, enabled, next_run_at, version,
             created_by, requested_by, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(instance_id)
    .bind(name)
    .bind(trigger.kind)
    .bind(trigger.cron_expression)
    .bind(trigger.interval_seconds)
    .bind(trigger.timezone)
    .bind(spec.action.kind())
    .bind(spec.action.payload().to_string())
    .bind(spec.enabled)
    .bind(next_run_at)
    .bind(actor_id)
    .bind(actor_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    get(pool, &id).await
}

pub async fn update(
    pool: &DbPool,
    id: &str,
    expected_version: u32,
    spec: ScheduleSpec,
    actor_id: &str,
) -> Result<Schedule, AppError> {
    let name = normalize_name(&spec.name)?;
    let trigger = normalize_trigger(spec.trigger)?;
    spec.action.validate()?;
    let now = Utc::now();
    let next_run_at = if spec.enabled {
        Some(next_run(&trigger.value, now)?.to_rfc3339())
    } else {
        None
    };
    let now = now.to_rfc3339();
    let result = sqlx::query(
        r#"
        UPDATE schedules SET
            name = ?, trigger_kind = ?, cron_expression = ?, interval_seconds = ?,
            timezone = ?, action_kind = ?, action_payload = ?, enabled = ?,
            next_run_at = ?, requested_by = ?, version = version + 1, updated_at = ?
        WHERE id = ? AND version = ?
        "#,
    )
    .bind(name)
    .bind(trigger.kind)
    .bind(trigger.cron_expression)
    .bind(trigger.interval_seconds)
    .bind(trigger.timezone)
    .bind(spec.action.kind())
    .bind(spec.action.payload().to_string())
    .bind(spec.enabled)
    .bind(next_run_at)
    .bind(actor_id)
    .bind(now)
    .bind(id)
    .bind(i64::from(expected_version))
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schedules WHERE id = ?)")
                .bind(id)
                .fetch_one(pool)
                .await?;
        return Err(if exists {
            AppError::Conflict("schedules.version_conflict".into())
        } else {
            AppError::NotFound("schedules.not_found".into())
        });
    }
    get(pool, id).await
}

pub async fn remove(pool: &DbPool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM schedules WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("schedules.not_found".into()));
    }
    Ok(())
}

pub struct SchedulerService {
    shutdown: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SchedulerService {
    pub fn start(state: AppState) -> Self {
        let (shutdown, receiver) = watch::channel(false);
        let task = tokio::spawn(run_scheduler(state, receiver));
        Self {
            shutdown,
            task: Some(task),
        }
    }

    pub async fn shutdown(&mut self) {
        let _ = self.shutdown.send(true);
        let Some(mut task) = self.task.take() else {
            return;
        };
        if tokio::time::timeout(Duration::from_secs(10), &mut task)
            .await
            .is_err()
        {
            tracing::warn!("scheduler shutdown timed out");
            task.abort();
            let _ = task.await;
        }
    }
}

async fn run_scheduler(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut executions = JoinSet::new();
    match claimed_runs(&state.pool).await {
        Ok(runs) => {
            for run in runs {
                spawn_execution(&mut executions, state.clone(), run);
            }
        }
        Err(error) => tracing::error!(%error, "failed to recover claimed schedule runs"),
    }

    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut maintenance = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = ticker.tick() => {
                for _ in 0..MAX_DUE_PER_TICK {
                    match claim_next_due(&state.pool, Utc::now()).await {
                        Ok(Some(run)) => spawn_execution(&mut executions, state.clone(), run),
                        Ok(None) => break,
                        Err(error) => {
                            tracing::error!(%error, "failed to claim a due schedule");
                            break;
                        }
                    }
                }
            }
            _ = maintenance.tick() => {
                if let Err(error) = prune_finished_runs(&state.pool).await {
                    tracing::warn!(%error, "failed to prune old schedule run metadata");
                }
            }
            result = executions.join_next(), if !executions.is_empty() => {
                if let Some(Err(error)) = result {
                    tracing::error!(%error, "scheduled execution task panicked");
                }
            }
        }
    }

    if tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(result) = executions.join_next().await {
            if let Err(error) = result {
                tracing::error!(%error, "scheduled execution task panicked during shutdown");
            }
        }
    })
    .await
    .is_err()
    {
        executions.abort_all();
    }
}

async fn prune_finished_runs(pool: &DbPool) -> Result<(), AppError> {
    let cutoff = (Utc::now() - chrono::Duration::days(RUN_RETENTION_DAYS)).to_rfc3339();
    sqlx::query(
        "DELETE FROM schedule_runs WHERE status IN ('submitted', 'failed') AND finished_at < ?",
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(())
}

fn spawn_execution(executions: &mut JoinSet<()>, state: AppState, run: ClaimedRun) {
    executions.spawn(async move {
        execute_claimed(&state, run).await;
    });
}

async fn execute_claimed(state: &AppState, run: ClaimedRun) {
    let result = dispatch(state, &run).await;
    match result {
        Ok(job) => {
            if let Err(error) = complete_run(&state.pool, &run, &job).await {
                tracing::error!(run_id = %run.id, %error, "failed to complete schedule run");
                return;
            }
            let _ = database::audit(
                &state.pool,
                Some(&run.requested_by),
                "schedule.triggered",
                "schedule",
                Some(&run.schedule_id),
                "success",
                serde_json::json!({
                    "instance_id": run.instance_id,
                    "scheduled_for": run.scheduled_for,
                    "action": run.action.kind(),
                    "job_id": job.id,
                }),
            )
            .await;
            state.events.publish(
                "schedule.triggered",
                Some(run.instance_id),
                serde_json::json!({
                    "schedule_id": run.schedule_id,
                    "scheduled_for": run.scheduled_for,
                    "action": run.action.kind(),
                    "job_id": job.id,
                }),
            );
        }
        Err(error) => {
            let code = schedule_error_code(&error);
            if let Err(update_error) = fail_run(&state.pool, &run.id, code).await {
                tracing::error!(run_id = %run.id, %update_error, "failed to mark schedule run failed");
            }
            tracing::warn!(schedule_id = %run.schedule_id, run_id = %run.id, code, %error, "scheduled action rejected");
            let _ = database::audit(
                &state.pool,
                Some(&run.requested_by),
                "schedule.triggered",
                "schedule",
                Some(&run.schedule_id),
                "failure",
                serde_json::json!({
                    "instance_id": run.instance_id,
                    "scheduled_for": run.scheduled_for,
                    "action": run.action.kind(),
                    "error_code": code,
                }),
            )
            .await;
        }
    }
}

async fn dispatch(state: &AppState, run: &ClaimedRun) -> Result<Job, AppError> {
    for permission in run.action.required_permissions() {
        ensure_execution_authorized(&state.pool, &run.requested_by, &run.instance_id, permission)
            .await?;
    }
    ensure_instance_exists(&state.pool, &run.instance_id).await?;
    let idempotency_key = format!("schedule:{}:{}", run.schedule_id, run.scheduled_for);

    match &run.action {
        ScheduleAction::Backup {} => {
            let (job, created, claim) = jobs::create_claimed(
                &state.pool,
                &run.instance_id,
                "backup.create",
                &run.requested_by,
                Some(&idempotency_key),
            )
            .await?;
            if created {
                let claim = claim.expect("newly-created jobs always carry a claim");
                match backups::insert(
                    &state.pool,
                    &run.instance_id,
                    Some(&job.id),
                    "scheduled",
                    &run.requested_by,
                )
                .await
                {
                    Ok(backup) => {
                        backups::spawn_create(state.clone(), job.clone(), backup.id, claim)
                    }
                    Err(error) => {
                        if jobs::fail(
                            &state.pool,
                            &job.id,
                            "backup_record_failed",
                            "backups.creation_failed",
                        )
                        .await
                        .is_ok()
                            && let Err(disarm_error) = claim.disarm_terminal().await
                        {
                            tracing::error!(job_id = %job.id, %disarm_error, "failed to disarm scheduled backup claim");
                        }
                        return Err(error);
                    }
                }
            }
            publish_job(state, &job);
            Ok(job)
        }
        ScheduleAction::Console { command } => {
            let active: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM jobs WHERE instance_id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
            )
            .bind(&run.instance_id)
            .fetch_one(&state.pool)
            .await?;
            if active != 0 {
                return Err(AppError::Conflict("jobs.instance_busy".into()));
            }
            let (job, created, claim) = jobs::create_claimed(
                &state.pool,
                &run.instance_id,
                "console",
                &run.requested_by,
                Some(&idempotency_key),
            )
            .await?;
            if created {
                let claim = claim.expect("newly-created jobs always carry a claim");
                if jobs::begin(&state.pool, &job.id).await? {
                    let result = state
                        .runtime
                        .send_console(&run.instance_id, command.clone())
                        .await;
                    match result {
                        Ok(()) => {
                            jobs::succeed(&state.pool, &job.id).await?;
                            claim.disarm_terminal().await?;
                            database::audit(
                                &state.pool,
                                Some(&run.requested_by),
                                "server.console_command",
                                "instance",
                                Some(&run.instance_id),
                                "success",
                                serde_json::json!({
                                    "contents_recorded": false,
                                    "schedule_id": run.schedule_id,
                                }),
                            )
                            .await?;
                        }
                        Err(error) => {
                            if jobs::fail(
                                &state.pool,
                                &job.id,
                                "console_failed",
                                "servers.console_unavailable",
                            )
                            .await
                            .is_ok()
                                && let Err(disarm_error) = claim.disarm_terminal().await
                            {
                                tracing::error!(job_id = %job.id, %disarm_error, "failed to disarm failed console job claim");
                            }
                            return Err(error);
                        }
                    }
                } else {
                    claim.disarm_terminal().await?;
                }
            }
            let job = jobs::get(&state.pool, &job.id).await?;
            publish_job(state, &job);
            Ok(job)
        }
        action => {
            let runtime_action = match action {
                ScheduleAction::Start {} => RuntimeAction::Start,
                ScheduleAction::Stop {} => RuntimeAction::Stop,
                ScheduleAction::Restart {} => RuntimeAction::Restart,
                // RuntimeAction::Install performs both first installation and staged update.
                ScheduleAction::Update {} => RuntimeAction::Install,
                ScheduleAction::Backup {} | ScheduleAction::Console { .. } => unreachable!(),
            };
            let (job, created, claim) = jobs::create_claimed(
                &state.pool,
                &run.instance_id,
                runtime_action.as_str(),
                &run.requested_by,
                Some(&idempotency_key),
            )
            .await?;
            if created {
                let claim = claim.expect("newly-created jobs always carry a claim");
                if let Err(error) = state
                    .runtime
                    .enqueue_claimed(job.clone(), runtime_action, claim)
                    .await
                {
                    let _ = jobs::fail(
                        &state.pool,
                        &job.id,
                        "runtime_enqueue_failed",
                        "servers.runtime_unavailable",
                    )
                    .await;
                    return Err(error);
                }
            }
            publish_job(state, &job);
            Ok(job)
        }
    }
}

fn publish_job(state: &AppState, job: &Job) {
    state.events.publish(
        "job.queued",
        job.instance_id.clone(),
        serde_json::to_value(job).unwrap_or_default(),
    );
}

async fn ensure_instance_exists(pool: &DbPool, instance_id: &str) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM instances WHERE id = ?)")
        .bind(instance_id)
        .fetch_one(pool)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound("servers.not_found".into()))
    }
}

async fn ensure_execution_authorized(
    pool: &DbPool,
    user_id: &str,
    instance_id: &str,
    action_permission: &str,
) -> Result<(), AppError> {
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT u.role_id, r.permissions
        FROM users u JOIN roles r ON r.id = u.role_id
        WHERE u.id = ? AND u.is_active = 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some((role, permissions)) = row else {
        return Err(AppError::Forbidden("schedules.actor_inactive".into()));
    };
    let permissions = parse_permissions(&permissions)?;
    if !contains_permission(&permissions, "schedule.manage")
        || !contains_permission(&permissions, action_permission)
    {
        return Err(AppError::Forbidden("auth.permission_denied".into()));
    }
    if matches!(role.as_str(), "owner" | "admin") {
        return Ok(());
    }
    let grants: Option<String> = sqlx::query_scalar(
        "SELECT permissions FROM user_instance_grants WHERE user_id = ? AND instance_id = ?",
    )
    .bind(user_id)
    .bind(instance_id)
    .fetch_optional(pool)
    .await?;
    let Some(grants) = grants else {
        return Err(AppError::Forbidden("auth.instance_not_assigned".into()));
    };
    let grants = parse_permissions(&grants)?;
    if grants.is_empty()
        || (contains_permission(&grants, "schedule.manage")
            && contains_permission(&grants, action_permission))
    {
        Ok(())
    } else {
        Err(AppError::Forbidden("auth.permission_denied".into()))
    }
}

fn parse_permissions(raw: &str) -> Result<Vec<String>, AppError> {
    serde_json::from_str(raw)
        .map_err(|_| AppError::Internal("stored permissions are invalid".into()))
}

fn contains_permission(permissions: &[String], required: &str) -> bool {
    permissions
        .iter()
        .any(|permission| permission == "*" || permission == required)
}

async fn schedule_row(pool: &DbPool, id: &str) -> Result<ScheduleRow, AppError> {
    sqlx::query_as("SELECT * FROM schedules WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("schedules.not_found".into()))
}

async fn claimed_runs(pool: &DbPool) -> Result<Vec<ClaimedRun>, AppError> {
    let rows: Vec<ClaimedRunRow> = sqlx::query_as(
        "SELECT id, schedule_id, instance_id, scheduled_for, action_kind, action_payload, requested_by FROM schedule_runs WHERE status = 'claimed' ORDER BY claimed_at",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

async fn claim_next_due(pool: &DbPool, now: DateTime<Utc>) -> Result<Option<ClaimedRun>, AppError> {
    let mut transaction = pool.begin().await?;
    let row: Option<ScheduleRow> = sqlx::query_as(
        "SELECT * FROM schedules WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? ORDER BY next_run_at, id LIMIT 1",
    )
    .bind(now.to_rfc3339())
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(row) = row else {
        transaction.rollback().await?;
        return Ok(None);
    };
    let schedule: Schedule = row.try_into()?;
    let scheduled_for = schedule
        .next_run_at
        .as_deref()
        .ok_or_else(|| AppError::Internal("due schedule has no next run".into()))?
        .parse::<DateTime<Utc>>()
        .map_err(|_| AppError::Internal("invalid stored schedule next run".into()))?;
    let next = next_after_catchup(&schedule.trigger, scheduled_for, now)?;
    let run_id = Uuid::new_v4().to_string();
    let payload = schedule.action.payload().to_string();
    let inserted = sqlx::query(
        r#"
        INSERT INTO schedule_runs
            (id, schedule_id, instance_id, scheduled_for, action_kind, action_payload,
             requested_by, status, claimed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, 'claimed', ?)
        ON CONFLICT(schedule_id, scheduled_for) DO NOTHING
        "#,
    )
    .bind(&run_id)
    .bind(&schedule.id)
    .bind(&schedule.instance_id)
    .bind(scheduled_for.to_rfc3339())
    .bind(schedule.action.kind())
    .bind(&payload)
    .bind(&schedule.requested_by)
    .bind(now.to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    if inserted.rows_affected() == 0 {
        transaction.rollback().await?;
        return Ok(None);
    }
    let updated = sqlx::query(
        "UPDATE schedules SET last_run_at = ?, next_run_at = ? WHERE id = ? AND enabled = 1 AND version = ? AND next_run_at = ?",
    )
    .bind(scheduled_for.to_rfc3339())
    .bind(next.map(|value| value.to_rfc3339()))
    .bind(&schedule.id)
    .bind(i64::from(schedule.version))
    .bind(scheduled_for.to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 0 {
        transaction.rollback().await?;
        return Ok(None);
    }
    transaction.commit().await?;
    Ok(Some(ClaimedRun {
        id: run_id,
        schedule_id: schedule.id,
        instance_id: schedule.instance_id,
        scheduled_for: scheduled_for.to_rfc3339(),
        action: schedule.action,
        requested_by: schedule.requested_by,
    }))
}

async fn complete_run(pool: &DbPool, run: &ClaimedRun, job: &Job) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE schedule_runs SET status = 'submitted', job_id = ?, finished_at = ? WHERE id = ? AND status = 'claimed'",
    )
    .bind(&job.id)
    .bind(&now)
    .bind(&run.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE schedules SET last_job_id = ? WHERE id = ?")
        .bind(&job.id)
        .bind(&run.schedule_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

async fn fail_run(pool: &DbPool, run_id: &str, code: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE schedule_runs SET status = 'failed', error_code = ?, finished_at = ? WHERE id = ? AND status = 'claimed'",
    )
    .bind(code)
    .bind(Utc::now().to_rfc3339())
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn normalize_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(AppError::BadRequest("schedules.invalid_name".into()));
    }
    Ok(name.to_string())
}

fn normalize_trigger(trigger: ScheduleTrigger) -> Result<NormalizedTrigger, AppError> {
    match trigger {
        ScheduleTrigger::Interval { seconds }
            if (MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&seconds) =>
        {
            Ok(NormalizedTrigger {
                kind: "interval",
                cron_expression: None,
                interval_seconds: Some(i64::try_from(seconds).expect("bounded interval")),
                timezone: "UTC".into(),
                value: ScheduleTrigger::Interval { seconds },
            })
        }
        ScheduleTrigger::Interval { .. } => {
            Err(AppError::BadRequest("schedules.invalid_interval".into()))
        }
        ScheduleTrigger::Cron {
            expression,
            timezone,
        } => {
            let expression = expression.split_ascii_whitespace().collect::<Vec<_>>();
            if !matches!(expression.len(), 6 | 7) || expression.iter().any(|field| field.len() > 64)
            {
                return Err(AppError::BadRequest(
                    "schedules.invalid_cron_expression".into(),
                ));
            }
            let expression = expression.join(" ");
            CronSchedule::from_str(&expression)
                .map_err(|_| AppError::BadRequest("schedules.invalid_cron_expression".into()))?;
            let timezone = Tz::from_str(timezone.trim())
                .map_err(|_| AppError::BadRequest("schedules.invalid_timezone".into()))?
                .to_string();
            Ok(NormalizedTrigger {
                kind: "cron",
                cron_expression: Some(expression.clone()),
                interval_seconds: None,
                timezone: timezone.clone(),
                value: ScheduleTrigger::Cron {
                    expression,
                    timezone,
                },
            })
        }
    }
}

fn next_run(trigger: &ScheduleTrigger, after: DateTime<Utc>) -> Result<DateTime<Utc>, AppError> {
    match trigger {
        ScheduleTrigger::Interval { seconds } => after
            .checked_add_signed(chrono::Duration::seconds(
                i64::try_from(*seconds)
                    .map_err(|_| AppError::BadRequest("schedules.invalid_interval".into()))?,
            ))
            .ok_or_else(|| AppError::BadRequest("schedules.invalid_interval".into())),
        ScheduleTrigger::Cron {
            expression,
            timezone,
        } => {
            let timezone = Tz::from_str(timezone)
                .map_err(|_| AppError::BadRequest("schedules.invalid_timezone".into()))?;
            let schedule = CronSchedule::from_str(expression)
                .map_err(|_| AppError::BadRequest("schedules.invalid_cron_expression".into()))?;
            schedule
                .after(&after.with_timezone(&timezone))
                .next()
                .map(|value| value.with_timezone(&Utc))
                .ok_or_else(|| AppError::BadRequest("schedules.cron_has_no_future_run".into()))
        }
    }
}

fn next_after_catchup(
    trigger: &ScheduleTrigger,
    scheduled_for: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AppError> {
    match trigger {
        ScheduleTrigger::Interval { seconds } => {
            let seconds = i64::try_from(*seconds)
                .map_err(|_| AppError::Internal("invalid stored schedule interval".into()))?;
            let elapsed = now
                .signed_duration_since(scheduled_for)
                .num_seconds()
                .max(0);
            let steps = elapsed
                .checked_div(seconds)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| AppError::Internal("schedule interval overflow".into()))?;
            let offset = seconds
                .checked_mul(steps)
                .ok_or_else(|| AppError::Internal("schedule interval overflow".into()))?;
            scheduled_for
                .checked_add_signed(chrono::Duration::seconds(offset))
                .map(Some)
                .ok_or_else(|| AppError::Internal("schedule interval overflow".into()))
        }
        ScheduleTrigger::Cron { .. } => match next_run(trigger, now) {
            Ok(value) => Ok(Some(value)),
            Err(AppError::BadRequest(message)) if message == "schedules.cron_has_no_future_run" => {
                Ok(None)
            }
            Err(error) => Err(error),
        },
    }
}

fn schedule_error_code(error: &AppError) -> &'static str {
    match error {
        AppError::Forbidden(_) | AppError::Unauthorized(_) => "authorization_revoked",
        AppError::Conflict(_) => "instance_busy",
        AppError::NotFound(_) => "resource_not_found",
        AppError::BadRequest(_) | AppError::PreconditionRequired(_) => "invalid_action",
        AppError::TooManyRequests(_) => "rate_limited",
        AppError::Internal(_) | AppError::Database(_) | AppError::Rich { .. } => "dispatch_failed",
    }
}

impl TryFrom<ScheduleRow> for Schedule {
    type Error = AppError;

    fn try_from(row: ScheduleRow) -> Result<Self, Self::Error> {
        let trigger = match row.trigger_kind.as_str() {
            "cron" => ScheduleTrigger::Cron {
                expression: row.cron_expression.ok_or_else(|| {
                    AppError::Internal("stored cron schedule has no expression".into())
                })?,
                timezone: row.timezone,
            },
            "interval" => ScheduleTrigger::Interval {
                seconds: u64::try_from(row.interval_seconds.ok_or_else(|| {
                    AppError::Internal("stored interval schedule has no interval".into())
                })?)
                .map_err(|_| AppError::Internal("invalid stored schedule interval".into()))?,
            },
            _ => {
                return Err(AppError::Internal("invalid stored schedule trigger".into()));
            }
        };
        let trigger = normalize_trigger(trigger)?.value;
        let action = ScheduleAction::from_storage(&row.action_kind, &row.action_payload)?;
        Ok(Self {
            id: row.id,
            instance_id: row.instance_id,
            name: row.name,
            trigger,
            action,
            enabled: row.enabled,
            next_run_at: row.next_run_at,
            last_run_at: row.last_run_at,
            last_job_id: row.last_job_id,
            version: u32::try_from(row.version)
                .map_err(|_| AppError::Internal("invalid stored schedule version".into()))?,
            created_by: row.created_by,
            requested_by: row.requested_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<ClaimedRunRow> for ClaimedRun {
    type Error = AppError;

    fn try_from(row: ClaimedRunRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            schedule_id: row.schedule_id,
            instance_id: row.instance_id,
            scheduled_for: row.scheduled_for,
            action: ScheduleAction::from_storage(&row.action_kind, &row.action_payload)?,
            requested_by: row.requested_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> (tempfile::TempDir, DbPool, String, String) {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite:{}/schedules.db?mode=rwc",
            directory.path().display()
        );
        let pool = crate::core::database::init_pool(&database_url)
            .await
            .unwrap();
        crate::core::database::run_migrations(&pool).await.unwrap();
        let user_id = Uuid::new_v4().to_string();
        let instance_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) VALUES (?, 'scheduler-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO game_profiles (id, revision, kind, manifest, created_at) VALUES ('test-profile', 1, 'builtin', '{}', ?)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances (id, name, profile_id, profile_revision, created_at, updated_at) VALUES (?, 'Scheduled test', 'test-profile', 1, ?, ?)",
        )
        .bind(&instance_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        (directory, pool, user_id, instance_id)
    }

    #[test]
    fn trigger_validation_is_strict_and_normalized() {
        assert!(normalize_trigger(ScheduleTrigger::Interval { seconds: 59 }).is_err());
        assert!(normalize_trigger(ScheduleTrigger::Interval { seconds: 60 }).is_ok());
        assert!(
            normalize_trigger(ScheduleTrigger::Cron {
                expression: "0 0 * * * *".into(),
                timezone: "Europe/Paris".into(),
            })
            .is_ok()
        );
        assert!(
            normalize_trigger(ScheduleTrigger::Cron {
                expression: "0 * * * *".into(),
                timezone: "Europe/Paris".into(),
            })
            .is_err()
        );
        assert!(
            normalize_trigger(ScheduleTrigger::Cron {
                expression: "0 0 * * * *".into(),
                timezone: "GMT+2".into(),
            })
            .is_err()
        );
    }

    #[test]
    fn interval_catchup_runs_once_and_preserves_its_phase() {
        let scheduled = "2026-01-01T00:00:00Z".parse().unwrap();
        let now = "2026-01-01T01:00:01Z".parse().unwrap();
        let next = next_after_catchup(&ScheduleTrigger::Interval { seconds: 600 }, scheduled, now)
            .unwrap()
            .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-01-01T01:10:00+00:00");
    }

    #[test]
    fn cron_uses_iana_timezone_and_produces_monotonic_utc_instants() {
        let trigger = ScheduleTrigger::Cron {
            expression: "0 30 2 * * *".into(),
            timezone: "Europe/Paris".into(),
        };
        let before_fall_back = "2026-10-24T23:00:00Z".parse().unwrap();
        let first = next_run(&trigger, before_fall_back).unwrap();
        let second = next_run(&trigger, first).unwrap();
        assert!(first > before_fall_back);
        assert!(second > first);
    }

    #[test]
    fn action_shape_never_accepts_shell_or_script_fields() {
        let shell = serde_json::from_value::<ScheduleAction>(serde_json::json!({
            "kind": "start",
            "shell": "rm -rf /"
        }));
        assert!(shell.is_err());
        let script = serde_json::from_value::<ScheduleAction>(serde_json::json!({
            "kind": "script",
            "command": "whoami"
        }));
        assert!(script.is_err());
    }

    #[test]
    fn restart_requires_both_lifecycle_permissions() {
        assert_eq!(
            ScheduleAction::Restart {}.required_permissions(),
            &["server.start", "server.stop"]
        );
    }

    #[tokio::test]
    async fn a_due_occurrence_is_claimed_exactly_once_and_recoverable() {
        let (_directory, pool, user_id, instance_id) = test_pool().await;
        let schedule = create(
            &pool,
            &instance_id,
            ScheduleSpec {
                name: "Every minute".into(),
                trigger: ScheduleTrigger::Interval { seconds: 60 },
                action: ScheduleAction::Start {},
                enabled: false,
            },
            &user_id,
        )
        .await
        .unwrap();
        let due: DateTime<Utc> = "2026-06-01T12:00:00Z".parse().unwrap();
        let now: DateTime<Utc> = "2026-06-01T12:03:05Z".parse().unwrap();
        sqlx::query("UPDATE schedules SET enabled = 1, next_run_at = ? WHERE id = ?")
            .bind(due.to_rfc3339())
            .bind(&schedule.id)
            .execute(&pool)
            .await
            .unwrap();

        let claimed = claim_next_due(&pool, now).await.unwrap().unwrap();
        assert_eq!(claimed.schedule_id, schedule.id);
        assert_eq!(claimed.scheduled_for, due.to_rfc3339());
        assert!(claim_next_due(&pool, now).await.unwrap().is_none());
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM schedule_runs WHERE schedule_id = ? AND scheduled_for = ?",
        )
        .bind(&schedule.id)
        .bind(due.to_rfc3339())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        let recovered = claimed_runs(&pool).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, claimed.id);
        let next: String = sqlx::query_scalar("SELECT next_run_at FROM schedules WHERE id = ?")
            .bind(&schedule.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(next, "2026-06-01T12:04:00+00:00");
    }
}
