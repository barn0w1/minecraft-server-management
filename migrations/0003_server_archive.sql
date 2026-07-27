-- Archived servers remain in the database permanently so their globally
-- unique names cannot be reused and their R2 repositories remain detached
-- from every destructive lifecycle operation.
ALTER TABLE servers
ADD COLUMN archived_at_ms INTEGER
    CHECK (
        archived_at_ms IS NULL
        OR (
            archived_at_ms >= created_at_ms
            AND archived_at_ms <= updated_at_ms
            AND desired_state = 'stopped'
        )
    );

CREATE INDEX servers_active_name_idx
ON servers (name)
WHERE archived_at_ms IS NULL;
