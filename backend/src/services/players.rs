use std::{collections::HashMap, sync::LazyLock};

use regex::Regex;
use serde::Serialize;
use sqlx::FromRow;
use tokio::sync::mpsc;

use crate::core::{DbPool, error::AppError, events::EventHub};

const PLAYER_OBSERVER_QUEUE: usize = 512;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ServerPlayer {
    pub player_key: String,
    pub display_name: String,
    pub external_id: Option<String>,
    pub source: String,
    pub online: bool,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub connected_at: Option<String>,
    pub disconnected_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerSnapshot {
    pub instance_id: String,
    pub online_count: usize,
    pub detection: &'static str,
    pub access_mode: &'static str,
    pub players: Vec<ServerPlayer>,
}

#[derive(Debug)]
enum Observation {
    Identified {
        name: String,
        external_id: String,
    },
    Connected {
        name: String,
        external_id: Option<String>,
        source: &'static str,
    },
    Disconnected {
        name: String,
        external_id: Option<String>,
    },
}

#[derive(Debug)]
struct PlayerDetector {
    profile_id: String,
    identities: HashMap<String, String>,
}

impl PlayerDetector {
    fn new(profile_id: String) -> Self {
        Self {
            profile_id,
            identities: HashMap::new(),
        }
    }

    fn observe(&mut self, line: &str) -> Vec<Observation> {
        let line = strip_ansi(line);
        let mut observations = match self.profile_id.as_str() {
            "hytale" => hytale_observations(&line),
            "minecraft-bedrock" => bedrock_observations(&line),
            id if id == "minecraft-java" || id.starts_with("minecraft-java-") => {
                minecraft_java_observations(&line)
            }
            "seven-days-to-die" => seven_days_observations(&line),
            "project-zomboid" => project_zomboid_observations(&line),
            "rust" | "valheim" | "palworld" => steam_observations(&line),
            _ => Vec::new(),
        };
        for observation in &observations {
            if let Observation::Identified { name, external_id } = observation {
                self.identities
                    .insert(normalize_name(name), external_id.clone());
            }
        }
        for observation in &mut observations {
            match observation {
                Observation::Connected {
                    name, external_id, ..
                }
                | Observation::Disconnected { name, external_id } => {
                    if external_id.is_none() {
                        *external_id = self.identities.get(&normalize_name(name)).cloned();
                    }
                }
                Observation::Identified { .. } => {}
            }
        }
        observations
    }
}

pub fn spawn_log_observer(
    pool: DbPool,
    events: EventHub,
    instance_id: String,
    profile_id: String,
) -> mpsc::Sender<String> {
    let (sender, mut receiver) = mpsc::channel::<String>(PLAYER_OBSERVER_QUEUE);
    tokio::spawn(async move {
        if let Err(error) = mark_all_offline(&pool, &events, &instance_id).await {
            tracing::warn!(instance_id, %error, "failed to reset player presence before start");
        }
        let mut detector = PlayerDetector::new(profile_id);
        while let Some(line) = receiver.recv().await {
            for observation in detector.observe(&line) {
                if let Err(error) =
                    apply_observation(&pool, &events, &instance_id, observation).await
                {
                    tracing::warn!(instance_id, %error, "failed to persist player observation");
                }
            }
        }
        if let Err(error) = mark_all_offline(&pool, &events, &instance_id).await {
            tracing::warn!(instance_id, %error, "failed to close player sessions after process exit");
        }
    });
    sender
}

pub async fn snapshot(
    pool: &DbPool,
    instance_id: &str,
    profile_id: &str,
) -> Result<PlayerSnapshot, AppError> {
    let players: Vec<ServerPlayer> = sqlx::query_as(
        r#"
        SELECT player_key, display_name, external_id, source, online,
               first_seen_at, last_seen_at, connected_at, disconnected_at
        FROM server_players
        WHERE instance_id = ?
        ORDER BY online DESC, last_seen_at DESC, display_name COLLATE NOCASE
        LIMIT 1000
        "#,
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await?;
    let online_count = players.iter().filter(|player| player.online).count();
    Ok(PlayerSnapshot {
        instance_id: instance_id.to_string(),
        online_count,
        detection: detection_mode(profile_id),
        access_mode: access_mode(profile_id),
        players,
    })
}

pub fn detection_mode(profile_id: &str) -> &'static str {
    match profile_id {
        "hytale" | "minecraft-bedrock" | "seven-days-to-die" | "project-zomboid" | "rust"
        | "valheim" | "palworld" => "console_log",
        id if id == "minecraft-java" || id.starts_with("minecraft-java-") => "console_log",
        _ => "unavailable",
    }
}

pub fn access_mode(profile_id: &str) -> &'static str {
    match profile_id {
        "hytale" | "minecraft-bedrock" | "valheim" | "rust" | "seven-days-to-die" => "native_files",
        id if id == "minecraft-java" || id.starts_with("minecraft-java-") => "native_files",
        "project-zomboid" => "console_commands",
        "palworld" => "shared_admin_password",
        "satisfactory" => "game_managed",
        _ => "unsupported",
    }
}

