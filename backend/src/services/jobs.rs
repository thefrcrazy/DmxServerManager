use serde::Deserialize;
use sqlx::FromRow;

use crate::{
    core::{DbPool, error::AppError},
    domain::v1::{Job, JobInteraction, JobState},
};

/// Owns the lifecycle of a newly-created active job until a worker has
/// persisted a deliberate hand-off or a terminal state.
///
/// Dropping an armed claim schedules an idempotent transition to
/// `interrupted`. This makes request cancellation, task abortion and panic
/// safe: an active row cannot be abandoned indefinitely merely because the
/// future that created or processed it disappeared.
#[derive(Debug)]
pub struct JobClaim {
    pool: DbPool,
    job_id: Option<String>,
}

#[cfg(test)]
struct PostInsertHook {
    kind: String,
    inserted: tokio::sync::oneshot::Sender<String>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(test)]
static POST_INSERT_HOOK: std::sync::Mutex<Option<PostInsertHook>> = std::sync::Mutex::new(None);

impl JobClaim {
    fn new(pool: &DbPool, job_id: String) -> Self {
        Self {
            pool: pool.clone(),
            job_id: Some(job_id),
        }
    }

    pub fn job_id(&self) -> &str {
        self.job_id
            .as_deref()
            .expect("an armed JobClaim always has a job id")
    }

    /// Disarms this claim only after the database confirms that the job is
    /// terminal. Cancellation while this check is in flight still drops an
    /// armed claim and therefore runs the interruption fallback.
    pub async fn disarm_terminal(mut self) -> Result<(), AppError> {
        self.disarm_if_state(|state| {
            matches!(state, "succeeded" | "failed" | "cancelled" | "interrupted")
        })
        .await
    }

    /// Deliberately hands a job to a user interaction. This is the sole active
    /// state in which a worker may exit without terminalising the job.
    pub async fn disarm_waiting(mut self) -> Result<(), AppError> {
        self.disarm_if_state(|state| state == "waiting_for_user")
            .await
    }

    async fn disarm_if_state(
        &mut self,
        allowed: impl FnOnce(&str) -> bool,
    ) -> Result<(), AppError> {
        let job_id = self.job_id();
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?;
        let state = state.ok_or_else(|| AppError::NotFound("jobs.not_found".into()))?;
        if !allowed(&state) {
            return Err(AppError::Conflict("jobs.claim_still_active".into()));
        }
        self.job_id = None;
        Ok(())
    }
}

