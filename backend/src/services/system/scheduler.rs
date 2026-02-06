use std::time::Duration;
use tokio::time;
use sysinfo::{System, RefreshKind, CpuRefreshKind, MemoryRefreshKind};
use chrono::{Utc, DateTime, Local};
use tracing::{info, error};
use std::str::FromStr;
use cron::Schedule;

use crate::core::database::DbPool;
use crate::services::game::manager::ProcessManager;
use crate::services::system::discord;

pub fn start(pool: DbPool, process_manager: ProcessManager) {
    let pool_clone = pool.clone();
    let pm_clone = process_manager.clone();

    tokio::spawn(async move {
        time::sleep(Duration::from_secs(5)).await;
        
        let mut interval = time::interval(Duration::from_secs(20));
        
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
        );

        loop {
            interval.tick().await;
            
            if let Err(e) = run_status_update(&pool_clone, &mut sys, &pm_clone).await {
                error!("Error in status scheduler: {e}");
            }
        }
    });

    start_task_scheduler(pool, process_manager);
}

fn start_task_scheduler(pool: DbPool, pm: ProcessManager) {
    tokio::spawn(async move {
        // Run every minute at :00
        let mut interval = time::interval(Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            if let Err(e) = check_and_run_tasks(&pool, &pm).await {
                error!("Error in task scheduler: {e}");
            }
        }
    });
}

