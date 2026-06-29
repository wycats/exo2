-- V017: Workspace-local active phase pin table.
--
-- Maps a canonical workspace root path to the phase currently pinned for that
-- workspace. This is project-local runtime state, not git-dumped context.

CREATE TABLE workspace_active_phase_data (
    workspace_root TEXT PRIMARY KEY,
    phase_id       INTEGER NOT NULL REFERENCES phases_data(id) ON DELETE CASCADE,
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_workspace_active_phase_phase ON workspace_active_phase_data(phase_id);

CREATE TABLE workspace_active_phase_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES ('workspace_active_phase_data', 0)
ON CONFLICT(table_name) DO NOTHING;
