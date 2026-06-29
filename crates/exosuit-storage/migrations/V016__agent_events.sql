-- V016: Agent events table for the Agent Activity Model (RFC 10183).
--
-- Append-only event log capturing agent tool calls and file saves.
-- NOT a reactive vtab — events don't need trace invalidation and
-- shouldn't trigger sidebar refreshes.

CREATE TABLE agent_events (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,
    timestamp   TEXT NOT NULL,
    agent_id    TEXT,
    event_type  TEXT NOT NULL
                CHECK (event_type IN ('command', 'file_save')),
    namespace   TEXT,
    operation   TEXT,
    entity_type TEXT,
    entity_id   TEXT,
    effect      TEXT
                CHECK (effect IS NULL OR effect IN ('read', 'write')),
    duration_ms INTEGER,
    summary     TEXT NOT NULL
);

CREATE INDEX idx_agent_events_timestamp ON agent_events(timestamp);
CREATE INDEX idx_agent_events_session ON agent_events(agent_id, timestamp);
CREATE INDEX idx_agent_events_entity ON agent_events(entity_type, entity_id);
