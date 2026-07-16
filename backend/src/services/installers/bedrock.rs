use std::{
    fs, io,
    path::{Path, PathBuf},
};

use axum::body::Bytes;
use futures::{Stream, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::domain::v1::StopStrategy;

use super::{
    ExpectedDigest, InstallContext, InstallResult, InstalledArtifact, InstallerError,
    InstallerExecutable, InstallerPlan, download_verified, extract_zip,
    minecraft::{merge_properties, read_bounded_regular_text},
};

const MAX_BEDROCK_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_BEDROCK_DATA_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_BEDROCK_DATA_ENTRIES: usize = 200_000;
const BEDROCK_UPLOAD_DIRECTORY: &str = ".dmx-bedrock-import";
const BEDROCK_UPLOAD_ARCHIVE: &str = "archive.zip";
const BEDROCK_UPLOAD_METADATA: &str = "metadata.json";
const MAX_UPLOAD_METADATA_BYTES: u64 = 4 * 1024;

#[derive(Debug, Clone)]
pub(super) struct LocalArchive {
    path: PathBuf,
    sha256: String,
    size: u64,
    version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct UploadMetadata {
    schema: u8,
    sha256: String,
    size_bytes: u64,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadLinksResponse {
    result: DownloadLinksResult,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadLinksResult {
    links: Vec<DownloadLink>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DownloadLink {
    download_type: String,
    download_url: String,
}

#[derive(Debug, Clone)]
struct ResolvedSource {
    url: Url,
    expected_sha256: Option<String>,
    size: Option<u64>,
    version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BedrockPlatform {
    Linux,
    Windows,
}

impl BedrockPlatform {
    fn executable(self) -> &'static str {
        match self {
            Self::Linux => "bedrock_server",
            Self::Windows => "bedrock_server.exe",
        }
    }
}

pub async fn install_bedrock(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    install_bedrock_for_platform(
        settings,
        instance_root,
        staging,
        context,
        current_platform()?,
    )
    .await
}

async fn install_bedrock_for_platform(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
    platform: BedrockPlatform,
) -> Result<InstallResult, InstallerError> {
    validate_eula(settings)?;
    validate_settings(settings)?;
    let version = settings
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| valid_version(value))
        .ok_or_else(|| {
            InstallerError::new(
                "bedrock_version_unavailable",
                "servers.bedrock_version_unavailable",
            )
        })?;
    let artifact = if let Some(archive) = &context.bedrock_archive {
        if archive.version != version {
            return Err(InstallerError::new(
                "bedrock_upload_version_mismatch",
                "servers.bedrock_archive_version_mismatch",
            ));
        }
        extract_zip(&archive.path, staging, context.archive_limits, None).await?;
        InstalledArtifact {
            name: "bedrock-server.zip".to_string(),
            sha256: archive.sha256.clone(),
            size: archive.size,
        }
    } else {
        let source = platform_source(context, platform).await?;
        if source.version != version {
            return Err(InstallerError::new(
                "bedrock_version_unavailable",
                "servers.bedrock_version_unavailable",
            ));
        }
        let archive =
            staging.with_file_name(format!(".bedrock-{}.zip", uuid::Uuid::new_v4().as_simple()));
        let expected_digest = source
            .expected_sha256
            .as_ref()
            .map(|sha256| ExpectedDigest::Sha256(sha256.clone()));
        let downloaded = download_verified(
            context,
            &source.url,
            &archive,
            MAX_BEDROCK_ARCHIVE_BYTES,
            expected_digest.as_ref(),
            source.size,
        )
        .await?;
        let extraction = extract_zip(&archive, staging, context.archive_limits, None).await;
        let _ = tokio::fs::remove_file(&archive).await;
        extraction?;
        InstalledArtifact {
            name: "bedrock-server.zip".to_string(),
            sha256: downloaded.sha256,
            size: downloaded.size,
        }
    };
    validate_executable(staging, platform).await?;
    preserve_bedrock_data(instance_root, staging).await?;
    write_configuration(instance_root, staging, settings).await?;

    Ok(InstallResult {
        plan: launch_plan_for(settings, platform)?,
        installed_version: version.to_string(),
        installed_build: None,
        artifacts: vec![artifact],
    })
}

pub(super) fn upload_directory(
    instance_root: &Path,
    job_id: &str,
) -> Result<PathBuf, InstallerError> {
    let job_id = uuid::Uuid::parse_str(job_id).map_err(|_| {
        InstallerError::new(
            "bedrock_upload_job_invalid",
            "servers.bedrock_archive_upload_invalid",
        )
    })?;
    Ok(instance_root
        .join(BEDROCK_UPLOAD_DIRECTORY)
        .join(job_id.hyphenated().to_string()))
}

pub(super) async fn store_local_archive<S, E>(
    instance_root: &Path,
    job_id: &str,
    expected_version: &str,
    expected_sha256: &str,
    mut stream: S,
) -> Result<InstalledArtifact, InstallerError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    if !valid_version(expected_version) || validate_digest(expected_sha256).is_err() {
        return Err(InstallerError::new(
            "bedrock_upload_metadata_invalid",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let expected_sha256 = expected_sha256.to_ascii_lowercase();
    let directory = upload_directory(instance_root, job_id)?;
    create_private_upload_directory(instance_root, &directory).await?;

    let destination = directory.join(BEDROCK_UPLOAD_ARCHIVE);
    let metadata_path = directory.join(BEDROCK_UPLOAD_METADATA);
    if tokio::fs::try_exists(&metadata_path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_check_failed", error))?
    {
        let existing = load_local_archive(instance_root, job_id, expected_version)
            .await?
            .ok_or_else(|| {
                InstallerError::new(
                    "bedrock_upload_incomplete",
                    "servers.bedrock_archive_upload_invalid",
                )
            })?;
        if existing.sha256 != expected_sha256 {
            return Err(InstallerError::new(
                "bedrock_upload_conflict",
                "servers.bedrock_archive_upload_conflict",
            ));
        }
        return Ok(InstalledArtifact {
            name: "bedrock-server.zip".to_string(),
            sha256: existing.sha256,
            size: existing.size,
        });
    }

    let temporary = directory.join(format!(".archive-{}.tmp", uuid::Uuid::new_v4().as_simple()));
    let mut file = open_private_new(&temporary).await?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                drop(file);
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(InstallerError::internal(
                    "bedrock_upload_stream_failed",
                    error,
                ));
            }
        };
        size = size.checked_add(chunk.len() as u64).ok_or_else(|| {
            InstallerError::new(
                "bedrock_upload_too_large",
                "servers.bedrock_archive_too_large",
            )
        })?;
        if size > MAX_BEDROCK_ARCHIVE_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(InstallerError::new(
                "bedrock_upload_too_large",
                "servers.bedrock_archive_too_large",
            ));
        }
        digest.update(&chunk);
        if let Err(error) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(InstallerError::internal(
                "bedrock_upload_write_failed",
                error,
            ));
        }
    }
    if size == 0 {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::new(
            "bedrock_upload_empty",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    if let Err(error) = file.flush().await {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::internal(
            "bedrock_upload_write_failed",
            error,
        ));
    }
    if let Err(error) = file.sync_all().await {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::internal(
            "bedrock_upload_write_failed",
            error,
        ));
    }
    drop(file);
    let actual_sha256 = format!("{:x}", digest.finalize());
    if actual_sha256 != expected_sha256 {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::new(
            "bedrock_upload_checksum_mismatch",
            "servers.bedrock_archive_checksum_mismatch",
        ));
    }

    if let Err(error) = tokio::fs::hard_link(&temporary, &destination).await {
        if error.kind() != io::ErrorKind::AlreadyExists {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(InstallerError::internal(
                "bedrock_upload_publish_failed",
                error,
            ));
        }
        if let Err(validation) = validate_archive_file(&destination, size, &actual_sha256).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(validation);
        }
    }
    let _ = tokio::fs::remove_file(&temporary).await;

    let metadata = UploadMetadata {
        schema: 1,
        sha256: actual_sha256.clone(),
        size_bytes: size,
        version: expected_version.to_string(),
    };
    publish_metadata(&metadata_path, &metadata).await?;
    sync_directory(&directory).await?;
    Ok(InstalledArtifact {
        name: "bedrock-server.zip".to_string(),
        sha256: actual_sha256,
        size,
    })
}

