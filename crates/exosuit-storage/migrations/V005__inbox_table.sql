-- V005__inbox_table.sql
-- Inbox table for user→agent intent communication
-- Based on RFC 10176 inbox schema with normalized tagged unions

PRAGMA foreign_keys = ON;

-- ═══════════════════════════════════════════════════════════
-- Inbox (core table, will be renamed to inbox_data)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE inbox (
    id          INTEGER PRIMARY KEY,           -- rowid alias, stable identity
    text_id     TEXT NOT NULL UNIQUE,          -- UUID string from TOML
    created_at  TEXT NOT NULL,                 -- RFC 3339 datetime
    updated_at  TEXT,                          -- RFC 3339 datetime (optional)
    
    -- Status lifecycle: pending → acknowledged → resolved → archived
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'acknowledged', 'resolved', 'archived')),
    
    -- Category of intent
    category    TEXT NOT NULL DEFAULT 'guidance'
                CHECK (category IN ('correction', 'guidance', 'question', 'priority')),
    
    -- Urgency for surfacing
    urgency     TEXT NOT NULL DEFAULT 'next-touch'
                CHECK (urgency IN ('immediate', 'next-touch', 'when-relevant')),
    
    -- Content
    subject     TEXT NOT NULL,                 -- brief summary (like email subject)
    body        TEXT NOT NULL DEFAULT '',      -- full content
    resolution  TEXT,                          -- when status = resolved
    
    -- Normalized tagged union: scope (IntentScope)
    scope_type  TEXT NOT NULL DEFAULT 'global'
                CHECK (scope_type IN ('global', 'phase', 'file', 'rust', 'typescript')),
    scope_value TEXT,                          -- NULL for global/rust/typescript
    
    -- Normalized tagged union: subject_ref (Option<SubjectRef>)
    subject_ref_type TEXT
                CHECK (subject_ref_type IS NULL OR subject_ref_type IN ('goal', 'task', 'phase', 'rfc')),
    subject_ref_id   TEXT,                     -- NULL when subject_ref_type is NULL
    
    -- Normalized tagged union: action (Option<IntentAction>)
    action_type TEXT
                CHECK (action_type IS NULL OR action_type IN ('complete-goal', 'complete-task', 'verify-task', 'add-note')),
    action_payload TEXT,                       -- evidence or note content (optional even when action_type set)
    
    -- Table-level constraints for tagged union consistency
    CHECK (
        (scope_type IN ('global', 'rust', 'typescript') AND scope_value IS NULL) OR
        (scope_type IN ('phase', 'file') AND scope_value IS NOT NULL)
    ),
    CHECK (
        (subject_ref_type IS NULL AND subject_ref_id IS NULL) OR
        (subject_ref_type IS NOT NULL AND subject_ref_id IS NOT NULL)
    )
);

-- Indexes for common queries
CREATE INDEX idx_inbox_status ON inbox(status);
CREATE INDEX idx_inbox_created ON inbox(created_at);
CREATE INDEX idx_inbox_scope_type ON inbox(scope_type);

-- ═══════════════════════════════════════════════════════════
-- Rename to shadow table convention (per V002 pattern)
-- ═══════════════════════════════════════════════════════════

ALTER TABLE inbox RENAME TO inbox_data;
