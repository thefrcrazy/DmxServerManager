use axum::{
    extract::{Path, State},
    Json,
};
use tracing::{info, error};
use std::path::{Path as StdPath, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::core::AppState;
use crate::core::error::AppError;
use crate::utils::templates;
use crate::api::servers::models::ServerRow;

pub async fn start_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;

    let process_working_dir = StdPath::new(&server.working_dir).to_path_buf();
    let process_working_dir_str = process_working_dir.to_str().unwrap_or(&server.working_dir);

    let config_json_path = process_working_dir.join("config.json");
    let server_config: Option<serde_json::Value> = server.config.as_ref().and_then(|c| serde_json::from_str(c).ok());
    
    let port = server.port as u16;
    let max_players = server_config.as_ref()
        .and_then(|c| c.get("MaxPlayers"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(100);
    let auth_mode = &server.auth_mode;

    let mut hytale_config_obj = if config_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&config_json_path).await {
             serde_json::from_str(&content).unwrap_or_else(|_| templates::generate_config_json(
                &server.name,
                max_players, 
                auth_mode 
            ))
        } else {
             templates::generate_config_json(
                &server.name,
                max_players, 
                auth_mode 
            )
        }
    } else {
        templates::generate_config_json(
            &server.name,
            max_players, 
            auth_mode 
        )
    };

    // Prepare updates
    let auth_store = if auth_mode == "authenticated" {
        serde_json::json!({ "Type": "Encrypted", "Path": "auth.enc" })
    } else {
        serde_json::json!({ "Type": "None" })
    };

    let updates = serde_json::json!({
        "Port": port,
        "ServerName": server.name,
        "MaxPlayers": max_players,
        "AuthCredentialStore": auth_store
    });

    // Merge updates into existing config
    templates::deep_merge(&mut hytale_config_obj, &updates);
    
    // Save updated config
    if let Ok(mut config_file) = fs::File::create(&config_json_path).await {
         let _ = config_file.write_all(serde_json::to_string_pretty(&hytale_config_obj).unwrap().as_bytes()).await;
    }

    // Process Manager config (internal use, lowercase keys are fine here)
    let mut pm_config = server.config.as_ref()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .unwrap_or(serde_json::json!({}));
    if let Some(obj) = pm_config.as_object_mut() {
        obj.insert("port".to_string(), serde_json::json!(server.port));
        obj.insert("bind_address".to_string(), serde_json::json!(server.bind_address));
    }

    state.process_manager.start(
        &server.id,
        &server.executable_path,
        process_working_dir_str,
        server.java_path.as_deref(),
        server.min_memory.as_deref(),
        server.max_memory.as_deref(),
        server.extra_args.as_deref(),
        Some(&pm_config),
        &server.game_type,
    )
    .await?;

    let pool_clone = state.pool.clone();
    let server_name = server.name.clone();
    let webhook_url = server.discord_webhook_url.clone().filter(|u| !u.is_empty());
        
    if let Some(url) = webhook_url {
        tokio::spawn(async move {
            crate::services::system::discord::send_notification(
                &pool_clone,
                "🟢 Serveur Démarré",
                &format!("Le serveur **{server_name}** a été démarré."),
                crate::services::system::discord::COLOR_SUCCESS,
                Some(&server_name),
                Some(&url),
            ).await;
        });
    }

    Ok(Json(serde_json::json!({ "status": "starting" })))
}

pub async fn stop_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: Option<ServerRow> = sqlx::query_as("SELECT * FROM servers WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?;
    
    state.process_manager.stop(&id).await?;
    
    if let Some(s) = server {
        let pool_clone = state.pool.clone();
        if let Some(url) = s.discord_webhook_url {
            if !url.is_empty() {
                tokio::spawn(async move {
                    crate::services::system::discord::send_notification(
                        &pool_clone,
                        "🔴 Serveur Arrêté",
                        &format!("Le serveur **{}** a été arrêté.", s.name),
                        crate::services::system::discord::COLOR_ERROR,
                        Some(&s.name),
                        Some(&url),
                    ).await;
                });
            }
        }
    }
    
    Ok(Json(serde_json::json!({ "status": "stopping" })))
}

