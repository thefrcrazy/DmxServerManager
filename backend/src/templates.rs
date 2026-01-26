//! Templates for Hytale server configuration files

use serde_json::{json, Value};

/// Generate the Hytale server config.json
pub fn generate_config_json(
    server_name: &str,
    max_players: u32,
    auth_mode: &str,
) -> Value {
    let auth_store = if auth_mode == "authenticated" {
        json!({
            "Type": "Encrypted",
            "Path": "auth.enc"
        })
    } else {
        json!({
            "Type": "None"
        })
    };

    json!({
        "Version": 3,
        "ServerName": server_name,
        "MOTD": "",
        "Password": "",
        "MaxPlayers": max_players,
        "MaxViewRadius": 12,
        "Defaults": {
            "World": "default",
            "GameMode": "Adventure"
        },
        "ConnectionTimeouts": {
            "JoinTimeouts": {}
        },
        "RateLimit": {},
        "Modules": {
            "PathPlugin": {
                "Modules": {}
            }
        },
        "LogLevels": {},
        "Mods": {},
        "DisplayTmpTagsInStrings": false,
        "PlayerStorage": {
            "Type": "Hytale"
        },
        "AuthCredentialStore": auth_store
    })
}

/// Generate manager.json (unified manager configuration)
pub fn generate_manager_json(
    server_id: &str,
    server_name: &str,
    install_dir: &str,
    bind_address: &str,
    port: u16,
    auth_mode: &str,
    java_path: Option<&str>,
    min_memory: Option<&str>,
    max_memory: Option<&str>,
) -> Value {
    json!({
        "server": {
            "id": server_id,
            "name": server_name,
            "installDir": install_dir,
            "bindAddress": bind_address,
            "port": port,
            "authMode": auth_mode,
            "patchline": "release"
        },
        "java": {
            "path": java_path.unwrap_or("java"),
            "minVersion": 25,
            "minMemory": min_memory.unwrap_or("2G"),
            "maxMemory": max_memory.unwrap_or("4G"),
            "useAotCache": true,
            "extraOpts": "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200"
        },
        "backup": {
            "enabled": true,
            "frequency": 30,
            "maxBackups": 7,
            "prefix": "hytale_backup"
        },
        "logs": {
            "retentionDays": 7
        },
        "discord": {
            "webhooks": [],
            "username": "Hytale Bot",
            "avatar": "",
            "notifications": {
                "start": true,
                "stop": true,
                "playerJoin": true,
                "playerLeave": true
            }
        },
        "watchdog": {
            "enabled": true
        },
        "disk": {
            "minSpaceGb": 5
        }
    })
}

