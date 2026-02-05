use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::Path as StdPath;
use tokio::fs;
use crate::{core::error::AppError as ApiError, core::AppState};
use crate::api::auth::AuthUser;
use super::crud::get_server_by_id_internal;

// ================= MODELS =================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WhitelistEntry {
    pub name: String,
    pub uuid: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BanEntry {
    pub target: String, // UUID usually
    pub by: String,
    pub reason: String,
    pub timestamp: i64,
    #[serde(rename = "type")]
    pub ban_type: String, // "infinite" etc
    // Optional fields for display if we can resolve names
    pub username: Option<String>,
    #[serde(rename = "bannedBy")]
    pub banned_by: Option<String>, 
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpEntry {
    pub uuid: String,
    pub groups: Vec<String>,
}

// Requests
#[derive(Debug, Deserialize)]
pub struct AddWhitelistRequest {
    pub name: String,
    pub uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveWhitelistRequest {
    pub name: Option<String>,
    pub uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddBanRequest {
    pub target: String, // UUID
    pub reason: String,
    #[allow(dead_code)]
    pub duration: Option<u64>, // Not used yet for Hytale bans which seem to be infinite or not
}

#[derive(Debug, Deserialize)]
pub struct AddOpRequest {
    pub uuid: String,
    pub group: Option<String>,
}

// ================= HANDLERS =================

fn get_player_file_path(working_dir: &str, filename: &str) -> std::path::PathBuf {
    let base_path = StdPath::new(working_dir);
    let server_path = base_path.join("server").join(filename);
    if server_path.exists() {
        server_path
    } else {
        base_path.join(filename)
    }
}

// --- WHITELIST ---

pub async fn get_whitelist(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let list = get_whitelist_internal(&state.pool, &id).await?;
    Ok(Json(list))
}

pub async fn get_whitelist_internal(pool: &crate::core::database::DbPool, id: &str) -> Result<Vec<WhitelistEntry>, ApiError> {
    let server = get_server_by_id_internal(pool, id).await?;
    let path = get_player_file_path(&server.working_dir, "whitelist.json");
    
    // Default empty
    let mut list: Vec<WhitelistEntry> = Vec::new();

    if path.exists() {
        let content = fs::read_to_string(&path).await.map_err(|e| ApiError::Internal(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!([]));
        
        if let Some(arr) = json.as_array() {
            // Flat array of names or objects?
            for item in arr {
                if let Some(str_val) = item.as_str() {
                     list.push(WhitelistEntry { name: str_val.to_string(), uuid: None });
                } else if let Some(obj) = item.as_object() {
                    // Try to parse Hytale format if it matches or standard MC
                    let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    let uuid = obj.get("uuid").and_then(|v| v.as_str()).map(|s| s.to_string());
                    list.push(WhitelistEntry { name, uuid });
                }
            }
        } else if let Some(obj) = json.as_object() {
             // Hytale format: { "list": ["uuid", "uuid"] } ? Or { "list": [...] }
             if let Some(l) = obj.get("list").and_then(|v| v.as_array()) {
                 for item in l {
                    if let Some(str_val) = item.as_str() {
                         list.push(WhitelistEntry { name: str_val.to_string(), uuid: Some(str_val.to_string()) });
                    }
                 }
             }
        }
    }
    Ok(list)
}

pub async fn add_whitelist(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<AddWhitelistRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "whitelist.json");

    // We only support appending to Hytale structure which seems to be { "list": [...] } or standard []
    // But wait, the user showed us `whitelist.json`? No, only `bans.json` was shown explicitly.
    // I entered `server/whitelist.json` for Hytale usually.
    
    // Simplistic implementation: READ -> MODIFY -> WRITE
    let mut current_list = get_whitelist_internal(&state.pool, &id).await?;
    
    // Check duplicate
    if current_list.iter().any(|e| e.name.eq_ignore_ascii_case(&payload.name) || (payload.uuid.is_some() && e.uuid == payload.uuid)) {
        return Ok(Json(serde_json::json!({ "status": "exists" })));
    }

    current_list.push(WhitelistEntry {
        name: payload.name.clone(),
        uuid: payload.uuid.clone().or_else(|| Some(payload.name.clone())), // Fallback UUID=Name for offline?
    });

    // Write back. Which format? Let's use generic list object for Hytale if that's what it expects, 
    // or flat array if we detected it was flat.
    // Actually, for Hytale server, standard `whitelist.json` is often `{"list": ["name1"]}` for names?
    // Let's write as { "list": [uuid keys] } if we want to follow strict Hytale
    // BUT we need to support User's specific setup.
    // I will write as I read. IF it was object with list, I keep it. Default to flat array if new.
    
    // Since I parsed into Vec<WhitelistEntry>, I lost the original file structure detail.
    // I'll stick to a safe default: standard JSON array of objects if standard MC, or Hytale list.
    // Let's look at `bans.json` provided: `[{...}]`. Flat array.
    // So let's assume `whitelist.json` is likely `{"list": [...]}` or `[...]`.
    
    // Re-read raw to detect format type
    let content = if path.exists() { fs::read_to_string(&path).await.unwrap_or_default() } else { "{}".to_string() };
    let is_flat_array = content.trim().starts_with('[');
    
    if is_flat_array {
        // Write as flat array of objects
        let json_list: Vec<serde_json::Value> = current_list.iter().map(|e| {
            serde_json::json!({
                "name": e.name,
                "uuid": e.uuid
            })
        }).collect();
        fs::write(&path, serde_json::to_string_pretty(&json_list).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
         // Assume object { "list": [...] }
         // For Hytale, list usually contains Names or UUIDs strings?
         // Let's write strings of names/uuids.
         let str_list: Vec<String> = current_list.iter().map(|e| e.name.clone()).collect();
         let wrapper = serde_json::json!({ "list": str_list });
         fs::write(&path, serde_json::to_string_pretty(&wrapper).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    Ok(Json(serde_json::json!({ "status": "ok", "entry": current_list.last() })))
}

pub async fn remove_whitelist(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<RemoveWhitelistRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "whitelist.json");
    
    if !path.exists() { return Ok(Json(serde_json::json!({ "status": "ok" }))); }

    let content = fs::read_to_string(&path).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let is_flat_array = content.trim().starts_with('[');
    
    if is_flat_array {
        let mut json: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
        json.retain(|item| {
            let name = item.get("name").and_then(|v| v.as_str());
            let uuid = item.get("uuid").and_then(|v| v.as_str());
            
            if let Some(target_uuid) = &payload.uuid {
                if uuid == Some(target_uuid) { return false; }
            }
            if let Some(target_name) = &payload.name {
                 if name == Some(target_name) { return false; }
            }
            true
        });
        fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
        // Object { list: [] }
         if let Ok(mut obj) = serde_json::from_str::<serde_json::Value>(&content) {
             if let Some(list) = obj.get_mut("list").and_then(|l| l.as_array_mut()) {
                 list.retain(|v| {
                     let s = v.as_str().unwrap_or("");
                     if let Some(target_uuid) = &payload.uuid {
                         if s == target_uuid { return false; }
                     }
                     if let Some(target_name) = &payload.name {
                         if s == target_name { return false; }
                     }
                     true
                 });
                 fs::write(&path, serde_json::to_string_pretty(&obj).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
             }
         }
    }

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

// --- BANS ---
// Schema: [{"type":"infinite","target":"uuid","by":"uuid","timestamp":123,"reason":"..."}]

pub async fn get_bans(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "bans.json");
    
    if !path.exists() { return Ok(Json(Vec::<BanEntry>::new())); }

    let content = fs::read_to_string(&path).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let bans: Vec<BanEntry> = serde_json::from_str(&content).unwrap_or_default();
    
    Ok(Json(bans))
}

pub async fn add_ban(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<AddBanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "bans.json");
    
    let mut bans: Vec<BanEntry> = if path.exists() {
        let c = fs::read_to_string(&path).await.unwrap_or_default();
        serde_json::from_str(&c).unwrap_or_default()
    } else {
        Vec::new()
    };
    
    // Check exists
    if bans.iter().any(|b| b.target == payload.target) {
        return Ok(Json(serde_json::json!({"status": "exists"})));
    }

    bans.push(BanEntry {
        target: payload.target,
        by: "00000000-0000-0000-0000-000000000000".to_string(), // Server/Console UUID placeholder
        reason: payload.reason,
        timestamp: chrono::Utc::now().timestamp_millis(),
        ban_type: "infinite".to_string(),
        username: None, // We don't store username in this format apparently? Or maybe we can?
        banned_by: None
    });

    fs::write(&path, serde_json::to_string_pretty(&bans).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}


// --- OPS ---
// Schema: { "users": { "uuid": { "groups": ["admin"] } } }

pub async fn get_ops(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "permissions.json");

    if !path.exists() { return Ok(Json(Vec::<OpEntry>::new())); }

    let content = fs::read_to_string(&path).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
    
    let mut list = Vec::new();
    if let Some(users) = json.get("users").and_then(|u| u.as_object()) {
        for (uuid, val) in users {
            let groups = val.get("groups")
                .and_then(|g| g.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            
            list.push(OpEntry {
                uuid: uuid.clone(),
                groups
            });
        }
    }

    Ok(Json(list))
}

pub async fn add_op(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<AddOpRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "permissions.json");

    let content = if path.exists() {
        fs::read_to_string(&path).await.unwrap_or_else(|_| "{}".to_string())
    } else {
        "{}".to_string()
    };
    
    let mut json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
    
    // Structure: { "users": { "<uuid>": { "groups": ["admin"] } } }
    if json.get("users").is_none() {
        json["users"] = serde_json::json!({});
    }
    
    let users = json.get_mut("users").unwrap().as_object_mut().unwrap();
    
    let group = payload.group.unwrap_or_else(|| "admin".to_string());
    
    users.insert(payload.uuid.clone(), serde_json::json!({
        "groups": [group]
    }));
    
    fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn remove_op(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<AddOpRequest>, 
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "permissions.json");

    if !path.exists() { return Ok(Json(serde_json::json!({"status": "ok"}))); }

    let content = fs::read_to_string(&path).await.unwrap_or_else(|_| "{}".to_string());
    let mut json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
    
    if let Some(users) = json.get_mut("users").and_then(|u| u.as_object_mut()) {
        users.remove(&payload.uuid);
        fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn remove_ban(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<AddBanRequest>, 
) -> Result<impl IntoResponse, ApiError> {
    let server = get_server_by_id_internal(&state.pool, &id).await?;
    let path = get_player_file_path(&server.working_dir, "bans.json");

    if !path.exists() { return Ok(Json(serde_json::json!({"status": "ok"}))); }
    
    let content = fs::read_to_string(&path).await.unwrap_or_default();
    let mut bans: Vec<BanEntry> = serde_json::from_str(&content).unwrap_or_default();
    
    let initial_len = bans.len();
    bans.retain(|b| b.target != payload.target);
    
    if bans.len() != initial_len {
        fs::write(&path, serde_json::to_string_pretty(&bans).unwrap()).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    }
    
    Ok(Json(serde_json::json!({"status": "ok"})))
}

