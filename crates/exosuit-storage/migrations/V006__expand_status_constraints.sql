-- V006__expand_status_constraints.sql
-- Add 'deferred' status to phases/goals/tasks
-- 
-- Semantic decisions:
-- - 'active' → normalized to 'in-progress' (same meaning)
-- - 'bankrupt' → normalized to 'abandoned' (same meaning, different scope)
-- - 'deferred' → added as new status (distinct: postponed but not abandoned)
--
-- Normalization happens in application code during import.
-- This migration only adds 'deferred' to the allowed values.

-- SQLite doesn't support ALTER TABLE to modify CHECK constraints,
-- so we need to recreate the tables.

PRAGMA foreign_keys = OFF;

-- ═══════════════════════════════════════════════════════════
-- Recreate phases_data with 'deferred' status
-- ═══════════════════════════════════════════════════════════

CREATE TABLE phases_data_new (
    id       INTEGER PRIMARY KEY,
    text_id  TEXT NOT NULL UNIQUE,
    title    TEXT NOT NULL,
    status   TEXT NOT NULL DEFAULT 'pending'
             CHECK (status IN ('pending', 'in-progress', 'completed', 'deferred', 'abandoned')),
    epoch_id INTEGER NOT NULL REFERENCES epochs_data(id) ON DELETE CASCADE,
    kind     TEXT NOT NULL DEFAULT 'regular'
             CHECK (kind IN ('regular', 'chore')),
    slug     TEXT
);

INSERT INTO phases_data_new SELECT * FROM phases_data;
DROP TABLE phases_data;
ALTER TABLE phases_data_new RENAME TO phases_data;

CREATE INDEX idx_phases_epoch ON phases_data(epoch_id);
CREATE INDEX idx_phases_status ON phases_data(status);

-- ═══════════════════════════════════════════════════════════
-- Recreate goals_data with 'deferred' status
-- ═══════════════════════════════════════════════════════════

CREATE TABLE goals_data_new (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    label          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in-progress', 'completed', 'deferred', 'abandoned')),
    phase_id       INTEGER NOT NULL REFERENCES phases_data(id) ON DELETE CASCADE,
    kind           TEXT DEFAULT 'regular'
                   CHECK (kind IN ('regular', 'strike')),
    rfc            TEXT,
    target_stage   INTEGER,
    started_at     TEXT,
    description    TEXT,
    completion_log TEXT,
    slug           TEXT
);

INSERT INTO goals_data_new SELECT * FROM goals_data;
DROP TABLE goals_data;
ALTER TABLE goals_data_new RENAME TO goals_data;

CREATE INDEX idx_goals_phase ON goals_data(phase_id);
CREATE INDEX idx_goals_status ON goals_data(status);

-- ═══════════════════════════════════════════════════════════
-- Recreate tasks_data with 'deferred' status
-- ═══════════════════════════════════════════════════════════

CREATE TABLE tasks_data_new (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in-progress', 'completed', 'deferred', 'skipped')),
    goal_id        INTEGER NOT NULL REFERENCES goals_data(id) ON DELETE CASCADE,
    completed_at   TEXT,
    completion_log TEXT,
    slug           TEXT
);

INSERT INTO tasks_data_new SELECT * FROM tasks_data;
DROP TABLE tasks_data;
ALTER TABLE tasks_data_new RENAME TO tasks_data;

CREATE INDEX idx_tasks_goal ON tasks_data(goal_id);
CREATE INDEX idx_tasks_status ON tasks_data(status);

PRAGMA foreign_keys = ON;
