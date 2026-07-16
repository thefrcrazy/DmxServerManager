use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
};

use flate2::read::GzDecoder;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::{
    ExpectedDigest, InstallContext, InstallerError, download_verified, extract_zip, read_json,
};

const MAX_JAVA_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct AdoptiumRelease {
    binaries: Vec<AdoptiumBinary>,
    version_data: AdoptiumVersion,
}

#[derive(Debug, Deserialize)]
struct AdoptiumVersion {
    major: u16,
    openjdk_version: String,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    architecture: String,
    image_type: String,
    jvm_impl: String,
    os: String,
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
    size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolchainMarker {
    schema: u8,
    major: u16,
    platform: String,
    openjdk_version: String,
    artifact_sha256: String,
}

pub async fn ensure_java(
    toolchain_root: &Path,
    major: u16,
    context: &InstallContext,
) -> Result<PathBuf, InstallerError> {
    validate_major(major)?;
    if let Ok(path) = installed_java_path(toolchain_root, major).await {
        return Ok(path);
    }
    ensure_directory_without_link(toolchain_root).await?;
    let platform = platform_name()?;
    let api_url = adoptium_api_url(&context.sources.adoptium_api_base, major)?;
    let releases: Vec<AdoptiumRelease> = read_json(context, &api_url).await?;
    let (version, package) = select_package(releases, major)?;
    validate_sha256(&package.checksum)?;
    let package_url = Url::parse(&package.link).map_err(|_| {
        InstallerError::new(
            "java_provider_url_invalid",
            "servers.provider_response_invalid",
        )
    })?;
    let expected_repository = format!("/adoptium/temurin{major}-binaries/releases/download/");
    if package_url.scheme() != "https"
        || package_url.host_str() != Some("github.com")
        || !package_url.path().starts_with(&expected_repository)
        || Path::new(&package.name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(package.name.as_str())
        || package
            .name
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | 0))
    {
        return Err(InstallerError::new(
            "java_package_invalid",
            "servers.provider_response_invalid",
        ));
    }
    let extension_ok = if cfg!(target_os = "windows") {
        package.name.ends_with(".zip")
    } else {
        package.name.ends_with(".tar.gz")
    };
    if !extension_ok {
        return Err(InstallerError::new(
            "java_archive_type_invalid",
            "servers.provider_response_invalid",
        ));
    }

    let operation = toolchain_root.join(format!(".staging-{}", uuid::Uuid::new_v4().as_simple()));
    tokio::fs::create_dir(&operation)
        .await
        .map_err(|error| InstallerError::internal("java_staging_failed", error))?;
    let archive = operation.join(&package.name);
    let install_result = async {
        let downloaded = download_verified(
            context,
            &package_url,
            &archive,
            MAX_JAVA_ARCHIVE_BYTES,
            Some(&ExpectedDigest::Sha256(package.checksum.clone())),
            Some(package.size),
        )
        .await?;
        let unpacked = operation.join("unpacked");
        if cfg!(target_os = "windows") {
            extract_zip(&archive, &unpacked, context.archive_limits, None).await?;
        } else {
            extract_tar_gz(&archive, &unpacked, context.archive_limits).await?;
        }
        let extracted_root = single_extracted_root(&unpacked).await?;
        validate_java_binary(&extracted_root).await?;
        let marker = ToolchainMarker {
            schema: 1,
            major,
            platform: platform.to_string(),
            openjdk_version: version,
            artifact_sha256: downloaded.sha256,
        };
        write_marker(&extracted_root, &marker).await?;

        let final_parent = toolchain_root.join(major.to_string());
        ensure_directory_without_link(&final_parent).await?;
        let final_directory = final_parent.join(platform);
        match tokio::fs::rename(&extracted_root, &final_directory).await {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                installed_java_path(toolchain_root, major).await?;
            }
            Err(error) => {
                return Err(InstallerError::internal(
                    "java_install_switch_failed",
                    error,
                ));
            }
        }
        installed_java_path(toolchain_root, major).await
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&operation).await;
    install_result
}

