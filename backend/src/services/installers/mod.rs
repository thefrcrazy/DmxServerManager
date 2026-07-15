mod archive;
mod bedrock;
#[allow(dead_code)]
pub mod hytale;
mod minecraft;
mod minecraft_loaders;
mod toolchains;

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(test)]
use std::{collections::BTreeMap, sync::Arc};

use futures::StreamExt;
use md5::Md5;
use reqwest::{Client, Url, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use tokio::io::AsyncWriteExt;

use crate::{
    core::{Settings, config::PinnedDownload, error::AppError},
    domain::v1::{GameProfile, ProfileKind, StopStrategy, safe_join},
};

pub use archive::{ArchiveLimits, extract_zip};

pub const USER_AGENT: &str = concat!(
    "DmxServerManager/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/thefrcrazy/DmxServerManager)"
);
const MAX_PROVIDER_JSON_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("{client_message}")]
pub struct InstallerError {
    pub code: &'static str,
    pub client_message: &'static str,
    pub internal: Option<String>,
}

impl InstallerError {
    pub fn new(code: &'static str, client_message: &'static str) -> Self {
        Self {
            code,
            client_message,
            internal: None,
        }
    }

    pub fn internal(code: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            code,
            client_message: "servers.installation_failed",
            internal: Some(error.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallerSources {
    pub minecraft_manifest: Url,
    pub paper_api_base: Url,
    pub fabric_meta_base: Url,
    pub fabric_maven_base: Url,
    pub forge_maven_base: Url,
    pub neoforge_maven_base: Url,
    pub purpur_api_base: Url,
    pub quilt_meta_base: Url,
    pub quilt_maven_base: Url,
    pub buildtools: VerifiedSource,
    pub hytale_downloader: Url,
    pub adoptium_api_base: Url,
    /// Mojang does not publish a stable, checksummed Bedrock download API.
    /// These remain absent in the production catalog until such a contract exists.
    pub bedrock_linux: Option<VerifiedSource>,
    pub bedrock_windows: Option<VerifiedSource>,
    allowed_hosts: BTreeSet<String>,
    allow_loopback_http: bool,
}

impl InstallerSources {
    pub fn official() -> Self {
        Self {
            minecraft_manifest: Url::parse(
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            )
            .expect("constant Mojang manifest URL is valid"),
            paper_api_base: Url::parse("https://fill.papermc.io/v3/")
                .expect("constant Paper API URL is valid"),
            fabric_meta_base: Url::parse("https://meta.fabricmc.net/v2/")
                .expect("constant Fabric Meta API URL is valid"),
            fabric_maven_base: Url::parse("https://maven.fabricmc.net/")
                .expect("constant Fabric Maven URL is valid"),
            forge_maven_base: Url::parse("https://maven.minecraftforge.net/")
                .expect("constant Forge Maven URL is valid"),
            neoforge_maven_base: Url::parse("https://maven.neoforged.net/releases/")
                .expect("constant NeoForge Maven URL is valid"),
            purpur_api_base: Url::parse("https://api.purpurmc.org/v2/")
                .expect("constant Purpur API URL is valid"),
            quilt_meta_base: Url::parse("https://meta.quiltmc.org/v3/")
                .expect("constant Quilt Meta API URL is valid"),
            quilt_maven_base: Url::parse("https://maven.quiltmc.org/repository/release/")
                .expect("constant Quilt Maven URL is valid"),
            // BuildTools publishes no signed digest manifest. This maintainer
            // pin is tied to immutable Jenkins build #200; updates must change
            // the URL, SHA-256, size and regression test together.
            buildtools: VerifiedSource {
                url: Url::parse(
                    "https://hub.spigotmc.org/jenkins/job/BuildTools/200/artifact/target/BuildTools.jar",
                )
                .expect("constant BuildTools URL is valid"),
                sha256: "b61fa90158f594ee95bea1a27399eb64d439b4c8ae9345bd4476a02ce49b06ff"
                    .to_string(),
                size: Some(3_606_248),
                version: "jenkins-200".to_string(),
            },
            hytale_downloader: Url::parse("https://downloader.hytale.com/hytale-downloader.zip")
                .expect("constant Hytale downloader URL is valid"),
            adoptium_api_base: Url::parse("https://api.adoptium.net/v3/")
                .expect("constant Adoptium API URL is valid"),
            bedrock_linux: None,
            bedrock_windows: None,
            allowed_hosts: [
                "piston-meta.mojang.com",
                "piston-data.mojang.com",
                "launcher.mojang.com",
                "fill.papermc.io",
                "fill-data.papermc.io",
                "meta.fabricmc.net",
                "maven.fabricmc.net",
                "maven.minecraftforge.net",
                "maven.neoforged.net",
                "api.purpurmc.org",
                "meta.quiltmc.org",
                "maven.quiltmc.org",
                "hub.spigotmc.org",
                "downloader.hytale.com",
                "api.adoptium.net",
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
                "www.minecraft.net",
                "minecraft.net",
                "minecraft.azureedge.net",
                "aka.ms",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            allow_loopback_http: false,
        }
    }

    #[cfg(test)]
    fn fixture(base: &Url) -> Self {
        let host = base.host_str().unwrap().to_string();
        Self {
            minecraft_manifest: base.join("minecraft/manifest.json").unwrap(),
            paper_api_base: base.join("paper/").unwrap(),
            fabric_meta_base: base.join("fabric-meta/").unwrap(),
            fabric_maven_base: base.join("fabric-maven/").unwrap(),
            forge_maven_base: base.join("forge-maven/").unwrap(),
            neoforge_maven_base: base.join("neoforge-maven/").unwrap(),
            purpur_api_base: base.join("purpur/").unwrap(),
            quilt_meta_base: base.join("quilt-meta/").unwrap(),
            quilt_maven_base: base.join("quilt-maven/").unwrap(),
            buildtools: VerifiedSource {
                url: base.join("spigot/BuildTools.jar").unwrap(),
                sha256: "0".repeat(64),
                size: None,
                version: "fixture".to_string(),
            },
            hytale_downloader: base.join("hytale/downloader.zip").unwrap(),
            adoptium_api_base: base.join("adoptium/").unwrap(),
            bedrock_linux: None,
            bedrock_windows: None,
            allowed_hosts: [host].into_iter().collect(),
            allow_loopback_http: true,
        }
    }

    fn validate_url(&self, url: &Url) -> Result<(), InstallerError> {
        let host = url.host_str().ok_or_else(|| {
            InstallerError::new("provider_url_invalid", "servers.provider_response_invalid")
        })?;
        let secure = url.scheme() == "https";
        let loopback_fixture = self.allow_loopback_http
            && url.scheme() == "http"
            && host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if (!secure && !loopback_fixture)
            || !self.allowed_hosts.contains(host)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(InstallerError::new(
                "provider_url_rejected",
                "servers.provider_response_invalid",
            ));
        }
        Ok(())
    }

    fn with_bedrock_configuration(
        mut self,
        linux: Option<&PinnedDownload>,
        windows: Option<&PinnedDownload>,
    ) -> Result<Self, InstallerError> {
        self.bedrock_linux = linux
            .map(|source| configured_bedrock_source(source, "linux"))
            .transpose()?;
        self.bedrock_windows = windows
            .map(|source| configured_bedrock_source(source, "win"))
            .transpose()?;
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct InstallContext {
    pub client: Client,
    pub sources: InstallerSources,
    pub archive_limits: ArchiveLimits,
    pub toolchain_root: Option<PathBuf>,
    bedrock_archive: Option<bedrock::LocalArchive>,
    #[cfg(test)]
    fixture_responses: Option<Arc<BTreeMap<String, Vec<u8>>>>,
}

impl InstallContext {
    pub fn official() -> Result<Self, InstallerError> {
        Self::with_sources(InstallerSources::official())
    }

    pub fn official_with_bedrock(settings: &Settings) -> Result<Self, InstallerError> {
        Self::with_sources(InstallerSources::official().with_bedrock_configuration(
            settings.bedrock_linux_source.as_ref(),
            settings.bedrock_windows_source.as_ref(),
        )?)
    }

    pub fn with_sources(sources: InstallerSources) -> Result<Self, InstallerError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(120))
            .timeout(Duration::from_secs(4 * 60 * 60))
            .build()
            .map_err(|error| InstallerError::internal("http_client_failed", error))?;
        Ok(Self {
            client,
            sources,
            archive_limits: ArchiveLimits::default(),
            toolchain_root: None,
            bedrock_archive: None,
            #[cfg(test)]
            fixture_responses: None,
        })
    }

    pub fn with_toolchain_root(mut self, root: PathBuf) -> Self {
        self.toolchain_root = Some(root);
        self
    }

    pub async fn with_bedrock_upload(
        mut self,
        instance_root: &Path,
        job_id: &str,
        expected_version: &str,
    ) -> Result<Self, InstallerError> {
        self.bedrock_archive =
            bedrock::load_local_archive(instance_root, job_id, expected_version).await?;
        Ok(self)
    }

    #[cfg(test)]
    fn with_fixture_responses(
        sources: InstallerSources,
        responses: BTreeMap<String, Vec<u8>>,
    ) -> Result<Self, InstallerError> {
        let mut context = Self::with_sources(sources)?;
        context.fixture_responses = Some(Arc::new(responses));
        Ok(context)
    }
}

pub async fn store_bedrock_upload<S, E>(
    instance_root: &Path,
    job_id: &str,
    expected_version: &str,
    expected_sha256: &str,
    stream: S,
) -> Result<InstalledArtifact, InstallerError>
where
    S: futures::Stream<Item = Result<axum::body::Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    bedrock::store_local_archive(
        instance_root,
        job_id,
        expected_version,
        expected_sha256,
        stream,
    )
    .await
}

pub async fn remove_bedrock_upload(
    instance_root: &Path,
    job_id: &str,
) -> Result<(), InstallerError> {
    bedrock::remove_local_archive(instance_root, job_id).await
}

fn configured_bedrock_source(
    source: &PinnedDownload,
    platform: &'static str,
) -> Result<VerifiedSource, InstallerError> {
    let url = Url::parse(&source.url).map_err(|_| {
        InstallerError::new(
            "bedrock_source_url_invalid",
            "servers.bedrock_source_configuration_invalid",
        )
    })?;
    bedrock::validate_configured_source(&url, &source.version, &source.sha256, platform)?;
    if source.size_bytes == Some(0)
        || source
            .size_bytes
            .is_some_and(|size| size > 4 * 1024 * 1024 * 1024)
    {
        return Err(InstallerError::new(
            "bedrock_source_size_invalid",
            "servers.bedrock_source_configuration_invalid",
        ));
    }
    Ok(VerifiedSource {
        url,
        sha256: source.sha256.to_ascii_lowercase(),
        size: source.size_bytes,
        version: source.version.clone(),
    })
}

#[derive(Debug, Clone)]
pub struct VerifiedSource {
    pub url: Url,
    pub sha256: String,
    pub size: Option<u64>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallerExecutable {
    ManagedJava { major: u16 },
    InstanceRelative { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerPlan {
    pub executable: InstallerExecutable,
    pub cwd_relative: String,
    pub args: Vec<String>,
    pub env: Vec<(OsString, OsString)>,
    pub stop: StopStrategy,
    pub restart_exit_codes: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    pub plan: InstallerPlan,
    pub installed_version: String,
    pub installed_build: Option<String>,
    pub artifacts: Vec<InstalledArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledArtifact {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallationMarker {
    schema: u8,
    profile_id: String,
    installed_version: String,
    installed_build: Option<String>,
    required_java_major: Option<u16>,
    artifacts: Vec<InstalledArtifact>,
}

#[derive(Debug, Clone)]
enum ExpectedDigest {
    Md5(String),
    Sha1(String),
    Sha256(String),
}

#[derive(Debug, Clone)]
struct DownloadedFile {
    md5: String,
    sha1: String,
    sha256: String,
    size: u64,
}

pub fn native_install_supported(profile_id: &str) -> bool {
    matches!(
        profile_id,
        "hytale"
            | "minecraft-java-vanilla"
            | "minecraft-java-paper"
            | "minecraft-java-fabric"
            | "minecraft-java-forge"
            | "minecraft-java-neoforge"
            | "minecraft-java-spigot"
            | "minecraft-java-purpur"
            | "minecraft-java-quilt"
            | "minecraft-bedrock"
    )
}

pub async fn install_hytale_downloaded(
    settings: &serde_json::Value,
    instance_root: &Path,
    archive: &Path,
    staging: &Path,
    context: &InstallContext,
    installed_version: String,
    downloader_artifact: InstalledArtifact,
) -> Result<InstallResult, InstallerError> {
    let result = hytale::install_downloaded_archive(
        settings,
        instance_root,
        archive,
        staging,
        context,
        installed_version,
        downloader_artifact,
    )
    .await?;
    if let Some(root) = &context.toolchain_root {
        toolchains::ensure_java(root, 25, context).await?;
    } else {
        return Err(InstallerError::new(
            "java_toolchain_root_missing",
            "servers.java_runtime_unavailable",
        ));
    }
    write_installation_marker("hytale", staging, &result).await?;
    Ok(result)
}

pub async fn install_native(
    profile_id: &str,
    settings: &serde_json::Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    let result = match profile_id {
        "minecraft-java-vanilla" => {
            minecraft::install_vanilla(settings, instance_root, staging, context).await
        }
        "minecraft-java-paper" => {
            minecraft::install_paper(settings, instance_root, staging, context).await
        }
        "minecraft-java-fabric" => {
            minecraft_loaders::install_fabric(settings, instance_root, staging, context).await
        }
        "minecraft-java-forge" => {
            minecraft_loaders::install_forge(settings, instance_root, staging, context).await
        }
        "minecraft-java-neoforge" => {
            minecraft_loaders::install_neoforge(settings, instance_root, staging, context).await
        }
        "minecraft-java-spigot" => {
            minecraft_loaders::install_spigot(settings, instance_root, staging, context).await
        }
        "minecraft-java-purpur" => {
            minecraft_loaders::install_purpur(settings, instance_root, staging, context).await
        }
        "minecraft-java-quilt" => {
            minecraft_loaders::install_quilt(settings, instance_root, staging, context).await
        }
        "minecraft-bedrock" => {
            bedrock::install_bedrock(settings, instance_root, staging, context).await
        }
        "hytale" => Err(InstallerError::new(
            "interactive_installer_not_available",
            "servers.hytale_interactive_install_not_available",
        )),
        _ => Err(InstallerError::new(
            "installer_not_implemented",
            "servers.installer_not_implemented",
        )),
    }?;
    if let InstallerExecutable::ManagedJava { major } = result.plan.executable
        && let Some(root) = &context.toolchain_root
    {
        toolchains::ensure_java(root, major, context).await?;
    }
    write_installation_marker(profile_id, staging, &result).await?;
    Ok(result)
}

/// Validates and adopts files imported outside the managed installer. Provider
/// metadata is still consulted to select the exact Java runtime; imported files
/// never get to choose a host executable or toolchain path.
pub async fn adopt_imported(
    profile_id: &str,
    settings: &serde_json::Value,
    game_root: &Path,
    context: &InstallContext,
) -> Result<Option<InstallResult>, InstallerError> {
    if !matches!(
        profile_id,
        "minecraft-java-vanilla"
            | "minecraft-java-paper"
            | "minecraft-java-fabric"
            | "minecraft-java-forge"
            | "minecraft-java-neoforge"
            | "minecraft-java-spigot"
            | "minecraft-java-purpur"
            | "minecraft-java-quilt"
    ) {
        return Ok(None);
    }
    let (installed_version, java_major) =
        minecraft::imported_runtime(settings, game_root, context).await?;
    if let Some(root) = &context.toolchain_root {
        toolchains::ensure_java(root, java_major, context).await?;
    }
    let (plan, installed_build) = if matches!(
        profile_id,
        "minecraft-java-vanilla" | "minecraft-java-paper"
    ) {
        let metadata = tokio::fs::symlink_metadata(game_root.join("server.jar"))
            .await
            .map_err(|error| InstallerError::internal("import_executable_missing", error))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(InstallerError::new(
                "import_executable_invalid",
                "imports.executable_invalid",
            ));
        }
        (minecraft::launch_plan(settings, java_major)?, None)
    } else {
        let plan = minecraft_loaders::validate_import(
            profile_id, settings, game_root, java_major, context,
        )
        .await?;
        let build = if profile_id == "minecraft-java-spigot" {
            "imported".to_string()
        } else {
            settings
                .get("loader_version")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| InstallerError::new("settings_invalid", "servers.settings_invalid"))?
                .to_string()
        };
        (plan, Some(build))
    };
    let result = InstallResult {
        plan,
        installed_version,
        installed_build,
        // The imported JAR has no trusted provider checksum. It is deliberately
        // not presented as a verified artifact.
        artifacts: Vec::new(),
    };
    write_installation_marker(profile_id, game_root, &result).await?;
    Ok(Some(result))
}

pub async fn managed_java_path(
    toolchain_root: &Path,
    major: u16,
) -> Result<PathBuf, InstallerError> {
    toolchains::installed_java_path(toolchain_root, major).await
}

pub async fn ensure_managed_java(
    context: &InstallContext,
    major: u16,
) -> Result<PathBuf, InstallerError> {
    let root = context.toolchain_root.as_ref().ok_or_else(|| {
        InstallerError::new(
            "java_toolchain_root_missing",
            "servers.java_runtime_unavailable",
        )
    })?;
    toolchains::ensure_java(root, major, context).await
}

pub async fn resume_native_install(
    profile_id: &str,
    settings: &serde_json::Value,
    staging: &Path,
) -> Result<InstallResult, InstallerError> {
    validate_resumable_staging_tree(staging).await?;
    let marker = read_installation_marker(profile_id, staging).await?;
    if profile_id.starts_with("minecraft-") {
        let requested_version = settings
            .get("version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| InstallerError::new("settings_invalid", "servers.settings_invalid"))?;
        if marker.installed_version != requested_version {
            return Err(InstallerError::new(
                "installed_version_changed",
                "servers.install_metadata_invalid",
            ));
        }
    }
    let plan = match profile_id {
        "hytale" => {
            hytale::validate_game_layout(staging).await?;
            if marker.required_java_major != Some(25) {
                return Err(InstallerError::new(
                    "java_version_missing",
                    "servers.install_metadata_invalid",
                ));
            }
            hytale::launch_plan(settings)?
        }
        "minecraft-java-vanilla" | "minecraft-java-paper" => {
            let java = marker.required_java_major.ok_or_else(|| {
                InstallerError::new("java_version_missing", "servers.install_metadata_invalid")
            })?;
            let plan = minecraft::validate_installed(settings, staging, java).await?;
            verify_marked_artifact(staging, &marker.artifacts, "server.jar").await?;
            plan
        }
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-spigot"
        | "minecraft-java-purpur"
        | "minecraft-java-quilt" => {
            let java = marker.required_java_major.ok_or_else(|| {
                InstallerError::new("java_version_missing", "servers.install_metadata_invalid")
            })?;
            let plan = minecraft_loaders::validate_installed(
                profile_id,
                settings,
                staging,
                java,
                marker.installed_build.as_deref(),
            )
            .await?;
            for artifact in runtime_artifacts(profile_id) {
                verify_marked_artifact(staging, &marker.artifacts, artifact).await?;
            }
            plan
        }
        "minecraft-bedrock" => bedrock::validate_installed(settings, staging).await?,
        _ => {
            return Err(InstallerError::new(
                "staging_resume_not_supported",
                "servers.install_metadata_invalid",
            ));
        }
    };
    Ok(InstallResult {
        plan,
        installed_version: marker.installed_version,
        installed_build: marker.installed_build,
        artifacts: marker.artifacts,
    })
}

fn runtime_artifacts(profile_id: &str) -> &'static [&'static str] {
    match profile_id {
        "minecraft-java-fabric" => &["fabric-server-launch.jar", "server.jar"],
        "minecraft-java-quilt" => &["quilt-server-launch.jar", "server.jar"],
        "minecraft-java-spigot" | "minecraft-java-purpur" => &["server.jar"],
        _ => &[],
    }
}

async fn verify_marked_artifact(
    root: &Path,
    artifacts: &[InstalledArtifact],
    name: &'static str,
) -> Result<(), InstallerError> {
    const MAX_RESUMED_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
    let mut matching = artifacts.iter().filter(|artifact| artifact.name == name);
    let expected = matching.next().cloned().ok_or_else(|| {
        InstallerError::new(
            "install_artifact_missing",
            "servers.install_metadata_invalid",
        )
    })?;
    if matching.next().is_some()
        || expected.size == 0
        || expected.size > MAX_RESUMED_ARTIFACT_BYTES
        || expected.sha256.len() != 64
        || !expected.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(InstallerError::new(
            "install_artifact_invalid",
            "servers.install_metadata_invalid",
        ));
    }
    let path = root.join(name);
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| InstallerError::internal("install_artifact_missing", error))?;
        if !metadata.is_file() || marker_is_link_like(&metadata) || metadata.len() != expected.size
        {
            return Err(InstallerError::new(
                "install_artifact_invalid",
                "servers.artifact_integrity_failed",
            ));
        }
        reject_marker_hardlink(&metadata)?;
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options
            .open(&path)
            .map_err(|error| InstallerError::internal("install_artifact_missing", error))?;
        let opened = file
            .metadata()
            .map_err(|error| InstallerError::internal("install_artifact_invalid", error))?;
        if !opened.is_file() || marker_is_link_like(&opened) || opened.len() != expected.size {
            return Err(InstallerError::new(
                "install_artifact_changed",
                "servers.artifact_integrity_failed",
            ));
        }
        reject_marker_hardlink(&opened)?;
        let mut digest = Sha256::new();
        let copied = std::io::copy(&mut file.take(expected.size + 1), &mut digest)
            .map_err(|error| InstallerError::internal("install_artifact_read_failed", error))?;
        if copied != expected.size
            || !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&expected.sha256)
        {
            return Err(InstallerError::new(
                "install_artifact_checksum_mismatch",
                "servers.artifact_integrity_failed",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|error| InstallerError::internal("install_artifact_worker_failed", error))?
}

pub async fn validate_resumable_staging_tree(root: &Path) -> Result<(), InstallerError> {
    const MAX_STAGING_ENTRIES: usize = 1_000_000;
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let metadata = fs::symlink_metadata(&root)
            .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
        if !metadata.is_dir() || marker_is_link_like(&metadata) {
            return Err(InstallerError::new(
                "install_tree_unsafe",
                "servers.instance_data_unsafe",
            ));
        }
        let mut pending = vec![root];
        let mut entries = 0_usize;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)
                .map_err(|error| InstallerError::internal("install_tree_invalid", error))?
            {
                let entry = entry
                    .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
                entries = entries.checked_add(1).ok_or_else(|| {
                    InstallerError::new("install_tree_too_large", "servers.installation_failed")
                })?;
                if entries > MAX_STAGING_ENTRIES {
                    return Err(InstallerError::new(
                        "install_tree_too_large",
                        "servers.installation_failed",
                    ));
                }
                let metadata = fs::symlink_metadata(entry.path())
                    .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
                if marker_is_link_like(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
                    return Err(InstallerError::new(
                        "install_tree_unsafe",
                        "servers.instance_data_unsafe",
                    ));
                }
                if metadata.is_file() {
                    reject_marker_hardlink(&metadata)?;
                } else {
                    pending.push(entry.path());
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| InstallerError::internal("install_tree_worker_failed", error))?
}

pub async fn native_launch_plan(
    profile_id: &str,
    settings: &serde_json::Value,
    game_root: &Path,
) -> Result<InstallerPlan, InstallerError> {
    match profile_id {
        "minecraft-java-vanilla" | "minecraft-java-paper" => {
            let marker = read_installation_marker(profile_id, game_root).await?;
            let java = marker.required_java_major.ok_or_else(|| {
                InstallerError::new("java_version_missing", "servers.install_metadata_invalid")
            })?;
            minecraft::validate_installed(settings, game_root, java).await
        }
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-spigot"
        | "minecraft-java-purpur"
        | "minecraft-java-quilt" => {
            let marker = read_installation_marker(profile_id, game_root).await?;
            let java = marker.required_java_major.ok_or_else(|| {
                InstallerError::new("java_version_missing", "servers.install_metadata_invalid")
            })?;
            minecraft_loaders::validate_installed(
                profile_id,
                settings,
                game_root,
                java,
                marker.installed_build.as_deref(),
            )
            .await
        }
        "minecraft-bedrock" => bedrock::validate_installed(settings, game_root).await,
        "hytale" => {
            let marker = read_installation_marker(profile_id, game_root).await?;
            if marker.required_java_major != Some(25) {
                return Err(InstallerError::new(
                    "java_version_missing",
                    "servers.install_metadata_invalid",
                ));
            }
            hytale::launch_plan(settings)
        }
        _ => Err(InstallerError::new(
            "runtime_not_implemented",
            "servers.runtime_not_implemented",
        )),
    }
}

#[allow(dead_code)]
pub fn declared_backup_paths(
    profile_id: &str,
    _settings: &serde_json::Value,
) -> Result<Vec<PathBuf>, AppError> {
    let literal = |paths: &[&str]| paths.iter().map(PathBuf::from).collect::<Vec<_>>();
    match profile_id {
        "valheim" => Ok(literal(&["data"])),
        "palworld" => Ok(literal(&["game/Pal/Saved"])),
        "minecraft-java-vanilla" => Ok(literal(&[
            "game/world",
            "game/world_nether",
            "game/world_the_end",
            "game/server.properties",
            "game/ops.json",
            "game/whitelist.json",
            "game/banned-players.json",
            "game/banned-ips.json",
        ])),
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-quilt" => Ok(literal(&[
            "game/world",
            "game/world_nether",
            "game/world_the_end",
            "game/server.properties",
            "game/ops.json",
            "game/whitelist.json",
            "game/banned-players.json",
            "game/banned-ips.json",
            "game/mods",
            "game/config",
            "game/defaultconfigs",
        ])),
        "minecraft-java-paper" | "minecraft-java-spigot" | "minecraft-java-purpur" => {
            Ok(literal(&[
                "game/world",
                "game/world_nether",
                "game/world_the_end",
                "game/server.properties",
                "game/ops.json",
                "game/whitelist.json",
                "game/banned-players.json",
                "game/banned-ips.json",
                "game/plugins",
                "game/config",
                "game/bukkit.yml",
                "game/spigot.yml",
                "game/paper.yml",
                "game/paper-global.yml",
                "game/paper-world-defaults.yml",
                "game/purpur.yml",
            ]))
        }
        "minecraft-bedrock" => Ok(literal(&[
            "game/worlds",
            "game/server.properties",
            "game/allowlist.json",
            "game/permissions.json",
            "game/behavior_packs",
            "game/resource_packs",
        ])),
        // Paths are relative to the instance root. The official server manual
        // documents these entries inside the `Server/` working directory.
        "hytale" => Ok(literal(&[
            "game/Server/universe",
            "game/Server/config.json",
            "game/Server/permissions.json",
            "game/Server/bans.json",
            "game/Server/whitelist.json",
            "game/Server/mods",
        ])),
        _ => Ok(Vec::new()),
    }
}

pub fn declared_backup_paths_for_profile(
    profile: &GameProfile,
    settings: &serde_json::Value,
) -> Result<Vec<PathBuf>, AppError> {
    if profile.kind != ProfileKind::SteamCustom {
        return declared_backup_paths(&profile.id, settings);
    }
    let steam = profile
        .steam_profile
        .as_ref()
        .ok_or_else(|| AppError::Internal("Steam profile definition is missing".into()))?;
    steam
        .save_paths
        .iter()
        .map(|relative| {
            safe_join(Path::new("game"), relative)
                .map_err(|_| AppError::BadRequest("servers.invalid_save_path".into()))
        })
        .collect()
}

async fn read_json<T: DeserializeOwned>(
    context: &InstallContext,
    url: &Url,
) -> Result<T, InstallerError> {
    let bytes = read_bytes(context, url, MAX_PROVIDER_JSON_BYTES).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| InstallerError::internal("provider_json_invalid", error))
}