pub(super) async fn load_local_archive(
    instance_root: &Path,
    job_id: &str,
    expected_version: &str,
) -> Result<Option<LocalArchive>, InstallerError> {
    if !valid_version(expected_version) {
        return Err(InstallerError::new(
            "bedrock_upload_version_invalid",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let directory = upload_directory(instance_root, job_id)?;
    let archive_path = directory.join(BEDROCK_UPLOAD_ARCHIVE);
    let metadata_path = directory.join(BEDROCK_UPLOAD_METADATA);
    let archive_exists = tokio::fs::try_exists(&archive_path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_check_failed", error))?;
    let metadata_exists = tokio::fs::try_exists(&metadata_path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_check_failed", error))?;
    if !archive_exists && !metadata_exists {
        return Ok(None);
    }
    if !archive_exists || !metadata_exists {
        return Err(InstallerError::new(
            "bedrock_upload_incomplete",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    validate_upload_directory(instance_root, &directory).await?;
    let metadata = read_upload_metadata(&metadata_path).await?;
    if metadata.schema != 1
        || metadata.version != expected_version
        || metadata.size_bytes == 0
        || metadata.size_bytes > MAX_BEDROCK_ARCHIVE_BYTES
        || validate_digest(&metadata.sha256).is_err()
    {
        return Err(InstallerError::new(
            "bedrock_upload_metadata_invalid",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let sha256 = metadata.sha256.to_ascii_lowercase();
    validate_archive_file(&archive_path, metadata.size_bytes, &sha256).await?;
    Ok(Some(LocalArchive {
        path: archive_path,
        sha256,
        size: metadata.size_bytes,
        version: metadata.version,
    }))
}

pub(super) async fn remove_local_archive(
    instance_root: &Path,
    job_id: &str,
) -> Result<(), InstallerError> {
    let directory = upload_directory(instance_root, job_id)?;
    match tokio::fs::symlink_metadata(&directory).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            tokio::fs::remove_dir_all(directory)
                .await
                .map_err(|error| InstallerError::internal("bedrock_upload_cleanup_failed", error))
        }
        Ok(_) => Err(InstallerError::new(
            "bedrock_upload_directory_unsafe",
            "servers.bedrock_archive_upload_invalid",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallerError::internal(
            "bedrock_upload_cleanup_failed",
            error,
        )),
    }
}

async fn create_private_upload_directory(
    instance_root: &Path,
    directory: &Path,
) -> Result<(), InstallerError> {
    let root_metadata = tokio::fs::symlink_metadata(instance_root)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_root_invalid", error))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "bedrock_upload_root_invalid",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let parent = instance_root.join(BEDROCK_UPLOAD_DIRECTORY);
    create_private_directory_async(&parent).await?;
    create_private_directory_async(directory).await?;
    validate_upload_directory(instance_root, directory).await
}

async fn create_private_directory_async(path: &Path) -> Result<(), InstallerError> {
    #[cfg(unix)]
    let mut builder = tokio::fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = tokio::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        builder.mode(0o700);
    }
    match builder.create(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
                InstallerError::internal("bedrock_upload_directory_failed", error)
            })?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(InstallerError::new(
                    "bedrock_upload_directory_unsafe",
                    "servers.bedrock_archive_upload_invalid",
                ))
            }
        }
        Err(error) => Err(InstallerError::internal(
            "bedrock_upload_directory_failed",
            error,
        )),
    }
}

async fn validate_upload_directory(
    instance_root: &Path,
    directory: &Path,
) -> Result<(), InstallerError> {
    for path in [
        instance_root.to_path_buf(),
        instance_root.join(BEDROCK_UPLOAD_DIRECTORY),
        directory.to_path_buf(),
    ] {
        let metadata = tokio::fs::symlink_metadata(path)
            .await
            .map_err(|error| InstallerError::internal("bedrock_upload_directory_failed", error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(InstallerError::new(
                "bedrock_upload_directory_unsafe",
                "servers.bedrock_archive_upload_invalid",
            ));
        }
    }
    Ok(())
}

async fn open_private_new(path: &Path) -> Result<tokio::fs::File, InstallerError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_write_failed", error))
}

async fn open_regular_read(
    path: &Path,
    maximum: u64,
) -> Result<(tokio::fs::File, u64), InstallerError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_read_failed", error))?;
    reject_unsafe_metadata(path, &metadata)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(InstallerError::new(
            "bedrock_upload_file_invalid",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_read_failed", error))?;
    let opened = file
        .metadata()
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_read_failed", error))?;
    reject_unsafe_metadata(path, &opened)?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(InstallerError::new(
            "bedrock_upload_file_changed",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    Ok((file, opened.len()))
}

async fn validate_archive_file(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), InstallerError> {
    let (mut file, size) = open_regular_read(path, MAX_BEDROCK_ARCHIVE_BYTES).await?;
    if size != expected_size {
        return Err(InstallerError::new(
            "bedrock_upload_size_mismatch",
            "servers.bedrock_archive_upload_invalid",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    let mut read = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| InstallerError::internal("bedrock_upload_read_failed", error))?;
        if count == 0 {
            break;
        }
        read = read.checked_add(count as u64).ok_or_else(|| {
            InstallerError::new(
                "bedrock_upload_too_large",
                "servers.bedrock_archive_too_large",
            )
        })?;
        if read > expected_size {
            return Err(InstallerError::new(
                "bedrock_upload_size_mismatch",
                "servers.bedrock_archive_upload_invalid",
            ));
        }
        digest.update(&buffer[..count]);
    }
    if read != expected_size || format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err(InstallerError::new(
            "bedrock_upload_checksum_mismatch",
            "servers.bedrock_archive_checksum_mismatch",
        ));
    }
    Ok(())
}

async fn read_upload_metadata(path: &Path) -> Result<UploadMetadata, InstallerError> {
    let (mut file, size) = open_regular_read(path, MAX_UPLOAD_METADATA_BYTES).await?;
    let capacity = usize::try_from(size).map_err(|_| {
        InstallerError::new(
            "bedrock_upload_metadata_invalid",
            "servers.bedrock_archive_upload_invalid",
        )
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)
        .await
        .map_err(|error| InstallerError::internal("bedrock_upload_read_failed", error))?;
    serde_json::from_slice(&bytes).map_err(|_| {
        InstallerError::new(
            "bedrock_upload_metadata_invalid",
            "servers.bedrock_archive_upload_invalid",
        )
    })
}

async fn publish_metadata(
    destination: &Path,
    metadata: &UploadMetadata,
) -> Result<(), InstallerError> {
    let directory = destination.parent().ok_or_else(|| {
        InstallerError::new(
            "bedrock_upload_directory_invalid",
            "servers.bedrock_archive_upload_invalid",
        )
    })?;
    let temporary = directory.join(format!(
        ".metadata-{}.tmp",
        uuid::Uuid::new_v4().as_simple()
    ));
    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| InstallerError::internal("bedrock_upload_metadata_failed", error))?;
    let mut file = open_private_new(&temporary).await?;
    if let Err(error) = file.write_all(&bytes).await {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::internal(
            "bedrock_upload_metadata_failed",
            error,
        ));
    }
    if let Err(error) = file.sync_all().await {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InstallerError::internal(
            "bedrock_upload_metadata_failed",
            error,
        ));
    }
    drop(file);
    match tokio::fs::hard_link(&temporary, destination).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = read_upload_metadata(destination).await?;
            if existing != *metadata {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(InstallerError::new(
                    "bedrock_upload_conflict",
                    "servers.bedrock_archive_upload_conflict",
                ));
            }
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(InstallerError::internal(
                "bedrock_upload_publish_failed",
                error,
            ));
        }
    }
    let _ = tokio::fs::remove_file(&temporary).await;
    Ok(())
}

#[cfg(unix)]
async fn sync_directory(path: &Path) -> Result<(), InstallerError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let directory = fs::File::open(path)
            .map_err(|error| InstallerError::internal("bedrock_upload_sync_failed", error))?;
        directory
            .sync_all()
            .map_err(|error| InstallerError::internal("bedrock_upload_sync_failed", error))
    })
    .await
    .map_err(|error| InstallerError::internal("bedrock_upload_sync_failed", error))?
}

#[cfg(not(unix))]
async fn sync_directory(_path: &Path) -> Result<(), InstallerError> {
    Ok(())
}

fn launch_plan_for(
    _settings: &Value,
    platform: BedrockPlatform,
) -> Result<InstallerPlan, InstallerError> {
    Ok(InstallerPlan {
        executable: InstallerExecutable::InstanceRelative {
            path: platform.executable().to_string(),
        },
        cwd_relative: ".".to_string(),
        args: Vec::new(),
        env: Vec::new(),
        stop: StopStrategy::Stdin {
            command: "stop".to_string(),
            timeout_seconds: 30,
        },
        restart_exit_codes: Vec::new(),
    })
}

async fn platform_source(
    context: &InstallContext,
    platform: BedrockPlatform,
) -> Result<ResolvedSource, InstallerError> {
    let source = match platform {
        BedrockPlatform::Linux => context.sources.bedrock_linux.as_ref(),
        BedrockPlatform::Windows => context.sources.bedrock_windows.as_ref(),
    };
    if let Some(source) = source {
        validate_digest(&source.sha256)?;
        return Ok(ResolvedSource {
            url: source.url.clone(),
            expected_sha256: Some(source.sha256.clone()),
            size: source.size,
            version: source.version.clone(),
        });
    }
    discover_official_source(context, platform).await
}

async fn discover_official_source(
    context: &InstallContext,
    platform: BedrockPlatform,
) -> Result<ResolvedSource, InstallerError> {
    let mut errors = Vec::new();
    let mut response = None;
    for api in std::iter::once(&context.sources.bedrock_download_api)
        .chain(context.sources.bedrock_download_fallback_api.iter())
    {
        match super::read_json(context, api).await {
            Ok(value) => {
                response = Some(value);
                break;
            }
            Err(error) => errors.push(format!(
                "{} ({})",
                api.host_str().unwrap_or("unknown"),
                error.code
            )),
        }
    }
    let response: DownloadLinksResponse = response.ok_or_else(|| InstallerError {
        code: "bedrock_official_source_unavailable",
        client_message: "servers.bedrock_automatic_install_unavailable",
        internal: Some(format!(
            "official Bedrock catalogs failed: {}",
            errors.join(", ")
        )),
    })?;
    let expected_type = match platform {
        BedrockPlatform::Linux => "serverBedrockLinux",
        BedrockPlatform::Windows => "serverBedrockWindows",
    };
    let link = response
        .result
        .links
        .into_iter()
        .find(|link| link.download_type == expected_type)
        .ok_or_else(|| {
            InstallerError::new(
                "bedrock_official_source_unavailable",
                "servers.bedrock_automatic_install_unavailable",
            )
        })?;
    let url = Url::parse(&link.download_url).map_err(|_| {
        InstallerError::new(
            "bedrock_source_url_invalid",
            "servers.provider_response_invalid",
        )
    })?;
    let version = discovered_source_version(&url, platform)?;
    Ok(ResolvedSource {
        url,
        expected_sha256: None,
        size: None,
        version,
    })
}

fn discovered_source_version(
    url: &Url,
    platform: BedrockPlatform,
) -> Result<String, InstallerError> {
    let platform = match platform {
        BedrockPlatform::Linux => "linux",
        BedrockPlatform::Windows => "win",
    };
    let prefix = format!("/bedrockdedicatedserver/bin-{platform}/bedrock-server-");
    let version = url
        .path()
        .strip_prefix(&prefix)
        .and_then(|path| path.strip_suffix(".zip"))
        .filter(|version| valid_version(version))
        .ok_or_else(|| {
            InstallerError::new(
                "bedrock_source_url_rejected",
                "servers.provider_response_invalid",
            )
        })?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.minecraft.net")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(InstallerError::new(
            "bedrock_source_url_rejected",
            "servers.provider_response_invalid",
        ));
    }
    Ok(version.to_string())
}

