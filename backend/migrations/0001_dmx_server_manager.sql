CREATE TABLE roles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    permissions TEXT NOT NULL CHECK (json_valid(permissions)),
    is_system INTEGER NOT NULL DEFAULT 0 CHECK (is_system IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_roles_name_nocase ON roles(name COLLATE NOCASE);

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role_id TEXT NOT NULL REFERENCES roles(id) ON UPDATE CASCADE ON DELETE RESTRICT,
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    language TEXT NOT NULL DEFAULT 'fr' CHECK (language IN ('fr', 'en')),
    accent_color TEXT NOT NULL DEFAULT '#3A82F6',
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    last_login_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE setup_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1))
);

INSERT INTO setup_state (singleton, completed) VALUES (1, 0);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    csrf_hash TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL
);

CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_expiry ON sessions(expires_at);

CREATE TABLE login_rate_limits (
    key_hash TEXT PRIMARY KEY,
    failure_count INTEGER NOT NULL CHECK (failure_count >= 0),
    window_started INTEGER NOT NULL,
    locked_until INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_login_rate_limits_updated ON login_rate_limits(updated_at);

CREATE TABLE game_profiles (
    id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN ('builtin', 'steam_custom')),
    manifest TEXT NOT NULL CHECK (json_valid(manifest)),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (id, revision)
);

CREATE TABLE instances (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    profile_revision INTEGER NOT NULL DEFAULT 1,
    settings TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(settings)),
    config_version INTEGER NOT NULL DEFAULT 1 CHECK (config_version > 0),
    installation_state TEXT NOT NULL DEFAULT 'not_installed'
        CHECK (installation_state IN ('not_installed', 'installing', 'installed', 'updating', 'failed')),
    installed_version TEXT,
    installed_build TEXT,
    desired_state TEXT NOT NULL DEFAULT 'stopped'
        CHECK (desired_state IN ('running', 'stopped')),
    runtime_state TEXT NOT NULL DEFAULT 'stopped'
        CHECK (runtime_state IN ('stopped', 'starting', 'running', 'stopping', 'crashed', 'unknown')),
    managed INTEGER NOT NULL DEFAULT 1 CHECK (managed IN (0, 1)),
    auto_start INTEGER NOT NULL DEFAULT 0 CHECK (auto_start IN (0, 1)),
    watchdog_enabled INTEGER NOT NULL DEFAULT 1 CHECK (watchdog_enabled IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (profile_id, profile_revision) REFERENCES game_profiles(id, revision) ON UPDATE CASCADE ON DELETE RESTRICT
);

CREATE INDEX idx_instances_profile ON instances(profile_id, profile_revision);

CREATE TABLE instance_secrets (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    nonce TEXT NOT NULL,
    ciphertext TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (instance_id, name)
);

CREATE TABLE user_instance_grants (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    permissions TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(permissions)),
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, instance_id)
);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    instance_id TEXT REFERENCES instances(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'waiting_for_user', 'succeeded', 'failed', 'cancelled', 'interrupted')),
    progress INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    idempotency_key TEXT UNIQUE,
    requested_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);

CREATE INDEX idx_jobs_instance_created ON jobs(instance_id, created_at DESC);
CREATE INDEX idx_jobs_state ON jobs(state);
CREATE UNIQUE INDEX idx_jobs_one_active_per_instance ON jobs(instance_id)
    WHERE instance_id IS NOT NULL AND state IN ('queued', 'running', 'waiting_for_user');

CREATE TABLE backups (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    creation_job_id TEXT UNIQUE REFERENCES jobs(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('manual', 'scheduled', 'pre_restore', 'pre_update')),
    status TEXT NOT NULL CHECK (status IN ('creating', 'ready', 'failed')),
    storage_name TEXT NOT NULL UNIQUE,
    checksum_sha256 TEXT,
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX idx_backups_instance_created ON backups(instance_id, created_at DESC);

CREATE TABLE job_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload)),
    created_at TEXT NOT NULL
);

CREATE TABLE port_reservations (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    protocol TEXT NOT NULL CHECK (protocol IN ('tcp', 'udp')),
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    purpose TEXT NOT NULL,
    PRIMARY KEY (protocol, port)
);

CREATE TABLE server_metrics (
    id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    cpu_usage REAL NOT NULL CHECK (cpu_usage >= 0),
    memory_bytes INTEGER NOT NULL CHECK (memory_bytes >= 0),
    disk_bytes INTEGER NOT NULL CHECK (disk_bytes >= 0),
    uptime_seconds INTEGER NOT NULL DEFAULT 0 CHECK (uptime_seconds >= 0),
    player_count INTEGER CHECK (player_count IS NULL OR player_count >= 0),
    recorded_at TEXT NOT NULL
);

CREATE INDEX idx_metrics_server_recorded ON server_metrics(server_id, recorded_at);

CREATE TABLE instance_mods (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('manual', 'modrinth', 'curseforge')),
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 255),
    relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 1024),
    checksum_sha256 TEXT NOT NULL CHECK (length(checksum_sha256) = 64),
    size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
    provider_project_id TEXT,
    provider_version_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    metadata TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata) AND json_type(metadata) = 'object'),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    UNIQUE (instance_id, relative_path)
);

