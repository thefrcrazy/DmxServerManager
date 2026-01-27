use actix_web::{web, HttpResponse, Result};
use serde::{Deserialize, Serialize};

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::ProcessManager;

#[derive(Debug, Serialize)]
pub struct WebhookTestResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
struct ServerInfo {
    name: String,
    status: String,
    game_type: String,
}

#[derive(Debug, Deserialize)]
pub struct TestWebhookRequest {
    pub webhook_url: Option<String>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/webhook")
            .route("/test", web::post().to(test_webhook)),
    );
}

async fn test_webhook(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    body: web::Json<TestWebhookRequest>,
) -> Result<HttpResponse, AppError> {
    // Get webhook URL from request or settings
    let webhook_url = if let Some(url) = &body.webhook_url {
        url.clone()
    } else {
        // Fetch from settings
        let settings: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = 'webhook_url'")
            .fetch_optional(pool.get_ref())
            .await?;
        
        settings
            .map(|s| s.0)
            .ok_or_else(|| AppError::BadRequest("No webhook URL configured".into()))?
    };

    if webhook_url.is_empty() {
        return Err(AppError::BadRequest("Webhook URL is empty".into()));
    }

    // Fetch all servers with their status
    let servers: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT name, game_type, id FROM servers ORDER BY name"
    )
    .fetch_all(pool.get_ref())
    .await?;

    let mut server_list: Vec<ServerInfo> = Vec::new();
    let mut online_count = 0;
    let mut offline_count = 0;

    for (name, game_type, id) in servers {
        let status = if pm.is_running(&id) {
            online_count += 1;
            "🟢 En ligne".to_string()
        } else {
            offline_count += 1;
            "🔴 Arrêté".to_string()
        };

        server_list.push(ServerInfo {
            name,
            status,
            game_type,
        });
    }

    // Get system stats
    let sys_info = {
        use sysinfo::{System, Disks};
        let mut sys = System::new_all();
        sys.refresh_all();
        
        let cpu = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / sys.cpus().len().max(1) as f32;
        let ram_total = sys.total_memory();
        let ram_used = sys.used_memory();
        
        let disks = Disks::new_with_refreshed_list();
        let mut disk_total = 0;
        let mut disk_available = 0;
        for disk in disks.list() {
            disk_total += disk.total_space();
            disk_available += disk.available_space();
        }
        let disk_used = disk_total - disk_available;
        
        (cpu, ram_used, ram_total, disk_used, disk_total)
    };

    // Build Discord rich embed
    let now = chrono::Local::now();
    let embed = serde_json::json!({
        "author": {
            "name": "Draveur Manager",
            "icon_url": "https://cdn.discordapp.com/emojis/1199040367842689154.webp"
        },
        "title": "📊 État du Système",
        "color": 0x3A82F6,
        "fields": [
            {
                "name": "Système",
                "value": format!("CPU: **{:.1}%**\nRAM: **{:.1}/{:.1} GB**\nDisk: **{:.1}/{:.1} GB**", 
                    sys_info.0,
                    sys_info.1 as f64 / 1024.0 / 1024.0 / 1024.0,
                    sys_info.2 as f64 / 1024.0 / 1024.0 / 1024.0,
                    sys_info.3 as f64 / 1024.0 / 1024.0 / 1024.0,
                    sys_info.4 as f64 / 1024.0 / 1024.0 / 1024.0
                ),
                "inline": false
            },
            {
                "name": format!("Serveurs ({}/{})", online_count, online_count + offline_count),
                "value": if server_list.is_empty() { 
                    "Aucun serveur détecté.".to_string() 
                } else { 
                    server_list.iter()
                        .map(|s| format!("{} **{}**", s.status, s.name))
                        .collect::<Vec<_>>()
                        .join("\n")
                },
                "inline": false
            }
        ],
        "footer": {
            "text": format!("Dernière mise à jour • {} à {}", 
                now.format("Aujourd'hui"),
                now.format("%H:%M")
            )
        },
        "timestamp": chrono::Utc::now().to_rfc3339()
    });

    // Use update_status_message to edit existing message or create new one
    // Note: We use the update_status_message from discord_service to persist and update the message
    crate::services::discord_service::update_status_message(pool.get_ref(), embed).await
        .map_err(|e| AppError::Internal(format!("Failed to update status message: {}", e)))?;

    Ok(HttpResponse::Ok().json(WebhookTestResponse {
        success: true,
        message: "Status message updated successfully".to_string(),
    }))
}