pub(super) async fn current_official_version(
    context: &InstallContext,
) -> Result<String, InstallerError> {
    discover_official_source(context, current_platform()?)
        .await
        .map(|source| source.version)
}

async fn validate_executable(
    staging: &Path,
    platform: BedrockPlatform,
) -> Result<(), InstallerError> {
    let path = staging.join(platform.executable());
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|error| InstallerError::internal("bedrock_executable_missing", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "bedrock_executable_invalid",
            "servers.installed_executable_invalid",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| InstallerError::internal("bedrock_permissions_failed", error))?;
    }
    Ok(())
}

fn current_platform() -> Result<BedrockPlatform, InstallerError> {
    if cfg!(target_os = "windows") {
        Ok(BedrockPlatform::Windows)
    } else if cfg!(target_os = "linux") {
        Ok(BedrockPlatform::Linux)
    } else {
        Err(InstallerError::new(
            "bedrock_platform_unsupported",
            "servers.platform_not_supported",
        ))
    }
}

pub(super) async fn validate_installed(
    settings: &Value,
    game_root: &Path,
) -> Result<InstallerPlan, InstallerError> {
    validate_eula(settings)?;
    validate_settings(settings)?;
    let platform = current_platform()?;
    validate_executable(game_root, platform).await?;
    launch_plan_for(settings, platform)
}

