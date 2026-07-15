//! Validated local `.dmxpack` catalogue storage.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::SeekFrom,
    path::{Component, Path, PathBuf},
};

use axum::body::Bytes;
use futures::Stream;
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    core::{DbPool, Settings, error::AppError},
    domain::v1::{GameProfile, SteamProfile},
    services::{
        installers::{ArchiveLimits, extract_zip},
        profiles::{ProfileRegistry, build_local_steam_profile},
        secure_fs,
    },
};

pub const FORMAT_NAME: &str = "dmxpack";
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CONTENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PNG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FILES: usize = 63;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogManifest {
    pub format: String,
    pub schema_version: u32,
    pub id: String,
    pub revision: u32,
    pub name: String,
    pub description: String,
    pub content: CatalogContent,
    pub files: Vec<CatalogFileDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogContent {
    SteamProfile {
        definition: String,
        settings_schema: String,
        ui_schema: String,
        #[serde(default)]
        icon: Option<String>,
    },
    Theme {
        tokens: String,
        #[serde(default)]
        logo: Option<String>,
        #[serde(default)]
        preview: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogFileDeclaration {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeTokens {
    pub accent: String,
    pub bg_primary: String,
    pub bg_secondary: String,
    pub bg_tertiary: String,
    pub bg_elevated: String,
    pub border: String,
    pub border_hover: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub info: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CatalogFile {
    pub role: String,
    pub path: String,
    pub media_type: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CatalogPackage {
    pub id: String,
    pub revision: u32,
    pub kind: String,
    pub schema_version: u32,
    pub name: String,
    pub description: String,
    pub archive_sha256: String,
    pub archive_size_bytes: u64,
    pub content_size_bytes: u64,
    pub manifest: CatalogManifest,
    pub files: Vec<CatalogFile>,
    pub theme_tokens: Option<ThemeTokens>,
    pub compatibility_status: String,
    pub created_at: String,
}

pub struct OpenCatalogAsset {
    pub file: tokio::fs::File,
    pub size_bytes: u64,
    pub media_type: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThemeSelection {
    Default,
    Catalog { package_id: String, revision: u32 },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThemeAsset {
    pub url: String,
    pub sha256: String,
    pub media_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThemeAssets {
    pub logo: Option<ThemeAsset>,
    pub preview: Option<ThemeAsset>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveTheme {
    pub selection: ThemeSelection,
    pub tokens: ThemeTokens,
    pub assets: ThemeAssets,
    pub version: u32,
    pub updated_at: String,
}

#[derive(Debug, FromRow)]
struct PackageRow {
    id: String,
    revision: i64,
    kind: String,
    schema_version: i64,
    name: String,
    description: String,
    archive_sha256: String,
    archive_size_bytes: i64,
    content_size_bytes: i64,
    manifest: String,
    created_at: String,
}

#[derive(Debug, FromRow)]
struct FileRow {
    package_id: String,
    package_revision: i64,
    role: String,
    relative_path: String,
    media_type: String,
    checksum_sha256: String,
    size_bytes: i64,
}

#[derive(Debug)]
struct ValidatedPackage {
    manifest: CatalogManifest,
    kind: &'static str,
    files: Vec<CatalogFile>,
    content_size_bytes: u64,
    profile: Option<GameProfile>,
    theme_tokens: Option<ThemeTokens>,
}

#[derive(Debug)]
struct UniqueJson(Value);

impl CatalogManifest {
    fn kind(&self) -> &'static str {
        match self.content {
            CatalogContent::SteamProfile { .. } => "steam_profile",
            CatalogContent::Theme { .. } => "theme",
        }
    }
}

pub async fn prepare(settings: &Settings) -> Result<(), AppError> {
    let root = settings.catalog_dir();
    ensure_private_directory(&root).await?;
    ensure_private_directory(&root.join("staging")).await?;
    ensure_private_directory(&root.join("revisions")).await?;
    ensure_private_directory(&root.join("trash")).await?;
    Ok(())
}

pub async fn cleanup_interrupted(pool: &DbPool, settings: &Settings) -> Result<(), AppError> {
    prepare(settings).await?;
    for directory in ["staging", "trash"] {
        let root = settings.catalog_dir().join(directory);
        let mut entries = tokio::fs::read_dir(&root).await?;
        while let Some(entry) = entries.next_entry().await? {
            remove_tree(&entry.path()).await?;
        }
    }
    let mut active = sqlx::query_scalar::<_, String>(
        "SELECT archive_sha256 FROM catalog_packages ORDER BY archive_sha256",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();
    let revisions = settings.catalog_dir().join("revisions");
    let mut entries = tokio::fs::read_dir(&revisions).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        if active.remove(name) {
            validate_revision_storage(&entry.path(), name).await?;
        } else {
            remove_tree(&entry.path()).await?;
        }
    }
    if !active.is_empty() {
        return Err(AppError::Internal(
            "one or more active catalog revisions are missing from storage".into(),
        ));
    }
    Ok(())
}

pub async fn stage_upload<S, E>(
    settings: &Settings,
    job_id: &str,
    expected_sha256: &str,
    stream: S,
) -> Result<u64, AppError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: fmt::Display,
{
    validate_uuid(job_id, "catalog.invalid_job_id")?;
    validate_sha256(expected_sha256, "catalog.invalid_archive_checksum")?;
    prepare(settings).await?;
    let staging = staging_root(settings, job_id);
    match tokio::fs::create_dir(&staging).await {
        Ok(()) => set_private_permissions(&staging).await?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(AppError::Conflict("catalog.staging_exists".into()));
        }
        Err(error) => return Err(error.into()),
    }
    let result = async {
        let written =
            secure_fs::write_stream(&staging, "package.dmxpack", stream, MAX_ARCHIVE_BYTES)
                .await
                .map_err(map_upload_error)?;
        if written == 0 {
            return Err(AppError::BadRequest("catalog.empty_archive".into()));
        }
        let digest = hash_path(&staging.join("package.dmxpack"), MAX_ARCHIVE_BYTES).await?;
        if digest.size_bytes != written || digest.sha256 != expected_sha256.to_ascii_lowercase() {
            return Err(AppError::BadRequest(
                "catalog.archive_checksum_mismatch".into(),
            ));
        }
        Ok(written)
    }
    .await;
    if result.is_err() {
        let _ = remove_tree(&staging).await;
    }
    result
}

pub async fn import_staged(
    pool: &DbPool,
    settings: &Settings,
    profiles: &ProfileRegistry,
    job_id: &str,
    actor_id: &str,
    archive_sha256: &str,
) -> Result<CatalogPackage, AppError> {
    validate_uuid(job_id, "catalog.invalid_job_id")?;
    validate_sha256(archive_sha256, "catalog.invalid_archive_checksum")?;
    let staging = staging_root(settings, job_id);
    let archive = staging.join("package.dmxpack");
    let archive_hash = hash_path(&archive, MAX_ARCHIVE_BYTES).await?;
    if archive_hash.sha256 != archive_sha256.to_ascii_lowercase() {
        return Err(AppError::BadRequest(
            "catalog.archive_checksum_mismatch".into(),
        ));
    }
    let content = staging.join("content");
    let extracted = extract_zip(
        &archive,
        &content,
        ArchiveLimits {
            max_entries: MAX_FILES + 1,
            max_file_bytes: MAX_JSON_BYTES,
            max_total_bytes: MAX_CONTENT_BYTES,
            max_compression_ratio: 50,
        },
        None,
    )
    .await
    .map_err(map_archive_error)?;
    let validated = validate_extracted(&content, &extracted).await?;
    persist_validated(
        pool,
        settings,
        profiles,
        actor_id,
        archive_hash,
        staging,
        validated,
    )
    .await
}

pub async fn discard_staging(settings: &Settings, job_id: &str) {
    if uuid::Uuid::parse_str(job_id).is_ok() {
        let _ = remove_tree(&staging_root(settings, job_id)).await;
    }
}

pub async fn list(pool: &DbPool, kind: Option<&str>) -> Result<Vec<CatalogPackage>, AppError> {
    if kind.is_some_and(|value| !matches!(value, "steam_profile" | "theme")) {
        return Err(AppError::BadRequest("catalog.invalid_kind".into()));
    }
    let rows: Vec<PackageRow> = if let Some(kind) = kind {
        sqlx::query_as(
            "SELECT p.* FROM catalog_packages p WHERE p.kind = ? AND p.revision = \
             (SELECT MAX(latest.revision) FROM catalog_packages latest WHERE latest.id = p.id) \
             ORDER BY p.name COLLATE NOCASE",
        )
        .bind(kind)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT p.* FROM catalog_packages p WHERE p.revision = \
             (SELECT MAX(latest.revision) FROM catalog_packages latest WHERE latest.id = p.id) \
             ORDER BY p.kind, p.name COLLATE NOCASE",
        )
        .fetch_all(pool)
        .await?
    };
    packages_from_rows(pool, rows).await
}

pub async fn revisions(
    pool: &DbPool,
    kind: &str,
    id: &str,
) -> Result<Vec<CatalogPackage>, AppError> {
    validate_identity(kind, id)?;
    let rows: Vec<PackageRow> = sqlx::query_as(
        "SELECT * FROM catalog_packages WHERE kind = ? AND id = ? ORDER BY revision DESC",
    )
    .bind(kind)
    .bind(id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Err(AppError::NotFound("catalog.not_found".into()));
    }
    packages_from_rows(pool, rows).await
}

pub async fn get(
    pool: &DbPool,
    kind: &str,
    id: &str,
    revision: u32,
) -> Result<CatalogPackage, AppError> {
    validate_identity(kind, id)?;
    let row: PackageRow =
        sqlx::query_as("SELECT * FROM catalog_packages WHERE kind = ? AND id = ? AND revision = ?")
            .bind(kind)
            .bind(id)
            .bind(revision)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound("catalog.not_found".into()))?;
    let mut packages = packages_from_rows(pool, vec![row]).await?;
    packages
        .pop()
        .ok_or_else(|| AppError::Internal("catalog package conversion failed".into()))
}

pub async fn open_asset(
    pool: &DbPool,
    settings: &Settings,
    kind: &str,
    id: &str,
    revision: u32,
    asset: &str,
) -> Result<OpenCatalogAsset, AppError> {
    validate_identity(kind, id)?;
    if !matches!(asset, "icon" | "logo" | "preview") {
        return Err(AppError::NotFound("catalog.asset_not_found".into()));
    }
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT p.archive_sha256, f.relative_path, f.checksum_sha256, f.size_bytes \
         FROM catalog_packages p JOIN catalog_files f \
           ON f.package_id = p.id AND f.package_revision = p.revision \
         WHERE p.kind = ? AND p.id = ? AND p.revision = ? AND f.role = ?",
    )
    .bind(kind)
    .bind(id)
    .bind(revision)
    .bind(asset)
    .fetch_optional(pool)
    .await?;
    let (archive_sha256, relative_path, expected_sha256, expected_size) =
        row.ok_or_else(|| AppError::NotFound("catalog.asset_not_found".into()))?;
    validate_sha256(&archive_sha256, "catalog.invalid_stored_checksum")?;
    let content = revision_root(settings, &archive_sha256).join("content");
    let (mut file, size_bytes) = secure_fs::open_regular_file(&content, &relative_path).await?;
    let expected_size = u64::try_from(expected_size)
        .map_err(|_| AppError::Internal("invalid stored catalog asset size".into()))?;
    if size_bytes != expected_size {
        return Err(AppError::Internal("catalog asset size mismatch".into()));
    }
    let digest = hash_open_file(&mut file, MAX_PNG_BYTES).await?;
    if digest.size_bytes != expected_size || digest.sha256 != expected_sha256 {
        return Err(AppError::Internal("catalog asset checksum mismatch".into()));
    }
    file.seek(SeekFrom::Start(0)).await?;
    Ok(OpenCatalogAsset {
        file,
        size_bytes,
        media_type: "image/png".into(),
        sha256: expected_sha256,
    })
}

pub async fn active_theme(pool: &DbPool) -> Result<ActiveTheme, AppError> {
    let row: (Option<String>, Option<i64>, i64, String) = sqlx::query_as(
        "SELECT package_id, package_revision, version, updated_at \
         FROM catalog_theme_selection WHERE singleton = 1",
    )
    .fetch_one(pool)
    .await?;
    theme_from_selection_row(pool, row).await
}

pub async fn select_theme(
    pool: &DbPool,
    actor_id: &str,
    selection: &ThemeSelection,
    expected_version: u32,
) -> Result<ActiveTheme, AppError> {
    if expected_version == 0 {
        return Err(AppError::BadRequest("catalog.theme.invalid_version".into()));
    }
    let mut transaction = pool.begin().await?;
    let (package_id, package_revision) = match selection {
        ThemeSelection::Default => (None, None),
        ThemeSelection::Catalog {
            package_id,
            revision,
        } => {
            validate_identity("theme", package_id)?;
            if *revision == 0 {
                return Err(AppError::BadRequest(
                    "catalog.theme.invalid_revision".into(),
                ));
            }
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM catalog_theme_revisions \
                 WHERE package_id = ? AND package_revision = ?)",
            )
            .bind(package_id)
            .bind(revision)
            .fetch_one(&mut *transaction)
            .await?;
            if !exists {
                return Err(AppError::NotFound(
                    "catalog.theme.revision_not_found".into(),
                ));
            }
            (Some(package_id.as_str()), Some(i64::from(*revision)))
        }
    };
    let now = chrono::Utc::now().to_rfc3339();
    let updated = sqlx::query(
        "UPDATE catalog_theme_selection SET package_id = ?, package_revision = ?, \
         version = version + 1, updated_by = ?, updated_at = ? \
         WHERE singleton = 1 AND version = ?",
    )
    .bind(package_id)
    .bind(package_revision)
    .bind(actor_id)
    .bind(&now)
    .bind(expected_version)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Conflict("catalog.theme.version_conflict".into()));
    }
    let version = expected_version
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("catalog.theme.version_exhausted".into()))?;
    sqlx::query(
        "INSERT INTO audit_events \
         (actor_user_id, action, resource_type, resource_id, outcome, metadata, created_at) \
         VALUES (?, 'catalog.theme_selected', 'catalog_theme', 'global', 'success', ?, ?)",
    )
    .bind(actor_id)
    .bind(serde_json::json!({"selection": selection, "version": version}).to_string())
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    active_theme(pool).await
}

