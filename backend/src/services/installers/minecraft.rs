use std::{collections::BTreeMap, path::Path};

use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::domain::v1::StopStrategy;

use super::{
    DownloadedFile, ExpectedDigest, InstallContext, InstallResult, InstalledArtifact,
    InstallerError, InstallerExecutable, InstallerPlan, download_verified, read_json,
};

const MAX_SERVER_JAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionReference>,
}

#[derive(Debug, Deserialize)]
struct VersionReference {
    id: String,
    url: String,
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct VersionMetadata {
    id: String,
    downloads: VersionDownloads,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
}

#[derive(Debug, Deserialize)]
struct VersionDownloads {
    server: Option<MojangDownload>,
}

#[derive(Debug, Deserialize)]
struct MojangDownload {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct JavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u16,
}

#[derive(Debug, Deserialize)]
struct PaperBuild {
    id: u32,
    channel: String,
    downloads: BTreeMap<String, PaperDownload>,
}

#[derive(Debug, Deserialize)]
struct PaperDownload {
    checksums: PaperChecksums,
    size: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct PaperChecksums {
    sha256: String,
}

struct ResolvedMinecraft {
    metadata: VersionMetadata,
}

pub async fn install_vanilla(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    validate_eula(settings)?;
    let requested_version = required_string(settings, "version")?;
    let resolved = resolve_minecraft_version(context, &requested_version, staging).await?;
    let java_major = required_java_major(&resolved.metadata)?;
    let server = resolved.metadata.downloads.server.ok_or_else(|| {
        InstallerError::new(
            "minecraft_server_unavailable",
            "servers.minecraft_server_unavailable",
        )
    })?;
    validate_hex_digest(&server.sha1, 40)?;
    let downloaded = download_verified(
        context,
        &parse_provider_url(&server.url)?,
        &staging.join("server.jar"),
        MAX_SERVER_JAR_BYTES,
        Some(&ExpectedDigest::Sha1(server.sha1)),
        Some(server.size),
    )
    .await?;

    preserve_java_data(instance_root, staging, false).await?;
    write_java_configuration(instance_root, staging, settings).await?;
    java_result(resolved.metadata.id, None, java_major, downloaded, settings)
}

pub async fn install_paper(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    validate_eula(settings)?;
    let requested_version = required_string(settings, "version")?;
    let resolved = resolve_minecraft_version(context, &requested_version, staging).await?;
    let java_major = required_java_major(&resolved.metadata)?;

    let builds_url = paper_builds_url(&context.sources.paper_api_base, &requested_version)?;
    let builds: Vec<PaperBuild> = read_json(context, &builds_url).await?;
    let build = builds
        .into_iter()
        .filter(|build| build.channel == "STABLE")
        .max_by_key(|build| build.id)
        .ok_or_else(|| {
            InstallerError::new(
                "paper_stable_build_unavailable",
                "servers.paper_stable_build_unavailable",
            )
        })?;
    let artifact = build.downloads.get("server:default").ok_or_else(|| {
        InstallerError::new(
            "paper_artifact_unavailable",
            "servers.provider_response_invalid",
        )
    })?;
    validate_hex_digest(&artifact.checksums.sha256, 64)?;
    let downloaded = download_verified(
        context,
        &parse_provider_url(&artifact.url)?,
        &staging.join("server.jar"),
        MAX_SERVER_JAR_BYTES,
        Some(&ExpectedDigest::Sha256(artifact.checksums.sha256.clone())),
        Some(artifact.size),
    )
    .await?;

    preserve_java_data(instance_root, staging, true).await?;
    write_java_configuration(instance_root, staging, settings).await?;
    java_result(
        resolved.metadata.id,
        Some(build.id.to_string()),
        java_major,
        downloaded,
        settings,
    )
}

pub fn launch_plan(settings: &Value, java_major: u16) -> Result<InstallerPlan, InstallerError> {
    let memory = settings
        .get("max_memory_mb")
        .and_then(Value::as_u64)
        .unwrap_or(4096);
    if !(512..=131_072).contains(&memory) {
        return Err(InstallerError::new(
            "minecraft_memory_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(InstallerPlan {
        executable: InstallerExecutable::ManagedJava { major: java_major },
        cwd_relative: ".".to_string(),
        args: vec![
            format!("-Xmx{memory}M"),
            "-jar".to_string(),
            "server.jar".to_string(),
            "nogui".to_string(),
        ],
        env: Vec::new(),
        stop: StopStrategy::Stdin {
            command: "stop".to_string(),
            timeout_seconds: 60,
        },
        restart_exit_codes: Vec::new(),
    })
}

pub(super) async fn validate_installed(
    settings: &Value,
    game_root: &Path,
    java_major: u16,
) -> Result<InstallerPlan, InstallerError> {
    validate_eula(settings)?;
    let _ = required_string(settings, "version")?;
    let path = game_root.join("server.jar");
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|error| InstallerError::internal("minecraft_runtime_missing", error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SERVER_JAR_BYTES
    {
        return Err(InstallerError::new(
            "minecraft_runtime_invalid",
            "servers.installed_executable_invalid",
        ));
    }
    launch_plan(settings, java_major)
}

async fn resolve_minecraft_version(
    context: &InstallContext,
    version: &str,
    staging: &Path,
) -> Result<ResolvedMinecraft, InstallerError> {
    let manifest: VersionManifest = read_json(context, &context.sources.minecraft_manifest).await?;
    let reference = manifest
        .versions
        .into_iter()
        .find(|candidate| candidate.id == version)
        .ok_or_else(|| {
            InstallerError::new(
                "minecraft_version_unknown",
                "servers.minecraft_version_unknown",
            )
        })?;
    validate_hex_digest(&reference.sha1, 40)?;
    let metadata_path = staging.join(format!(
        ".provider-metadata-{}.json",
        uuid::Uuid::new_v4().as_simple()
    ));
    download_verified(
        context,
        &parse_provider_url(&reference.url)?,
        &metadata_path,
        MAX_METADATA_BYTES,
        Some(&ExpectedDigest::Sha1(reference.sha1)),
        None,
    )
    .await?;
    let bytes = tokio::fs::read(&metadata_path)
        .await
        .map_err(|error| InstallerError::internal("provider_metadata_failed", error))?;
    let _ = tokio::fs::remove_file(&metadata_path).await;
    let metadata: VersionMetadata = serde_json::from_slice(&bytes)
        .map_err(|error| InstallerError::internal("provider_json_invalid", error))?;
    if metadata.id != version {
        return Err(InstallerError::new(
            "minecraft_version_mismatch",
            "servers.provider_response_invalid",
        ));
    }
    Ok(ResolvedMinecraft { metadata })
}

fn required_java_major(metadata: &VersionMetadata) -> Result<u16, InstallerError> {
    let major = metadata
        .java_version
        .as_ref()
        .map(|java| java.major_version)
        .filter(|major| matches!(*major, 8 | 16 | 17 | 21 | 25))
        .ok_or_else(|| {
            InstallerError::new(
                "minecraft_java_version_unknown",
                "servers.minecraft_java_version_unknown",
            )
        })?;
    Ok(major)
}

pub(super) async fn imported_runtime(
    settings: &Value,
    game_root: &Path,
    context: &InstallContext,
) -> Result<(String, u16), InstallerError> {
    let requested_version = required_string(settings, "version")?;
    let resolved = resolve_minecraft_version(context, &requested_version, game_root).await?;
    let java_major = required_java_major(&resolved.metadata)?;
    Ok((resolved.metadata.id, java_major))
}

fn paper_builds_url(base: &Url, version: &str) -> Result<Url, InstallerError> {
    let mut url = base
        .join("projects/paper/versions/")
        .map_err(|error| InstallerError::internal("paper_url_failed", error))?;
    url.path_segments_mut()
        .map_err(|_| InstallerError::new("paper_url_failed", "servers.provider_response_invalid"))?
        .pop_if_empty()
        .push(version)
        .push("builds");
    Ok(url)
}

fn parse_provider_url(value: &str) -> Result<Url, InstallerError> {
    Url::parse(value).map_err(|_| {
        InstallerError::new("provider_url_invalid", "servers.provider_response_invalid")
    })
}

fn java_result(
    installed_version: String,
    installed_build: Option<String>,
    java_major: u16,
    downloaded: DownloadedFile,
    settings: &Value,
) -> Result<InstallResult, InstallerError> {
    Ok(InstallResult {
        plan: launch_plan(settings, java_major)?,
        installed_version,
        installed_build,
        artifacts: vec![InstalledArtifact {
            name: "server.jar".to_string(),
            sha256: downloaded.sha256,
            size: downloaded.size,
        }],
    })
}

async fn preserve_java_data(
    instance_root: &Path,
    staging: &Path,
    plugins: bool,
) -> Result<(), InstallerError> {
    let current = instance_root.join("game");
    let mut paths = preserved_world_paths(instance_root).await?;
    paths.extend(
        [
            "ops.json",
            "whitelist.json",
            "banned-players.json",
            "banned-ips.json",
        ]
        .map(str::to_string),
    );
    if plugins {
        paths.extend(
            [
                "plugins",
                "config",
                "bukkit.yml",
                "spigot.yml",
                "paper.yml",
                "paper-global.yml",
                "paper-world-defaults.yml",
            ]
            .map(str::to_string),
        );
    }
    for relative in paths {
        copy_optional_without_links(&current.join(&relative), &staging.join(&relative)).await?;
    }
    Ok(())
}

pub(super) async fn preserved_world_paths(
    instance_root: &Path,
) -> Result<Vec<String>, InstallerError> {
    let properties = read_bounded_regular_text(
        &instance_root.join("game"),
        "server.properties",
        1024 * 1024,
    )
    .await?
    .unwrap_or_default();
    let level_name = properties
        .lines()
        .filter_map(|line| property_value(line, "level-name"))
        .next_back()
        .unwrap_or("world");
    if level_name.is_empty()
        || level_name.len() > 255
        || level_name.trim_end() != level_name
        || matches!(level_name, "." | "..")
        || level_name.contains(['/', '\\', ':', '\0'])
        || level_name.chars().any(char::is_control)
    {
        return Err(InstallerError::new(
            "minecraft_level_name_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    Ok(vec![
        level_name.to_string(),
        format!("{level_name}_nether"),
        format!("{level_name}_the_end"),
    ])
}

pub(super) async fn copy_optional_without_links(
    source: &Path,
    destination: &Path,
) -> Result<(), InstallerError> {
    let metadata = match tokio::fs::symlink_metadata(source).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(InstallerError::internal("preserve_data_failed", error)),
    };
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(InstallerError::new(
            "preserve_data_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
        }
        tokio::fs::copy(source, destination)
            .await
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
        return Ok(());
    }

    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_directory, target_directory)) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&source_directory)
            .await
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
                return Err(InstallerError::new(
                    "preserve_data_unsafe",
                    "servers.instance_data_unsafe",
                ));
            }
            let target = target_directory.join(entry.file_name());
            if file_type.is_dir() {
                tokio::fs::create_dir(&target)
                    .await
                    .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
                pending.push((entry.path(), target));
            } else {
                tokio::fs::copy(entry.path(), target)
                    .await
                    .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            }
        }
    }
    Ok(())
}

