use std::{path::Path, time::Duration};

use futures::StreamExt;
use reqwest::{Client, Response, Url, header, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest as _, Sha1};
use sha2::{Sha256, Sha512};
use tokio::io::AsyncReadExt;

use crate::{
    core::{DbPool, error::AppError},
    services::{installers::USER_AGENT, secrets::SecretStore, secure_fs},
};

const CURSEFORGE_SETTING_KEY: &str = "mods.curseforge_api_key";
const CURSEFORGE_SETTING_AAD: &str = "global:curseforge_api_key";
const MAX_PROVIDER_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MOD_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Modrinth,
    CurseForge,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::CurseForge => "curseforge",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Compatibility {
    pub profile_id: String,
    pub game_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RequiredDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderArtifact {
    pub provider: Provider,
    pub project_id: String,
    pub version_id: String,
    pub display_name: String,
    pub url: Url,
    pub expected_size: u64,
    expected_digest: ProviderDigest,
    pub required_dependencies: Vec<RequiredDependency>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
enum ProviderDigest {
    Sha1(String),
    Sha512(String),
}

#[derive(Debug, Clone)]
pub struct DownloadedArtifact {
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedSetting {
    nonce: String,
    ciphertext: String,
}

pub async fn curseforge_configured(pool: &DbPool) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE key = ?")
        .bind(CURSEFORGE_SETTING_KEY)
        .fetch_one(pool)
        .await?;
    Ok(count == 1)
}

pub async fn set_curseforge_api_key(
    pool: &DbPool,
    secrets: &SecretStore,
    api_key: &str,
) -> Result<(), AppError> {
    validate_api_key(api_key)?;
    let (nonce, ciphertext) = secrets.seal(CURSEFORGE_SETTING_AAD, api_key)?;
    let value = serde_json::to_string(&EncryptedSetting { nonce, ciphertext })
        .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(CURSEFORGE_SETTING_KEY)
    .bind(value)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_curseforge_api_key(pool: &DbPool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(CURSEFORGE_SETTING_KEY)
        .execute(pool)
        .await?;
    Ok(())
}

async fn curseforge_api_key(pool: &DbPool, secrets: &SecretStore) -> Result<String, AppError> {
    let value: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(CURSEFORGE_SETTING_KEY)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("mods.curseforge_not_configured".into()))?;
    let encrypted: EncryptedSetting = serde_json::from_str(&value)
        .map_err(|_| AppError::Internal("encrypted CurseForge setting is invalid".into()))?;
    secrets.open(
        CURSEFORGE_SETTING_AAD,
        &encrypted.nonce,
        &encrypted.ciphertext,
    )
}

pub async fn resolve_modrinth(
    project_id: &str,
    version_id: &str,
    compatibility: &Compatibility,
) -> Result<ProviderArtifact, AppError> {
    validate_modrinth_id(project_id)?;
    validate_modrinth_id(version_id)?;
    let expected_loader = expected_modrinth_loader(&compatibility.profile_id)?;
    let game_version = compatibility
        .game_version
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("mods.game_version_required".into()))?;
    let client = provider_client()?;
    let project_url = provider_url("https://api.modrinth.com/v2/", &["project", project_id])?;
    let project: ModrinthProject = get_json(&client, project_url, None).await?;
    if project.id != project_id
        || !matches!(project.project_type.as_str(), "mod" | "plugin")
        || project.server_side == "unsupported"
    {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }

    let version_url = provider_url("https://api.modrinth.com/v2/", &["version", version_id])?;
    let version: ModrinthVersion = get_json(&client, version_url, None).await?;
    if version.id != version_id
        || version.project_id != project_id
        || !version
            .loaders
            .iter()
            .any(|loader| loader.eq_ignore_ascii_case(expected_loader))
        || !version
            .game_versions
            .iter()
            .any(|item| item == game_version)
    {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }
    let file = select_modrinth_file(&version.files)?;
    validate_provider_filename(&file.filename)?;
    validate_hex(&file.hashes.sha512, 128, "mods.provider_checksum_missing")?;
    if file.size == 0 || file.size > MAX_MOD_BYTES {
        return Err(AppError::BadRequest("mods.provider_file_too_large".into()));
    }
    let url = Url::parse(&file.url)
        .map_err(|_| AppError::BadRequest("mods.provider_url_rejected".into()))?;
    validate_download_url(Provider::Modrinth, &url)?;
    let required_dependencies = version
        .dependencies
        .into_iter()
        .filter(|dependency| dependency.dependency_type == "required")
        .map(|dependency| RequiredDependency {
            project_id: dependency.project_id,
            version_id: dependency.version_id,
        })
        .collect::<Vec<_>>();
    if required_dependencies
        .iter()
        .any(|dependency| dependency.project_id.is_none() && dependency.version_id.is_none())
    {
        return Err(AppError::BadRequest(
            "mods.provider_dependency_invalid".into(),
        ));
    }

    Ok(ProviderArtifact {
        provider: Provider::Modrinth,
        project_id: project_id.to_string(),
        version_id: version_id.to_string(),
        display_name: file.filename.clone(),
        url,
        expected_size: file.size,
        expected_digest: ProviderDigest::Sha512(file.hashes.sha512.clone()),
        required_dependencies,
        metadata: serde_json::json!({
            "project_type": project.project_type,
            "version_name": version.name,
            "loader": expected_loader,
            "game_version": game_version,
            "provider_hash": {"algorithm": "sha512", "value": file.hashes.sha512},
        }),
    })
}

pub async fn resolve_curseforge(
    pool: &DbPool,
    secrets: &SecretStore,
    project_id: &str,
    version_id: &str,
    compatibility: &Compatibility,
) -> Result<ProviderArtifact, AppError> {
    let project_id_number = validate_curseforge_id(project_id)?;
    let version_id_number = validate_curseforge_id(version_id)?;
    let api_key = curseforge_api_key(pool, secrets).await?;
    let client = provider_client()?;

    let project_url = provider_url("https://api.curseforge.com/v1/", &["mods", project_id])?;
    let project: CurseForgeEnvelope<CurseForgeProject> =
        get_json(&client, project_url, Some(&api_key)).await?;
    if project.data.id != project_id_number
        || !project.data.is_available
        || project.data.allow_mod_distribution == Some(false)
        || !curseforge_project_path_matches(
            &compatibility.profile_id,
            project.data.links.website_url.as_deref(),
        )
    {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }

    let file_url = provider_url(
        "https://api.curseforge.com/v1/",
        &["mods", project_id, "files", version_id],
    )?;
    let file: CurseForgeEnvelope<CurseForgeFile> =
        get_json(&client, file_url, Some(&api_key)).await?;
    let file = file.data;
    if file.id != version_id_number
        || file.mod_id != project_id_number
        || file.game_id != project.data.game_id
        || !file.is_available
    {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }

    let game_url = provider_url(
        "https://api.curseforge.com/v1/",
        &["games", &file.game_id.to_string()],
    )?;
    let game: CurseForgeEnvelope<CurseForgeGame> =
        get_json(&client, game_url, Some(&api_key)).await?;
    validate_curseforge_compatibility(compatibility, &game.data.slug, &file.game_versions)?;
    validate_provider_filename(&file.file_name)?;
    if file.file_length == 0 || file.file_length > MAX_MOD_BYTES {
        return Err(AppError::BadRequest("mods.provider_file_too_large".into()));
    }
    let sha1 = file
        .hashes
        .iter()
        .find(|hash| hash.algo == 1)
        .map(|hash| hash.value.as_str())
        .ok_or_else(|| AppError::BadRequest("mods.provider_checksum_missing".into()))?;
    validate_hex(sha1, 40, "mods.provider_checksum_missing")?;
    let download_url = file
        .download_url
        .ok_or_else(|| AppError::BadRequest("mods.provider_distribution_disabled".into()))?;
    let url = Url::parse(&download_url)
        .map_err(|_| AppError::BadRequest("mods.provider_url_rejected".into()))?;
    validate_download_url(Provider::CurseForge, &url)?;
    let required_dependencies = file
        .dependencies
        .into_iter()
        .filter(|dependency| dependency.relation_type == 3)
        .map(|dependency| RequiredDependency {
            project_id: Some(dependency.mod_id.to_string()),
            version_id: None,
        })
        .collect::<Vec<_>>();

    Ok(ProviderArtifact {
        provider: Provider::CurseForge,
        project_id: project_id.to_string(),
        version_id: version_id.to_string(),
        display_name: file.file_name,
        url,
        expected_size: file.file_length,
        expected_digest: ProviderDigest::Sha1(sha1.to_string()),
        required_dependencies,
        metadata: serde_json::json!({
            "game": game.data.slug,
            "game_versions": file.game_versions,
            "provider_hash": {"algorithm": "sha1", "value": sha1},
        }),
    })
}

pub async fn download(
    artifact: &ProviderArtifact,
    root: &Path,
    relative_path: &str,
) -> Result<DownloadedArtifact, AppError> {
    let client = provider_client()?;
    let response = get_download_response(&client, artifact.provider, artifact.url.clone()).await?;
    if response
        .content_length()
        .is_some_and(|size| size != artifact.expected_size || size > MAX_MOD_BYTES)
    {
        return Err(AppError::BadRequest("mods.provider_size_mismatch".into()));
    }
    let stream = response.bytes_stream();
    let written = secure_fs::write_stream(root, relative_path, stream, MAX_MOD_BYTES).await?;
    if written != artifact.expected_size {
        let _ = secure_fs::delete_entry(root, relative_path).await;
        return Err(AppError::BadRequest("mods.provider_size_mismatch".into()));
    }
    let digests = match hash_jar(root, relative_path).await {
        Ok(value) => value,
        Err(error) => {
            let _ = secure_fs::delete_entry(root, relative_path).await;
            return Err(error);
        }
    };
    let valid = match &artifact.expected_digest {
        ProviderDigest::Sha1(value) => digests.sha1.eq_ignore_ascii_case(value),
        ProviderDigest::Sha512(value) => digests.sha512.eq_ignore_ascii_case(value),
    };
    if !valid {
        let _ = secure_fs::delete_entry(root, relative_path).await;
        return Err(AppError::BadRequest(
            "mods.provider_checksum_mismatch".into(),
        ));
    }
    Ok(DownloadedArtifact {
        sha256: digests.sha256,
        size_bytes: written,
    })
}

fn provider_client() -> Result<Client, AppError> {
    Client::builder()
        .redirect(Policy::none())
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(120))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn provider_url(base: &str, segments: &[&str]) -> Result<Url, AppError> {
    let mut url = Url::parse(base).map_err(|error| AppError::Internal(error.to_string()))?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| AppError::Internal("provider base URL cannot be extended".into()))?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: Url,
    api_key: Option<&str>,
) -> Result<T, AppError> {
    let mut request = client.get(url).header(header::ACCEPT, "application/json");
    if let Some(api_key) = api_key {
        request = request.header("x-api-key", api_key);
    }
    let response = request
        .send()
        .await
        .map_err(|error| AppError::Internal(format!("mod provider request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(if response.status() == reqwest::StatusCode::NOT_FOUND {
            AppError::NotFound("mods.provider_file_not_found".into())
        } else if matches!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            AppError::BadRequest("mods.provider_credentials_rejected".into())
        } else if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            AppError::TooManyRequests("mods.provider_rate_limited".into())
        } else {
            AppError::Internal(format!("mod provider returned HTTP {}", response.status()))
        });
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_PROVIDER_JSON_BYTES)
    {
        return Err(AppError::BadRequest(
            "mods.provider_response_too_large".into(),
        ));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            AppError::Internal(format!("mod provider response failed: {error}"))
        })?;
        if bytes.len().saturating_add(chunk.len()) > MAX_PROVIDER_JSON_BYTES as usize {
            return Err(AppError::BadRequest(
                "mods.provider_response_too_large".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Internal(format!("mod provider JSON is invalid: {error}")))
}

async fn get_download_response(
    client: &Client,
    provider: Provider,
    mut url: Url,
) -> Result<Response, AppError> {
    for _ in 0..=3 {
        validate_download_url(provider, &url)?;
        let response = client
            .get(url.clone())
            .header(
                header::ACCEPT,
                "application/java-archive, application/octet-stream",
            )
            .send()
            .await
            .map_err(|error| AppError::Internal(format!("mod download failed: {error}")))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| AppError::BadRequest("mods.provider_url_rejected".into()))?;
            url = url
                .join(location)
                .map_err(|_| AppError::BadRequest("mods.provider_url_rejected".into()))?;
            continue;
        }
        if !response.status().is_success() {
            return Err(
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    AppError::TooManyRequests("mods.provider_rate_limited".into())
                } else {
                    AppError::Internal(format!(
                        "mod provider download returned HTTP {}",
                        response.status()
                    ))
                },
            );
        }
        return Ok(response);
    }
    Err(AppError::BadRequest("mods.provider_redirect_limit".into()))
}

