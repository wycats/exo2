-- V025: Canonical project-flow relationships and machine-local provider evidence.
--
-- Portable relationship authority is kept separate from legacy phase RFC rows.
-- Provider observations and prepared external reads are local execution state.

CREATE TABLE campaign_rfc_objectives_data (
    id                  INTEGER PRIMARY KEY,
    text_id             TEXT NOT NULL UNIQUE,
    phase_id            INTEGER NOT NULL REFERENCES phases_data(id) ON DELETE RESTRICT,
    rfc_ulid            TEXT NOT NULL,
    rfc_number_snapshot INTEGER NOT NULL,
    rfc_title_snapshot  TEXT NOT NULL,
    observed_stage      INTEGER NOT NULL CHECK(observed_stage BETWEEN 0 AND 4),
    target_stage        INTEGER CHECK(target_stage BETWEEN 0 AND 4),
    relation            TEXT NOT NULL CHECK(relation IN ('drives', 'implements', 'validates')),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(phase_id, rfc_ulid)
);

CREATE INDEX idx_campaign_rfc_objectives_phase
ON campaign_rfc_objectives_data(phase_id);

CREATE INDEX idx_campaign_rfc_objectives_rfc
ON campaign_rfc_objectives_data(rfc_ulid);

CREATE TABLE project_flow_pull_requests_data (
    id         INTEGER PRIMARY KEY,
    text_id    TEXT NOT NULL UNIQUE,
    provider   TEXT NOT NULL CHECK(provider = 'github'),
    repository TEXT NOT NULL,
    number     INTEGER NOT NULL CHECK(number > 0),
    url        TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(provider, repository, number)
);

CREATE TABLE phase_pull_request_relations_data (
    id          INTEGER PRIMARY KEY,
    phase_id    INTEGER NOT NULL REFERENCES phases_data(id) ON DELETE RESTRICT,
    artifact_id INTEGER NOT NULL REFERENCES project_flow_pull_requests_data(id) ON DELETE RESTRICT,
    role        TEXT NOT NULL CHECK(role IN ('implements', 'validates')),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(phase_id, artifact_id)
);

CREATE INDEX idx_phase_pull_request_relations_phase
ON phase_pull_request_relations_data(phase_id);

CREATE INDEX idx_phase_pull_request_relations_artifact
ON phase_pull_request_relations_data(artifact_id);

CREATE TABLE project_flow_pull_request_observations_data (
    id              INTEGER PRIMARY KEY,
    artifact_id     INTEGER NOT NULL UNIQUE
                            REFERENCES project_flow_pull_requests_data(id) ON DELETE CASCADE,
    title           TEXT,
    lifecycle       TEXT CHECK(lifecycle IN ('open', 'closed', 'merged')),
    head_oid        TEXT,
    review_state    TEXT CHECK(review_state IN ('none', 'pending', 'approved', 'changes_requested', 'unknown')),
    checks_state    TEXT CHECK(checks_state IN ('none', 'pending', 'passing', 'failing', 'unknown')),
    last_success_at TEXT,
    last_attempt_at TEXT NOT NULL,
    last_error      TEXT
);

CREATE TABLE project_flow_prepared_reads (
    request_id             TEXT PRIMARY KEY,
    request_hash           TEXT NOT NULL,
    normalized_payload     TEXT NOT NULL,
    phase_text_id          TEXT NOT NULL,
    targets_json           TEXT NOT NULL,
    provider_results_json  TEXT,
    owner_instance_id      TEXT NOT NULL,
    owner_pid              INTEGER NOT NULL,
    owner_process_start_id TEXT NOT NULL,
    recovery_class         TEXT NOT NULL CHECK(recovery_class = 'prepared_external_read'),
    state                  TEXT NOT NULL DEFAULT 'prepared'
                                CHECK(state IN ('prepared', 'ready', 'terminalizing', 'completed', 'abandoned')),
    prepared_at            TEXT NOT NULL,
    completed_at           TEXT,
    result_json            TEXT
);

CREATE TABLE campaign_rfc_objectives_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE project_flow_pull_requests_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE phase_pull_request_relations_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE project_flow_pull_request_observations_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES
    ('campaign_rfc_objectives_data', 0),
    ('project_flow_pull_requests_data', 0),
    ('phase_pull_request_relations_data', 0),
    ('project_flow_pull_request_observations_data', 0)
ON CONFLICT(table_name) DO NOTHING;
