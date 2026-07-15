CREATE TABLE catalog_packages (
    id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN ('steam_profile', 'theme')),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    description TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 500),
    archive_sha256 TEXT NOT NULL UNIQUE CHECK (
        length(archive_sha256) = 64
        AND archive_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    archive_size_bytes INTEGER NOT NULL CHECK (archive_size_bytes BETWEEN 1 AND 16777216),
    content_size_bytes INTEGER NOT NULL CHECK (content_size_bytes BETWEEN 1 AND 33554432),
    manifest TEXT NOT NULL CHECK (
        json_valid(manifest)
        AND json_type(manifest) = 'object'
    ),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (id, revision)
);

CREATE INDEX idx_catalog_packages_kind_name
    ON catalog_packages(kind, name COLLATE NOCASE, revision DESC);

CREATE TABLE catalog_files (
    package_id TEXT NOT NULL,
    package_revision INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (
        role IN ('definition', 'settings_schema', 'ui_schema', 'tokens', 'icon', 'logo', 'preview')
    ),
    relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 256),
    media_type TEXT NOT NULL CHECK (media_type IN ('application/json', 'image/png')),
    checksum_sha256 TEXT NOT NULL CHECK (
        length(checksum_sha256) = 64
        AND checksum_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    size_bytes INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 4194304),
    PRIMARY KEY (package_id, package_revision, relative_path),
    UNIQUE (package_id, package_revision, role),
    FOREIGN KEY (package_id, package_revision)
        REFERENCES catalog_packages(id, revision) ON DELETE CASCADE
);

CREATE TABLE catalog_theme_revisions (
    package_id TEXT NOT NULL,
    package_revision INTEGER NOT NULL,
    tokens TEXT NOT NULL CHECK (json_valid(tokens) AND json_type(tokens) = 'object'),
    PRIMARY KEY (package_id, package_revision),
    FOREIGN KEY (package_id, package_revision)
        REFERENCES catalog_packages(id, revision) ON DELETE CASCADE
);

CREATE TABLE catalog_profile_revisions (
    package_id TEXT NOT NULL,
    package_revision INTEGER NOT NULL,
    profile_id TEXT NOT NULL,
    profile_revision INTEGER NOT NULL,
    PRIMARY KEY (package_id, package_revision),
    UNIQUE (profile_id, profile_revision),
    FOREIGN KEY (package_id, package_revision)
        REFERENCES catalog_packages(id, revision) ON DELETE CASCADE,
    FOREIGN KEY (profile_id, profile_revision)
        REFERENCES game_profiles(id, revision) ON DELETE RESTRICT
);

CREATE TABLE catalog_revision_tombstones (
    id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN ('steam_profile', 'theme')),
    archive_sha256 TEXT NOT NULL CHECK (
        length(archive_sha256) = 64
        AND archive_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    deleted_at TEXT NOT NULL,
    PRIMARY KEY (id, revision)
);

CREATE TRIGGER catalog_packages_no_update
BEFORE UPDATE ON catalog_packages
BEGIN
    SELECT RAISE(ABORT, 'catalog packages are immutable');
END;

CREATE TRIGGER catalog_files_no_update
BEFORE UPDATE ON catalog_files
BEGIN
    SELECT RAISE(ABORT, 'catalog files are immutable');
END;

CREATE TRIGGER catalog_theme_revisions_no_update
BEFORE UPDATE ON catalog_theme_revisions
BEGIN
    SELECT RAISE(ABORT, 'catalog theme revisions are immutable');
END;

CREATE TRIGGER catalog_profile_revisions_no_update
BEFORE UPDATE ON catalog_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'catalog profile revisions are immutable');
END;

CREATE TRIGGER catalog_revision_tombstones_no_update
BEFORE UPDATE ON catalog_revision_tombstones
BEGIN
    SELECT RAISE(ABORT, 'catalog revision tombstones are immutable');
END;

CREATE TRIGGER catalog_revision_tombstones_no_delete
BEFORE DELETE ON catalog_revision_tombstones
BEGIN
    SELECT RAISE(ABORT, 'catalog revision tombstones are immutable');
END;

CREATE TRIGGER catalog_packages_quota
BEFORE INSERT ON catalog_packages
WHEN
    (SELECT COUNT(*) FROM catalog_packages) >= 256
    OR (SELECT COALESCE(SUM(archive_size_bytes), 0) FROM catalog_packages)
        + NEW.archive_size_bytes > 536870912
    OR (SELECT COALESCE(SUM(content_size_bytes), 0) FROM catalog_packages)
        + NEW.content_size_bytes > 536870912
BEGIN
    SELECT RAISE(ABORT, 'catalog quota exceeded');
END;
