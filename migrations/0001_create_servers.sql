CREATE TABLE servers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    generation INTEGER NOT NULL CHECK (generation > 0),
    desired_state TEXT NOT NULL CHECK (desired_state IN ('running', 'stopped')),
    spec_json TEXT NOT NULL CHECK (json_valid(spec_json)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX servers_desired_state_idx ON servers (desired_state);