pub async fn remove(
    pool: &DbPool,
    settings: &Settings,
    profiles: &ProfileRegistry,
    kind: &str,
    id: &str,
    revision: u32,
) -> Result<CatalogPackage, AppError> {
    let package = get(pool, kind, id, revision).await?;
    let linked_profile: Option<(String, i64)> = sqlx::query_as(
        "SELECT profile_id, profile_revision FROM catalog_profile_revisions \
         WHERE package_id = ? AND package_revision = ?",
    )
    .bind(id)
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    if let Some((profile_id, profile_revision)) = &linked_profile {
        let pinned: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instances WHERE profile_id = ? AND profile_revision = ?",
        )
        .bind(profile_id)
        .bind(profile_revision)
        .fetch_one(pool)
        .await?;
        if pinned != 0 {
            return Err(AppError::Conflict("catalog.revision_in_use".into()));
        }
    }

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO catalog_revision_tombstones \
         (id, revision, kind, archive_sha256, deleted_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(revision)
    .bind(kind)
    .bind(&package.archive_sha256)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;
    if let Some((profile_id, profile_revision)) = &linked_profile {
        sqlx::query(
            "DELETE FROM catalog_profile_revisions \
             WHERE package_id = ? AND package_revision = ?",
        )
        .bind(id)
        .bind(revision)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM game_profiles WHERE id = ? AND revision = ?")
            .bind(profile_id)
            .bind(profile_revision)
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
    }
    sqlx::query("DELETE FROM catalog_packages WHERE id = ? AND revision = ?")
        .bind(id)
        .bind(revision)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await.map_err(map_database_error)?;

    if let Some((profile_id, profile_revision)) = linked_profile
        && let Ok(profile_revision) = u32::try_from(profile_revision)
    {
        profiles.unregister_custom_revision(&profile_id, profile_revision);
    }
    let source = revision_root(settings, &package.archive_sha256);
    let trash = settings
        .catalog_dir()
        .join("trash")
        .join(uuid::Uuid::new_v4().to_string());
    match tokio::fs::rename(&source, &trash).await {
        Ok(()) => {
            if let Err(error) = remove_tree(&trash).await {
                tracing::warn!(%error, "failed to remove catalog trash after commit");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(archive_sha256 = %package.archive_sha256, "deleted catalog storage was already absent");
        }
        Err(error) => {
            // SQLite is authoritative after commit. The unreferenced revision is
            // deliberately left in place and removed by startup recovery.
            tracing::warn!(%error, "failed to quarantine deleted catalog storage");
        }
    }
    Ok(package)
}

pub async fn profile_is_catalog_managed(pool: &DbPool, id: &str) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog_packages \
                       WHERE id = ? AND kind = 'steam_profile') \
             OR EXISTS(SELECT 1 FROM catalog_revision_tombstones \
                       WHERE id = ? AND kind = 'steam_profile')",
    )
    .bind(id)
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn validate_extracted(
    content: &Path,
    extracted: &[PathBuf],
) -> Result<ValidatedPackage, AppError> {
    let actual = extracted
        .iter()
        .map(|path| relative_utf8(content, path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !actual.contains("manifest.json") {
        return Err(AppError::BadRequest("catalog.manifest_missing".into()));
    }
    let manifest_value =
        read_unique_json(&content.join("manifest.json"), MAX_MANIFEST_BYTES).await?;
    let manifest: CatalogManifest = serde_json::from_value(manifest_value)
        .map_err(|_| AppError::BadRequest("catalog.manifest_invalid".into()))?;
    validate_manifest_metadata(&manifest)?;

    let declared = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut expected = declared.clone();
    expected.insert("manifest.json".into());
    if expected != actual || declared.len() != manifest.files.len() {
        return Err(AppError::BadRequest("catalog.file_set_mismatch".into()));
    }

    let roles = roles_for_content(&manifest.content)?;
    if roles.len() != manifest.files.len()
        || roles.keys().collect::<BTreeSet<_>>() != declared.iter().collect::<BTreeSet<_>>()
    {
        return Err(AppError::BadRequest("catalog.file_roles_invalid".into()));
    }

    let mut files = Vec::with_capacity(manifest.files.len());
    let mut content_size_bytes = 0_u64;
    for declaration in &manifest.files {
        let role = roles
            .get(&declaration.path)
            .ok_or_else(|| AppError::BadRequest("catalog.file_roles_invalid".into()))?;
        validate_file_declaration(declaration, role)?;
        let digest = hash_path(&content.join(&declaration.path), MAX_JSON_BYTES).await?;
        if digest.sha256 != declaration.sha256 || digest.size_bytes != declaration.size_bytes {
            return Err(AppError::BadRequest(
                "catalog.file_checksum_mismatch".into(),
            ));
        }
        if declaration.media_type == "image/png" {
            if declaration.size_bytes > MAX_PNG_BYTES {
                return Err(AppError::BadRequest("catalog.asset_too_large".into()));
            }
            validate_png(&content.join(&declaration.path)).await?;
        }
        content_size_bytes = content_size_bytes
            .checked_add(declaration.size_bytes)
            .ok_or_else(|| AppError::BadRequest("catalog.content_too_large".into()))?;
        files.push(CatalogFile {
            role: (*role).into(),
            path: declaration.path.clone(),
            media_type: declaration.media_type.clone(),
            sha256: declaration.sha256.clone(),
            size_bytes: declaration.size_bytes,
        });
    }
    if content_size_bytes > MAX_CONTENT_BYTES {
        return Err(AppError::BadRequest("catalog.content_too_large".into()));
    }

    let (profile, theme_tokens) = match &manifest.content {
        CatalogContent::SteamProfile {
            definition,
            settings_schema,
            ui_schema,
            ..
        } => {
            let steam: SteamProfile = serde_json::from_value(
                read_unique_json(&content.join(definition), MAX_JSON_BYTES).await?,
            )
            .map_err(|_| AppError::BadRequest("catalog.profile_invalid".into()))?;
            let profile = build_local_steam_profile(
                manifest.id.clone(),
                manifest.revision,
                manifest.name.clone(),
                manifest.description.clone(),
                steam,
            )?;
            let packaged_settings =
                read_unique_json(&content.join(settings_schema), MAX_JSON_BYTES).await?;
            let packaged_ui = read_unique_json(&content.join(ui_schema), MAX_JSON_BYTES).await?;
            if packaged_settings != profile.settings_schema || packaged_ui != profile.ui_schema {
                return Err(AppError::BadRequest(
                    "catalog.profile_schema_mismatch".into(),
                ));
            }
            (Some(profile), None)
        }
        CatalogContent::Theme { tokens, .. } => {
            let tokens: ThemeTokens = serde_json::from_value(
                read_unique_json(&content.join(tokens), MAX_JSON_BYTES).await?,
            )
            .map_err(|_| AppError::BadRequest("catalog.theme_tokens_invalid".into()))?;
            validate_theme_tokens(&tokens)?;
            (None, Some(tokens))
        }
    };

    Ok(ValidatedPackage {
        kind: manifest.kind(),
        manifest,
        files,
        content_size_bytes,
        profile,
        theme_tokens,
    })
}

async fn persist_validated(
    pool: &DbPool,
    settings: &Settings,
    profiles: &ProfileRegistry,
    actor_id: &str,
    archive: FileDigest,
    staging: PathBuf,
    validated: ValidatedPackage,
) -> Result<CatalogPackage, AppError> {
    let mut transaction = pool.begin().await?;
    let previous: Option<(i64, String)> = sqlx::query_as(
        "SELECT revision, kind FROM (\
           SELECT revision, kind FROM catalog_packages WHERE id = ? \
           UNION ALL \
           SELECT revision, kind FROM catalog_revision_tombstones WHERE id = ?\
         ) ORDER BY revision DESC LIMIT 1",
    )
    .bind(&validated.manifest.id)
    .bind(&validated.manifest.id)
    .fetch_optional(&mut *transaction)
    .await?;
    let expected_revision = previous
        .as_ref()
        .map(|(revision, _)| revision + 1)
        .unwrap_or(1);
    if i64::from(validated.manifest.revision) != expected_revision {
        return Err(AppError::Conflict(
            "catalog.revision_must_follow_latest".into(),
        ));
    }
    if previous
        .as_ref()
        .is_some_and(|(_, kind)| kind != validated.kind)
    {
        return Err(AppError::Conflict("catalog.kind_is_immutable".into()));
    }
    if validated.profile.is_some() {
        let profile_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM game_profiles WHERE id = ?")
                .bind(&validated.manifest.id)
                .fetch_one(&mut *transaction)
                .await?;
        let catalog_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_profile_revisions WHERE profile_id = ?",
        )
        .bind(&validated.manifest.id)
        .fetch_one(&mut *transaction)
        .await?;
        if profile_count != catalog_count {
            return Err(AppError::Conflict("catalog.profile_origin_conflict".into()));
        }
        let previous_app_id: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(json_extract(g.manifest, '$.steam_profile.app_id') AS INTEGER) \
             FROM game_profiles g JOIN catalog_profile_revisions c \
               ON c.profile_id = g.id AND c.profile_revision = g.revision \
             WHERE g.id = ? ORDER BY g.revision DESC LIMIT 1",
        )
        .bind(&validated.manifest.id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let (Some(previous_app_id), Some(profile)) = (previous_app_id, &validated.profile)
            && profile
                .steam_profile
                .as_ref()
                .is_none_or(|steam| i64::from(steam.app_id) != previous_app_id)
        {
            return Err(AppError::Conflict(
                "catalog.profile_app_id_is_immutable".into(),
            ));
        }
    }

    let manifest_json = serde_json::to_string(&validated.manifest)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO catalog_packages \
         (id, revision, kind, schema_version, name, description, archive_sha256, \
          archive_size_bytes, content_size_bytes, manifest, created_by, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&validated.manifest.id)
    .bind(validated.manifest.revision)
    .bind(validated.kind)
    .bind(validated.manifest.schema_version)
    .bind(&validated.manifest.name)
    .bind(&validated.manifest.description)
    .bind(&archive.sha256)
    .bind(i64::try_from(archive.size_bytes).expect("archive quota fits i64"))
    .bind(i64::try_from(validated.content_size_bytes).expect("content quota fits i64"))
    .bind(manifest_json)
    .bind(actor_id)
    .bind(&created_at)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;
    for file in &validated.files {
        sqlx::query(
            "INSERT INTO catalog_files \
             (package_id, package_revision, role, relative_path, media_type, checksum_sha256, size_bytes) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&validated.manifest.id)
        .bind(validated.manifest.revision)
        .bind(&file.role)
        .bind(&file.path)
        .bind(&file.media_type)
        .bind(&file.sha256)
        .bind(i64::try_from(file.size_bytes).expect("file quota fits i64"))
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
    }
    if let Some(profile) = &validated.profile {
        sqlx::query(
            "INSERT INTO game_profiles (id, revision, kind, manifest, created_by, created_at) \
             VALUES (?, ?, 'steam_custom', ?, ?, ?)",
        )
        .bind(&profile.id)
        .bind(profile.revision)
        .bind(
            serde_json::to_string(profile)
                .map_err(|error| AppError::Internal(error.to_string()))?,
        )
        .bind(actor_id)
        .bind(&created_at)
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        sqlx::query(
            "INSERT INTO catalog_profile_revisions \
             (package_id, package_revision, profile_id, profile_revision) VALUES (?, ?, ?, ?)",
        )
        .bind(&validated.manifest.id)
        .bind(validated.manifest.revision)
        .bind(&profile.id)
        .bind(profile.revision)
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
    }
    if let Some(tokens) = &validated.theme_tokens {
        sqlx::query(
            "INSERT INTO catalog_theme_revisions (package_id, package_revision, tokens) \
             VALUES (?, ?, ?)",
        )
        .bind(&validated.manifest.id)
        .bind(validated.manifest.revision)
        .bind(serde_json::to_string(tokens).map_err(|error| AppError::Internal(error.to_string()))?)
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
    }

    let destination = revision_root(settings, &archive.sha256);
    if tokio::fs::try_exists(&destination).await? {
        return Err(AppError::Conflict(
            "catalog.archive_already_installed".into(),
        ));
    }
    tokio::fs::rename(&staging, &destination).await?;
    if let Err(error) = transaction.commit().await.map_err(map_database_error) {
        let _ = remove_tree(&destination).await;
        return Err(error);
    }
    if let Some(profile) = validated.profile {
        profiles.register(profile);
    }
    get(
        pool,
        validated.kind,
        &validated.manifest.id,
        validated.manifest.revision,
    )
    .await
}

async fn packages_from_rows(
    pool: &DbPool,
    rows: Vec<PackageRow>,
) -> Result<Vec<CatalogPackage>, AppError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let all_files: Vec<FileRow> = sqlx::query_as(
        "SELECT package_id, package_revision, role, relative_path, media_type, \
         checksum_sha256, size_bytes FROM catalog_files \
         ORDER BY package_id, package_revision, relative_path",
    )
    .fetch_all(pool)
    .await?;
    let all_tokens: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT package_id, package_revision, tokens FROM catalog_theme_revisions")
            .fetch_all(pool)
            .await?;
    let mut files = BTreeMap::<(String, i64), Vec<CatalogFile>>::new();
    for row in all_files {
        let key = (row.package_id.clone(), row.package_revision);
        files.entry(key).or_default().push(file_from_row(row)?);
    }
    let mut tokens = all_tokens
        .into_iter()
        .map(|(id, revision, value)| {
            let tokens = serde_json::from_str::<ThemeTokens>(&value)
                .map_err(|_| AppError::Internal("stored theme tokens are invalid".into()))?;
            Ok(((id, revision), tokens))
        })
        .collect::<Result<BTreeMap<_, _>, AppError>>()?;
    rows.into_iter()
        .map(|row| {
            let key = (row.id.clone(), row.revision);
            package_from_row(
                row,
                files.remove(&key).unwrap_or_default(),
                tokens.remove(&key),
            )
        })
        .collect()
}

fn package_from_row(
    row: PackageRow,
    files: Vec<CatalogFile>,
    theme_tokens: Option<ThemeTokens>,
) -> Result<CatalogPackage, AppError> {
    let manifest: CatalogManifest = serde_json::from_str(&row.manifest)
        .map_err(|_| AppError::Internal("stored catalog manifest is invalid".into()))?;
    Ok(CatalogPackage {
        id: row.id,
        revision: u32::try_from(row.revision)
            .map_err(|_| AppError::Internal("stored catalog revision is invalid".into()))?,
        kind: row.kind,
        schema_version: u32::try_from(row.schema_version)
            .map_err(|_| AppError::Internal("stored catalog schema version is invalid".into()))?,
        name: row.name,
        description: row.description,
        archive_sha256: row.archive_sha256,
        archive_size_bytes: u64::try_from(row.archive_size_bytes)
            .map_err(|_| AppError::Internal("stored catalog archive size is invalid".into()))?,
        content_size_bytes: u64::try_from(row.content_size_bytes)
            .map_err(|_| AppError::Internal("stored catalog content size is invalid".into()))?,
        manifest,
        files,
        theme_tokens,
        compatibility_status: "unverified".into(),
        created_at: row.created_at,
    })
}

async fn theme_from_selection_row(
    pool: &DbPool,
    row: (Option<String>, Option<i64>, i64, String),
) -> Result<ActiveTheme, AppError> {
    let (package_id, package_revision, version, updated_at) = row;
    let version = u32::try_from(version)
        .map_err(|_| AppError::Internal("stored theme version is invalid".into()))?;
    match (package_id, package_revision) {
        (None, None) => Ok(ActiveTheme {
            selection: ThemeSelection::Default,
            tokens: default_theme_tokens(),
            assets: ThemeAssets {
                logo: None,
                preview: None,
            },
            version,
            updated_at,
        }),
        (Some(package_id), Some(package_revision)) => {
            validate_identity("theme", &package_id)
                .map_err(|_| AppError::Internal("stored theme identity is invalid".into()))?;
            let revision = u32::try_from(package_revision)
                .map_err(|_| AppError::Internal("stored theme revision is invalid".into()))?;
            let tokens: String = sqlx::query_scalar(
                "SELECT tokens FROM catalog_theme_revisions \
                 WHERE package_id = ? AND package_revision = ?",
            )
            .bind(&package_id)
            .bind(package_revision)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::Internal("selected theme revision is missing".into()))?;
            let tokens: ThemeTokens = serde_json::from_str(&tokens)
                .map_err(|_| AppError::Internal("stored theme tokens are invalid".into()))?;
            validate_theme_tokens(&tokens)
                .map_err(|_| AppError::Internal("stored theme tokens are unsafe".into()))?;

            let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
                "SELECT role, checksum_sha256, media_type, size_bytes FROM catalog_files \
                 WHERE package_id = ? AND package_revision = ? \
                 AND role IN ('logo', 'preview') ORDER BY role",
            )
            .bind(&package_id)
            .bind(package_revision)
            .fetch_all(pool)
            .await?;
            let mut logo = None;
            let mut preview = None;
            for (role, sha256, media_type, size_bytes) in rows {
                validate_sha256(&sha256, "catalog.invalid_stored_checksum").map_err(|_| {
                    AppError::Internal("stored theme asset checksum is invalid".into())
                })?;
                let size_bytes = u64::try_from(size_bytes)
                    .map_err(|_| AppError::Internal("stored theme asset size is invalid".into()))?;
                if media_type != "image/png" || size_bytes == 0 || size_bytes > MAX_PNG_BYTES {
                    return Err(AppError::Internal(
                        "stored theme asset metadata is invalid".into(),
                    ));
                }
                let asset = ThemeAsset {
                    url: format!(
                        "/api/v1/catalog/theme/{package_id}/revisions/{revision}/assets/{role}"
                    ),
                    sha256,
                    media_type,
                    size_bytes,
                };
                match role.as_str() {
                    "logo" if logo.is_none() => logo = Some(asset),
                    "preview" if preview.is_none() => preview = Some(asset),
                    _ => {
                        return Err(AppError::Internal(
                            "stored theme asset role is invalid".into(),
                        ));
                    }
                }
            }
            Ok(ActiveTheme {
                selection: ThemeSelection::Catalog {
                    package_id,
                    revision,
                },
                tokens,
                assets: ThemeAssets { logo, preview },
                version,
                updated_at,
            })
        }
        _ => Err(AppError::Internal(
            "stored theme selection is inconsistent".into(),
        )),
    }
}