async fn apply_observation(
    pool: &DbPool,
    events: &EventHub,
    instance_id: &str,
    observation: Observation,
) -> Result<(), AppError> {
    let (name, external_id, source, online) = match observation {
        Observation::Identified { .. } => return Ok(()),
        Observation::Connected {
            name,
            external_id,
            source,
        } => (name, external_id, source, true),
        Observation::Disconnected { name, external_id } => {
            (name, external_id, "generic_log", false)
        }
    };
    let Some(name) = sanitize_name(&name) else {
        return Ok(());
    };
    let external_id = external_id.and_then(|id| sanitize_external_id(&id));
    let key = player_key(&name, external_id.as_deref());
    let now = chrono::Utc::now().to_rfc3339();
    if online {
        sqlx::query(
            r#"
            INSERT INTO server_players
                (instance_id, player_key, display_name, external_id, source, online,
                 first_seen_at, last_seen_at, connected_at, disconnected_at)
            VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, NULL)
            ON CONFLICT(instance_id, player_key) DO UPDATE SET
                display_name = excluded.display_name,
                external_id = COALESCE(excluded.external_id, server_players.external_id),
                source = excluded.source,
                online = 1,
                last_seen_at = excluded.last_seen_at,
                connected_at = excluded.connected_at,
                disconnected_at = NULL
            "#,
        )
        .bind(instance_id)
        .bind(&key)
        .bind(&name)
        .bind(external_id.as_deref())
        .bind(source)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    } else {
        let changed = sqlx::query(
            "UPDATE server_players SET online = 0, last_seen_at = ?, disconnected_at = ? \
             WHERE instance_id = ? AND player_key = ? AND online = 1",
        )
        .bind(&now)
        .bind(&now)
        .bind(instance_id)
        .bind(&key)
        .execute(pool)
        .await?
        .rows_affected();
        if changed == 0 {
            sqlx::query(
                "UPDATE server_players SET online = 0, last_seen_at = ?, disconnected_at = ? \
                 WHERE instance_id = ? AND lower(display_name) = lower(?) AND online = 1",
            )
            .bind(&now)
            .bind(&now)
            .bind(instance_id)
            .bind(&name)
            .execute(pool)
            .await?;
        }
    }
    publish_count(pool, events, instance_id).await
}

pub async fn mark_all_offline(
    pool: &DbPool,
    events: &EventHub,
    instance_id: &str,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let changed = sqlx::query(
        "UPDATE server_players SET online = 0, last_seen_at = ?, disconnected_at = ? \
         WHERE instance_id = ? AND online = 1",
    )
    .bind(&now)
    .bind(&now)
    .bind(instance_id)
    .execute(pool)
    .await?
    .rows_affected();
    if changed > 0 {
        publish_count(pool, events, instance_id).await?;
    }
    Ok(())
}

async fn publish_count(
    pool: &DbPool,
    events: &EventHub,
    instance_id: &str,
) -> Result<(), AppError> {
    let online_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM server_players WHERE instance_id = ? AND online = 1",
    )
    .bind(instance_id)
    .fetch_one(pool)
    .await?;
    events.publish(
        "server.players",
        Some(instance_id.to_string()),
        serde_json::json!({"online_count": online_count}),
    );
    Ok(())
}