pub(super) async fn apply_configuration(
    game_root: &Path,
    settings: &Value,
) -> Result<(), InstallerError> {
    validate_eula(settings)?;
    validate_settings(settings)?;
    let instance_root = game_root.parent().ok_or_else(|| {
        InstallerError::new(
            "configuration_path_invalid",
            "servers.configuration_invalid",
        )
    })?;
    write_configuration(instance_root, game_root, settings).await
}

pub(super) fn validate_configured_source(
    url: &Url,
    version: &str,
    sha256: &str,
    platform: &'static str,
) -> Result<(), InstallerError> {
    if !matches!(platform, "linux" | "win")
        || version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        || validate_digest(sha256).is_err()
    {
        return Err(InstallerError::new(
            "bedrock_source_metadata_invalid",
            "servers.bedrock_source_configuration_invalid",
        ));
    }
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(InstallerError::new(
            "bedrock_source_url_invalid",
            "servers.bedrock_source_configuration_invalid",
        ));
    }
    let archive = format!("bedrock-server-{version}.zip");
    let path_valid = match host {
        "minecraft.azureedge.net" => url.path() == format!("/bin-{platform}/{archive}"),
        "www.minecraft.net" | "minecraft.net" => {
            url.path() == format!("/bedrockdedicatedserver/bin-{platform}/{archive}")
        }
        "aka.ms" => url.path().eq_ignore_ascii_case("/MinecraftBDS"),
        _ => false,
    };
    if !path_valid {
        return Err(InstallerError::new(
            "bedrock_source_url_rejected",
            "servers.bedrock_source_configuration_invalid",
        ));
    }
    Ok(())
}