pub async fn installed_java_path(
    toolchain_root: &Path,
    major: u16,
) -> Result<PathBuf, InstallerError> {
    validate_major(major)?;
    let platform = platform_name()?;
    let directory = toolchain_root.join(major.to_string()).join(platform);
    let marker_bytes = tokio::fs::read(directory.join(".dmx-toolchain.json"))
        .await
        .map_err(|error| InstallerError::internal("java_runtime_unavailable", error))?;
    if marker_bytes.len() > 64 * 1024 {
        return Err(InstallerError::new(
            "java_marker_invalid",
            "servers.java_runtime_unavailable",
        ));
    }
    let marker: ToolchainMarker = serde_json::from_slice(&marker_bytes)
        .map_err(|error| InstallerError::internal("java_marker_invalid", error))?;
    if marker.schema != 1
        || marker.major != major
        || marker.platform != platform
        || marker.artifact_sha256.len() != 64
        || !marker
            .artifact_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(InstallerError::new(
            "java_marker_invalid",
            "servers.java_runtime_unavailable",
        ));
    }
    let executable = directory.join(java_relative_path());
    let metadata = tokio::fs::symlink_metadata(&executable)
        .await
        .map_err(|error| InstallerError::internal("java_runtime_unavailable", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "java_executable_invalid",
            "servers.java_runtime_unavailable",
        ));
    }
    Ok(executable)
}

fn select_package(
    releases: Vec<AdoptiumRelease>,
    expected_major: u16,
) -> Result<(String, AdoptiumPackage), InstallerError> {
    let expected_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err(InstallerError::new(
            "java_platform_unsupported",
            "servers.platform_unsupported",
        ));
    };
    select_package_for_os(releases, expected_major, expected_os)
}

fn select_package_for_os(
    releases: Vec<AdoptiumRelease>,
    expected_major: u16,
    expected_os: &str,
) -> Result<(String, AdoptiumPackage), InstallerError> {
    for release in releases {
        if release.version_data.major != expected_major {
            continue;
        }
        if let Some(binary) = release.binaries.into_iter().find(|binary| {
            binary.architecture == "x64"
                && binary.image_type == "jre"
                && binary.jvm_impl == "hotspot"
                && binary.os == expected_os
        }) {
            return Ok((release.version_data.openjdk_version, binary.package));
        }
    }
    Err(InstallerError::new(
        "java_release_unavailable",
        "servers.java_runtime_unavailable",
    ))
}

fn adoptium_api_url(base: &Url, major: u16) -> Result<Url, InstallerError> {
    validate_major(major)?;
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err(InstallerError::new(
            "java_platform_unsupported",
            "servers.platform_unsupported",
        ));
    };
    let mut url = base
        .join(&format!("assets/feature_releases/{major}/ga"))
        .map_err(|error| InstallerError::internal("java_provider_url_failed", error))?;
    url.query_pairs_mut()
        .append_pair("architecture", "x64")
        .append_pair("heap_size", "normal")
        .append_pair("image_type", "jre")
        .append_pair("jvm_impl", "hotspot")
        .append_pair("os", os)
        .append_pair("page", "0")
        .append_pair("page_size", "1")
        .append_pair("project", "jdk")
        .append_pair("sort_method", "DATE")
        .append_pair("sort_order", "DESC")
        .append_pair("vendor", "eclipse");
    Ok(url)
}

async fn extract_tar_gz(
    archive: &Path,
    destination: &Path,
    limits: super::ArchiveLimits,
) -> Result<(), InstallerError> {
    let archive = archive.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || extract_tar_gz_blocking(&archive, &destination, limits))
        .await
        .map_err(|error| InstallerError::internal("archive_worker_failed", error))?
}

