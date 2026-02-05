use axum::{
    extract::{Path, State},
    Json,
    http::StatusCode,
};
use tracing::{info, error};
use std::path::{Path as StdPath};
use chrono::Utc;
use walkdir::WalkDir;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::core::AppState;
use crate::core::error::AppError;
use crate::utils::memory::{parse_memory_to_bytes, calculate_total_memory};
use crate::utils::templates;
use crate::core::database::DbPool;

use crate::api::servers::models::{ServerRow, ServerResponse, CreateServerRequest, Player, PlayerRow};
use super::lifecycle::spawn_hytale_installation;

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

        let mut players_vec = Vec::new();
        if is_running {
            if let Some(online) = pm.get_online_players(&s.id).await {
                for p_name in online {
                     players_vec.push(Player {
                         name: p_name,
                         uuid: None,
                         is_online: true,
                         last_seen: Utc::now().to_rfc3339(),
                         player_ip: None,
                         is_op: false,
                         is_banned: false,
                         is_whitelisted: false,
                     });
                }
            }
        }
        let players = if players_vec.is_empty() { None } else { Some(players_vec) };

        let config_json = s.config.as_ref().and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
        let mut max_players = config_json.as_ref()
            .and_then(|c| c.get("MaxPlayers"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        if max_players.is_none() {
            let config_path = StdPath::new(&s.working_dir).join("config.json");
            if let Ok(content) = fs::read_to_string(config_path).await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    max_players = json.get("MaxPlayers").and_then(|v| v.as_u64()).map(|v| v as u32);
                }
            }
        }

        let started_at = pm.get_server_started_at(&s.id).await;
        let (cpu, cpu_norm, mem, mut disk) = pm.get_metrics_data(&s.id).await;

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
    let directories = [&server_base_path];

    for dir in directories {
        if let Err(e) = fs::create_dir_all(dir).await {
            return Err(AppError::Internal(format!(
                "Failed to create directory {dir:?}: {e}"
            )));
        }
    }

    info!("Created server directory structure at {:?}", server_base_path);

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

    let mut final_executable = body.executable_path.clone();
    let install_path = server_base_path.clone();

    if body.game_type == "hytale" {
        spawn_hytale_installation(state.pool.clone(), state.process_manager.clone(), id.clone(), install_path.clone());
        final_executable = "Server/HytaleServer.jar".to_string(); 
    }

    let config_str = body.config.as_ref().map(|c| c.to_string());
    let actual_working_dir = server_base_path.to_str().unwrap_or(&body.working_dir);
    let actual_executable_str = &final_executable;

    let hytale_config = templates::generate_config_json(server_name, 100, auth_mode);
    let config_json_path = server_base_path.join("config.json");
    let mut config_file = fs::File::create(&config_json_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to create config.json: {e}"))
    })?;
    config_file.write_all(serde_json::to_string_pretty(&hytale_config).unwrap().as_bytes())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to write config.json: {e}")))?;

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
    
    let player_rows: Vec<PlayerRow> = sqlx::query_as(
        "SELECT player_name, player_id, player_ip, is_online, last_seen FROM server_players WHERE server_id = ?"
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut players_map: std::collections::HashMap<String, Player> = player_rows.into_iter().map(|row| (row.player_name.clone(), Player {
        name: row.player_name,
        uuid: row.player_id,
        is_online: row.is_online != 0,
        last_seen: row.last_seen,
        player_ip: row.player_ip,
        is_op: false,
        is_banned: false,
        is_whitelisted: false,
    })).collect();

    // Merge with real-time online players
    if let Some(online_names) = pm.get_online_players(&id).await {
        for name in online_names {
            players_map.entry(name.clone())
                .and_modify(|p| {
                     p.is_online = true; 
                     p.last_seen = chrono::Utc::now().to_rfc3339();
                })
                .or_insert(Player {
                    name: name.clone(),
                    uuid: None, // We don't have UUID from pm.get_online_players (only names)
                    is_online: true,
                    last_seen: chrono::Utc::now().to_rfc3339(),
                    player_ip: None,
                    is_op: false,
                    is_banned: false,
                    is_whitelisted: false,
                });
        }
    }

    // Load meta from server files (whitelist, etc.)
    let meta = load_player_meta(&server.working_dir).await;
    for (key, m) in &meta {
        // Try to find existing player by Name (key) OR UUID (key)
        let mut target_name = None;
        
        if players_map.contains_key(key) {
            target_name = Some(key.clone());
        } else {
            // Check if 'key' is an UUID that matches an existing player's UUID
            for (p_name, p) in &players_map {
                if let Some(uid) = &p.uuid {
                    if uid == key {
                        target_name = Some(p_name.clone());
                        break;
                    }
                }
            }
        }

        if let Some(t_name) = target_name {
            players_map.entry(t_name)
                .and_modify(|p| {
                    p.is_op = m.is_op;
                    p.is_banned = m.is_banned;
                    p.is_whitelisted = m.is_whitelisted;
                    // If we matched by name but didn't have UUID, and key looks like UUID, save it
                    if p.uuid.is_none() && (key.len() == 36 || key.len() == 32) {
                         p.uuid = Some(key.clone());
                    }
                });
        } else {
            // New entry not found in DB or online
            // If key looks like UUID, put it in uuid field. Name will be UUID for now (frontend can handle display)
            let is_uuid = key.len() == 36 || (key.len() == 32 && !key.contains(' '));
            let uuid = if is_uuid { Some(key.clone()) } else { None };
            
            players_map.insert(key.clone(), Player {
                name: key.clone(),
                uuid,
                is_online: false,
                last_seen: "Jamais".to_string(), 
                player_ip: None,
                is_op: m.is_op,
                is_banned: m.is_banned,
                is_whitelisted: m.is_whitelisted,
            });
        }
    }

    let mut final_players: Vec<Player> = players_map.into_values().collect();
    final_players.sort_by(|a, b| {
        b.is_online.cmp(&a.is_online)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
    });

    let players = Some(final_players);

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
        
        // Prepare mapped config
        let mut mapped_vals = templates::map_to_hytale_config(config_json);
        
        // Also inject top-level body fields if they are present and might be newer
        if let Some(port) = body.port {
            mapped_vals["Port"] = serde_json::json!(port);
        }
        if let Some(auth_mode) = &body.auth_mode {
            let auth_store = if auth_mode == "authenticated" {
                serde_json::json!({ "Type": "Encrypted", "Path": "auth.enc" })
            } else {
                serde_json::json!({ "Type": "None" })
            };
            mapped_vals["AuthCredentialStore"] = auth_store;
        }
        mapped_vals["ServerName"] = serde_json::json!(body.name);

        // Use helper to merge with existing file if it exists
        async fn merge_and_write(path: &std::path::Path, new_vals: &serde_json::Value) -> Result<(), std::io::Error> {
            let mut current_config = if path.exists() {
                let content = tokio::fs::read_to_string(path).await?;
                serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
            } else {
                serde_json::json!({})
            };

            crate::utils::templates::deep_merge(&mut current_config, new_vals);
            let json_str = serde_json::to_string_pretty(&current_config)?;
            tokio::fs::write(path, json_str).await?;
            Ok(())
        }

        if let Err(e) = merge_and_write(&root_config_path, &mapped_vals).await {
            error!("Failed to merge/write root config.json for server {}: {}", id, e);
        }

        if server_dir.exists() {
            if !universe_dir.exists() {
                let _ = tokio::fs::create_dir_all(&universe_dir).await;
            }
            if let Err(e) = merge_and_write(&nested_config_path, &mapped_vals).await {
                error!("Failed to merge/write nested config.json for server {}: {}", id, e);
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

pub async fn get_server_by_id_internal(pool: &DbPool, id: &str) -> Result<ServerRow, AppError> {
    sqlx::query_as("SELECT * FROM servers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))
}

struct PlayerMeta {
    is_op: bool,
    is_whitelisted: bool,
    is_banned: bool,
}

async fn load_player_meta(working_dir: &str) -> std::collections::HashMap<String, PlayerMeta> {
    let mut meta_map = std::collections::HashMap::new();
    let base_path = StdPath::new(working_dir);
    let server_path = base_path.join("server");

    // Helper to try multiple paths
    let try_paths = |filename: &str| {
        let p1 = server_path.join(filename);
        let p2 = base_path.join(filename);
        if p1.exists() { Some(p1) }
        else if p2.exists() { Some(p2) }
        else { None }
    };

    // OPs (permissions.json)
    if let Some(path) = try_paths("permissions.json") {
        if let Ok(c) = fs::read_to_string(&path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
                 if let Some(users) = json.get("users").and_then(|u| u.as_object()) {
                     for uuid in users.keys() {
                         meta_map.entry(uuid.to_string()).or_insert(PlayerMeta { is_op: true, is_whitelisted: false, is_banned: false }).is_op = true;
                     }
                 }
            }
        }
    }
    
    // Whitelist
    if let Some(path) = try_paths("whitelist.json") {
        if let Ok(c) = fs::read_to_string(&path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
                 // Try array format
                 if let Some(arr) = json.as_array() {
                     for item in arr {
                         if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                             meta_map.entry(name.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: true, is_banned: false }).is_whitelisted = true;
                         }
                     }
                 } 
                 // Try Hytale object format { "list": [...] }
                 else if let Some(list) = json.get("list").and_then(|l| l.as_array()) {
                     for item in list {
                         if let Some(s) = item.as_str() {
                             meta_map.entry(s.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: true, is_banned: false }).is_whitelisted = true;
                         }
                     }
                 }
            }
        }
    }

    // Bans
    if let Some(path) = try_paths("bans.json") {
        if let Ok(c) = fs::read_to_string(&path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&c) {
                 if let Some(arr) = json.as_array() {
                     for item in arr {
                         if let Some(target) = item.get("target").and_then(|v| v.as_str()) {
                             meta_map.entry(target.to_string()).or_insert(PlayerMeta { is_op: false, is_whitelisted: false, is_banned: true }).is_banned = true;
                         }
                     }
                 }
            }
        }
    }

    meta_map
}
