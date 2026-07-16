use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use regex::Regex;
use reqwest::Url;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::v1::StopStrategy;

use super::{
    InstallContext, InstalledArtifact, InstallerError, InstallerExecutable, InstallerPlan,
    download_verified, extract_zip,
};

const MAX_DOWNLOADER_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CREDENTIAL_BYTES: u64 = 16 * 1024;

pub const DOWNLOADER_CREDENTIAL_SECRET: &str = "hytale_downloader_credentials";
const HYTALE_DEVICE_VERIFICATION_URI: &str = "https://accounts.hytale.com/device";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAuthorization {
    pub verification_uri: String,
    pub user_code: Option<String>,
}

/// A command contract for the official downloader. The caller must execute it
/// through the normal contained-process supervisor. `credential_file` belongs
/// to an ephemeral 0700 directory: its contents must be moved to SecretStore
/// and the plaintext removed before the job completes.
#[derive(Debug, Clone)]
pub struct HytaleDownloaderPlan {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
    pub output_archive: PathBuf,
    pub credential_file: PathBuf,
    pub downloader_artifact: InstalledArtifact,
}

impl HytaleDownloaderPlan {
    pub fn version_args(&self) -> Vec<OsString> {
        vec![
            OsString::from("-print-version"),
            OsString::from("-skip-update-check"),
        ]
    }
}

