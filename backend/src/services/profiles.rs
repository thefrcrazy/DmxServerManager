use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, RwLock},
};

use regex::Regex;
use serde_json::{Value, json};

use crate::{
    core::{DbPool, error::AppError},
    domain::v1::{
        GameProfile, LifecycleSpec, PortProtocol, PortSpec, ProfileKind, SteamProfile,
        SteamStopStrategy, StopStrategy, SupportedPlatform, safe_join,
    },
};

#[derive(Debug, Clone)]
pub struct ProfileRegistry {
    profiles: Arc<RwLock<BTreeMap<(String, u32), GameProfile>>>,
}

impl ProfileRegistry {
    pub fn builtins() -> Self {
        let profiles = [
            hytale(),
            minecraft_java_unified(),
            minecraft_java("minecraft-java-vanilla", "Minecraft Java — Vanilla"),
            minecraft_java("minecraft-java-paper", "Minecraft Java — Paper"),
            minecraft_java("minecraft-java-fabric", "Minecraft Java — Fabric"),
            minecraft_java("minecraft-java-forge", "Minecraft Java — Forge"),
            minecraft_java("minecraft-java-neoforge", "Minecraft Java — NeoForge"),
            minecraft_java("minecraft-java-spigot", "Minecraft Java — Spigot"),
            minecraft_java("minecraft-java-purpur", "Minecraft Java — Purpur"),
            minecraft_java("minecraft-java-quilt", "Minecraft Java — Quilt"),
            minecraft_bedrock(),
            valheim(),
            palworld(),
            satisfactory(),
            seven_days_to_die(),
            project_zomboid(),
            rust_server(),
        ]
        .into_iter()
        .map(|profile| ((profile.id.clone(), profile.revision), profile))
        .collect();

        Self {
            profiles: Arc::new(RwLock::new(profiles)),
        }
    }

    pub fn all(&self) -> Vec<GameProfile> {
        let profiles = self.profiles.read().expect("profile registry poisoned");
        let mut latest = BTreeMap::<String, GameProfile>::new();
        for profile in profiles.values() {
            latest.insert(profile.id.clone(), profile.clone());
        }
        latest.into_values().collect()
    }

    pub fn get(&self, id: &str) -> Option<GameProfile> {
        self.profiles
            .read()
            .expect("profile registry poisoned")
            .values()
            .filter(|profile| profile.id == id)
            .max_by_key(|profile| profile.revision)
            .cloned()
    }

    pub fn get_revision(&self, id: &str, revision: u32) -> Option<GameProfile> {
        self.profiles
            .read()
            .expect("profile registry poisoned")
            .get(&(id.to_string(), revision))
            .cloned()
    }

    pub fn revisions(&self, id: &str) -> Vec<GameProfile> {
        self.profiles
            .read()
            .expect("profile registry poisoned")
            .values()
            .filter(|profile| profile.id == id)
            .cloned()
            .collect()
    }

    pub fn register(&self, profile: GameProfile) {
        self.profiles
            .write()
            .expect("profile registry poisoned")
            .insert((profile.id.clone(), profile.revision), profile);
    }

    pub fn unregister_custom(&self, id: &str) {
        self.profiles
            .write()
            .expect("profile registry poisoned")
            .retain(|(profile_id, _), profile| {
                profile_id != id || profile.kind != ProfileKind::SteamCustom
            });
    }

    pub fn unregister_custom_revision(&self, id: &str, revision: u32) {
        self.profiles
            .write()
            .expect("profile registry poisoned")
            .retain(|(profile_id, profile_revision), profile| {
                profile_id != id
                    || *profile_revision != revision
                    || profile.kind != ProfileKind::SteamCustom
            });
    }

    pub fn validate_settings(&self, profile_id: &str, settings: &Value) -> Result<(), AppError> {
        let profile = self
            .get(profile_id)
            .ok_or_else(|| AppError::BadRequest("profiles.unknown".into()))?;
        validate_profile_settings(&profile, settings)
    }

    pub fn validate_settings_revision(
        &self,
        profile_id: &str,
        revision: u32,
        settings: &Value,
    ) -> Result<(), AppError> {
        let profile = self
            .get_revision(profile_id, revision)
            .ok_or_else(|| AppError::BadRequest("profiles.unknown_revision".into()))?;
        validate_profile_settings(&profile, settings)
    }

    pub async fn load_persisted(&self, pool: &DbPool) -> Result<(), AppError> {
        let compiled_builtin_revisions = {
            let profiles = self.profiles.read().expect("profile registry poisoned");
            profiles
                .values()
                .filter(|profile| profile.kind == ProfileKind::Builtin)
                .map(|profile| (profile.id.clone(), profile.revision))
                .collect::<BTreeMap<_, _>>()
        };
        let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT id, revision, kind, manifest FROM game_profiles ORDER BY id, revision",
        )
        .fetch_all(pool)
        .await?;
        for (id, revision, kind, manifest) in rows {
            let profile: GameProfile = serde_json::from_str(&manifest)
                .map_err(|_| AppError::Internal("stored game profile is invalid".into()))?;
            let revision = u32::try_from(revision)
                .map_err(|_| AppError::Internal("stored profile revision is invalid".into()))?;
            let expected_kind = match kind.as_str() {
                "builtin" => ProfileKind::Builtin,
                "steam_custom" => ProfileKind::SteamCustom,
                _ => {
                    return Err(AppError::Internal("stored profile kind is invalid".into()));
                }
            };
            if profile.id != id || profile.revision != revision || profile.kind != expected_kind {
                return Err(AppError::Internal(
                    "stored game profile manifest does not match its revision".into(),
                ));
            }
            match profile.kind {
                ProfileKind::SteamCustom => validate_local_steam_profile(&profile)?,
                ProfileKind::Builtin => {
                    let Some(compiled_revision) = compiled_builtin_revisions.get(&profile.id)
                    else {
                        return Err(AppError::Internal(
                            "stored builtin profile is not supported by this release".into(),
                        ));
                    };
                    if profile.steam_profile.is_some() || profile.revision > *compiled_revision {
                        return Err(AppError::Internal(
                            "stored builtin profile is newer than this release".into(),
                        ));
                    }
                    if let Some(compiled) = self.get_revision(&profile.id, profile.revision)
                        && compiled != profile
                    {
                        return Err(AppError::Internal(
                            "builtin profile revision is immutable".into(),
                        ));
                    }
                }
            }
            self.register(profile);
        }
        Ok(())
    }

    pub async fn persist_builtins(&self, pool: &DbPool) -> Result<(), AppError> {
        let builtins = {
            let profiles = self.profiles.read().expect("profile registry poisoned");
            profiles
                .values()
                .filter(|profile| profile.kind == ProfileKind::Builtin)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut transaction = pool.begin().await?;
        for profile in builtins {
            let manifest = serde_json::to_string(&profile)
                .map_err(|error| AppError::Internal(error.to_string()))?;
            sqlx::query(
                r#"
                INSERT INTO game_profiles (id, revision, kind, manifest, created_at)
                VALUES (?, ?, 'builtin', ?, ?)
                ON CONFLICT(id, revision) DO NOTHING
                "#,
            )
            .bind(&profile.id)
            .bind(profile.revision)
            .bind(&manifest)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
            let persisted: (String, String) = sqlx::query_as(
                "SELECT kind, manifest FROM game_profiles WHERE id = ? AND revision = ?",
            )
            .bind(&profile.id)
            .bind(profile.revision)
            .fetch_one(&mut *transaction)
            .await?;
            let persisted_profile: GameProfile = serde_json::from_str(&persisted.1)
                .map_err(|_| AppError::Internal("stored game profile is invalid".into()))?;
            if persisted.0 != "builtin" || persisted_profile != profile {
                return Err(AppError::Internal(
                    "builtin profile revision is immutable".into(),
                ));
            }
        }
        transaction.commit().await?;
        Ok(())
    }
}

