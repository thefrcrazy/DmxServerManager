use std::{
    ffi::OsStr,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::VerifyingKey;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteConnectOptions;

const DEFAULT_RELEASE_CHECK_INTERVAL_SECONDS: u64 = 6 * 60 * 60;
const MIN_RELEASE_CHECK_INTERVAL_SECONDS: u64 = 15 * 60;
const MAX_RELEASE_CHECK_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const MIN_SETUP_TOKEN_BYTES: usize = 32;
const MAX_SETUP_TOKEN_BYTES: usize = 256;
const OFFICIAL_RELEASE_MANIFEST_URL: &str =
    "https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/release-manifest.json";
const OFFICIAL_RELEASE_PUBLIC_KEY: &str = include_str!("../../release-public-key.b64url");

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    #[default]
    Native,
    Docker,
}

impl std::str::FromStr for DeploymentMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "native" => Ok(Self::Native),
            "docker" => Ok(Self::Docker),
            _ => Err("expected native or docker"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReleaseCheckSettings {
    pub manifest_url: Url,
    pub public_key: [u8; 32],
    pub interval_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PinnedDownload {
    pub url: String,
    pub version: String,
    pub sha256: String,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub bind: SocketAddr,
    pub database_url: String,
    pub master_key_file: PathBuf,
    pub steamcmd_path: PathBuf,
    pub bedrock_linux_source: Option<PinnedDownload>,
    pub bedrock_windows_source: Option<PinnedDownload>,
    pub import_roots: Vec<PathBuf>,
    pub trusted_proxies: Vec<IpAddr>,
    pub reverse_proxy: bool,
    pub log: String,
    pub dev_origin: Option<String>,
    pub setup_token: Option<String>,
    pub session_ttl_hours: i64,
    pub deployment_mode: DeploymentMode,
    pub release_check: Option<ReleaseCheckSettings>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSettings {
    data_dir: Option<PathBuf>,
    static_dir: Option<PathBuf>,
    bind: Option<SocketAddr>,
    database_url: Option<String>,
    master_key_file: Option<PathBuf>,
    steamcmd_path: Option<PathBuf>,
    bedrock_linux_source: Option<PinnedDownload>,
    bedrock_windows_source: Option<PinnedDownload>,
    import_roots: Option<Vec<PathBuf>>,
    trusted_proxies: Option<Vec<IpAddr>>,
    reverse_proxy: Option<bool>,
    log: Option<String>,
    dev_origin: Option<String>,
    setup_token: Option<String>,
    session_ttl_hours: Option<i64>,
    deployment_mode: Option<DeploymentMode>,
    release_manifest_url: Option<String>,
    release_public_key: Option<String>,
    release_check_interval_seconds: Option<u64>,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let config_file = std::env::var_os("DMX_CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(default_config_file);
        let file = read_config(&config_file)?;

        let data_dir = env_path("DMX_DATA_DIR")
            .or(file.data_dir)
            .unwrap_or_else(default_data_dir);
        let static_dir = env_path("DMX_STATIC_DIR")
            .or(file.static_dir)
            .map(Ok)
            .unwrap_or_else(default_static_dir)?;
        let bind = env_parse("DMX_BIND")?
            .or(file.bind)
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 5500)));
        let database_url = std::env::var("DMX_DATABASE_URL")
            .ok()
            .or(file.database_url)
            .unwrap_or_else(|| sqlite_url(&data_dir.join("dmx-server-manager.db")));
        let master_key_file = env_path("DMX_MASTER_KEY_FILE")
            .or(file.master_key_file)
            .unwrap_or_else(|| data_dir.join("master.key"));
        let steamcmd_path = env_path("DMX_STEAMCMD_PATH")
            .or(file.steamcmd_path)
            .unwrap_or_else(default_steamcmd_path);
        if !steamcmd_path.is_absolute() {
            anyhow::bail!(
                "DMX_STEAMCMD_PATH/steamcmd_path must be an absolute path to the trusted SteamCMD executable"
            );
        }
        let bedrock_linux_source =
            pinned_download_from_env("DMX_BEDROCK_LINUX", file.bedrock_linux_source)?;
        let bedrock_windows_source =
            pinned_download_from_env("DMX_BEDROCK_WINDOWS", file.bedrock_windows_source)?;
        let import_roots = std::env::var("DMX_IMPORT_ROOTS")
            .ok()
            .map(|value| split_paths(&value))
            .or(file.import_roots)
            .unwrap_or_default();
        let trusted_proxies = std::env::var("DMX_TRUSTED_PROXIES")
            .ok()
            .map(|value| parse_ips(&value))
            .transpose()?
            .or(file.trusted_proxies)
            .unwrap_or_default();
        let reverse_proxy = env_bool("DMX_REVERSE_PROXY")?
            .or(file.reverse_proxy)
            .unwrap_or(false);
        let log = std::env::var("DMX_LOG")
            .ok()
            .or(file.log)
            .unwrap_or_else(|| "dmx_server_manager=info,tower_http=info".into());
        let dev_origin = std::env::var("DMX_DEV_ORIGIN").ok().or(file.dev_origin);
        let setup_token = std::env::var("DMX_SETUP_TOKEN")
            .ok()
            .filter(|value| !value.is_empty())
            .or(file.setup_token);
        if let Some(token) = setup_token.as_deref() {
            validate_setup_token(token)?;
        }
        let session_ttl_hours = env_parse("DMX_SESSION_TTL_HOURS")?
            .or(file.session_ttl_hours)
            .unwrap_or(24);
        if !(1..=24 * 30).contains(&session_ttl_hours) {
            anyhow::bail!("DMX_SESSION_TTL_HOURS must be between 1 and 720");
        }
        let deployment_mode = std::env::var("DMX_DEPLOYMENT_MODE")
            .ok()
            .map(|value| value.parse())
            .transpose()
            .map_err(|error| anyhow::anyhow!("invalid DMX_DEPLOYMENT_MODE: {error}"))?
            .or(file.deployment_mode)
            .unwrap_or_default();
        let env_release_manifest_url = std::env::var("DMX_RELEASE_MANIFEST_URL")
            .ok()
            .filter(|value| !value.is_empty());
        let env_release_public_key = std::env::var("DMX_RELEASE_PUBLIC_KEY")
            .ok()
            .filter(|value| !value.is_empty());
        let (release_manifest_url, release_public_key) = release_configuration_values(
            env_release_manifest_url,
            env_release_public_key,
            file.release_manifest_url,
            file.release_public_key,
        );
        let release_check_interval_seconds = env_parse("DMX_RELEASE_CHECK_INTERVAL_SECONDS")?
            .or(file.release_check_interval_seconds)
            .unwrap_or(DEFAULT_RELEASE_CHECK_INTERVAL_SECONDS);
        let release_check = release_check_settings(
            release_manifest_url,
            release_public_key,
            release_check_interval_seconds,
        )?;

        let mut settings = Self {
            config_file,
            data_dir,
            static_dir,
            bind,
            database_url,
            master_key_file,
            steamcmd_path,
            bedrock_linux_source,
            bedrock_windows_source,
            import_roots,
            trusted_proxies,
            reverse_proxy,
            log,
            dev_origin,
            setup_token,
            session_ttl_hours,
            deployment_mode,
            release_check,
        };
        settings.validate_and_normalize_import_roots()?;
        validate_bedrock_source(settings.bedrock_linux_source.as_ref(), "linux")?;
        validate_bedrock_source(settings.bedrock_windows_source.as_ref(), "win")?;
        settings.validate_runtime_security()?;
        Ok(settings)
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.data_dir.join("instances")
    }

    pub fn catalog_dir(&self) -> PathBuf {
        self.data_dir.join("catalog")
    }

    pub fn secure_cookies(&self) -> bool {
        // Plain HTTP cookies exist only for the explicitly declared loopback
        // Vite development origin. Native release installs still emit Secure,
        // even while the listener itself remains bound to loopback.
        self.dev_origin.is_none()
    }

    fn validate_runtime_security(&self) -> anyhow::Result<()> {
        if !self.bind.ip().is_loopback() && !self.reverse_proxy {
            anyhow::bail!("remote binding requires DMX_REVERSE_PROXY=true and a TLS reverse proxy");
        }
        if self.reverse_proxy && self.trusted_proxies.is_empty() {
            anyhow::bail!("DMX_REVERSE_PROXY=true requires DMX_TRUSTED_PROXIES");
        }
        if let Some(origin) = &self.dev_origin {
            if self.reverse_proxy || !self.bind.ip().is_loopback() {
                anyhow::bail!(
                    "DMX_DEV_ORIGIN is restricted to a direct loopback development listener"
                );
            }
            validate_dev_origin(origin)?;
        }
        Ok(())
    }

    fn validate_and_normalize_import_roots(&mut self) -> anyhow::Result<()> {
        let database_path = sqlite_database_path(&self.database_url)?;
        let normalized_data_dir = normalize_security_path(&self.data_dir, "data directory")?;
        let normalized_config_file =
            normalize_security_path(&self.config_file, "configuration file")?;
        let normalized_master_key =
            normalize_security_path(&self.master_key_file, "master key file")?;

        let mut sensitive_paths = vec![
            ("data directory", normalized_data_dir),
            ("configuration file", normalized_config_file),
            ("master key file", normalized_master_key.clone()),
        ];
        if let Some(database_path) = database_path {
            sensitive_paths.push((
                "SQLite database",
                normalize_security_path(&database_path, "SQLite database")?,
            ));
        }
        if let Some(secrets_directory) = normalized_master_key.parent() {
            sensitive_paths.push(("secrets directory", secrets_directory.to_path_buf()));
        }

        self.import_roots = validate_import_root_boundaries(&self.import_roots, &sensitive_paths)?;
        Ok(())
    }
}