fn hytale_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = HYTALE_CONNECTED.captures(line) {
        return vec![connected(&captures, "hytale")];
    }
    if let Some(captures) = HYTALE_DISCONNECTED.captures(line) {
        return vec![disconnected(&captures)];
    }
    Vec::new()
}

fn minecraft_java_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = MINECRAFT_UUID.captures(line) {
        return vec![Observation::Identified {
            name: capture(&captures, "name"),
            external_id: capture(&captures, "id"),
        }];
    }
    if let Some(captures) = MINECRAFT_JOINED.captures(line) {
        return vec![Observation::Connected {
            name: capture(&captures, "name"),
            external_id: None,
            source: "minecraft_java",
        }];
    }
    if let Some(captures) = MINECRAFT_LEFT.captures(line) {
        return vec![Observation::Disconnected {
            name: capture(&captures, "name"),
            external_id: None,
        }];
    }
    Vec::new()
}

fn bedrock_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = BEDROCK_CONNECTED.captures(line) {
        return vec![connected(&captures, "minecraft_bedrock")];
    }
    if let Some(captures) = BEDROCK_DISCONNECTED.captures(line) {
        return vec![disconnected(&captures)];
    }
    Vec::new()
}

fn seven_days_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = SEVEN_DAYS_CONNECTED.captures(line) {
        return vec![connected(&captures, "console_log")];
    }
    if let Some(captures) = SEVEN_DAYS_DISCONNECTED.captures(line) {
        return vec![disconnected(&captures)];
    }
    Vec::new()
}

fn project_zomboid_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = PROJECT_ZOMBOID_CONNECTED.captures(line) {
        return vec![Observation::Connected {
            name: capture(&captures, "name"),
            external_id: captures.name("id").map(|value| value.as_str().to_string()),
            source: "console_log",
        }];
    }
    if let Some(captures) = PROJECT_ZOMBOID_DISCONNECTED.captures(line) {
        return vec![Observation::Disconnected {
            name: capture(&captures, "name"),
            external_id: None,
        }];
    }
    Vec::new()
}

fn steam_observations(line: &str) -> Vec<Observation> {
    if let Some(captures) = STEAM_CONNECTED.captures(line) {
        return vec![connected(&captures, "steam")];
    }
    if let Some(captures) = STEAM_DISCONNECTED.captures(line) {
        return vec![disconnected(&captures)];
    }
    Vec::new()
}

fn connected(captures: &regex::Captures<'_>, source: &'static str) -> Observation {
    Observation::Connected {
        name: capture(captures, "name"),
        external_id: captures.name("id").map(|value| value.as_str().to_string()),
        source,
    }
}

fn disconnected(captures: &regex::Captures<'_>) -> Observation {
    Observation::Disconnected {
        name: capture(captures, "name"),
        external_id: captures.name("id").map(|value| value.as_str().to_string()),
    }
}

fn capture(captures: &regex::Captures<'_>, name: &str) -> String {
    captures
        .name(name)
        .map_or_else(String::new, |value| value.as_str().trim().to_string())
}

fn player_key(name: &str, external_id: Option<&str>) -> String {
    external_id.map_or_else(
        || format!("name:{}", normalize_name(name)),
        |id| format!("id:{}", id.to_ascii_lowercase()),
    )
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn sanitize_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control))
        .then(|| name.to_string())
}

fn sanitize_external_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty()
        && id.len() <= 255
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'@')))
    .then(|| id.to_string())
}

fn strip_ansi(line: &str) -> String {
    ANSI_ESCAPE.replace_all(line, "").into_owned()
}

