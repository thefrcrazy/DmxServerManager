use actix_web::{web, HttpResponse, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use tracing::{info, error, warn};
use std::path::Path;

use crate::db::DbPool;
use walkdir::WalkDir;
use crate::utils::memory::{parse_memory_to_bytes, calculate_total_memory};
use crate::error::AppError;
use crate::services::ProcessManager;
use crate::templates;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub game_type: String,
    pub executable_path: String,
    pub working_dir: String,
    pub java_path: Option<String>,
    pub min_memory: Option<String>,
    pub max_memory: Option<String>,
    pub extra_args: Option<String>,
    pub config: Option<serde_json::Value>,
    pub auto_start: Option<bool>,
    
    // New fields
    pub backup_enabled: Option<bool>,
    pub backup_frequency: Option<u32>,
    pub backup_max_backups: Option<u32>,
    pub backup_prefix: Option<String>,
    pub discord_username: Option<String>,
    pub discord_avatar: Option<String>,
    pub discord_webhook_url: Option<String>,
    pub discord_notifications: Option<serde_json::Value>,
    pub logs_retention_days: Option<u32>,
    pub watchdog_enabled: Option<bool>,
    
    // Server settings
    pub auth_mode: Option<String>,
    pub bind_address: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct Player {
    pub name: String,
    pub is_online: bool,
    pub last_seen: String,
    pub is_op: bool,
    pub is_banned: bool,
    pub is_whitelisted: bool,
}

#[derive(Debug, Serialize)]
pub struct ServerResponse {
    pub id: String,
    pub name: String,
    pub game_type: String,
    pub status: String,
    pub executable_path: String,
    pub working_dir: String,
    pub java_path: Option<String>,
    pub min_memory: Option<String>,
    pub max_memory: Option<String>,
    pub extra_args: Option<String>,
    pub config: Option<serde_json::Value>,
    pub auto_start: bool,
    pub created_at: String,
    pub updated_at: String,
    pub dir_exists: bool,
    pub players: Option<Vec<Player>>,
    pub max_players: Option<u32>,
    pub port: Option<u16>,
    pub bind_address: Option<String>,
    
    // New fields
    pub backup_enabled: bool,
    pub backup_frequency: u32,
    pub backup_max_backups: u32,
    pub backup_prefix: String,
    pub discord_username: Option<String>,
    pub discord_avatar: Option<String>,
    pub discord_webhook_url: Option<String>,
    pub discord_notifications: Option<serde_json::Value>,
    pub logs_retention_days: u32,
    pub watchdog_enabled: bool,
    pub auth_mode: String,

    pub cpu_usage: f32,
    pub memory_usage_bytes: u64,
    pub max_memory_bytes: u64,
    pub max_heap_bytes: u64,
    pub disk_usage_bytes: u64,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(FromRow)]
struct PlayerRow {
    player_name: String,
    is_online: i32,
    last_seen: String,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/servers")
            .route("", web::get().to(list_servers))
            .route("", web::post().to(create_server))
            .route("/{id}", web::get().to(get_server))
            .route("/{id}", web::put().to(update_server))
            .route("/{id}", web::delete().to(delete_server))
            .route("/{id}/start", web::post().to(start_server))
            .route("/{id}/stop", web::post().to(stop_server))
            .route("/{id}/restart", web::post().to(restart_server))
            .route("/{id}/kill", web::post().to(kill_server))
            .route("/{id}/reinstall", web::post().to(reinstall_server))
            .route("/{id}/command", web::post().to(send_command))
            // Files API
            .route("/{id}/files", web::get().to(list_server_files))
            .route("/{id}/files/read", web::get().to(read_server_file))
            .route("/{id}/files/write", web::post().to(write_server_file))
            .route("/{id}/files/delete", web::post().to(delete_server_file)),
    );
}