fn default_theme_tokens() -> ThemeTokens {
    ThemeTokens {
        accent: "#3A82F6".into(),
        bg_primary: "#000000".into(),
        bg_secondary: "#0A0A0A".into(),
        bg_tertiary: "#111111".into(),
        bg_elevated: "#161616".into(),
        border: "#27272A".into(),
        border_hover: "#71717A".into(),
        text_primary: "#FFFFFF".into(),
        text_secondary: "#D4D4D8".into(),
        text_muted: "#A1A1AA".into(),
        success: "#10B981".into(),
        warning: "#F59E0B".into(),
        danger: "#EF4444".into(),
        info: "#3B82F6".into(),
    }
}

fn file_from_row(row: FileRow) -> Result<CatalogFile, AppError> {
    Ok(CatalogFile {
        role: row.role,
        path: row.relative_path,
        media_type: row.media_type,
        sha256: row.checksum_sha256,
        size_bytes: u64::try_from(row.size_bytes)
            .map_err(|_| AppError::Internal("stored catalog file size is invalid".into()))?,
    })
}

fn validate_manifest_metadata(manifest: &CatalogManifest) -> Result<(), AppError> {
    if manifest.format != FORMAT_NAME || manifest.schema_version != SCHEMA_VERSION {
        return Err(AppError::BadRequest(
            "catalog.schema_version_unsupported".into(),
        ));
    }
    validate_identity(manifest.kind(), &manifest.id)?;
    if manifest.revision == 0
        || !valid_label(&manifest.name, 80)
        || !valid_label(&manifest.description, 500)
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_FILES
    {
        return Err(AppError::BadRequest("catalog.manifest_invalid".into()));
    }
    let mut previous = None::<&str>;
    for file in &manifest.files {
        if previous.is_some_and(|value| value >= file.path.as_str()) {
            return Err(AppError::BadRequest("catalog.files_must_be_sorted".into()));
        }
        previous = Some(&file.path);
    }
    Ok(())
}