async fn check_and_run_tasks(pool: &DbPool, pm: &ProcessManager) -> anyhow::Result<()> {
    let now = Local::now();
    let now_time = now.format("%H:%M").to_string();
    
    let schedules: Vec<crate::api::servers::models::ScheduleRow> = sqlx::query_as(
        "SELECT * FROM schedules WHERE enabled = 1"
    )
    .fetch_all(pool)
    .await?;

    for s in schedules {
        let should_run = match s.task_type.as_str() {
            "basic" => {
                if let Some(time) = &s.time {
                    time == &now_time
                } else {
                    false
                }
            },
            "cron" => {
                if let Some(expr) = &s.cron_expression {
                    if let Ok(schedule) = Schedule::from_str(expr) {
                        // Check if the current minute is a match
                        // Schedule::upcoming returns the NEXT occurrences. 
                        // To check if 'now' matches, we check if the next occurrence is within the next 61 seconds 
                        // from 1 second ago.
                        let one_sec_ago = now - chrono::Duration::seconds(1);
                        if let Some(next) = schedule.after(&one_sec_ago).next() {
                            // Convert both to same timezone for comparison
                            let next_local: DateTime<Local> = next;
                            next_local.format("%Y-%m-%d %H:%M").to_string() == now.format("%Y-%m-%d %H:%M").to_string()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            },
            _ => false,
        };

        if should_run {
            info!("Running scheduled task '{}' for server {}", s.name, s.server_id);
            
            let server: Option<crate::api::servers::models::ServerRow> = sqlx::query_as("SELECT * FROM servers WHERE id = ?")
                .bind(&s.server_id)
                .fetch_optional(pool)
                .await?;

            if let Some(srv) = server {
                let config_json = srv.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
                
                match s.action.as_str() {
                    "start" => { 
                        let _ = pm.start(
                            &srv.id,
                            &srv.executable_path,
                            &srv.working_dir,
                            srv.java_path.as_deref(),
                            srv.min_memory.as_deref(),
                            srv.max_memory.as_deref(),
                            srv.extra_args.as_deref(),
                            config_json.as_ref(),
                            &srv.game_type,
                            srv.nice_level
                        ).await; 
                    },
                    "stop" => { let _ = pm.stop(&s.server_id).await; },
                    "restart" => { 
                        let _ = pm.restart(
                            &srv.id,
                            &srv.executable_path,
                            &srv.working_dir,
                            srv.java_path.as_deref(),
                            srv.min_memory.as_deref(),
                            srv.max_memory.as_deref(),
                            srv.extra_args.as_deref(),
                            config_json.as_ref(),
                            &srv.game_type,
                            srv.nice_level
                        ).await; 
                    },
                    "backup" => {
                        let filename = format!("backup_{}_{}.tar.gz", s.server_id, Utc::now().format("%Y%m%d_%H%M%S"));
                        let backup_path = format!("backups/{filename}");
                        
                        // Use the async backup service
                        let working_dir = srv.working_dir.clone();
                        let backup_path_clone = backup_path.clone();
                        let pool_clone = pool.clone();
                        let s_id = s.server_id.clone();
                        
                        tokio::spawn(async move {
                            if let Ok(size) = crate::services::system::backup::create_archive(working_dir, backup_path_clone).await {
                                 let _ = sqlx::query("INSERT INTO backups (id, server_id, filename, size_bytes, created_at) VALUES (?, ?, ?, ?, ?)")
                                    .bind(uuid::Uuid::new_v4().to_string())
                                    .bind(&s_id)
                                    .bind(&filename)
                                    .bind(size as i64)
                                    .bind(Utc::now().to_rfc3339())
                                    .execute(&pool_clone)
                                    .await;
                            }
                        });
                    },
                    _ => {}
                }
            } else {
                error!("Server {} not found for scheduled task {}", s.server_id, s.id);
            }

            if s.delete_after != 0 {
                sqlx::query("DELETE FROM schedules WHERE id = ?").bind(&s.id).execute(pool).await?;
            }
        }
    }

    Ok(())
}

async fn run_status_update(pool: &DbPool, sys: &mut System, pm: &ProcessManager) -> anyhow::Result<()> {
    sys.refresh_cpu_all();
    sys.refresh_memory();
    
    let cpu_usage = sys.global_cpu_usage();
    let ram_used = sys.used_memory();
    let ram_total = sys.total_memory();
    
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
    
    if !found_root && !disks.list().is_empty() {
        let disk = &disks.list()[0];
        disk_total = disk.total_space();
        disk_available = disk.available_space();
    }
    
    let disk_used = disk_total - disk_available;
    
    let ram_used_gb = ram_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let ram_total_gb = ram_total as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_used_gb = disk_used as f64 / 1024.0 / 1024.0 / 1024.0;
    let disk_total_gb = disk_total as f64 / 1024.0 / 1024.0 / 1024.0;

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
            let mut details = String::new();
            
            let online_players = pm.get_online_players(&id).await
                .map(|p| p.len()).unwrap_or(0);
                
            let mut max_players = 100;
            if let Some(conf) = config_str.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok()) {
                if let Some(mp) = conf.get("MaxPlayers").and_then(|v| v.as_u64()) {
                     max_players = mp as usize;
                }
            }
            
            details.push_str(&format!("👥 {online_players}/{max_players}"));
            
            if let Some(started_at) = pm.get_server_started_at(&id).await {
                let duration = Utc::now().signed_duration_since(started_at);
                let hours = duration.num_hours();
                let minutes = duration.num_minutes() % 60;
                details.push_str(&format!(" • ⏱️ {hours}h{minutes}m"));
            }
            
            if let Some(pid_u32) = pm.get_server_pid(&id).await {
                 let pid = sysinfo::Pid::from(pid_u32 as usize);
                 sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
                 
                 if let Some(proc) = sys.process(pid) {
                     let cpu = proc.cpu_usage();
                     let mem_bytes = proc.memory();
                     let mem_mb = mem_bytes as f64 / 1024.0 / 1024.0;
                     let mem_gb = mem_mb / 1024.0;
                     
                     if mem_gb >= 1.0 {
                         details.push_str(&format!(" • 📊 CPU: {cpu:.1}% RAM: {mem_gb:.1} GB"));
                     } else {
                         details.push_str(&format!(" • 📊 CPU: {cpu:.1}% RAM: {mem_mb:.0} MB"));
                     }
                 }
            }

            server_lines.push(format!("🟢 **{name}**\n╰ {details}"));
        } else {
            server_lines.push(format!("🔴 **{name}**"));
        }
    }
    
    let server_list_str = if server_lines.is_empty() {
        "Aucun serveur détecté.".to_string()
    } else {
        let mut result = server_lines.join("\n\n");
        if result.len() > 1000 {
            result.truncate(1000);
            result.push_str("\n...");
        }
        result
    };

    let now = Local::now();
    let embed = serde_json::json!({
        "author": {
            "name": "Draveur Manager",
            "icon_url": "https://raw.githubusercontent.com/thefrcrazy/Draveur-Manager/refs/heads/main/frontend/public/draveur-manager-logo.png"
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

    discord::update_status_message(pool, embed).await?;

    Ok(())
}