async fn list_servers(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
) -> Result<HttpResponse, AppError> {
    let servers: Vec<ServerRow> = sqlx::query_as(
        "SELECT * FROM servers"
    )
    .fetch_all(pool.get_ref())
    .await?;

    let mut responses = Vec::new();
    for s in servers {
        // Check if the working directory exists
        let dir_exists = Path::new(&s.working_dir).exists();
        let is_running = pm.is_running(&s.id);
        
        let status = if !dir_exists { 
            "missing" 
        } else if pm.is_installing(&s.id) {
            if pm.is_auth_required(&s.id) { "auth_required" } else { "installing" }
        } else if is_running {
            "running"
        } else {
            "stopped"
        };

        // For list view, we just return currently online players as simple Player objects
        let mut players_vec = Vec::new();
        if is_running {
            if let Some(online) = pm.get_online_players(&s.id).await {
                for p_name in online {
                     players_vec.push(Player {
                         name: p_name,
                         is_online: true,
                         last_seen: Utc::now().to_rfc3339(),
                         is_op: false,
                         is_banned: false,
                         is_whitelisted: false,
                     });
                }
            }
        }
        let players = if players_vec.is_empty() { None } else { Some(players_vec) };

        // Parse max_players (DB config or file config)
        let config_json = s.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
        let mut max_players = config_json.as_ref()
            .and_then(|c| c.get("MaxPlayers"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        if max_players.is_none() {
            // Try reading from config.json
            let config_path = Path::new(&s.working_dir).join("server").join("config.json");
            if let Ok(content) = fs::read_to_string(config_path).await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
                }
            }
        }

        let started_at = pm.get_server_started_at(&s.id).await;
        let (cpu, mem, mut disk) = pm.get_metrics_data(&s.id).await;

        // Fallback for offline disk usage
        if disk == 0 {
            disk = WalkDir::new(&s.working_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum();
        }

        let heap_bytes = parse_memory_to_bytes(s.max_memory.as_deref().unwrap_or("4G"));
        let total_bytes = calculate_total_memory(heap_bytes);
        
        // Parse notifications JSON
        let notifications = s.discord_notifications.as_ref()
            .and_then(|n| serde_json::from_str(n).ok());

        responses.push(ServerResponse {
            id: s.id,
            name: s.name,
            game_type: s.game_type,
            status: status.to_string(),
            executable_path: s.executable_path,
            working_dir: s.working_dir,
            java_path: s.java_path,
            min_memory: s.min_memory,
            max_memory: s.max_memory,
            extra_args: s.extra_args,
            config: config_json.clone(),
            auto_start: s.auto_start != 0,
            created_at: s.created_at,
            updated_at: s.updated_at,
            dir_exists,
            players,
            max_players,
            port: Some(s.port as u16),
            bind_address: Some(s.bind_address),
            
            backup_enabled: s.backup_enabled != 0,
            backup_frequency: s.backup_frequency as u32,
            backup_max_backups: s.backup_max_backups as u32,
            backup_prefix: s.backup_prefix,
            discord_username: s.discord_username,
            discord_avatar: s.discord_avatar,
            discord_webhook_url: s.discord_webhook_url,
            discord_notifications: notifications,
            logs_retention_days: s.logs_retention_days as u32,
            watchdog_enabled: s.watchdog_enabled != 0,
            auth_mode: s.auth_mode,

            cpu_usage: cpu,
            memory_usage_bytes: mem,
            max_memory_bytes: total_bytes,
            max_heap_bytes: heap_bytes,
            disk_usage_bytes: disk,
            started_at,
        });
    }

    Ok(HttpResponse::Ok().json(responses))
}

async fn create_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    body: web::Json<CreateServerRequest>,
) -> Result<HttpResponse, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let auto_start = body.auto_start.unwrap_or(false) as i32;

    // Create server directory with ID subdirectory
    // Structure:
    //   {uuid}/Server/           - Hytale server binary (and dependencies)
    //   {uuid}/logs/             - Logs
    //   {uuid}/universe/         - World data
    //   {uuid}/config.json       - Hytale config
    let server_base_path = Path::new(&body.working_dir).join(&id);
    // Create base directories
    let directories = [
        &server_base_path,
    ];

    for dir in directories {
        if let Err(e) = fs::create_dir_all(dir).await {
            return Err(AppError::Internal(format!(
                "Failed to create directory {:?}: {}",
                dir, e
            )));
        }
    }

    info!("Created server directory structure at {:?}", server_base_path);

    // Extract configuration from request body
    let config_value = body.config.as_ref();
    let server_name = &body.name;

    let auth_mode = config_value
        .and_then(|c| c.get("auth_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("authenticated");
    let bind_address = config_value
        .and_then(|c| c.get("bind_address"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0.0");
    let port: u16 = config_value
        .and_then(|c| c.get("port"))
        .and_then(|v| v.as_u64())
        .unwrap_or(5520) as u16;

    // Auto-download server jar if requested
    let mut final_executable = body.executable_path.clone();
    // Use root as the install path for Hytale Downloader - it produces "Server/"
    let install_path = server_base_path.clone();

    if body.game_type == "hytale" {
        spawn_hytale_installation(pool.get_ref().clone(), pm.get_ref().clone(), id.clone(), install_path.clone());
        
        // We set executable path tentatively to the expected location
        final_executable = "Server/HytaleServer.jar".to_string(); 
    }

    let config_str = body.config.as_ref().map(|c| c.to_string());

    // Store server in database with the correct paths
    let actual_working_dir = server_base_path.to_str().unwrap_or(&body.working_dir);
    // Executable path is relative to working_dir (which is root)
    let actual_executable_str = &final_executable;

    // Generate and write config.json (Hytale server config) at ROOT
    let hytale_config = templates::generate_config_json(
        server_name,
        100, 
        auth_mode
    );
    let config_json_path = server_base_path.join("config.json");
    let mut config_file = fs::File::create(&config_json_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to create config.json: {}", e))
    })?;
    config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write config.json: {}", e)))?;

    info!("Generated Hytale config.json for server {}", id);

    // Insert into DB with new columns (using defaults for now)
    sqlx::query(
        "INSERT INTO servers (
            id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at,
            backup_enabled, backup_frequency, backup_max_backups, backup_prefix,
            discord_username, discord_avatar, discord_webhook_url, discord_notifications,
            logs_retention_days, watchdog_enabled,
            auth_mode, bind_address, port
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            1, 30, 7, 'hytale_backup',
            'Hytale Bot', '', '', '{}',
            7, 1,
            ?, ?, ?
        )",
    )
    .bind(&id)
    .bind(&body.name)
    .bind(&body.game_type)
    .bind(actual_executable_str)
    .bind(actual_working_dir)
    .bind(&body.java_path)
    .bind(&body.min_memory)
    .bind(&body.max_memory)
    .bind(&body.extra_args)
    .bind(config_str) // stores raw request config
    .bind(auto_start)
    .bind(&now)
    .bind(&now)
    .bind(auth_mode)
    .bind(bind_address)
    .bind(port)
    .execute(pool.get_ref())
    .await?;

    Ok(HttpResponse::Created().json(serde_json::json!({ 
        "id": id,
        "working_dir": actual_working_dir,
        "message": "Server directory structure created. Download the server files using hytale-downloader."
    })))
}

// Helper to run a command and stream its stdout/stderr to the process manager logs
async fn run_with_logs(
    cmd: &mut tokio::process::Command, 
    pm: ProcessManager, 
    id: String, 
    log_prefix: &str,
    log_file_path: Option<std::path::PathBuf>
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn command: {}", e)),
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut stdout_reader = tokio::io::BufReader::new(stdout);
    let mut stderr_reader = tokio::io::BufReader::new(stderr);

    // Create a shared writer if a path is provided
    let file_writer = if let Some(path) = log_file_path {
        match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await 
        {
            Ok(f) => Some(std::sync::Arc::new(tokio::sync::Mutex::new(f))),
            Err(e) => {
                error!("Failed to open log file: {}", e);
                None
            }
        }
    } else {
        None
    };

    let pm_clone1 = pm.clone();
    let id_clone1 = id.clone();
    let prefix1 = log_prefix.to_string();
    let file_writer1 = file_writer.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        while let Ok(byte) = stdout_reader.read_u8().await {
            if byte == b'\n' || byte == b'\r' {
                if !buffer.is_empty() {
                    let line = String::from_utf8_lossy(&buffer).to_string();
                    // Basic broadcast
                    pm_clone1.broadcast_log(&id_clone1, format!("{}{}", prefix1, line)).await;
                    
                    if let Some(writer) = &file_writer1 {
                        let mut guard = writer.lock().await;
                        // Write the line + newline to file for readability
                        let _ = guard.write_all(line.as_bytes()).await;
                        let _ = guard.write_all(b"\n").await;
                    }
                    buffer.clear();
                }
            } else {
                buffer.push(byte);
            }
        }
    });

    let pm_clone2 = pm.clone();
    let id_clone2 = id.clone();
    let prefix2 = log_prefix.to_string();
    let file_writer2 = file_writer.clone(); 
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        while let Ok(byte) = stderr_reader.read_u8().await {
             if byte == b'\n' || byte == b'\r' {
                if !buffer.is_empty() {
                    let line = String::from_utf8_lossy(&buffer).to_string();
                    pm_clone2.broadcast_log(&id_clone2, format!("{}[ERR] {}", prefix2, line)).await;
                    
                    if let Some(writer) = &file_writer2 {
                        let mut guard = writer.lock().await;
                        let _ = guard.write_all(line.as_bytes()).await;
                        let _ = guard.write_all(b"\n").await;
                    }
                    buffer.clear();
                }
            } else {
                buffer.push(byte);
            }
        }
    });

    let status = child.wait().await;
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("Command failed with exit code: {:?}", s.code())),
        Err(e) => Err(format!("Failed to wait for command: {}", e)),
    }
}

