use std::{collections::BTreeMap, path::Path};

use axum::body::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::AsyncReadExt;

use crate::core::{DbPool, database, error::AppError, events::EventHub};

use super::{secrets::SecretStore, secure_fs};

pub const MAX_CONFIG_TEXT_BYTES: usize = secure_fs::MAX_TEXT_BYTES;
const MAX_DISCOVERED_FILES: usize = 256;
const MAX_DISCOVERY_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCategory {
    Configuration,
    Access,
}

impl ConfigCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Access => "access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    Json,
    Properties,
    Ini,
    Toml,
    Yaml,
    Xml,
    Lua,
    Text,
}

impl ConfigFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Properties => "properties",
            Self::Ini => "ini",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Xml => "xml",
            Self::Lua => "lua",
            Self::Text => "text",
        }
    }

    fn from_path(path: &str) -> Option<Self> {
        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())?
            .to_ascii_lowercase();
        match extension.as_str() {
            "json" => Some(Self::Json),
            "properties" | "cfg" | "conf" => Some(Self::Properties),
            "ini" => Some(Self::Ini),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "xml" => Some(Self::Xml),
            "lua" => Some(Self::Lua),
            "txt" | "options" => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigFileSpec {
    pub relative_path: String,
    pub category: ConfigCategory,
    pub format: ConfigFormat,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigChangeSummary {
    pub id: String,
    pub status: String,
    pub content_sha256: String,
    pub error_code: Option<String>,
    pub queued_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFileSummary {
    pub path: String,
    pub category: ConfigCategory,
    pub format: ConfigFormat,
    pub exists: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub sha256: Option<String>,
    pub queued_change: Option<ConfigChangeSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFileDocument {
    pub file: ConfigFileSummary,
    pub content: String,
    pub queued_content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFileList {
    pub items: Vec<ConfigFileSummary>,
    pub pending_count: usize,
}

pub struct QueueConfigChange<'a> {
    pub path: &'a str,
    pub content: &'a str,
    pub expected_sha256: Option<&'a str>,
    pub queued_by: &'a str,
}

#[derive(Debug, FromRow)]
struct InstanceConfiguration {
    profile_id: String,
    settings: String,
}

#[derive(Debug, Clone, FromRow)]
struct ConfigChangeRow {
    id: String,
    instance_id: String,
    relative_path: String,
    format: String,
    category: String,
    base_sha256: Option<String>,
    content_sha256: String,
    content_nonce: String,
    content_ciphertext: String,
    status: String,
    error_code: Option<String>,
    queued_by: String,
    created_at: String,
}

pub async fn list(
    pool: &DbPool,
    root: &Path,
    instance_id: &str,
) -> Result<ConfigFileList, AppError> {
    let specs = specs_for_instance(pool, root, instance_id).await?;
    let changes = latest_changes(pool, instance_id).await?;
    let mut items = Vec::with_capacity(specs.len());
    for spec in specs {
        let state = read_current(root, &spec.relative_path).await?;
        let change = changes.get(&spec.relative_path).map(change_summary);
        items.push(summary(&spec, state.as_ref(), change));
    }
    let pending_count = items
        .iter()
        .filter(|item| {
            item.queued_change
                .as_ref()
                .is_some_and(|change| change.status == "pending")
        })
        .count();
    Ok(ConfigFileList {
        items,
        pending_count,
    })
}

pub async fn read(
    pool: &DbPool,
    secrets: &SecretStore,
    root: &Path,
    instance_id: &str,
    path: &str,
) -> Result<ConfigFileDocument, AppError> {
    let spec = resolve_spec(pool, root, instance_id, path).await?;
    let current = read_current(root, &spec.relative_path).await?;
    let change = latest_change(pool, instance_id, &spec.relative_path).await?;
    let queued_content = change
        .as_ref()
        .map(|change| decrypt_change(secrets, change))
        .transpose()?;
    Ok(ConfigFileDocument {
        file: summary(&spec, current.as_ref(), change.as_ref().map(change_summary)),
        content: current.map_or_else(String::new, |current| current.content),
        queued_content,
    })
}

pub async fn queue(
    pool: &DbPool,
    secrets: &SecretStore,
    events: &EventHub,
    root: &Path,
    instance_id: &str,
    change: QueueConfigChange<'_>,
) -> Result<ConfigFileDocument, AppError> {
    let QueueConfigChange {
        path,
        content,
        expected_sha256,
        queued_by,
    } = change;
    let spec = resolve_spec(pool, root, instance_id, path).await?;
    validate_content(spec.format, content)?;
    if spec.category == ConfigCategory::Access && spec.format == ConfigFormat::Text {
        validate_access_list(content)?;
    }
    let current = read_current(root, &spec.relative_path).await?;
    let current_sha = current.as_ref().map(|current| current.sha256.as_str());
    if current_sha != expected_sha256 {
        return Err(AppError::Conflict("config_files.source_changed".into()));
    }

    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM config_changes WHERE instance_id = ? AND relative_path = ? AND status = 'pending'",
    )
    .bind(instance_id)
    .bind(&spec.relative_path)
    .fetch_optional(pool)
    .await?;
    let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let associated_data = change_associated_data(&id, instance_id, &spec.relative_path);
    let (nonce, ciphertext) =
        secrets.seal_payload(&associated_data, content, MAX_CONFIG_TEXT_BYTES)?;
    let content_sha256 = sha256(content.as_bytes());
    let now = Utc::now().to_rfc3339();
    let mut transaction = pool.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE config_changes SET format = ?, category = ?, base_sha256 = ?, content_sha256 = ?,
            content_nonce = ?, content_ciphertext = ?, error_code = NULL, queued_by = ?, updated_at = ?
        WHERE id = ? AND instance_id = ? AND status = 'pending'
        "#,
    )
    .bind(spec.format.as_str())
    .bind(spec.category.as_str())
    .bind(current_sha)
    .bind(&content_sha256)
    .bind(&nonce)
    .bind(&ciphertext)
    .bind(queued_by)
    .bind(&now)
    .bind(&id)
    .bind(instance_id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if updated == 0 {
        sqlx::query(
            r#"
            INSERT INTO config_changes
                (id, instance_id, relative_path, format, category, base_sha256, content_sha256,
                 content_nonce, content_ciphertext, status, queued_by, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(instance_id)
        .bind(&spec.relative_path)
        .bind(spec.format.as_str())
        .bind(spec.category.as_str())
        .bind(current_sha)
        .bind(&content_sha256)
        .bind(&nonce)
        .bind(&ciphertext)
        .bind(queued_by)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    database::audit(
        pool,
        Some(queued_by),
        "server.config_queued",
        "instance",
        Some(instance_id),
        "success",
        serde_json::json!({
            "change_id": id,
            "path": &spec.relative_path,
            "content_sha256": content_sha256,
        }),
    )
    .await?;
    events.publish(
        "config.queued",
        Some(instance_id.to_string()),
        serde_json::json!({"change_id": id, "path": &spec.relative_path}),
    );
    read(pool, secrets, root, instance_id, &spec.relative_path).await
}

pub async fn cancel(
    pool: &DbPool,
    events: &EventHub,
    instance_id: &str,
    path: &str,
    actor_user_id: &str,
) -> Result<(), AppError> {
    let changed = sqlx::query(
        "UPDATE config_changes SET status = 'cancelled', updated_at = ?, error_code = NULL \
         WHERE instance_id = ? AND relative_path = ? AND status = 'pending'",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(instance_id)
    .bind(path)
    .execute(pool)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(AppError::NotFound("config_files.pending_not_found".into()));
    }
    database::audit(
        pool,
        Some(actor_user_id),
        "server.config_cancelled",
        "instance",
        Some(instance_id),
        "success",
        serde_json::json!({"path": path}),
    )
    .await?;
    events.publish(
        "config.cancelled",
        Some(instance_id.to_string()),
        serde_json::json!({"path": path}),
    );
    Ok(())
}

pub async fn apply_pending(
    pool: &DbPool,
    secrets: &SecretStore,
    events: &EventHub,
    root: &Path,
    instance_id: &str,
) -> Result<usize, AppError> {
    let rows: Vec<ConfigChangeRow> = sqlx::query_as(
        r#"
        SELECT id, instance_id, relative_path, format, category, base_sha256, content_sha256,
               content_nonce, content_ciphertext, status, error_code, queued_by, created_at
        FROM config_changes
        WHERE instance_id = ? AND status = 'pending'
        ORDER BY created_at, id
        "#,
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let specs = specs_for_instance(pool, root, instance_id)
        .await?
        .into_iter()
        .map(|spec| (spec.relative_path.clone(), spec))
        .collect::<BTreeMap<_, _>>();
    let mut applied = 0_usize;
    let mut blocked = false;
    for row in rows {
        let Some(spec) = specs.get(&row.relative_path) else {
            mark_change(pool, &row.id, "failed", Some("config_file_not_allowed")).await?;
            blocked = true;
            continue;
        };
        if spec.format.as_str() != row.format || spec.category.as_str() != row.category {
            mark_change(
                pool,
                &row.id,
                "failed",
                Some("config_file_contract_changed"),
            )
            .await?;
            blocked = true;
            continue;
        }
        let content = match decrypt_change(secrets, &row) {
            Ok(content) => content,
            Err(error) => {
                tracing::error!(change_id = %row.id, instance_id, %error, "queued config decryption failed");
                mark_change(pool, &row.id, "failed", Some("config_decryption_failed")).await?;
                blocked = true;
                continue;
            }
        };
        if validate_content(spec.format, &content).is_err()
            || sha256(content.as_bytes()) != row.content_sha256
        {
            mark_change(pool, &row.id, "failed", Some("config_content_invalid")).await?;
            blocked = true;
            continue;
        }
        let current = read_current(root, &row.relative_path).await?;
        let current_sha = current.as_ref().map(|current| current.sha256.as_str());
        if current_sha != row.base_sha256.as_deref() {
            mark_change(pool, &row.id, "conflict", Some("config_source_changed")).await?;
            events.publish(
                "config.conflict",
                Some(instance_id.to_string()),
                serde_json::json!({"change_id": row.id, "path": row.relative_path}),
            );
            blocked = true;
            continue;
        }
        if let Err(error) = secure_fs::write_declared_bytes(
            root,
            &row.relative_path,
            Bytes::from(content),
            MAX_CONFIG_TEXT_BYTES,
        )
        .await
        {
            tracing::error!(change_id = %row.id, instance_id, %error, "queued config write failed");
            mark_change(pool, &row.id, "failed", Some("config_write_failed")).await?;
            blocked = true;
            continue;
        }
        let written = read_current(root, &row.relative_path)
            .await?
            .ok_or_else(|| AppError::Internal("queued config disappeared after write".into()))?;
        if written.sha256 != row.content_sha256 {
            mark_change(
                pool,
                &row.id,
                "failed",
                Some("config_write_verification_failed"),
            )
            .await?;
            blocked = true;
            continue;
        }
        mark_change(pool, &row.id, "applied", None).await?;
        database::audit(
            pool,
            Some(&row.queued_by),
            "server.config_applied",
            "instance",
            Some(instance_id),
            "success",
            serde_json::json!({
                "change_id": row.id,
                "path": row.relative_path,
                "content_sha256": row.content_sha256,
            }),
        )
        .await?;
        events.publish(
            "config.applied",
            Some(instance_id.to_string()),
            serde_json::json!({"change_id": row.id, "path": row.relative_path}),
        );
        applied = applied.saturating_add(1);
    }
    if blocked {
        Err(AppError::Conflict(
            "config_files.pending_changes_blocked".into(),
        ))
    } else {
        Ok(applied)
    }
}

async fn specs_for_instance(
    pool: &DbPool,
    root: &Path,
    instance_id: &str,
) -> Result<Vec<ConfigFileSpec>, AppError> {
    let instance: InstanceConfiguration =
        sqlx::query_as("SELECT profile_id, settings FROM instances WHERE id = ?")
            .bind(instance_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    let settings: Value = serde_json::from_str(&instance.settings)
        .map_err(|_| AppError::Internal("stored instance settings are invalid".into()))?;
    let mut specs = static_specs(&instance.profile_id, &settings);
    discover_specs(root, &instance.profile_id, &mut specs).await?;
    let mut unique = BTreeMap::new();
    for spec in specs {
        unique.entry(spec.relative_path.clone()).or_insert(spec);
    }
    Ok(unique.into_values().collect())
}

async fn resolve_spec(
    pool: &DbPool,
    root: &Path,
    instance_id: &str,
    path: &str,
) -> Result<ConfigFileSpec, AppError> {
    specs_for_instance(pool, root, instance_id)
        .await?
        .into_iter()
        .find(|spec| spec.relative_path == path)
        .ok_or_else(|| AppError::Forbidden("config_files.path_not_allowed".into()))
}

fn static_specs(profile_id: &str, settings: &Value) -> Vec<ConfigFileSpec> {
    let mut specs = Vec::new();
    let mut add = |path: &str, category: ConfigCategory, format: ConfigFormat| {
        specs.push(ConfigFileSpec {
            relative_path: path.to_string(),
            category,
            format,
        });
    };
    match profile_id {
        "hytale" => {
            add(
                "game/Server/config.json",
                ConfigCategory::Configuration,
                ConfigFormat::Json,
            );
            for path in [
                "game/Server/permissions.json",
                "game/Server/whitelist.json",
                "game/Server/bans.json",
            ] {
                add(path, ConfigCategory::Access, ConfigFormat::Json);
            }
        }
        id if id == "minecraft-java" || id.starts_with("minecraft-java-") => {
            for (path, format) in [
                ("game/server.properties", ConfigFormat::Properties),
                ("game/bukkit.yml", ConfigFormat::Yaml),
                ("game/spigot.yml", ConfigFormat::Yaml),
                ("game/paper.yml", ConfigFormat::Yaml),
                ("game/purpur.yml", ConfigFormat::Yaml),
                ("game/config/paper-global.yml", ConfigFormat::Yaml),
                ("game/config/paper-world-defaults.yml", ConfigFormat::Yaml),
            ] {
                add(path, ConfigCategory::Configuration, format);
            }
            for path in [
                "game/ops.json",
                "game/whitelist.json",
                "game/banned-players.json",
                "game/banned-ips.json",
            ] {
                add(path, ConfigCategory::Access, ConfigFormat::Json);
            }
        }
        "minecraft-bedrock" => {
            add(
                "game/server.properties",
                ConfigCategory::Configuration,
                ConfigFormat::Properties,
            );
            add(
                "game/permissions.json",
                ConfigCategory::Access,
                ConfigFormat::Json,
            );
            add(
                "game/allowlist.json",
                ConfigCategory::Access,
                ConfigFormat::Json,
            );
        }
        "valheim" => {
            for path in [
                "data/adminlist.txt",
                "data/permittedlist.txt",
                "data/bannedlist.txt",
            ] {
                add(path, ConfigCategory::Access, ConfigFormat::Text);
            }
        }
        "palworld" => {
            let platform = if cfg!(windows) {
                "WindowsServer"
            } else {
                "LinuxServer"
            };
            for file in ["PalWorldSettings.ini", "GameUserSettings.ini", "Engine.ini"] {
                add(
                    &format!("game/Pal/Saved/Config/{platform}/{file}"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Ini,
                );
            }
        }
        "satisfactory" => {
            let platform = if cfg!(windows) {
                "WindowsServer"
            } else {
                "LinuxServer"
            };
            for file in [
                "Game.ini",
                "GameUserSettings.ini",
                "Engine.ini",
                "Scalability.ini",
            ] {
                add(
                    &format!("game/FactoryGame/Saved/Config/{platform}/{file}"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Ini,
                );
            }
        }
        "seven-days-to-die" => {
            add(
                "game/dmx-serverconfig.xml",
                ConfigCategory::Configuration,
                ConfigFormat::Xml,
            );
            add(
                "data/7days-to-die/Saves/serveradmin.xml",
                ConfigCategory::Access,
                ConfigFormat::Xml,
            );
        }
        "project-zomboid" => {
            if let Some(name) = safe_setting_component(settings, "server_name") {
                add(
                    &format!("data/Zomboid/Server/{name}.ini"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Ini,
                );
                add(
                    &format!("data/Zomboid/Server/{name}_SandboxVars.lua"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Lua,
                );
                add(
                    &format!("data/Zomboid/Server/{name}_spawnregions.lua"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Lua,
                );
            }
        }
        "rust" => {
            if let Some(identity) = safe_setting_component(settings, "identity") {
                add(
                    &format!("game/server/{identity}/cfg/server.cfg"),
                    ConfigCategory::Configuration,
                    ConfigFormat::Properties,
                );
                add(
                    &format!("game/server/{identity}/cfg/users.cfg"),
                    ConfigCategory::Access,
                    ConfigFormat::Properties,
                );
                add(
                    &format!("game/server/{identity}/cfg/bans.cfg"),
                    ConfigCategory::Access,
                    ConfigFormat::Properties,
                );
            }
        }
        _ => {}
    }
    specs
}

fn safe_setting_component<'a>(settings: &'a Value, key: &str) -> Option<&'a str> {
    settings.get(key).and_then(Value::as_str).filter(|value| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

async fn discover_specs(
    root: &Path,
    profile_id: &str,
    specs: &mut Vec<ConfigFileSpec>,
) -> Result<(), AppError> {
    let roots: &[&str] = if profile_id == "hytale" {
        &["game/Server/universe/worlds"]
    } else if profile_id == "minecraft-java" || profile_id.starts_with("minecraft-java-") {
        &["game/config", "game/world"]
    } else if profile_id == "rust" {
        &["game/oxide/config"]
    } else {
        &[]
    };
    for relative in roots {
        scan_tree(root, relative, specs).await?;
    }
    Ok(())
}

async fn scan_tree(
    root: &Path,
    relative_root: &str,
    specs: &mut Vec<ConfigFileSpec>,
) -> Result<(), AppError> {
    let start = root.join(relative_root);
    let metadata = match tokio::fs::symlink_metadata(&start).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata_is_link_like(&metadata) {
        return Ok(());
    }
    let mut pending = vec![(start, 0_usize)];
    let mut discovered = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DISCOVERY_DEPTH || discovered >= MAX_DISCOVERED_FILES {
            continue;
        }
        let mut entries = tokio::fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            if discovered >= MAX_DISCOVERED_FILES {
                break;
            }
            let entry_path = entry.path();
            let metadata = tokio::fs::symlink_metadata(&entry_path).await?;
            if metadata_is_link_like(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push((entry_path, depth.saturating_add(1)));
                continue;
            }
            if !metadata.is_file() || metadata.len() > MAX_CONFIG_TEXT_BYTES as u64 {
                continue;
            }
            let relative = entry_path
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .map(|path| path.replace('\\', "/"));
            let Some(relative) = relative else {
                continue;
            };
            let Some(format) = ConfigFormat::from_path(&relative) else {
                continue;
            };
            if is_sensitive_dynamic_path(&relative) {
                continue;
            }
            specs.push(ConfigFileSpec {
                relative_path: relative,
                category: ConfigCategory::Configuration,
                format,
            });
            discovered = discovered.saturating_add(1);
        }
    }
    Ok(())
}

fn is_sensitive_dynamic_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with("auth.enc")
        || lower.ends_with("auth.key")
        || lower.contains("credential")
        || lower.contains("token")
        || lower.ends_with(".key")
        || lower.ends_with(".pem")
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

struct CurrentFile {
    content: String,
    size_bytes: u64,
    modified_at: Option<String>,
    sha256: String,
}

async fn read_current(root: &Path, path: &str) -> Result<Option<CurrentFile>, AppError> {
    let (file, size) = match secure_fs::open_declared_regular_file(root, path).await {
        Ok(file) => file,
        Err(AppError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if size > MAX_CONFIG_TEXT_BYTES as u64 {
        return Err(AppError::BadRequest("config_files.text_too_large".into()));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take((MAX_CONFIG_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_CONFIG_TEXT_BYTES || bytes.contains(&0) {
        return Err(AppError::BadRequest("config_files.not_text".into()));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| AppError::BadRequest("config_files.not_utf8".into()))?;
    let modified_at = tokio::fs::metadata(root.join(path))
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(|time| DateTime::<Utc>::from(time).to_rfc3339());
    Ok(Some(CurrentFile {
        sha256: sha256(content.as_bytes()),
        content,
        size_bytes: size,
        modified_at,
    }))
}

fn summary(
    spec: &ConfigFileSpec,
    current: Option<&CurrentFile>,
    queued_change: Option<ConfigChangeSummary>,
) -> ConfigFileSummary {
    ConfigFileSummary {
        path: spec.relative_path.clone(),
        category: spec.category,
        format: spec.format,
        exists: current.is_some(),
        size_bytes: current.map_or(0, |current| current.size_bytes),
        modified_at: current.and_then(|current| current.modified_at.clone()),
        sha256: current.map(|current| current.sha256.clone()),
        queued_change,
    }
}

fn validate_content(format: ConfigFormat, content: &str) -> Result<(), AppError> {
    if content.len() > MAX_CONFIG_TEXT_BYTES
        || content.contains('\0')
        || content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(AppError::BadRequest("config_files.invalid_text".into()));
    }
    match format {
        ConfigFormat::Json => {
            serde_json::from_str::<Value>(content)
                .map_err(|_| AppError::BadRequest("config_files.invalid_json".into()))?;
        }
        ConfigFormat::Toml => {
            toml::from_str::<toml::Value>(content)
                .map_err(|_| AppError::BadRequest("config_files.invalid_toml".into()))?;
        }
        ConfigFormat::Xml if !content.trim().is_empty() => {
            let trimmed = content.trim();
            if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
                return Err(AppError::BadRequest("config_files.invalid_xml".into()));
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_access_list(content: &str) -> Result<(), AppError> {
    const MAX_ACCESS_ENTRIES: usize = 5_000;
    const MAX_ACCESS_LINE_CHARS: usize = 128;
    if content.lines().count() > MAX_ACCESS_ENTRIES
        || content
            .lines()
            .any(|line| line.chars().count() > MAX_ACCESS_LINE_CHARS)
    {
        return Err(AppError::BadRequest(
            "config_files.invalid_access_list".into(),
        ));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn change_associated_data(id: &str, instance_id: &str, path: &str) -> String {
    format!("config-change:{id}:{instance_id}:{path}")
}

fn decrypt_change(secrets: &SecretStore, change: &ConfigChangeRow) -> Result<String, AppError> {
    secrets.open_payload(
        &change_associated_data(&change.id, &change.instance_id, &change.relative_path),
        &change.content_nonce,
        &change.content_ciphertext,
        MAX_CONFIG_TEXT_BYTES,
    )
}

fn change_summary(change: &ConfigChangeRow) -> ConfigChangeSummary {
    ConfigChangeSummary {
        id: change.id.clone(),
        status: change.status.clone(),
        content_sha256: change.content_sha256.clone(),
        error_code: change.error_code.clone(),
        queued_at: change.created_at.clone(),
    }
}

async fn latest_changes(
    pool: &DbPool,
    instance_id: &str,
) -> Result<BTreeMap<String, ConfigChangeRow>, AppError> {
    let rows: Vec<ConfigChangeRow> = sqlx::query_as(
        r#"
        SELECT id, instance_id, relative_path, format, category, base_sha256, content_sha256,
               content_nonce, content_ciphertext, status, error_code, queued_by, created_at
        FROM config_changes
        WHERE instance_id = ? AND status IN ('pending', 'conflict', 'failed')
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(instance_id)
    .fetch_all(pool)
    .await?;
    let mut changes = BTreeMap::new();
    for row in rows {
        changes.entry(row.relative_path.clone()).or_insert(row);
    }
    Ok(changes)
}

async fn latest_change(
    pool: &DbPool,
    instance_id: &str,
    path: &str,
) -> Result<Option<ConfigChangeRow>, AppError> {
    sqlx::query_as(
        r#"
        SELECT id, instance_id, relative_path, format, category, base_sha256, content_sha256,
               content_nonce, content_ciphertext, status, error_code, queued_by, created_at
        FROM config_changes
        WHERE instance_id = ? AND relative_path = ? AND status IN ('pending', 'conflict', 'failed')
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(instance_id)
    .bind(path)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

async fn mark_change(
    pool: &DbPool,
    id: &str,
    status: &str,
    error_code: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE config_changes SET status = ?, error_code = ?, updated_at = ?, applied_at = \
         CASE WHEN ? = 'applied' THEN ? ELSE applied_at END WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(error_code)
    .bind(&now)
    .bind(status)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn config_fixture() -> (
        tempfile::TempDir,
        DbPool,
        SecretStore,
        EventHub,
        String,
        String,
    ) {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/config.db?mode=rwc", root.path().display());
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let profiles = crate::services::profiles::ProfileRegistry::builtins();
        profiles.persist_builtins(&pool).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'config-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, installation_state, \
              desired_state, runtime_state, created_at, updated_at) \
             VALUES (?, 'config-fixture', 'hytale', 2, '{}', 'installed', 'stopped', 'stopped', ?, ?)",
        )
        .bind(&instance_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        tokio::fs::create_dir_all(root.path().join("game/Server"))
            .await
            .unwrap();
        let secrets = SecretStore::load_or_create(&root.path().join("master.key")).unwrap();
        (root, pool, secrets, EventHub::new(16), instance_id, user_id)
    }

    #[test]
    fn builtins_expose_native_configuration_and_access_files() {
        let hytale = static_specs("hytale", &serde_json::json!({}));
        assert!(
            hytale
                .iter()
                .any(|file| file.relative_path == "game/Server/config.json")
        );
        assert!(hytale.iter().any(|file| {
            file.relative_path == "game/Server/permissions.json"
                && file.category == ConfigCategory::Access
        }));

        let rust = static_specs("rust", &serde_json::json!({"identity": "dmxserver"}));
        assert!(
            rust.iter()
                .any(|file| file.relative_path.ends_with("/cfg/users.cfg"))
        );
    }

    #[test]
    fn structured_formats_are_validated_before_queueing() {
        assert!(validate_content(ConfigFormat::Json, "{\"ok\":true}").is_ok());
        assert!(validate_content(ConfigFormat::Json, "{broken").is_err());
        assert!(validate_content(ConfigFormat::Toml, "port = 5520").is_ok());
        assert!(validate_content(ConfigFormat::Toml, "port = [").is_err());
        assert!(validate_content(ConfigFormat::Text, "admin\n123").is_ok());
        assert!(validate_content(ConfigFormat::Text, "bad\0value").is_err());
        assert!(validate_access_list("# preserved\n76561198000000000").is_ok());
        assert!(validate_access_list(&"x".repeat(129)).is_err());
    }

    #[test]
    fn dynamic_discovery_rejects_secret_like_files() {
        assert!(is_sensitive_dynamic_path("game/config/token.json"));
        assert!(is_sensitive_dynamic_path("game/config/private.key"));
        assert!(!is_sensitive_dynamic_path("game/config/paper-global.yml"));
    }

    #[tokio::test]
    async fn queued_content_is_encrypted_and_applied_atomically() {
        let (root, pool, secrets, events, instance_id, user_id) = config_fixture().await;
        let path = "game/Server/config.json";
        tokio::fs::write(root.path().join(path), br#"{"Name":"before"}"#)
            .await
            .unwrap();
        let initial = read(&pool, &secrets, root.path(), &instance_id, path)
            .await
            .unwrap();
        let content = r#"{"Name":"after"}"#;
        let queued = queue(
            &pool,
            &secrets,
            &events,
            root.path(),
            &instance_id,
            QueueConfigChange {
                path,
                content,
                expected_sha256: initial.file.sha256.as_deref(),
                queued_by: &user_id,
            },
        )
        .await
        .unwrap();
        assert_eq!(queued.queued_content.as_deref(), Some(content));
        assert_eq!(
            tokio::fs::read_to_string(root.path().join(path))
                .await
                .unwrap(),
            r#"{"Name":"before"}"#
        );
        let ciphertext: String = sqlx::query_scalar(
            "SELECT content_ciphertext FROM config_changes WHERE instance_id = ?",
        )
        .bind(&instance_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!ciphertext.contains("after"));

        assert_eq!(
            apply_pending(&pool, &secrets, &events, root.path(), &instance_id)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            tokio::fs::read_to_string(root.path().join(path))
                .await
                .unwrap(),
            content
        );
    }

    #[tokio::test]
    async fn changed_live_file_blocks_a_queued_overwrite() {
        let (root, pool, secrets, events, instance_id, user_id) = config_fixture().await;
        let path = "game/Server/config.json";
        tokio::fs::write(root.path().join(path), br#"{"Name":"before"}"#)
            .await
            .unwrap();
        let initial = read(&pool, &secrets, root.path(), &instance_id, path)
            .await
            .unwrap();
        queue(
            &pool,
            &secrets,
            &events,
            root.path(),
            &instance_id,
            QueueConfigChange {
                path,
                content: r#"{"Name":"queued"}"#,
                expected_sha256: initial.file.sha256.as_deref(),
                queued_by: &user_id,
            },
        )
        .await
        .unwrap();
        tokio::fs::write(root.path().join(path), br#"{"Name":"game-change"}"#)
            .await
            .unwrap();

        assert!(
            apply_pending(&pool, &secrets, &events, root.path(), &instance_id)
                .await
                .is_err()
        );
        let status: String =
            sqlx::query_scalar("SELECT status FROM config_changes WHERE instance_id = ?")
                .bind(&instance_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "conflict");
        assert_eq!(
            tokio::fs::read_to_string(root.path().join(path))
                .await
                .unwrap(),
            r#"{"Name":"game-change"}"#
        );
    }
}
