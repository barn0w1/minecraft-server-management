ALTER TABLE servers
ADD COLUMN next_fencing_token INTEGER NOT NULL DEFAULT 1
CHECK (next_fencing_token > 0);

CREATE TABLE server_instances (
    id TEXT PRIMARY KEY NOT NULL,
    server_id TEXT NOT NULL REFERENCES servers (id),
    server_generation INTEGER NOT NULL CHECK (server_generation > 0),
    resolved_spec_json TEXT NOT NULL CHECK (json_valid(resolved_spec_json)),
    fencing_token INTEGER NOT NULL CHECK (fencing_token > 0),
    stop_requested_at_ms INTEGER CHECK (stop_requested_at_ms >= 0),
    terminated_at_ms INTEGER CHECK (terminated_at_ms >= 0),
    terminal_result TEXT CHECK (terminal_result IN ('completed', 'failed')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    CHECK (
        (terminated_at_ms IS NULL AND terminal_result IS NULL)
        OR (terminated_at_ms IS NOT NULL AND terminal_result IS NOT NULL)
    ),
    CHECK (stop_requested_at_ms IS NULL OR stop_requested_at_ms >= created_at_ms),
    CHECK (terminated_at_ms IS NULL OR terminated_at_ms >= created_at_ms),
    CHECK (
        stop_requested_at_ms IS NULL
        OR terminated_at_ms IS NULL
        OR terminated_at_ms >= stop_requested_at_ms
    ),
    UNIQUE (server_id, fencing_token)
) STRICT;

CREATE UNIQUE INDEX server_instances_one_active_per_server_idx
ON server_instances (server_id)
WHERE terminated_at_ms IS NULL;

CREATE INDEX server_instances_server_history_idx
ON server_instances (server_id, fencing_token DESC, id);