fn sqlite_database_path(database_url: &str) -> anyhow::Result<Option<PathBuf>> {
    let database_and_query = database_url
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:");
    let (database, query) = database_and_query
        .split_once('?')
        .map_or((database_and_query, ""), |(database, query)| {
            (database, query)
        });
    let explicitly_in_memory = database == ":memory:"
        || query.split('&').any(|parameter| {
            parameter
                .split_once('=')
                .is_some_and(|(name, value)| name == "mode" && value == "memory")
        });
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(|error| anyhow::anyhow!("invalid DMX_DATABASE_URL: {error}"))?;
    if explicitly_in_memory {
        return Ok(None);
    }
    let path = options.get_filename();
    if path.as_os_str().is_empty()
        || path
            .as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("file:")
    {
        anyhow::bail!(
            "DMX_DATABASE_URL must use an unambiguous SQLite filesystem path or an explicit in-memory mode"
        );
    }
    Ok(Some(path.to_path_buf()))
}

fn validate_import_root_boundaries(
    import_roots: &[PathBuf],
    sensitive_paths: &[(&str, PathBuf)],
) -> anyhow::Result<Vec<PathBuf>> {
    let mut normalized_roots = Vec::with_capacity(import_roots.len());
    for configured_root in import_roots {
        if !configured_root.is_absolute() {
            anyhow::bail!("DMX_IMPORT_ROOTS entries must be absolute paths");
        }
        let root = normalize_security_path(configured_root, "import root")?;
        for (label, sensitive_path) in sensitive_paths {
            if paths_overlap(&root, sensitive_path) {
                anyhow::bail!(
                    "DMX_IMPORT_ROOTS entry {} overlaps the internal {label}",
                    configured_root.display()
                );
            }
        }
        normalized_roots.push(root);
    }
    Ok(normalized_roots)
}

