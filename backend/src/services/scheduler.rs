use std::time::Duration;
use actix_web::web;
use tokio::time;
use sysinfo::{System, RefreshKind, CpuRefreshKind, MemoryRefreshKind};
use crate::db::DbPool;
use crate::services::process_manager::ProcessManager;
use crate::services::discord_service;

pub fn start(pool: web::Data<DbPool>, process_manager: web::Data<ProcessManager>) {
    tokio::spawn(async move {
        // Wait a bit for server start
        time::sleep(Duration::from_secs(5)).await;
        
        // Loop interval 1 minute
        let mut interval = time::interval(Duration::from_secs(60));
        
        // System info instance
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
        );

        loop {
            interval.tick().await;
            
            if let Err(e) = run_status_update(&pool, &mut sys, &process_manager).await {
                eprintln!("Error in status scheduler: {}", e);
            }
        }
    });
}

async fn run_status_update(pool: &DbPool, sys: &mut System, pm: &ProcessManager) -> anyhow::Result<()> {
    // 1. Refresh System Stats
    sys.refresh_cpu_all();
    sys.refresh_memory();
    // sleep tiny bit for CPU usage calculation (only if needed/not handled by loop)
    // The loop interval is 15s in start(), so refresh should be fine, but sysinfo needs delay between refreshes for CPU.
    // Since we call this every 15s, the previous call context is lost but the System struct persists.
    // sys.refresh_cpu_all() computes usage since LAST refresh. So it should work fine without sleep if called periodically.
    // However, firs call might be 0. Let's keep it simple.
    
    let cpu_usage = sys.global_cpu_usage();
    let ram_used = sys.used_memory();
    let ram_total = sys.total_memory();
    
    // Disks
    // Disks - filter for root only (like API)
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut disk_total = 0;
    let mut disk_available = 0;
    let mut found_root = false;
    
    for disk in disks.list() {
        if disk.mount_point() == std::path::Path::new("/") {
            disk_total = disk.total_space();
            disk_available = disk.available_space();
            found_root = true;
            break;
        }
    }
    
    // Fallback if no root found
    if !found_root && !disks.list().is_empty() {
        let disk = &disks.list()[0];
        disk_total = disk.total_space();
        disk_available = disk.available_space();
    }
    
    let disk_used = disk_total - disk_available;
    
    // Format stats
    let ram_used_gb = ram_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let ram_total_gb = ram_total as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_used_gb = disk_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_total_gb = disk_total as f64 / 1024.0 / 1024.0 / 1024.0;

    // 2. Get Servers Info
    // Note: Instead of reading filesystem manually, we should ideally use the DB or consistent method.
    // But sticking to existing logic for now OR aligning with webhook.rs which uses DB servers table?
    // webhook.rs uses DB. scheduler.rs used generic FS scan. 
    // Let's use DB to be consistent with webhook.rs if possible.
    // But scheduler.rs imports suggest we have access to pool.
    
    // Let's use the DB servers table like webhook.rs to ensure "Serveurs (X/Y)" matches the dashboard.
    // Reading FS is risky if names don't match IDs perfectly.
    
    // 2. Get Servers Info
    // Fetch config as well to get MaxPlayers
    let servers: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT name, id, config FROM servers ORDER BY name"
    )
    .fetch_all(pool)
    .await?;

    let mut total_servers = 0;
    let mut online_servers = 0;
    let mut server_lines = Vec::new();

    for (name, id, config_str) in servers {
        total_servers += 1;
        let is_running = pm.is_running(&id);
        
        if is_running {
            online_servers += 1;
            
            // Get rich stats
            let mut details = String::new();
            
            // 1. Players
            let online_players = pm.get_online_players(&id).await
                .map(|p| p.len()).unwrap_or(0);
                
            // Parse MaxPlayers
            let mut max_players = 100; // Default
            if let Some(conf) = config_str.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok()) {
                if let Some(mp) = conf.get("MaxPlayers").and_then(|v| v.as_u64()) {
                     max_players = mp as usize;
                }
            }
            
            details.push_str(&format!("👥 {}/{}", online_players, max_players));
            
            // 2. Uptime
            if let Some(started_at) = pm.get_server_started_at(&id).await {
                let duration = chrono::Utc::now().signed_duration_since(started_at);
                let hours = duration.num_hours();
                let minutes = duration.num_minutes() % 60;
                details.push_str(&format!(" • ⏱️ {}h{}m", hours, minutes));
            }
            
            // 3. CPU/RAM
            if let Some(pid_u32) = pm.get_server_pid(&id).await {
                 let pid = sysinfo::Pid::from(pid_u32 as usize);
                 // Need to refresh specific process
                 // sysinfo 0.30+
                 sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
                 
                 if let Some(proc) = sys.process(pid) {
                     let cpu = proc.cpu_usage();
                     let mem_bytes = proc.memory();
                     let mem_mb = mem_bytes as f64 / 1024.0 / 1024.0;
                     let mem_gb = mem_mb / 1024.0;
                     
                     if mem_gb >= 1.0 {
                         details.push_str(&format!(" • 📊 CPU: {:.1}% RAM: {:.1} GB", cpu, mem_gb));
                     } else {
                         details.push_str(&format!(" • 📊 CPU: {:.1}% RAM: {:.0} MB", cpu, mem_mb));
                     }
                 }
            }

            server_lines.push(format!("🟢 **{}**\n╰ {}", name, details));
        } else {
            server_lines.push(format!("🔴 **{}**", name));
        }
    }
    
    let server_list_str = if server_lines.is_empty() {
        "Aucun serveur détecté.".to_string()
    } else {
        let mut result = server_lines.join("\n\n"); // Double newline for spacing
        // Basic truncation check
        if result.len() > 1000 {
            result.truncate(1000);
            result.push_str("\n...");
        }
        result
    };

    // 3. Build Rich Embed
    let now = chrono::Local::now();
    let embed = serde_json::json!({
        "author": {
            "name": "Kweebec Manager",
            "icon_url": "https://raw.githubusercontent.com/thefrcrazy/Kweebec-Manager/refs/heads/main/frontend/public/kweebec-manager-logo.png"
        },
        "title": "📊 État du Système",
        "color": 0x3A82F6,
        "fields": [
            {
                "name": "Système",
                "value": format!("CPU: **{:.1}%**\nRAM: **{:.1}/{:.1} GB**\nDisk: **{:.1}/{:.1} GB**", 
                    cpu_usage,
                    ram_used_gb, ram_total_gb,
                    disk_used_gb, disk_total_gb
                ),
                "inline": false
            },
            {
                "name": format!("Serveurs ({}/{})", online_servers, total_servers),
                "value": server_list_str,
                "inline": false
            }
        ],
        "footer": {
            "text": format!("Dernière mise à jour • {} à {}", 
                now.format("Aujourd'hui"),
                now.format("%H:%M")
            )
        }
    });

    // 4. Update Message
    discord_service::update_status_message(pool, embed).await?;

    Ok(())
}
