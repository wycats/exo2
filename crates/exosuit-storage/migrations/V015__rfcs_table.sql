-- V015__rfcs_table.sql
-- RFC metadata table for reactive RFC state.
-- Source of truth for queryable metadata; RFC prose stays on disk.
-- Identity is the ULID, stored as an HTML comment anchor in the file.

CREATE TABLE rfcs_data (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,          -- ULID (stable identity)
    rfc_number  INTEGER NOT NULL,               -- numeric ID (e.g. 10181), not unique across withdrawn/replaced
    title       TEXT NOT NULL,
    stage       INTEGER NOT NULL DEFAULT 0     -- 0=idea, 1=proposal, 2=draft, 3=candidate, 4=stable
                CHECK (stage BETWEEN 0 AND 4),
    status      TEXT NOT NULL DEFAULT 'active' -- active, archived, withdrawn
                CHECK (status IN ('active', 'archived', 'withdrawn')),
    feature     TEXT,                          -- feature grouping tag
    slug        TEXT NOT NULL,                 -- URL-friendly slug from filename
    file_path   TEXT NOT NULL,                 -- relative path from repo root

    -- Lifecycle metadata (moved from frontmatter)
    superseded_by     TEXT,                    -- RFC number that supersedes this one
    supersedes        TEXT,                    -- RFC number(s) this one supersedes
    withdrawal_reason TEXT,                    -- why it was withdrawn
    archived_reason   TEXT,                    -- why it was archived
    consolidated_into TEXT,                    -- RFC number it was consolidated into

    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT
);

CREATE INDEX idx_rfcs_stage ON rfcs_data(stage);
CREATE INDEX idx_rfcs_status ON rfcs_data(status);
CREATE INDEX idx_rfcs_feature ON rfcs_data(feature);
CREATE INDEX idx_rfcs_rfc_number ON rfcs_data(rfc_number);

-- RFC cross-references (relations from frontmatter)
CREATE TABLE rfc_relations (
    rfc_id      INTEGER NOT NULL REFERENCES rfcs_data(id),
    related_rfc TEXT NOT NULL,                 -- RFC number as string
    relation    TEXT NOT NULL DEFAULT 'related' -- relation type
                CHECK (relation IN ('related', 'depends-on', 'extends')),
    PRIMARY KEY (rfc_id, related_rfc)
);