/// Resolve every existing prefix (including symlinks) while retaining a normalized suffix for
/// paths that are intentionally provisioned later. Ambiguous dot components are rejected instead
/// of being interpreted lexically, because symlink + `..` semantics are platform-dependent.
fn normalize_security_path(path: &Path, label: &str) -> anyhow::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    validate_unambiguous_components(path, label)?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let mut existing_prefix = absolute.clone();
    let mut missing_suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&existing_prefix) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = existing_prefix.file_name().ok_or_else(|| {
                    anyhow::anyhow!("cannot normalize missing {label}: {}", path.display())
                })?;
                missing_suffix.push(component.to_os_string());
                if !existing_prefix.pop() {
                    anyhow::bail!("cannot normalize {label}: {}", path.display());
                }
            }
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "cannot inspect {label} {}: {error}",
                    path.display()
                ));
            }
        }
    }

    let mut normalized = fs::canonicalize(&existing_prefix).map_err(|error| {
        anyhow::anyhow!("cannot canonicalize {label} {}: {error}", path.display())
    })?;
    for component in missing_suffix.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

fn validate_unambiguous_components(path: &Path, label: &str) -> anyhow::Result<()> {
    for component in path.components() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            anyhow::bail!("{label} must not contain . or .. components");
        }
        #[cfg(windows)]
        if let Component::Normal(value) = component {
            let value = value.to_string_lossy();
            if value.ends_with('.') || value.ends_with(' ') || value.contains(':') {
                anyhow::bail!("{label} contains a path component that is ambiguous on Windows");
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    is_same_or_descendant(left, right) || is_same_or_descendant(right, left)
}

fn is_same_or_descendant(path: &Path, ancestor: &Path) -> bool {
    let mut path_components = path.components();
    ancestor.components().all(|ancestor_component| {
        path_components
            .next()
            .is_some_and(|path_component| components_equal(path_component, ancestor_component))
    })
}

fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    os_strings_equal(left.as_os_str(), right.as_os_str())
}

