use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use sqlx::FromRow;
use uuid::Uuid;

use crate::core::{DbPool, Settings, error::AppError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageMode {
    Managed,
    Attached,
}

#[derive(Debug, Clone)]
pub struct InstanceStorage {
    pub mode: StorageMode,
    pub root: PathBuf,
}

#[derive(Debug, FromRow)]
struct StorageRow {
    storage_mode: String,
    data_path: Option<String>,
    managed: bool,
}

pub fn managed_root(settings: &Settings, instance_id: &str) -> Result<PathBuf, AppError> {
    let id = Uuid::parse_str(instance_id)
        .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
    Ok(settings.instances_dir().join(id.to_string()))
}

/// Import jobs are intentionally not resumed after a panel crash because their
/// source may have changed. Migration marks them interrupted; startup then
/// removes their private staging area without following a substituted root.
pub async fn cleanup_interrupted_imports(settings: &Settings) -> Result<(), AppError> {
    let staging = settings.data_dir.join("import-staging");
    let metadata = match tokio::fs::symlink_metadata(&staging).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(AppError::Internal(
            "unsafe import staging root; refusing startup cleanup".into(),
        ));
    }
    tokio::fs::remove_dir_all(staging).await?;
    Ok(())
}

pub async fn resolve(
    pool: &DbPool,
    settings: &Settings,
    instance_id: &str,
) -> Result<InstanceStorage, AppError> {
    managed_root(settings, instance_id)?;
    let row: StorageRow =
        sqlx::query_as("SELECT storage_mode, data_path, managed FROM instances WHERE id = ?")
            .bind(instance_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;

    match (row.storage_mode.as_str(), row.managed, row.data_path) {
        ("managed", true, None) => Ok(InstanceStorage {
            mode: StorageMode::Managed,
            root: managed_root(settings, instance_id)?,
        }),
        ("attached", false, Some(path)) => {
            let root = validate_import_source(settings, Path::new(&path)).await?;
            if root.as_path() != Path::new(&path) {
                return Err(AppError::Conflict("imports.attached_path_changed".into()));
            }
            Ok(InstanceStorage {
                mode: StorageMode::Attached,
                root,
            })
        }
        _ => Err(AppError::Internal(
            "invalid instance storage configuration".into(),
        )),
    }
}

/// Canonicalizes a user-selected import directory, verifies that it remains below
/// one of the configured roots, and rejects every link/reparse-point component.
pub async fn validate_import_source(
    settings: &Settings,
    requested: &Path,
) -> Result<PathBuf, AppError> {
    let requested = requested.to_path_buf();
    let roots = settings.import_roots.clone();
    tokio::task::spawn_blocking(move || validate_import_source_blocking(&roots, &requested))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

fn validate_import_source_blocking(
    configured_roots: &[PathBuf],
    requested: &Path,
) -> Result<PathBuf, AppError> {
    if configured_roots.is_empty() {
        return Err(AppError::Forbidden("imports.roots_not_configured".into()));
    }
    if !requested.is_absolute()
        || requested.as_os_str().is_empty()
        || requested
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(AppError::BadRequest("imports.invalid_source_path".into()));
    }
    let requested_metadata = fs::symlink_metadata(requested)
        .map_err(|_| AppError::BadRequest("imports.source_not_found".into()))?;
    if !requested_metadata.is_dir() || is_link_like(&requested_metadata) {
        return Err(AppError::BadRequest("imports.source_not_directory".into()));
    }
    let canonical = fs::canonicalize(requested)
        .map_err(|_| AppError::BadRequest("imports.source_not_found".into()))?;

    for configured_root in configured_roots {
        if !configured_root.is_absolute() {
            continue;
        }
        let root_metadata = match fs::symlink_metadata(configured_root) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !root_metadata.is_dir() || is_link_like(&root_metadata) {
            continue;
        }
        let root = match fs::canonicalize(configured_root) {
            Ok(root) => root,
            Err(_) => continue,
        };
        let (walk_root, relative) = if let Ok(relative) = requested.strip_prefix(configured_root) {
            (configured_root.as_path(), relative)
        } else if let Ok(relative) = requested.strip_prefix(&root) {
            (root.as_path(), relative)
        } else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            return Err(AppError::BadRequest(
                "imports.source_must_be_below_root".into(),
            ));
        }
        let mut current = walk_root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(AppError::BadRequest("imports.invalid_source_path".into()));
            };
            current.push(component);
            let metadata = fs::symlink_metadata(&current)
                .map_err(|_| AppError::BadRequest("imports.source_not_found".into()))?;
            if is_link_like(&metadata) {
                return Err(AppError::BadRequest("imports.links_forbidden".into()));
            }
        }
        if !canonical.starts_with(&root) || canonical == root {
            return Err(AppError::Forbidden("imports.source_outside_roots".into()));
        }
        return Ok(canonical);
    }

    Err(AppError::Forbidden("imports.source_outside_roots".into()))
}