CREATE INDEX idx_instance_mods_instance_created
    ON instance_mods(instance_id, created_at DESC);

CREATE TABLE audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id TEXT REFERENCES users(id) ON DELETE RESTRICT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    metadata TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata)),
    created_at TEXT NOT NULL
);

CREATE INDEX idx_audit_created ON audit_events(created_at DESC);
CREATE INDEX idx_audit_resource ON audit_events(resource_type, resource_id);

CREATE TRIGGER audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit events are immutable');
END;

CREATE TRIGGER audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit events are immutable');
END;

CREATE TRIGGER job_events_no_update
BEFORE UPDATE ON job_events
BEGIN
    SELECT RAISE(ABORT, 'job events are immutable');
END;

CREATE TRIGGER job_events_no_delete
BEFORE DELETE ON job_events
BEGIN
    SELECT RAISE(ABORT, 'job events are immutable');
END;

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE schedules (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('cron', 'interval')),
    cron_expression TEXT,
    interval_seconds INTEGER,
    timezone TEXT NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    action_kind TEXT NOT NULL CHECK (action_kind IN ('start', 'stop', 'restart', 'backup', 'update', 'console')),
    action_payload TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(action_payload) AND json_type(action_payload) = 'object'),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    next_run_at TEXT,
    last_run_at TEXT,
    last_job_id TEXT REFERENCES jobs(id) ON DELETE SET NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    requested_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (trigger_kind = 'cron' AND cron_expression IS NOT NULL AND interval_seconds IS NULL)
        OR
        (trigger_kind = 'interval' AND cron_expression IS NULL AND interval_seconds BETWEEN 60 AND 31536000)
    )
);

CREATE INDEX idx_schedules_instance ON schedules(instance_id, created_at DESC);
CREATE INDEX idx_schedules_due ON schedules(enabled, next_run_at)
    WHERE enabled = 1 AND next_run_at IS NOT NULL;

CREATE TABLE schedule_runs (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    scheduled_for TEXT NOT NULL,
    action_kind TEXT NOT NULL CHECK (action_kind IN ('start', 'stop', 'restart', 'backup', 'update', 'console')),
    action_payload TEXT NOT NULL CHECK (json_valid(action_payload) AND json_type(action_payload) = 'object'),
    requested_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK (status IN ('claimed', 'submitted', 'failed')),
    job_id TEXT UNIQUE REFERENCES jobs(id) ON DELETE SET NULL,
    error_code TEXT,
    claimed_at TEXT NOT NULL,
    finished_at TEXT,
    UNIQUE (schedule_id, scheduled_for)
);

CREATE INDEX idx_schedule_runs_status ON schedule_runs(status, claimed_at);
CREATE INDEX idx_schedule_runs_schedule ON schedule_runs(schedule_id, scheduled_for DESC);
CREATE INDEX idx_schedule_runs_finished ON schedule_runs(finished_at)
    WHERE status IN ('submitted', 'failed');

CREATE TABLE chat_messages (
    id TEXT PRIMARY KEY,
    author_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    body TEXT CHECK (body IS NULL OR (length(body) BETWEEN 1 AND 4000)),
    created_at TEXT NOT NULL,
    deleted_at TEXT,
    deleted_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    CHECK (
        (deleted_at IS NULL AND body IS NOT NULL AND deleted_by IS NULL)
        OR (deleted_at IS NOT NULL AND body IS NULL)
    )
);

CREATE INDEX idx_chat_messages_created ON chat_messages(created_at DESC, id DESC);

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    message_key TEXT NOT NULL CHECK (length(message_key) BETWEEN 1 AND 128),
    data TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data) AND json_type(data) = 'object'),
    read_at TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_notifications_user_created
    ON notifications(user_id, created_at DESC, id DESC);
CREATE INDEX idx_notifications_user_unread
    ON notifications(user_id, created_at DESC)
    WHERE read_at IS NULL;

INSERT INTO roles (id, name, permissions, is_system, created_at, updated_at) VALUES
('owner', 'Owner', '["*"]', 1, datetime('now'), datetime('now')),
('admin', 'Admin', '["chat.read","chat.write","job.read","mods.manage","notifications.read","profile.read","schedule.manage","server.backup","server.backup.read","server.console.read","server.console.write","server.create","server.delete","server.files.read","server.files.write","server.kill","server.read","server.start","server.stop","server.update","server.update_game","user.create","user.read","user.update"]', 1, datetime('now'), datetime('now')),
('operator', 'Operator', '["chat.read","chat.write","job.read","mods.manage","notifications.read","profile.read","schedule.manage","server.backup","server.backup.read","server.console.read","server.console.write","server.files.read","server.files.write","server.read","server.start","server.stop","server.update","server.update_game"]', 1, datetime('now'), datetime('now')),
('viewer', 'Viewer', '["chat.read","job.read","notifications.read","profile.read","server.backup.read","server.console.read","server.read"]', 1, datetime('now'), datetime('now'));