pub(super) async fn write_java_configuration(
    instance_root: &Path,
    staging: &Path,
    settings: &Value,
) -> Result<(), InstallerError> {
    let port = settings
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(25_565);
    if !(1..=65_535).contains(&port) {
        return Err(InstallerError::new(
            "minecraft_port_invalid",
            "servers.settings_invalid",
        ));
    }
    let current_properties = read_bounded_regular_text(
        &instance_root.join("game"),
        "server.properties",
        1024 * 1024,
    )
    .await?
    .unwrap_or_default();
    let properties = merge_properties(&current_properties, &[("server-port", port.to_string())]);
    write_new(
        staging.join("server.properties").as_path(),
        properties.as_bytes(),
    )
    .await?;
    write_new(staging.join("eula.txt").as_path(), b"eula=true\n").await
}

/// Reads a configuration file through the same no-follow/hardlink checks used by
/// the file manager. A game process must not be able to make an update copy a
/// host file into the next release by replacing a configuration file with a link.
pub(super) async fn read_bounded_regular_text(
    root: &Path,
    relative: &str,
    max_bytes: u64,
) -> Result<Option<String>, InstallerError> {
    let path = root.join(relative);
    match tokio::fs::symlink_metadata(&path).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(InstallerError::internal(
                "configuration_metadata_failed",
                error,
            ));
        }
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(InstallerError::new(
                "configuration_path_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        Ok(_) => {}
    }

    let (file, size) = crate::services::secure_fs::open_regular_file(root, relative)
        .await
        .map_err(|_| {
            InstallerError::new("configuration_path_unsafe", "servers.instance_data_unsafe")
        })?;
    if size > max_bytes {
        return Err(InstallerError::new(
            "configuration_too_large",
            "servers.configuration_invalid",
        ));
    }
    let mut contents = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut contents)
        .await
        .map_err(|error| InstallerError::internal("configuration_read_failed", error))?;
    if contents.len() as u64 > max_bytes {
        return Err(InstallerError::new(
            "configuration_too_large",
            "servers.configuration_invalid",
        ));
    }
    String::from_utf8(contents).map(Some).map_err(|_| {
        InstallerError::new(
            "configuration_encoding_invalid",
            "servers.configuration_invalid",
        )
    })
}

