-- V019: Phase ownership claims.
--
-- Separates a workspace's focused phase from the owner that is allowed to
-- mutate that phase. Ownership includes machine-local workspace identity, so it
-- is intentionally runtime state and is not emitted in git-friendly SQL dumps.

CREATE TABLE phase_ownership_data (
    phase_id                  INTEGER PRIMARY KEY REFERENCES phases_data(id) ON DELETE CASCADE,
    owner_kind                TEXT NOT NULL CHECK(owner_kind IN ('workspace', 'branch', 'pr')),
    owner_id                  TEXT NOT NULL,
    claimed_by_workspace_id   TEXT,
    claimed_by_workspace_root TEXT,
    claimed_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at                TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_phase_ownership_owner
ON phase_ownership_data(owner_kind, owner_id);

CREATE INDEX idx_phase_ownership_workspace
ON phase_ownership_data(claimed_by_workspace_id);

CREATE TABLE phase_ownership_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES ('phase_ownership_data', 0)
ON CONFLICT(table_name) DO NOTHING;
