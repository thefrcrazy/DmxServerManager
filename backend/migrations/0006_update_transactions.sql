CREATE TABLE instance_update_transactions (
    instance_id TEXT PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    job_id TEXT NOT NULL UNIQUE REFERENCES jobs(id) ON DELETE CASCADE,
    previous_installation_state TEXT NOT NULL
        CHECK (previous_installation_state IN ('installed', 'not_installed')),
    previous_installed_version TEXT,
    previous_installed_build TEXT,
    previous_desired_state TEXT NOT NULL
        CHECK (previous_desired_state IN ('running', 'stopped')),
    restart_after INTEGER NOT NULL CHECK (restart_after IN (0, 1)),
    phase TEXT NOT NULL DEFAULT 'preparing'
        CHECK (phase IN ('preparing', 'committed', 'finalizing', 'rolling_back', 'rolled_back')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_instance_update_transactions_job
    ON instance_update_transactions(job_id);