pub fn build_local_steam_profile(
    id: String,
    revision: u32,
    name: String,
    description: String,
    steam_profile: SteamProfile,
) -> Result<GameProfile, AppError> {
    let platforms = supported_steam_platforms(&steam_profile);
    let lifecycle = LifecycleSpec {
        stop: match &steam_profile.stop_strategy {
            SteamStopStrategy::Stdin {
                command,
                timeout_seconds,
            } => StopStrategy::Stdin {
                command: command.clone(),
                timeout_seconds: *timeout_seconds,
            },
            SteamStopStrategy::Interrupt { timeout_seconds } => StopStrategy::Interrupt {
                timeout_seconds: *timeout_seconds,
            },
            SteamStopStrategy::Terminate { timeout_seconds } => StopStrategy::Terminate {
                timeout_seconds: *timeout_seconds,
            },
        },
        ready_log_pattern: steam_profile.ready_log_pattern.clone(),
    };
    let properties = steam_profile
        .ports
        .iter()
        .map(|port| {
            (
                port.name.clone(),
                json!({
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "default": port.default,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let profile = GameProfile {
        id,
        revision,
        name,
        description,
        kind: ProfileKind::SteamCustom,
        platforms,
        capabilities: [
            "settings",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        ports: steam_profile.ports.clone(),
        lifecycle,
        settings_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": [],
            "properties": properties,
        }),
        ui_schema: json!({"layout": "sections"}),
        steam_profile: Some(steam_profile),
    };
    validate_local_steam_profile(&profile)?;
    Ok(profile)
}

pub async fn load_steam_profile_revision(
    pool: &DbPool,
    id: &str,
    revision: u32,
) -> Result<Option<SteamProfile>, AppError> {
    let manifest: Option<String> = sqlx::query_scalar(
        "SELECT manifest FROM game_profiles WHERE id = ? AND revision = ? \
         AND kind = 'steam_custom'",
    )
    .bind(id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    let Some(manifest) = manifest else {
        return Ok(None);
    };
    let profile: GameProfile = serde_json::from_str(&manifest)
        .map_err(|_| AppError::Internal("stored Steam profile is invalid".into()))?;
    validate_local_steam_profile(&profile)?;
    if profile.id != id || profile.revision != revision {
        return Err(AppError::Internal(
            "stored Steam profile manifest does not match its revision".into(),
        ));
    }
    Ok(profile.steam_profile)
}

pub fn validate_local_steam_profile(profile: &GameProfile) -> Result<(), AppError> {
    if profile.kind != ProfileKind::SteamCustom
        || profile.revision == 0
        || profile.id.len() < 7
        || profile.id.len() > 64
        || !profile.id.starts_with("steam-")
        || profile.id == "steam-custom"
        || profile.id.ends_with('-')
        || profile.id.as_bytes()[6..]
            .iter()
            .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && *byte != b'-')
        || profile.id.contains("--")
    {
        return Err(AppError::BadRequest("profiles.steam.invalid_id".into()));
    }
    validate_profile_label(&profile.name, 80, "profiles.steam.invalid_name")?;
    validate_profile_label(
        &profile.description,
        500,
        "profiles.steam.invalid_description",
    )?;
    let steam = profile
        .steam_profile
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("profiles.steam.definition_required".into()))?;
    if steam.app_id == 0 {
        return Err(AppError::BadRequest("profiles.steam.invalid_app_id".into()));
    }
    if let Some(branch) = &steam.branch
        && !Regex::new(r"^[A-Za-z0-9._-]{1,64}$")
            .expect("constant branch regex")
            .is_match(branch)
    {
        return Err(AppError::BadRequest("profiles.steam.invalid_branch".into()));
    }
    validate_steam_executable(steam.executable.linux_x86_64.as_deref(), false)?;
    validate_steam_executable(steam.executable.windows_x86_64.as_deref(), true)?;
    if steam.executable.linux_x86_64.is_none() && steam.executable.windows_x86_64.is_none() {
        return Err(AppError::BadRequest(
            "profiles.steam.executable_required".into(),
        ));
    }
    if supported_steam_platforms(steam) != profile.platforms {
        return Err(AppError::BadRequest(
            "profiles.steam.platform_manifest_invalid".into(),
        ));
    }
    if steam.arguments.len() > 128 {
        return Err(AppError::BadRequest(
            "profiles.steam.invalid_arguments".into(),
        ));
    }
    let port_names = steam
        .ports
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();
    for argument in &steam.arguments {
        if argument.len() > 8_192
            || argument.contains('\0')
            || argument
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\t'))
        {
            return Err(AppError::BadRequest(
                "profiles.steam.invalid_arguments".into(),
            ));
        }
        if argument == "{{instance_dir}}" {
            continue;
        }
        if let Some(name) = argument
            .strip_prefix("{{port:")
            .and_then(|value| value.strip_suffix("}}"))
        {
            if port_names.contains(name) {
                continue;
            }
            return Err(AppError::BadRequest(
                "profiles.steam.invalid_placeholder".into(),
            ));
        }
        if argument.contains("{{") || argument.contains("}}") {
            return Err(AppError::BadRequest(
                "profiles.steam.invalid_placeholder".into(),
            ));
        }
    }
    validate_steam_ports(&steam.ports)?;
    validate_steam_save_paths(&steam.save_paths)?;
    if let Some(pattern) = &steam.ready_log_pattern
        && (pattern.is_empty() || pattern.len() > 256 || Regex::new(pattern).is_err())
    {
        return Err(AppError::BadRequest(
            "profiles.steam.invalid_ready_pattern".into(),
        ));
    }
    match &steam.stop_strategy {
        SteamStopStrategy::Stdin {
            command,
            timeout_seconds,
        } => {
            if command.is_empty()
                || command.len() > 256
                || command.contains(['\0', '\r', '\n'])
                || !(1..=300).contains(timeout_seconds)
            {
                return Err(AppError::BadRequest(
                    "profiles.steam.invalid_stop_strategy".into(),
                ));
            }
        }
        SteamStopStrategy::Interrupt { timeout_seconds }
        | SteamStopStrategy::Terminate { timeout_seconds } => {
            if !(1..=300).contains(timeout_seconds) {
                return Err(AppError::BadRequest(
                    "profiles.steam.invalid_stop_strategy".into(),
                ));
            }
        }
    }
    Ok(())
}

fn supported_steam_platforms(steam: &SteamProfile) -> Vec<SupportedPlatform> {
    let mut platforms = Vec::new();
    if steam.executable.linux_x86_64.is_some() {
        platforms.push(SupportedPlatform::LinuxX86_64);
    }
    if steam.executable.windows_x86_64.is_some() {
        platforms.push(SupportedPlatform::WindowsX86_64);
    }
    platforms
}

fn validate_profile_label(value: &str, max: usize, error: &str) -> Result<(), AppError> {
    if value.trim() != value
        || value.is_empty()
        || value.chars().count() > max
        || value.chars().any(char::is_control)
    {
        Err(AppError::BadRequest(error.into()))
    } else {
        Ok(())
    }
}

fn validate_steam_executable(value: Option<&str>, windows: bool) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.len() > 512 || safe_join(Path::new("game"), value).is_err() {
        return Err(AppError::BadRequest(
            "profiles.steam.invalid_executable".into(),
        ));
    }
    if windows && !value.to_ascii_lowercase().ends_with(".exe") {
        return Err(AppError::BadRequest(
            "profiles.steam.invalid_executable".into(),
        ));
    }
    Ok(())
}

fn validate_steam_ports(ports: &[PortSpec]) -> Result<(), AppError> {
    if ports.is_empty() || ports.len() > 16 {
        return Err(AppError::BadRequest("profiles.steam.invalid_ports".into()));
    }
    let mut names = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for port in ports {
        if port.name.is_empty()
            || port.name.len() > 32
            || !port.name.as_bytes()[0].is_ascii_lowercase()
            || !port
                .name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            || port.default == 0
            || !names.insert(port.name.clone())
            || !bindings.insert((port.protocol.clone(), port.default))
        {
            return Err(AppError::BadRequest("profiles.steam.invalid_ports".into()));
        }
    }
    for port in ports {
        if let Some(parent_name) = &port.adjacent_to {
            let parent = ports
                .iter()
                .find(|candidate| &candidate.name == parent_name)
                .ok_or_else(|| AppError::BadRequest("profiles.steam.invalid_ports".into()))?;
            if parent.name == port.name
                || parent.protocol != port.protocol
                || parent.default.checked_add(1) != Some(port.default)
            {
                return Err(AppError::BadRequest("profiles.steam.invalid_ports".into()));
            }
        }
    }
    Ok(())
}

fn validate_steam_save_paths(paths: &[String]) -> Result<(), AppError> {
    if paths.is_empty() || paths.len() > 32 {
        return Err(AppError::BadRequest(
            "profiles.steam.invalid_save_paths".into(),
        ));
    }
    let mut unique = BTreeSet::new();
    for path in paths {
        if path.len() > 512
            || path.contains(['*', '?', '['])
            || safe_join(Path::new("game"), path).is_err()
            || !unique.insert(path)
        {
            return Err(AppError::BadRequest(
                "profiles.steam.invalid_save_paths".into(),
            ));
        }
    }
    Ok(())
}

fn validate_profile_settings(profile: &GameProfile, settings: &Value) -> Result<(), AppError> {
    let profile_id = profile.id.as_str();
    let object = settings
        .as_object()
        .ok_or_else(|| AppError::BadRequest("servers.settings_must_be_object".into()))?;

    if serde_json::to_vec(settings)
        .map_err(|_| AppError::BadRequest("servers.invalid_settings".into()))?
        .len()
        > 64 * 1024
    {
        return Err(AppError::BadRequest("servers.settings_too_large".into()));
    }

    validate_json_schema(settings, &profile.settings_schema, "settings")?;

    for forbidden in ["password", "token", "secret", "api_key", "webhook_url"] {
        if object
            .keys()
            .any(|key| key.to_ascii_lowercase().contains(forbidden))
        {
            return Err(AppError::BadRequest(
                "servers.secrets_require_secret_endpoint".into(),
            ));
        }
    }

    for port in &profile.ports {
        if let Some(value) = object.get(&port.name) {
            let number = value
                .as_u64()
                .ok_or_else(|| AppError::BadRequest("servers.invalid_port".into()))?;
            if !(1..=65_535).contains(&number) {
                return Err(AppError::BadRequest("servers.invalid_port".into()));
            }
        }
    }

    if profile_id.starts_with("minecraft-")
        && object.get("eula_accepted").and_then(Value::as_bool) != Some(true)
    {
        return Err(AppError::BadRequest(
            "servers.minecraft_eula_required".into(),
        ));
    }

    let minecraft_loader = if profile_id == "minecraft-java" {
        object.get("loader").and_then(Value::as_str)
    } else {
        profile_id.strip_prefix("minecraft-java-")
    };
    if minecraft_loader.is_some_and(|loader| {
        matches!(loader, "fabric" | "forge" | "neoforge" | "purpur" | "quilt")
    }) {
        let loader_name = minecraft_loader.expect("checked above");
        let loader = object
            .get("loader_version")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("servers.minecraft_loader_required".into()))?;
        let floating = matches!(
            loader.to_ascii_lowercase().as_str(),
            "latest" | "recommended" | "stable"
        );
        if loader.is_empty()
            || loader.len() > 96
            || floating
            || !loader.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            })
        {
            return Err(AppError::BadRequest(
                "servers.minecraft_loader_invalid".into(),
            ));
        }
        if loader_name == "purpur"
            && (loader.len() > 12
                || !loader.bytes().all(|byte| byte.is_ascii_digit())
                || loader.starts_with('0'))
        {
            return Err(AppError::BadRequest(
                "servers.minecraft_loader_invalid".into(),
            ));
        }
    }
    if profile_id == "minecraft-java"
        && !matches!(
            minecraft_loader,
            Some(
                "vanilla"
                    | "paper"
                    | "fabric"
                    | "forge"
                    | "neoforge"
                    | "spigot"
                    | "purpur"
                    | "quilt"
            )
        )
    {
        return Err(AppError::BadRequest(
            "servers.minecraft_loader_invalid".into(),
        ));
    }

    if profile_id == "valheim" {
        let server_name = object
            .get("server_name")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("servers.settings_invalid".into()))?;
        let world_name = object
            .get("world_name")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("servers.settings_invalid".into()))?;
        if server_name.trim() != server_name || server_name.chars().any(char::is_control) {
            return Err(AppError::BadRequest(
                "servers.valheim_server_name_invalid".into(),
            ));
        }
        if world_name.trim() != world_name
            || world_name.ends_with(['.', ' '])
            || world_name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
            || world_name.chars().any(char::is_control)
        {
            return Err(AppError::BadRequest(
                "servers.valheim_world_name_invalid".into(),
            ));
        }
        let game_port = object.get("port").and_then(Value::as_u64).unwrap_or(2456);
        let query_port = object
            .get("query_port")
            .and_then(Value::as_u64)
            .unwrap_or(game_port + 1);
        if query_port != game_port + 1 {
            return Err(AppError::BadRequest(
                "servers.valheim_adjacent_ports_required".into(),
            ));
        }
    }
    if profile_id == "palworld"
        && let Some(name) = object.get("server_name").and_then(Value::as_str)
        && (name.trim() != name || name.chars().any(char::is_control))
    {
        return Err(AppError::BadRequest(
            "servers.palworld_server_name_invalid".into(),
        ));
    }
    if matches!(profile_id, "seven-days-to-die" | "project-zomboid" | "rust")
        && let Some(name) = object.get("server_name").and_then(Value::as_str)
        && (name.trim() != name || name.is_empty() || name.chars().any(char::is_control))
    {
        return Err(AppError::BadRequest("servers.server_name_invalid".into()));
    }
    if profile_id == "satisfactory" {
        let port = object.get("port").and_then(Value::as_u64).unwrap_or(7777);
        let reliable_port = object
            .get("reliable_port")
            .and_then(Value::as_u64)
            .unwrap_or(8888);
        if port == reliable_port {
            return Err(AppError::BadRequest(
                "servers.distinct_ports_required".into(),
            ));
        }
    }
    if profile_id == "seven-days-to-die" {
        let port = object.get("port").and_then(Value::as_u64).unwrap_or(26_900);
        let query_port = object
            .get("query_port")
            .and_then(Value::as_u64)
            .unwrap_or(26_901);
        let steam_port = object
            .get("steam_port")
            .and_then(Value::as_u64)
            .unwrap_or(26_902);
        if query_port != port + 1 || steam_port != port + 2 {
            return Err(AppError::BadRequest(
                "servers.adjacent_ports_required".into(),
            ));
        }
    }
    if profile_id == "project-zomboid" {
        let server_name = object
            .get("server_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !server_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AppError::BadRequest(
                "servers.project_zomboid_server_name_invalid".into(),
            ));
        }
    }
    if profile_id == "rust" {
        let identity = object
            .get("identity")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if identity.is_empty()
            || identity.len() > 48
            || !identity
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AppError::BadRequest("servers.rust_identity_invalid".into()));
        }
        let game_port = object.get("port").and_then(Value::as_u64).unwrap_or(28_015);
        let query_port = object
            .get("query_port")
            .and_then(Value::as_u64)
            .unwrap_or(28_017);
        let rcon_port = object
            .get("rcon_port")
            .and_then(Value::as_u64)
            .unwrap_or(28_016);
        if BTreeSet::from([game_port, query_port, rcon_port]).len() != 3 {
            return Err(AppError::BadRequest(
                "servers.distinct_ports_required".into(),
            ));
        }
    }
    if profile_id == "minecraft-bedrock" {
        let port = object.get("port").and_then(Value::as_u64).unwrap_or(19_132);
        let port_v6 = object
            .get("port_v6")
            .and_then(Value::as_u64)
            .unwrap_or(19_133);
        if port == port_v6 {
            return Err(AppError::BadRequest(
                "servers.bedrock_distinct_ports_required".into(),
            ));
        }
        if object.get("enable_lan_visibility").and_then(Value::as_bool) == Some(true)
            && (port != 19_132 || port_v6 != 19_133)
        {
            return Err(AppError::BadRequest(
                "servers.bedrock_lan_visibility_port_conflict".into(),
            ));
        }
        if let Some(name) = object.get("server_name").and_then(Value::as_str)
            && (name.contains(';') || name.chars().any(char::is_control))
        {
            return Err(AppError::BadRequest(
                "servers.bedrock_server_name_invalid".into(),
            ));
        }
        if let Some(name) = object.get("level_name").and_then(Value::as_str)
            && (name.trim() != name
                || name.ends_with(['.', ' '])
                || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
                || name.chars().any(char::is_control))
        {
            return Err(AppError::BadRequest(
                "servers.bedrock_level_name_invalid".into(),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn base_profile(
    id: &str,
    name: &str,
    description: &str,
    platforms: Vec<SupportedPlatform>,
    ports: Vec<PortSpec>,
    capabilities: &[&str],
    lifecycle: LifecycleSpec,
    properties: Value,
    required: &[&str],
) -> GameProfile {
    GameProfile {
        id: id.to_string(),
        revision: 1,
        name: name.to_string(),
        description: description.to_string(),
        kind: ProfileKind::Builtin,
        platforms,
        capabilities: capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        ports,
        lifecycle,
        settings_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": required,
            "properties": properties,
        }),
        ui_schema: json!({"layout": "sections"}),
        steam_profile: None,
    }
}

fn game_port(name: &str, protocol: PortProtocol, default: u16) -> PortSpec {
    PortSpec {
        name: name.to_string(),
        protocol,
        default,
        adjacent_to: None,
    }
}

fn stdin_stop(command: &str) -> LifecycleSpec {
    LifecycleSpec {
        stop: StopStrategy::Stdin {
            command: command.to_string(),
            timeout_seconds: 30,
        },
        ready_log_pattern: None,
    }
}

fn hytale() -> GameProfile {
    base_profile(
        "hytale",
        "Hytale",
        "Serveur Hytale officiel avec Java 25 et authentification device.",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![game_port("port", PortProtocol::Udp, 5520)],
        &[
            "settings",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
            "mods",
        ],
        LifecycleSpec {
            stop: StopStrategy::Stdin {
                command: "stop".into(),
                timeout_seconds: 30,
            },
            ready_log_pattern: Some("Hytale Server Booted".into()),
        },
        json!({
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 5520},
            "max_memory_mb": {"type": "integer", "minimum": 1024, "maximum": 131072, "default": 8192},
            "auth_mode": {"type": "string", "enum": ["authenticated", "offline"], "default": "authenticated"}
        }),
        &[],
    )
}

fn minecraft_java_unified() -> GameProfile {
    base_profile(
        "minecraft-java",
        "Minecraft Java",
        "Serveur Minecraft Java avec loader, version et runtime Java configurables.",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![game_port("port", PortProtocol::Tcp, 25565)],
        &[
            "settings",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
            "mods",
        ],
        LifecycleSpec {
            stop: StopStrategy::Stdin {
                command: "stop".into(),
                timeout_seconds: 60,
            },
            ready_log_pattern: Some("Done \\(.+\\)! For help".into()),
        },
        json!({
            "loader": {
                "type": "string",
                "enum": ["vanilla", "paper", "fabric", "forge", "neoforge", "spigot", "purpur", "quilt"],
                "default": "vanilla",
                "title": "Loader",
                "x-dmx-immutable-after-install": true
            },
            "version": {
                "type": "string",
                "minLength": 1,
                "maxLength": 64,
                "title": "Minecraft version",
                "x-dmx-immutable-after-install": true
            },
            "loader_version": {
                "type": "string",
                "minLength": 1,
                "maxLength": 96,
                "pattern": "^[A-Za-z0-9._+-]+$",
                "title": "Loader version",
                "description": "Exact immutable provider version when the selected loader requires one.",
                "x-dmx-immutable-after-install": true
            },
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 25565},
            "max_memory_mb": {"type": "integer", "minimum": 512, "maximum": 131072, "default": 4096},
            "eula_accepted": {"type": "boolean", "const": true}
        }),
        &["loader", "version", "eula_accepted"],
    )
}

