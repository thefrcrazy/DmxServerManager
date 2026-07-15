ALTER TABLE jobs ADD COLUMN cancel_requested_at TEXT;

CREATE INDEX idx_jobs_install_cancel_requested
    ON jobs(instance_id, cancel_requested_at)
    WHERE kind = 'install' AND cancel_requested_at IS NOT NULL;
