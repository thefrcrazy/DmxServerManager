ALTER TABLE instance_update_transactions ADD COLUMN previous_settings TEXT;
ALTER TABLE instance_update_transactions ADD COLUMN previous_config_version INTEGER;

UPDATE instance_update_transactions
SET previous_settings = (
        SELECT settings FROM instances WHERE instances.id = instance_update_transactions.instance_id
    ),
    previous_config_version = (
        SELECT config_version FROM instances WHERE instances.id = instance_update_transactions.instance_id
    );
