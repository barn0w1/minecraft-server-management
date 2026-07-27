CREATE TABLE servers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE CHECK (length(trim(name)) > 0 AND length(name) <= 128 AND instr(name, char(0)) = 0),
    generation INTEGER NOT NULL CHECK (generation > 0),
    desired_state TEXT NOT NULL CHECK (desired_state IN ('running', 'stopped')),
    spec_json TEXT NOT NULL CHECK (json_valid(spec_json)),
    current_snapshot_id TEXT CHECK (
        current_snapshot_id IS NULL
        OR (length(trim(current_snapshot_id)) > 0 AND length(current_snapshot_id) <= 256 AND instr(current_snapshot_id, char(0)) = 0)
    ),
    next_fencing_token INTEGER NOT NULL DEFAULT 1 CHECK (next_fencing_token > 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms)
) STRICT;

CREATE INDEX servers_desired_state_idx ON servers (desired_state);

CREATE TABLE server_instances (
    id TEXT PRIMARY KEY NOT NULL,
    server_id TEXT NOT NULL REFERENCES servers (id),
    server_generation INTEGER NOT NULL CHECK (server_generation > 0),
    resolved_spec_json TEXT NOT NULL CHECK (json_valid(resolved_spec_json)),
    fencing_token INTEGER NOT NULL CHECK (fencing_token > 0),
    source_snapshot_id TEXT CHECK (
        source_snapshot_id IS NULL
        OR (length(trim(source_snapshot_id)) > 0 AND length(source_snapshot_id) <= 256 AND instr(source_snapshot_id, char(0)) = 0)
    ),
    data_prepared_at_ms INTEGER CHECK (data_prepared_at_ms >= 0),
    process_running INTEGER NOT NULL DEFAULT 0 CHECK (process_running IN (0, 1)),
    process_observed_at_ms INTEGER CHECK (process_observed_at_ms >= 0),
    result_snapshot_id TEXT CHECK (
        result_snapshot_id IS NULL
        OR (length(trim(result_snapshot_id)) > 0 AND length(result_snapshot_id) <= 256 AND instr(result_snapshot_id, char(0)) = 0)
    ),
    stop_requested_at_ms INTEGER CHECK (stop_requested_at_ms >= 0),
    terminated_at_ms INTEGER CHECK (terminated_at_ms >= 0),
    terminal_result TEXT CHECK (terminal_result IN ('completed', 'failed')),
    last_error TEXT CHECK (last_error IS NULL OR (length(trim(last_error)) > 0 AND length(last_error) <= 8192 AND instr(last_error, char(0)) = 0)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    CHECK (
        (terminated_at_ms IS NULL AND terminal_result IS NULL)
        OR (terminated_at_ms IS NOT NULL AND terminal_result IS NOT NULL)
    ),
    CHECK (process_running = 0 OR process_observed_at_ms IS NOT NULL),
    CHECK (terminated_at_ms IS NULL OR process_running = 0),
    CHECK (data_prepared_at_ms IS NULL OR data_prepared_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (process_observed_at_ms IS NULL OR process_observed_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (stop_requested_at_ms IS NULL OR stop_requested_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (terminated_at_ms IS NULL OR terminated_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (
        terminated_at_ms IS NULL
        OR terminated_at_ms >= max(
            coalesce(data_prepared_at_ms, 0),
            coalesce(process_observed_at_ms, 0),
            coalesce(stop_requested_at_ms, 0)
        )
    ),
    UNIQUE (server_id, fencing_token)
) STRICT;

CREATE UNIQUE INDEX server_instances_one_active_per_server_idx
ON server_instances (server_id)
WHERE terminated_at_ms IS NULL;

CREATE INDEX server_instances_server_history_idx
ON server_instances (server_id, fencing_token DESC, id);

CREATE TABLE compute_instances (
    id TEXT PRIMARY KEY NOT NULL,
    server_instance_id TEXT NOT NULL REFERENCES server_instances (id),
    provider TEXT NOT NULL CHECK (provider IN ('local_process', 'akamai')),
    provider_instance_id TEXT CHECK (
        provider_instance_id IS NULL
        OR (length(trim(provider_instance_id)) > 0 AND length(provider_instance_id) <= 256 AND instr(provider_instance_id, char(0)) = 0)
    ),
    public_ipv4 TEXT CHECK (
        public_ipv4 IS NULL
        OR (length(trim(public_ipv4)) > 0 AND length(public_ipv4) <= 64 AND instr(public_ipv4, char(0)) = 0)
    ),
    connection_token TEXT NOT NULL CHECK (length(trim(connection_token)) > 0 AND length(connection_token) <= 256 AND instr(connection_token, char(0)) = 0),
    enrollment_token TEXT CHECK (
        enrollment_token IS NULL
        OR (length(trim(enrollment_token)) > 0 AND length(enrollment_token) <= 256 AND instr(enrollment_token, char(0)) = 0)
    ),
    agent_csr_pem TEXT CHECK (
        agent_csr_pem IS NULL
        OR (length(agent_csr_pem) > 0 AND length(agent_csr_pem) <= 32768 AND instr(agent_csr_pem, char(0)) = 0)
    ),
    agent_certificate_chain_pem TEXT CHECK (
        agent_certificate_chain_pem IS NULL
        OR (length(agent_certificate_chain_pem) > 0 AND length(agent_certificate_chain_pem) <= 65536 AND instr(agent_certificate_chain_pem, char(0)) = 0)
    ),
    agent_certificate_der BLOB CHECK (
        agent_certificate_der IS NULL OR length(agent_certificate_der) BETWEEN 1 AND 65536
    ),
    agent_certificate_expires_at_ms INTEGER CHECK (agent_certificate_expires_at_ms >= 0),
    process_id INTEGER CHECK (process_id > 0),
    agent_connected_at_ms INTEGER CHECK (agent_connected_at_ms >= 0),
    shutdown_requested_at_ms INTEGER CHECK (shutdown_requested_at_ms >= 0),
    terminated_at_ms INTEGER CHECK (terminated_at_ms >= 0),
    terminal_result TEXT CHECK (terminal_result IN ('deleted', 'failed')),
    failure_message TEXT CHECK (failure_message IS NULL OR (length(trim(failure_message)) > 0 AND length(failure_message) <= 8192 AND instr(failure_message, char(0)) = 0)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    CHECK (
        (terminated_at_ms IS NULL AND terminal_result IS NULL)
        OR (terminated_at_ms IS NOT NULL AND terminal_result IS NOT NULL)
    ),
    CHECK (
        (
            provider = 'local_process'
            AND provider_instance_id IS NULL
            AND public_ipv4 IS NULL
            AND enrollment_token IS NULL
            AND agent_csr_pem IS NULL
            AND agent_certificate_chain_pem IS NULL
            AND agent_certificate_der IS NULL
            AND agent_certificate_expires_at_ms IS NULL
        )
        OR (provider = 'akamai' AND process_id IS NULL)
    ),
    CHECK (
        (agent_csr_pem IS NULL AND agent_certificate_chain_pem IS NULL AND agent_certificate_der IS NULL AND agent_certificate_expires_at_ms IS NULL)
        OR (agent_csr_pem IS NOT NULL AND agent_certificate_chain_pem IS NOT NULL AND agent_certificate_der IS NOT NULL AND agent_certificate_expires_at_ms IS NOT NULL)
    ),
    CHECK (agent_certificate_expires_at_ms IS NULL OR agent_certificate_expires_at_ms >= created_at_ms),
    CHECK (agent_connected_at_ms IS NULL OR agent_connected_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (shutdown_requested_at_ms IS NULL OR shutdown_requested_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (terminated_at_ms IS NULL OR terminated_at_ms BETWEEN created_at_ms AND updated_at_ms),
    CHECK (
        terminated_at_ms IS NULL
        OR terminated_at_ms >= max(
            coalesce(agent_connected_at_ms, 0),
            coalesce(shutdown_requested_at_ms, 0)
        )
    )
) STRICT;

CREATE UNIQUE INDEX compute_instances_one_active_per_server_instance_idx
ON compute_instances (server_instance_id)
WHERE terminated_at_ms IS NULL;

CREATE INDEX compute_instances_server_instance_history_idx
ON compute_instances (server_instance_id, created_at_ms DESC, id);

CREATE TABLE snapshots (
    id TEXT NOT NULL CHECK (length(trim(id)) > 0 AND length(id) <= 256 AND instr(id, char(0)) = 0),
    server_id TEXT NOT NULL REFERENCES servers (id),
    server_instance_id TEXT NOT NULL REFERENCES server_instances (id),
    fencing_token INTEGER NOT NULL CHECK (fencing_token > 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (server_id, id),
    UNIQUE (server_id, fencing_token)
) STRICT;

CREATE INDEX snapshots_server_history_idx
ON snapshots (server_id, fencing_token DESC, id);
