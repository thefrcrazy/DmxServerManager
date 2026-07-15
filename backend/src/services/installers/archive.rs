use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use tokio::task;
use zip::ZipArchive;

use super::InstallerError;

#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_compression_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_file_bytes: 8 * 1024 * 1024 * 1024,
            max_total_bytes: 16 * 1024 * 1024 * 1024,
            max_compression_ratio: 250,
        }
    }
}

/// Extracts an archive into a new staging directory. ZIP paths, duplicate paths,
/// links, special files, decompression bombs and quota overruns are rejected.
pub async fn extract_zip(
    archive: &Path,
    destination: &Path,
    limits: ArchiveLimits,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<Vec<PathBuf>, InstallerError> {
    let archive = archive.to_path_buf();
    let destination = destination.to_path_buf();
    let selected_paths = selected_paths.cloned();
    task::spawn_blocking(move || {
        extract_zip_blocking(&archive, &destination, limits, selected_paths.as_ref())
    })
    .await
    .map_err(|error| InstallerError::internal("archive_worker_failed", error))?
}

fn extract_zip_blocking(
    archive_path: &Path,
    destination: &Path,
    limits: ArchiveLimits,
    selected_paths: Option<&BTreeSet<String>>,
) -> Result<Vec<PathBuf>, InstallerError> {
    ensure_clean_destination(destination)?;
    let file = std::fs::File::open(archive_path)
        .map_err(|error| InstallerError::internal("archive_open_failed", error))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| InstallerError::internal("archive_invalid", error))?;
    if archive.len() > limits.max_entries {
        return Err(InstallerError::new(
            "archive_too_many_entries",
            "servers.archive_too_many_entries",
        ));
    }

    let mut total = 0_u64;
    let mut seen = BTreeSet::new();
    let mut extracted = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| InstallerError::internal("archive_invalid", error))?;
        let relative = validated_entry_path(&entry)?;
        let normalized = relative
            .to_str()
            .ok_or_else(|| {
                InstallerError::new("archive_non_utf8_path", "servers.archive_invalid_path")
            })?
            .replace('\\', "/");
        if !seen.insert(normalized.clone()) {
            return Err(InstallerError::new(
                "archive_duplicate_path",
                "servers.archive_duplicate_path",
            ));
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
        if total > limits.max_total_bytes {
            return Err(InstallerError::new(
                "archive_too_large",
                "servers.archive_quota_exceeded",
            ));
        }
        validate_compression_ratio(entry.compressed_size(), size, limits)?;

        if selected_paths.is_some_and(|paths| !paths.contains(&normalized)) {
            continue;
        }

        let output = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)
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
            .open(&output)
            .map_err(|error| InstallerError::internal("archive_extract_failed", error))?;
        // Never trust the central-directory size as an I/O limit. A malformed
        // entry may not write one byte beyond the amount that was preflighted.
        let copied = io::copy(
            &mut entry.by_ref().take(size.saturating_add(1)),
            &mut output_file,
        )
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
        extracted.push(output);
    }
    Ok(extracted)
}

fn ensure_clean_destination(destination: &Path) -> Result<(), InstallerError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            InstallerError::new("unsafe_staging_path", "servers.unsafe_staging_path"),
        ),
        Ok(_) => {
            if fs::read_dir(destination)
                .map_err(|error| InstallerError::internal("staging_read_failed", error))?
                .next()
                .is_some()
            {
                return Err(InstallerError::new(
                    "staging_not_empty",
                    "servers.staging_not_empty",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(destination)
            .map_err(|error| InstallerError::internal("staging_create_failed", error)),
        Err(error) => Err(InstallerError::internal("staging_read_failed", error)),
    }
}

fn validated_entry_path<R: io::Read>(
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<PathBuf, InstallerError> {
    let path = entry.enclosed_name().ok_or_else(|| {
        InstallerError::new("archive_path_traversal", "servers.archive_invalid_path")
    })?;
    if path.as_os_str().is_empty()
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
    if entry.name().contains('\\')
        || entry.name().contains(':')
        || entry.name().contains('\0')
        || entry.name().chars().any(char::is_control)
    {
        return Err(InstallerError::new(
            "archive_invalid_path",
            "servers.archive_invalid_path",
        ));
    }

    if let Some(mode) = entry.unix_mode() {
        let kind = mode & 0o170_000;
        let regular = kind == 0 || kind == 0o100_000;
        let directory = kind == 0o040_000 && entry.is_dir();
        if !regular && !directory {
            return Err(InstallerError::new(
                "archive_link_or_special_file",
                "servers.archive_unsafe_entry",
            ));
        }
    }
    Ok(path.to_path_buf())
}

fn validate_compression_ratio(
    compressed: u64,
    uncompressed: u64,
    limits: ArchiveLimits,
) -> Result<(), InstallerError> {
    if uncompressed == 0 {
        return Ok(());
    }
    if compressed == 0
        || (uncompressed > 1024 * 1024
            && uncompressed / compressed.max(1) > limits.max_compression_ratio)
    {
        return Err(InstallerError::new(
            "archive_compression_ratio_exceeded",
            "servers.archive_quota_exceeded",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    fn fixture(entries: &[(&str, &[u8], Option<u32>)]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("fixture.zip");
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        for (name, contents, mode) in entries {
            let mut options = SimpleFileOptions::default();
            if let Some(mode) = mode {
                options = options.unix_permissions(*mode);
            }
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
        (directory, archive_path)
    }

    #[tokio::test]
    async fn extracts_regular_files_without_honouring_archive_permissions() {
        let (directory, archive) = fixture(&[("Server/server.jar", b"jar", Some(0o100777))]);
        let output = directory.path().join("output");
        let extracted = extract_zip(&archive, &output, ArchiveLimits::default(), None)
            .await
            .unwrap();
        assert_eq!(extracted, vec![output.join("Server/server.jar")]);
        assert_eq!(
            std::fs::read(output.join("Server/server.jar")).unwrap(),
            b"jar"
        );
    }

    #[tokio::test]
    async fn rejects_traversal_and_links() {
        let (directory, archive) = fixture(&[("../escaped", b"no", None)]);
        let error = extract_zip(
            &archive,
            &directory.path().join("output"),
            ArchiveLimits::default(),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "archive_path_traversal");

        let directory = tempdir().unwrap();
        let archive = directory.path().join("symlink.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.add_symlink("link", "target", SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();
        let error = extract_zip(
            &archive,
            &directory.path().join("output"),
            ArchiveLimits::default(),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "archive_link_or_special_file");

        let (directory, archive) = fixture(&[("server.jar:alternate", b"no", None)]);
        let error = extract_zip(
            &archive,
            &directory.path().join("output"),
            ArchiveLimits::default(),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "archive_invalid_path");
    }

    #[tokio::test]
    async fn enforces_file_and_entry_quotas() {
        let (directory, archive) = fixture(&[("large", b"12345", None)]);
        let limits = ArchiveLimits {
            max_file_bytes: 4,
            ..ArchiveLimits::default()
        };
        let error = extract_zip(&archive, &directory.path().join("output"), limits, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "archive_file_too_large");
    }
}