fn extract_tar_gz_blocking(
    archive_path: &Path,
    destination: &Path,
    limits: super::ArchiveLimits,
) -> Result<(), InstallerError> {
    fs::create_dir(destination)
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    let archive_file = fs::File::open(archive_path)
        .map_err(|error| InstallerError::internal("archive_open_failed", error))?;
    let compressed_size = archive_file
        .metadata()
        .map_err(|error| InstallerError::internal("archive_open_failed", error))?
        .len();
    let decoder = GzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut count = 0_usize;
    let mut total = 0_u64;
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    for entry in archive
        .entries()
        .map_err(|error| InstallerError::internal("archive_invalid", error))?
    {
        count += 1;
        if count > limits.max_entries {
            return Err(InstallerError::new(
                "archive_too_many_entries",
                "servers.archive_too_many_entries",
            ));
        }
        let mut entry =
            entry.map_err(|error| InstallerError::internal("archive_invalid", error))?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() && !kind.is_hard_link() {
            return Err(InstallerError::new(
                "archive_link_or_special_file",
                "servers.archive_unsafe_entry",
            ));
        }
        let relative = entry
            .path()
            .map_err(|error| InstallerError::internal("archive_invalid", error))?
            .into_owned();
        validate_relative_path(&relative)?;
        if !seen.insert(relative.clone()) {
            return Err(InstallerError::new(
                "archive_duplicate_path",
                "servers.archive_duplicate_path",
            ));
        }
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|error| InstallerError::internal("archive_invalid", error))?
                .ok_or_else(|| {
                    InstallerError::new(
                        "archive_link_target_missing",
                        "servers.archive_unsafe_entry",
                    )
                })?
                .into_owned();
            let target = normalize_archive_link_target(&relative, &target, kind.is_symlink())?;
            links.push((relative, target));
            continue;
        }
        let size = entry.size();
        if size > limits.max_file_bytes {
            return Err(InstallerError::new(
                "archive_file_too_large",
                "servers.archive_quota_exceeded",
            ));
        }
        total = total.checked_add(size).ok_or_else(|| {
            InstallerError::new("archive_size_overflow", "servers.archive_quota_exceeded")
        })?;
        if total > limits.max_total_bytes
            || (total > 1024 * 1024
                && (compressed_size == 0
                    || total / compressed_size.max(1) > limits.max_compression_ratio))
        {
            return Err(InstallerError::new(
                "archive_quota_exceeded",
                "servers.archive_quota_exceeded",
            ));
        }
        let output = destination.join(&relative);
        if kind.is_dir() {
            fs::create_dir_all(output)
                .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
        }
        let mut output_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
        let copied = io::copy(&mut entry, &mut output_file)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
        if copied != size {
            return Err(InstallerError::new(
                "archive_size_mismatch",
                "servers.archive_invalid",
            ));
        }
        output_file
            .sync_all()
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    }
    materialize_archive_links(
        destination,
        links,
        limits,
        compressed_size,
        &mut count,
        &mut total,
    )?;
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), InstallerError> {
    if path.as_os_str().is_empty()
        || path.to_str().is_none()
        || path.to_string_lossy().contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(InstallerError::new(
            "archive_path_traversal",
            "servers.archive_invalid_path",
        ));
    }
    Ok(())
}