fn minecraft_java(id: &str, name: &str) -> GameProfile {
    let exposes_mods = matches!(
        id,
        "minecraft-java-paper"
            | "minecraft-java-fabric"
            | "minecraft-java-forge"
            | "minecraft-java-neoforge"
            | "minecraft-java-spigot"
            | "minecraft-java-purpur"
            | "minecraft-java-quilt"
    );
    let requires_loader_version = matches!(
        id,
        "minecraft-java-fabric"
            | "minecraft-java-forge"
            | "minecraft-java-neoforge"
            | "minecraft-java-purpur"
            | "minecraft-java-quilt"
    );
    let mut capabilities = vec![
        "settings",
        "install",
        "lifecycle",
        "console",
        "files",
        "backups",
        "metrics",
    ];
    if exposes_mods {
        capabilities.push("mods");
    }
    let mut properties = json!({
        "version": {
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "title": "Minecraft version"
        },
        "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 25565},
        "max_memory_mb": {"type": "integer", "minimum": 512, "maximum": 131072, "default": 4096},
        "eula_accepted": {"type": "boolean", "const": true}
    });
    if requires_loader_version {
        properties
            .as_object_mut()
            .expect("Minecraft settings properties must be an object")
            .insert(
                "loader_version".into(),
                json!({
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 96,
                    "pattern": if id == "minecraft-java-purpur" { "^[1-9][0-9]{0,11}$" } else { "^[A-Za-z0-9._+-]+$" },
                    "title": if id == "minecraft-java-purpur" { "Purpur build" } else { "Loader version" },
                    "description": "Exact immutable provider version; floating aliases are rejected.",
                    "x-dmx-immutable-after-install": true
                }),
            );
    }
    let mut required = vec!["version", "eula_accepted"];
    if requires_loader_version {
        required.push("loader_version");
    }
    base_profile(
        id,
        name,
        "Serveur Minecraft Java avec version et runtime Java épinglés.",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![game_port("port", PortProtocol::Tcp, 25565)],
        &capabilities,
        LifecycleSpec {
            stop: StopStrategy::Stdin {
                command: "stop".into(),
                timeout_seconds: 60,
            },
            ready_log_pattern: Some("Done \\(.+\\)! For help".into()),
        },
        properties,
        &required,
    )
}

