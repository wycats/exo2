-- V009: Add notes and started_at columns to tasks_data (shadow table)
--
-- notes: free-text notes on a task (stored in implementation-plan.toml as task.notes)
-- started_at: RFC 3339 datetime when task was started (set by `exo task start`)
--
-- Only alter the shadow table (tasks_data). The virtual table (tasks) is
-- recreated from the shadow table on each connection via register_reactive_module.

ALTER TABLE tasks_data ADD COLUMN notes TEXT;
ALTER TABLE tasks_data ADD COLUMN started_at TEXT;
