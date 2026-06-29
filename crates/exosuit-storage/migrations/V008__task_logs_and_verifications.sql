-- V008: Add task_logs and task_verifications tables
--
-- Supports TDD workflow: log entries and verification results per task.

CREATE TABLE task_logs (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL REFERENCES tasks_data(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL DEFAULT 'note',
    message     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE task_verifications (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL REFERENCES tasks_data(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    command     TEXT,
    result      TEXT NOT NULL,
    details     TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_task_logs_task ON task_logs(task_id);
CREATE INDEX idx_task_verifications_task ON task_verifications(task_id);