fn roles_for_content(content: &CatalogContent) -> Result<BTreeMap<String, &'static str>, AppError> {
    let mut roles = BTreeMap::new();
    let mut insert = |path: &str, role| {
        if roles.insert(path.to_string(), role).is_some() {
            Err(AppError::BadRequest("catalog.file_roles_invalid".into()))
        } else {
            Ok(())
        }
    };
    match content {
        CatalogContent::SteamProfile {
            definition,
            settings_schema,
            ui_schema,
            icon,
        } => {
            insert(definition, "definition")?;
            insert(settings_schema, "settings_schema")?;
            insert(ui_schema, "ui_schema")?;
            if let Some(icon) = icon {
                insert(icon, "icon")?;
            }
        }
        CatalogContent::Theme {
            tokens,
            logo,
            preview,
        } => {
            insert(tokens, "tokens")?;
            if let Some(logo) = logo {
                insert(logo, "logo")?;
            }
            if let Some(preview) = preview {
                insert(preview, "preview")?;
            }
        }
    }
    Ok(roles)
}

fn validate_file_declaration(file: &CatalogFileDeclaration, role: &str) -> Result<(), AppError> {
    validate_pack_path(&file.path)?;
    validate_sha256(&file.sha256, "catalog.file_checksum_invalid")?;
    let image = matches!(role, "icon" | "logo" | "preview");
    let expected_media = if image {
        "image/png"
    } else {
        "application/json"
    };
    if file.media_type != expected_media
        || file.size_bytes == 0
        || file.size_bytes > if image { MAX_PNG_BYTES } else { MAX_JSON_BYTES }
        || (image && !file.path.ends_with(".png"))
        || (!image && !file.path.ends_with(".json"))
    {
        return Err(AppError::BadRequest(
            "catalog.file_declaration_invalid".into(),
        ));
    }
    Ok(())
}

