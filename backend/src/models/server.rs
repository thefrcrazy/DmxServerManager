#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub game_type: GameType,
    pub executable_path: String,
    pub working_dir: String,
    pub java_path: Option<String>,
    pub min_memory: Option<String>,
    pub max_memory: Option<String>,
    pub extra_args: Option<String>,
    pub auto_start: bool,
    pub created_at: String,
    pub updated_at: String,

    // Config settings (formerly manager.json)
    pub backup_enabled: bool,
    pub backup_frequency: u32,
    pub backup_max_backups: u32,
    pub backup_prefix: String,

    pub discord_username: Option<String>,
    pub discord_avatar: Option<String>,
    pub discord_webhook_url: Option<String>,
    pub discord_notifications: Option<String>, // JSON string

    pub logs_retention_days: u32,
    pub watchdog_enabled: bool,

    pub auth_mode: String,
    pub bind_address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GameType {
    Hytale,
}

impl std::fmt::Display for GameType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameType::Hytale => write!(f, "hytale"),
        }
    }
}

impl std::str::FromStr for GameType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hytale" => Ok(GameType::Hytale),
            _ => Err(format!("Unknown game type: {s}")),
        }
    }
}
