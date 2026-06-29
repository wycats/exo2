-- V012__axioms_table.sql
-- Axioms table for project steering rules
-- Migrated from docs/agent-context/axioms.*.toml per RFC 10180

-- ═══════════════════════════════════════════════════════════
-- Axioms (canonical state, structured steering data)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE axioms (
    id          INTEGER PRIMARY KEY,           -- rowid alias, stable identity
    text_id     TEXT NOT NULL UNIQUE,          -- human-readable ID (e.g. "green-to-green")
    scope       TEXT NOT NULL DEFAULT 'workflow'
                CHECK (scope IN ('workflow', 'system', 'design')),
    principle   TEXT NOT NULL,                 -- the axiom statement
    rationale   TEXT,                          -- why this axiom exists
    notes       TEXT,                          -- additional context
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_axioms_scope ON axioms(scope);
CREATE INDEX idx_axioms_text_id ON axioms(text_id);

-- ═══════════════════════════════════════════════════════════
-- Axiom implications (one-to-many)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE axiom_implications (
    axiom_id    INTEGER NOT NULL REFERENCES axioms(id) ON DELETE CASCADE,
    implication TEXT NOT NULL,
    sort_key    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (axiom_id, sort_key)
);

-- ═══════════════════════════════════════════════════════════
-- Axiom tags (junction table)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE axiom_tags (
    axiom_id INTEGER NOT NULL REFERENCES axioms(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (axiom_id, tag)
);

CREATE INDEX idx_axiom_tags_tag ON axiom_tags(tag);