pub async fn prepare_hytale_downloader(
    work_root: &Path,
    context: &InstallContext,
) -> Result<HytaleDownloaderPlan, InstallerError> {
    let archive = work_root.join(format!(
        ".hytale-downloader-{}.zip",
        uuid::Uuid::new_v4().as_simple()
    ));
    // Hypixel currently publishes no detached digest for this bootstrap archive.
    // Authenticity therefore relies on HTTPS plus the exact downloader.hytale.com
    // allowlist. The computed SHA-256 is evidence for audit/diagnostics only; it is
    // deliberately not described as a provider-verified checksum.
    let downloaded = download_verified(
        context,
        &context.sources.hytale_downloader,
        &archive,
        MAX_DOWNLOADER_ARCHIVE_BYTES,
        None,
        None,
    )
    .await?;
    let executable_name = if cfg!(windows) {
        "hytale-downloader-windows-amd64.exe"
    } else {
        "hytale-downloader-linux-amd64"
    };
    let tool_directory = work_root.join("hytale-downloader");
    let selected = [executable_name.to_string()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let extraction = extract_zip(
        &archive,
        &tool_directory,
        context.archive_limits,
        Some(&selected),
    )
    .await;
    let _ = tokio::fs::remove_file(&archive).await;
    extraction?;

    let executable = tool_directory.join(executable_name);
    let metadata = tokio::fs::symlink_metadata(&executable)
        .await
        .map_err(|error| InstallerError::internal("hytale_downloader_missing", error))?;
    if !metadata.is_file() || metadata_is_link_like(&metadata) {
        return Err(InstallerError::new(
            "hytale_downloader_invalid",
            "servers.provider_response_invalid",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| InstallerError::internal("hytale_downloader_permissions", error))?;
    }

    let credential_directory = work_root.join("hytale-credentials");
    create_private_directory(&credential_directory).await?;
    let output_archive = work_root.join("hytale-game.zip");
    Ok(HytaleDownloaderPlan {
        executable,
        cwd: credential_directory.clone(),
        args: vec![
            OsString::from("-download-path"),
            output_archive.as_os_str().to_os_string(),
            OsString::from("-skip-update-check"),
        ],
        output_archive,
        credential_file: credential_directory.join(".hytale-downloader-credentials.json"),
        downloader_artifact: InstalledArtifact {
            name: "hytale-downloader".to_string(),
            sha256: downloaded.sha256,
            size: downloaded.size,
        },
    })
}

pub async fn extract_hytale_game_archive(
    archive: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<(), InstallerError> {
    extract_zip(archive, staging, context.archive_limits, None).await?;
    validate_game_layout(staging).await
}

pub async fn validate_game_layout(staging: &Path) -> Result<(), InstallerError> {
    for directory in [staging.to_path_buf(), staging.join("Server")] {
        let metadata = tokio::fs::symlink_metadata(&directory)
            .await
            .map_err(|error| InstallerError::internal("hytale_layout_invalid", error))?;
        if !metadata.is_dir() || metadata_is_link_like(&metadata) {
            return Err(InstallerError::new(
                "hytale_layout_invalid",
                "servers.hytale_archive_invalid",
            ));
        }
    }
    for required in [
        "Assets.zip",
        "Server/HytaleServer.jar",
        "Server/HytaleServer.aot",
    ] {
        let path = staging.join(required);
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| InstallerError::internal("hytale_layout_invalid", error))?;
        if !metadata.is_file() || metadata_is_link_like(&metadata) {
            return Err(InstallerError::new(
                "hytale_layout_invalid",
                "servers.hytale_archive_invalid",
            ));
        }
    }
    Ok(())
}

/// Builds a complete update candidate from the server-owned staging tree.
///
/// Hytale writes its update below `game/updater/staging` and exits with code 8.
/// That directory must never be executed in place: it is copied without
/// following links, validated as a complete game layout, then the mutable
/// instance data is overlaid from the currently installed game. The runtime
/// can consequently switch whole directories with rename-based rollback.
pub async fn prepare_runtime_update(
    current_game: &Path,
    provider_staging: &Path,
    candidate: &Path,
) -> Result<(), InstallerError> {
    validate_game_layout(provider_staging).await?;
    remove_path_if_exists(candidate).await?;
    copy_tree_without_links(provider_staging, candidate).await?;
    preserve_server_data(current_game, candidate).await?;
    validate_game_layout(candidate).await
}

/// Copies only Hytale's mutable, instance-owned files. This is also used just
/// before rolling an update back so world/config changes made during the
/// probation window are not lost.
pub async fn preserve_runtime_data(
    current_game: &Path,
    destination_game: &Path,
) -> Result<(), InstallerError> {
    preserve_server_data(current_game, destination_game).await
}

pub async fn install_downloaded_archive(
    settings: &Value,
    instance_root: &Path,
    archive: &Path,
    staging: &Path,
    context: &InstallContext,
    installed_version: String,
    downloader_artifact: InstalledArtifact,
) -> Result<super::InstallResult, InstallerError> {
    extract_hytale_game_archive(archive, staging, context).await?;
    preserve_server_data(&instance_root.join("game"), staging).await?;
    let archive_artifact = inspect_artifact("hytale-game-archive", archive).await?;
    Ok(super::InstallResult {
        plan: launch_plan(settings)?,
        installed_version,
        installed_build: None,
        artifacts: vec![downloader_artifact, archive_artifact],
    })
}

pub fn parse_printed_version(output: &str) -> Option<String> {
    let output = strip_terminal_controls(output);
    for line in output.lines().rev() {
        let line = line.trim();
        let candidate = line
            .split_once(':')
            .filter(|(prefix, _)| {
                let prefix = prefix.trim().to_ascii_lowercase();
                matches!(
                    prefix.as_str(),
                    "version" | "game version" | "available version" | "release"
                )
            })
            .map_or(line, |(_, value)| value.trim());
        if valid_version(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

pub fn credential_redactions(document: &str) -> Result<Vec<String>, InstallerError> {
    let value = validate_credentials(document.as_bytes())?;
    let mut values = Vec::new();
    collect_secret_strings(&value, &mut values);
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    values.truncate(64);
    Ok(values)
}

pub async fn write_plaintext_credentials(
    path: &Path,
    document: &str,
) -> Result<(), InstallerError> {
    validate_credentials(document.as_bytes())?;
    let path = path.to_path_buf();
    let document = document.as_bytes().to_vec();
    tokio::task::spawn_blocking(move || {
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            }
            let mut file = options
                .open(&path)
                .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?;
            file.write_all(&document)
                .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?;
            file.sync_all()
                .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result
    })
    .await
    .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?
}

pub async fn read_plaintext_credentials(path: &Path) -> Result<String, InstallerError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| InstallerError::internal("hytale_credentials_missing", error))?;
    if !metadata.is_file()
        || metadata_is_link_like(&metadata)
        || metadata.len() > MAX_CREDENTIAL_BYTES
    {
        return Err(InstallerError::new(
            "hytale_credentials_invalid",
            "servers.hytale_credentials_invalid",
        ));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?;
    validate_credentials(&bytes)?;
    String::from_utf8(bytes)
        .map_err(|error| InstallerError::internal("hytale_credentials_invalid", error))
}

/// Best-effort overwrite followed by unlink. This removes the usable plaintext
/// file, but does not claim physical erasure on copy-on-write or flash storage.
pub async fn remove_plaintext_credentials(path: &Path) -> Result<(), InstallerError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(InstallerError::internal(
                    "hytale_credentials_cleanup_failed",
                    error,
                ));
            }
        };
        if !metadata.is_file() || metadata_is_link_like(&metadata) {
            return Err(InstallerError::new(
                "hytale_credentials_invalid",
                "servers.hytale_credentials_invalid",
            ));
        }
        let mut options = OpenOptions::new();
        options.write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options.open(&path).map_err(|error| {
            InstallerError::internal("hytale_credentials_cleanup_failed", error)
        })?;
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            InstallerError::internal("hytale_credentials_cleanup_failed", error)
        })?;
        let mut remaining = metadata.len();
        let zeros = [0_u8; 4096];
        while remaining > 0 {
            let count = usize::try_from(remaining.min(zeros.len() as u64)).expect("bounded");
            file.write_all(&zeros[..count]).map_err(|error| {
                InstallerError::internal("hytale_credentials_cleanup_failed", error)
            })?;
            remaining -= count as u64;
        }
        file.sync_all().map_err(|error| {
            InstallerError::internal("hytale_credentials_cleanup_failed", error)
        })?;
        drop(file);
        std::fs::remove_file(path)
            .map_err(|error| InstallerError::internal("hytale_credentials_cleanup_failed", error))
    })
    .await
    .map_err(|error| InstallerError::internal("hytale_credentials_cleanup_failed", error))?
}

