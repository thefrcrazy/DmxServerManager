use std::time::Duration;

use futures::{StreamExt, stream};
use reqwest::{Client, Url, redirect::Policy};
use serde::Serialize;
use sqlx::FromRow;
use tokio::sync::{broadcast, watch};

use crate::{
    core::{AppState, DbPool, error::AppError, events::ApiEvent},
    services::{installers::USER_AGENT, secrets::SecretStore},
};

pub const ALLOWED_EVENTS: &[&str] = &[
    "backup.created",
    "backup.restored",
    "job.failed",
    "server.crashed",
    "server.started",
    "server.stopped",
    "server.update_applied",
    "server.update_failed",
    "server.update_rolled_back",
];

const MAX_WEBHOOKS: i64 = 16;
const DELIVERY_CONCURRENCY: usize = 4;

#[derive(Debug, FromRow)]
struct DeliveryTarget {
    id: String,
    url_nonce: String,
    url_ciphertext: String,
    #[sqlx(json)]
    events: Vec<String>,
}

#[derive(Serialize)]
struct DiscordMessage<'a> {
    username: &'static str,
    content: &'a str,
    allowed_mentions: DiscordAllowedMentions,
}

#[derive(Serialize)]
struct DiscordAllowedMentions {
    parse: [String; 0],
}

pub struct WebhookDispatcher {
    shutdown: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl WebhookDispatcher {
    pub fn start(state: AppState) -> Result<Self, AppError> {
        let client = webhook_client()?;
        let (shutdown, receiver) = watch::channel(false);
        let events = state.events.subscribe();
        let task = tokio::spawn(run_dispatcher(state, client, events, receiver));
        Ok(Self {
            shutdown,
            task: Some(task),
        })
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
            tracing::warn!("webhook dispatcher shutdown timed out");
            task.abort();
            let _ = task.await;
        }
    }
}

pub fn validate_event_set(mut events: Vec<String>) -> Result<Vec<String>, AppError> {
    if events.is_empty() || events.len() > ALLOWED_EVENTS.len() {
        return Err(AppError::BadRequest("webhooks.events_invalid".into()));
    }
    events.sort_unstable();
    events.dedup();
    if events
        .iter()
        .any(|event| !ALLOWED_EVENTS.contains(&event.as_str()))
    {
        return Err(AppError::BadRequest("webhooks.events_invalid".into()));
    }
    Ok(events)
}

pub fn validate_discord_webhook_url(value: &str) -> Result<Url, AppError> {
    if value.len() > 2_048 {
        return Err(AppError::BadRequest("webhooks.url_invalid".into()));
    }
    let url = Url::parse(value).map_err(|_| AppError::BadRequest("webhooks.url_invalid".into()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("discord.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::BadRequest("webhooks.url_invalid".into()));
    }
    let segments = url
        .path_segments()
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    if segments.len() != 4
        || segments[0] != "api"
        || segments[1] != "webhooks"
        || !segments[2].bytes().all(|byte| byte.is_ascii_digit())
        || segments[2].is_empty()
        || segments[3].len() < 32
        || segments[3].len() > 256
        || !segments[3]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::BadRequest("webhooks.url_invalid".into()));
    }
    Ok(url)
}

pub fn encrypted_url(
    secrets: &SecretStore,
    id: &str,
    url: &str,
) -> Result<(String, String), AppError> {
    validate_discord_webhook_url(url)?;
    secrets.seal(&format!("discord_webhook:{id}:url"), url)
}