fn spawn_hytale_installation(pool: DbPool, pm: ProcessManager, id: String, server_path: std::path::PathBuf) {
    tokio::spawn(async move {
        // Register "installing" process to allow log streaming
        let working_dir_str = server_path.to_string_lossy().to_string();
        if let Err(e) = pm.register_installing(&id, &working_dir_str).await {
            error!("Failed to register installing process: {}", e);
            return;
        }
        
        let logs_dir = server_path.join("logs");
        if !logs_dir.exists() {
             let _ = tokio::fs::create_dir_all(&logs_dir).await;
        }

        let install_log_path = logs_dir.join("install.log");
        let _ = tokio::fs::write(&install_log_path, "Starting Hytale Server Installation...\n").await;
        
        let log_file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&install_log_path)
            .await
            .ok()
            .map(|f| std::sync::Arc::new(tokio::sync::Mutex::new(f)));

        let log_helper = |msg: String| {
            let pm = pm.clone();
            let id = id.clone();
            let log_file = log_file.clone();
            async move {
                // Check for auth requirement
                if (msg.contains("IMPORTANT") && (msg.contains("authentifier") || msg.contains("authenticate"))) ||
                   (msg.contains("[HytaleServer] No server tokens configured")) ||
                   (msg.contains("/auth login to authenticate")) {
                    pm.set_auth_required(&id, true);
                }

                pm.broadcast_log(&id, msg.clone()).await;
                if let Some(f) = log_file {
                    let mut guard = f.lock().await;
                    let _ = guard.write_all(format!("{}\n", msg).as_bytes()).await;
                }
            }
        };

        log_helper("🚀 Initialization de l'installation du serveur...".to_string()).await;

        let zip_url = "https://downloader.hytale.com/hytale-downloader.zip";
        let zip_name = "hytale-downloader.zip";
        let dest_path = server_path.join(zip_name);

        log_helper(format!("⬇️ Téléchargement de Hytale Downloader depuis {}...", zip_url)).await;
        info!("Downloading Hytale downloader from {} to {:?}", zip_url, dest_path);

        // 1. Download ZIP
        if let Err(e) = run_with_logs(
            &mut tokio::process::Command::new("curl")
                .arg("-L")
                .arg("-o")
                .arg(&dest_path)
                .arg(zip_url),
            pm.clone(),
            id.clone(),
            "",
            Some(install_log_path.clone())
        ).await {
            log_helper(format!("❌ {}", e)).await;
            pm.remove(&id).await;
            return;
        }

        log_helper("✅ Téléchargement terminé.".to_string()).await;
        log_helper("📦 Extraction de l'archive...".to_string()).await;
        
        // 2. Unzip
        info!("Extracting Hytale downloader...");
        if let Err(e) = run_with_logs(
            &mut tokio::process::Command::new("unzip")
                .arg("-o")
                .arg(&dest_path)
                .arg("-d")
                .arg(&server_path),
            pm.clone(),
            id.clone(),
            "",
            Some(install_log_path.clone())
        ).await {
            pm.broadcast_log(&id, format!("❌ {}", e)).await;
            pm.remove(&id).await;
            return;
        }
        
        log_helper("✅ Extraction terminée.".to_string()).await;

        // 3. Cleanup unused files
        log_helper("🧹 Nettoyage des fichiers temporaires...".to_string()).await;
        
        // Remove .zip
        if let Err(e) = tokio::fs::remove_file(&dest_path).await {
            warn!("Failed to remove zip file: {}", e);
        }

        // Remove QUICKSTART.md
        let quickstart_path = server_path.join("QUICKSTART.md");
        if quickstart_path.exists() {
             let _ = tokio::fs::remove_file(quickstart_path).await;
        }

        // Determine OS and remove other binary
        let mut executable_name = "hytale-downloader-linux-amd64".to_string();
        let windows_binary = "hytale-downloader-windows-amd64.exe";
        let linux_binary = "hytale-downloader-linux-amd64";

        if std::env::consts::OS == "linux" {
            executable_name = linux_binary.to_string();
            let _ = tokio::fs::remove_file(server_path.join(windows_binary)).await;
        } else if std::env::consts::OS == "windows" {
             executable_name = windows_binary.to_string();
             let _ = tokio::fs::remove_file(server_path.join(linux_binary)).await;
        } else {
            // Mac or other
            // Mac or other
             if cfg!(target_os = "macos") {
                 // Warning: Mac is not officially supported by Hytale Downloader (binary is Linux/Windows)
                 // Users typically need to run Windows binary via Wine or Linux binary via Docker/VM.
                 // We will try Linux binary as a fallback/placeholder but warn the user.
                 log_helper("⚠️ Attention : macOS détecté. Le Hytale Downloader (Linux binary) peut ne pas fonctionner nativement.".to_string()).await;
                 executable_name = linux_binary.to_string(); 
                 let _ = tokio::fs::remove_file(server_path.join(windows_binary)).await;
            }
        }
        
        let executable_path = server_path.join(&executable_name);
        
        // Chmod +x
        if std::env::consts::OS != "windows" {
            let _ = tokio::process::Command::new("chmod")
                .arg("+x")
                .arg(&executable_path)
                .status()
                .await;
        }

        // 4. Run Downloader
        log_helper(format!("⏳ Exécution du downloader ({}) pour récupérer le serveur...", executable_name)).await;
        log_helper("⚠️ IMPORTANT : Le downloader va vous demander de vous authentifier via une URL.".to_string()).await;
        log_helper("⚠️ Surveillez les logs ci-dessous :".to_string()).await;

        if let Err(e) = run_with_logs(
            &mut tokio::process::Command::new(&executable_path)
                .current_dir(&server_path),
            pm.clone(),
            id.clone(),
            "",
            Some(install_log_path.clone())
        ).await {
            log_helper(format!("❌ {}", e)).await;
            // Don't abort immediately
        } else {
            log_helper("✅ Downloader terminé avec succès.".to_string()).await;
        }

         // 4.5 Check for downloaded ZIP (the actual server) and unzip it
         if let Ok(mut entries) = tokio::fs::read_dir(&server_path).await {
             while let Ok(Some(entry)) = entries.next_entry().await {
                 let path = entry.path();
                 if let Some(ext) = path.extension() {
                     if ext == "zip" {
                          let file_name = path.file_name().unwrap().to_string_lossy();
                          // Exclude hytale-downloader.zip (already extracted) and Assets (keep it)
                          if file_name != "hytale-downloader.zip" && file_name != "Assets.zip" {
                              log_helper(format!("📦 Décompression du serveur : {}...", file_name)).await;
                              
                              if let Err(e) = run_with_logs(
                                 &mut tokio::process::Command::new("unzip")
                                     .arg("-o")
                                     .arg(&path)
                                     .arg("-d")
                                     .arg(&server_path),
                                 pm.clone(),
                                 id.clone(),
                                 "",
                                  Some(install_log_path.clone())
                              ).await {
                                  log_helper(format!("❌ Erreur extraction: {}", e)).await;
                              } else {
                                 log_helper("✅ Décompression terminée.".to_string()).await;
                                 // cleanup the server zip
                                 let _ = tokio::fs::remove_file(&path).await;
                             }
                          }
                     }
                 }
             }
         }
        // 4.6 No longer flattening "server/Server" to "server"
        // The user requested to keep the "server/Server/" path as it is autogenerated by hytale-downloader.
        // We will just clean up the scripts from the nested folder if they exist, or the root.

        let nested_bundle_dir = server_path.join("Server");

        // Cleanup scripts (check both locations just in case)
        let _ = tokio::fs::remove_file(server_path.join("start.bat")).await;
        let _ = tokio::fs::remove_file(server_path.join("start.sh")).await;
        if nested_bundle_dir.exists() {
             let _ = tokio::fs::remove_file(nested_bundle_dir.join("start.bat")).await;
             let _ = tokio::fs::remove_file(nested_bundle_dir.join("start.sh")).await;
        }

        // 5. Verify HytaleServer.jar exists
        // Check in nested "Server" directory (automatically created by downloader)
        let nested_bundle_dir = server_path.join("Server");
        let nested_jar_path = nested_bundle_dir.join("HytaleServer.jar");
        
        if nested_jar_path.exists() {
             log_helper("✨ HytaleServer.jar présent. Installation terminée !".to_string()).await;
             
             // Update DB executable path to ensure it points to the jar relative to working dir
             // Since working dir is {uuid}, we point to Server/HytaleServer.jar
             let update_result = sqlx::query("UPDATE servers SET executable_path = ? WHERE id = ?")
                .bind("Server/HytaleServer.jar")
                .bind(&id)
                .execute(&pool)
                .await;
             
             if let Err(e) = update_result {
                 error!("Failed to update server executable path in DB: {}", e);
             }
        } else {
             log_helper("⚠️ Attention: HytaleServer.jar non trouvé après exécution (attendu: server/Server/HytaleServer.jar).".to_string()).await;
        }

        // Cleanup virtual process
        pm.remove(&id).await;
    });
}