pub async fn restart_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;

    let process_working_dir = StdPath::new(&server.working_dir).to_path_buf();
    let process_working_dir_str = process_working_dir.to_str().unwrap_or(&server.working_dir);

    state.process_manager.restart(
        &server.id,
        &server.executable_path,
        process_working_dir_str,
        server.java_path.as_deref(),
        server.min_memory.as_deref(),
        server.max_memory.as_deref(),
        server.extra_args.as_deref(),
        server.config.as_ref().and_then(|c| serde_json::from_str(c).ok()).as_ref(),
        &server.game_type,
    )
    .await?;

    Ok(Json(serde_json::json!({ "status": "restarting" })))
}

pub async fn kill_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.process_manager.kill(&id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn reinstall_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;

    let pm = &state.process_manager;
    if pm.is_running(&id) {
        info!("Stopping server {} for reinstallation...", id);
        pm.stop(&id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await; 
    }

    let base_path = StdPath::new(&server.working_dir);
    if !base_path.exists() {
         let _ = fs::create_dir_all(base_path).await;
    }

    info!("Cleaning up server binaries in {:?} (preserving user data)...", base_path);
    
    let files_to_delete = vec![
        "HytaleServer.jar",
        "HytaleServer.aot",
        "lib", 
        "Assets.zip",
        "hytale-downloader.zip",
        "QUICKSTART.md",
        "hytale-downloader-linux-amd64",
        "hytale-downloader-windows-amd64.exe",
        "start.bat",
        "start.sh",
        "Server" 
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
    
    let config_json_path = base_path.join("config.json");
    if !config_json_path.exists() {
        let auth_default = "authenticated".to_string();
        let auth_mode = server.config.as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v.get("auth_mode").map(|v| v.as_str().unwrap_or("authenticated").to_string()))
            .unwrap_or(auth_default);
            
        let hytale_config = templates::generate_config_json(
            &server.name,
            100, 
            &auth_mode
        );
        if let Ok(mut config_file) = fs::File::create(&config_json_path).await {
            let _ = config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes()).await;
        }
    }

    spawn_hytale_installation(state.pool.clone(), pm.clone(), id.clone(), base_path.to_path_buf());

    Ok(Json(serde_json::json!({ 
        "success": true,
        "message": "Reinstallation started",
        "working_dir": base_path.to_string_lossy()
    })))
}