async fn run_dispatcher(
    state: AppState,
    client: Client,
    mut events: broadcast::Receiver<ApiEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok(event) if ALLOWED_EVENTS.contains(&event.event_type.as_str()) => {
                    deliver_event(&state.pool, &state.secrets, &client, &event).await;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "webhook dispatcher dropped lagged panel events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

async fn deliver_event(pool: &DbPool, secrets: &SecretStore, client: &Client, event: &ApiEvent) {
    let targets = match sqlx::query_as::<_, DeliveryTarget>(
        "SELECT id, url_nonce, url_ciphertext, events FROM discord_webhooks \
         WHERE enabled = 1 ORDER BY created_at LIMIT ?",
    )
    .bind(MAX_WEBHOOKS)
    .fetch_all(pool)
    .await
    {
        Ok(targets) => targets,
        Err(error) => {
            tracing::warn!(%error, "could not load Discord webhook subscriptions");
            return;
        }
    };
    let content = match &event.instance_id {
        Some(instance_id) => format!(
            "DmxServerManager: `{}` pour l'instance `{instance_id}`.",
            event.event_type
        ),
        None => format!("DmxServerManager: `{}`.", event.event_type),
    };
    stream::iter(targets.into_iter().filter(|target| {
        target
            .events
            .iter()
            .any(|configured| configured == &event.event_type)
    }))
    .for_each_concurrent(DELIVERY_CONCURRENCY, |target| {
        deliver_one(pool, secrets, client, target, &content)
    })
    .await;
}

async fn deliver_one(
    pool: &DbPool,
    secrets: &SecretStore,
    client: &Client,
    target: DeliveryTarget,
    content: &str,
) {
    let result = async {
        let plaintext = secrets.open(
            &format!("discord_webhook:{}:url", target.id),
            &target.url_nonce,
            &target.url_ciphertext,
        )?;
        let url = validate_discord_webhook_url(&plaintext)?;
        let response = client
            .post(url)
            .json(&DiscordMessage {
                username: "DmxServerManager",
                content,
                allowed_mentions: DiscordAllowedMentions { parse: [] },
            })
            .send()
            .await
            .map_err(|_| AppError::Internal("webhook delivery failed".into()))?;
        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "webhook HTTP status {}",
                response.status().as_u16()
            )));
        }
        Ok::<(), AppError>(())
    }
    .await;
    let now = chrono::Utc::now().to_rfc3339();
    let error_code = result.as_ref().err().map(|_| "delivery_failed");
    if let Err(error) = sqlx::query(
        "UPDATE discord_webhooks SET last_delivery_at = ?, last_error_code = ? WHERE id = ?",
    )
    .bind(now)
    .bind(error_code)
    .bind(&target.id)
    .execute(pool)
    .await
    {
        tracing::warn!(webhook_id = %target.id, %error, "could not persist webhook delivery status");
    }
    if result.is_err() {
        tracing::warn!(webhook_id = %target.id, "Discord webhook delivery failed");
    }
}

fn webhook_client() -> Result<Client, AppError> {
    Client::builder()
        .redirect(Policy::none())
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| AppError::Internal("webhook client initialization failed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_urls_are_exact_and_cannot_be_used_for_ssrf() {
        let valid = "https://discord.com/api/webhooks/123456789012345678/abcdefghijklmnopqrstuvwxyzABCDEF0123456789";
        assert!(validate_discord_webhook_url(valid).is_ok());
        for invalid in [
            "http://discord.com/api/webhooks/1/abcdefghijklmnopqrstuvwxyzABCDEF0123456789",
            "https://discord.com.evil.example/api/webhooks/1/abcdefghijklmnopqrstuvwxyzABCDEF0123456789",
            "https://discord.com:444/api/webhooks/1/abcdefghijklmnopqrstuvwxyzABCDEF0123456789",
            "https://discord.com/api/webhooks/1/abcdefghijklmnopqrstuvwxyzABCDEF0123456789?next=https://evil.example",
            "https://evil.example/api/webhooks/1/abcdefghijklmnopqrstuvwxyzABCDEF0123456789",
        ] {
            assert!(validate_discord_webhook_url(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn webhook_events_are_closed_and_deduplicated() {
        assert_eq!(
            validate_event_set(vec!["server.started".into(), "server.started".into()]).unwrap(),
            vec!["server.started"]
        );
        assert!(validate_event_set(vec!["arbitrary.event".into()]).is_err());
    }
}