fn normalize_archive_link_target(
    link_path: &Path,
    target: &Path,
    relative_to_parent: bool,
) -> Result<PathBuf, InstallerError> {
    if target.as_os_str().is_empty()
        || target.to_str().is_none()
        || target.to_string_lossy().contains('\\')
        || target.is_absolute()
    {
        return Err(InstallerError::new(
            "archive_link_target_invalid",
            "servers.archive_unsafe_entry",
        ));
    }
    let mut components = Vec::<OsString>::new();
    if relative_to_parent && let Some(parent) = link_path.parent() {
        for component in parent.components() {
            if let Component::Normal(value) = component {
                components.push(value.to_os_string());
            }
        }
    }
    for component in target.components() {
        match component {
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err(InstallerError::new(
                        "archive_link_target_traversal",
                        "servers.archive_unsafe_entry",
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(InstallerError::new(
                    "archive_link_target_invalid",
                    "servers.archive_unsafe_entry",
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(InstallerError::new(
            "archive_link_target_invalid",
            "servers.archive_unsafe_entry",
        ));
    }
    Ok(components.into_iter().collect())
}

fn materialize_archive_links(
    destination: &Path,
    mut links: Vec<(PathBuf, PathBuf)>,
    limits: super::ArchiveLimits,
    compressed_size: u64,
    count: &mut usize,
    total: &mut u64,
) -> Result<(), InstallerError> {
    while !links.is_empty() {
        let mut deferred = Vec::new();
        let mut progressed = false;
        for (relative, target) in links {
            let source = destination.join(&target);
            match fs::symlink_metadata(&source) {
                Ok(metadata) if !metadata.file_type().is_symlink() => {
                    copy_materialized_entry(
                        &source,
                        &destination.join(&relative),
                        limits,
                        compressed_size,
                        count,
                        total,
                    )?;
                    progressed = true;
                }
                Ok(_) => {
                    return Err(InstallerError::new(
                        "archive_link_target_invalid",
                        "servers.archive_unsafe_entry",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    deferred.push((relative, target));
                }
                Err(error) => {
                    return Err(InstallerError::internal("archive_extract_failed", error));
                }
            }
        }
        if !progressed {
            return Err(InstallerError::new(
                "archive_link_target_missing",
                "servers.archive_unsafe_entry",
            ));
        }
        links = deferred;
    }
    Ok(())
}

fn copy_materialized_entry(
    source: &Path,
    destination: &Path,
    limits: super::ArchiveLimits,
    compressed_size: u64,
    count: &mut usize,
    total: &mut u64,
) -> Result<(), InstallerError> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(InstallerError::new(
            "archive_link_target_invalid",
            "servers.archive_unsafe_entry",
        ));
    }
    *count = (*count).checked_add(1).ok_or_else(|| {
        InstallerError::new(
            "archive_too_many_entries",
            "servers.archive_too_many_entries",
        )
    })?;
    if *count > limits.max_entries {
        return Err(InstallerError::new(
            "archive_too_many_entries",
            "servers.archive_too_many_entries",
        ));
    }
    if metadata.is_dir() {
        if destination.starts_with(source) {
            return Err(InstallerError::new(
                "archive_link_target_recursive",
                "servers.archive_unsafe_entry",
            ));
        }
        fs::create_dir(destination)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
        for entry in fs::read_dir(source)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?
        {
            let entry =
                entry.map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
            copy_materialized_entry(
                &entry.path(),
                &destination.join(entry.file_name()),
                limits,
                compressed_size,
                count,
                total,
            )?;
        }
        return Ok(());
    }
    let size = metadata.len();
    if size > limits.max_file_bytes {
        return Err(InstallerError::new(
            "archive_file_too_large",
            "servers.archive_quota_exceeded",
        ));
    }
    *total = total.checked_add(size).ok_or_else(|| {
        InstallerError::new("archive_size_overflow", "servers.archive_quota_exceeded")
    })?;
    if *total > limits.max_total_bytes
        || (*total > 1024 * 1024
            && (compressed_size == 0
                || *total / compressed_size.max(1) > limits.max_compression_ratio))
    {
        return Err(InstallerError::new(
            "archive_quota_exceeded",
            "servers.archive_quota_exceeded",
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    }
    let mut input = fs::File::open(source)
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    let copied = io::copy(&mut input, &mut output)
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
    if copied != size {
        return Err(InstallerError::new(
            "archive_size_mismatch",
            "servers.archive_invalid",
        ));
    }
    output
        .sync_all()
        .map_err(|error| InstallerError::internal("archive_extract_failed", error))
}

async fn single_extracted_root(unpacked: &Path) -> Result<PathBuf, InstallerError> {
    let mut entries = tokio::fs::read_dir(unpacked)
        .await
        .map_err(|error| InstallerError::internal("java_archive_invalid", error))?;
    let first = entries
        .next_entry()
        .await
        .map_err(|error| InstallerError::internal("java_archive_invalid", error))?
        .ok_or_else(|| {
            InstallerError::new("java_archive_empty", "servers.provider_response_invalid")
        })?;
    if entries
        .next_entry()
        .await
        .map_err(|error| InstallerError::internal("java_archive_invalid", error))?
        .is_some()
        || !first
            .file_type()
            .await
            .map_err(|error| InstallerError::internal("java_archive_invalid", error))?
            .is_dir()
    {
        return Err(InstallerError::new(
            "java_archive_layout_invalid",
            "servers.provider_response_invalid",
        ));
    }
    Ok(first.path())
}

async fn validate_java_binary(root: &Path) -> Result<(), InstallerError> {
    let executable = root.join(java_relative_path());
    let metadata = tokio::fs::symlink_metadata(&executable)
        .await
        .map_err(|error| InstallerError::internal("java_executable_missing", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "java_executable_invalid",
            "servers.provider_response_invalid",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| InstallerError::internal("java_permissions_failed", error))?;
    }
    Ok(())
}

async fn write_marker(root: &Path, marker: &ToolchainMarker) -> Result<(), InstallerError> {
    let contents = serde_json::to_vec_pretty(marker)
        .map_err(|error| InstallerError::internal("java_marker_failed", error))?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(".dmx-toolchain.json"))
        .await
        .map_err(|error| InstallerError::internal("java_marker_failed", error))?;
    use tokio::io::AsyncWriteExt;
    file.write_all(&contents)
        .await
        .map_err(|error| InstallerError::internal("java_marker_failed", error))?;
    file.sync_all()
        .await
        .map_err(|error| InstallerError::internal("java_marker_failed", error))
}

async fn ensure_directory_without_link(path: &Path) -> Result<(), InstallerError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(InstallerError::new(
            "java_toolchain_path_unsafe",
            "servers.java_runtime_unavailable",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(path)
                .await
                .map_err(|error| InstallerError::internal("java_toolchain_path_failed", error))
        }
        Err(error) => Err(InstallerError::internal(
            "java_toolchain_path_failed",
            error,
        )),
    }
}

fn java_relative_path() -> &'static str {
    if cfg!(target_os = "windows") {
        "bin/java.exe"
    } else {
        "bin/java"
    }
}

fn platform_name() -> Result<&'static str, InstallerError> {
    if cfg!(target_os = "windows") {
        Ok("windows-x64")
    } else if cfg!(target_os = "linux") {
        Ok("linux-x64")
    } else {
        Err(InstallerError::new(
            "java_platform_unsupported",
            "servers.platform_unsupported",
        ))
    }
}

fn validate_major(major: u16) -> Result<(), InstallerError> {
    if matches!(major, 8 | 16 | 17 | 21 | 25) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "java_version_unsupported",
            "servers.java_runtime_unavailable",
        ))
    }
}