async fn reinstall_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    // 1. Check if server exists
    let server: ServerRow = sqlx::query_as(
        "SELECT id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    // 2. Stop server if running
    if pm.is_running(&id) {
        info!("Stopping server {} for reinstallation...", id);
        pm.stop(&id).await?;
        // Wait a bit for file release
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await; 
    }

    // 3. Clean up server binaries ONLY (Preserve world, config, logs)
    // Flattened structure: server files are directly in working_dir/
    let base_path = Path::new(&server.working_dir);
    
    // Ensure base directory exists
    if !base_path.exists() {
         let _ = fs::create_dir_all(base_path).await;
    }

    info!("Cleaning up server binaries in {:?} (preserving user data)...", base_path);
    
    // List of files/dirs to delete for a clean "binary" reinstall
    let files_to_delete = vec![
        "HytaleServer.jar",
        "HytaleServer.aot",
        "lib", // directory
        "Assets.zip",
        "hytale-downloader.zip",
        "QUICKSTART.md",
        "hytale-downloader-linux-amd64",
        "hytale-downloader-windows-amd64.exe",
        "start.bat",
        "start.sh",
        "Server" // Legacy nested folder, important for migrating old servers
    ];
    
    for name in files_to_delete {
        let p = base_path.join(name);
        if p.exists() {
            if p.is_dir() {
                let _ = fs::remove_dir_all(&p).await;
            } else {
                let _ = fs::remove_file(&p).await;
            }
        }
    }
    
    // Check if config.json exists at ROOT, if NOT, generate it.
    let config_json_path = base_path.join("config.json");
    if !config_json_path.exists() {
        let auth_default = "authenticated".to_string();
        let auth_mode = server.config.as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v.get("auth_mode").map(|v| v.as_str().unwrap_or("authenticated").to_string()))
            .unwrap_or(auth_default);
            
        let hytale_config = templates::generate_config_json(
            &server.name,
            100, // Default max players if regenerating
            &auth_mode
        );
        
        // Write config.json
        if let Ok(mut config_file) = fs::File::create(&config_json_path).await {
            let _ = config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes()).await;
        }
    }

    // 4. Trigger Hytale installation
    // Pass root path as install path
    spawn_hytale_installation(pool.get_ref().clone(), pm.get_ref().clone(), id.clone(), base_path.to_path_buf());

    Ok(HttpResponse::Ok().json(serde_json::json!({ 
        "success": true,
        "message": "Reinstallation started",
        "working_dir": base_path.to_string_lossy()
    })))
}

