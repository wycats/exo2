-- Drop the epoch column from rowset_revisions.
-- Row-set revisions are now persistent monotonic counters (RFC 10165 §6).
-- The epoch UUID was process-scoped and reset on every restart, defeating
-- the purpose of persistent storage. The counter alone is the revision.
ALTER TABLE rowset_revisions DROP COLUMN epoch;