fn minecraft_bedrock() -> GameProfile {
    base_profile(
        "minecraft-bedrock",
        "Minecraft Bedrock",
        "Serveur Bedrock Dedicated Server officiel.",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![
            game_port("port", PortProtocol::Udp, 19132),
            game_port("port_v6", PortProtocol::Udp, 19133),
        ],
        &[
            "settings",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        stdin_stop("stop"),
        json!({
            "version": {"type": "string", "minLength": 1, "maxLength": 64},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 19132},
            "port_v6": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 19133},
            "server_name": {"type": "string", "minLength": 1, "maxLength": 64, "default": "Dedicated Server"},
            "level_name": {"type": "string", "minLength": 1, "maxLength": 64, "default": "Bedrock level"},
            "gamemode": {"type": "string", "enum": ["survival", "creative", "adventure"], "default": "survival"},
            "difficulty": {"type": "string", "enum": ["peaceful", "easy", "normal", "hard"], "default": "easy"},
            "max_players": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 10},
            "online_mode": {"type": "boolean", "default": true},
            "allow_list": {"type": "boolean", "default": false},
            "enable_lan_visibility": {"type": "boolean", "default": false},
            "view_distance": {"type": "integer", "minimum": 5, "maximum": 96, "default": 32},
            "tick_distance": {"type": "integer", "minimum": 4, "maximum": 12, "default": 4},
            "player_idle_timeout": {"type": "integer", "minimum": 0, "maximum": 10080, "default": 30},
            "default_player_permission_level": {"type": "string", "enum": ["visitor", "member", "operator"], "default": "member"},
            "texturepack_required": {"type": "boolean", "default": false},
            "eula_accepted": {"type": "boolean", "const": true}
        }),
        &["version", "eula_accepted"],
    )
}

