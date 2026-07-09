-- V022: Add machine-local RFC workspace observations.
--
-- RFC 10196 separates portable canonical RFC metadata from the document view
-- observed by one local workspace. Snapshot, observation, and diagnostic rows
-- are reactive, while baseline and quarantine records support the one-time
-- transition from pre-overlay shared state.

CREATE TABLE rfc_workspace_snapshots_data (
    id              INTEGER PRIMARY KEY,
    workspace_root  TEXT NOT NULL UNIQUE,
    branch_name     TEXT,
    head_oid        TEXT NOT NULL,
    document_digest BLOB NOT NULL CHECK(length(document_digest) = 32),
    canonical_ref   TEXT,
    canonical_oid   TEXT,
    observed_at     TEXT NOT NULL
);

CREATE TABLE rfc_workspace_observations_data (
    id                         INTEGER PRIMARY KEY,
    workspace_root             TEXT NOT NULL,
    text_id                    TEXT NOT NULL,
    rfc_number                 INTEGER NOT NULL,
    title                      TEXT NOT NULL,
    stage                      INTEGER NOT NULL CHECK(stage BETWEEN 0 AND 4),
    stage_source               TEXT NOT NULL
                                   CHECK(stage_source IN ('path', 'marker', 'legacy')),
    status                     TEXT NOT NULL
                                   CHECK(status IN ('active', 'archived', 'withdrawn')),
    feature                    TEXT,
    feature_declared           INTEGER NOT NULL CHECK(feature_declared IN (0, 1)),
    slug                       TEXT NOT NULL,
    file_path                  TEXT NOT NULL,
    superseded_by              TEXT,
    superseded_by_declared     INTEGER NOT NULL CHECK(superseded_by_declared IN (0, 1)),
    supersedes                 TEXT,
    supersedes_declared        INTEGER NOT NULL CHECK(supersedes_declared IN (0, 1)),
    withdrawal_reason          TEXT,
    withdrawal_reason_declared INTEGER NOT NULL CHECK(withdrawal_reason_declared IN (0, 1)),
    archived_reason            TEXT,
    archived_reason_declared   INTEGER NOT NULL CHECK(archived_reason_declared IN (0, 1)),
    consolidated_into          TEXT,
    consolidated_into_declared INTEGER NOT NULL CHECK(consolidated_into_declared IN (0, 1)),
    branch_name                TEXT,
    head_oid                   TEXT NOT NULL,
    observed_at                TEXT NOT NULL,
    UNIQUE(workspace_root, text_id),
    UNIQUE(workspace_root, file_path),
    FOREIGN KEY(workspace_root)
        REFERENCES rfc_workspace_snapshots_data(workspace_root)
        ON DELETE CASCADE
);

CREATE INDEX idx_rfc_workspace_observations_number
    ON rfc_workspace_observations_data(workspace_root, rfc_number);
CREATE INDEX idx_rfc_workspace_observations_lifecycle
    ON rfc_workspace_observations_data(workspace_root, status, stage);

CREATE TABLE rfc_workspace_diagnostics_data (
    id              INTEGER PRIMARY KEY,
    workspace_root  TEXT NOT NULL,
    file_path       TEXT NOT NULL,
    diagnostic_code TEXT NOT NULL,
    text_id         TEXT,
    rfc_number      INTEGER,
    message         TEXT NOT NULL,
    observed_at     TEXT NOT NULL,
    UNIQUE(workspace_root, file_path, diagnostic_code),
    FOREIGN KEY(workspace_root)
        REFERENCES rfc_workspace_snapshots_data(workspace_root)
        ON DELETE CASCADE
);

CREATE INDEX idx_rfc_workspace_diagnostics_number
    ON rfc_workspace_diagnostics_data(workspace_root, rfc_number);

CREATE TABLE rfc_workspace_snapshots_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE rfc_workspace_observations_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE rfc_workspace_diagnostics_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES
    ('rfc_workspace_snapshots_data', 0),
    ('rfc_workspace_observations_data', 0),
    ('rfc_workspace_diagnostics_data', 0)
ON CONFLICT(table_name) DO NOTHING;

CREATE TABLE rfc_canonical_baseline (
    singleton    INTEGER PRIMARY KEY CHECK(singleton = 1),
    canonical_ref TEXT NOT NULL,
    canonical_oid TEXT NOT NULL,
    completed_at  TEXT NOT NULL
);

CREATE TABLE rfc_canonical_quarantine (
    text_id           TEXT PRIMARY KEY,
    rfc_number        INTEGER NOT NULL,
    serialized_row    TEXT NOT NULL,
    quarantine_reason TEXT NOT NULL,
    quarantined_at    TEXT NOT NULL
);

CREATE INDEX idx_rfc_canonical_quarantine_number
    ON rfc_canonical_quarantine(rfc_number);