async fn preserve_bedrock_data(instance_root: &Path, staging: &Path) -> Result<(), InstallerError> {
    let current = instance_root.join("game");
    for relative in [
        "worlds",
        "allowlist.json",
        "permissions.json",
        "behavior_packs",
        "resource_packs",
    ] {
        let source = current.join(relative);
        match tokio::fs::symlink_metadata(&source).await {
            Ok(metadata) => reject_unsafe_metadata(&source, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(InstallerError::internal("preserve_data_failed", error));
            }
        }
        let destination = staging.join(relative);
        if tokio::fs::try_exists(&destination)
            .await
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?
        {
            if tokio::fs::symlink_metadata(&destination)
                .await
                .is_ok_and(|metadata| metadata.is_dir())
            {
                tokio::fs::remove_dir_all(&destination)
                    .await
                    .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            } else {
                tokio::fs::remove_file(&destination)
                    .await
                    .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            }
        }
        copy_bedrock_data(&source, &destination).await?;
    }
    Ok(())
}

async fn copy_bedrock_data(source: &Path, destination: &Path) -> Result<(), InstallerError> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || copy_bedrock_data_blocking(&source, &destination))
        .await
        .map_err(|error| InstallerError::internal("preserve_worker_failed", error))?
}

fn copy_bedrock_data_blocking(source: &Path, destination: &Path) -> Result<(), InstallerError> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(InstallerError::internal("preserve_data_failed", error)),
    };
    reject_unsafe_metadata(source, &metadata)?;
    if metadata.is_file() {
        copy_bedrock_file(source, destination, metadata.len())?;
        return Ok(());
    }
    create_private_directory(destination)?;
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    while let Some((source_directory, target_directory)) = pending.pop() {
        let directory_metadata = fs::symlink_metadata(&source_directory)
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
        reject_unsafe_metadata(&source_directory, &directory_metadata)?;
        if !directory_metadata.is_dir() {
            return Err(InstallerError::new(
                "preserve_data_changed",
                "servers.instance_data_unsafe",
            ));
        }
        for entry in fs::read_dir(&source_directory)
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?
        {
            let entry =
                entry.map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            entries = entries.checked_add(1).ok_or_else(|| {
                InstallerError::new("preserve_data_too_large", "servers.instance_data_unsafe")
            })?;
            if entries > MAX_BEDROCK_DATA_ENTRIES {
                return Err(InstallerError::new(
                    "preserve_data_too_many_entries",
                    "servers.instance_data_unsafe",
                ));
            }
            let source_path = entry.path();
            let target_path = target_directory.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path)
                .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
            reject_unsafe_metadata(&source_path, &metadata)?;
            if metadata.is_dir() {
                create_private_directory(&target_path)?;
                pending.push((source_path, target_path));
            } else if metadata.is_file() {
                bytes = bytes.checked_add(metadata.len()).ok_or_else(|| {
                    InstallerError::new("preserve_data_too_large", "servers.instance_data_unsafe")
                })?;
                if bytes > MAX_BEDROCK_DATA_BYTES {
                    return Err(InstallerError::new(
                        "preserve_data_too_large",
                        "servers.instance_data_unsafe",
                    ));
                }
                copy_bedrock_file(&source_path, &target_path, metadata.len())?;
            }
        }
    }
    Ok(())
}