pub(super) fn merge_properties(existing: &str, updates: &[(&str, String)]) -> String {
    let update_map = updates.iter().cloned().collect::<BTreeMap<_, _>>();
    let mut written = BTreeMap::<&str, bool>::new();
    let mut output = Vec::new();
    for line in existing.lines() {
        if let Some(key) = property_key(line)
            && let Some(value) = update_map.get(key)
        {
            if !written.contains_key(key) {
                output.push(format!("{key}={value}"));
                written.insert(key, true);
            }
            continue;
        }
        output.push(line.to_string());
    }
    for (key, value) in updates {
        if !written.contains_key(key) {
            output.push(format!("{key}={value}"));
        }
    }
    let mut result = output.join("\n");
    result.push('\n');
    result
}

fn property_key(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let index = line
        .char_indices()
        .find_map(|(index, character)| {
            (character == '=' || character == ':' || character.is_ascii_whitespace())
                .then_some(index)
        })
        .unwrap_or(line.len());
    let key = &line[..index];
    (!key.is_empty()).then_some(key)
}

fn property_value<'a>(line: &'a str, expected_key: &str) -> Option<&'a str> {
    let line = line.trim_start();
    (property_key(line) == Some(expected_key)).then(|| {
        let remainder = line[expected_key.len()..].trim_start();
        remainder
            .strip_prefix(['=', ':'])
            .unwrap_or(remainder)
            .trim_start()
    })
}

