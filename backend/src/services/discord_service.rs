use crate::db::DbPool;
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
            "text": format!("Kweebec Manager v{}", env!("CARGO_PKG_VERSION"))
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
    let webhook_url: Option<(String,)> = 
        sqlx::query_as("SELECT value FROM settings WHERE key = 'webhook_url'")
            .fetch_optional(pool)
            .await?;

    let Some((url,)) = webhook_url else {
        return Ok(()); // Not configured
    };
    if url.is_empty() {
        return Ok(());
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
        let edit_url = format!("{}/messages/{}", url, msg_id);
        let resp = client.patch(&edit_url).json(&payload).send().await?;

        if resp.status().is_success() {
            return Ok(());
        }
        // If edit failed (e.g. 404 message deleted), fallthrough to create new one
    }

    // 4. CREATE new message (wait=true to get ID)
    let create_url = format!("{}?wait=true", url);
    let resp = client.post(&create_url).json(&payload).send().await?;

    if resp.status().is_success() {
        let json: Value = resp.json().await?;
        if let Some(new_id) = json["id"].as_str() {
            // 5. Save new ID
            // Check if key exists first for UPSERT logic (SQLite specific)
            let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM settings WHERE key = 'discord_status_message_id'")
                .fetch_one(pool)
                .await?;

            if exists.0 > 0 {
                sqlx::query("UPDATE settings SET value = ?, updated_at = CURRENT_TIMESTAMP WHERE key = 'discord_status_message_id'")
                    .bind(new_id)
                    .execute(pool)
                    .await?;
            } else {
                sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES ('discord_status_message_id', ?, CURRENT_TIMESTAMP)")
                    .bind(new_id)
                    .execute(pool)
                    .await?;
            }
        }
    }

    Ok(())
}

// Webhook colors
pub const COLOR_SUCCESS: u32 = 0x10B981; // Green
pub const COLOR_ERROR: u32 = 0xEF4444;   // Red  
#[allow(dead_code)]
pub const COLOR_INFO: u32 = 0x3B82F6;    // Blue
#[allow(dead_code)]
pub const COLOR_WARNING: u32 = 0xF59E0B; // Orange
#[allow(dead_code)]
pub const COLOR_ACCENT: u32 = 0xFF591E;  // Kweebec Orange