async fn get_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    let dir_exists = Path::new(&server.working_dir).exists();
    let is_running = pm.is_running(&server.id);
    let status = if !dir_exists {
        "missing"
    } else if pm.is_installing(&server.id) {
        if pm.is_auth_required(&server.id) { "auth_required" } else { "installing" }
    } else if is_running {
        "running"
    } else {
        "stopped"
    };
    
    // Fetch persistent players from DB
    let player_rows: Vec<PlayerRow> = sqlx::query_as(
        "SELECT player_name, is_online, last_seen FROM server_players WHERE server_id = ?"
    )
    .bind(&id)
    .fetch_all(pool.get_ref())
    .await
    .unwrap_or_default();

    // Create a map for easy lookup/merging
    let mut players_map: std::collections::HashMap<String, Player> = player_rows.into_iter().map(|row| (row.player_name.clone(), Player {
        name: row.player_name,
        is_online: row.is_online != 0,
        last_seen: row.last_seen,
        is_op: false,
        is_banned: false,
        is_whitelisted: false,
    })).collect();

    // Merge with real-time in-memory players (Authority on "Online" status)
    if let Some(online_names) = pm.get_online_players(&id).await {
        for name in online_names {
            players_map.entry(name.clone())
                .and_modify(|p| {
                     p.is_online = true; 
                     // Update last_seen to now if online
                     p.last_seen = chrono::Utc::now().to_rfc3339();
                })
                .or_insert(Player {
                    name: name.clone(),
                    is_online: true,
                    last_seen: chrono::Utc::now().to_rfc3339(),
                    is_op: false,
                    is_banned: false,
                    is_whitelisted: false,
                });
        }
    }

    // Load extra metadata (OP, Banned, Whitelisted) from server files
    let meta = load_player_meta(&server.working_dir).await;
    
    // Ensure players discovered via files are in the map
    for (name, m) in &meta {
        players_map.entry(name.clone()).or_insert(Player {
            name: name.clone(),
            is_online: false,
            last_seen: "Jamais".to_string(), // Placeholder for players found only in files
            is_op: m.is_op,
            is_banned: m.is_banned,
            is_whitelisted: m.is_whitelisted,
        });
    }

    // Update metadata for existing players
    for (name, p) in players_map.iter_mut() {
        if let Some(m) = meta.get(name) {
            p.is_op = m.is_op;
            p.is_banned = m.is_banned;
            p.is_whitelisted = m.is_whitelisted;
        }
    }

    let mut final_players: Vec<Player> = players_map.into_values().collect();
    // Sort: Online first, then by Last Seen
    final_players.sort_by(|a, b| {
        b.is_online.cmp(&a.is_online)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });

    let players = if final_players.is_empty() { None } else { Some(final_players) };

    // Parse max_players (DB config or file config)
    let config_json = server.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
    let mut max_players = config_json.as_ref()
        .and_then(|c| c.get("MaxPlayers"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if max_players.is_none() {
        // Try reading from config.json (universe)
        let config_path = Path::new(&server.working_dir).join("server").join("universe").join("config.json");
        if let Ok(content) = fs::read_to_string(config_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
            }
        }
    }

    let port = Some(server.port as u16);
    let bind_address = Some(server.bind_address.clone());

    let started_at = pm.get_server_started_at(&server.id).await;
    let (cpu, mem, mut disk) = pm.get_metrics_data(&server.id).await;

    // Fallback for offline disk usage
    if disk == 0 {
        disk = WalkDir::new(&server.working_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();
    }

    let heap_bytes = parse_memory_to_bytes(server.max_memory.as_deref().unwrap_or("4G"));
    let total_bytes = calculate_total_memory(heap_bytes);

        // Parse notifications JSON
        let notifications = server.discord_notifications.as_ref()
            .and_then(|n| serde_json::from_str(n).ok());

        Ok(HttpResponse::Ok().json(ServerResponse {
        id: server.id,
        name: server.name,
        game_type: server.game_type,
        status: status.to_string(),
        executable_path: server.executable_path,
        working_dir: server.working_dir,
        java_path: server.java_path,
        min_memory: server.min_memory,
        max_memory: server.max_memory,
        extra_args: server.extra_args,
        config: config_json,
        auto_start: server.auto_start != 0,
        created_at: server.created_at,
        updated_at: server.updated_at,
        dir_exists,
        players,
        max_players,
        port,
        bind_address,
        
        backup_enabled: server.backup_enabled != 0,
        backup_frequency: server.backup_frequency as u32,
        backup_max_backups: server.backup_max_backups as u32,
        backup_prefix: server.backup_prefix,
        discord_username: server.discord_username,
        discord_avatar: server.discord_avatar,
        discord_webhook_url: server.discord_webhook_url,
        discord_notifications: notifications,
        logs_retention_days: server.logs_retention_days as u32,
        watchdog_enabled: server.watchdog_enabled != 0,
        auth_mode: server.auth_mode,

        cpu_usage: cpu,
        memory_usage_bytes: mem,
        max_memory_bytes: total_bytes,
        max_heap_bytes: heap_bytes,
        disk_usage_bytes: disk,
        started_at,
    }))
}

async fn kill_server(
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    pm.kill(&id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn update_server(
    pool: web::Data<DbPool>,
    path: web::Path<String>,
    body: web::Json<CreateServerRequest>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let now = Utc::now().to_rfc3339();
    let auto_start = body.auto_start.unwrap_or(false) as i32;

    let config_str = body.config.as_ref().map(|c| c.to_string());
    let notifications_str = body.discord_notifications.as_ref().map(|c| c.to_string());

    let result = sqlx::query(
        "UPDATE servers SET 
        name = ?, game_type = ?, executable_path = ?, working_dir = ?, java_path = ?, min_memory = ?, max_memory = ?, extra_args = ?, config = ?, auto_start = ?, updated_at = ?,
        backup_enabled = COALESCE(?, backup_enabled),
        backup_frequency = COALESCE(?, backup_frequency),
        backup_max_backups = COALESCE(?, backup_max_backups),
        backup_prefix = COALESCE(?, backup_prefix),
        discord_username = COALESCE(?, discord_username),
        discord_avatar = COALESCE(?, discord_avatar),
        discord_webhook_url = COALESCE(?, discord_webhook_url),
        discord_notifications = COALESCE(?, discord_notifications),
        logs_retention_days = COALESCE(?, logs_retention_days),
        watchdog_enabled = COALESCE(?, watchdog_enabled),
        auth_mode = COALESCE(?, auth_mode),
        bind_address = COALESCE(?, bind_address),
        port = COALESCE(?, port)
        WHERE id = ?",
    )
    .bind(&body.name)
    .bind(&body.game_type)
    .bind(&body.executable_path)
    .bind(&body.working_dir)
    .bind(&body.java_path)
    .bind(&body.min_memory)
    .bind(&body.max_memory)
    .bind(&body.extra_args)
    .bind(config_str)
    .bind(auto_start)
    .bind(&now)
    // New fields bindings
    .bind(body.backup_enabled.map(|b| b as i32))
    .bind(body.backup_frequency)
    .bind(body.backup_max_backups)
    .bind(&body.backup_prefix)
    .bind(&body.discord_username)
    .bind(&body.discord_avatar)
    .bind(&body.discord_webhook_url)
    .bind(notifications_str)
    .bind(body.logs_retention_days)
    .bind(body.watchdog_enabled.map(|b| b as i32))
    .bind(&body.auth_mode)
    .bind(&body.bind_address)
    .bind(body.port)
    .bind(&id)
    .execute(pool.get_ref())
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Server not found".into()));
    }

    // Write config to file to ensure Hytale picks up changes (like port, seed, etc.)
    if let Some(config_json) = &body.config {
        let root_config_path = Path::new(&body.working_dir).join("config.json");
        let server_dir = Path::new(&body.working_dir).join("server");
        let universe_dir = server_dir.join("universe");
        let nested_config_path = universe_dir.join("config.json");
        
        if let Ok(json_str) = serde_json::to_string_pretty(config_json) {
            // Write to root
            if let Err(e) = tokio::fs::write(&root_config_path, &json_str).await {
                error!("Failed to write root config.json for server {}: {}", id, e);
            }
            
            // Check if server directory exists (flattened)
            if server_dir.exists() {
                 if !universe_dir.exists() {
                     let _ = tokio::fs::create_dir_all(&universe_dir).await;
                 }
                 
                 if let Err(e) = tokio::fs::write(&nested_config_path, &json_str).await {
                    error!("Failed to write nested server/universe/config.json for server {}: {}", id, e);
                } else {
                    info!("Updated server/universe/config.json for server {}", id);
                }
            }
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn delete_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    // Fetch working directory before deletion
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&id)
        .fetch_optional(pool.get_ref())
        .await?;

    // Stop server if running
    if pm.is_running(&id) {
        pm.stop(&id).await?;
    }

    let result = sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(&id)
        .execute(pool.get_ref())
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Server not found".into()));
    }

    // Delete server directory
    if let Some((working_dir,)) = server {
        let path = Path::new(&working_dir);
        if path.exists() {
             // Only delete if it looks like our server directory (security check)
             // We configured it as {base}/{uuid} so we should check if it ends with the ID or contains it
             // For now, trusting the DB path as it's what we created.
             if let Err(e) = tokio::fs::remove_dir_all(path).await {
                 error!("Failed to remove server directory {}: {}", working_dir, e);
                 // Don't fail the request, just log it
             } else {
                 info!("Removed server directory: {}", working_dir);
             }
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn start_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    // Use the working directory from the database directly. 
    let process_working_dir = Path::new(&server.working_dir).to_path_buf();
    
    // Legacy support: Check if we should use the "server" subdirectory
    // If working_dir/server/Server exists, we might be in legacy mode, 
    // BUT we are moving to root-based working_dir.
    // If working_dir contains startup scripts or binary directly (or in Server/), we run from there.
    // Logic: If working_dir/server exists AND it was explicitly used before, we might assume legacy.
    // But simplified approach: Trust DB working_dir.
    // No longer appending "server" unless absolutely sure.
    // We REMOVE the auto-append "server" block.
    
    let process_working_dir_str = process_working_dir.to_str().unwrap_or(&server.working_dir);

    // Regenerate config.json to ensure latest port/settings are applied
    let config_json_path = process_working_dir.join("config.json");
    let server_config: Option<serde_json::Value> = server.config.as_ref().and_then(|c| serde_json::from_str(c).ok());
    
    let port = server.port as u16;
        
    let max_players = server_config.as_ref()
        .and_then(|c| c.get("MaxPlayers"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(100);

    let auth_mode = &server.auth_mode;

    let hytale_config = templates::generate_config_json(
        &server.name,
        max_players, 
        auth_mode 
    );
    
    // Inject Port manually
    let mut hytale_config_obj = serde_json::to_value(hytale_config).unwrap();
    if let Some(obj) = hytale_config_obj.as_object_mut() {
        obj.insert("Port".to_string(), serde_json::json!(port));
    }
    
    if let Ok(mut config_file) = fs::File::create(&config_json_path).await {
         let _ = config_file.write_all(serde_json::to_string_pretty(&hytale_config_obj).unwrap().as_bytes()).await;
    }

    // Inject port and bind address into config for process manager
    let mut pm_config = server.config.as_ref()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .unwrap_or(serde_json::json!({}));
        
    if let Some(obj) = pm_config.as_object_mut() {
        obj.insert("port".to_string(), serde_json::json!(server.port));
        obj.insert("bind_address".to_string(), serde_json::json!(server.bind_address));
    }

    pm.start(
        &server.id,
        &server.executable_path,
        process_working_dir_str,
        server.java_path.as_deref(),
        server.min_memory.as_deref(),
        server.max_memory.as_deref(),
        server.extra_args.as_deref(),
        Some(&pm_config),
    )
    .await?;

    // Send webhook notification
    // Send webhook notification (Placeholder for now)
    let pool_clone = pool.get_ref().clone();
    let server_name = server.name.clone();
    // Extract webhook_url from server config
    // Extract webhook_url from server columns

        
    let webhook_url = server.discord_webhook_url.clone();
    
    // Filter out empty webhook url
    let webhook_url = webhook_url.filter(|u| !u.is_empty());
        
    if let Some(url) = webhook_url {
        tokio::spawn(async move {
            crate::services::discord_service::send_notification(
                &pool_clone,
                "🟢 Serveur Démarré",
                &format!("Le serveur **{}** a été démarré.", server_name),
                crate::services::discord_service::COLOR_SUCCESS,
                Some(&server_name),
                Some(&url), // Explicitly pass the URL
            ).await;
        });
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "starting" })))
}

async fn stop_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    
    // Get server name and config for webhook
    let server: Option<ServerRow> = sqlx::query_as("SELECT * FROM servers WHERE id = ?")
        .bind(&id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    pm.stop(&id).await?;
    
    // Send webhook notification
    if let Some(s) = server {
        let pool_clone = pool.get_ref().clone();
        
        // Use webhook from DB column
        if let Some(url) = s.discord_webhook_url {
            if !url.is_empty() {
                tokio::spawn(async move {
                    crate::services::discord_service::send_notification(
                        &pool_clone,
                        "🔴 Serveur Arrêté",
                        &format!("Le serveur **{}** a été arrêté.", s.name),
                        crate::services::discord_service::COLOR_ERROR,
                        Some(&s.name),
                        Some(&url),
                    ).await;
                });
            }
        }
    }
    
    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "stopping" })))
}