#[cfg(not(windows))]
fn os_strings_equal(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

#[cfg(windows)]
fn os_strings_equal(left: &OsStr, right: &OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn release_configuration_values(
    env_manifest_url: Option<String>,
    env_public_key: Option<String>,
    file_manifest_url: Option<String>,
    file_public_key: Option<String>,
) -> (Option<String>, Option<String>) {
    match (env_manifest_url, env_public_key) {
        (None, None) => match (file_manifest_url, file_public_key) {
            (None, None) => (
                Some(OFFICIAL_RELEASE_MANIFEST_URL.to_owned()),
                Some(OFFICIAL_RELEASE_PUBLIC_KEY.trim().to_owned()),
            ),
            configured => configured,
        },
        configured => configured,
    }
}

fn validate_dev_origin(value: &str) -> anyhow::Result<()> {
    let origin =
        Url::parse(value).map_err(|error| anyhow::anyhow!("invalid DMX_DEV_ORIGIN: {error}"))?;
    let host_is_loopback = origin.host_str() == Some("localhost")
        || origin
            .host_str()
            .map(|host| {
                host.strip_prefix('[')
                    .and_then(|value| value.strip_suffix(']'))
                    .unwrap_or(host)
            })
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
    if origin.scheme() != "http"
        || !host_is_loopback
        || origin.username() != ""
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        anyhow::bail!(
            "DMX_DEV_ORIGIN must be an exact HTTP loopback origin without credentials, path, query or fragment"
        );
    }
    Ok(())
}

fn validate_setup_token(value: &str) -> anyhow::Result<()> {
    if !(MIN_SETUP_TOKEN_BYTES..=MAX_SETUP_TOKEN_BYTES).contains(&value.len()) {
        anyhow::bail!(
            "DMX_SETUP_TOKEN/setup_token must be between {MIN_SETUP_TOKEN_BYTES} and {MAX_SETUP_TOKEN_BYTES} bytes"
        );
    }
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        anyhow::bail!(
            "DMX_SETUP_TOKEN/setup_token must not contain whitespace or control characters"
        );
    }
    Ok(())
}