fn valheim() -> GameProfile {
    let mut query_port = game_port("query_port", PortProtocol::Udp, 2457);
    query_port.adjacent_to = Some("port".into());
    base_profile(
        "valheim",
        "Valheim",
        "Serveur Valheim installé anonymement par SteamCMD (AppID 896660).",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![game_port("port", PortProtocol::Udp, 2456), query_port],
        &[
            "settings",
            "secrets",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            stop: StopStrategy::Interrupt {
                timeout_seconds: 60,
            },
            ready_log_pattern: Some("Game server connected".into()),
        },
        json!({
            "server_name": {"type": "string", "minLength": 1, "maxLength": 64},
            "world_name": {"type": "string", "minLength": 1, "maxLength": 64},
            "port": {"type": "integer", "minimum": 1, "maximum": 65534, "default": 2456},
            "query_port": {"type": "integer", "minimum": 2, "maximum": 65535, "default": 2457},
            "crossplay": {"type": "boolean", "default": false},
            "server_password": {"type": "string", "minLength": 5, "maxLength": 64, "secret": true, "writeOnly": true}
        }),
        &["server_name", "world_name", "server_password"],
    )
}

fn palworld() -> GameProfile {
    let mut profile = base_profile(
        "palworld",
        "Palworld",
        "Serveur Palworld installé anonymement par SteamCMD (AppID 2394010).",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![game_port("port", PortProtocol::Udp, 8211)],
        &[
            "settings",
            "secrets",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            // PalServer installs an Unreal shutdown handler. SIGINT on Unix and
            // CTRL_BREAK on Windows let it flush the world before exiting.
            stop: StopStrategy::Interrupt {
                timeout_seconds: 60,
            },
            ready_log_pattern: Some("Running Palworld dedicated server on".into()),
        },
        json!({
            "server_name": {"type": "string", "minLength": 1, "maxLength": 64},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 8211},
            "public_server": {"type": "boolean", "default": false},
            "server_password": {"type": "string", "minLength": 1, "maxLength": 64, "secret": true, "writeOnly": true},
            "admin_password": {"type": "string", "minLength": 1, "maxLength": 64, "secret": true, "writeOnly": true}
        }),
        &["server_name"],
    );
    // Revision 1 used the early Steam breakpad line as readiness evidence.
    // Keep persisted revision 1 immutable and publish the corrected server
    // readiness contract as a new built-in profile revision.
    profile.revision = 2;
    profile
}