static ANSI_ESCAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ANSI regex is valid"));
static HYTALE_CONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Connection complete for (?P<name>[^()\r\n]{1,128}) \((?P<id>[0-9a-fA-F-]{36})\)")
        .expect("Hytale player regex is valid")
});
static HYTALE_DISCONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?P<name>[^()\r\n]{1,128}) \((?P<id>[0-9a-f-]{36})\).*(?:disconnect|left|closed)",
    )
    .expect("Hytale disconnect regex is valid")
});
static MINECRAFT_UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"UUID of player (?P<name>[A-Za-z0-9_]{1,16}) is (?P<id>[0-9a-fA-F-]{32,36})")
        .expect("Minecraft UUID regex is valid")
});
static MINECRAFT_JOINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?P<name>[A-Za-z0-9_]{1,16}) (?:joined the game|logged in with entity id)")
        .expect("Minecraft join regex is valid")
});
static MINECRAFT_LEFT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?P<name>[A-Za-z0-9_]{1,16}) (?:left the game|lost connection)")
        .expect("Minecraft leave regex is valid")
});
static BEDROCK_CONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Player connected: (?P<name>[^,\r\n]{1,128}),\s*xuid:\s*(?P<id>[0-9]+)")
        .expect("Bedrock join regex is valid")
});
static BEDROCK_DISCONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Player disconnected: (?P<name>[^,\r\n]{1,128}),\s*xuid:\s*(?P<id>[0-9]+)")
        .expect("Bedrock leave regex is valid")
});
static SEVEN_DAYS_CONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"PlayerSpawnedInWorld.*PlayerName='(?P<name>[^']{1,128})'.*(?:CrossplatformId|OwnerID)='(?P<id>[^']{1,255})'",
    )
    .expect("7DTD join regex is valid")
});
static SEVEN_DAYS_DISCONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)Player disconnected:.*(?:OwnerID|PlayerID)='(?P<id>[^']{1,255})'.*PlayerName='(?P<name>[^']{1,128})'",
    )
    .expect("7DTD leave regex is valid")
});
static PROJECT_ZOMBOID_CONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:(?P<id>7656119[0-9]{10}).*)?[\"'](?P<name>[^\"']{1,128})[\"'].*fully connected"#,
    )
    .expect("Project Zomboid join regex is valid")
});
static PROJECT_ZOMBOID_DISCONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:disconnect|connection closed).*[\"'](?P<name>[^\"']{1,128})[\"']"#)
        .expect("Project Zomboid leave regex is valid")
});
static STEAM_CONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?P<id>7656119[0-9]{10})[/ :]\s*(?P<name>[A-Za-z0-9_. -]{1,64}).*(?:joined|connected|authenticated)",
    )
    .expect("Steam join regex is valid")
});
static STEAM_DISCONNECTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?P<id>7656119[0-9]{10})[/ :]\s*(?P<name>[A-Za-z0-9_. -]{1,64}).*(?:left|disconnect|closed)",
    )
    .expect("Steam leave regex is valid")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_hytale_connection_log() {
        let mut detector = PlayerDetector::new("hytale".into());
        let observations = detector.observe(
            "[PasswordPacketHandler] Connection complete for TheFRcRaZy (ebc152b4-fdfd-4467-aadc-865ad4c87ea4) (SNI: game.example.com)",
        );
        assert!(matches!(
            observations.as_slice(),
            [Observation::Connected { name, external_id: Some(id), source: "hytale" }]
                if name == "TheFRcRaZy" && id == "ebc152b4-fdfd-4467-aadc-865ad4c87ea4"
        ));
    }

    #[test]
    fn minecraft_uuid_is_reused_for_join_and_leave() {
        let mut detector = PlayerDetector::new("minecraft-java".into());
        detector.observe("UUID of player Alex is 12345678-1234-1234-1234-123456789abc");
        let joined = detector.observe("Alex joined the game");
        assert!(matches!(
            joined.as_slice(),
            [Observation::Connected { external_id: Some(id), .. }]
                if id == "12345678-1234-1234-1234-123456789abc"
        ));
        let left = detector.observe("Alex left the game");
        assert!(matches!(
            left.as_slice(),
            [Observation::Disconnected { external_id: Some(id), .. }]
                if id == "12345678-1234-1234-1234-123456789abc"
        ));
    }

    #[test]
    fn bedrock_xuid_is_detected() {
        let mut detector = PlayerDetector::new("minecraft-bedrock".into());
        let observations = detector.observe("Player connected: Steve, xuid: 2533274790000000");
        assert!(matches!(
            observations.as_slice(),
            [Observation::Connected { name, external_id: Some(id), .. }]
                if name == "Steve" && id == "2533274790000000"
        ));
    }
}
