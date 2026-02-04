use axum::{
    extract::{Path, State},
    Json,
    http::StatusCode,
};
use tracing::{info, error};
use std::path::{Path as StdPath, PathBuf};
use chrono::Utc;
use walkdir::WalkDir;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{AppState, error::AppError};
use crate::utils::memory::{parse_memory_to_bytes, calculate_total_memory};
use crate::templates;
use crate::services::ProcessManager;
use crate::db::DbPool;

use super::models::{ServerRow, ServerResponse, CreateServerRequest, Player, PlayerRow, CommandRequest};

pub async fn list_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ServerResponse>>, AppError> {
    let servers: Vec<ServerRow> = sqlx::query_as(
        "SELECT * FROM servers"
    )
    .fetch_all(&state.pool)
    .await?;

    let mut responses = Vec::new();
    let pm = &state.process_manager;
    
    for s in servers {
        // Check if the working directory exists
        let dir_exists = StdPath::new(&s.working_dir).exists();
        let is_running = pm.is_running(&s.id);
        
        let status = if !dir_exists { 
            "missing" 
        } else if pm.is_installing(&s.id) {
            if pm.is_auth_required(&s.id) { "auth_required" } else { "installing" }
        } else if is_running {
             if pm.is_auth_required(&s.id) { "auth_required" } else { "running" }
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
            let config_path = StdPath::new(&s.working_dir).join("config.json");
            if let Ok(content) = fs::read_to_string(config_path).await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
                }
            }
        }

        let started_at = pm.get_server_started_at(&s.id).await;
        let (cpu, cpu_norm, mem, mut disk) = pm.get_metrics_data(&s.id).await;

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
            cpu_usage_normalized: cpu_norm,
            memory_usage_bytes: mem,
            max_memory_bytes: total_bytes,
            max_heap_bytes: heap_bytes,
            disk_usage_bytes: disk,
            started_at,
        });
    }

    Ok(Json(responses))
}

pub async fn create_server(
    State(state): State<AppState>,
    Json(body): Json<CreateServerRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let auto_start = body.auto_start.unwrap_or(false) as i32;

    let server_base_path = StdPath::new(&body.working_dir).join(&id);
    // Create base directories
    let directories = [
        &server_base_path,
    ];

    for dir in directories {
        if let Err(e) = fs::create_dir_all(dir).await {
            return Err(AppError::Internal(format!(
                "Failed to create directory {dir:?}: {e}"
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
    let install_path = server_base_path.clone();

    if body.game_type == "hytale" {
        spawn_hytale_installation(state.pool.clone(), state.process_manager.clone(), id.clone(), install_path.clone());
        
        final_executable = "Server/HytaleServer.jar".to_string(); 
    }

    let config_str = body.config.as_ref().map(|c| c.to_string());

    let actual_working_dir = server_base_path.to_str().unwrap_or(&body.working_dir);
    let actual_executable_str = &final_executable;

    // Generate and write config.json (Hytale server config) at ROOT
    let hytale_config = templates::generate_config_json(
        server_name,
        100, 
        auth_mode
    );
    let config_json_path = server_base_path.join("config.json");
    let mut config_file = fs::File::create(&config_json_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to create config.json: {e}"))
    })?;
    config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write config.json: {e}")))?;

    info!("Generated Hytale config.json for server {}", id);

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
    .bind(config_str) 
    .bind(auto_start)
    .bind(&now)
    .bind(&now)
    .bind(auth_mode)
    .bind(bind_address)
    .bind(port)
    .execute(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ 
        "id": id,
        "working_dir": actual_working_dir,
        "message": "servers.create_success_message"
    }))))
}

