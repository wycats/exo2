-- V003__revision_tables.sql
-- Create revision tracking tables for reactive virtual table layer
-- Per RFC 10165: Row revisions (content digests) and Row-Set revisions (epoch counters)

-- ═══════════════════════════════════════════════════════════
-- Row revision tables (*_rev): Store content digests per row
-- ═══════════════════════════════════════════════════════════

-- Each *_rev table stores BLAKE3 content digests keyed by rowid.
-- These are updated on INSERT/UPDATE via xUpdate callback.

CREATE TABLE epochs_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE phases_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE goals_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE tasks_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

CREATE TABLE phase_rfcs_rev (
    rowid INTEGER PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32)
);

-- ═══════════════════════════════════════════════════════════
-- Row-Set revision table: Epoch-scoped counters per table
-- ═══════════════════════════════════════════════════════════

-- Tracks membership changes (INSERT/DELETE) per table.
-- epoch: UUID generated at process start (stored as TEXT for readability)
-- counter: Monotonic counter, bumped on any row addition/removal

CREATE TABLE rowset_revisions (
    table_name TEXT PRIMARY KEY,
    epoch TEXT NOT NULL,
    counter INTEGER NOT NULL DEFAULT 0
);

-- Initialize rowset revisions for all shadow tables
-- Epoch will be updated on first access by RevisionStore
INSERT INTO rowset_revisions (table_name, epoch, counter) VALUES
    ('epochs_data', '00000000-0000-0000-0000-000000000000', 0),
    ('phases_data', '00000000-0000-0000-0000-000000000000', 0),
    ('goals_data', '00000000-0000-0000-0000-000000000000', 0),
    ('tasks_data', '00000000-0000-0000-0000-000000000000', 0),
    ('phase_rfcs_data', '00000000-0000-0000-0000-000000000000', 0);
