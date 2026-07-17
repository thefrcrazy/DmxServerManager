CREATE TABLE config_changes (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 1024),
    format TEXT NOT NULL CHECK (format IN ('json', 'properties', 'ini', 'toml', 'yaml', 'xml', 'lua', 'text')),
    category TEXT NOT NULL CHECK (category IN ('configuration', 'access')),
    base_sha256 TEXT CHECK (base_sha256 IS NULL OR length(base_sha256) = 64),
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    content_nonce TEXT NOT NULL,
    content_ciphertext TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'applied', 'conflict', 'failed', 'cancelled')),
    error_code TEXT,
    queued_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    applied_at TEXT
);

CREATE UNIQUE INDEX idx_config_changes_one_pending_file
    ON config_changes(instance_id, relative_path)
    WHERE status = 'pending';

CREATE INDEX idx_config_changes_instance_status
    ON config_changes(instance_id, status, created_at);

CREATE TABLE server_players (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    player_key TEXT NOT NULL CHECK (length(player_key) BETWEEN 1 AND 255),
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 128),
    external_id TEXT CHECK (external_id IS NULL OR length(external_id) BETWEEN 1 AND 255),
    source TEXT NOT NULL CHECK (source IN ('hytale', 'minecraft_java', 'minecraft_bedrock', 'steam', 'console_log', 'generic_log')),
    online INTEGER NOT NULL DEFAULT 0 CHECK (online IN (0, 1)),
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    connected_at TEXT,
    disconnected_at TEXT,
    PRIMARY KEY (instance_id, player_key)
);

CREATE INDEX idx_server_players_instance_online
    ON server_players(instance_id, online, last_seen_at DESC);