async fn read_bytes(
    context: &InstallContext,
    url: &Url,
    max_bytes: u64,
) -> Result<Vec<u8>, InstallerError> {
    context.sources.validate_url(url)?;
    #[cfg(test)]
    if let Some(bytes) = fixture_response(context, url) {
        if bytes.len() as u64 > max_bytes {
            return Err(InstallerError::new(
                "provider_response_too_large",
                "servers.provider_response_invalid",
            ));
        }
        return Ok(bytes);
    }
    let response = get_with_safe_redirects(context, url).await?;
    if !response.status().is_success() {
        return Err(InstallerError::new(
            "provider_request_failed",
            "servers.provider_unavailable",
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > max_bytes)
    {
        return Err(InstallerError::new(
            "provider_response_too_large",
            "servers.provider_response_invalid",
        ));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| InstallerError::internal("provider_response_failed", error))?;
        let next_len = body.len().checked_add(chunk.len()).ok_or_else(|| {
            InstallerError::new(
                "provider_response_too_large",
                "servers.provider_response_invalid",
            )
        })?;
        if u64::try_from(next_len).unwrap_or(u64::MAX) > max_bytes {
            return Err(InstallerError::new(
                "provider_response_too_large",
                "servers.provider_response_invalid",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn download_verified(
    context: &InstallContext,
    url: &Url,
    destination: &Path,
    max_bytes: u64,
    expected: Option<&ExpectedDigest>,
    expected_size: Option<u64>,
) -> Result<DownloadedFile, InstallerError> {
    context.sources.validate_url(url)?;
    if tokio::fs::try_exists(destination)
        .await
        .map_err(|error| InstallerError::internal("download_path_failed", error))?
    {
        return Err(InstallerError::new(
            "download_destination_exists",
            "servers.staging_not_empty",
        ));
    }
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| InstallerError::internal("download_path_failed", error))?;
    }
    let partial = destination.with_extension(format!("part-{}", uuid::Uuid::new_v4().as_simple()));
    let result =
        download_to_partial(context, url, &partial, max_bytes, expected, expected_size).await;
    let downloaded = match result {
        Ok(downloaded) => downloaded,
        Err(error) => {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(error);
        }
    };
    tokio::fs::rename(&partial, destination)
        .await
        .map_err(|error| InstallerError::internal("download_commit_failed", error))?;
    Ok(downloaded)
}

async fn download_to_partial(
    context: &InstallContext,
    url: &Url,
    partial: &Path,
    max_bytes: u64,
    expected: Option<&ExpectedDigest>,
    expected_size: Option<u64>,
) -> Result<DownloadedFile, InstallerError> {
    #[cfg(test)]
    if let Some(bytes) = fixture_response(context, url) {
        return write_fixture_download(&bytes, partial, max_bytes, expected, expected_size).await;
    }
    let response = get_with_safe_redirects(context, url).await?;
    if !response.status().is_success() {
        return Err(InstallerError::new(
            "artifact_download_failed",
            "servers.provider_unavailable",
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > max_bytes || expected_size.is_some_and(|value| value != size))
    {
        return Err(InstallerError::new(
            "artifact_size_mismatch",
            "servers.artifact_integrity_failed",
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(partial)
        .await
        .map_err(|error| InstallerError::internal("download_path_failed", error))?;
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut size = 0_u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| InstallerError::internal("artifact_download_failed", error))?;
        size = size.checked_add(chunk.len() as u64).ok_or_else(|| {
            InstallerError::new("artifact_too_large", "servers.artifact_integrity_failed")
        })?;
        if size > max_bytes {
            return Err(InstallerError::new(
                "artifact_too_large",
                "servers.artifact_integrity_failed",
            ));
        }
        md5.update(&chunk);
        sha1.update(&chunk);
        sha256.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| InstallerError::internal("artifact_write_failed", error))?;
    }
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("artifact_write_failed", error))?;
    let downloaded = DownloadedFile {
        md5: format!("{:x}", md5.finalize()),
        sha1: format!("{:x}", sha1.finalize()),
        sha256: format!("{:x}", sha256.finalize()),
        size,
    };
    if expected_size.is_some_and(|value| value != downloaded.size)
        || expected.is_some_and(|digest| match digest {
            ExpectedDigest::Md5(value) => !downloaded.md5.eq_ignore_ascii_case(value),
            ExpectedDigest::Sha1(value) => !downloaded.sha1.eq_ignore_ascii_case(value),
            ExpectedDigest::Sha256(value) => !downloaded.sha256.eq_ignore_ascii_case(value),
        })
    {
        return Err(InstallerError::new(
            "artifact_checksum_mismatch",
            "servers.artifact_integrity_failed",
        ));
    }
    Ok(downloaded)
}

async fn get_with_safe_redirects(
    context: &InstallContext,
    initial: &Url,
) -> Result<reqwest::Response, InstallerError> {
    let mut url = initial.clone();
    for _ in 0..=5 {
        context.sources.validate_url(&url)?;
        let response = context
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| InstallerError::internal("provider_request_failed", error))?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                InstallerError::new(
                    "provider_redirect_invalid",
                    "servers.provider_response_invalid",
                )
            })?;
        url = url.join(location).map_err(|_| {
            InstallerError::new(
                "provider_redirect_invalid",
                "servers.provider_response_invalid",
            )
        })?;
    }
    Err(InstallerError::new(
        "provider_redirect_limit",
        "servers.provider_response_invalid",
    ))
}

#[cfg(test)]
fn fixture_response(context: &InstallContext, url: &Url) -> Option<Vec<u8>> {
    context
        .fixture_responses
        .as_ref()?
        .get(url.as_str())
        .cloned()
}

#[cfg(test)]
async fn write_fixture_download(
    bytes: &[u8],
    partial: &Path,
    max_bytes: u64,
    expected: Option<&ExpectedDigest>,
    expected_size: Option<u64>,
) -> Result<DownloadedFile, InstallerError> {
    let size = bytes.len() as u64;
    if size > max_bytes || expected_size.is_some_and(|value| value != size) {
        return Err(InstallerError::new(
            "artifact_size_mismatch",
            "servers.artifact_integrity_failed",
        ));
    }
    let md5 = format!("{:x}", Md5::digest(bytes));
    let sha1 = format!("{:x}", Sha1::digest(bytes));
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    if expected.is_some_and(|digest| match digest {
        ExpectedDigest::Md5(value) => !md5.eq_ignore_ascii_case(value),
        ExpectedDigest::Sha1(value) => !sha1.eq_ignore_ascii_case(value),
        ExpectedDigest::Sha256(value) => !sha256.eq_ignore_ascii_case(value),
    }) {
        return Err(InstallerError::new(
            "artifact_checksum_mismatch",
            "servers.artifact_integrity_failed",
        ));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(partial)
        .await
        .map_err(|error| InstallerError::internal("download_path_failed", error))?;
    file.write_all(bytes)
        .await
        .map_err(|error| InstallerError::internal("artifact_write_failed", error))?;
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("artifact_write_failed", error))?;
    Ok(DownloadedFile {
        md5,
        sha1,
        sha256,
        size,
    })
}

async fn write_installation_marker(
    profile_id: &str,
    staging: &Path,
    result: &InstallResult,
) -> Result<(), InstallerError> {
    let required_java_major = match result.plan.executable {
        InstallerExecutable::ManagedJava { major } => Some(major),
        InstallerExecutable::InstanceRelative { .. } => None,
    };
    let marker = InstallationMarker {
        schema: 1,
        profile_id: profile_id.to_string(),
        installed_version: result.installed_version.clone(),
        installed_build: result.installed_build.clone(),
        required_java_major,
        artifacts: result.artifacts.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|error| InstallerError::internal("install_metadata_failed", error))?;
    let path = staging.join(".dmx-install.json");
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|error| InstallerError::internal("install_metadata_failed", error))?;
    file.write_all(&bytes)
        .await
        .map_err(|error| InstallerError::internal("install_metadata_failed", error))?;
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("install_metadata_failed", error))
}