async fn restart_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    // We must run the server binary from the 'server' subdirectory to ensure it finds config.json
    let process_working_dir = Path::new(&server.working_dir).join("server");
    let process_working_dir_str = process_working_dir.to_str().unwrap_or(&server.working_dir);

    pm.restart(
        &server.id,
        &server.executable_path,
        process_working_dir_str,
        server.java_path.as_deref(),
        server.min_memory.as_deref(),
        server.max_memory.as_deref(),
        server.extra_args.as_deref(),
        server.config.as_ref().and_then(|c| serde_json::from_str(c).ok()).as_ref(),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "restarting" })))
}


#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

async fn send_command(
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
    body: web::Json<CommandRequest>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    pm.send_command(&id, &body.command).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

#[derive(Debug, FromRow)]
struct ServerRow {
    id: String,
    name: String,
    game_type: String,
    executable_path: String,
    working_dir: String,
    java_path: Option<String>,
    min_memory: Option<String>,
    max_memory: Option<String>,
    extra_args: Option<String>,
    config: Option<String>,
    auto_start: i32,
    created_at: String,
    updated_at: String,
    
    // New fields
    #[sqlx(default)]
    backup_enabled: i32,
    #[sqlx(default)]
    backup_frequency: i32,
    #[sqlx(default)]
    backup_max_backups: i32,
    #[sqlx(default)]
    backup_prefix: String,
    #[sqlx(default)]
    discord_username: Option<String>,
    #[sqlx(default)]
    discord_avatar: Option<String>,
    #[sqlx(default)]
    discord_webhook_url: Option<String>,
    #[sqlx(default)]
    discord_notifications: Option<String>,
    #[sqlx(default)]
    logs_retention_days: i32,
    #[sqlx(default)]
    watchdog_enabled: i32,
    #[sqlx(default)]
    auth_mode: String,
    #[sqlx(default)]
    bind_address: String,
    #[sqlx(default)]
    port: i32,
}

// ============= Server Files API =============

#[derive(Debug, Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FilesQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadFileQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
struct WriteFileRequest {
    path: String,
    content: String,
}

async fn list_server_files(
    pool: web::Data<DbPool>,
    path_param: web::Path<String>,
    query: web::Query<FilesQuery>,
) -> Result<HttpResponse, AppError> {
    let server_id = path_param.into_inner();
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    // Build the path - relative to working_dir (includes server/ and manager/)
    let base_path = Path::new(&working_dir);
    let relative_path = query.path.clone().unwrap_or_default();
    let full_path = if relative_path.is_empty() {
        base_path.to_path_buf()
    } else {
        base_path.join(&relative_path)
    };
    
    // Security: ensure path is within server directory
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("Path not found".into()));
    }
    
    if !full_path.is_dir() {
        return Err(AppError::BadRequest("Path is not a directory".into()));
    }
    
    let mut entries: Vec<FileEntry> = Vec::new();
    
    // Add parent directory if not at root
    if !relative_path.is_empty() {
        let parent = Path::new(&relative_path).parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        entries.push(FileEntry {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            size: None,
        });
    }
    
    // Read directory entries
    let read_dir = std::fs::read_dir(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read directory: {}", e)))?;
    
    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        let is_dir = entry_path.is_dir();
        let size = if is_dir { None } else { entry_path.metadata().ok().map(|m| m.len()) };
        
        if let Some(name) = entry_path.file_name() {
            let name_str = name.to_string_lossy().to_string();
            let rel_path = if relative_path.is_empty() {
                name_str.clone()
            } else {
                format!("{}/{}", relative_path, name_str)
            };
            
            entries.push(FileEntry {
                name: name_str,
                path: rel_path,
                is_dir,
                size,
            });
        }
    }
    
    // Sort: directories first, then alphabetically
    entries.sort_by(|a, b| {
        if a.name == ".." {
            std::cmp::Ordering::Less
        } else if b.name == ".." {
            std::cmp::Ordering::Greater
        } else if a.is_dir && !b.is_dir {
            std::cmp::Ordering::Less
        } else if !a.is_dir && b.is_dir {
            std::cmp::Ordering::Greater
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "current_path": relative_path,
        "entries": entries
    })))
}