fn validate_identity(kind: &str, id: &str) -> Result<(), AppError> {
    let prefix = match kind {
        "steam_profile" => "steam-",
        "theme" => "theme-",
        _ => return Err(AppError::BadRequest("catalog.invalid_kind".into())),
    };
    if id.len() < 7
        || id.len() > 64
        || !id.starts_with(prefix)
        || id == "steam-custom"
        || id.ends_with('-')
        || id.contains("--")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AppError::BadRequest("catalog.invalid_id".into()));
    }
    Ok(())
}

fn validate_pack_path(path: &str) -> Result<(), AppError> {
    if path.is_empty()
        || path.len() > 256
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains("//")
        || !path.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-/".contains(&byte)
        })
    {
        return Err(AppError::BadRequest("catalog.file_path_invalid".into()));
    }
    for component in Path::new(path).components() {
        let Component::Normal(component) = component else {
            return Err(AppError::BadRequest("catalog.file_path_invalid".into()));
        };
        let component = component
            .to_str()
            .ok_or_else(|| AppError::BadRequest("catalog.file_path_invalid".into()))?;
        let stem = component.split('.').next().unwrap_or_default();
        if component == "."
            || component == ".."
            || matches!(stem, "con" | "prn" | "aux" | "nul")
            || (stem.len() == 4
                && matches!(&stem[..3], "com" | "lpt")
                && stem.as_bytes()[3].is_ascii_digit()
                && stem.as_bytes()[3] != b'0')
        {
            return Err(AppError::BadRequest("catalog.file_path_invalid".into()));
        }
    }
    Ok(())
}

