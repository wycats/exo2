-- V024__phase_completion_time.sql
-- Persist phase completion evidence so between-phase views do not infer
-- recency from roadmap order.

ALTER TABLE phases_data ADD COLUMN completed_at TEXT;

-- Older phase rows predate an explicit completion timestamp. Preserve the
-- strongest available evidence by using the latest completed child task.
UPDATE phases_data
SET completed_at = (
    SELECT task.completed_at
    FROM goals_data AS goal
    JOIN tasks_data AS task ON task.goal_id = goal.id
    WHERE goal.phase_id = phases_data.id
      AND task.completed_at IS NOT NULL
    ORDER BY unixepoch(task.completed_at) DESC, task.id DESC
    LIMIT 1
)
WHERE status = 'completed';
