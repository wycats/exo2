-- V013__perception_event_schema.sql
-- RFC 10181: Shared Perception — consolidate inbox fields into perception event model
--
-- Replaces:
--   scope_type/scope_value + subject_ref_type/subject_ref_id → entity_type/entity_id
--   category → intent (claim/concern/inquiry/fyi)
--   urgency → priority
--   action_type/action_payload → (removed, steering interprets intent)
--
-- Adds:
--   source (user-feedback/system-observation/plan-mutation)
--   confidence (high/low/null)

PRAGMA foreign_keys = ON;

-- ═══════════════════════════════════════════════════════════
-- Step 1: Create the new table with the perception event schema
-- ═══════════════════════════════════════════════════════════

CREATE TABLE inbox_data_new (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    updated_at  TEXT,

    -- Status lifecycle: pending → acknowledged → resolved → archived
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'acknowledged', 'resolved', 'archived')),

    -- Entity scope: what is this about?
    entity_type TEXT NOT NULL DEFAULT 'project'
                CHECK (entity_type IN ('goal', 'task', 'rfc', 'phase', 'epoch', 'project')),
    entity_id   TEXT,  -- NULL for project-level items

    -- Source: who created it?
    source      TEXT NOT NULL DEFAULT 'user-feedback'
                CHECK (source IN ('user-feedback', 'system-observation', 'plan-mutation')),

    -- Intent: what is the sender communicating?
    intent      TEXT NOT NULL DEFAULT 'fyi'
                CHECK (intent IN ('claim', 'concern', 'inquiry', 'fyi')),

    -- Priority: when should the agent see this?
    priority    TEXT NOT NULL DEFAULT 'next-touch'
                CHECK (priority IN ('immediate', 'next-touch', 'when-relevant')),

    -- Confidence: strength of a claim (null for non-claims)
    confidence  TEXT
                CHECK (confidence IS NULL OR confidence IN ('high', 'low')),

    -- Content
    subject     TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',
    resolution  TEXT,

    -- Constraints
    CHECK (
        (entity_type = 'project' AND entity_id IS NULL) OR
        (entity_type != 'project' AND entity_id IS NOT NULL)
    )
);

-- ═══════════════════════════════════════════════════════════
-- Step 2: Migrate existing data
-- ═══════════════════════════════════════════════════════════

INSERT INTO inbox_data_new (
    id, text_id, created_at, updated_at,
    status, entity_type, entity_id,
    source, intent, priority, confidence,
    subject, body, resolution
)
SELECT
    id, text_id, created_at, updated_at,
    status,
    -- Entity scope: subject_ref takes precedence, then scope, then 'project'
    CASE
        WHEN subject_ref_type IS NOT NULL THEN subject_ref_type
        WHEN scope_type = 'phase' THEN 'phase'
        ELSE 'project'
    END,
    CASE
        WHEN subject_ref_id IS NOT NULL THEN subject_ref_id
        WHEN scope_type = 'phase' THEN scope_value
        ELSE NULL
    END,
    -- Source: all existing items are user-feedback
    'user-feedback',
    -- Intent: map category to intent
    CASE category
        WHEN 'correction' THEN 'concern'
        WHEN 'guidance'   THEN 'fyi'
        WHEN 'question'   THEN 'inquiry'
        WHEN 'priority'   THEN 'concern'
        ELSE 'fyi'
    END,
    -- Priority: urgency maps directly
    urgency,
    -- Confidence: null for all existing items
    NULL,
    subject, body, resolution
FROM inbox_data;

-- ═══════════════════════════════════════════════════════════
-- Step 3: Swap tables
-- ═══════════════════════════════════════════════════════════

DROP TABLE inbox_data;
ALTER TABLE inbox_data_new RENAME TO inbox_data;

-- ═══════════════════════════════════════════════════════════
-- Step 4: Recreate indexes
-- ═══════════════════════════════════════════════════════════

CREATE INDEX idx_inbox_status ON inbox_data(status);
CREATE INDEX idx_inbox_created ON inbox_data(created_at);
CREATE INDEX idx_inbox_entity ON inbox_data(entity_type, entity_id);
CREATE INDEX idx_inbox_priority ON inbox_data(priority);