fn release_check_settings(
    manifest_url: Option<String>,
    public_key: Option<String>,
    interval_seconds: u64,
) -> anyhow::Result<Option<ReleaseCheckSettings>> {
    if !(MIN_RELEASE_CHECK_INTERVAL_SECONDS..=MAX_RELEASE_CHECK_INTERVAL_SECONDS)
        .contains(&interval_seconds)
    {
        anyhow::bail!("DMX_RELEASE_CHECK_INTERVAL_SECONDS must be between 900 and 86400");
    }
    let (manifest_url, public_key) = match (manifest_url, public_key) {
        (None, None) => return Ok(None),
        (Some(url), Some(key)) if !url.is_empty() && !key.is_empty() => (url, key),
        _ => anyhow::bail!(
            "DMX_RELEASE_MANIFEST_URL and DMX_RELEASE_PUBLIC_KEY must be configured together"
        ),
    };
    let manifest_url = Url::parse(&manifest_url)
        .map_err(|_| anyhow::anyhow!("DMX_RELEASE_MANIFEST_URL is invalid"))?;
    if !is_safe_release_url(&manifest_url, false)
        || !manifest_url.path().ends_with("/release-manifest.json")
    {
        anyhow::bail!(
            "DMX_RELEASE_MANIFEST_URL must be an official HTTPS URL ending in /release-manifest.json"
        );
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(public_key)
        .map_err(|_| anyhow::anyhow!("DMX_RELEASE_PUBLIC_KEY must be base64url without padding"))?;
    let public_key: [u8; 32] = decoded.try_into().map_err(|_| {
        anyhow::anyhow!("DMX_RELEASE_PUBLIC_KEY must encode exactly 32 Ed25519 bytes")
    })?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| anyhow::anyhow!("DMX_RELEASE_PUBLIC_KEY is not a valid Ed25519 public key"))?;
    if verifying_key.is_weak() {
        anyhow::bail!("DMX_RELEASE_PUBLIC_KEY must not be a weak Ed25519 public key");
    }
    Ok(Some(ReleaseCheckSettings {
        manifest_url,
        public_key,
        interval_seconds,
    }))
}

pub(crate) fn is_safe_release_url(url: &Url, allow_signed_query: bool) -> bool {
    let host = url.host_str();
    let allowed_host = matches!(
        host,
        Some(
            "github.com"
                | "raw.githubusercontent.com"
                | "objects.githubusercontent.com"
                | "release-assets.githubusercontent.com"
                | "thefrcrazy.github.io"
        )
    );
    url.scheme() == "https"
        && allowed_host
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && (allow_signed_query || url.query().is_none())
        && url.fragment().is_none()
        && url.as_str().len() <= 4_096
}

fn validate_bedrock_source(
    source: Option<&PinnedDownload>,
    platform: &'static str,
) -> anyhow::Result<()> {
    let Some(source) = source else {
        return Ok(());
    };
    if source.version.is_empty()
        || source.version.len() > 64
        || !source
            .version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        anyhow::bail!("DMX_BEDROCK_{platform}_VERSION is invalid");
    }
    if source.sha256.len() != 64 || !source.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("DMX_BEDROCK_{platform}_SHA256 must be an exact SHA-256 digest");
    }
    if source.size_bytes == Some(0)
        || source
            .size_bytes
            .is_some_and(|size| size > 4 * 1024 * 1024 * 1024)
    {
        anyhow::bail!("DMX_BEDROCK_{platform}_SIZE_BYTES is invalid");
    }
    let url = Url::parse(&source.url)
        .map_err(|_| anyhow::anyhow!("DMX_BEDROCK_{platform}_URL is invalid"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(
            url.host_str(),
            Some("www.minecraft.net" | "minecraft.net" | "minecraft.azureedge.net" | "aka.ms")
        )
    {
        anyhow::bail!("DMX_BEDROCK_{platform}_URL must use an explicit official HTTPS host");
    }
    Ok(())
}

fn read_config(path: &PathBuf) -> anyhow::Result<FileSettings> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents)
            .map_err(|error| anyhow::anyhow!("invalid {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileSettings::default()),
        Err(error) => Err(error.into()),
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn env_bool(name: &str) -> anyhow::Result<Option<bool>> {
    std::env::var(name)
        .ok()
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" => Ok(false),
            _ => anyhow::bail!("{name} must be true or false"),
        })
        .transpose()
}