fn validate_download_url(provider: Provider, url: &Url) -> Result<(), AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::BadRequest("mods.provider_url_rejected".into()))?;
    let allowed = match provider {
        Provider::Modrinth => host == "cdn.modrinth.com",
        Provider::CurseForge => host == "forgecdn.net" || host.ends_with(".forgecdn.net"),
    };
    if url.scheme() != "https"
        || !allowed
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::BadRequest("mods.provider_url_rejected".into()));
    }
    Ok(())
}

fn validate_api_key(value: &str) -> Result<(), AppError> {
    if !(16..=512).contains(&value.len())
        || value.trim() != value
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\"' && byte != b'\\')
    {
        return Err(AppError::BadRequest("mods.invalid_api_key".into()));
    }
    Ok(())
}

fn validate_modrinth_id(value: &str) -> Result<(), AppError> {
    if !(1..=64).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(AppError::BadRequest("mods.invalid_provider_id".into()));
    }
    Ok(())
}

fn validate_curseforge_id(value: &str) -> Result<u32, AppError> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::BadRequest("mods.invalid_provider_id".into()));
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| AppError::BadRequest("mods.invalid_provider_id".into()))
}

fn validate_provider_filename(filename: &str) -> Result<(), AppError> {
    if filename.is_empty()
        || filename.chars().count() > 255
        || filename.chars().any(char::is_control)
        || Path::new(filename)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(filename)
        || !filename.to_ascii_lowercase().ends_with(".jar")
    {
        return Err(AppError::BadRequest(
            "mods.provider_file_not_supported".into(),
        ));
    }
    Ok(())
}

