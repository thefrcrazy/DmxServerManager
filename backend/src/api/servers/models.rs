use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
    pub cpu_usage_normalized: f32, // New field
    pub memory_usage_bytes: u64,
    pub max_memory_bytes: u64,
    pub max_heap_bytes: u64,
    pub disk_usage_bytes: u64,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(FromRow)]
pub struct PlayerRow {
    pub player_name: String,
    pub is_online: i32,
    pub last_seen: String,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, FromRow)]
pub struct ServerRow {
    pub id: String,
    pub name: String,
    pub game_type: String,
    pub executable_path: String,
    pub working_dir: String,
    pub java_path: Option<String>,
    pub min_memory: Option<String>,
    pub max_memory: Option<String>,
    pub extra_args: Option<String>,
    pub config: Option<String>,
    pub auto_start: i32,
    pub created_at: String,
    pub updated_at: String,
    
    // New fields
    #[sqlx(default)]
    pub backup_enabled: i32,
    #[sqlx(default)]
    pub backup_frequency: i32,
    #[sqlx(default)]
    pub backup_max_backups: i32,
    #[sqlx(default)]
    pub backup_prefix: String,
    #[sqlx(default)]
    pub discord_username: Option<String>,
    #[sqlx(default)]
    pub discord_avatar: Option<String>,
    #[sqlx(default)]
    pub discord_webhook_url: Option<String>,
    #[sqlx(default)]
    pub discord_notifications: Option<String>,
    #[sqlx(default)]
    pub logs_retention_days: i32,
    #[sqlx(default)]
    pub watchdog_enabled: i32,
    #[sqlx(default)]
    pub auth_mode: String,
    #[sqlx(default)]
    pub bind_address: String,
    #[sqlx(default)]
    pub port: i32,
}

// ============= Server Files API Models =============

#[derive(Debug, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct FilesQuery {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReadFileQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteFileRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileRequest {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameFileRequest {
    pub path: String,
    pub new_name: String,
}

#[derive(Debug, Deserialize)]
pub struct CopyFileRequest {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Deserialize)]
pub struct MoveFileRequest {
    pub source: String,
    pub destination: String,
}