fn pinned_download_from_env(
    prefix: &str,
    fallback: Option<PinnedDownload>,
) -> anyhow::Result<Option<PinnedDownload>> {
    let url = std::env::var(format!("{prefix}_URL")).ok();
    let version = std::env::var(format!("{prefix}_VERSION")).ok();
    let sha256 = std::env::var(format!("{prefix}_SHA256")).ok();
    let size_bytes = env_parse::<u64>(&format!("{prefix}_SIZE_BYTES"))?;
    if url.is_none() && version.is_none() && sha256.is_none() && size_bytes.is_none() {
        return Ok(fallback);
    }
    Ok(Some(PinnedDownload {
        url: url.ok_or_else(|| anyhow::anyhow!("{prefix}_URL is required"))?,
        version: version.ok_or_else(|| anyhow::anyhow!("{prefix}_VERSION is required"))?,
        sha256: sha256.ok_or_else(|| anyhow::anyhow!("{prefix}_SHA256 is required"))?,
        size_bytes,
    }))
}

fn env_parse<T>(name: &str) -> anyhow::Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .map_err(|error| anyhow::anyhow!("invalid {name}: {error}"))
        })
        .transpose()
}

fn split_paths(value: &str) -> Vec<PathBuf> {
    value
        .split(if cfg!(windows) { ';' } else { ':' })
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn parse_ips(value: &str) -> anyhow::Result<Vec<IpAddr>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse()
                .map_err(|error| anyhow::anyhow!("invalid trusted proxy {part}: {error}"))
        })
        .collect()
}

fn sqlite_url(path: &std::path::Path) -> String {
    format!("sqlite:{}?mode=rwc", path.display())
}

fn default_config_file() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"));
        base.join("DmxServerManager").join("config.toml")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/dmx-server-manager/config.toml")
    }
}

fn default_data_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        return PathBuf::from("data");
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"));
        base.join("DmxServerManager")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/dmx-server-manager")
    }
}

fn default_static_dir() -> anyhow::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let parent = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot determine the executable directory"))?;
    Ok(parent.join("static"))
}