fn validate_hex(value: &str, length: usize, message: &'static str) -> Result<(), AppError> {
    if value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(AppError::BadRequest(message.into()))
    }
}

fn expected_modrinth_loader(profile_id: &str) -> Result<&'static str, AppError> {
    match profile_id {
        "minecraft-java-fabric" => Ok("fabric"),
        "minecraft-java-forge" => Ok("forge"),
        "minecraft-java-neoforge" => Ok("neoforge"),
        "minecraft-java-quilt" => Ok("quilt"),
        "minecraft-java-paper" => Ok("paper"),
        "minecraft-java-purpur" => Ok("purpur"),
        "minecraft-java-spigot" => Ok("spigot"),
        _ => Err(AppError::BadRequest(
            "mods.provider_not_supported_by_profile".into(),
        )),
    }
}

fn curseforge_project_path_matches(profile_id: &str, raw: Option<&str>) -> bool {
    let Some(raw) = raw else { return false };
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" || url.host_str() != Some("www.curseforge.com") {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    match profile_id {
        "hytale" => path.starts_with("/hytale/mods/"),
        "minecraft-java-paper" | "minecraft-java-purpur" | "minecraft-java-spigot" => {
            path.starts_with("/minecraft/bukkit-plugins/")
        }
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-quilt" => path.starts_with("/minecraft/mc-mods/"),
        _ => false,
    }
}