impl Drop for JobClaim {
    fn drop(&mut self) {
        let Some(job_id) = self.job_id.take() else {
            return;
        };
        let pool = self.pool.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::error!(
                %job_id,
                "active job claim dropped outside a Tokio runtime; interruption could not be scheduled"
            );
            return;
        };
        handle.spawn(async move {
            const RETRIES: usize = 3;
            for attempt in 1..=RETRIES {
                match interrupt_abandoned(&pool, &job_id).await {
                    Ok(true) => {
                        tracing::warn!(%job_id, "abandoned active job marked interrupted");
                        return;
                    }
                    Ok(false) => {
                        tracing::debug!(%job_id, "dropped job claim was already inactive");
                        return;
                    }
                    Err(error) if attempt < RETRIES => {
                        tracing::warn!(
                            %job_id,
                            %error,
                            attempt,
                            "failed to interrupt abandoned job; retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(25 * attempt as u64))
                            .await;
                    }
                    Err(error) => {
                        tracing::error!(
                            %job_id,
                            %error,
                            "failed to interrupt abandoned job after retries"
                        );
                        return;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
async fn pause_after_claim_created(kind: &str, job_id: &str) {
    let hook = {
        let mut slot = POST_INSERT_HOOK.lock().expect("post-insert hook lock");
        if slot.as_ref().is_some_and(|hook| hook.kind == kind) {
            slot.take()
        } else {
            None
        }
    };
    if let Some(hook) = hook {
        let _ = hook.inserted.send(job_id.to_string());
        let _ = hook.resume.await;
    }
}

#[derive(Debug, FromRow)]
pub struct JobRow {
    pub id: String,
    pub instance_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub progress: i64,
    pub requested_by: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub interaction_payload: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitingForUserPayload {
    job_id: String,
    interaction: JobInteraction,
}

#[cfg(test)]
pub async fn create(
    pool: &DbPool,
    instance_id: &str,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
) -> Result<(Job, bool), AppError> {
    let (job, created, claim) = create_internal(
        pool,
        instance_id,
        kind,
        requested_by,
        idempotency_key,
        false,
    )
    .await?;
    debug_assert!(claim.is_none());
    Ok((job, created))
}

/// Creates an instance-scoped job and returns an armed claim exactly when a
/// new row was inserted. The claim is constructed synchronously after the
/// INSERT completes and before the next await point.
pub async fn create_claimed(
    pool: &DbPool,
    instance_id: &str,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
) -> Result<(Job, bool, Option<JobClaim>), AppError> {
    create_internal(pool, instance_id, kind, requested_by, idempotency_key, true).await
}

async fn create_internal(
    pool: &DbPool,
    instance_id: &str,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
    claim_new: bool,
) -> Result<(Job, bool, Option<JobClaim>), AppError> {
    if kind != "install" {
        let unresolved_update: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM instance_update_transactions WHERE instance_id = ?)",
        )
        .bind(instance_id)
        .fetch_one(pool)
        .await?;
        if unresolved_update {
            return Err(AppError::Conflict(
                "servers.update_transaction_incomplete".into(),
            ));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result = if let Some(idempotency_key) = idempotency_key {
        sqlx::query(
            r#"
            INSERT INTO jobs
                (id, instance_id, kind, state, progress, idempotency_key, requested_by, created_at)
            VALUES (?, ?, ?, 'queued', 0, ?, ?, ?)
            ON CONFLICT(idempotency_key) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(instance_id)
        .bind(kind)
        .bind(idempotency_key)
        .bind(requested_by)
        .bind(&now)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            r#"
            INSERT INTO jobs
                (id, instance_id, kind, state, progress, requested_by, created_at)
            VALUES (?, ?, ?, 'queued', 0, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(instance_id)
        .bind(kind)
        .bind(requested_by)
        .bind(&now)
        .execute(pool)
        .await
    };
    let result = match result {
        Ok(result) => result,
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(AppError::Conflict("jobs.instance_busy".into()));
        }
        Err(error) => return Err(error.into()),
    };

    if result.rows_affected() == 1 {
        // Do not insert an await between the successful INSERT and creation of
        // the claim. Futures can only be cancelled at an await boundary, so
        // this closes the post-INSERT cancellation window.
        let claim = claim_new.then(|| JobClaim::new(pool, id.clone()));
        #[cfg(test)]
        if claim.is_some() {
            pause_after_claim_created(kind, &id).await;
        }
        append_event(pool, &id, "job.queued", serde_json::json!({"kind": kind})).await?;
        return Ok((get(pool, &id).await?, true, claim));
    }

    let key = idempotency_key.ok_or_else(|| AppError::Conflict("jobs.could_not_create".into()))?;
    let existing = find_idempotent(pool, key, instance_id, kind, requested_by)
        .await?
        .ok_or_else(|| AppError::Conflict("jobs.idempotency_key_conflict".into()))?;
    Ok((existing, false, None))
}

/// Creates a persisted job that is not scoped to a game-server instance.
///
/// Global jobs are reserved for panel-level operations such as importing a
/// catalogue package. Their idempotency key still shares the global database
/// namespace, so callers must namespace it before calling this function.
#[cfg(test)]
pub async fn create_global(
    pool: &DbPool,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
) -> Result<(Job, bool), AppError> {
    let (job, created, claim) =
        create_global_internal(pool, kind, requested_by, idempotency_key, false).await?;
    debug_assert!(claim.is_none());
    Ok((job, created))
}

/// Creates a panel-level job and returns an armed claim exactly when the row
/// was newly inserted.
pub async fn create_global_claimed(
    pool: &DbPool,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
) -> Result<(Job, bool, Option<JobClaim>), AppError> {
    create_global_internal(pool, kind, requested_by, idempotency_key, true).await
}

async fn create_global_internal(
    pool: &DbPool,
    kind: &str,
    requested_by: &str,
    idempotency_key: Option<&str>,
    claim_new: bool,
) -> Result<(Job, bool, Option<JobClaim>), AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result = if let Some(idempotency_key) = idempotency_key {
        sqlx::query(
            "INSERT INTO jobs \
             (id, instance_id, kind, state, progress, idempotency_key, requested_by, created_at) \
             VALUES (?, NULL, ?, 'queued', 0, ?, ?, ?) \
             ON CONFLICT(idempotency_key) DO NOTHING",
        )
        .bind(&id)
        .bind(kind)
        .bind(idempotency_key)
        .bind(requested_by)
        .bind(&now)
        .execute(pool)
        .await?
    } else {
        sqlx::query(
            "INSERT INTO jobs \
             (id, instance_id, kind, state, progress, requested_by, created_at) \
             VALUES (?, NULL, ?, 'queued', 0, ?, ?)",
        )
        .bind(&id)
        .bind(kind)
        .bind(requested_by)
        .bind(&now)
        .execute(pool)
        .await?
    };

    if result.rows_affected() == 1 {
        // See create_internal: the guard must exist before append_event/get can
        // yield or fail.
        let claim = claim_new.then(|| JobClaim::new(pool, id.clone()));
        #[cfg(test)]
        if claim.is_some() {
            pause_after_claim_created(kind, &id).await;
        }
        append_event(pool, &id, "job.queued", serde_json::json!({"kind": kind})).await?;
        return Ok((get(pool, &id).await?, true, claim));
    }

    let key = idempotency_key.ok_or_else(|| AppError::Conflict("jobs.could_not_create".into()))?;
    let existing = find_global_idempotent(pool, key, kind, requested_by)
        .await?
        .ok_or_else(|| AppError::Conflict("jobs.idempotency_key_conflict".into()))?;
    Ok((existing, false, None))
}

pub async fn get(pool: &DbPool, id: &str) -> Result<Job, AppError> {
    let row: JobRow = sqlx::query_as(
        "SELECT j.id, j.instance_id, j.kind, j.state, j.progress, j.requested_by, \
         j.error_code, j.error_message, j.created_at, j.started_at, j.finished_at, \
         CASE WHEN j.state = 'waiting_for_user' THEN ( \
             SELECT e.payload FROM job_events e \
             WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user' \
             ORDER BY e.id DESC LIMIT 1 \
         ) ELSE NULL END AS interaction_payload \
         FROM jobs j WHERE j.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("jobs.not_found".into()))?;
    row.try_into()
}

pub async fn find_idempotent(
    pool: &DbPool,
    key: &str,
    instance_id: &str,
    kind: &str,
    requested_by: &str,
) -> Result<Option<Job>, AppError> {
    let row: Option<JobRow> = sqlx::query_as(
        "SELECT j.id, j.instance_id, j.kind, j.state, j.progress, j.requested_by, \
         j.error_code, j.error_message, j.created_at, j.started_at, j.finished_at, \
         CASE WHEN j.state = 'waiting_for_user' THEN ( \
             SELECT e.payload FROM job_events e \
             WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user' \
             ORDER BY e.id DESC LIMIT 1 \
         ) ELSE NULL END AS interaction_payload \
         FROM jobs j WHERE j.idempotency_key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let existing: Job = row.try_into()?;
    if existing.instance_id.as_deref() != Some(instance_id)
        || existing.kind != kind
        || existing.requested_by != requested_by
    {
        return Err(AppError::Conflict("jobs.idempotency_key_conflict".into()));
    }
    Ok(Some(existing))
}

pub async fn find_global_idempotent(
    pool: &DbPool,
    key: &str,
    kind: &str,
    requested_by: &str,
) -> Result<Option<Job>, AppError> {
    let row: Option<JobRow> = sqlx::query_as(
        "SELECT j.id, j.instance_id, j.kind, j.state, j.progress, j.requested_by, \
         j.error_code, j.error_message, j.created_at, j.started_at, j.finished_at, \
         CASE WHEN j.state = 'waiting_for_user' THEN ( \
             SELECT e.payload FROM job_events e \
             WHERE e.job_id = j.id AND e.event_type = 'job.waiting_for_user' \
             ORDER BY e.id DESC LIMIT 1 \
         ) ELSE NULL END AS interaction_payload \
         FROM jobs j WHERE j.idempotency_key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let existing: Job = row.try_into()?;
    if existing.instance_id.is_some()
        || existing.kind != kind
        || existing.requested_by != requested_by
    {
        return Err(AppError::Conflict("jobs.idempotency_key_conflict".into()));
    }
    Ok(Some(existing))
}

pub async fn begin(pool: &DbPool, id: &str) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET state = 'running', progress = 1, started_at = ? \
         WHERE id = ? AND state = 'queued'",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 1 {
        append_event(pool, id, "job.running", serde_json::json!({"progress": 1})).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub async fn progress(pool: &DbPool, id: &str, value: u8) -> Result<(), AppError> {
    let value = value.min(99);
    sqlx::query("UPDATE jobs SET progress = ? WHERE id = ? AND state = 'running'")
        .bind(value)
        .bind(id)
        .execute(pool)
        .await?;
    append_event(
        pool,
        id,
        "job.progress",
        serde_json::json!({"progress": value}),
    )
    .await
}

pub async fn wait_for_user(
    pool: &DbPool,
    id: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    if !payload.is_object() {
        return Err(AppError::Internal(
            "waiting-for-user payload must be an object".into(),
        ));
    }
    let result = sqlx::query(
        "UPDATE jobs SET state = 'waiting_for_user' WHERE id = ? AND state = 'running'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
    }
    append_event(pool, id, "job.waiting_for_user", payload).await
}

pub async fn resume_from_user(pool: &DbPool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE jobs SET state = 'running' WHERE id = ? AND state = 'waiting_for_user'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
    }
    append_event(
        pool,
        id,
        "job.running",
        serde_json::json!({"resumed_after_user_action": true}),
    )
    .await
}

pub async fn requeue_from_user(pool: &DbPool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE jobs SET state = 'queued', error_code = NULL, error_message = NULL, \
         finished_at = NULL WHERE id = ? AND state = 'waiting_for_user'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
    }
    append_event(
        pool,
        id,
        "job.queued",
        serde_json::json!({"resumed_after_user_action": true}),
    )
    .await
}

pub async fn expire_waiting(
    pool: &DbPool,
    id: &str,
    code: &str,
    client_message: &str,
) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET state = 'failed', progress = 100, error_code = ?, error_message = ?, \
         finished_at = ? WHERE id = ? AND state = 'waiting_for_user'",
    )
    .bind(code)
    .bind(client_message)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(false);
    }
    append_event(
        pool,
        id,
        "job.failed",
        serde_json::json!({"error_code": code}),
    )
    .await?;
    Ok(true)
}

pub async fn succeed(pool: &DbPool, id: &str) -> Result<(), AppError> {
    finish(pool, id, "succeeded", 100, None, None).await
}

pub async fn fail(
    pool: &DbPool,
    id: &str,
    code: &str,
    client_message: &str,
) -> Result<(), AppError> {
    finish(pool, id, "failed", 100, Some(code), Some(client_message)).await
}

pub async fn cancel(pool: &DbPool, id: &str, code: &str) -> Result<(), AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET state = 'cancelled', progress = 100, error_code = ?, \
         error_message = 'jobs.cancelled', finished_at = ? \
         WHERE id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(code)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
    }
    append_event(
        pool,
        id,
        "job.cancelled",
        serde_json::json!({"error_code": code}),
    )
    .await
}

pub async fn request_install_cancel(
    pool: &DbPool,
    id: &str,
    instance_id: &str,
) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut transaction = pool.begin().await?;
    let requested = sqlx::query(
        "UPDATE jobs SET cancel_requested_at = ? \
         WHERE id = ? AND instance_id = ? AND kind = 'install' \
         AND state IN ('queued', 'running', 'waiting_for_user') \
         AND cancel_requested_at IS NULL",
    )
    .bind(&now)
    .bind(id)
    .bind(instance_id)
    .execute(&mut *transaction)
    .await?;
    if requested.rows_affected() == 1 {
        sqlx::query(
            "INSERT INTO job_events (job_id, event_type, payload, created_at) \
             VALUES (?, 'job.cancel_requested', '{\"error_code\":\"cancelled_by_user\"}', ?)",
        )
        .bind(id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(true);
    }
    transaction.rollback().await?;

    let state: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT state, cancel_requested_at FROM jobs \
         WHERE id = ? AND instance_id = ? AND kind = 'install'",
    )
    .bind(id)
    .bind(instance_id)
    .fetch_optional(pool)
    .await?;
    Ok(state.is_some_and(|(state, requested_at)| {
        state == "cancelled"
            || (requested_at.is_some()
                && matches!(state.as_str(), "queued" | "running" | "waiting_for_user"))
    }))
}

pub async fn install_cancel_requested(pool: &DbPool, id: &str) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM jobs WHERE id = ? AND kind = 'install' \
         AND state IN ('queued', 'running', 'waiting_for_user') \
         AND cancel_requested_at IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn cancel_pending(pool: &DbPool, id: &str, code: &str) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET state = 'cancelled', progress = 100, error_code = ?, \
         error_message = 'jobs.cancelled', finished_at = ? \
         WHERE id = ? AND state IN ('queued', 'waiting_for_user')",
    )
    .bind(code)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(false);
    }
    append_event(
        pool,
        id,
        "job.cancelled",
        serde_json::json!({"error_code": code}),
    )
    .await?;
    Ok(true)
}