fn default_steamcmd_path() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"));
        base.join("DmxServerManager")
            .join("data")
            .join("toolchains")
            .join("steamcmd")
            .join("steamcmd.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/usr/games/steamcmd")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_root_test_settings(root: &Path) -> Settings {
        Settings {
            config_file: root.join("configuration/config.toml"),
            data_dir: root.join("internal/data"),
            static_dir: root.join("static"),
            bind: SocketAddr::from(([127, 0, 0, 1], 5500)),
            database_url: sqlite_url(&root.join("database/panel.db")),
            master_key_file: root.join("secrets/master.key"),
            steamcmd_path: root.join("bin/steamcmd"),
            bedrock_linux_source: None,
            bedrock_windows_source: None,
            import_roots: Vec::new(),
            trusted_proxies: Vec::new(),
            reverse_proxy: false,
            log: "error".into(),
            dev_origin: None,
            setup_token: None,
            session_ttl_hours: 24,
            deployment_mode: DeploymentMode::Native,
            release_check: None,
        }
    }

    #[test]
    fn default_steamcmd_path_is_absolute() {
        assert!(default_steamcmd_path().is_absolute());
    }

    #[test]
    fn development_origin_is_exact_and_loopback_only() {
        for valid in [
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "http://[::1]:5173",
        ] {
            assert!(validate_dev_origin(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "https://localhost:5173",
            "http://example.com:5173",
            "http://localhost:5173/path",
            "http://localhost:5173/?token=secret",
            "http://user@localhost:5173",
        ] {
            assert!(validate_dev_origin(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn setup_tokens_require_bounded_non_whitespace_bytes() {
        assert!(validate_setup_token(&"a".repeat(MIN_SETUP_TOKEN_BYTES)).is_ok());
        assert!(validate_setup_token(&"a".repeat(MAX_SETUP_TOKEN_BYTES)).is_ok());

        let too_short = "a".repeat(MIN_SETUP_TOKEN_BYTES - 1);
        let too_long = "a".repeat(MAX_SETUP_TOKEN_BYTES + 1);
        let length_error = validate_setup_token(&too_short).unwrap_err().to_string();
        assert!(length_error.contains("between 32 and 256 bytes"));
        assert!(validate_setup_token(&too_long).is_err());

        for invalid in [
            format!("{} ", "a".repeat(MIN_SETUP_TOKEN_BYTES - 1)),
            format!("{}\n", "a".repeat(MIN_SETUP_TOKEN_BYTES - 1)),
            format!("{}\u{2003}", "a".repeat(MIN_SETUP_TOKEN_BYTES)),
            format!("{}\u{0000}", "a".repeat(MIN_SETUP_TOKEN_BYTES)),
        ] {
            let error = validate_setup_token(&invalid).unwrap_err().to_string();
            assert!(
                error.contains("must not contain whitespace or control characters"),
                "unexpected validation error: {error}"
            );
        }
    }

    #[test]
    fn parses_path_lists_without_empty_entries() {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let value = format!("one{separator}{separator}two");
        assert_eq!(
            split_paths(&value),
            vec![PathBuf::from("one"), PathBuf::from("two")]
        );
    }

    #[test]
    fn import_roots_cannot_be_parents_equal_to_or_children_of_internal_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let base = import_root_test_settings(root);

        for forbidden in [
            root.to_path_buf(),
            base.data_dir.clone(),
            base.data_dir.join("operator-visible"),
            PathBuf::from(
                SqliteConnectOptions::from_str(&base.database_url)
                    .unwrap()
                    .get_filename(),
            ),
            base.config_file.clone(),
            base.master_key_file.clone(),
            base.master_key_file.parent().unwrap().to_path_buf(),
            base.master_key_file
                .parent()
                .unwrap()
                .join("operator-visible"),
        ] {
            let mut settings = base.clone();
            settings.import_roots = vec![forbidden.clone()];
            assert!(
                settings.validate_and_normalize_import_roots().is_err(),
                "unsafe import root was accepted: {}",
                forbidden.display()
            );
        }

        let mut settings = base;
        settings.import_roots = vec![root.join("imports")];
        settings.validate_and_normalize_import_roots().unwrap();
        assert!(settings.import_roots[0].is_absolute());
    }

    #[test]
    fn relative_import_roots_are_rejected_at_startup() {
        let temporary = tempfile::tempdir().unwrap();
        let mut settings = import_root_test_settings(temporary.path());
        settings.import_roots = vec![PathBuf::from("imports")];
        assert!(settings.validate_and_normalize_import_roots().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn import_root_normalization_resolves_existing_symlink_prefixes() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let internal = temporary.path().join("internal");
        fs::create_dir_all(&internal).unwrap();
        let alias = temporary.path().join("alias");
        symlink(&internal, &alias).unwrap();

        let sensitive = normalize_security_path(&internal.join("data"), "data").unwrap();
        let result = validate_import_root_boundaries(
            &[alias.join("data/operator-visible")],
            &[("data directory", sensitive)],
        );
        assert!(result.is_err());
    }

    #[test]
    fn parses_only_literal_proxy_addresses() {
        assert!(parse_ips("127.0.0.1, ::1").is_ok());
        assert!(parse_ips("10.0.0.0/8").is_err());
    }

    #[test]
    fn pinned_download_configuration_is_all_or_nothing() {
        let fallback = PinnedDownload {
            url: "https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.zip".into(),
            version: "1.2.3".into(),
            sha256: "a".repeat(64),
            size_bytes: Some(42),
        };
        assert_eq!(
            pinned_download_from_env("DMX_TEST_BEDROCK_UNSET", Some(fallback.clone())).unwrap(),
            Some(fallback)
        );
    }

    #[test]
    fn bedrock_sources_require_official_https_and_exact_integrity_metadata() {
        let valid = PinnedDownload {
            url: "https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.zip".into(),
            version: "1.2.3".into(),
            sha256: "a".repeat(64),
            size_bytes: Some(42),
        };
        assert!(validate_bedrock_source(Some(&valid), "linux").is_ok());
        for invalid in [
            PinnedDownload {
                url: "http://www.minecraft.net/archive.zip".into(),
                ..valid.clone()
            },
            PinnedDownload {
                url: "https://evil.example/archive.zip".into(),
                ..valid.clone()
            },
            PinnedDownload {
                sha256: "not-a-digest".into(),
                ..valid.clone()
            },
            PinnedDownload {
                version: String::new(),
                ..valid.clone()
            },
        ] {
            assert!(validate_bedrock_source(Some(&invalid), "linux").is_err());
        }
    }

    #[test]
    fn release_configuration_is_paired_and_rejects_ssrf_urls() {
        let public_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32])
            .verifying_key()
            .to_bytes();
        let key = URL_SAFE_NO_PAD.encode(public_key);
        let valid = release_check_settings(
            Some("https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/release-manifest.json".into()),
            Some(key.clone()),
            3_600,
        )
        .unwrap()
        .unwrap();
        assert_eq!(valid.public_key, public_key);
        assert!(
            release_check_settings(
                Some("https://127.0.0.1/release-manifest.json".into()),
                Some(key.clone()),
                3_600
            )
            .is_err()
        );
        assert!(
            release_check_settings(
                Some("https://github.com.evil.example/release-manifest.json".into()),
                Some(key.clone()),
                3_600
            )
            .is_err()
        );
        assert!(
            release_check_settings(
                Some("https://github.com/release-manifest.json?token=secret".into()),
                Some(key.clone()),
                3_600
            )
            .is_err()
        );
        assert!(
            release_check_settings(
                Some("https://github.com/release-manifest.json".into()),
                None,
                3_600
            )
            .is_err()
        );
        assert!(release_check_settings(None, None, 60).is_err());

        let mut weak_key = [0_u8; 32];
        weak_key[0] = 1;
        assert!(
            release_check_settings(
                Some("https://github.com/release-manifest.json".into()),
                Some(URL_SAFE_NO_PAD.encode(weak_key)),
                3_600,
            )
            .is_err()
        );
    }

    #[test]
    fn official_release_defaults_apply_only_without_an_override() {
        let expected_key = URL_SAFE_NO_PAD
            .decode(OFFICIAL_RELEASE_PUBLIC_KEY.trim())
            .unwrap();
        assert_eq!(expected_key.len(), 32);

        let defaults = release_configuration_values(None, None, None, None);
        let defaults = release_check_settings(defaults.0, defaults.1, 3_600)
            .unwrap()
            .unwrap();
        assert_eq!(
            defaults.manifest_url.as_str(),
            OFFICIAL_RELEASE_MANIFEST_URL
        );
        assert_eq!(defaults.public_key.as_slice(), expected_key.as_slice());

        let file_url = "https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/release-manifest.json";
        let file_key = URL_SAFE_NO_PAD.encode(
            ed25519_dalek::SigningKey::from_bytes(&[8_u8; 32])
                .verifying_key()
                .to_bytes(),
        );
        let configured =
            release_configuration_values(None, None, Some(file_url.into()), Some(file_key.clone()));
        assert_eq!(configured, (Some(file_url.into()), Some(file_key)));
    }

    #[test]
    fn release_overrides_are_never_completed_from_another_source_or_defaults() {
        let file_key = URL_SAFE_NO_PAD.encode(
            ed25519_dalek::SigningKey::from_bytes(&[9_u8; 32])
                .verifying_key()
                .to_bytes(),
        );
        let partial_env = release_configuration_values(
            Some("https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/release-manifest.json".into()),
            None,
            Some(OFFICIAL_RELEASE_MANIFEST_URL.into()),
            Some(file_key),
        );
        assert!(release_check_settings(partial_env.0, partial_env.1, 3_600).is_err());

        let partial_file = release_configuration_values(
            None,
            None,
            Some(OFFICIAL_RELEASE_MANIFEST_URL.into()),
            None,
        );
        assert!(release_check_settings(partial_file.0, partial_file.1, 3_600).is_err());
    }
}