fn validate_curseforge_compatibility(
    compatibility: &Compatibility,
    game_slug: &str,
    game_versions: &[String],
) -> Result<(), AppError> {
    if compatibility.profile_id == "hytale" {
        return if game_slug.eq_ignore_ascii_case("hytale") {
            Ok(())
        } else {
            Err(AppError::BadRequest("mods.provider_incompatible".into()))
        };
    }
    if !game_slug.eq_ignore_ascii_case("minecraft") {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }
    let game_version = compatibility
        .game_version
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("mods.game_version_required".into()))?;
    if !game_versions.iter().any(|value| value == game_version) {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }
    let loader = match compatibility.profile_id.as_str() {
        "minecraft-java-fabric" => Some("fabric"),
        "minecraft-java-forge" => Some("forge"),
        "minecraft-java-neoforge" => Some("neoforge"),
        "minecraft-java-quilt" => Some("quilt"),
        "minecraft-java-paper" | "minecraft-java-purpur" | "minecraft-java-spigot" => None,
        _ => return Err(AppError::BadRequest("mods.provider_incompatible".into())),
    };
    if loader.is_some_and(|loader| {
        !game_versions
            .iter()
            .any(|value| value.eq_ignore_ascii_case(loader))
    }) {
        return Err(AppError::BadRequest("mods.provider_incompatible".into()));
    }
    Ok(())
}

