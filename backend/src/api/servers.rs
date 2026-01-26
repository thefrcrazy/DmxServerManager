use actix_web::{web, HttpResponse, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::io::AsyncBufReadExt;
use tracing::{info, error, warn};
use std::path::Path;

use crate::db::DbPool;
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
    pub players: Option<Vec<String>>,
    pub max_players: Option<u32>,
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
            .route("/{id}/files/write", web::post().to(write_server_file)),
    );
}

async fn list_servers(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
) -> Result<HttpResponse, AppError> {
    let servers: Vec<ServerRow> = sqlx::query_as(
        "SELECT id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at FROM servers"
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
        } else if is_running {
            "running"
        } else {
            "stopped"
        };

        let players = if is_running {
            pm.get_online_players(&s.id).await
        } else {
            None
        };

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
            config: config_json,
            auto_start: s.auto_start != 0,
            created_at: s.created_at,
            updated_at: s.updated_at,
            dir_exists,
            players,
            max_players,
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
    //   {uuid}/manager.json      - Manager config
    //   {uuid}/server/           - Hytale server files (working directory)
    let server_base_path = Path::new(&body.working_dir).join(&id);
    let server_path = server_base_path.join("server");
    let backups_path = server_base_path.join("backups");
    
    // Create all directories (Hytale official structure)
    // We only create the base structure, the server will generate the rest (.cache, logs, universe, etc.)
    let directories = [
        &server_base_path,
        &server_path,
        &backups_path,
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
    // max_players et al are used for manager.json but we don't generate server config anymore
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


    // Generate and write manager/manager.json (unified config)
    let manager_config = templates::generate_manager_json(
        &id,
        server_name,
        server_base_path.to_str().unwrap_or(""),
        bind_address,
        port,
        auth_mode,
        body.java_path.as_deref(),
        body.min_memory.as_deref(),
        body.max_memory.as_deref(),
    );
    let manager_json_path = server_base_path.join("manager.json");
    let mut file = fs::File::create(&manager_json_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to create manager.json: {}", e))
    })?;
    file.write_all(serde_json::to_string_pretty(&manager_config).unwrap().as_bytes())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write manager.json: {}", e)))?;

    info!("Generated manager configuration for server {}", id);

    // Auto-download server jar if requested
    let mut final_executable = body.executable_path.clone();
    if body.game_type == "hytale" {
        spawn_hytale_installation(pool.get_ref().clone(), pm.get_ref().clone(), id.clone(), server_path.clone());
        
        // We set executable path tentatively...
        final_executable = "HytaleServer.jar".to_string(); 
    }

    let config_str = body.config.as_ref().map(|c| c.to_string());

    // Store server in database with the correct paths
    let actual_working_dir = server_base_path.to_str().unwrap_or(&body.working_dir);
    let actual_executable = server_path.join(&final_executable);
    let actual_executable_str = actual_executable.to_str().unwrap_or(&final_executable);

    // Generate and write config.json (Hytale server config)
    // We do this to ensure defaults (like MaxPlayers) are set as desired
    // Note: 100 is the default MaxPlayers requested
    let hytale_config = templates::generate_config_json(
        server_name,
        100, 
        auth_mode
    );
    let config_json_path = server_path.join("config.json");
    let mut config_file = fs::File::create(&config_json_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to create config.json: {}", e))
    })?;
    config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write config.json: {}", e)))?;

    info!("Generated Hytale config.json for server {}", id);

    sqlx::query(
        "INSERT INTO servers (id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(config_str) // This stores the raw request config, but we also wrote the actual file above
    .bind(auto_start)
    .bind(&now)
    .bind(&now)
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
        if let Err(e) = pm.register_installing(&id).await {
            error!("Failed to register installing process: {}", e);
            return;
        }
        
        let logs_dir = server_path.join("logs");
        if !logs_dir.exists() {
             let _ = tokio::fs::create_dir_all(&logs_dir).await;
        }

        let install_log_path = logs_dir.join("install.log");
        let _ = tokio::fs::write(&install_log_path, "Starting Hytale Server Installation...\n").await;
        
        pm.broadcast_log(&id, "🚀 Initialization de l'installation du serveur...".into()).await;

        let zip_url = "https://downloader.hytale.com/hytale-downloader.zip";
        let zip_name = "hytale-downloader.zip";
        let dest_path = server_path.join(zip_name);

        pm.broadcast_log(&id, format!("⬇️ Téléchargement de Hytale Downloader depuis {}...", zip_url)).await;
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
            pm.broadcast_log(&id, format!("❌ {}", e)).await;
            pm.remove(&id).await;
            return;
        }

        pm.broadcast_log(&id, "✅ Téléchargement terminé.".into()).await;
        pm.broadcast_log(&id, "📦 Extraction de l'archive...".into()).await;
        
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
        
        pm.broadcast_log(&id, "✅ Extraction terminée.".into()).await;

        // 3. Cleanup unused files
        pm.broadcast_log(&id, "🧹 Nettoyage des fichiers temporaires...".into()).await;
        
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
             if cfg!(target_os = "macos") {
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
        pm.broadcast_log(&id, format!("⏳ Exécution du downloader ({}) pour récupérer le serveur...", executable_name)).await;
        pm.broadcast_log(&id, "⚠️ IMPORTANT : Le downloader va vous demander de vous authentifier via une URL.".into()).await;
        pm.broadcast_log(&id, "⚠️ Surveillez les logs ci-dessous :".into()).await;

        if let Err(e) = run_with_logs(
            &mut tokio::process::Command::new(&executable_path)
                .current_dir(&server_path),
            pm.clone(),
            id.clone(),
            "",
            Some(install_log_path.clone())
        ).await {
            pm.broadcast_log(&id, format!("❌ {}", e)).await;
            // Don't abort immediately
        } else {
            pm.broadcast_log(&id, "✅ Downloader terminé avec succès.".into()).await;
        }

        // 4.5 Check for downloaded ZIP (the actual server) and unzip it
        let mut extracted = false;
        if let Ok(mut entries) = tokio::fs::read_dir(&server_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "zip" {
                         let file_name = path.file_name().unwrap().to_string_lossy();
                         // Exclude hytale-downloader.zip (already extracted) and Assets (keep it)
                         if file_name != "hytale-downloader.zip" && file_name != "Assets.zip" {
                             pm.broadcast_log(&id, format!("📦 Décompression du serveur : {}...", file_name)).await;
                             
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
                                pm.broadcast_log(&id, format!("❌ Erreur extraction: {}", e)).await;
                            } else {
                                extracted = true;
                                pm.broadcast_log(&id, "✅ Décompression terminée.".into()).await;
                                // cleanup the server zip
                                let _ = tokio::fs::remove_file(&path).await;
                            }
                         }
                    }
                }
            }
        }

        // 5. Verify HytaleServer.jar exists
        let jar_path = server_path.join("HytaleServer.jar");
        if jar_path.exists() {
             pm.broadcast_log(&id, "✨ HytaleServer.jar présent. Installation terminée !".into()).await;
             
             // Update DB executable path to ensure it points to the jar (fixes legacy/broken paths)
             // We need to execute a query. spawn_hytale_installation has `pool: DbPool`.
             // DbPool is likely sqlx::Pool.
             let update_result = sqlx::query("UPDATE servers SET executable_path = ? WHERE id = ?")
                .bind(server_path.join("HytaleServer.jar").to_str().unwrap_or("HytaleServer.jar"))
                .bind(&id)
                .execute(&pool)
                .await;
                
             if let Err(e) = update_result {
                 error!("Failed to update server executable path in DB: {}", e);
             }
        } else {
             pm.broadcast_log(&id, "⚠️ Attention: HytaleServer.jar non trouvé après exécution.".into()).await;
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
    // We assume structure is {working_dir}/server for the game files
    let server_base_path = Path::new(&server.working_dir);
    let server_path = server_base_path.join("server");
    let backups_path = server_base_path.join("backups");
    
    // Ensure base directories exist (Restores structure if deleted)
    if !server_base_path.exists() {
         let _ = fs::create_dir_all(server_base_path).await;
    }
    if !backups_path.exists() {
         let _ = fs::create_dir_all(&backups_path).await;
    }
    
    // Restore manager.json if missing
    let manager_json_path = server_base_path.join("manager.json");
    if !manager_json_path.exists() {
        info!("Restoring missing manager.json for server {}", id);
        
        let config_val = server.config.as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
            
        let bind_address = config_val.as_ref()
            .and_then(|c| c.get("bind_address"))
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
            
        let port = config_val.as_ref()
            .and_then(|c| c.get("port"))
            .and_then(|v| v.as_u64())
            .unwrap_or(5520) as u16;
            
        let auth_mode = config_val.as_ref()
            .and_then(|c| c.get("auth_mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("authenticated");

        let manager_config = templates::generate_manager_json(
            &id,
            &server.name,
            server.working_dir.as_str(),
            bind_address,
            port,
            auth_mode,
            server.java_path.as_deref(),
            server.min_memory.as_deref(),
            server.max_memory.as_deref(),
        );

        if let Ok(mut file) = fs::File::create(&manager_json_path).await {
            let _ = file.write_all(serde_json::to_string_pretty(&manager_config).unwrap().as_bytes()).await;
        }
    }

    if server_path.exists() {
        info!("Cleaning up server binaries in {:?} (preserving user data)...", server_path);
        
        // List of files/dirs to delete for a clean "binary" reinstall
        let files_to_delete = vec![
            "HytaleServer.jar",
            "HytaleServer.aot",
            "lib", // directory
            "Assets.zip",
            "hytale-downloader.zip",
            "QUICKSTART.md",
            "hytale-downloader-linux-amd64",
            "hytale-downloader-windows-amd64.exe"
        ];
        
        for name in files_to_delete {
            let p = server_path.join(name);
            if p.exists() {
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p).await;
                } else {
                    let _ = fs::remove_file(&p).await;
                }
            }
        }
    } else {
        // Create if missing
        if let Err(e) = fs::create_dir_all(&server_path).await {
            return Err(AppError::Internal(format!("Failed to create server directory: {}", e)));
        }
    }
    
    // Check if config.json exists, if NOT, generate it.
    let config_json_path = server_path.join("config.json");
    if !config_json_path.exists() {
        let config_json = server.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
        let max_players = config_json.as_ref()
            .and_then(|c| c.get("MaxPlayers"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(100);
            
        let hytale_config = templates::generate_config_json(
            &server.name,
            max_players, 
            "authenticated" 
        );
        
        let mut config_file = fs::File::create(&config_json_path).await.map_err(|e| {
            AppError::Internal(format!("Failed to create config.json: {}", e))
        })?;
        config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write config.json: {}", e)))?;
    }


    // 4. Trigger Installation
    spawn_hytale_installation(pool.get_ref().clone(), pm.get_ref().clone(), id.clone(), server_path);

    Ok(HttpResponse::Ok().json(serde_json::json!({ 
        "success": true, 
        "message": "Reinstallation started" 
    })))
}

async fn get_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    let server: ServerRow = sqlx::query_as(
        "SELECT id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    let is_running = pm.is_running(&server.id);
    let status = if is_running {
        "running"
    } else {
        "stopped"
    };

    let dir_exists = Path::new(&server.working_dir).exists();
    
    let players = if is_running {
        pm.get_online_players(&server.id).await
    } else {
        None
    };

    // Parse max_players (DB config or file config)
    let config_json = server.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
    let mut max_players = config_json.as_ref()
        .and_then(|c| c.get("MaxPlayers"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if max_players.is_none() {
        // Try reading from config.json
        let config_path = Path::new(&server.working_dir).join("server").join("config.json");
        if let Ok(content) = fs::read_to_string(config_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
            }
        }
    }

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

    let result = sqlx::query(
        "UPDATE servers SET name = ?, game_type = ?, executable_path = ?, working_dir = ?, java_path = ?, min_memory = ?, max_memory = ?, extra_args = ?, config = ?, auto_start = ?, updated_at = ? WHERE id = ?",
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
    .bind(&id)
    .execute(pool.get_ref())
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Server not found".into()));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn delete_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

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

    Ok(HttpResponse::Ok().json(serde_json::json!({ "success": true })))
}

async fn start_server(
    pool: web::Data<DbPool>,
    pm: web::Data<ProcessManager>,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();

    let server: ServerRow = sqlx::query_as(
        "SELECT id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Server not found".into()))?;

    // We must run the server binary from the 'server' subdirectory to ensure it finds config.json
    let process_working_dir = Path::new(&server.working_dir).join("server");
    let process_working_dir_str = process_working_dir.to_str().unwrap_or(&server.working_dir);

    pm.start(
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

    // Send webhook notification
    let pool_clone = pool.get_ref().clone();
    let server_name = server.name.clone();
    // Extract webhook_url from server config
    let config: Option<serde_json::Value> = server.config.clone()
        .and_then(|c| serde_json::from_str(&c).ok());
        
    // Send webhook notification
    let pool_clone = pool.get_ref().clone();
    let server_name = server.name.clone();
    // Extract webhook_url from server config
    let config: Option<serde_json::Value> = server.config.clone()
        .and_then(|c| serde_json::from_str(&c).ok());
        
    let webhook_url = config.as_ref()
        .and_then(|c| c.get("discord_webhook_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
        
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
    let server_data: Option<(String, Option<String>)> = sqlx::query_as("SELECT name, config FROM servers WHERE id = ?")
        .bind(&id)
        .fetch_optional(pool.get_ref())
        .await?;
    
    pm.stop(&id).await?;
    
    // Send webhook notification
    if let Some((name, config_str)) = server_data {
        let pool_clone = pool.get_ref().clone();
        
        let config: Option<serde_json::Value> = config_str
            .and_then(|c| serde_json::from_str(&c).ok());
            
        let webhook_url = config.as_ref()
            .and_then(|c| c.get("discord_webhook_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
            
        if let Some(url) = webhook_url {
            tokio::spawn(async move {
                crate::services::discord_service::send_notification(
                    &pool_clone,
                    "🔴 Serveur Arrêté",
                    &format!("Le serveur **{}** a été arrêté.", name),
                    crate::services::discord_service::COLOR_ERROR,
                    Some(&name),
                    Some(&url),
                ).await;
            });
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
        "SELECT id, name, game_type, executable_path, working_dir, java_path, min_memory, max_memory, extra_args, config, auto_start, created_at, updated_at FROM servers WHERE id = ?"
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