// Helpers
pub fn spawn_hytale_installation(pool: crate::core::database::DbPool, pm: crate::services::game::ProcessManager, id: String, server_path: PathBuf) {
    tokio::spawn(async move {
        let (tx_start, rx_start) = tokio::sync::oneshot::channel::<()>();
        
        let pm_inner = pm.clone();
        let id_inner = id.clone();
        let server_path_inner = server_path.clone();
        
        let handle = tokio::spawn(async move {
            if rx_start.await.is_err() {
                return; 
            }
            
            let logs_dir = server_path_inner.join("logs");
            if !logs_dir.exists() {
                 let _ = tokio::fs::create_dir_all(&logs_dir).await;
            }
            let install_log_path = logs_dir.join("install.log");
            let _ = tokio::fs::write(&install_log_path, "Starting Hytale Server Installation...\n").await;
            
            let log_file = tokio::fs::OpenOptions::new()
                .create(true).append(true).open(&install_log_path).await.ok()
                .map(|f| std::sync::Arc::new(tokio::sync::Mutex::new(f)));

            let broadcast = |msg: String| {
                let pm = pm_inner.clone();
                let id = id_inner.clone();
                let log_file = log_file.clone();
                async move {
                    if (msg.contains("IMPORTANT") && (msg.contains("authentifier") || msg.contains("authenticate"))) ||
                       (msg.contains("[HytaleServer] No server tokens configured")) ||
                       (msg.contains("/auth login to authenticate")) {
                        pm.set_auth_required(&id, true);
                    }
                    pm.broadcast_log(&id, msg.clone()).await;
                    if let Some(f) = log_file {
                        let mut guard = f.lock().await;
                        let _ = guard.write_all(format!("{msg}\n").as_bytes()).await;
                    }
                }
            };

            broadcast("🚀 Initialization de l'installation du serveur...".to_string()).await;

            let zip_url = "https://downloader.hytale.com/hytale-downloader.zip";
            let zip_name = "hytale-downloader.zip";
            let dest_path = server_path_inner.join(zip_name);

            broadcast(format!("⬇️ Téléchargement de Hytale Downloader depuis {zip_url}...")).await;
            
            if let Err(e) = run_with_logs(
                tokio::process::Command::new("curl")
                    .arg("-L").arg("-o").arg(&dest_path).arg(zip_url),
                pm_inner.clone(), id_inner.clone(), "", Some(install_log_path.clone())
            ).await {
                broadcast(format!("❌ {e}")).await;
                 pm_inner.remove(&id_inner).await;
                 return;
            }
            
            broadcast("✅ Téléchargement terminé.".to_string()).await;
            broadcast("📦 Extraction de l'archive...".to_string()).await;
            
            if let Err(e) = run_with_logs(
                tokio::process::Command::new("unzip")
                    .arg("-o").arg(&dest_path).arg("-d").arg(&server_path_inner),
                pm_inner.clone(), id_inner.clone(), "", Some(install_log_path.clone())
            ).await {
                broadcast(format!("❌ {e}")).await;
                pm_inner.remove(&id_inner).await;
                return;
            }
            broadcast("✅ Extraction terminée.".to_string()).await;
            broadcast("🧹 Nettoyage des fichiers temporaires...".to_string()).await;
            
            let _ = tokio::fs::remove_file(&dest_path).await;
            let _ = tokio::fs::remove_file(server_path_inner.join("QUICKSTART.md")).await;

            let mut executable_name = "hytale-downloader-linux-amd64".to_string();
            let windows_binary = "hytale-downloader-windows-amd64.exe";
            let linux_binary = "hytale-downloader-linux-amd64";

            if std::env::consts::OS == "linux" {
                executable_name = linux_binary.to_string();
                let _ = tokio::fs::remove_file(server_path_inner.join(windows_binary)).await;
            } else if std::env::consts::OS == "windows" {
                 executable_name = windows_binary.to_string();
                 let _ = tokio::fs::remove_file(server_path_inner.join(linux_binary)).await;
            } else if cfg!(target_os = "macos") {
                 broadcast("⚠️ Attention : macOS détecté. Le Hytale Downloader (Linux binary) peut ne pas fonctionner nativement.".to_string()).await;
                 executable_name = linux_binary.to_string(); 
                 let _ = tokio::fs::remove_file(server_path_inner.join(windows_binary)).await;
            }
            
            let executable_path = server_path_inner.join(&executable_name);
            if std::env::consts::OS != "windows" {
                let _ = tokio::process::Command::new("chmod").arg("+x").arg(&executable_path).status().await;
            }

            broadcast(format!("⏳ Exécution du downloader ({executable_name}) pour récupérer le serveur...")).await;
            broadcast("⚠️ IMPORTANT : Le downloader va vous demander de vous authentifier via une URL.".to_string()).await;
            
            if let Err(e) = run_with_logs(
                tokio::process::Command::new(&executable_path).current_dir(&server_path_inner),
                pm_inner.clone(), id_inner.clone(), "", Some(install_log_path.clone())
            ).await {
                broadcast(format!("❌ {e}")).await;
            } else {
                broadcast("✅ Downloader terminé avec succès.".to_string()).await;
            }

            if let Ok(mut entries) = tokio::fs::read_dir(&server_path_inner).await {
                 while let Ok(Some(entry)) = entries.next_entry().await {
                     let path = entry.path();
                     if let Some(ext) = path.extension() {
                         if ext == "zip" {
                              let file_name = path.file_name().unwrap().to_string_lossy();
                              if file_name != "hytale-downloader.zip" && file_name != "Assets.zip" {
                                  broadcast(format!("📦 Décompression du serveur : {file_name}...")).await;
                                  if let Err(e) = run_with_logs(
                                     tokio::process::Command::new("unzip").arg("-o").arg(&path).arg("-d").arg(&server_path_inner),
                                     pm_inner.clone(), id_inner.clone(), "", Some(install_log_path.clone())
                                  ).await {
                                      broadcast(format!("❌ Erreur extraction: {e}")).await;
                                  } else {
                                     broadcast("✅ Décompression terminée.".to_string()).await;
                                     let _ = tokio::fs::remove_file(&path).await;
                                 }
                              }
                         }
                     }
                 }
            }

            let nested_bundle_dir = server_path_inner.join("Server");
            let _ = tokio::fs::remove_file(server_path_inner.join("start.bat")).await;
            let _ = tokio::fs::remove_file(server_path_inner.join("start.sh")).await;
            if nested_bundle_dir.exists() {
                 let _ = tokio::fs::remove_file(nested_bundle_dir.join("start.bat")).await;
                 let _ = tokio::fs::remove_file(nested_bundle_dir.join("start.sh")).await;
            }

            let nested_jar_path = nested_bundle_dir.join("HytaleServer.jar");
            if nested_jar_path.exists() {
                 broadcast("✨ HytaleServer.jar présent. Installation terminée !".to_string()).await;
                 let _ = sqlx::query("UPDATE servers SET executable_path = ? WHERE id = ?")
                    .bind("Server/HytaleServer.jar")
                    .bind(&id_inner)
                    .execute(&pool)
                    .await;
            } else {
                 broadcast("⚠️ Attention: HytaleServer.jar non trouvé après exécution.".to_string()).await;
            }
            pm_inner.remove(&id_inner).await;
        });

        let working_dir_str = server_path.to_string_lossy().to_string();
        if let Err(e) = pm.register_installing(&id, &working_dir_str, Some(handle.abort_handle())).await {
            error!("Failed to register installing process: {}", e);
            handle.abort(); 
        } else {
            let _ = tx_start.send(());
        }
    });
}