fn select_modrinth_file(files: &[ModrinthFile]) -> Result<&ModrinthFile, AppError> {
    let primary = files.iter().filter(|file| file.primary).collect::<Vec<_>>();
    match primary.as_slice() {
        [file] => Ok(*file),
        [] if files.len() == 1 => Ok(&files[0]),
        _ => Err(AppError::BadRequest(
            "mods.provider_primary_file_ambiguous".into(),
        )),
    }
}

#[derive(Debug)]
struct JarDigests {
    sha1: String,
    sha256: String,
    sha512: String,
}

async fn hash_jar(root: &Path, relative: &str) -> Result<JarDigests, AppError> {
    let (mut file, expected_size) = secure_fs::open_regular_file(root, relative).await?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut signature = [0_u8; 4];
    let mut signature_len = 0_usize;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if signature_len < signature.len() {
            let copy = (signature.len() - signature_len).min(read);
            signature[signature_len..signature_len + copy].copy_from_slice(&buffer[..copy]);
            signature_len += copy;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::BadRequest("mods.provider_file_too_large".into()))?;
        if total > MAX_MOD_BYTES {
            return Err(AppError::BadRequest("mods.provider_file_too_large".into()));
        }
        sha1.update(&buffer[..read]);
        sha256.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    if total != expected_size || signature_len != 4 || signature != *b"PK\x03\x04" {
        return Err(AppError::BadRequest("mods.invalid_archive".into()));
    }
    Ok(JarDigests {
        sha1: format!("{:x}", sha1.finalize()),
        sha256: format!("{:x}", sha256.finalize()),
        sha512: format!("{:x}", sha512.finalize()),
    })
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    id: String,
    project_type: String,
    server_side: String,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    project_id: String,
    name: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    files: Vec<ModrinthFile>,
    #[serde(default)]
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    hashes: ModrinthHashes,
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    sha512: String,
}