pub fn launch_plan(settings: &Value) -> Result<InstallerPlan, InstallerError> {
    let port = settings.get("port").and_then(Value::as_u64).unwrap_or(5520);
    if !(1..=65_535).contains(&port) {
        return Err(InstallerError::new(
            "hytale_port_invalid",
            "servers.settings_invalid",
        ));
    }
    let memory = settings
        .get("max_memory_mb")
        .and_then(Value::as_u64)
        .unwrap_or(8192);
    if !(1024..=131_072).contains(&memory) {
        return Err(InstallerError::new(
            "hytale_memory_invalid",
            "servers.settings_invalid",
        ));
    }
    let auth_mode = settings
        .get("auth_mode")
        .and_then(Value::as_str)
        .unwrap_or("authenticated");
    if !matches!(auth_mode, "authenticated" | "offline") {
        return Err(InstallerError::new(
            "hytale_auth_mode_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(InstallerPlan {
        executable: InstallerExecutable::ManagedJava { major: 25 },
        cwd_relative: "Server".to_string(),
        args: vec![
            format!("-Xmx{memory}M"),
            "-XX:AOTCache=HytaleServer.aot".to_string(),
            "-jar".to_string(),
            "HytaleServer.jar".to_string(),
            "--assets".to_string(),
            "../Assets.zip".to_string(),
            "--bind".to_string(),
            format!("0.0.0.0:{port}"),
            "--auth-mode".to_string(),
            auth_mode.to_string(),
        ],
        env: Vec::new(),
        stop: StopStrategy::Stdin {
            command: "stop".to_string(),
            timeout_seconds: 30,
        },
        restart_exit_codes: vec![8],
    })
}

pub fn detect_device_authorization(output: &str) -> Option<DeviceAuthorization> {
    let sanitized = strip_terminal_controls(output);
    let url_pattern = Regex::new(
        r"https://(?:accounts\.hytale\.com/device|oauth\.accounts\.hytale\.com/oauth2/device/verify)[^\s\x00-\x1f]*",
    )
    .ok()?;
    let url = url_pattern
        .find_iter(&sanitized)
        .last()
        .and_then(|matched| {
            let matched = matched
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '"', '\'']);
            let url = Url::parse(matched).ok()?;
            let official_device_page =
                url.host_str() == Some("accounts.hytale.com") && url.path() == "/device";
            let downloader_verification_page = url.host_str() == Some("oauth.accounts.hytale.com")
                && url.path() == "/oauth2/device/verify";
            (url.scheme() == "https"
                && (official_device_page || downloader_verification_page)
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none())
            .then_some(url)
        });
    let query_code = url
        .as_ref()
        .and_then(|url| {
            url.query_pairs()
                .find_map(|(key, value)| (key == "user_code").then(|| value.into_owned()))
        })
        .filter(|value| valid_user_code(value));
    let text_code = Regex::new(
        r"(?i)(?:authorization|user[_ ]?|device|verification)\s*code\s*[:=]\s*([A-Z0-9-]{4,32})",
    )
    .ok()?
    .captures_iter(&sanitized)
    .last()
    .and_then(|captures| captures.get(1))
    .map(|value| value.as_str().to_ascii_uppercase())
    .filter(|value| valid_user_code(value));
    let standalone_code = url.as_ref().and_then(|_| {
        Regex::new(r"(?m)^\s*([A-Z0-9-]{6,16})\s*$")
            .ok()?
            .captures_iter(&sanitized)
            .last()
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_ascii_uppercase())
            .filter(|value| valid_user_code(value))
    });
    if url.is_none() && text_code.is_none() {
        return None;
    }
    Some(DeviceAuthorization {
        // The downloader currently prints an internal OAuth verification route
        // on some releases. Hypixel's public device-flow contract documents this
        // stable page and optional user_code query instead.
        verification_uri: HYTALE_DEVICE_VERIFICATION_URI.to_string(),
        user_code: query_code.or(text_code).or(standalone_code),
    })
}