async fn read_server_file(
    pool: web::Data<DbPool>,
    path_param: web::Path<String>,
    query: web::Query<ReadFileQuery>,
) -> Result<HttpResponse, AppError> {
    let server_id = path_param.into_inner();
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = Path::new(&working_dir);
    let full_path = base_path.join(&query.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
        return Err(AppError::BadRequest("Cannot read a directory".into()));
    }
    
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to read file: {}", e)))?;
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "path": query.path,
        "content": content
    })))
}

async fn write_server_file(
    pool: web::Data<DbPool>,
    path_param: web::Path<String>,
    body: web::Json<WriteFileRequest>,
) -> Result<HttpResponse, AppError> {
    let server_id = path_param.into_inner();
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = Path::new(&working_dir);
    let full_path = base_path.join(&body.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    std::fs::write(&full_path, &body.content)
        .map_err(|e| AppError::Internal(format!("Failed to write file: {}", e)))?;
    
    info!("File written: {:?}", full_path);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

#[derive(Debug, Deserialize)]
struct DeleteFileRequest {
    path: String,
}

async fn delete_server_file(
    pool: web::Data<DbPool>,
    path_param: web::Path<String>,
    body: web::Json<DeleteFileRequest>,
) -> Result<HttpResponse, AppError> {
    let server_id = path_param.into_inner();
    
    // Get server working directory
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&server_id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    let working_dir = server
        .ok_or_else(|| AppError::NotFound("Server not found".into()))?
        .0;
    
    let base_path = Path::new(&working_dir);
    let full_path = base_path.join(&body.path);
    
    // Security check
    if !full_path.starts_with(&base_path) {
        return Err(AppError::BadRequest("Invalid path".into()));
    }
    
    if !full_path.exists() {
        return Err(AppError::NotFound("File not found".into()));
    }
    
    if full_path.is_dir() {
         return Err(AppError::BadRequest("Cannot delete a directory with this endpoint".into()));
    }
    
    std::fs::remove_file(&full_path)
        .map_err(|e| AppError::Internal(format!("Failed to delete file: {}", e)))?;
    
    info!("File deleted: {:?}", full_path);
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "path": body.path
    })))
}

