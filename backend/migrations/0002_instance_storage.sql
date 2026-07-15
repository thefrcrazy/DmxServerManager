ALTER TABLE instances ADD COLUMN storage_mode TEXT NOT NULL DEFAULT 'managed'
    CHECK (storage_mode IN ('managed', 'attached'));

ALTER TABLE instances ADD COLUMN data_path TEXT;

CREATE TRIGGER instances_storage_consistency_insert
BEFORE INSERT ON instances
WHEN NOT (
    (NEW.storage_mode = 'managed' AND NEW.managed = 1 AND NEW.data_path IS NULL)
    OR
    (NEW.storage_mode = 'attached' AND NEW.managed = 0 AND NEW.data_path IS NOT NULL AND length(NEW.data_path) > 0)
)
BEGIN
    SELECT RAISE(ABORT, 'invalid instance storage configuration');
END;

CREATE TRIGGER instances_storage_consistency_update
BEFORE UPDATE OF storage_mode, data_path, managed ON instances
WHEN NOT (
    (NEW.storage_mode = 'managed' AND NEW.managed = 1 AND NEW.data_path IS NULL)
    OR
    (NEW.storage_mode = 'attached' AND NEW.managed = 0 AND NEW.data_path IS NOT NULL AND length(NEW.data_path) > 0)
)
BEGIN
    SELECT RAISE(ABORT, 'invalid instance storage configuration');
END;