fn validate_sha256(value: &str) -> Result<(), InstallerError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "java_checksum_invalid",
            "servers.provider_response_invalid",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    #[test]
    fn java_majors_are_a_closed_set() {
        for major in [8, 16, 17, 21, 25] {
            assert!(validate_major(major).is_ok());
        }
        assert!(validate_major(22).is_err());
    }

    #[test]
    fn traversal_paths_are_rejected() {
        assert!(validate_relative_path(Path::new("jdk/bin/java")).is_ok());
        assert!(validate_relative_path(Path::new("../escape")).is_err());
        assert!(validate_relative_path(Path::new("/absolute")).is_err());
    }

    #[test]
    fn provider_selection_revalidates_platform_and_java_major() {
        let release = AdoptiumRelease {
            version_data: AdoptiumVersion {
                major: 21,
                openjdk_version: "21.0.11+10-LTS".to_string(),
            },
            binaries: vec![AdoptiumBinary {
                architecture: "x64".to_string(),
                image_type: "jre".to_string(),
                jvm_impl: "hotspot".to_string(),
                os: "linux".to_string(),
                package: AdoptiumPackage {
                    checksum: "a".repeat(64),
                    link: "https://github.com/adoptium/temurin21-binaries/releases/download/test/jre.tar.gz".to_string(),
                    name: "jre.tar.gz".to_string(),
                    size: 1,
                },
            }],
        };
        let (_, package) = select_package_for_os(vec![release], 21, "linux").unwrap();
        assert_eq!(package.name, "jre.tar.gz");
    }

    #[tokio::test]
    async fn tar_extraction_writes_only_regular_bounded_entries() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("jre.tar.gz");
        let file = fs::File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let contents = b"fake-java";
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "jdk/bin/java", Cursor::new(contents))
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let destination = root.path().join("unpacked");
        extract_tar_gz(
            &archive_path,
            &destination,
            super::super::ArchiveLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(destination.join("jdk/bin/java")).unwrap(),
            contents
        );
    }

    #[tokio::test]
    async fn tar_extraction_materializes_safe_internal_links() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("jre.tar.gz");
        let file = fs::File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let contents = b"license";
        let mut file_header = tar::Header::new_gnu();
        file_header.set_size(contents.len() as u64);
        file_header.set_mode(0o644);
        file_header.set_cksum();
        builder
            .append_data(
                &mut file_header,
                "jdk/legal/ASSEMBLY_EXCEPTION",
                Cursor::new(contents),
            )
            .unwrap();
        let mut link_header = tar::Header::new_gnu();
        link_header.set_entry_type(tar::EntryType::Symlink);
        link_header.set_size(0);
        link_header.set_mode(0o644);
        link_header
            .set_link_name("../legal/ASSEMBLY_EXCEPTION")
            .unwrap();
        link_header.set_cksum();
        builder
            .append_data(&mut link_header, "jdk/lib/ASSEMBLY_EXCEPTION", io::empty())
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let destination = root.path().join("unpacked");
        extract_tar_gz(
            &archive_path,
            &destination,
            super::super::ArchiveLimits::default(),
        )
        .await
        .unwrap();
        let materialized = destination.join("jdk/lib/ASSEMBLY_EXCEPTION");
        assert_eq!(fs::read(&materialized).unwrap(), contents);
        assert!(
            !fs::symlink_metadata(materialized)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn archive_link_targets_cannot_escape_the_archive() {
        assert!(
            normalize_archive_link_target(
                Path::new("jdk/lib/unsafe"),
                Path::new("../../../escape"),
                true,
            )
            .is_err()
        );
    }
}