pub async fn validate_instance_tree(root: &Path, require_game: bool) -> Result<(), AppError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        validate_tree_blocking(&root, require_game, 100_000, 16 * 1024 * 1024 * 1024)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

fn validate_tree_blocking(
    root: &Path,
    require_game: bool,
    max_entries: usize,
    max_bytes: u64,
) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|_| AppError::BadRequest("imports.source_not_found".into()))?;
    if !metadata.is_dir() || is_link_like(&metadata) {
        return Err(AppError::BadRequest("imports.source_not_directory".into()));
    }
    if require_game {
        let game = root.join("game");
        let metadata = fs::symlink_metadata(&game)
            .map_err(|_| AppError::BadRequest("imports.game_directory_required".into()))?;
        if !metadata.is_dir() || is_link_like(&metadata) {
            return Err(AppError::BadRequest(
                "imports.game_directory_required".into(),
            ));
        }
    }

    let mut pending = vec![(root.to_path_buf(), PathBuf::new())];
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    while let Some((directory, relative_directory)) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            entries = entries
                .checked_add(1)
                .ok_or_else(|| AppError::BadRequest("imports.quota_exceeded".into()))?;
            if entries > max_entries {
                return Err(AppError::BadRequest("imports.too_many_entries".into()));
            }
            let path = entry.path();
            let relative = relative_directory.join(entry.file_name());
            if invalid_import_path(&relative) {
                return Err(AppError::BadRequest("imports.invalid_entry_path".into()));
            }
            if forbidden_import_path(&relative) {
                return Err(AppError::BadRequest("imports.protected_file_found".into()));
            }
            let metadata = fs::symlink_metadata(&path)?;
            if is_link_like(&metadata) {
                return Err(AppError::BadRequest("imports.links_forbidden".into()));
            }
            if metadata.is_dir() {
                pending.push((path, relative));
            } else if metadata.is_file() {
                reject_hardlink(&path, &metadata)?;
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| AppError::BadRequest("imports.quota_exceeded".into()))?;
                if bytes > max_bytes || metadata.len() > 8 * 1024 * 1024 * 1024 {
                    return Err(AppError::BadRequest("imports.quota_exceeded".into()));
                }
            } else {
                return Err(AppError::BadRequest(
                    "imports.special_files_forbidden".into(),
                ));
            }
        }
    }
    Ok(())
}

fn invalid_import_path(relative: &Path) -> bool {
    let Some(value) = relative.to_str() else {
        return true;
    };
    if value.len() > 4_096 {
        return true;
    }
    relative.components().any(|component| {
        let Component::Normal(component) = component else {
            return true;
        };
        let Some(value) = component.to_str() else {
            return true;
        };
        value.is_empty()
            || value == "."
            || value == ".."
            || value.len() > 255
            || value
                .chars()
                .any(|character| matches!(character, ':' | '\\' | '\0') || character.is_control())
    })
}

fn forbidden_import_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        let Component::Normal(component) = component else {
            return true;
        };
        let value = component.to_string_lossy().to_ascii_lowercase();
        value.starts_with(".dmx-")
            || matches!(
                value.as_str(),
                ".env"
                    | "master.key"
                    | "credentials.json"
                    | "credential.json"
                    | "tokens.json"
                    | "token.json"
                    | "hytale-auth.json"
                    | ".staging"
                    | ".restore"
            )
    })
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