struct PlayerMeta {
    is_op: bool,
    is_whitelisted: bool,
    is_banned: bool,
}

async fn load_player_meta(working_dir: &str) -> std::collections::HashMap<String, PlayerMeta> {
    let mut meta_map: std::collections::HashMap<String, PlayerMeta> = std::collections::HashMap::new();
    let server_path = Path::new(working_dir).join("server");

    // 1. Load Permissions (OPs)
    let ops_path = server_path.join("permissions.json");
    if let Ok(content) = fs::read_to_string(ops_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(arr) = json.as_array() {
                for item in arr {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        let entry = meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: false, is_banned: false });
                        entry.is_op = true;
                    }
                }
            }
        }
    }

    // 2. Load Whitelist
    let wl_path = server_path.join("whitelist.json");
    if let Ok(content) = fs::read_to_string(wl_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(arr) = json.as_array() {
                for item in arr {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        let entry = meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: false, is_banned: false });
                        entry.is_whitelisted = true;
                    }
                }
            }
        }
    }

    // 3. Load Bans
    let bans_path = server_path.join("bans.json");
    if let Ok(content) = fs::read_to_string(bans_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(arr) = json.as_array() {
                for item in arr {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        let entry = meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: false, is_banned: false });
                        entry.is_banned = true;
                    }
                }
            }
        }
    }

    meta_map
}

