use crate::core::database::DbPool;
use anyhow::Result;
use serde_json::Value;

/// Send a standard Discord webhook notification (fire-and-forget)
pub async fn send_notification(
    pool: &DbPool,
    title: &str,
    description: &str,
    color: u32,
    server_name: Option<&str>,
    override_webhook_url: Option<&str>,
) {
    // Determine which URL to use
    let url = if let Some(u) = override_webhook_url {
        if u.is_empty() { return; }
        u.to_string()
    } else {
        // Fallback to global setting ONLY if no specific server name logic implies otherwise? 
        // For now, standard fallback.
        let val: Option<(String,)> = 
            sqlx::query_as("SELECT value FROM settings WHERE key = 'webhook_url'")
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        
        match val {
            Some((u,)) => u,
            None => return,
        }
    };

    if url.is_empty() {
        return;
    }

    // Build embed
    let mut embed = serde_json::json!({
        "title": title,
        "description": description,
        "color": color,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "footer": {
            "text": format!("Draveur Manager v{}", env!("CARGO_PKG_VERSION"))
        }
    });

    if let Some(name) = server_name {
        embed["fields"] = serde_json::json!([{
            "name": "Serveur",
            "value": name,
            "inline": true
        }]);
    }

    let payload = serde_json::json!({
        "embeds": [embed]
    });

    let client = reqwest::Client::new();
    let _ = client.post(&url).json(&payload).send().await;
}

/// Update or Create the persistent Status Message
pub async fn update_status_message(
    pool: &DbPool,
    embed: Value,
) -> Result<()> {
    // 1. Get Webhook URL
    let webhook_url_setting: Option<(String,)> = 
        sqlx::query_as("SELECT value FROM settings WHERE key = 'webhook_url'")
            .fetch_optional(pool)
            .await?;

    let Some(mut url) = webhook_url_setting.map(|s| s.0) else {
        return Ok(()); // Not configured
    };
    if url.is_empty() {
        return Ok(());
    }

    // Sanitize URL (remove trailing slashes)
    if url.ends_with('/') {
        url = url.trim_end_matches('/').to_string();
    }

    // 2. Get saved Message ID
    let msg_id_opt: Option<(String,)> = 
        sqlx::query_as("SELECT value FROM settings WHERE key = 'discord_status_message_id'")
            .fetch_optional(pool)
            .await?;
    
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "embeds": [embed]
    });

    // 3. Try EDIT if ID exists
    if let Some((msg_id,)) = msg_id_opt {
        if !msg_id.is_empty() {
            let edit_url = format!("{url}/messages/{msg_id}");
            tracing::debug!("Attempting to update Discord status message: {}", msg_id);
            let resp = client.patch(&edit_url).json(&payload).send().await?;

            if resp.status().is_success() {
                return Ok(());
            }

            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_else(|_| "Unknown error".into());
            
            // IF error is 404 (Message Deleted), we fall through to create a new one
            // Other errors (429 Rate Limit, etc.) should stay as errors
            if status == reqwest::StatusCode::NOT_FOUND {
                tracing::warn!("Discord status message {} not found (404), creating a new one.", msg_id);
            } else {
                tracing::error!("Failed to update Discord status: {} - {}", status, error_body);
                return Err(anyhow::anyhow!("Discord API error: {status}"));
            }
        }
    }

    // 4. CREATE new message (wait=true to get ID)
    tracing::info!("Creating a new persistent Discord status message...");
    let create_url = format!("{url}?wait=true");
    let resp = client.post(&create_url).json(&payload).send().await?;

    if resp.status().is_success() {
        let json: Value = resp.json().await?;
        if let Some(new_id) = json["id"].as_str() {
            // 5. Save new ID
            sqlx::query("INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('discord_status_message_id', ?, CURRENT_TIMESTAMP)")
                .bind(new_id)
                .execute(pool)
                .await?;
            tracing::info!("Saved new Discord status message ID: {}", new_id);
        }
    } else {
        let status = resp.status();
        let error_body = resp.text().await.unwrap_or_else(|_| "Unknown error".into());
        tracing::error!("Failed to create Discord status message: {} - {}", status, error_body);
        return Err(anyhow::anyhow!("Discord API error: {status}"));
    }

    Ok(())
}

// Webhook colors
pub const COLOR_SUCCESS: u32 = 0x10B981; // Green
pub const COLOR_ERROR: u32 = 0xEF4444;   // Red