fn satisfactory() -> GameProfile {
    base_profile(
        "satisfactory",
        "Satisfactory",
        "Serveur Satisfactory installé anonymement par SteamCMD (AppID 1690800).",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![
            game_port("port", PortProtocol::Udp, 7777),
            game_port("port", PortProtocol::Tcp, 7777),
            game_port("reliable_port", PortProtocol::Tcp, 8888),
        ],
        &[
            "settings",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            stop: StopStrategy::Interrupt {
                timeout_seconds: 90,
            },
            ready_log_pattern: Some("Game Engine Initialized".into()),
        },
        json!({
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 7777},
            "reliable_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 8888}
        }),
        &[],
    )
}

fn seven_days_to_die() -> GameProfile {
    let mut query_port = game_port("query_port", PortProtocol::Udp, 26901);
    query_port.adjacent_to = Some("port".into());
    let mut steam_port = game_port("steam_port", PortProtocol::Udp, 26902);
    steam_port.adjacent_to = Some("query_port".into());
    base_profile(
        "seven-days-to-die",
        "7 Days to Die",
        "Serveur 7 Days to Die installé anonymement par SteamCMD (AppID 294420).",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![
            game_port("port", PortProtocol::Tcp, 26900),
            game_port("port", PortProtocol::Udp, 26900),
            query_port,
            steam_port,
        ],
        &[
            "settings",
            "secrets",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            stop: StopStrategy::Stdin {
                command: "shutdown".into(),
                timeout_seconds: 90,
            },
            ready_log_pattern: Some("INF GameServer.LogOn successful".into()),
        },
        json!({
            "server_name": {"type": "string", "minLength": 1, "maxLength": 64, "default": "7 Days to Die Server"},
            "world_name": {"type": "string", "minLength": 1, "maxLength": 64, "default": "Navezgane"},
            "game_name": {"type": "string", "minLength": 1, "maxLength": 64, "default": "DmxWorld"},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 26900},
            "query_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 26901},
            "steam_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 26902},
            "max_players": {"type": "integer", "minimum": 1, "maximum": 64, "default": 8},
            "public_server": {"type": "boolean", "default": false},
            "server_password": {"type": "string", "minLength": 1, "maxLength": 64, "secret": true, "writeOnly": true}
        }),
        &["server_name", "world_name", "game_name"],
    )
}

fn project_zomboid() -> GameProfile {
    base_profile(
        "project-zomboid",
        "Project Zomboid",
        "Serveur Project Zomboid installé anonymement par SteamCMD (AppID 380870).",
        vec![SupportedPlatform::LinuxX86_64],
        vec![
            game_port("port", PortProtocol::Udp, 16261),
            game_port("steam_port", PortProtocol::Udp, 8766),
            game_port("steam_query_port", PortProtocol::Udp, 8767),
        ],
        &[
            "settings",
            "secrets",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            stop: StopStrategy::Stdin {
                command: "quit".into(),
                timeout_seconds: 90,
            },
            ready_log_pattern: Some("SERVER STARTED".into()),
        },
        json!({
            "server_name": {"type": "string", "minLength": 1, "maxLength": 48, "pattern": "^[A-Za-z0-9_-]+$", "default": "dmxserver"},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 16261},
            "steam_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 8766},
            "steam_query_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 8767},
            "admin_password": {"type": "string", "minLength": 8, "maxLength": 64, "secret": true, "writeOnly": true}
        }),
        &["server_name", "admin_password"],
    )
}

fn rust_server() -> GameProfile {
    base_profile(
        "rust",
        "Rust",
        "Serveur Rust installé anonymement par SteamCMD (AppID 258550).",
        vec![
            SupportedPlatform::LinuxX86_64,
            SupportedPlatform::WindowsX86_64,
        ],
        vec![
            game_port("port", PortProtocol::Udp, 28015),
            game_port("rcon_port", PortProtocol::Tcp, 28016),
            game_port("query_port", PortProtocol::Udp, 28017),
        ],
        &[
            "settings",
            "secrets",
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ],
        LifecycleSpec {
            stop: StopStrategy::Interrupt {
                timeout_seconds: 90,
            },
            ready_log_pattern: Some("Server startup complete".into()),
        },
        json!({
            "server_name": {"type": "string", "minLength": 1, "maxLength": 128, "default": "Rust Server"},
            "identity": {"type": "string", "minLength": 1, "maxLength": 48, "pattern": "^[A-Za-z0-9_-]+$", "default": "dmxserver"},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 28015},
            "rcon_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 28016},
            "query_port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 28017},
            "max_players": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50},
            "world_size": {"type": "integer", "minimum": 1000, "maximum": 6000, "default": 3500},
            "seed": {"type": "integer", "minimum": 0, "maximum": 2147483647, "default": 12345},
            "rcon_password": {"type": "string", "minLength": 12, "maxLength": 128, "secret": true, "writeOnly": true}
        }),
        &["server_name", "identity", "rcon_password"],
    )
}