async fn finish(
    pool: &DbPool,
    id: &str,
    state: &str,
    progress: u8,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET state = ?, progress = ?, error_code = ?, error_message = ?, \
         finished_at = ? WHERE id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(state)
    .bind(progress)
    .bind(error_code)
    .bind(error_message)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("jobs.invalid_state_transition".into()));
    }
    append_event(
        pool,
        id,
        if state == "succeeded" {
            "job.succeeded"
        } else {
            "job.failed"
        },
        serde_json::json!({"error_code": error_code}),
    )
    .await
}

async fn interrupt_abandoned(pool: &DbPool, id: &str) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut transaction = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE jobs SET state = 'interrupted', progress = 100, \
         error_code = 'job_claim_dropped', error_message = 'jobs.interrupted', finished_at = ? \
         WHERE id = ? AND state IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(&now)
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 0 {
        transaction.rollback().await?;
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO job_events (job_id, event_type, payload, created_at) \
         VALUES (?, 'job.interrupted', '{\"error_code\":\"job_claim_dropped\"}', ?)",
    )
    .bind(id)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(true)
}

pub async fn append_event(
    pool: &DbPool,
    job_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO job_events (job_id, event_type, payload, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(job_id)
    .bind(event_type)
    .bind(payload.to_string())
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

impl TryFrom<JobRow> for Job {
    type Error = AppError;

    fn try_from(row: JobRow) -> Result<Self, Self::Error> {
        let state = match row.state.as_str() {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "waiting_for_user" => JobState::WaitingForUser,
            "succeeded" => JobState::Succeeded,
            "failed" => JobState::Failed,
            "cancelled" => JobState::Cancelled,
            "interrupted" => JobState::Interrupted,
            _ => return Err(AppError::Internal("invalid job state".into())),
        };
        let interaction = validated_interaction(
            &row.id,
            &state,
            row.instance_id.as_deref(),
            row.interaction_payload.as_deref(),
        );
        Ok(Job {
            id: row.id,
            instance_id: row.instance_id,
            kind: row.kind,
            state,
            progress: u8::try_from(row.progress)
                .map_err(|_| AppError::Internal("invalid job progress".into()))?,
            requested_by: row.requested_by,
            error_code: row.error_code,
            error_message: row.error_message,
            created_at: row.created_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            interaction,
        })
    }
}

pub(crate) fn validated_interaction(
    job_id: &str,
    state: &JobState,
    job_instance_id: Option<&str>,
    payload: Option<&str>,
) -> Option<JobInteraction> {
    if *state != JobState::WaitingForUser {
        return None;
    }
    let envelope: WaitingForUserPayload = serde_json::from_str(payload?).ok()?;
    if envelope.job_id != job_id || !interaction_is_safe(&envelope.interaction, job_instance_id) {
        return None;
    }
    Some(envelope.interaction)
}

fn interaction_is_safe(interaction: &JobInteraction, job_instance_id: Option<&str>) -> bool {
    match interaction {
        JobInteraction::OauthDevice {
            verification_uri,
            user_code,
        } => {
            if job_instance_id.is_none() {
                return false;
            }
            let Ok(uri) = reqwest::Url::parse(verification_uri) else {
                return false;
            };
            let valid_uri = uri.scheme() == "https"
                && uri.host_str() == Some("accounts.hytale.com")
                && uri.port().is_none()
                && uri.path() == "/device"
                && uri.username().is_empty()
                && uri.password().is_none()
                && uri.fragment().is_none()
                && uri
                    .query_pairs()
                    .all(|(key, value)| key == "user_code" && valid_user_code(value.as_ref()));
            valid_uri && user_code.as_deref().is_none_or(valid_user_code)
        }
        JobInteraction::BedrockArchiveUpload {
            instance_id,
            version,
            method,
            path,
            required_sha256_header,
            max_bytes,
        } => {
            let Some(job_instance_id) = job_instance_id else {
                return false;
            };
            uuid::Uuid::parse_str(instance_id).is_ok()
                && instance_id == job_instance_id
                && version.as_deref().is_none_or(|value| {
                    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
                })
                && method == "POST"
                && path == &format!("/api/v1/servers/{instance_id}/imports/zip")
                && required_sha256_header == "x-dmx-archive-sha256"
                && *max_bytes == 4 * 1024 * 1024 * 1024
        }
    }
}

fn valid_user_code(value: &str) -> bool {
    (4..=32).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use crate::core::database;

    use super::*;

    async fn global_job_fixture(name: &str) -> (tempfile::TempDir, DbPool, String) {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/jobs.db?mode=rwc", root.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users \
             (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, ?, 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(name)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        (root, pool, user_id)
    }

    async fn wait_for_job_state(pool: &DbPool, job_id: &str, expected: &str) {
        for _ in 0..100 {
            let state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(pool)
                .await
                .unwrap();
            if state == expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(pool)
            .await
            .unwrap();
        panic!("job {job_id} stayed in {state}, expected {expected}");
    }

    #[test]
    fn job_claim_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<JobClaim>();
    }

    #[tokio::test]
    async fn dropping_a_claim_interrupts_the_active_job_once() {
        let (_root, pool, user_id) = global_job_fixture("claim-drop-owner").await;
        let (job, created, claim) = create_global_claimed(&pool, "claim.drop", &user_id, None)
            .await
            .unwrap();
        assert!(created);
        drop(claim.expect("new job claim"));

        wait_for_job_state(&pool, &job.id, "interrupted").await;
        let row: (i64, Option<String>) = sqlx::query_as(
            "SELECT COUNT(*), MAX(payload) FROM job_events \
             WHERE job_id = ? AND event_type = 'job.interrupted'",
        )
        .bind(&job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, 1);
        assert!(row.1.unwrap().contains("job_claim_dropped"));

        // A second cleanup attempt is a no-op and cannot duplicate the event.
        assert!(!interrupt_abandoned(&pool, &job.id).await.unwrap());
        let interrupted_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ? AND event_type = 'job.interrupted'",
        )
        .bind(&job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(interrupted_events, 1);
    }

    #[tokio::test]
    async fn terminal_job_can_disarm_without_an_interruption_event() {
        let (_root, pool, user_id) = global_job_fixture("claim-terminal-owner").await;
        let (job, _, claim) = create_global_claimed(&pool, "claim.terminal", &user_id, None)
            .await
            .unwrap();
        succeed(&pool, &job.id).await.unwrap();
        claim
            .expect("new job claim")
            .disarm_terminal()
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let refreshed = get(&pool, &job.id).await.unwrap();
        assert_eq!(refreshed.state, JobState::Succeeded);
        let interrupted_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ? AND event_type = 'job.interrupted'",
        )
        .bind(&job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(interrupted_events, 0);
    }

    #[tokio::test]
    async fn aborting_a_spawned_future_before_work_starts_drops_its_claim() {
        let (_root, pool, user_id) = global_job_fixture("claim-abort-owner").await;
        let (job, _, claim) = create_global_claimed(&pool, "claim.abort", &user_id, None)
            .await
            .unwrap();
        let claimed_worker = async move {
            let _claim = claim.expect("new job claim");
            std::future::pending::<()>().await;
        };
        let handle = tokio::spawn(claimed_worker);
        handle.abort();
        let _ = handle.await;

        wait_for_job_state(&pool, &job.id, "interrupted").await;
    }

    #[tokio::test]
    async fn cancellation_at_the_first_post_insert_await_interrupts_the_job() {
        let (_root, pool, user_id) = global_job_fixture("claim-post-insert-owner").await;
        let kind = format!("claim.post_insert.{}", uuid::Uuid::new_v4());
        let (inserted_tx, inserted_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        {
            let mut hook = POST_INSERT_HOOK.lock().unwrap();
            assert!(hook.is_none());
            *hook = Some(PostInsertHook {
                kind: kind.clone(),
                inserted: inserted_tx,
                resume: resume_rx,
            });
        }
        let task_pool = pool.clone();
        let task_user_id = user_id.clone();
        let task = tokio::spawn(async move {
            create_global_claimed(&task_pool, &kind, &task_user_id, None).await
        });
        let job_id = inserted_rx.await.expect("INSERT completed");
        task.abort();
        let _ = task.await;
        drop(resume_tx);

        wait_for_job_state(&pool, &job_id, "interrupted").await;
        let queued_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ? AND event_type = 'job.queued'",
        )
        .bind(&job_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            queued_events, 0,
            "cancellation happened before append_event"
        );
    }

    #[tokio::test]
    async fn global_jobs_are_idempotent_but_never_instance_scoped() {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/jobs.db?mode=rwc", root.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users \
             (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'global-job-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let (created, is_new) = create_global(
            &pool,
            "catalog.import:aaaaaaaa",
            &user_id,
            Some("catalog:test-key"),
        )
        .await
        .unwrap();
        assert!(is_new);
        assert!(created.instance_id.is_none());
        let (replayed, is_new) = create_global(
            &pool,
            "catalog.import:aaaaaaaa",
            &user_id,
            Some("catalog:test-key"),
        )
        .await
        .unwrap();
        assert!(!is_new);
        assert_eq!(created.id, replayed.id);
        assert!(
            create_global(
                &pool,
                "catalog.import:bbbbbbbb",
                &user_id,
                Some("catalog:test-key"),
            )
            .await
            .is_err(),
            "an idempotency key must not be replayed for a different archive"
        );
    }

    #[tokio::test]
    async fn waiting_interaction_survives_refresh_but_never_reflects_arbitrary_payloads() {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/waiting-jobs.db?mode=rwc", root.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users \
             (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'waiting-job-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO game_profiles (id, revision, kind, manifest, created_at) \
             VALUES ('minecraft-bedrock', 1, 'builtin', '{}', ?)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, installation_state, created_at, updated_at) \
             VALUES (?, 'Bedrock', 'minecraft-bedrock', 1, ?, 'installing', ?, ?)",
        )
        .bind(&instance_id)
        .bind(serde_json::json!({"version": "1.21.0"}).to_string())
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let (job, _) = create(&pool, &instance_id, "install", &user_id, None)
            .await
            .unwrap();
        assert!(begin(&pool, &job.id).await.unwrap());
        wait_for_user(
            &pool,
            &job.id,
            serde_json::json!({
                "job_id": job.id,
                "interaction": {
                    "kind": "bedrock_archive_upload",
                    "instance_id": instance_id,
                    "version": "1.21.0",
                    "method": "POST",
                    "path": format!("/api/v1/servers/{}/imports/zip", instance_id),
                    "required_sha256_header": "x-dmx-archive-sha256",
                    "max_bytes": 4_u64 * 1024 * 1024 * 1024
                }
            }),
        )
        .await
        .unwrap();

        let refreshed = get(&pool, &job.id).await.unwrap();
        assert!(matches!(
            refreshed.interaction,
            Some(JobInteraction::BedrockArchiveUpload { ref instance_id, .. })
                if instance_id == &refreshed.instance_id.clone().unwrap()
        ));

        append_event(
            &pool,
            &job.id,
            "job.waiting_for_user",
            serde_json::json!({
                "job_id": job.id,
                "interaction": {
                    "kind": "bedrock_archive_upload",
                    "instance_id": instance_id,
                    "version": "1.21.0",
                    "method": "POST",
                    "path": format!("/api/v1/servers/{}/imports/zip", instance_id),
                    "required_sha256_header": "x-dmx-archive-sha256",
                    "max_bytes": 4_u64 * 1024 * 1024 * 1024,
                    "secret": "must-never-be-reflected"
                }
            }),
        )
        .await
        .unwrap();
        let sanitized = get(&pool, &job.id).await.unwrap();
        assert_eq!(sanitized.interaction, None);
        assert!(
            !serde_json::to_string(&sanitized)
                .unwrap()
                .contains("must-never-be-reflected")
        );

        assert!(
            cancel_pending(&pool, &job.id, "cancelled_by_test")
                .await
                .unwrap()
        );
        assert!(
            fail(&pool, &job.id, "late_failure", "jobs.failed")
                .await
                .is_err()
        );
        let terminal_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM job_events WHERE job_id = ? \
             AND event_type IN ('job.succeeded', 'job.failed', 'job.cancelled')",
        )
        .bind(&job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(terminal_events, 1);
    }
}
