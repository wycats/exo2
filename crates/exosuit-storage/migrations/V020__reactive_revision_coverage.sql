-- V020: Complete revision metadata coverage for all reactive tables.
--
-- Earlier migrations added new *_data tables after the original revision
-- schema. This migration gives every table in REACTIVE_TABLES a matching
-- *_rev table and rowset counter seed.

CREATE TABLE IF NOT EXISTS ideas_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE IF NOT EXISTS inbox_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE IF NOT EXISTS rfcs_rev (
    rowid  INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

INSERT INTO rowset_revisions (table_name, counter)
VALUES
    ('ideas_data', 0),
    ('inbox_data', 0),
    ('rfcs_data', 0)
ON CONFLICT(table_name) DO NOTHING;