fn validate_json_schema(value: &Value, schema: &Value, path: &str) -> Result<(), AppError> {
    let expected_type = schema.get("type").and_then(Value::as_str);
    let type_matches = match expected_type {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        None => true,
        Some(_) => false,
    };
    if !type_matches {
        return Err(AppError::BadRequest(format!(
            "servers.settings_type:{path}"
        )));
    }

    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err(AppError::BadRequest(format!(
            "servers.settings_const:{path}"
        )));
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        return Err(AppError::BadRequest(format!(
            "servers.settings_enum:{path}"
        )));
    }

    if let Some(object) = value.as_object() {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false)
            && let Some(key) = object.keys().find(|key| !properties.contains_key(*key))
        {
            return Err(AppError::BadRequest(format!(
                "servers.settings_unknown:{path}.{key}"
            )));
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if properties
                    .get(key)
                    .and_then(|property| property.get("secret"))
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    continue;
                }
                if !object.contains_key(key) {
                    return Err(AppError::BadRequest(format!(
                        "servers.settings_required:{path}.{key}"
                    )));
                }
            }
        }
        for (key, child) in object {
            if let Some(child_schema) = properties.get(key) {
                validate_json_schema(child, child_schema, &format!("{path}.{key}"))?;
            }
        }
    }

    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|min| length < min)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|max| length > max)
        {
            return Err(AppError::BadRequest(format!(
                "servers.settings_length:{path}"
            )));
        }
    }
    if let Some(number) = value.as_f64()
        && (schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|min| number < min)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|max| number > max))
    {
        return Err(AppError::BadRequest(format!(
            "servers.settings_range:{path}"
        )));
    }
    if let Some(array) = value.as_array() {
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|min| (array.len() as u64) < min)
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|max| array.len() as u64 > max)
        {
            return Err(AppError::BadRequest(format!(
                "servers.settings_items:{path}"
            )));
        }
        if let Some(items) = schema.get("items") {
            for (index, child) in array.iter().enumerate() {
                validate_json_schema(child, items, &format!("{path}[{index}]"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::v1::SteamExecutable;

    #[test]
    fn registry_contains_every_committed_builtin_profile() {
        let registry = ProfileRegistry::builtins();
        for id in [
            "hytale",
            "minecraft-java",
            "minecraft-java-vanilla",
            "minecraft-java-paper",
            "minecraft-java-fabric",
            "minecraft-java-forge",
            "minecraft-java-neoforge",
            "minecraft-java-spigot",
            "minecraft-java-purpur",
            "minecraft-java-quilt",
            "minecraft-bedrock",
            "valheim",
            "palworld",
            "satisfactory",
            "seven-days-to-die",
            "project-zomboid",
            "rust",
        ] {
            assert!(registry.get(id).is_some(), "missing profile {id}");
        }
        assert_eq!(
            registry.get("palworld").map(|profile| profile.revision),
            Some(2)
        );
        assert!(registry.all().iter().all(|profile| {
            profile.capabilities.iter().all(|capability| {
                matches!(
                    capability.as_str(),
                    "settings"
                        | "secrets"
                        | "install"
                        | "lifecycle"
                        | "console"
                        | "files"
                        | "backups"
                        | "metrics"
                        | "mods"
                )
            })
        }));
        let bedrock = registry.get("minecraft-bedrock").unwrap();
        for capability in [
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
        ] {
            assert!(bedrock.capabilities.iter().any(|value| value == capability));
        }
        assert!(matches!(
            registry.get("palworld").unwrap().lifecycle.stop,
            StopStrategy::Interrupt {
                timeout_seconds: 60
            }
        ));
        let hytale = registry.get("hytale").unwrap();
        for capability in [
            "install",
            "lifecycle",
            "console",
            "files",
            "backups",
            "metrics",
            "mods",
        ] {
            assert!(
                hytale.capabilities.iter().any(|value| value == capability),
                "Hytale must expose {capability} only while its backend is implemented"
            );
        }
        for id in [
            "minecraft-java-vanilla",
            "minecraft-java-paper",
            "minecraft-java-fabric",
            "minecraft-java-forge",
            "minecraft-java-neoforge",
            "minecraft-java-spigot",
            "minecraft-java-purpur",
            "minecraft-java-quilt",
        ] {
            let profile = registry.get(id).unwrap();
            assert!(profile.capabilities.iter().any(|value| value == "install"));
        }
        for id in [
            "minecraft-java-paper",
            "minecraft-java-fabric",
            "minecraft-java-forge",
            "minecraft-java-neoforge",
            "minecraft-java-spigot",
            "minecraft-java-purpur",
            "minecraft-java-quilt",
        ] {
            let profile = registry.get(id).unwrap();
            assert!(profile.capabilities.iter().any(|value| value == "mods"));
        }
        for id in [
            "minecraft-java-fabric",
            "minecraft-java-forge",
            "minecraft-java-neoforge",
            "minecraft-java-purpur",
            "minecraft-java-quilt",
        ] {
            let profile = registry.get(id).unwrap();
            let required = profile.settings_schema["required"].as_array().unwrap();
            assert!(required.iter().any(|value| value == "loader_version"));
            assert!(profile.settings_schema["properties"]["loader_version"].is_object());
        }
        let spigot = registry.get("minecraft-java-spigot").unwrap();
        assert!(spigot.settings_schema["properties"]["loader_version"].is_null());
        let unified = registry.get("minecraft-java").unwrap();
        assert_eq!(
            unified.settings_schema["properties"]["loader"]["default"],
            "vanilla"
        );
    }

    #[test]
    fn minecraft_loader_profiles_require_exact_safe_versions() {
        let registry = ProfileRegistry::builtins();
        let base = json!({
            "version": "1.21.1",
            "loader_version": "0.16.14",
            "eula_accepted": true
        });
        registry
            .validate_settings("minecraft-java-fabric", &base)
            .unwrap();
        registry
            .validate_settings(
                "minecraft-java",
                &json!({
                    "loader": "fabric",
                    "version": "1.21.1",
                    "loader_version": "0.16.14",
                    "eula_accepted": true
                }),
            )
            .unwrap();
        assert!(
            registry
                .validate_settings(
                    "minecraft-java",
                    &json!({
                        "loader": "fabric",
                        "version": "1.21.1",
                        "eula_accepted": true
                    }),
                )
                .is_err()
        );
        for value in ["latest", "../loader", "loader/version", "loader version"] {
            let mut invalid = base.clone();
            invalid["loader_version"] = json!(value);
            assert!(
                registry
                    .validate_settings("minecraft-java-fabric", &invalid)
                    .is_err(),
                "accepted unsafe loader version {value:?}"
            );
        }
        for value in ["0", "0123", "latest", "12.3"] {
            assert!(
                registry
                    .validate_settings(
                        "minecraft-java-purpur",
                        &json!({
                            "version": "1.21.1",
                            "loader_version": value,
                            "eula_accepted": true
                        })
                    )
                    .is_err(),
                "accepted invalid Purpur build {value:?}"
            );
        }
        registry
            .validate_settings(
                "minecraft-java-purpur",
                &json!({
                    "version": "1.21.1",
                    "loader_version": "2568",
                    "eula_accepted": true
                }),
            )
            .unwrap();
    }

    #[test]
    fn local_steam_profiles_reject_host_executables_and_invalid_placeholders() {
        let profile = SteamProfile {
            app_id: 896_660,
            branch: None,
            executable: SteamExecutable {
                linux_x86_64: Some("/bin/sh".into()),
                windows_x86_64: None,
            },
            arguments: vec!["{{env:PATH}}".into()],
            ports: vec![game_port("game", PortProtocol::Udp, 2456)],
            save_paths: vec!["worlds".into()],
            ready_log_pattern: None,
            stop_strategy: SteamStopStrategy::Interrupt {
                timeout_seconds: 30,
            },
        };
        assert!(
            build_local_steam_profile(
                "steam-valheim-test".into(),
                1,
                "Valheim test".into(),
                "Test".into(),
                profile,
            )
            .is_err()
        );
    }

    #[test]
    fn local_steam_profile_is_immutable_by_revision_in_registry() {
        let registry = ProfileRegistry::builtins();
        for revision in [1, 2] {
            let profile = build_local_steam_profile(
                "steam-example".into(),
                revision,
                format!("Example {revision}"),
                "Anonymous SteamCMD server".into(),
                SteamProfile {
                    app_id: 42,
                    branch: None,
                    executable: SteamExecutable {
                        linux_x86_64: Some("server".into()),
                        windows_x86_64: Some("server.exe".into()),
                    },
                    arguments: vec!["--port".into(), "{{port:game}}".into()],
                    ports: vec![game_port("game", PortProtocol::Udp, 27015)],
                    save_paths: vec!["saves".into()],
                    ready_log_pattern: Some("Ready".into()),
                    stop_strategy: SteamStopStrategy::Terminate {
                        timeout_seconds: 30,
                    },
                },
            )
            .unwrap();
            registry.register(profile);
        }
        assert_eq!(registry.get("steam-example").unwrap().revision, 2);
        assert_eq!(
            registry.get_revision("steam-example", 1).unwrap().name,
            "Example 1"
        );
    }

    #[test]
    fn minecraft_requires_explicit_eula_acceptance() {
        let registry = ProfileRegistry::builtins();
        let settings = json!({"version": "1.21.8", "eula_accepted": false});
        assert!(
            registry
                .validate_settings("minecraft-java-vanilla", &settings)
                .is_err()
        );
    }

    #[test]
    fn native_steam_profile_names_cannot_become_paths_or_multiline_configuration() {
        let registry = ProfileRegistry::builtins();
        let valid = json!({
            "server_name": "Friends server",
            "world_name": "Dedicated-01"
        });
        registry.validate_settings("valheim", &valid).unwrap();
        for world_name in ["../outside", "folder/world", "bad\nworld", "trailing."] {
            let mut invalid = valid.clone();
            invalid["world_name"] = json!(world_name);
            assert!(registry.validate_settings("valheim", &invalid).is_err());
        }
        assert!(
            registry
                .validate_settings(
                    "palworld",
                    &json!({"server_name": "Name\nAdminPassword=oops"}),
                )
                .is_err()
        );
    }

    #[test]
    fn schema_validation_rejects_missing_and_unknown_settings() {
        let registry = ProfileRegistry::builtins();
        assert!(
            registry
                .validate_settings(
                    "minecraft-java-vanilla",
                    &json!({"version": "1.21.8", "eula_accepted": true, "shell": "/bin/sh"}),
                )
                .is_err()
        );
        assert!(
            registry
                .validate_settings("minecraft-java-vanilla", &json!({"eula_accepted": true}),)
                .is_err()
        );
    }

    #[tokio::test]
    async fn builtin_revisions_are_insert_only_and_historical_revisions_reload() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE game_profiles (\
                id TEXT NOT NULL, revision INTEGER NOT NULL, kind TEXT NOT NULL, \
                manifest TEXT NOT NULL, created_at TEXT NOT NULL, \
                PRIMARY KEY (id, revision)\
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let revision_one = hytale();
        let registry_one = registry_with_builtins([revision_one.clone()]);
        registry_one.persist_builtins(&pool).await.unwrap();

        let mut revision_two = revision_one.clone();
        revision_two.revision = 2;
        revision_two.description = "Hytale profile revision two".into();
        let registry_two = registry_with_builtins([revision_two.clone()]);
        registry_two.persist_builtins(&pool).await.unwrap();
        registry_two.load_persisted(&pool).await.unwrap();

        assert_eq!(registry_two.get("hytale"), Some(revision_two));
        assert_eq!(
            registry_two.get_revision("hytale", 1),
            Some(revision_one.clone())
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_profiles WHERE id = 'hytale'",)
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );

        let mut mutated_revision_one = revision_one;
        mutated_revision_one.description = "silently mutated".into();
        let invalid_registry = registry_with_builtins([mutated_revision_one]);
        assert!(invalid_registry.persist_builtins(&pool).await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_profiles WHERE id = 'hytale'",)
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }

    fn registry_with_builtins(profiles: impl IntoIterator<Item = GameProfile>) -> ProfileRegistry {
        ProfileRegistry {
            profiles: Arc::new(RwLock::new(
                profiles
                    .into_iter()
                    .map(|profile| ((profile.id.clone(), profile.revision), profile))
                    .collect(),
            )),
        }
    }
}