async fn write_new(path: &Path, contents: &[u8]) -> Result<(), InstallerError> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    file.write_all(contents)
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))
}

fn required_string(settings: &Value, key: &'static str) -> Result<String, InstallerError> {
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .map(str::to_string)
        .ok_or_else(|| InstallerError::new("settings_invalid", "servers.settings_invalid"))
}

pub(super) fn validate_eula(settings: &Value) -> Result<(), InstallerError> {
    if settings.get("eula_accepted").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "minecraft_eula_required",
            "servers.minecraft_eula_required",
        ))
    }
}

fn validate_hex_digest(value: &str, length: usize) -> Result<(), InstallerError> {
    if value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "provider_checksum_invalid",
            "servers.provider_response_invalid",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sha1::{Digest as _, Sha1};
    use sha2::Sha256;
    use tempfile::tempdir;

    use super::*;
    use crate::services::installers::InstallerSources;

    fn metadata_bytes(base: &str, jar: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": "1.21.11",
            "javaVersion": {"majorVersion": 21},
            "downloads": {"server": {
                "url": format!("{base}minecraft/server.jar"),
                "sha1": format!("{:x}", Sha1::digest(jar)),
                "size": jar.len()
            }}
        }))
        .unwrap()
    }

    fn fixture_context() -> InstallContext {
        let base = "https://fixtures.invalid/";
        let base_url = Url::parse(base).unwrap();
        let jar = b"fixture-server-jar".to_vec();
        let metadata = metadata_bytes(base, &jar);
        let manifest = serde_json::to_vec(&serde_json::json!({
            "versions": [{
                "id": "1.21.11",
                "url": format!("{base}minecraft/version.json"),
                "sha1": format!("{:x}", Sha1::digest(&metadata))
            }]
        }))
        .unwrap();
        let paper_builds = serde_json::to_vec(&serde_json::json!([{
            "id": 42,
            "channel": "STABLE",
            "downloads": {"server:default": {
                "url": format!("{base}paper/server.jar"),
                "size": jar.len(),
                "checksums": {"sha256": format!("{:x}", Sha256::digest(&jar))}
            }}
        }]))
        .unwrap();
        let responses = BTreeMap::from([
            (format!("{base}minecraft/manifest.json"), manifest),
            (format!("{base}minecraft/version.json"), metadata),
            (format!("{base}minecraft/server.jar"), jar.clone()),
            (
                format!("{base}paper/projects/paper/versions/1.21.11/builds"),
                paper_builds,
            ),
            (format!("{base}paper/server.jar"), jar),
        ]);
        InstallContext::with_fixture_responses(InstallerSources::fixture(&base_url), responses)
            .unwrap()
    }

    #[tokio::test]
    async fn vanilla_install_is_checksummed_and_preserves_world_and_unknown_properties() {
        let context = fixture_context();
        let root = tempdir().unwrap();
        tokio::fs::create_dir_all(root.path().join("game/world"))
            .await
            .unwrap();
        tokio::fs::write(root.path().join("game/world/level.dat"), b"world")
            .await
            .unwrap();
        tokio::fs::write(
            root.path().join("game/server.properties"),
            "# keep\nserver-port=1234\ncustom-field=unchanged\n",
        )
        .await
        .unwrap();
        let staging = root.path().join("staging");
        tokio::fs::create_dir(&staging).await.unwrap();
        let settings = serde_json::json!({
            "version": "1.21.11",
            "port": 25570,
            "max_memory_mb": 4096,
            "eula_accepted": true
        });
        let result = install_vanilla(&settings, root.path(), &staging, &context)
            .await
            .unwrap();
        assert_eq!(result.installed_version, "1.21.11");
        assert_eq!(
            result.plan.executable,
            InstallerExecutable::ManagedJava { major: 21 }
        );
        assert_eq!(
            tokio::fs::read(staging.join("world/level.dat"))
                .await
                .unwrap(),
            b"world"
        );
        let properties = tokio::fs::read_to_string(staging.join("server.properties"))
            .await
            .unwrap();
        assert!(properties.contains("custom-field=unchanged"));
        assert!(properties.contains("server-port=25570"));
        assert_eq!(
            tokio::fs::read_to_string(staging.join("eula.txt"))
                .await
                .unwrap(),
            "eula=true\n"
        );
    }

    #[tokio::test]
    async fn update_preserves_a_safe_custom_level_name_and_rejects_traversal() {
        let root = tempdir().unwrap();
        tokio::fs::create_dir_all(root.path().join("game/custom_world"))
            .await
            .unwrap();
        tokio::fs::write(
            root.path().join("game/server.properties"),
            "level-name=custom_world\n",
        )
        .await
        .unwrap();
        assert_eq!(
            preserved_world_paths(root.path()).await.unwrap(),
            [
                "custom_world",
                "custom_world_nether",
                "custom_world_the_end"
            ]
        );
        tokio::fs::write(
            root.path().join("game/server.properties"),
            "level-name=../other-instance\n",
        )
        .await
        .unwrap();
        assert!(preserved_world_paths(root.path()).await.is_err());
    }

    #[tokio::test]
    async fn paper_uses_latest_stable_build_and_sha256() {
        let context = fixture_context();
        let root = tempdir().unwrap();
        let staging = root.path().join("staging");
        tokio::fs::create_dir(&staging).await.unwrap();
        let settings = serde_json::json!({
            "version": "1.21.11",
            "eula_accepted": true
        });
        let result = install_paper(&settings, root.path(), &staging, &context)
            .await
            .unwrap();
        assert_eq!(result.installed_version, "1.21.11");
        assert_eq!(result.installed_build.as_deref(), Some("42"));
        assert_eq!(result.artifacts[0].name, "server.jar");
    }

    #[test]
    fn properties_keep_unknown_fields_and_collapse_known_duplicates() {
        let merged = merge_properties(
            "# comment\nserver-port=1\nunknown=yes\nserver-port=2\n",
            &[("server-port", "25565".to_string())],
        );
        assert_eq!(merged.matches("server-port=").count(), 1);
        assert!(merged.contains("unknown=yes"));
        assert!(merged.contains("# comment"));
    }

    #[test]
    fn eula_is_never_implicitly_accepted() {
        let error = validate_eula(&serde_json::json!({})).unwrap_err();
        assert_eq!(error.code, "minecraft_eula_required");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn properties_are_never_read_through_links() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        tokio::fs::create_dir_all(root.path().join("game"))
            .await
            .unwrap();
        tokio::fs::create_dir(root.path().join("staging"))
            .await
            .unwrap();
        let outside = root.path().join("outside-secret");
        tokio::fs::write(&outside, b"secret=value\n").await.unwrap();
        symlink(&outside, root.path().join("game/server.properties")).unwrap();

        let error = write_java_configuration(
            root.path(),
            &root.path().join("staging"),
            &serde_json::json!({"port": 25565}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "configuration_path_unsafe");
    }

    #[tokio::test]
    #[ignore = "live pre-release smoke: queries Mojang and Paper metadata without downloading a server"]
    async fn live_official_mojang_and_paper_metadata_contracts_are_compatible() {
        let context = InstallContext::official().unwrap();
        let directory = tempdir().unwrap();
        let resolved = resolve_minecraft_version(&context, "1.21.8", directory.path())
            .await
            .unwrap();
        assert_eq!(resolved.metadata.id, "1.21.8");
        assert!(matches!(required_java_major(&resolved.metadata), Ok(21)));

        let builds_url = paper_builds_url(&context.sources.paper_api_base, "1.21.8").unwrap();
        let builds: Vec<PaperBuild> = read_json(&context, &builds_url).await.unwrap();
        let stable = builds
            .iter()
            .filter(|build| build.channel == "STABLE")
            .max_by_key(|build| build.id)
            .unwrap();
        let artifact = stable.downloads.get("server:default").unwrap();
        validate_hex_digest(&artifact.checksums.sha256, 64).unwrap();
        assert!(artifact.size > 0);
        context
            .sources
            .validate_url(&parse_provider_url(&artifact.url).unwrap())
            .unwrap();
    }
}