pub fn merge_json_preserving_unknown(
    existing: &[u8],
    updates: &serde_json::Map<String, Value>,
) -> Result<Vec<u8>, InstallerError> {
    let mut value: Value = if existing.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(existing)
            .map_err(|error| InstallerError::internal("hytale_config_invalid", error))?
    };
    let object = value.as_object_mut().ok_or_else(|| {
        InstallerError::new("hytale_config_invalid", "servers.configuration_invalid")
    })?;
    for (key, value) in updates {
        object.insert(key.clone(), value.clone());
    }
    serde_json::to_vec_pretty(&value)
        .map_err(|error| InstallerError::internal("hytale_config_invalid", error))
}

async fn preserve_server_data(current_game: &Path, staging: &Path) -> Result<(), InstallerError> {
    if !tokio::fs::try_exists(current_game)
        .await
        .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?
    {
        return Ok(());
    }
    for root in [current_game, staging] {
        let metadata = tokio::fs::symlink_metadata(root)
            .await
            .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
        if !metadata.is_dir() || metadata_is_link_like(&metadata) {
            return Err(InstallerError::new(
                "hytale_preserve_unsafe",
                "servers.unsafe_save_path",
            ));
        }
    }
    for relative in [
        ".dmx-install.json",
        "Server/universe",
        "Server/mods",
        "Server/backups",
        "Server/config.json",
        "Server/permissions.json",
        "Server/bans.json",
        "Server/whitelist.json",
        "Server/auth.enc",
        "Server/auth.key",
    ] {
        let source = current_game.join(relative);
        let metadata = match tokio::fs::symlink_metadata(&source).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(InstallerError::internal("hytale_preserve_failed", error)),
        };
        if metadata_is_link_like(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(InstallerError::new(
                "hytale_preserve_unsafe",
                "servers.unsafe_save_path",
            ));
        }
        let destination = staging.join(relative);
        remove_path_if_exists(&destination).await?;
        if metadata.is_dir() {
            copy_tree_without_links(&source, &destination).await?;
        } else {
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
            }
            tokio::fs::copy(source, destination)
                .await
                .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
        }
    }
    Ok(())
}

