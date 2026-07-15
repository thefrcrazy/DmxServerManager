CREATE UNIQUE INDEX IF NOT EXISTS idx_instances_attached_data_path_unique
ON instances(data_path)
WHERE storage_mode = 'attached' AND data_path IS NOT NULL;