fn validate_theme_tokens(tokens: &ThemeTokens) -> Result<(), AppError> {
    let colors = [
        &tokens.accent,
        &tokens.bg_primary,
        &tokens.bg_secondary,
        &tokens.bg_tertiary,
        &tokens.bg_elevated,
        &tokens.border,
        &tokens.border_hover,
        &tokens.text_primary,
        &tokens.text_secondary,
        &tokens.text_muted,
        &tokens.success,
        &tokens.warning,
        &tokens.danger,
        &tokens.info,
    ];
    if colors.iter().any(|color| parse_color(color).is_none()) {
        return Err(AppError::BadRequest("catalog.theme_tokens_invalid".into()));
    }
    for foreground in [
        &tokens.text_primary,
        &tokens.text_secondary,
        &tokens.text_muted,
    ] {
        for background in [
            &tokens.bg_primary,
            &tokens.bg_secondary,
            &tokens.bg_tertiary,
            &tokens.bg_elevated,
        ] {
            if contrast_ratio(foreground, background) < 4.5 {
                return Err(AppError::BadRequest(
                    "catalog.theme_contrast_invalid".into(),
                ));
            }
        }
    }
    for foreground in [
        &tokens.accent,
        &tokens.success,
        &tokens.warning,
        &tokens.danger,
        &tokens.info,
        &tokens.border_hover,
    ] {
        if contrast_ratio(foreground, &tokens.bg_primary) < 3.0 {
            return Err(AppError::BadRequest(
                "catalog.theme_contrast_invalid".into(),
            ));
        }
    }
    Ok(())
}

async fn validate_png(path: &Path) -> Result<(), AppError> {
    let bytes = tokio::fs::read(path).await?;
    if bytes.len() < 45 || bytes[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return Err(AppError::BadRequest("catalog.png_invalid".into()));
    }
    let mut offset = 8_usize;
    let mut first = true;
    let mut saw_data = false;
    let mut saw_end = false;
    while offset < bytes.len() {
        if bytes.len() - offset < 12 {
            return Err(AppError::BadRequest("catalog.png_invalid".into()));
        }
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let chunk_end = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .filter(|value| *value <= bytes.len())
            .ok_or_else(|| AppError::BadRequest("catalog.png_invalid".into()))?;
        let kind = &bytes[offset + 4..offset + 8];
        let data = &bytes[offset + 8..offset + 8 + length];
        let expected_crc =
            u32::from_be_bytes(bytes[offset + 8 + length..chunk_end].try_into().unwrap());
        if png_crc(kind, data) != expected_crc
            || !matches!(kind, b"IHDR" | b"PLTE" | b"tRNS" | b"IDAT" | b"IEND")
        {
            return Err(AppError::BadRequest("catalog.png_invalid".into()));
        }
        if first {
            if kind != b"IHDR" || length != 13 {
                return Err(AppError::BadRequest("catalog.png_invalid".into()));
            }
            let width = u32::from_be_bytes(data[..4].try_into().unwrap());
            let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
            let bit_depth = data[8];
            let color_type = data[9];
            let valid_depth = match color_type {
                0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                _ => false,
            };
            if width == 0
                || height == 0
                || width > 2048
                || height > 2048
                || width.saturating_mul(height) > 4_000_000
                || !valid_depth
                || data[10] != 0
                || data[11] != 0
                || !matches!(data[12], 0 | 1)
            {
                return Err(AppError::BadRequest("catalog.png_invalid".into()));
            }
            first = false;
        } else if kind == b"IHDR" {
            return Err(AppError::BadRequest("catalog.png_invalid".into()));
        }
        if kind == b"IDAT" {
            saw_data = true;
        }
        if kind == b"IEND" {
            if length != 0 || chunk_end != bytes.len() {
                return Err(AppError::BadRequest("catalog.png_invalid".into()));
            }
            saw_end = true;
        }
        offset = chunk_end;
    }
    if first || !saw_data || !saw_end {
        return Err(AppError::BadRequest("catalog.png_invalid".into()));
    }
    Ok(())
}

fn png_crc(kind: &[u8], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7 || !value.starts_with('#') {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ])
}

fn contrast_ratio(left: &str, right: &str) -> f64 {
    let luminance = |color: [u8; 3]| {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
    };
    let left = luminance(parse_color(left).expect("validated color"));
    let right = luminance(parse_color(right).expect("validated color"));
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

fn valid_label(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= maximum
        && !value.chars().any(char::is_control)
}

fn validate_sha256(value: &str, message: &str) -> Result<(), AppError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::BadRequest(message.into()));
    }
    Ok(())
}

fn validate_uuid(value: &str, message: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AppError::BadRequest(message.into()))
}

fn relative_utf8(root: &Path, path: &Path) -> Result<String, AppError> {
    path.strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| AppError::BadRequest("catalog.file_path_invalid".into()))
}

async fn read_unique_json(path: &Path, maximum: u64) -> Result<Value, AppError> {
    let metadata = tokio::fs::metadata(path).await?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(AppError::BadRequest("catalog.json_file_invalid".into()));
    }
    let bytes = tokio::fs::read(path).await?;
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let UniqueJson(value) = UniqueJson::deserialize(&mut deserializer)
        .map_err(|_| AppError::BadRequest("catalog.json_file_invalid".into()))?;
    deserializer
        .end()
        .map_err(|_| AppError::BadRequest("catalog.json_file_invalid".into()))?;
    Ok(value)
}

#[derive(Debug)]
struct FileDigest {
    sha256: String,
    size_bytes: u64,
}

async fn hash_path(path: &Path, maximum: u64) -> Result<FileDigest, AppError> {
    let mut file = tokio::fs::File::open(path).await?;
    hash_open_file(&mut file, maximum).await
}

async fn hash_open_file(file: &mut tokio::fs::File, maximum: u64) -> Result<FileDigest, AppError> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size_bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        size_bytes = size_bytes
            .checked_add(read as u64)
            .ok_or_else(|| AppError::BadRequest("catalog.file_too_large".into()))?;
        if size_bytes > maximum {
            return Err(AppError::BadRequest("catalog.file_too_large".into()));
        }
        digest.update(&buffer[..read]);
    }
    Ok(FileDigest {
        sha256: format!("{:x}", digest.finalize()),
        size_bytes,
    })
}

fn map_upload_error(error: AppError) -> AppError {
    match error {
        AppError::BadRequest(message) if message == "files.upload_too_large" => {
            AppError::BadRequest("catalog.archive_too_large".into())
        }
        AppError::BadRequest(message) if message == "files.upload_invalid" => {
            AppError::BadRequest("catalog.upload_invalid".into())
        }
        other => other,
    }
}

fn map_archive_error(error: crate::services::installers::InstallerError) -> AppError {
    tracing::warn!(code = error.code, detail = ?error.internal, "dmxpack archive rejected");
    if matches!(
        error.code,
        "archive_too_many_entries"
            | "archive_file_too_large"
            | "archive_too_large"
            | "archive_compression_ratio_exceeded"
            | "archive_size_overflow"
    ) {
        AppError::BadRequest("catalog.archive_quota_exceeded".into())
    } else {
        AppError::BadRequest("catalog.archive_invalid".into())
    }
}

