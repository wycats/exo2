-- V021: Canonical outcomes for atomic project-state requests.
--
-- These rows live beside the state mutation they describe so SQLite can
-- commit the mutation and its replayable response in one transaction. They
-- are runtime recovery metadata and are intentionally excluded from portable
-- SQL projections.

CREATE TABLE atomic_request_outcomes (
    request_id    TEXT PRIMARY KEY,
    request_hash  TEXT NOT NULL,
    effect        TEXT NOT NULL CHECK (effect IN ('write', 'exec')),
    response_json TEXT NOT NULL,
    committed_at  INTEGER NOT NULL
);

CREATE INDEX idx_atomic_request_outcomes_committed_at
    ON atomic_request_outcomes(committed_at);
