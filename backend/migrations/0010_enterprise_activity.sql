ALTER TABLE sessions ADD COLUMN user_agent TEXT NOT NULL DEFAULT 'Unknown client';

CREATE TABLE panel_settings (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    advertised_game_host TEXT,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO panel_settings (singleton, advertised_game_host, version, updated_at)
VALUES (1, NULL, 1, datetime('now'));

CREATE INDEX idx_jobs_activity_cursor ON jobs(created_at DESC, id DESC);
CREATE INDEX idx_jobs_activity_state_cursor ON jobs(state, created_at DESC, id DESC);
CREATE INDEX idx_audit_actor_cursor ON audit_events(actor_user_id, id DESC);
CREATE INDEX idx_audit_action_cursor ON audit_events(action, id DESC);
CREATE INDEX idx_audit_outcome_cursor ON audit_events(outcome, id DESC);

DROP TABLE IF EXISTS chat_messages;
DROP TABLE IF EXISTS notifications;

UPDATE roles
SET permissions = COALESCE(
    (
        SELECT json_group_array(value)
        FROM json_each(roles.permissions)
        WHERE value NOT IN ('chat.read', 'chat.write', 'notifications.read')
    ),
    '[]'
)
WHERE id <> 'owner';

UPDATE roles
SET permissions = '["audit.read","job.read","mods.manage","panel.network.manage","profile.read","schedule.manage","server.backup","server.backup.read","server.config.raw.read","server.config.raw.write","server.console.read","server.console.write","server.create","server.delete","server.files.read","server.files.write","server.kill","server.read","server.start","server.stop","server.update","server.update_game","user.create","user.read","user.update"]',
    updated_at = datetime('now')
WHERE id = 'admin';
