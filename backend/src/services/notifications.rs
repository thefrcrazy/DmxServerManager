use serde::Serialize;
use sqlx::FromRow;

use crate::core::{DbPool, error::AppError, events::EventHub};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Notification {
    pub id: String,
    pub kind: String,
    pub message_key: String,
    #[sqlx(json)]
    pub data: serde_json::Value,
    pub read_at: Option<String>,
    pub created_at: String,
}

pub async fn create(
    pool: &DbPool,
    events: &EventHub,
    user_id: &str,
    kind: &str,
    message_key: &str,
    data: serde_json::Value,
) -> Result<Notification, AppError> {
    validate_identifier(kind, 64)?;
    validate_identifier(message_key, 128)?;
    if !data.is_object() {
        return Err(AppError::Internal(
            "notification data must be a JSON object".into(),
        ));
    }

    let notification = Notification {
        id: uuid::Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        message_key: message_key.to_string(),
        data,
        read_at: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, message_key, data, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&notification.id)
    .bind(user_id)
    .bind(&notification.kind)
    .bind(&notification.message_key)
    .bind(notification.data.to_string())
    .bind(&notification.created_at)
    .execute(pool)
    .await?;
    events.publish_to_user(
        "notification.created",
        user_id,
        serde_json::to_value(&notification)
            .map_err(|error| AppError::Internal(error.to_string()))?,
    );
    Ok(notification)
}

fn validate_identifier(value: &str, max_len: usize) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(AppError::Internal(
            "invalid internal notification identifier".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_identifiers_are_closed_ascii_values() {
        assert!(validate_identifier("jobs.failed", 64).is_ok());
        assert!(validate_identifier("jobs/<script>", 64).is_err());
        assert!(validate_identifier("", 64).is_err());
    }

    #[tokio::test]
    async fn notifications_are_persisted_and_targeted_to_one_user() {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!(
            "sqlite:{}/notifications.db?mode=rwc",
            directory.path().display()
        );
        let pool = crate::core::database::init_pool(&database_url)
            .await
            .unwrap();
        crate::core::database::run_migrations(&pool).await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES ('alice', 'alice', 'unused', 'viewer', ?, ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        let events = EventHub::new(8);
        let mut receiver = events.subscribe();

        let notification = create(
            &pool,
            &events,
            "alice",
            "job.failed",
            "notifications.job_failed",
            serde_json::json!({"job_id": "job-1"}),
        )
        .await
        .unwrap();

        let stored: (String, String) =
            sqlx::query_as("SELECT message_key, data FROM notifications WHERE id = ?")
                .bind(&notification.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored.0, "notifications.job_failed");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored.1).unwrap()["job_id"],
            "job-1"
        );
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.audience_user_id.as_deref(), Some("alice"));
        assert_eq!(event.event_type, "notification.created");
    }
}
