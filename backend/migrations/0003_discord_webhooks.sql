CREATE TABLE discord_webhooks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    url_nonce TEXT NOT NULL,
    url_ciphertext TEXT NOT NULL,
    events TEXT NOT NULL CHECK (json_valid(events) AND json_type(events) = 'array'),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    last_delivery_at TEXT,
    last_error_code TEXT,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_discord_webhooks_enabled ON discord_webhooks(enabled) WHERE enabled = 1;

CREATE TRIGGER limit_discord_webhooks
BEFORE INSERT ON discord_webhooks
WHEN (SELECT count(*) FROM discord_webhooks) >= 16
BEGIN
    SELECT RAISE(ABORT, 'discord webhook limit reached');
END;
