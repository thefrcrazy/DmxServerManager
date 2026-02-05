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

/// Recursively merge two JSON values.

/// `a` is the base value (will be modified), `b` is the new value to merge into `a`.

pub fn deep_merge(a: &mut Value, b: &Value) {

    match (a, b) {

        (Value::Object(a), Value::Object(b)) => {

            for (k, v) in b {

                deep_merge(a.entry(k.clone()).or_insert(Value::Null), v);

            }

        }

        (a, b) => *a = b.clone(),

    }

}



/// Map flat frontend config keys to structured Hytale config.json keys

pub fn map_to_hytale_config(flat_config: &Value) -> Value {

    let mut hytale_config = json!({});

    

    if let Some(obj) = flat_config.as_object() {

        for (k, v) in obj {

            match k.as_str() {

                "name" | "ServerName" => { hytale_config["ServerName"] = v.clone(); }

                "motd" | "MOTD" => { hytale_config["MOTD"] = v.clone(); }

                "max_players" | "MaxPlayers" => { hytale_config["MaxPlayers"] = v.clone(); }

                "port" | "Port" => { hytale_config["Port"] = v.clone(); }

                "password" | "Password" => { hytale_config["Password"] = v.clone(); }

                "view_distance" | "MaxViewRadius" => { hytale_config["MaxViewRadius"] = v.clone(); }

                "world_name" => {

                    if hytale_config.get("Defaults").is_none() { hytale_config["Defaults"] = json!({}); }

                    hytale_config["Defaults"]["World"] = v.clone();

                }

                "game_mode" => {

                    if hytale_config.get("Defaults").is_none() { hytale_config["Defaults"] = json!({}); }

                    hytale_config["Defaults"]["GameMode"] = v.clone();

                }

                "auth_mode" => {

                    let auth_store = if v.as_str() == Some("authenticated") {

                        json!({ "Type": "Encrypted", "Path": "auth.enc" })

                    } else {

                        json!({ "Type": "None" })

                    };

                    hytale_config["AuthCredentialStore"] = auth_store;

                }

                // Handle already structured keys (PascalCase) by preserving them

                _ if k.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) => {

                    hytale_config[k] = v.clone();

                }

                _ => {} // Ignore other flat keys like bind_address, etc.

            }

        }

    }

    

    hytale_config

}
