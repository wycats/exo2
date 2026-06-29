-- V004__ideas_table.sql
-- Ideas table for capturing feature ideas, improvements, and observations
-- Based on docs/agent-context/ideas.toml schema

PRAGMA foreign_keys = ON;

-- ═══════════════════════════════════════════════════════════
-- Ideas (core table, will be renamed to ideas_data)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE ideas (
    id          INTEGER PRIMARY KEY,           -- rowid alias, stable identity
    text_id     TEXT NOT NULL UNIQUE,          -- UUID string from TOML
    title       TEXT NOT NULL,
    description TEXT,
    status      TEXT NOT NULL DEFAULT 'new'
                CHECK (status IN ('new', 'archived')),
    created_at  TEXT NOT NULL,                 -- RFC 3339 datetime
    source      TEXT NOT NULL DEFAULT 'user'
                CHECK (source IN ('user', 'agent'))
);

CREATE INDEX idx_ideas_status ON ideas(status);
CREATE INDEX idx_ideas_created ON ideas(created_at);

-- ═══════════════════════════════════════════════════════════
-- Idea tags (junction table for tags array)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE idea_tags (
    idea_id INTEGER NOT NULL REFERENCES ideas(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (idea_id, tag)
);

CREATE INDEX idx_idea_tags_tag ON idea_tags(tag);

-- ═══════════════════════════════════════════════════════════
-- Idea task references (junction table for related_tasks array)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE idea_task_refs (
    idea_id  INTEGER NOT NULL REFERENCES ideas(id) ON DELETE CASCADE,
    task_ref TEXT NOT NULL,                    -- task ID reference (string)
    PRIMARY KEY (idea_id, task_ref)
);

-- ═══════════════════════════════════════════════════════════
-- Rename to shadow table convention (per V002 pattern)
-- ═══════════════════════════════════════════════════════════

ALTER TABLE ideas RENAME TO ideas_data;
-- Junction tables are NOT renamed (per RFC 10165: "Junction tables are plain tables")
