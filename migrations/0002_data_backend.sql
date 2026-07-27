-- v0.2.0 makes the storage backend explicit in the persisted server
-- specification. Existing v0.1.x records are migrated in place so an
-- operator can upgrade without discarding audit history.
UPDATE servers
SET spec_json = json_set(
    spec_json,
    '$.data.backend',
    CASE
        WHEN json_extract(spec_json, '$.data.repository') LIKE 's3:%'
            THEN 'r2_restic'
        ELSE 'local_restic'
    END
)
WHERE json_type(spec_json, '$.data.backend') IS NULL;

UPDATE server_instances
SET resolved_spec_json = json_set(
    resolved_spec_json,
    '$.data.backend',
    CASE
        WHEN json_extract(resolved_spec_json, '$.data.repository') LIKE 's3:%'
            THEN 'r2_restic'
        ELSE 'local_restic'
    END
)
WHERE json_type(resolved_spec_json, '$.data.backend') IS NULL;
