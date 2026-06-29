-- V001__core_tables.sql
-- Core plan hierarchy: epoch → phase → goal → task
-- Based on RFC 10176 (Project State Model)

PRAGMA foreign_keys = ON;

-- ═══════════════════════════════════════════════════════════
-- Epochs
-- ═══════════════════════════════════════════════════════════

CREATE TABLE epochs (
    id       INTEGER PRIMARY KEY,          -- rowid alias, stable identity
    text_id  TEXT NOT NULL UNIQUE,         -- ULID or legacy id
    title    TEXT NOT NULL,
    slug     TEXT,
    reviewed INTEGER NOT NULL DEFAULT 0    -- boolean: has epoch been reviewed?
);

-- ═══════════════════════════════════════════════════════════
-- Phases
-- ═══════════════════════════════════════════════════════════

CREATE TABLE phases (
    id       INTEGER PRIMARY KEY,
    text_id  TEXT NOT NULL UNIQUE,
    title    TEXT NOT NULL,
    status   TEXT NOT NULL DEFAULT 'pending'
             CHECK (status IN ('pending', 'in-progress', 'completed')),
    epoch_id INTEGER NOT NULL REFERENCES epochs(id) ON DELETE CASCADE,
    kind     TEXT NOT NULL DEFAULT 'regular'
             CHECK (kind IN ('regular', 'chore')),
    slug     TEXT
);

CREATE INDEX idx_phases_epoch ON phases(epoch_id);
CREATE INDEX idx_phases_status ON phases(status);

-- ═══════════════════════════════════════════════════════════
-- Goals
-- ═══════════════════════════════════════════════════════════

CREATE TABLE goals (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    label          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in-progress', 'completed', 'abandoned')),
    phase_id       INTEGER NOT NULL REFERENCES phases(id) ON DELETE CASCADE,
    kind           TEXT DEFAULT 'regular'
                   CHECK (kind IN ('regular', 'strike')),
    rfc            TEXT,                        -- RFC number (e.g., "00238")
    target_stage   INTEGER,                     -- target RFC stage for promotion
    started_at     TEXT,                        -- RFC 3339 datetime
    description    TEXT,
    completion_log TEXT,                        -- required for completed/abandoned
    slug           TEXT
);

CREATE INDEX idx_goals_phase ON goals(phase_id);
CREATE INDEX idx_goals_status ON goals(status);

-- ═══════════════════════════════════════════════════════════
-- Tasks
-- ═══════════════════════════════════════════════════════════

CREATE TABLE tasks (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in-progress', 'completed', 'skipped')),
    goal_id        INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    completed_at   TEXT,                        -- RFC 3339 datetime
    completion_log TEXT,
    slug           TEXT
);

CREATE INDEX idx_tasks_goal ON tasks(goal_id);
CREATE INDEX idx_tasks_status ON tasks(status);

-- ═══════════════════════════════════════════════════════════
-- Phase ↔ RFC associations
-- ═══════════════════════════════════════════════════════════

CREATE TABLE phase_rfcs (
    id       INTEGER PRIMARY KEY,
    phase_id INTEGER NOT NULL REFERENCES phases(id) ON DELETE CASCADE,
    rfc_id   TEXT NOT NULL,                    -- RFC number (text)
    target   INTEGER,                          -- target stage for promotion
    relation TEXT NOT NULL DEFAULT 'related'
             CHECK (relation IN ('driving', 'related', 'blocked')),
    UNIQUE(phase_id, rfc_id)
);

CREATE INDEX idx_phase_rfcs_phase ON phase_rfcs(phase_id);

-- ═══════════════════════════════════════════════════════════
-- Entity aliases (cross-cutting)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE entity_aliases (
    entity_type TEXT NOT NULL
                CHECK (entity_type IN ('epoch', 'phase', 'goal', 'task')),
    entity_id   INTEGER NOT NULL,              -- rowid in corresponding table
    alias       TEXT NOT NULL,
    PRIMARY KEY (entity_type, alias)
);

CREATE INDEX idx_aliases_entity ON entity_aliases(entity_type, entity_id);