fn copy_bedrock_file(
    source: &Path,
    destination: &Path,
    expected_size: u64,
) -> Result<(), InstallerError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    }
    let mut input_options = fs::OpenOptions::new();
    input_options.read(true);
    let mut output_options = fs::OpenOptions::new();
    output_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        input_options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        output_options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        input_options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        output_options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut input = input_options
        .open(source)
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    let metadata = input
        .metadata()
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    reject_unsafe_metadata(source, &metadata)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(InstallerError::new(
            "preserve_data_changed",
            "servers.instance_data_unsafe",
        ));
    }
    let mut output = output_options
        .open(destination)
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    let copied = io::copy(&mut input, &mut output)
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?;
    if copied != expected_size {
        return Err(InstallerError::new(
            "preserve_data_changed",
            "servers.instance_data_unsafe",
        ));
    }
    output
        .sync_all()
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))
}

fn create_private_directory(path: &Path) -> Result<(), InstallerError> {
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))
}

fn reject_unsafe_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), InstallerError> {
    if metadata_is_link_like(metadata) || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(InstallerError::new(
            "preserve_data_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    if crate::services::secure_fs::file_has_multiple_links(path, metadata)
        .map_err(|error| InstallerError::internal("preserve_data_failed", error))?
    {
        return Err(InstallerError::new(
            "preserve_data_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

async fn write_configuration(
    instance_root: &Path,
    staging: &Path,
    settings: &Value,
) -> Result<(), InstallerError> {
    let port = settings
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(19_132);
    if !(1..=65_535).contains(&port) {
        return Err(InstallerError::new(
            "bedrock_port_invalid",
            "servers.settings_invalid",
        ));
    }
    let port_v6 = settings
        .get("port_v6")
        .and_then(Value::as_u64)
        .unwrap_or(19_133);
    if !(1..=65_535).contains(&port_v6) || port == port_v6 {
        return Err(InstallerError::new(
            "bedrock_port_invalid",
            "servers.settings_invalid",
        ));
    }
    let existing = match read_bounded_regular_text(
        &instance_root.join("game"),
        "server.properties",
        1024 * 1024,
    )
    .await?
    {
        Some(contents) => contents,
        None => read_bounded_regular_text(staging, "server.properties", 1024 * 1024)
            .await?
            .unwrap_or_default(),
    };
    let mut updates = vec![
        ("server-port", port.to_string()),
        ("server-portv6", port_v6.to_string()),
        (
            "enable-lan-visibility",
            settings
                .get("enable_lan_visibility")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .to_string(),
        ),
    ];
    for (setting, property) in [
        ("server_name", "server-name"),
        ("level_name", "level-name"),
        ("gamemode", "gamemode"),
        ("difficulty", "difficulty"),
        (
            "default_player_permission_level",
            "default-player-permission-level",
        ),
    ] {
        if let Some(value) = settings.get(setting).and_then(Value::as_str) {
            updates.push((property, value.to_string()));
        }
    }
    for (setting, property) in [
        ("max_players", "max-players"),
        ("view_distance", "view-distance"),
        ("tick_distance", "tick-distance"),
        ("player_idle_timeout", "player-idle-timeout"),
    ] {
        if let Some(value) = settings.get(setting).and_then(Value::as_u64) {
            updates.push((property, value.to_string()));
        }
    }
    for (setting, property) in [
        ("online_mode", "online-mode"),
        ("allow_list", "allow-list"),
        ("texturepack_required", "texturepack-required"),
    ] {
        if let Some(value) = settings.get(setting).and_then(Value::as_bool) {
            updates.push((property, value.to_string()));
        }
    }
    let properties = merge_properties(&existing, &updates);
    let destination = staging.join("server.properties");
    let temporary = staging.join(format!(
        ".server-properties-{}.tmp",
        uuid::Uuid::new_v4().as_simple()
    ));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    file.write_all(properties.as_bytes())
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    if tokio::fs::try_exists(&destination)
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))?
    {
        tokio::fs::remove_file(&destination)
            .await
            .map_err(|error| InstallerError::internal("configuration_write_failed", error))?;
    }
    tokio::fs::rename(temporary, destination)
        .await
        .map_err(|error| InstallerError::internal("configuration_write_failed", error))
}

fn validate_eula(settings: &Value) -> Result<(), InstallerError> {
    if settings.get("eula_accepted").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "minecraft_eula_required",
            "servers.minecraft_eula_required",
        ))
    }
}

fn validate_settings(settings: &Value) -> Result<(), InstallerError> {
    let port = settings
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(19_132);
    let port_v6 = settings
        .get("port_v6")
        .and_then(Value::as_u64)
        .unwrap_or(19_133);
    if !(1..=65_535).contains(&port)
        || !(1..=65_535).contains(&port_v6)
        || port == port_v6
        || (settings
            .get("enable_lan_visibility")
            .and_then(Value::as_bool)
            == Some(true)
            && (port != 19_132 || port_v6 != 19_133))
    {
        return Err(InstallerError::new(
            "bedrock_port_invalid",
            "servers.settings_invalid",
        ));
    }
    if let Some(value) = settings.get("server_name").and_then(Value::as_str)
        && (value.is_empty()
            || value.len() > 64
            || value.contains(';')
            || value.chars().any(char::is_control))
    {
        return Err(InstallerError::new(
            "bedrock_server_name_invalid",
            "servers.settings_invalid",
        ));
    }
    if let Some(value) = settings.get("level_name").and_then(Value::as_str)
        && (value.is_empty()
            || value.len() > 64
            || value.trim() != value
            || value.ends_with(['.', ' '])
            || value.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
            || value.chars().any(char::is_control))
    {
        return Err(InstallerError::new(
            "bedrock_level_name_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), InstallerError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "provider_checksum_invalid",
            "servers.provider_response_invalid",
        ))
    }
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::installers::{InstallerSources, VerifiedSource};
    use axum::body::Bytes;
    use futures::stream;
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, io::Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn archive_fixture(executable: &str) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        for (name, contents) in [
            (executable, b"fixture executable".as_slice()),
            (
                "server.properties",
                b"server-port=19132\nfrom-archive=kept\n".as_slice(),
            ),
            ("allowlist.json", b"[]".as_slice()),
            ("permissions.json", b"[]".as_slice()),
        ] {
            archive
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn production_catalog_uses_dynamic_official_discovery_without_a_fake_checksum() {
        let context = InstallContext::official().unwrap();
        assert!(context.sources.bedrock_linux.is_none());
        assert!(context.sources.bedrock_windows.is_none());
        assert_eq!(
            context.sources.bedrock_download_api.as_str(),
            "https://net.web.minecraft-services.net/api/v1.0/download/links"
        );
        assert_eq!(
            context
                .sources
                .bedrock_download_fallback_api
                .as_ref()
                .map(Url::as_str),
            Some("https://net-secondary.web.minecraft-services.net/api/v1.0/download/links")
        );
    }

    #[tokio::test]
    async fn official_download_catalog_resolves_the_exact_platform_archive() {
        let base = Url::parse("http://127.0.0.1:32123/").unwrap();
        let sources = InstallerSources::fixture(&base);
        let api = sources.bedrock_download_api.to_string();
        let mut responses = BTreeMap::new();
        responses.insert(
            api,
            br#"{"result":{"links":[{"downloadType":"serverBedrockWindows","downloadUrl":"https://www.minecraft.net/bedrockdedicatedserver/bin-win/bedrock-server-1.26.33.2.zip"},{"downloadType":"serverBedrockLinux","downloadUrl":"https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.26.33.2.zip"}]}}"#.to_vec(),
        );
        let context = InstallContext::with_fixture_responses(sources, responses).unwrap();
        let resolved = discover_official_source(&context, BedrockPlatform::Linux)
            .await
            .unwrap();
        assert_eq!(resolved.version, "1.26.33.2");
        assert!(resolved.expected_sha256.is_none());
    }

    #[tokio::test]
    async fn official_download_catalog_uses_the_secondary_endpoint_on_failure() {
        let base = Url::parse("http://127.0.0.1:32123/").unwrap();
        let mut sources = InstallerSources::fixture(&base);
        let primary = sources.bedrock_download_api.to_string();
        let fallback = base.join("bedrock/downloads-secondary.json").unwrap();
        sources.bedrock_download_fallback_api = Some(fallback.clone());
        let mut responses = BTreeMap::new();
        responses.insert(primary, b"invalid-json".to_vec());
        responses.insert(
            fallback.to_string(),
            br#"{"result":{"links":[{"downloadType":"serverBedrockLinux","downloadUrl":"https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.26.33.2.zip"}]}}"#.to_vec(),
        );
        let context = InstallContext::with_fixture_responses(sources, responses).unwrap();
        let resolved = discover_official_source(&context, BedrockPlatform::Linux)
            .await
            .unwrap();
        assert_eq!(resolved.version, "1.26.33.2");
    }

    #[test]
    fn configured_sources_are_exact_https_official_archives() {
        let digest = "a".repeat(64);
        for (platform, url) in [
            (
                "linux",
                "https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.4.zip",
            ),
            (
                "win",
                "https://minecraft.azureedge.net/bin-win/bedrock-server-1.2.3.4.zip",
            ),
        ] {
            assert!(
                validate_configured_source(
                    &Url::parse(url).unwrap(),
                    "1.2.3.4",
                    &digest,
                    platform,
                )
                .is_ok()
            );
        }
        for url in [
            "http://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.4.zip",
            "https://evil.example/bin-linux/bedrock-server-1.2.3.4.zip",
            "https://www.minecraft.net/other/bedrock-server-1.2.3.4.zip",
            "https://www.minecraft.net/bedrockdedicatedserver/bin-win/bedrock-server-1.2.3.4.zip",
            "https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.4.zip?token=secret",
        ] {
            assert!(
                validate_configured_source(&Url::parse(url).unwrap(), "1.2.3.4", &digest, "linux",)
                    .is_err(),
                "accepted unsafe Bedrock source {url}"
            );
        }
        assert!(
            validate_configured_source(
                &Url::parse("https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-1.2.3.4.zip").unwrap(),
                "1.2.3.4",
                "not-a-digest",
                "linux",
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn linux_and_windows_archives_install_from_pinned_fixtures() {
        for platform in [BedrockPlatform::Linux, BedrockPlatform::Windows] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("instance");
            let staging = root.join(".staging/job");
            tokio::fs::create_dir_all(root.join("game/worlds/World"))
                .await
                .unwrap();
            tokio::fs::write(root.join("game/worlds/World/level.dat"), b"world")
                .await
                .unwrap();
            tokio::fs::write(root.join("game/allowlist.json"), b"[{\"name\":\"Owner\"}]")
                .await
                .unwrap();
            tokio::fs::write(
                root.join("game/server.properties"),
                b"server-port=19000\nunknown-property=preserved\n",
            )
            .await
            .unwrap();

            let base = Url::parse("http://127.0.0.1:32123/").unwrap();
            let mut sources = InstallerSources::fixture(&base);
            let bytes = archive_fixture(platform.executable());
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let source = VerifiedSource {
                url: base.join("bedrock/server.zip").unwrap(),
                sha256: digest.clone(),
                size: Some(bytes.len() as u64),
                version: "1.2.3.4".into(),
            };
            match platform {
                BedrockPlatform::Linux => sources.bedrock_linux = Some(source.clone()),
                BedrockPlatform::Windows => sources.bedrock_windows = Some(source.clone()),
            }
            let context = InstallContext::with_fixture_responses(
                sources,
                BTreeMap::from([(source.url.to_string(), bytes)]),
            )
            .unwrap();
            let settings = serde_json::json!({
                "version": "1.2.3.4",
                "eula_accepted": true,
                "port": 19144,
            });
            let result =
                install_bedrock_for_platform(&settings, &root, &staging, &context, platform)
                    .await
                    .unwrap();

            assert_eq!(result.installed_version, "1.2.3.4");
            assert_eq!(result.artifacts[0].sha256, digest);
            assert!(
                tokio::fs::try_exists(staging.join(platform.executable()))
                    .await
                    .unwrap()
            );
            assert_eq!(
                tokio::fs::read(staging.join("worlds/World/level.dat"))
                    .await
                    .unwrap(),
                b"world"
            );
            assert_eq!(
                tokio::fs::read(staging.join("allowlist.json"))
                    .await
                    .unwrap(),
                b"[{\"name\":\"Owner\"}]"
            );
            assert_eq!(
                tokio::fs::read(staging.join("permissions.json"))
                    .await
                    .unwrap(),
                b"[]"
            );
            let properties = tokio::fs::read_to_string(staging.join("server.properties"))
                .await
                .unwrap();
            assert!(properties.contains("server-port=19144"));
            assert!(properties.contains("unknown-property=preserved"));
        }
    }

    #[tokio::test]
    async fn owner_supplied_archive_is_verified_and_installs_on_linux_and_windows() {
        for platform in [BedrockPlatform::Linux, BedrockPlatform::Windows] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("instance");
            tokio::fs::create_dir_all(&root).await.unwrap();
            let job_id = uuid::Uuid::new_v4().to_string();
            let archive = archive_fixture(platform.executable());
            let sha256 = format!("{:x}", Sha256::digest(&archive));
            let artifact = store_local_archive(
                &root,
                &job_id,
                "1.2.3.4",
                &sha256,
                stream::iter([Ok::<_, std::io::Error>(Bytes::from(archive.clone()))]),
            )
            .await
            .unwrap();
            assert_eq!(artifact.sha256, sha256);
            assert_eq!(artifact.size, archive.len() as u64);

            let context = InstallContext::official()
                .unwrap()
                .with_bedrock_upload(&root, &job_id, "1.2.3.4")
                .await
                .unwrap();
            let staging = root.join(".staging").join(&job_id);
            let result = install_bedrock_for_platform(
                &serde_json::json!({
                    "version": "1.2.3.4",
                    "eula_accepted": true
                }),
                &root,
                &staging,
                &context,
                platform,
            )
            .await
            .unwrap();
            assert_eq!(result.artifacts, vec![artifact]);
            assert!(
                tokio::fs::try_exists(staging.join(platform.executable()))
                    .await
                    .unwrap()
            );
            assert!(
                tokio::fs::try_exists(staging.join("allowlist.json"))
                    .await
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn owner_supplied_archive_rejects_wrong_digest_without_publishing() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("instance");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let job_id = uuid::Uuid::new_v4().to_string();
        let result = store_local_archive(
            &root,
            &job_id,
            "1.2.3.4",
            &"0".repeat(64),
            stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
                b"not the digest",
            ))]),
        )
        .await;
        assert_eq!(result.unwrap_err().code, "bedrock_upload_checksum_mismatch");
        assert!(
            load_local_archive(&root, &job_id, "1.2.3.4")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn eula_is_explicit_for_bedrock() {
        assert_eq!(
            validate_eula(&serde_json::json!({})).unwrap_err().code,
            "minecraft_eula_required"
        );
    }
}