pub async fn get_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerResponse>, AppError> {

    let server: ServerRow = sqlx::query_as(
        "SELECT * FROM servers WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;

    let pm = &state.process_manager;
    let dir_exists = StdPath::new(&server.working_dir).exists();
    let is_running = pm.is_running(&server.id);
    let status = if !dir_exists {
        "missing"
    } else if pm.is_installing(&server.id) {
        if pm.is_auth_required(&server.id) { "auth_required" } else { "installing" }
    } else if is_running {
        if pm.is_auth_required(&server.id) { "auth_required" } else { "running" }
    } else {
        "stopped"
    };
    
    // Fetch persistent players from DB
    let player_rows: Vec<PlayerRow> = sqlx::query_as(
        "SELECT player_name, is_online, last_seen FROM server_players WHERE server_id = ?"
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut players_map: std::collections::HashMap<String, Player> = player_rows.into_iter().map(|row| (row.player_name.clone(), Player {
        name: row.player_name,
        is_online: row.is_online != 0,
        last_seen: row.last_seen,
        is_op: false,
        is_banned: false,
        is_whitelisted: false,
    })).collect();

    // Merge with real-time in-memory players
    if let Some(online_names) = pm.get_online_players(&id).await {
        for name in online_names {
            players_map.entry(name.clone())
                .and_modify(|p| {
                     p.is_online = true; 
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

    let meta = load_player_meta(&server.working_dir).await;
    
    for (name, m) in &meta {
        players_map.entry(name.clone()).or_insert(Player {
            name: name.clone(),
            is_online: false,
            last_seen: "Jamais".to_string(), 
            is_op: m.is_op,
            is_banned: m.is_banned,
            is_whitelisted: m.is_whitelisted,
        });
    }

    for (name, p) in players_map.iter_mut() {
        if let Some(m) = meta.get(name) {
            p.is_op = m.is_op;
            p.is_banned = m.is_banned;
            p.is_whitelisted = m.is_whitelisted;
        }
    }

    let mut final_players: Vec<Player> = players_map.into_values().collect();
    final_players.sort_by(|a, b| {
        b.is_online.cmp(&a.is_online)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });

    let players = if final_players.is_empty() { None } else { Some(final_players) };

    let config_json = server.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
    let mut max_players = config_json.as_ref()
        .and_then(|c| c.get("MaxPlayers"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if max_players.is_none() {
        let config_path = StdPath::new(&server.working_dir).join("config.json");
        if let Ok(content) = fs::read_to_string(config_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
            }
        }
    }

    let port = Some(server.port as u16);
    let bind_address = Some(server.bind_address.clone());

    let started_at = pm.get_server_started_at(&server.id).await;
    let (cpu, cpu_norm, mem, mut disk) = pm.get_metrics_data(&server.id).await;

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

    let notifications = server.discord_notifications.as_ref()
        .and_then(|n| serde_json::from_str(n).ok());

    Ok(Json(ServerResponse {
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
        cpu_usage_normalized: cpu_norm,
        memory_usage_bytes: mem,
        max_memory_bytes: total_bytes,
        max_heap_bytes: heap_bytes,
        disk_usage_bytes: disk,
        started_at,
    }))
}

pub async fn kill_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.process_manager.kill(&id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn update_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateServerRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("servers.not_found".into()));
    }

    if let Some(config_json) = &body.config {
        let root_config_path = StdPath::new(&body.working_dir).join("config.json");
        let server_dir = StdPath::new(&body.working_dir).join("server");
        let universe_dir = server_dir.join("universe");
        let nested_config_path = universe_dir.join("config.json");
        
        if let Ok(json_str) = serde_json::to_string_pretty(config_json) {
            if let Err(e) = tokio::fs::write(&root_config_path, &json_str).await {
                error!("Failed to write root config.json for server {}: {}", id, e);
            }
            if server_dir.exists() {
                 if !universe_dir.exists() {
                     let _ = tokio::fs::create_dir_all(&universe_dir).await;
                 }
                 if let Err(e) = tokio::fs::write(&nested_config_path, &json_str).await {
                    error!("Failed to write nested server/universe/config.json for server {}: {}", id, e);
                }
            }
        }
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn delete_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let server: Option<(String,)> = sqlx::query_as("SELECT working_dir FROM servers WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?;

    let pm = &state.process_manager;
    if pm.is_running(&id) {
        pm.stop(&id).await?;
    }

    let result = sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("servers.not_found".into()));
    }

    if let Some((working_dir,)) = server {
        let path = StdPath::new(&working_dir);
        if path.exists() {
             if let Err(e) = tokio::fs::remove_dir_all(path).await {
                 error!("Failed to remove server directory {}: {}", working_dir, e);
             } else {
                 info!("Removed server directory: {}", working_dir);
             }
        }
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

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

    let hytale_config = templates::generate_config_json(
        &server.name,
        max_players, 
        auth_mode 
    );
    
    let mut hytale_config_obj = serde_json::to_value(hytale_config).unwrap();
    if let Some(obj) = hytale_config_obj.as_object_mut() {
        obj.insert("Port".to_string(), serde_json::json!(port));
    }
    if let Ok(mut config_file) = fs::File::create(&config_json_path).await {
         let _ = config_file.write_all(serde_json::to_string_pretty(&hytale_config_obj).unwrap().as_bytes()).await;
    }

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
            crate::services::discord_service::send_notification(
                &pool_clone,
                "🟢 Serveur Démarré",
                &format!("Le serveur **{server_name}** a été démarré."),
                crate::services::discord_service::COLOR_SUCCESS,
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

    let process_working_dir = StdPath::new(&server.working_dir).join("server");
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

pub async fn send_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CommandRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.process_manager.send_command(&id, &body.command).await?;
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
fn spawn_hytale_installation(pool: DbPool, pm: ProcessManager, id: String, server_path: PathBuf) {
    tokio::spawn(async move {
        let (tx_start, rx_start) = tokio::sync::oneshot::channel::<()>();
        
        let pm_inner = pm.clone();
        let id_inner = id.clone();
        let server_path_inner = server_path.clone();
        
        let handle = tokio::spawn(async move {
            // Wait for registration to complete
            if rx_start.await.is_err() {
                return; // Aborted before start
            }
            
            let _working_dir_str = server_path_inner.to_string_lossy().to_string();
            // Note: register_installing is done by parent
            
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
            
            // 1. Download
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
            
            // 2. Unzip
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

        // Register the task
        let working_dir_str = server_path.to_string_lossy().to_string();
        if let Err(e) = pm.register_installing(&id, &working_dir_str, Some(handle.abort_handle())).await {
            error!("Failed to register installing process: {}", e);
            handle.abort(); // Cancel the task since we couldn't register it
        } else {
            // Signal the task to start
            let _ = tx_start.send(());
        }
    });
}

async fn run_with_logs(
    cmd: &mut tokio::process::Command, 
    pm: ProcessManager, 
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

struct PlayerMeta {
    is_op: bool,
    is_whitelisted: bool,
    is_banned: bool,
}

async fn load_player_meta(working_dir: &str) -> std::collections::HashMap<String, PlayerMeta> {
    let mut meta_map = std::collections::HashMap::new();
    let server_path = StdPath::new(working_dir).join("server");

    // OPs
    let path = server_path.join("permissions.json");
    if let Ok(c) = fs::read_to_string(&path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
             if let Some(arr) = json.as_array() {
                 for item in arr {
                     if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                         meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: true, is_whitelisted: false, is_banned: false }).is_op = true;
                     }
                 }
             }
        }
    }
    
    // Whitelist
    let path = server_path.join("whitelist.json");
    if let Ok(c) = fs::read_to_string(&path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
             if let Some(arr) = json.as_array() {
                 for item in arr {
                     if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                         meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: true, is_banned: false }).is_whitelisted = true;
                     }
                 }
             }
        }
    }

    // Bans
    let path = server_path.join("bans.json");
    if let Ok(c) = fs::read_to_string(&path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
             if let Some(arr) = json.as_array() {
                 for item in arr {
                     if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                         meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: false, is_banned: true }).is_banned = true;
                     }
                 }
             }
        }
    }

    meta_map
}