#[derive(Debug, Deserialize)]
struct ModrinthDependency {
    version_id: Option<String>,
    project_id: Option<String>,
    dependency_type: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeEnvelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeProject {
    id: u32,
    game_id: u32,
    links: CurseForgeLinks,
    is_available: bool,
    allow_mod_distribution: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeLinks {
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CurseForgeGame {
    slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeFile {
    id: u32,
    game_id: u32,
    mod_id: u32,
    is_available: bool,
    file_name: String,
    file_length: u64,
    download_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    hashes: Vec<CurseForgeHash>,
    #[serde(default)]
    dependencies: Vec<CurseForgeDependency>,
}

#[derive(Debug, Deserialize)]
struct CurseForgeHash {
    value: String,
    algo: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeDependency {
    mod_id: u32,
    relation_type: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_and_urls_are_closed() {
        assert!(validate_modrinth_id("AAbb0123").is_ok());
        assert!(validate_modrinth_id("slug-with-dash").is_err());
        assert_eq!(validate_curseforge_id("1431902").unwrap(), 1_431_902);
        assert!(validate_curseforge_id("../1").is_err());

        let modrinth = Url::parse("https://cdn.modrinth.com/data/a/version/b.jar").unwrap();
        assert!(validate_download_url(Provider::Modrinth, &modrinth).is_ok());
        let forged = Url::parse("https://cdn.modrinth.com.example.org/a.jar").unwrap();
        assert!(validate_download_url(Provider::Modrinth, &forged).is_err());
        let curseforge = Url::parse("https://edge.forgecdn.net/files/1/2/a.jar").unwrap();
        assert!(validate_download_url(Provider::CurseForge, &curseforge).is_ok());
        let query =
            Url::parse("https://edge.forgecdn.net/a.jar?redirect=http://127.0.0.1").unwrap();
        assert!(validate_download_url(Provider::CurseForge, &query).is_err());
    }

    #[test]
    fn compatibility_checks_exact_game_version_and_loader() {
        let fabric = Compatibility {
            profile_id: "minecraft-java-fabric".into(),
            game_version: Some("1.21.8".into()),
        };
        assert!(
            validate_curseforge_compatibility(
                &fabric,
                "minecraft",
                &["1.21.8".into(), "Fabric".into()]
            )
            .is_ok()
        );
        assert!(
            validate_curseforge_compatibility(
                &fabric,
                "minecraft",
                &["1.21.7".into(), "Fabric".into()]
            )
            .is_err()
        );
        assert!(
            validate_curseforge_compatibility(
                &fabric,
                "minecraft",
                &["1.21.8".into(), "Forge".into()]
            )
            .is_err()
        );
    }

    #[test]
    fn curseforge_project_category_is_profile_specific() {
        assert!(curseforge_project_path_matches(
            "minecraft-java-paper",
            Some("https://www.curseforge.com/minecraft/bukkit-plugins/example")
        ));
        assert!(!curseforge_project_path_matches(
            "minecraft-java-paper",
            Some("https://www.curseforge.com/minecraft/mc-mods/example")
        ));
        assert!(curseforge_project_path_matches(
            "hytale",
            Some("https://www.curseforge.com/hytale/mods/example")
        ));
    }

    #[test]
    fn api_keys_reject_whitespace_and_control_characters() {
        assert!(validate_api_key("0123456789abcdef").is_ok());
        assert!(validate_api_key(" 0123456789abcdef").is_err());
        assert!(validate_api_key("short").is_err());
        assert!(validate_api_key("0123456789abcde\n").is_err());
    }

    #[tokio::test]
    #[ignore = "live pre-release smoke: queries immutable Modrinth project and version metadata"]
    async fn live_modrinth_metadata_contract_is_compatible() {
        let artifact = resolve_modrinth(
            "P7dR8mSH",
            "g58ofrov",
            &Compatibility {
                profile_id: "minecraft-java-fabric".into(),
                game_version: Some("1.21.8".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(artifact.project_id, "P7dR8mSH");
        assert_eq!(artifact.version_id, "g58ofrov");
        assert_eq!(artifact.url.host_str(), Some("cdn.modrinth.com"));
        assert!(artifact.expected_size > 0);
    }

    #[tokio::test]
    async fn curseforge_key_is_encrypted_write_only_and_removable() {
        let directory = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/mods.db?mode=rwc", directory.path().display());
        let pool = crate::core::database::init_pool(&database_url)
            .await
            .unwrap();
        crate::core::database::run_migrations(&pool).await.unwrap();
        let secrets = SecretStore::load_or_create(&directory.path().join("master.key")).unwrap();
        let api_key = "curseforge-secret-key-123456"; // gitleaks:allow

        set_curseforge_api_key(&pool, &secrets, api_key)
            .await
            .unwrap();
        assert!(curseforge_configured(&pool).await.unwrap());
        let stored: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'mods.curseforge_api_key'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!stored.contains(api_key));
        assert_eq!(curseforge_api_key(&pool, &secrets).await.unwrap(), api_key);

        clear_curseforge_api_key(&pool).await.unwrap();
        assert!(!curseforge_configured(&pool).await.unwrap());
    }
}
