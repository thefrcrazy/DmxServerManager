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