async fn copy_tree_without_links(source: &Path, destination: &Path) -> Result<(), InstallerError> {
    let source_metadata = tokio::fs::symlink_metadata(source)
        .await
        .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
    if !source_metadata.is_dir() || metadata_is_link_like(&source_metadata) {
        return Err(InstallerError::new(
            "hytale_preserve_unsafe",
            "servers.unsafe_save_path",
        ));
    }
    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_dir, destination_dir)) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&source_dir)
            .await
            .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?
        {
            let metadata = tokio::fs::symlink_metadata(entry.path())
                .await
                .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
            if metadata_is_link_like(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
                return Err(InstallerError::new(
                    "hytale_preserve_unsafe",
                    "servers.unsafe_save_path",
                ));
            }
            let next_destination = destination_dir.join(entry.file_name());
            if metadata.is_dir() {
                tokio::fs::create_dir(&next_destination)
                    .await
                    .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
                pending.push((entry.path(), next_destination));
            } else {
                tokio::fs::copy(entry.path(), next_destination)
                    .await
                    .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
            }
        }
    }
    Ok(())
}

async fn remove_path_if_exists(path: &Path) -> Result<(), InstallerError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata_is_link_like(&metadata) => {
            return Err(InstallerError::new(
                "hytale_preserve_unsafe",
                "servers.unsafe_save_path",
            ));
        }
        Ok(metadata) if metadata.is_file() => {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
        }
        Ok(metadata) if metadata.is_dir() => {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|error| InstallerError::internal("hytale_preserve_failed", error))?;
        }
        Ok(_) => {
            return Err(InstallerError::new(
                "hytale_preserve_unsafe",
                "servers.unsafe_save_path",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(InstallerError::internal("hytale_preserve_failed", error)),
    }
    Ok(())
}

async fn inspect_artifact(name: &str, path: &Path) -> Result<InstalledArtifact, InstallerError> {
    let name = name.to_string();
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path)
            .map_err(|error| InstallerError::internal("hytale_archive_read_failed", error))?;
        let size = file
            .metadata()
            .map_err(|error| InstallerError::internal("hytale_archive_read_failed", error))?
            .len();
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| InstallerError::internal("hytale_archive_read_failed", error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(InstalledArtifact {
            name,
            sha256: format!("{:x}", hasher.finalize()),
            size,
        })
    })
    .await
    .map_err(|error| InstallerError::internal("hytale_archive_read_failed", error))?
}

fn validate_credentials(bytes: &[u8]) -> Result<Value, InstallerError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_CREDENTIAL_BYTES || bytes.contains(&0) {
        return Err(InstallerError::new(
            "hytale_credentials_invalid",
            "servers.hytale_credentials_invalid",
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| InstallerError::internal("hytale_credentials_invalid", error))?;
    if !value.is_object() {
        return Err(InstallerError::new(
            "hytale_credentials_invalid",
            "servers.hytale_credentials_invalid",
        ));
    }
    Ok(value)
}

fn collect_secret_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) if value.len() >= 8 => output.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect_secret_strings(value, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_secret_strings(value, output);
            }
        }
        _ => {}
    }
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

async fn create_private_directory(path: &Path) -> Result<(), InstallerError> {
    tokio::fs::create_dir(path)
        .await
        .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| InstallerError::internal("hytale_credentials_failed", error))?;
    }
    Ok(())
}

