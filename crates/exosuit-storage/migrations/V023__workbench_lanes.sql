-- V023: Portable workbench lanes and machine-local workspace focus.
--
-- RFC 10202 makes lane identity and phase association portable project state,
-- while keeping each linked worktree's focused lane out of SQL projections.

CREATE TABLE workbench_lanes_data (
    id                 INTEGER PRIMARY KEY,
    text_id            TEXT NOT NULL UNIQUE,
    title              TEXT NOT NULL
                               CHECK(length(trim(
                                   title,
                                   char(9) || char(10) || char(11) ||
                                   char(12) || char(13) || ' '
                               )) > 0),
    intent             TEXT NOT NULL
                               CHECK(length(trim(
                                   intent,
                                   char(9) || char(10) || char(11) ||
                                   char(12) || char(13) || ' '
                               )) > 0),
    state              TEXT NOT NULL CHECK(state IN ('prepared', 'executing')),
    execution_phase_id INTEGER NOT NULL
                               REFERENCES phases_data(id) ON DELETE RESTRICT,
    created_at         TEXT NOT NULL
                               DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at         TEXT NOT NULL
                               DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_workbench_lanes_execution_phase
ON workbench_lanes_data(execution_phase_id);

CREATE INDEX idx_workbench_lanes_state
ON workbench_lanes_data(state);

CREATE TABLE workspace_lane_focus_data (
    workspace_root TEXT PRIMARY KEY,
    lane_id        INTEGER NOT NULL
                           REFERENCES workbench_lanes_data(id) ON DELETE CASCADE,
    updated_at     TEXT NOT NULL
                           DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_workspace_lane_focus_lane
ON workspace_lane_focus_data(lane_id);

CREATE TABLE workbench_lanes_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE workspace_lane_focus_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES
    ('workbench_lanes_data', 0),
    ('workspace_lane_focus_data', 0)
ON CONFLICT(table_name) DO NOTHING;
