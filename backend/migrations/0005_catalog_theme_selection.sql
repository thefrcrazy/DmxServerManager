CREATE TABLE catalog_theme_selection (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    package_id TEXT,
    package_revision INTEGER,
    version INTEGER NOT NULL CHECK (version > 0),
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (package_id IS NULL AND package_revision IS NULL)
        OR (package_id IS NOT NULL AND package_revision IS NOT NULL)
    ),
    FOREIGN KEY (package_id, package_revision)
        REFERENCES catalog_theme_revisions(package_id, package_revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

INSERT INTO catalog_theme_selection
    (singleton, package_id, package_revision, version, updated_at)
VALUES (1, NULL, NULL, 1, '1970-01-01T00:00:00Z');
