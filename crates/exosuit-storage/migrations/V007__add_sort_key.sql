-- V007: Add sort_key column for fractional indexing (RFC 10032)
--
-- Enables O(1) reorder operations via lexicographically sortable keys.
-- NULL sort_key falls back to rowid insertion order.

ALTER TABLE phases_data ADD COLUMN sort_key TEXT;
ALTER TABLE goals_data ADD COLUMN sort_key TEXT;
ALTER TABLE tasks_data ADD COLUMN sort_key TEXT;

-- Composite indexes for efficient ordered queries within parent scope
CREATE INDEX idx_phases_sort ON phases_data(epoch_id, sort_key);
CREATE INDEX idx_goals_sort ON goals_data(phase_id, sort_key);
CREATE INDEX idx_tasks_sort ON tasks_data(goal_id, sort_key);
