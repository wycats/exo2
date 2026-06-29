-- V010: Add sort_key to epochs_data for ordering support.
-- Mirrors the sort_key pattern from V007 (phases, goals, tasks).
--
-- Unlike V007 (which added nullable sort_key), this uses NOT NULL from the start.
-- Existing rows are backfilled with zero-padded rowid values that sort correctly
-- and are compatible with FractionalIndex (any lexicographically-ordered string works).

PRAGMA foreign_keys = OFF;

CREATE TABLE epochs_data_new (
    id       INTEGER PRIMARY KEY,
    text_id  TEXT NOT NULL UNIQUE,
    title    TEXT NOT NULL,
    slug     TEXT,
    reviewed INTEGER NOT NULL DEFAULT 0,
    sort_key TEXT NOT NULL DEFAULT ''
);

-- Backfill sort_key with zero-padded rowid for existing rows (preserves insertion order)
INSERT INTO epochs_data_new (id, text_id, title, slug, reviewed, sort_key)
    SELECT id, text_id, title, slug, reviewed, printf('%08d', id)
    FROM epochs_data
    ORDER BY id;

DROP TABLE epochs_data;
ALTER TABLE epochs_data_new RENAME TO epochs_data;

CREATE INDEX idx_epochs_sort ON epochs_data(sort_key);

PRAGMA foreign_keys = ON;