fn valid_user_code(value: &str) -> bool {
    (4..=32).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn strip_terminal_controls(value: &str) -> String {
    let mut result = String::with_capacity(value.len().min(64 * 1024));
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if result.len() >= 64 * 1024 {
            break;
        }
        if character == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if !character.is_control() || matches!(character, '\n' | '\r' | '\t') {
            result.push(character);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn device_authorization_only_accepts_the_official_origin() {
        let detected = detect_device_authorization(
            "Visit https://accounts.hytale.com/device?user_code=ABCD-1234\nWaiting...",
        )
        .unwrap();
        assert_eq!(detected.user_code.as_deref(), Some("ABCD-1234"));
        assert!(
            detect_device_authorization("https://accounts.hytale.com.evil/device?user_code=X")
                .is_none()
        );
        assert!(
            detect_device_authorization("http://accounts.hytale.com/device?user_code=X").is_none()
        );
    }

    #[test]
    fn device_authorization_accepts_the_official_downloader_code_line() {
        let detected = detect_device_authorization("Authorization code: ABCD-1234").unwrap();
        assert_eq!(
            detected.verification_uri,
            "https://accounts.hytale.com/device"
        );
        assert_eq!(detected.user_code.as_deref(), Some("ABCD-1234"));
        assert!(detect_device_authorization("Process exit code: 8").is_none());
    }

    #[test]
    fn downloader_internal_verification_route_is_canonicalized() {
        let detected = detect_device_authorization(
            "Please visit the following URL to authenticate:\n\
             https://oauth.accounts.hytale.com/oauth2/device/verify\n\
             ABCD1234\n",
        )
        .unwrap();
        assert_eq!(detected.verification_uri, HYTALE_DEVICE_VERIFICATION_URI);
        assert_eq!(detected.user_code.as_deref(), Some("ABCD1234"));
    }

    #[test]
    fn hytale_launch_is_java_25_and_handles_only_documented_update_exit_code() {
        let plan =
            launch_plan(&serde_json::json!({"port": 5520, "auth_mode": "authenticated"})).unwrap();
        assert_eq!(
            plan.executable,
            InstallerExecutable::ManagedJava { major: 25 }
        );
        assert_eq!(plan.cwd_relative, "Server");
        assert_eq!(plan.restart_exit_codes, vec![8]);
    }

    #[test]
    fn configuration_merge_keeps_unknown_fields() {
        let existing = br#"{"UnknownProviderField":true,"Port":1}"#;
        let updates = serde_json::json!({"Port": 5520})
            .as_object()
            .unwrap()
            .clone();
        let merged = merge_json_preserving_unknown(existing, &updates).unwrap();
        let value: Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(value["UnknownProviderField"], true);
        assert_eq!(value["Port"], 5520);
    }

    #[test]
    fn printed_version_and_credential_redactions_are_bounded() {
        assert_eq!(
            parse_printed_version("Hytale Downloader\nGame version: 2026.06.15-abcd\n").as_deref(),
            Some("2026.06.15-abcd")
        );
        assert!(parse_printed_version("Hytale Downloader\nDone\n").is_none());
        assert!(parse_printed_version("https://evil.invalid/token").is_none());
        let redactions = credential_redactions(
            r#"{"access_token":"access-secret-value","nested":{"refresh":"refresh-secret-value"}}"#,
        )
        .unwrap();
        assert!(redactions.contains(&"access-secret-value".to_string()));
        assert!(redactions.contains(&"refresh-secret-value".to_string()));
        assert!(credential_redactions("[]").is_err());
    }

    #[tokio::test]
    async fn archive_layout_requires_aot_and_preserves_server_owned_data() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("instance");
        let current = root.join("game/Server");
        tokio::fs::create_dir_all(current.join("universe/worlds/default"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(current.join("mods"))
            .await
            .unwrap();
        tokio::fs::write(
            current.join("config.json"),
            br#"{"UnknownProviderField":true}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(current.join("universe/worlds/default/world.bin"), b"save")
            .await
            .unwrap();
        tokio::fs::write(current.join("mods/example.jar"), b"mod")
            .await
            .unwrap();

        let archive = directory.path().join("game.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = ZipWriter::new(file);
        for (name, contents) in [
            ("Assets.zip", b"assets".as_slice()),
            ("Server/HytaleServer.jar", b"jar".as_slice()),
            ("Server/HytaleServer.aot", b"aot".as_slice()),
            ("Server/config.json", br#"{"NewDefault":true}"#.as_slice()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();

        let staging = root.join("staging");
        let context = InstallContext::official().unwrap();
        let result = install_downloaded_archive(
            &serde_json::json!({}),
            &root,
            &archive,
            &staging,
            &context,
            "2026.06.15-abcd".into(),
            InstalledArtifact {
                name: "downloader".into(),
                sha256: "00".repeat(32),
                size: 1,
            },
        )
        .await
        .unwrap();
        assert_eq!(result.installed_version, "2026.06.15-abcd");
        assert_eq!(
            tokio::fs::read(staging.join("Server/config.json"))
                .await
                .unwrap(),
            br#"{"UnknownProviderField":true}"#
        );
        assert_eq!(
            tokio::fs::read(staging.join("Server/universe/worlds/default/world.bin"))
                .await
                .unwrap(),
            b"save"
        );
        assert_eq!(
            tokio::fs::read(staging.join("Server/mods/example.jar"))
                .await
                .unwrap(),
            b"mod"
        );
    }

    #[tokio::test]
    async fn runtime_update_candidate_preserves_instance_data() {
        let directory = tempfile::tempdir().unwrap();
        let current = directory.path().join("game");
        let provider = current.join("updater/staging");
        let candidate = directory.path().join("candidate");
        tokio::fs::create_dir_all(current.join("Server/universe"))
            .await
            .unwrap();
        tokio::fs::write(current.join("Server/universe/world.bin"), b"world")
            .await
            .unwrap();
        tokio::fs::write(current.join("Server/config.json"), br#"{"Custom":true}"#)
            .await
            .unwrap();
        tokio::fs::create_dir_all(provider.join("Server"))
            .await
            .unwrap();
        tokio::fs::write(provider.join("Assets.zip"), b"new-assets")
            .await
            .unwrap();
        tokio::fs::write(provider.join("Server/HytaleServer.jar"), b"new-jar")
            .await
            .unwrap();
        tokio::fs::write(provider.join("Server/HytaleServer.aot"), b"new-aot")
            .await
            .unwrap();
        tokio::fs::write(provider.join("Server/config.json"), b"provider-default")
            .await
            .unwrap();

        prepare_runtime_update(&current, &provider, &candidate)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(candidate.join("Server/HytaleServer.jar"))
                .await
                .unwrap(),
            b"new-jar"
        );
        assert_eq!(
            tokio::fs::read(candidate.join("Server/universe/world.bin"))
                .await
                .unwrap(),
            b"world"
        );
        assert_eq!(
            tokio::fs::read(candidate.join("Server/config.json"))
                .await
                .unwrap(),
            br#"{"Custom":true}"#
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_update_rejects_a_linked_provider_staging_tree() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let external = directory.path().join("external-stage");
        tokio::fs::create_dir_all(game.join("updater"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(external.join("Server"))
            .await
            .unwrap();
        tokio::fs::write(external.join("Assets.zip"), b"assets")
            .await
            .unwrap();
        tokio::fs::write(external.join("Server/HytaleServer.jar"), b"jar")
            .await
            .unwrap();
        tokio::fs::write(external.join("Server/HytaleServer.aot"), b"aot")
            .await
            .unwrap();
        symlink(&external, game.join("updater/staging")).unwrap();

        let error = prepare_runtime_update(
            &game,
            &game.join("updater/staging"),
            &directory.path().join("candidate"),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "hytale_layout_invalid");
    }

    #[tokio::test]
    async fn plaintext_credentials_are_private_validated_and_removed() {
        let directory = tempfile::tempdir().unwrap();
        let credentials_dir = directory.path().join("credentials");
        create_private_directory(&credentials_dir).await.unwrap();
        let path = credentials_dir.join(".hytale-downloader-credentials.json");
        let document = r#"{"access_token":"never-log-this-token"}"#;
        write_plaintext_credentials(&path, document).await.unwrap();
        assert_eq!(read_plaintext_credentials(&path).await.unwrap(), document);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        remove_plaintext_credentials(&path).await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    #[ignore = "live pre-release smoke: downloads the official Hytale bootstrap without executing it"]
    async fn live_official_downloader_archive_has_the_expected_platform_layout() {
        let directory = tempfile::tempdir().unwrap();
        let context = InstallContext::official().unwrap();
        let plan = prepare_hytale_downloader(directory.path(), &context)
            .await
            .unwrap();

        assert!(plan.executable.is_file());
        assert!(plan.downloader_artifact.size > 0);
        assert_eq!(plan.downloader_artifact.sha256.len(), 64);
        assert_eq!(plan.args.len(), 3);
    }
}