fn map_database_error(error: sqlx::Error) -> AppError {
    let message = error.to_string();
    if message.contains("catalog quota exceeded") {
        AppError::Conflict("catalog.quota_exceeded".into())
    } else if message.contains("UNIQUE constraint failed") {
        AppError::Conflict("catalog.revision_exists".into())
    } else if message.contains("FOREIGN KEY constraint failed") {
        AppError::Conflict("catalog.revision_in_use".into())
    } else {
        error.into()
    }
}

fn staging_root(settings: &Settings, job_id: &str) -> PathBuf {
    settings.catalog_dir().join("staging").join(job_id)
}

fn revision_root(settings: &Settings, archive_sha256: &str) -> PathBuf {
    settings
        .catalog_dir()
        .join("revisions")
        .join(archive_sha256)
}

async fn ensure_private_directory(path: &Path) -> Result<(), AppError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            set_private_permissions(path).await
        }
        Ok(_) => Err(AppError::Internal(format!(
            "unsafe catalog directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(path).await?;
            set_private_permissions(path).await
        }
        Err(error) => Err(error.into()),
    }
}

async fn validate_revision_storage(path: &Path, archive_sha256: &str) -> Result<(), AppError> {
    validate_sha256(archive_sha256, "catalog.invalid_stored_checksum")?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::Internal(
            "active catalog revision storage is not a regular directory".into(),
        ));
    }
    for child in [path.join("package.dmxpack"), path.join("content")] {
        let metadata = tokio::fs::symlink_metadata(&child).await?;
        if metadata.file_type().is_symlink()
            || (!metadata.is_file() && child.ends_with("package.dmxpack"))
            || (!metadata.is_dir() && child.ends_with("content"))
        {
            return Err(AppError::Internal(
                "active catalog revision storage is incomplete or unsafe".into(),
            ));
        }
    }
    Ok(())
}

async fn set_private_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