fn reject_hardlink(path: &Path, metadata: &fs::Metadata) -> Result<(), AppError> {
    if crate::services::secure_fs::file_has_multiple_links(path, metadata)? {
        Err(AppError::BadRequest("imports.hardlinks_forbidden".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path, import_root: PathBuf) -> Settings {
        Settings {
            config_file: root.join("config.toml"),
            data_dir: root.join("data"),
            static_dir: root.join("static"),
            bind: "127.0.0.1:5500".parse().unwrap(),
            database_url: "sqlite::memory:".into(),
            master_key_file: root.join("master.key"),
            steamcmd_path: PathBuf::from("steamcmd"),
            bedrock_linux_source: None,
            bedrock_windows_source: None,
            import_roots: vec![import_root],
            trusted_proxies: Vec::new(),
            reverse_proxy: false,
            log: "error".into(),
            dev_origin: None,
            setup_token: None,
            session_ttl_hours: 24,
            deployment_mode: crate::core::config::DeploymentMode::Native,
            release_check: None,
        }
    }

    #[tokio::test]
    async fn import_sources_must_be_strict_descendants() {
        let temporary = tempfile::tempdir().unwrap();
        let allowed = temporary.path().join("allowed");
        let source = allowed.join("server");
        fs::create_dir_all(&source).unwrap();
        let settings = settings(temporary.path(), allowed.clone());
        assert_eq!(
            validate_import_source(&settings, &source).await.unwrap(),
            fs::canonicalize(source).unwrap()
        );
        assert!(validate_import_source(&settings, &allowed).await.is_err());
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        assert!(validate_import_source(&settings, &outside).await.is_err());
    }

    #[test]
    fn imported_names_are_cross_platform_safe() {
        assert!(!invalid_import_path(Path::new("game/world/level.dat")));
        assert!(invalid_import_path(Path::new("game/server.jar:stream")));
        assert!(invalid_import_path(Path::new("game/bad\\name")));
        assert!(invalid_import_path(Path::new("game/bad\nname")));
    }

    #[tokio::test]
    async fn interrupted_staging_is_cleaned_without_following_a_root_link() {
        let temporary = tempfile::tempdir().unwrap();
        let allowed = temporary.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        let settings = settings(temporary.path(), allowed);
        let staging = settings.data_dir.join("import-staging");
        fs::create_dir_all(staging.join("job/candidate/game")).unwrap();
        fs::write(staging.join("job/candidate/game/world"), b"partial").unwrap();
        cleanup_interrupted_imports(&settings).await.unwrap();
        assert!(!staging.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = temporary.path().join("outside");
            fs::create_dir_all(&outside).unwrap();
            fs::create_dir_all(&settings.data_dir).unwrap();
            symlink(&outside, &staging).unwrap();
            assert!(cleanup_interrupted_imports(&settings).await.is_err());
            assert!(outside.exists());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn import_sources_and_trees_reject_links_and_hardlinks() {
        use std::{fs::hard_link, os::unix::fs::symlink};

        let temporary = tempfile::tempdir().unwrap();
        let allowed = temporary.path().join("allowed");
        let source = allowed.join("server");
        fs::create_dir_all(source.join("game")).unwrap();
        let settings = settings(temporary.path(), allowed.clone());
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, allowed.join("linked")).unwrap();
        assert!(
            validate_import_source(&settings, &allowed.join("linked"))
                .await
                .is_err()
        );

        let inside = allowed.join("inside");
        fs::create_dir(&inside).unwrap();
        symlink(&inside, allowed.join("inside-link")).unwrap();
        assert!(
            validate_import_source(&settings, &allowed.join("inside-link"))
                .await
                .is_err()
        );

        let original = source.join("game/world.dat");
        fs::write(&original, b"world").unwrap();
        hard_link(&original, source.join("game/world-copy.dat")).unwrap();
        assert!(validate_instance_tree(&source, true).await.is_err());
    }
}
