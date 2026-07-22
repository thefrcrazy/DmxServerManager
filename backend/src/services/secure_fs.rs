use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use axum::body::Bytes;
use futures::{Stream, StreamExt};
use serde::Serialize;

use crate::core::error::AppError;

pub const MAX_UPLOAD_BYTES: u64 = 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_LIST_ENTRIES: usize = 10_000;
pub const MAX_BACKUP_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedEntry {
    pub name: String,
    pub path: String,
    pub kind: ManagedEntryKind,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BackupFile {
    pub source: PathBuf,
    pub archive_name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
struct RelativePath {
    path: PathBuf,
    normalized: String,
}

impl RelativePath {
    fn parse(raw: &str, allow_root: bool) -> Result<Self, AppError> {
        Self::parse_with_policy(raw, allow_root, false)
    }

    fn declared(raw: &str) -> Result<Self, AppError> {
        Self::parse_with_policy(raw, false, true)
    }

    fn parse_with_policy(
        raw: &str,
        allow_root: bool,
        allow_protected: bool,
    ) -> Result<Self, AppError> {
        if raw.len() > 1_024
            || raw.contains('\0')
            || raw.contains('\\')
            || raw.contains(':')
            || raw.chars().any(char::is_control)
        {
            return Err(AppError::BadRequest("files.invalid_path".into()));
        }
        if raw.starts_with('/') {
            return Err(AppError::BadRequest("files.invalid_path".into()));
        }
        let raw = raw.trim_end_matches('/');
        if raw.is_empty() {
            if allow_root {
                return Ok(Self {
                    path: PathBuf::new(),
                    normalized: String::new(),
                });
            }
            return Err(AppError::BadRequest("files.path_required".into()));
        }

        let path = Path::new(raw);
        let mut normalized = Vec::new();
        for component in path.components() {
            let Component::Normal(component) = component else {
                return Err(AppError::BadRequest("files.invalid_path".into()));
            };
            let value = component
                .to_str()
                .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?;
            if value.is_empty() || value == "." || value == ".." || value.len() > 255 {
                return Err(AppError::BadRequest("files.invalid_path".into()));
            }
            normalized.push(value);
        }
        let normalized = normalized.join("/");
        if !allow_protected && is_protected_path(&normalized) {
            return Err(AppError::Forbidden("files.protected_path".into()));
        }
        Ok(Self {
            path: PathBuf::from(&normalized),
            normalized,
        })
    }

    fn from_declared(path: &Path) -> Result<Self, AppError> {
        let value = path
            .to_str()
            .ok_or_else(|| AppError::BadRequest("backups.invalid_declared_path".into()))?
            .replace('\\', "/");
        Self::parse(&value, false)
            .map_err(|_| AppError::BadRequest("backups.invalid_declared_path".into()))
    }
}

pub async fn list_directory(root: &Path, relative: &str) -> Result<Vec<ManagedEntry>, AppError> {
    let root = root.to_path_buf();
    let relative = RelativePath::parse(relative, true)?;
    tokio::task::spawn_blocking(move || list_directory_blocking(&root, &relative))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

pub async fn validate_directory_root(path: &Path) -> Result<(), AppError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || checked_root(&path).map(|_| ()))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

pub async fn open_regular_file(
    root: &Path,
    relative: &str,
) -> Result<(tokio::fs::File, u64), AppError> {
    let root = root.to_path_buf();
    let relative = RelativePath::parse(relative, false)?;
    let (file, size) = tokio::task::spawn_blocking(move || {
        let path = resolve_existing(&root, &relative)?;
        let metadata = fs::symlink_metadata(&path).map_err(map_not_found)?;
        if !metadata.is_file() || is_link_like(&metadata) {
            return Err(AppError::BadRequest("files.not_regular".into()));
        }
        reject_hardlinked_file(&path, &metadata)?;
        let file = open_read_no_follow(&path)?;
        validate_open_file(&file)?;
        Ok((file, metadata.len()))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok((tokio::fs::File::from_std(file), size))
}

/// Opens a path selected from a server-profile-owned allowlist. This keeps all
/// traversal, symlink, reparse-point and hardlink protections while allowing a
/// dedicated configuration API to reach files hidden from the generic browser
/// (for example PalWorldSettings.ini).
pub(crate) async fn open_declared_regular_file(
    root: &Path,
    relative: &str,
) -> Result<(tokio::fs::File, u64), AppError> {
    let root = root.to_path_buf();
    let relative = RelativePath::declared(relative)?;
    let (file, size) = tokio::task::spawn_blocking(move || {
        let path = resolve_existing(&root, &relative)?;
        let metadata = fs::symlink_metadata(&path).map_err(map_not_found)?;
        if !metadata.is_file() || is_link_like(&metadata) {
            return Err(AppError::BadRequest("files.not_regular".into()));
        }
        reject_hardlinked_file(&path, &metadata)?;
        let file = open_read_no_follow(&path)?;
        validate_open_file(&file)?;
        Ok((file, metadata.len()))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok((tokio::fs::File::from_std(file), size))
}

pub async fn write_stream<S, E>(
    root: &Path,
    relative: &str,
    mut stream: S,
    max_bytes: u64,
) -> Result<u64, AppError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let relative = RelativePath::parse(relative, false)?;
    let (temporary, destination, prepared_file, mut cleanup) =
        prepare_write(root, &relative).await?;
    // Declared after `cleanup` so cancellation drops the open handle before the synchronous
    // best-effort removal in `TemporaryWriteCleanup` (required on Windows).
    let mut file = prepared_file;
    let mut written = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AppError::BadRequest("files.upload_invalid".into()))?;
        written = written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| AppError::BadRequest("files.upload_too_large".into()))?;
        if written > max_bytes {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(AppError::BadRequest("files.upload_too_large".into()));
        }
        if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    file.sync_all().await?;
    drop(file);
    if let Err(error) = replace_regular_file(&temporary, &destination).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    cleanup.0 = None;
    Ok(written)
}

struct TemporaryWriteCleanup(Option<PathBuf>);

impl Drop for TemporaryWriteCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

pub async fn write_bytes(
    root: &Path,
    relative: &str,
    contents: Bytes,
    max_bytes: usize,
) -> Result<u64, AppError> {
    if contents.len() > max_bytes {
        return Err(AppError::BadRequest("files.upload_too_large".into()));
    }
    write_stream(
        root,
        relative,
        futures::stream::iter([Ok::<_, std::convert::Infallible>(contents)]),
        max_bytes as u64,
    )
    .await
}

pub(crate) async fn write_declared_bytes(
    root: &Path,
    relative: &str,
    contents: Bytes,
    max_bytes: usize,
) -> Result<u64, AppError> {
    if contents.len() > max_bytes {
        return Err(AppError::BadRequest("files.upload_too_large".into()));
    }
    let relative = RelativePath::declared(relative)?;
    let prepare_root = root.to_path_buf();
    let prepare_relative = relative.clone();
    tokio::task::spawn_blocking(move || ensure_declared_parent(&prepare_root, &prepare_relative))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))??;

    let (temporary, destination, prepared_file, mut cleanup) =
        prepare_write(root, &relative).await?;
    let mut file = prepared_file;
    if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut file, &contents).await {
        drop(file);
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    file.sync_all().await?;
    drop(file);
    if let Err(error) = replace_regular_file(&temporary, &destination).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    cleanup.0 = None;
    Ok(contents.len() as u64)
}

pub async fn create_directory(root: &Path, relative: &str) -> Result<(), AppError> {
    let root = root.to_path_buf();
    let relative = RelativePath::parse(relative, false)?;
    tokio::task::spawn_blocking(move || {
        let destination = resolve_for_create(&root, &relative)?;
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
            .create(&destination)
            .map_err(|error| match error.kind() {
                io::ErrorKind::AlreadyExists => AppError::Conflict("files.exists".into()),
                _ => error.into(),
            })
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

pub async fn delete_entry(root: &Path, relative: &str) -> Result<(), AppError> {
    let root = root.to_path_buf();
    let relative = RelativePath::parse(relative, false)?;
    tokio::task::spawn_blocking(move || {
        let path = resolve_existing(&root, &relative)?;
        let metadata = fs::symlink_metadata(&path).map_err(map_not_found)?;
        if is_link_like(&metadata) {
            return Err(AppError::Forbidden("files.links_forbidden".into()));
        }
        if metadata.is_file() {
            reject_hardlinked_file(&path, &metadata)?;
            fs::remove_file(path)?;
            Ok(())
        } else if metadata.is_dir() {
            fs::remove_dir(path).map_err(|error| match error.kind() {
                io::ErrorKind::DirectoryNotEmpty => {
                    AppError::Conflict("files.directory_not_empty".into())
                }
                _ => error.into(),
            })
        } else {
            Err(AppError::BadRequest("files.not_regular".into()))
        }
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

pub async fn scan_backup_files(
    root: &Path,
    declared_paths: &[PathBuf],
    max_total_bytes: u64,
) -> Result<Vec<BackupFile>, AppError> {
    let root = root.to_path_buf();
    let declared_paths = declared_paths.to_vec();
    tokio::task::spawn_blocking(move || {
        scan_backup_files_blocking(&root, &declared_paths, max_total_bytes)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

pub fn normalize_declared_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, AppError> {
    let mut paths = paths
        .iter()
        .map(|path| RelativePath::from_declared(path))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort_by(|left, right| {
        left.path
            .components()
            .count()
            .cmp(&right.path.components().count())
            .then_with(|| left.normalized.cmp(&right.normalized))
    });
    let mut normalized = Vec::<RelativePath>::new();
    for candidate in paths {
        if normalized.iter().any(|parent| {
            candidate.normalized == parent.normalized
                || candidate
                    .normalized
                    .strip_prefix(&parent.normalized)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            continue;
        }
        normalized.push(candidate);
    }
    Ok(normalized.into_iter().map(|value| value.path).collect())
}

pub fn validate_restore_entry(path: &Path, declared_paths: &[PathBuf]) -> Result<(), AppError> {
    let entry = RelativePath::from_declared(path)
        .map_err(|_| AppError::BadRequest("backups.archive_path_not_allowed".into()))?;
    let declared = declared_paths
        .iter()
        .map(|path| RelativePath::from_declared(path))
        .collect::<Result<Vec<_>, _>>()?;
    if declared.iter().any(|allowed| {
        entry.normalized == allowed.normalized
            || entry
                .normalized
                .strip_prefix(&allowed.normalized)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }) {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "backups.archive_path_not_allowed".into(),
        ))
    }
}

pub fn open_backup_source(file: &BackupFile) -> Result<File, AppError> {
    let metadata = fs::symlink_metadata(&file.source).map_err(map_not_found)?;
    if !metadata.is_file() || is_link_like(&metadata) || metadata.len() != file.size_bytes {
        return Err(AppError::BadRequest("backups.source_changed".into()));
    }
    reject_hardlinked_file(&file.source, &metadata)?;
    let opened = open_read_no_follow(&file.source)?;
    validate_open_file(&opened)?;
    Ok(opened)
}

fn list_directory_blocking(
    root: &Path,
    relative: &RelativePath,
) -> Result<Vec<ManagedEntry>, AppError> {
    let directory = resolve_existing(root, relative)?;
    if !fs::symlink_metadata(&directory)
        .map_err(map_not_found)?
        .is_dir()
    {
        return Err(AppError::BadRequest("files.not_directory".into()));
    }
    let mut entries = Vec::new();
    for item in fs::read_dir(directory)? {
        if entries.len() >= MAX_LIST_ENTRIES {
            return Err(AppError::BadRequest("files.too_many_entries".into()));
        }
        let item = item?;
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| AppError::BadRequest("files.non_utf8_name".into()))?;
        let child_relative = if relative.normalized.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", relative.normalized)
        };
        if is_protected_path(&child_relative) {
            continue;
        }
        let metadata = fs::symlink_metadata(item.path())?;
        if is_link_like(&metadata)
            || (!metadata.is_file() && !metadata.is_dir())
            || (metadata.is_file() && file_has_multiple_links(&item.path(), &metadata)?)
        {
            continue;
        }
        entries.push(ManagedEntry {
            name,
            path: child_relative,
            kind: if metadata.is_dir() {
                ManagedEntryKind::Directory
            } else {
                ManagedEntryKind::File
            },
            size_bytes: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
            modified_at: metadata.modified().ok().map(system_time),
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

async fn prepare_write(
    root: &Path,
    relative: &RelativePath,
) -> Result<(PathBuf, PathBuf, tokio::fs::File, TemporaryWriteCleanup), AppError> {
    let root = root.to_path_buf();
    let relative = relative.clone();
    let (temporary, destination, file, cleanup) = tokio::task::spawn_blocking(move || {
        let destination = resolve_for_create(&root, &relative)?;
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if !metadata.is_file() || is_link_like(&metadata) {
                return Err(AppError::BadRequest("files.not_regular".into()));
            }
            reject_hardlinked_file(&destination, &metadata)?;
        }
        let parent = destination
            .parent()
            .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?;
        let temporary = parent.join(format!(".dmx-write-{}", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(&temporary)?;
        let cleanup = TemporaryWriteCleanup(Some(temporary.clone()));
        Ok((temporary, destination, file, cleanup))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    Ok((
        temporary,
        destination,
        tokio::fs::File::from_std(file),
        cleanup,
    ))
}

async fn replace_regular_file(temporary: &Path, destination: &Path) -> Result<(), AppError> {
    #[cfg(not(windows))]
    {
        tokio::fs::rename(temporary, destination).await?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let rollback =
            destination.with_file_name(format!(".dmx-write-rollback-{}", uuid::Uuid::new_v4()));
        let existed = tokio::fs::try_exists(destination).await?;
        if existed {
            tokio::fs::rename(destination, &rollback).await?;
        }
        if let Err(error) = tokio::fs::rename(temporary, destination).await {
            if existed {
                let _ = tokio::fs::rename(&rollback, destination).await;
            }
            return Err(error.into());
        }
        if existed {
            tokio::fs::remove_file(rollback).await?;
        }
        Ok(())
    }
}

fn resolve_existing(root: &Path, relative: &RelativePath) -> Result<PathBuf, AppError> {
    let canonical_root = checked_root(root)?;
    let mut current = root.to_path_buf();
    for component in relative.path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(map_not_found)?;
        if is_link_like(&metadata) {
            return Err(AppError::Forbidden("files.links_forbidden".into()));
        }
    }
    let canonical = fs::canonicalize(&current).map_err(map_not_found)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(AppError::Forbidden("files.path_escape".into()));
    }
    Ok(current)
}

fn resolve_for_create(root: &Path, relative: &RelativePath) -> Result<PathBuf, AppError> {
    let parent = relative
        .path
        .parent()
        .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?;
    let parent_value = parent
        .to_str()
        .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?
        .replace('\\', "/");
    let parent = RelativePath::parse(&parent_value, true)?;
    let parent = resolve_existing(root, &parent)?;
    if !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(AppError::BadRequest("files.parent_not_directory".into()));
    }
    let name = relative
        .path
        .file_name()
        .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?;
    Ok(parent.join(name))
}

fn ensure_declared_parent(root: &Path, relative: &RelativePath) -> Result<(), AppError> {
    let canonical_root = checked_root(root)?;
    let parent = relative
        .path
        .parent()
        .ok_or_else(|| AppError::BadRequest("files.invalid_path".into()))?;
    let mut current = root.to_path_buf();
    for component in parent.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.is_dir() || is_link_like(&metadata) {
                    return Err(AppError::Forbidden("files.links_forbidden".into()));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                #[cfg(unix)]
                let mut builder = fs::DirBuilder::new();
                #[cfg(not(unix))]
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
        let canonical = fs::canonicalize(&current)?;
        if !canonical.starts_with(&canonical_root) {
            return Err(AppError::Forbidden("files.path_escape".into()));
        }
    }
    Ok(())
}

fn checked_root(root: &Path) -> Result<PathBuf, AppError> {
    let metadata = fs::symlink_metadata(root).map_err(map_not_found)?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(AppError::Internal("unsafe instance root".into()));
    }
    fs::canonicalize(root).map_err(Into::into)
}

fn scan_backup_files_blocking(
    root: &Path,
    declared_paths: &[PathBuf],
    max_total_bytes: u64,
) -> Result<Vec<BackupFile>, AppError> {
    checked_root(root)?;
    let mut pending = Vec::new();
    for declared in declared_paths {
        let relative = RelativePath::from_declared(declared)?;
        match resolve_existing(root, &relative) {
            Ok(path) => pending.push((path, relative.normalized)),
            Err(AppError::NotFound(_)) => continue,
            Err(error) => return Err(error),
        }
    }

    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    while let Some((path, archive_name)) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(map_not_found)?;
        if is_link_like(&metadata) {
            return Err(AppError::BadRequest("backups.link_found".into()));
        }
        if is_protected_path(&archive_name) {
            continue;
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| AppError::BadRequest("backups.non_utf8_name".into()))?;
                let child_name = format!("{archive_name}/{name}");
                pending.push((entry.path(), child_name));
            }
        } else if metadata.is_file() {
            reject_hardlinked_file(&path, &metadata)?;
            if !seen.insert(archive_name.clone()) {
                continue;
            }
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| AppError::BadRequest("backups.quota_exceeded".into()))?;
            if total > max_total_bytes || files.len() >= MAX_BACKUP_ENTRIES {
                return Err(AppError::BadRequest("backups.quota_exceeded".into()));
            }
            files.push(BackupFile {
                source: path,
                archive_name,
                size_bytes: metadata.len(),
            });
        } else {
            return Err(AppError::BadRequest("backups.special_file_found".into()));
        }
    }
    files.sort_by(|left, right| left.archive_name.cmp(&right.archive_name));
    Ok(files)
}

fn is_protected_path(path: &str) -> bool {
    let normalized = path.trim_matches('/').to_ascii_lowercase();
    let components = normalized.split('/').collect::<Vec<_>>();
    let Some(first) = components.first().copied() else {
        return false;
    };
    if matches!(first, ".staging" | ".backups" | ".restore" | "logs")
        || first.starts_with(".restore-")
        || first.starts_with(".deleting-")
        || first.starts_with(".dmx-")
    {
        return true;
    }
    components.iter().any(|component| {
        matches!(
            *component,
            ".env"
                | "master.key"
                | "palworldsettings.ini"
                | "credentials.json"
                | "credential.json"
                | "tokens.json"
                | "token.json"
                | "hytale-auth.json"
                | "hytale_auth.json"
        ) || component.ends_with(".pem")
            || component.ends_with(".key")
            || component.starts_with(".dmx-")
    })
}

fn map_not_found(error: io::Error) -> AppError {
    if error.kind() == io::ErrorKind::NotFound {
        AppError::NotFound("files.not_found".into())
    } else {
        error.into()
    }
}

fn open_read_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
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
    options.open(path)
}

fn reject_hardlinked_file(path: &Path, metadata: &fs::Metadata) -> Result<(), AppError> {
    if file_has_multiple_links(path, metadata)? {
        Err(AppError::Forbidden("files.hardlinks_forbidden".into()))
    } else {
        Ok(())
    }
}

pub(crate) fn file_has_multiple_links(path: &Path, metadata: &fs::Metadata) -> io::Result<bool> {
    if !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Ok(metadata.nlink() > 1)
    }
    #[cfg(windows)]
    {
        let file = open_read_no_follow(path)?;
        windows_file_is_linked(&file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "hard-link detection is unavailable on this platform",
        ))
    }
}

#[cfg(windows)]
fn windows_file_is_linked(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
        },
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle remains owned by `file` for the duration of the call and
    // `information` points to initialized writable storage.
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut information) };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.nNumberOfLinks > 1
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(windows)]
fn validate_open_file(file: &File) -> Result<(), AppError> {
    if windows_file_is_linked(file)? {
        Err(AppError::Forbidden("files.links_forbidden".into()))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn validate_open_file(_file: &File) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn system_time(value: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(value).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_absolute_windows_and_secret_paths() {
        for path in [
            "../outside",
            "/etc/passwd",
            "C:/Windows/System32",
            "game\\world",
            "logs/server.log",
            ".restore-job/staging/game/world/level.dat",
            "game/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini",
            "game/private.pem",
        ] {
            assert!(RelativePath::parse(path, false).is_err(), "accepted {path}");
        }
        assert!(RelativePath::parse("game/world/level.dat", false).is_ok());
        assert!(
            validate_restore_entry(Path::new("game/server.jar"), &[PathBuf::from("game/world")])
                .is_err()
        );
    }

    #[tokio::test]
    async fn writes_and_reads_a_bounded_regular_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("game")).unwrap();
        let contents = Bytes::from_static(b"hello");
        write_bytes(directory.path(), "game/message.txt", contents, 32)
            .await
            .unwrap();
        let (mut file, size) = open_regular_file(directory.path(), "game/message.txt")
            .await
            .unwrap();
        let mut output = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut file, &mut output)
            .await
            .unwrap();
        assert_eq!(size, 5);
        assert_eq!(output, "hello");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinks_and_hardlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("game")).unwrap();
        fs::write(outside.path().join("secret"), b"no").unwrap();
        symlink(outside.path(), directory.path().join("game/link")).unwrap();
        assert!(
            open_regular_file(directory.path(), "game/link/secret")
                .await
                .is_err()
        );

        fs::write(directory.path().join("game/world.dat"), b"world").unwrap();
        fs::hard_link(
            directory.path().join("game/world.dat"),
            directory.path().join("game/alias.dat"),
        )
        .unwrap();
        assert!(
            open_regular_file(directory.path(), "game/world.dat")
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn backup_scan_rejects_links_and_excludes_secret_files() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("game/Pal/Saved/Config/LinuxServer")).unwrap();
        fs::write(directory.path().join("game/Pal/Saved/world.sav"), b"world").unwrap();
        fs::write(
            directory
                .path()
                .join("game/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini"),
            b"password",
        )
        .unwrap();
        let files = scan_backup_files(directory.path(), &[PathBuf::from("game/Pal/Saved")], 1024)
            .await
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].archive_name, "game/Pal/Saved/world.sav");

        symlink(
            directory.path().join("game/Pal/Saved/world.sav"),
            directory.path().join("game/Pal/Saved/link"),
        )
        .unwrap();
        assert!(
            scan_backup_files(directory.path(), &[PathBuf::from("game/Pal/Saved")], 1024,)
                .await
                .is_err()
        );
    }
}