async fn remove_tree(path: &Path) -> Result<(), AppError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        tokio::fs::remove_file(path).await?;
    } else if metadata.is_dir() {
        tokio::fs::remove_dir_all(path).await?;
    } else {
        return Err(AppError::Internal("unsafe catalog entry".into()));
    }
    Ok(())
}

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueJson)
            .ok_or_else(|| E::custom("invalid JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueJson(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        UniqueJson::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(UniqueJson(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(UniqueJson(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
            }
            let UniqueJson(value) = object.next_value()?;
            values.insert(key, value);
        }
        Ok(UniqueJson(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write, net::SocketAddr, sync::Arc};

    use futures::stream;
    use sha2::{Digest as _, Sha256};
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::{
        core::{Settings, config::DeploymentMode, database},
        domain::v1::{PortProtocol, PortSpec, SteamExecutable, SteamStopStrategy},
    };

    struct TestContext {
        _root: TempDir,
        settings: Settings,
        pool: DbPool,
        profiles: Arc<ProfileRegistry>,
        owner_id: String,
    }

    async fn context() -> TestContext {
        let root = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite:{}/catalog.db?mode=rwc", root.path().display());
        let settings = Settings {
            config_file: root.path().join("config.toml"),
            data_dir: root.path().join("data"),
            static_dir: root.path().join("static"),
            bind: SocketAddr::from(([127, 0, 0, 1], 5500)),
            database_url: database_url.clone(),
            master_key_file: root.path().join("master.key"),
            steamcmd_path: root.path().join("steamcmd"),
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
        };
        tokio::fs::create_dir_all(&settings.data_dir).await.unwrap();
        let pool = database::init_pool(&database_url).await.unwrap();
        database::run_migrations(&pool).await.unwrap();
        let profiles = Arc::new(ProfileRegistry::builtins());
        profiles.persist_builtins(&pool).await.unwrap();
        let owner_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users \
             (id, username, password_hash, role_id, created_at, updated_at) \
             VALUES (?, 'catalog-owner', 'unused', 'owner', ?, ?)",
        )
        .bind(&owner_id)
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        prepare(&settings).await.unwrap();
        TestContext {
            _root: root,
            settings,
            pool,
            profiles,
            owner_id,
        }
    }

    fn steam_definition(app_id: u32) -> SteamProfile {
        SteamProfile {
            app_id,
            branch: None,
            executable: SteamExecutable {
                linux_x86_64: Some("server".into()),
                windows_x86_64: Some("server.exe".into()),
            },
            arguments: vec!["--port".into(), "{{port:game_port}}".into()],
            ports: vec![PortSpec {
                name: "game_port".into(),
                protocol: PortProtocol::Udp,
                default: 27_015,
                adjacent_to: None,
            }],
            save_paths: vec!["saves".into()],
            ready_log_pattern: Some("Ready".into()),
            stop_strategy: SteamStopStrategy::Terminate {
                timeout_seconds: 30,
            },
        }
    }

    fn steam_pack(
        id: &str,
        revision: u32,
        app_id: u32,
        invalid_schema: bool,
        undeclared_file: Option<(&str, &[u8])>,
    ) -> Vec<u8> {
        let steam = steam_definition(app_id);
        let profile = build_local_steam_profile(
            id.into(),
            revision,
            "Fixture server".into(),
            "Anonymous SteamCMD fixture".into(),
            steam.clone(),
        )
        .unwrap();
        let mut files = BTreeMap::<String, Vec<u8>>::new();
        files.insert(
            "profile/definition.json".into(),
            serde_json::to_vec(&steam).unwrap(),
        );
        files.insert(
            "profile/settings-schema.json".into(),
            if invalid_schema {
                b"{}".to_vec()
            } else {
                serde_json::to_vec(&profile.settings_schema).unwrap()
            },
        );
        files.insert(
            "profile/ui-schema.json".into(),
            serde_json::to_vec(&profile.ui_schema).unwrap(),
        );
        let declarations = files
            .iter()
            .map(|(path, bytes)| CatalogFileDeclaration {
                path: path.clone(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                size_bytes: bytes.len() as u64,
                media_type: "application/json".into(),
            })
            .collect();
        let manifest = CatalogManifest {
            format: FORMAT_NAME.into(),
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            revision,
            name: "Fixture server".into(),
            description: "Anonymous SteamCMD fixture".into(),
            content: CatalogContent::SteamProfile {
                definition: "profile/definition.json".into(),
                settings_schema: "profile/settings-schema.json".into(),
                ui_schema: "profile/ui-schema.json".into(),
                icon: None,
            },
            files: declarations,
        };
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        if let Some((name, bytes)) = undeclared_file {
            files.insert(name.into(), bytes.to_vec());
        }
        zip_bytes(files)
    }

    fn theme_pack_with_tokens(tokens: &[u8]) -> Vec<u8> {
        let path = "theme/tokens.json";
        let manifest = CatalogManifest {
            format: FORMAT_NAME.into(),
            schema_version: SCHEMA_VERSION,
            id: "theme-fixture".into(),
            revision: 1,
            name: "Fixture theme".into(),
            description: "Strict design-token fixture".into(),
            content: CatalogContent::Theme {
                tokens: path.into(),
                logo: None,
                preview: None,
            },
            files: vec![CatalogFileDeclaration {
                path: path.into(),
                sha256: format!("{:x}", Sha256::digest(tokens)),
                size_bytes: tokens.len() as u64,
                media_type: "application/json".into(),
            }],
        };
        zip_bytes(BTreeMap::from([
            (
                "manifest.json".into(),
                serde_json::to_vec(&manifest).unwrap(),
            ),
            (path.into(), tokens.to_vec()),
        ]))
    }

    fn zip_bytes(files: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        for (path, bytes) in files {
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    async fn import_pack(
        context: &TestContext,
        bytes: Vec<u8>,
    ) -> Result<CatalogPackage, AppError> {
        let job_id = uuid::Uuid::new_v4().to_string();
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        stage_upload(
            &context.settings,
            &job_id,
            &sha256,
            stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from(bytes))]),
        )
        .await?;
        let result = import_staged(
            &context.pool,
            &context.settings,
            &context.profiles,
            &job_id,
            &context.owner_id,
            &sha256,
        )
        .await;
        if result.is_err() {
            discard_staging(&context.settings, &job_id).await;
        }
        result
    }

    #[tokio::test]
    async fn steam_revisions_are_immutable_and_instances_remain_pinned() {
        let context = context().await;
        let first = import_pack(&context, steam_pack("steam-fixture", 1, 90, false, None))
            .await
            .unwrap();
        assert_eq!(first.revision, 1);
        assert!(context.profiles.get_revision("steam-fixture", 1).is_some());

        let instance_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO instances \
             (id, name, profile_id, profile_revision, settings, created_at, updated_at) \
             VALUES (?, 'pinned', 'steam-fixture', 1, '{}', ?, ?)",
        )
        .bind(&instance_id)
        .bind(&now)
        .bind(&now)
        .execute(&context.pool)
        .await
        .unwrap();

        assert!(
            import_pack(&context, steam_pack("steam-fixture", 2, 91, false, None),)
                .await
                .is_err(),
            "a package revision must not change the Steam AppID"
        );
        let second = import_pack(&context, steam_pack("steam-fixture", 2, 90, false, None))
            .await
            .unwrap();
        assert_eq!(second.revision, 2);
        let pinned: i64 = sqlx::query_scalar("SELECT profile_revision FROM instances WHERE id = ?")
            .bind(&instance_id)
            .fetch_one(&context.pool)
            .await
            .unwrap();
        assert_eq!(
            pinned, 1,
            "installing a revision must never auto-upgrade an instance"
        );
        assert!(
            remove(
                &context.pool,
                &context.settings,
                &context.profiles,
                "steam_profile",
                "steam-fixture",
                1,
            )
            .await
            .is_err(),
            "a pinned revision must not be deletable"
        );

        remove(
            &context.pool,
            &context.settings,
            &context.profiles,
            "steam_profile",
            "steam-fixture",
            2,
        )
        .await
        .unwrap();
        assert!(context.profiles.get_revision("steam-fixture", 2).is_none());
        assert!(
            import_pack(&context, steam_pack("steam-fixture", 2, 90, false, None),)
                .await
                .is_err(),
            "a tombstoned revision number must never be reused"
        );
        import_pack(&context, steam_pack("steam-fixture", 3, 90, false, None))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn undeclared_code_and_noncanonical_schemas_are_rejected() {
        let context = context().await;
        assert!(
            import_pack(
                &context,
                steam_pack(
                    "steam-extra-code",
                    1,
                    90,
                    false,
                    Some(("assets/theme.css", b"body { display: none; }")),
                ),
            )
            .await
            .is_err()
        );
        assert!(
            import_pack(
                &context,
                steam_pack("steam-schema-mismatch", 1, 90, true, None),
            )
            .await
            .is_err()
        );
        let installed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_packages")
            .fetch_one(&context.pool)
            .await
            .unwrap();
        assert_eq!(installed, 0);
    }

    #[tokio::test]
    async fn themes_accept_only_closed_safe_accessible_tokens() {
        let context = context().await;
        let injected = br##"{
            "accent":"url(javascript:alert(1))","bg_primary":"#000000",
            "bg_secondary":"#000000","bg_tertiary":"#000000","bg_elevated":"#000000",
            "border":"#ffffff","border_hover":"#ffffff","text_primary":"#ffffff",
            "text_secondary":"#ffffff","text_muted":"#ffffff","success":"#ffffff",
            "warning":"#ffffff","danger":"#ffffff","info":"#ffffff"
        }"##;
        assert!(
            import_pack(&context, theme_pack_with_tokens(injected))
                .await
                .is_err()
        );

        let low_contrast = br##"{
            "accent":"#ffffff","bg_primary":"#ffffff","bg_secondary":"#ffffff",
            "bg_tertiary":"#ffffff","bg_elevated":"#ffffff","border":"#ffffff",
            "border_hover":"#ffffff","text_primary":"#ffffff","text_secondary":"#ffffff",
            "text_muted":"#ffffff","success":"#ffffff","warning":"#ffffff",
            "danger":"#ffffff","info":"#ffffff"
        }"##;
        assert!(
            import_pack(&context, theme_pack_with_tokens(low_contrast))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn startup_recovery_removes_orphans_and_refuses_missing_active_storage() {
        let context = context().await;
        let package = import_pack(&context, steam_pack("steam-recovery", 1, 90, false, None))
            .await
            .unwrap();
        let orphan = revision_root(&context.settings, &"f".repeat(64));
        tokio::fs::create_dir_all(orphan.join("content"))
            .await
            .unwrap();
        tokio::fs::write(orphan.join("package.dmxpack"), b"orphan")
            .await
            .unwrap();
        let interrupted = context
            .settings
            .catalog_dir()
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string());
        tokio::fs::create_dir_all(&interrupted).await.unwrap();

        cleanup_interrupted(&context.pool, &context.settings)
            .await
            .unwrap();
        assert!(!tokio::fs::try_exists(orphan).await.unwrap());
        assert!(!tokio::fs::try_exists(interrupted).await.unwrap());

        remove_tree(&revision_root(&context.settings, &package.archive_sha256))
            .await
            .unwrap();
        assert!(
            cleanup_interrupted(&context.pool, &context.settings)
                .await
                .is_err(),
            "startup must not silently accept a database revision with missing files"
        );
    }

    #[tokio::test]
    async fn global_theme_selection_is_exact_versioned_and_protects_active_revision() {
        let context = context().await;
        let initial = active_theme(&context.pool).await.unwrap();
        assert_eq!(initial.selection, ThemeSelection::Default);
        assert_eq!(initial.version, 1);
        validate_theme_tokens(&initial.tokens).unwrap();

        let tokens = serde_json::to_vec(&default_theme_tokens()).unwrap();
        import_pack(&context, theme_pack_with_tokens(&tokens))
            .await
            .unwrap();
        assert!(
            select_theme(
                &context.pool,
                &context.owner_id,
                &ThemeSelection::Catalog {
                    package_id: "theme-missing".into(),
                    revision: 1,
                },
                1,
            )
            .await
            .is_err()
        );

        let selected = select_theme(
            &context.pool,
            &context.owner_id,
            &ThemeSelection::Catalog {
                package_id: "theme-fixture".into(),
                revision: 1,
            },
            1,
        )
        .await
        .unwrap();
        assert_eq!(selected.version, 2);
        assert_eq!(selected.tokens, default_theme_tokens());
        assert_eq!(selected.assets.logo, None);
        assert!(
            select_theme(
                &context.pool,
                &context.owner_id,
                &ThemeSelection::Default,
                1,
            )
            .await
            .is_err(),
            "a stale theme version must not overwrite a newer selection"
        );
        assert!(
            remove(
                &context.pool,
                &context.settings,
                &context.profiles,
                "theme",
                "theme-fixture",
                1,
            )
            .await
            .is_err(),
            "the active theme revision must not be deletable"
        );

        let reset = select_theme(
            &context.pool,
            &context.owner_id,
            &ThemeSelection::Default,
            2,
        )
        .await
        .unwrap();
        assert_eq!(reset.selection, ThemeSelection::Default);
        assert_eq!(reset.version, 3);
        remove(
            &context.pool,
            &context.settings,
            &context.profiles,
            "theme",
            "theme-fixture",
            1,
        )
        .await
        .unwrap();
    }
}