async fn read_installation_marker(
    profile_id: &str,
    game_root: &Path,
) -> Result<InstallationMarker, InstallerError> {
    let path = game_root.join(".dmx-install.json");
    let bytes = tokio::task::spawn_blocking(move || read_marker_no_follow(&path))
        .await
        .map_err(|error| InstallerError::internal("install_metadata_worker_failed", error))??;
    let marker: InstallationMarker = serde_json::from_slice(&bytes)
        .map_err(|error| InstallerError::internal("install_metadata_invalid", error))?;
    if marker.schema != 1 || marker.profile_id != profile_id {
        return Err(InstallerError::new(
            "install_metadata_invalid",
            "servers.install_metadata_invalid",
        ));
    }
    Ok(marker)
}

fn read_marker_no_follow(path: &Path) -> Result<Vec<u8>, InstallerError> {
    const MAX_MARKER_BYTES: u64 = 256 * 1024;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| InstallerError::internal("install_metadata_missing", error))?;
    if !metadata.is_file() || marker_is_link_like(&metadata) || metadata.len() > MAX_MARKER_BYTES {
        return Err(InstallerError::new(
            "install_metadata_invalid",
            "servers.install_metadata_invalid",
        ));
    }
    reject_marker_hardlink(&metadata)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|error| InstallerError::internal("install_metadata_missing", error))?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| InstallerError::internal("install_metadata_invalid", error))?;
    if !opened_metadata.is_file()
        || marker_is_link_like(&opened_metadata)
        || opened_metadata.len() != metadata.len()
    {
        return Err(InstallerError::new(
            "install_metadata_changed",
            "servers.install_metadata_invalid",
        ));
    }
    reject_marker_hardlink(&opened_metadata)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| InstallerError::internal("install_metadata_invalid", error))?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(InstallerError::new(
            "install_metadata_invalid",
            "servers.install_metadata_invalid",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn marker_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn marker_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn marker_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn reject_marker_hardlink(metadata: &fs::Metadata) -> Result<(), InstallerError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() > 1 {
        Err(InstallerError::new(
            "install_metadata_hardlink",
            "servers.install_metadata_invalid",
        ))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn reject_marker_hardlink(metadata: &fs::Metadata) -> Result<(), InstallerError> {
    use std::os::windows::fs::MetadataExt;
    if metadata.number_of_links() > 1 {
        Err(InstallerError::new(
            "install_metadata_hardlink",
            "servers.install_metadata_invalid",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn reject_marker_hardlink(_metadata: &fs::Metadata) -> Result<(), InstallerError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn installation_metadata_rejects_links() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside.json");
        fs::write(&outside, b"{}").unwrap();
        symlink(&outside, directory.path().join(".dmx-install.json")).unwrap();
        assert!(
            read_installation_marker("minecraft-java-vanilla", directory.path())
                .await
                .is_err()
        );
    }

    #[test]
    fn official_sources_are_https_and_strictly_allowlisted() {
        let sources = InstallerSources::official();
        assert!(sources.validate_url(&sources.minecraft_manifest).is_ok());
        assert!(sources.validate_url(&sources.paper_api_base).is_ok());
        assert!(sources.validate_url(&sources.hytale_downloader).is_ok());
        assert!(
            sources
                .validate_url(&Url::parse("https://evil.example/file.jar").unwrap())
                .is_err()
        );
        assert!(
            sources
                .validate_url(&Url::parse("https://fill.papermc.io.evil.example/file.jar").unwrap())
                .is_err()
        );
    }

    #[test]
    fn built_in_backup_paths_do_not_consult_instance_controlled_steam_paths() {
        assert!(
            declared_backup_paths(
                "steam-custom",
                &serde_json::json!({"save_paths": ["../secret"]})
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            declared_backup_paths("hytale", &serde_json::json!({}))
                .unwrap()
                .contains(&PathBuf::from("game/Server/universe"))
        );
    }

    #[tokio::test]
    async fn completed_minecraft_staging_resumes_only_with_matching_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let server = b"verified server jar";
        fs::write(directory.path().join("server.jar"), server).unwrap();
        let settings = serde_json::json!({
            "version": "1.21.11",
            "eula_accepted": true,
            "max_memory_mb": 4096
        });
        let result = InstallResult {
            plan: minecraft::launch_plan(&settings, 21).unwrap(),
            installed_version: "1.21.11".into(),
            installed_build: None,
            artifacts: vec![InstalledArtifact {
                name: "server.jar".into(),
                sha256: format!("{:x}", Sha256::digest(server)),
                size: server.len() as u64,
            }],
        };
        write_installation_marker("minecraft-java-vanilla", directory.path(), &result)
            .await
            .unwrap();

        assert!(
            resume_native_install("minecraft-java-vanilla", &settings, directory.path())
                .await
                .is_ok()
        );
        fs::write(directory.path().join("server.jar"), b"tampered server jar").unwrap();
        assert!(
            resume_native_install("minecraft-java-vanilla", &settings, directory.path())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn completed_minecraft_staging_rejects_a_different_requested_version() {
        let directory = tempfile::tempdir().unwrap();
        let server = b"verified server jar";
        fs::write(directory.path().join("server.jar"), server).unwrap();
        let installed_settings = serde_json::json!({
            "version": "1.21.11",
            "eula_accepted": true
        });
        let result = InstallResult {
            plan: minecraft::launch_plan(&installed_settings, 21).unwrap(),
            installed_version: "1.21.11".into(),
            installed_build: None,
            artifacts: vec![InstalledArtifact {
                name: "server.jar".into(),
                sha256: format!("{:x}", Sha256::digest(server)),
                size: server.len() as u64,
            }],
        };
        write_installation_marker("minecraft-java-vanilla", directory.path(), &result)
            .await
            .unwrap();

        let changed = serde_json::json!({"version": "1.22", "eula_accepted": true});
        assert!(
            resume_native_install("minecraft-java-vanilla", &changed, directory.path())
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resumed_staging_rejects_links_anywhere_in_the_tree() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("data")).unwrap();
        symlink("../outside", directory.path().join("data/link")).unwrap();
        assert!(
            validate_resumable_staging_tree(directory.path())
                .await
                .is_err()
        );
    }
}