async fn run_with_logs(
    cmd: &mut tokio::process::Command, 
    pm: crate::services::game::ProcessManager, 
    id: String, 
    log_prefix: &str,
    log_file_path: Option<PathBuf>
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    
    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut stdout_reader = tokio::io::BufReader::new(stdout);
    let mut stderr_reader = tokio::io::BufReader::new(stderr);

    let file_writer = if let Some(path) = log_file_path {
        tokio::fs::OpenOptions::new().create(true).append(true).open(path).await.ok()
            .map(|f| std::sync::Arc::new(tokio::sync::Mutex::new(f)))
    } else { None };

    let pm1 = pm.clone(); let id1 = id.clone(); let p1 = log_prefix.to_string(); let fw1 = file_writer.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        while let Ok(byte) = stdout_reader.read_u8().await {
            if byte == b'\n' || byte == b'\r' {
                if !buffer.is_empty() {
                    let line = String::from_utf8_lossy(&buffer).to_string();
                    pm1.broadcast_log(&id1, format!("{p1}{line}")).await;
                    if let Some(writer) = &fw1 {
                        let mut guard = writer.lock().await;
                        let _ = guard.write_all(line.as_bytes()).await;
                        let _ = guard.write_all(b"\n").await;
                    }
                    buffer.clear();
                }
            } else { buffer.push(byte); }
        }
    });

    let pm2 = pm.clone(); let id2 = id.clone(); let p2 = log_prefix.to_string(); let fw2 = file_writer.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        while let Ok(byte) = stderr_reader.read_u8().await {
             if byte == b'\n' || byte == b'\r' {
                if !buffer.is_empty() {
                    let line = String::from_utf8_lossy(&buffer).to_string();
                    pm2.broadcast_log(&id2, format!("{p2}[ERR] {line}")).await;
                    if let Some(writer) = &fw2 {
                        let mut guard = writer.lock().await;
                        let _ = guard.write_all(line.as_bytes()).await;
                        let _ = guard.write_all(b"\n").await;
                    }
                    buffer.clear();
                }
            } else { buffer.push(byte); }
        }
    });

    let status = child.wait().await;
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("Command failed with exit code: {:?}", s.code())),
        Err(e) => Err(format!("Failed to wait for command: {e}")),
    }
}