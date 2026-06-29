//! SQLite-based writer for project state mutations.
//!
//! This is the write path counterpart to `SqliteLoader` (the read path).
//! Command-level state mutations go through reactive virtual tables; storage
//! maintenance paths may still address shadow tables directly.

use crate::api::protocol::ErrorCode;
use crate::failure::ExoFailure;
use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use exosuit_storage::{
    Connection, Database, OptionalExtension, open_database, open_memory_database,
};
use fractional_index::FractionalIndex;
use std::collections::BTreeSet;
use std::path::Path;

/// Writer for project state mutations to `SQLite` database.
#[derive(Debug)]
pub struct SqliteWriter {
    db: Database,
}

/// Canonical task identity and its planning context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTaskReference {
    pub row_id: i64,
    pub task_id: String,
    pub title: String,
    pub goal_id: String,
    pub phase_id: String,
    pub phase_status: String,
}

impl SqliteWriter {
    /// Open a database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = open_database(path.as_ref())
            .with_context(|| format!("Failed to open database at {}", path.as_ref().display()))?;
        Ok(Self { db })
    }

    /// Create an in-memory database for testing.
    pub fn open_memory() -> Result<Self> {
        let db = open_memory_database().context("Failed to create in-memory database")?;
        Ok(Self { db })
    }

    /// Get a reference to the underlying database.
    pub const fn database(&self) -> &Database {
        &self.db
    }

    // ─────────────────────────────────────────────────────────────────────
    // Epochs
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new epoch. Returns the generated `text_id`.
    pub fn add_epoch(&self, title: &str, slug: Option<&str>, aliases: &[String]) -> Result<String> {
        let conn = self.db.connection();
        let text_id = ulid::Ulid::new().to_string().to_lowercase();

        // Compute sort_key for ordering (append after last epoch)
        let last_key: Option<String> = conn
            .query_row(
                "SELECT sort_key FROM epochs_data WHERE sort_key IS NOT NULL ORDER BY sort_key DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        let sort_key = match last_key {
            Some(ref k) => {
                let prev = FractionalIndex::from_string(k)
                    .map_err(|e| anyhow!("Invalid sort_key '{k}': {e}"))?;
                FractionalIndex::new_after(&prev).to_string()
            }
            None => FractionalIndex::default().to_string(),
        };

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES (?1, ?2, ?3, 0, ?4)",
            (&text_id, title, slug, &sort_key),
        )
        .context("Failed to insert epoch")?;

        if !aliases.is_empty() {
            let epoch_rowid = conn.last_insert_rowid();
            for alias in aliases {
                conn.execute(
                    "INSERT INTO entity_aliases (entity_type, entity_id, alias) VALUES ('epoch', ?1, ?2)",
                    (epoch_rowid, alias.as_str()),
                )
                .with_context(|| format!("Failed to insert alias '{alias}' for epoch"))?;
            }
        }

        Ok(text_id)
    }

    /// Remove an epoch by `text_id`. Cascades to phases/goals/tasks.
    pub fn remove_epoch(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        delete_entity_aliases(conn, "epoch", "epochs_data", text_id)?;
        let rows = conn
            .execute("DELETE FROM epochs WHERE text_id = ?", [text_id])
            .context("Failed to delete epoch")?;
        if rows == 0 {
            return Err(anyhow!("Epoch not found: {text_id}"));
        }
        Ok(())
    }

    /// Update an epoch's reviewed status.
    pub fn update_epoch_reviewed(&self, text_id: &str, reviewed: bool) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE epochs SET reviewed = ?1 WHERE text_id = ?2",
                (i64::from(reviewed), text_id),
            )
            .context("Failed to update epoch reviewed status")?;
        if rows == 0 {
            return Err(anyhow!("Epoch not found: {text_id}"));
        }
        Ok(())
    }

    /// Update an epoch's title.
    pub fn update_epoch_title(&self, text_id: &str, title: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE epochs SET title = ?1 WHERE text_id = ?2",
                (title, text_id),
            )
            .context("Failed to update epoch title")?;
        if rows == 0 {
            return Err(anyhow!("Epoch not found: {text_id}"));
        }
        Ok(())
    }

    /// Bankrupt an epoch by marking pending/in-progress phases/goals as abandoned.
    pub fn bankrupt_epoch(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let epoch_id = resolve_id(conn, "epochs_data", text_id)?;

        conn.execute(
            "UPDATE phases
             SET status = 'abandoned'
             WHERE epoch_id = ?1 AND status IN ('pending', 'in-progress')",
            [epoch_id],
        )
        .context("Failed to update phase statuses for bankrupt epoch")?;

        conn.execute(
            "UPDATE goals
             SET status = 'abandoned'
             WHERE phase_id IN (SELECT id FROM phases_data WHERE epoch_id = ?1)
             AND status IN ('pending', 'in-progress')",
            [epoch_id],
        )
        .context("Failed to update goal statuses for bankrupt epoch")?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Phases
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new phase to an epoch. Returns the generated `text_id`.
    pub fn add_phase(
        &self,
        epoch_text_id: &str,
        title: &str,
        kind: &str,
        slug: Option<&str>,
        aliases: &[String],
    ) -> Result<String> {
        let conn = self.db.connection();
        let epoch_id = resolve_id(conn, "epochs_data", epoch_text_id)?;
        let sort_key = append_sort_key(conn, "phases_data", "epoch_id", epoch_id)?;
        let text_id = ulid::Ulid::new().to_string().to_lowercase();
        conn.execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, sort_key, slug)
             VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6)",
            (&text_id, title, epoch_id, kind, &sort_key, slug),
        )
        .context("Failed to insert phase")?;

        if !aliases.is_empty() {
            let phase_rowid = conn.last_insert_rowid();
            for alias in aliases {
                conn.execute(
                    "INSERT INTO entity_aliases (entity_type, entity_id, alias) VALUES ('phase', ?1, ?2)",
                    (phase_rowid, alias.as_str()),
                )
                .with_context(|| format!("Failed to insert alias '{alias}' for phase"))?;
            }
        }

        Ok(text_id)
    }

    /// Remove a phase by `text_id`.
    pub fn remove_phase(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        delete_entity_aliases(conn, "phase", "phases_data", text_id)?;
        let rows = conn
            .execute("DELETE FROM phases WHERE text_id = ?", [text_id])
            .context("Failed to delete phase")?;
        if rows == 0 {
            return Err(anyhow!("Phase not found: {text_id}"));
        }
        Ok(())
    }

    /// Update a phase's status.
    pub fn update_phase_status(&self, text_id: &str, status: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE phases SET status = ?1 WHERE text_id = ?2",
                (status, text_id),
            )
            .context("Failed to update phase status")?;
        if rows == 0 {
            return Err(anyhow!("Phase not found: {text_id}"));
        }
        Ok(())
    }

    /// Update a phase's title.
    pub fn update_phase_title(&self, text_id: &str, title: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE phases SET title = ?1 WHERE text_id = ?2",
                (title, text_id),
            )
            .context("Failed to update phase title")?;
        if rows == 0 {
            return Err(anyhow!("Phase not found: {text_id}"));
        }
        Ok(())
    }

    /// Replace RFC associations for a phase.
    pub fn replace_phase_rfcs(&self, text_id: &str, rfcs: &[String]) -> Result<()> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", text_id)?;

        conn.execute("DELETE FROM phase_rfcs WHERE phase_id = ?", [phase_id])
            .context("Failed to clear phase RFCs")?;

        for rfc_id in rfcs {
            conn.execute(
                "INSERT INTO phase_rfcs (phase_id, rfc_id, relation)
                 VALUES (?1, ?2, 'related')",
                (phase_id, rfc_id),
            )
            .with_context(|| format!("Failed to insert phase RFC '{rfc_id}'"))?;
        }

        Ok(())
    }

    /// Pin a workspace root to a phase.
    pub fn set_workspace_active_phase(
        &self,
        workspace_root: &str,
        phase_text_id: &str,
    ) -> Result<()> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        let now = Utc::now().to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE workspace_active_phase
                 SET phase_id = ?2, updated_at = ?3
                 WHERE workspace_root = ?1",
                (workspace_root, phase_id, &now),
            )
            .context("Failed to update workspace active phase")?;

        if rows == 0 {
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO workspace_active_phase (workspace_root, phase_id, updated_at)
                 SELECT ?1, ?2, ?3
                 WHERE NOT EXISTS (
                     SELECT 1 FROM workspace_active_phase_data WHERE workspace_root = ?1
                 )",
                    (workspace_root, phase_id, &now),
                )
                .context("Failed to insert workspace active phase")?;

            if inserted == 0 {
                let rows = conn
                    .execute(
                        "UPDATE workspace_active_phase
                         SET phase_id = ?2, updated_at = ?3
                         WHERE workspace_root = ?1",
                        (workspace_root, phase_id, &now),
                    )
                    .context("Failed to update workspace active phase after concurrent insert")?;
                if rows == 0 {
                    return Err(anyhow!(
                        "Workspace active phase could not be set for {workspace_root}"
                    ));
                }
            }
        }

        Ok(())
    }

    /// Clear the phase pin for a workspace root.
    pub fn clear_workspace_active_phase(&self, workspace_root: &str) -> Result<()> {
        let conn = self.db.connection();
        conn.execute(
            "DELETE FROM workspace_active_phase WHERE workspace_root = ?1",
            [workspace_root],
        )
        .context("Failed to clear workspace active phase")?;

        Ok(())
    }

    /// Claim ownership of a phase for a workspace, branch, or future PR owner.
    pub fn set_phase_owner(
        &self,
        phase_text_id: &str,
        owner_kind: &str,
        owner_id: &str,
        claimed_by_workspace_id: Option<&str>,
        claimed_by_workspace_root: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        let now = Utc::now().to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE phase_ownership
                 SET owner_kind = ?2,
                     owner_id = ?3,
                     claimed_by_workspace_id = ?4,
                     claimed_by_workspace_root = ?5,
                     claimed_at = CASE
                        WHEN owner_kind != ?2 OR owner_id != ?3 THEN ?6
                        ELSE claimed_at
                     END,
                     updated_at = ?6
                 WHERE phase_id = ?1",
                (
                    phase_id,
                    owner_kind,
                    owner_id,
                    claimed_by_workspace_id,
                    claimed_by_workspace_root,
                    &now,
                ),
            )
            .context("Failed to update phase owner")?;

        if rows == 0 {
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO phase_ownership
                    (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?6
                 WHERE NOT EXISTS (
                     SELECT 1 FROM phase_ownership_data WHERE phase_id = ?1
                 )",
                    (
                        phase_id,
                        owner_kind,
                        owner_id,
                        claimed_by_workspace_id,
                        claimed_by_workspace_root,
                        &now,
                    ),
                )
                .context("Failed to insert phase owner")?;

            if inserted == 0 {
                let rows = conn
                    .execute(
                        "UPDATE phase_ownership
                         SET owner_kind = ?2,
                             owner_id = ?3,
                             claimed_by_workspace_id = ?4,
                             claimed_by_workspace_root = ?5,
                             claimed_at = CASE
                                WHEN owner_kind != ?2 OR owner_id != ?3 THEN ?6
                                ELSE claimed_at
                             END,
                             updated_at = ?6
                         WHERE phase_id = ?1",
                        (
                            phase_id,
                            owner_kind,
                            owner_id,
                            claimed_by_workspace_id,
                            claimed_by_workspace_root,
                            &now,
                        ),
                    )
                    .context("Failed to update phase owner after concurrent insert")?;
                if rows == 0 {
                    return Err(anyhow!("Phase owner could not be set for {phase_text_id}"));
                }
            }
        }

        Ok(())
    }

    /// Claim ownership only if the phase owner still matches the expected owner.
    pub fn claim_phase_owner_if_current(
        &self,
        phase_text_id: &str,
        owner_kind: &str,
        owner_id: &str,
        claimed_by_workspace_id: Option<&str>,
        claimed_by_workspace_root: Option<&str>,
        expected_owner: Option<(&str, &str)>,
    ) -> Result<bool> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        let now = Utc::now().to_rfc3339();

        if let Some((expected_kind, expected_id)) = expected_owner {
            let rows = conn
                .execute(
                    "UPDATE phase_ownership
                 SET owner_kind = ?2,
                     owner_id = ?3,
                     claimed_by_workspace_id = ?4,
                     claimed_by_workspace_root = ?5,
                     claimed_at = CASE
                        WHEN owner_kind != ?2 OR owner_id != ?3 THEN ?6
                        ELSE claimed_at
                     END,
                     updated_at = ?6
                 WHERE phase_id = ?1
                   AND owner_kind = ?7
                   AND owner_id = ?8",
                    (
                        phase_id,
                        owner_kind,
                        owner_id,
                        claimed_by_workspace_id,
                        claimed_by_workspace_root,
                        &now,
                        expected_kind,
                        expected_id,
                    ),
                )
                .context("Failed to conditionally claim phase owner")?;
            return Ok(rows > 0);
        } else {
            let rows = conn
                .execute(
                    "INSERT OR IGNORE INTO phase_ownership
                        (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
                     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?6
                     WHERE NOT EXISTS (
                         SELECT 1 FROM phase_ownership_data WHERE phase_id = ?1
                     )",
                    (
                        phase_id,
                        owner_kind,
                    owner_id,
                    claimed_by_workspace_id,
                    claimed_by_workspace_root,
                    &now,
                ),
                )
                .context("Failed to conditionally claim phase owner")?;
            return Ok(rows > 0);
        }
    }

    /// Clear the ownership claim for a phase.
    pub fn clear_phase_owner(&self, phase_text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        conn.execute(
            "DELETE FROM phase_ownership WHERE phase_id = ?1",
            [phase_id],
        )
        .context("Failed to clear phase owner")?;

        Ok(())
    }

    /// Clear the ownership claim only if it still matches the inspected owner.
    pub fn clear_phase_owner_if_current(
        &self,
        phase_text_id: &str,
        owner_kind: &str,
        owner_id: &str,
    ) -> Result<bool> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        let rows = conn
            .execute(
                "DELETE FROM phase_ownership
                 WHERE phase_id = ?1 AND owner_kind = ?2 AND owner_id = ?3",
                (phase_id, owner_kind, owner_id),
            )
            .context("Failed to conditionally clear phase owner")?;

        Ok(rows > 0)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Goals
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new goal to a phase. Uses the provided `goal_id` as `text_id`.
    ///
    /// All optional fields match the `Goal` struct in context.rs.
    #[allow(clippy::too_many_arguments)]
    pub fn add_goal(
        &self,
        phase_text_id: &str,
        goal_id: &str,
        label: &str,
        rfc: Option<&str>,
        target_stage: Option<u8>,
        kind: Option<&str>,
        description: Option<&str>,
        started_at: Option<&str>,
        slug: Option<&str>,
        aliases: &[String],
    ) -> Result<String> {
        let conn = self.db.connection();
        let phase_id = resolve_id(conn, "phases_data", phase_text_id)?;
        let sort_key = append_sort_key(conn, "goals_data", "phase_id", phase_id)?;
        conn.execute(
            "INSERT INTO goals
             (text_id, label, status, phase_id, sort_key, rfc, target_stage, kind, description, started_at, slug)
             VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                goal_id,
                label,
                phase_id,
                &sort_key,
                rfc,
                target_stage.map(i32::from),
                kind.unwrap_or("regular"),
                description,
                started_at,
                slug,
            ),
        )
        .context("Failed to insert goal")?;

        // Insert aliases
        if !aliases.is_empty() {
            let goal_rowid = conn.last_insert_rowid();
            for alias in aliases {
                conn.execute(
                    "INSERT INTO entity_aliases (entity_type, entity_id, alias) VALUES ('goal', ?1, ?2)",
                    (goal_rowid, alias.as_str()),
                )
                .with_context(|| format!("Failed to insert alias '{alias}' for goal '{goal_id}'"))?;
            }
        }

        Ok(goal_id.to_string())
    }

    /// Add a new strike goal to a phase.
    ///
    /// Strike goals start as `in-progress` with `kind = "strike"`.
    pub fn add_strike_goal(
        &self,
        phase_text_id: &str,
        goal_id: &str,
        label: &str,
        description: &str,
    ) -> Result<String> {
        let now = Utc::now().to_rfc3339();
        let id = self.add_goal(
            phase_text_id,
            goal_id,
            label,
            None,
            None,
            Some("strike"),
            Some(description),
            Some(&now),
            None,
            &[],
        )?;
        // Strike goals start in-progress, not pending
        self.update_goal_status(goal_id, "in-progress")?;
        Ok(id)
    }

    /// Remove a goal by `text_id`.
    pub fn remove_goal(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        delete_entity_aliases(conn, "goal", "goals_data", text_id)?;
        let rows = conn
            .execute("DELETE FROM goals WHERE text_id = ?", [text_id])
            .context("Failed to delete goal")?;
        if rows == 0 {
            return Err(anyhow!("Goal not found: {text_id}"));
        }
        Ok(())
    }

    /// Update a goal's status.
    pub fn update_goal_status(&self, text_id: &str, status: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE goals SET status = ?1 WHERE text_id = ?2",
                (status, text_id),
            )
            .context("Failed to update goal status")?;
        if rows == 0 {
            return Err(anyhow!("Goal not found: {text_id}"));
        }
        Ok(())
    }

    /// Update a goal's label.
    pub fn update_goal_label(&self, text_id: &str, label: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE goals SET label = ?1 WHERE text_id = ?2",
                (label, text_id),
            )
            .context("Failed to update goal label")?;
        if rows == 0 {
            return Err(anyhow!("Goal not found: {text_id}"));
        }
        Ok(())
    }

    /// Set a goal's completion log.
    pub fn update_goal_completion_log(&self, text_id: &str, log: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE goals SET completion_log = ?1 WHERE text_id = ?2",
                (log, text_id),
            )
            .context("Failed to update goal completion log")?;
        if rows == 0 {
            return Err(anyhow!("Goal not found: {text_id}"));
        }
        Ok(())
    }

    /// Reorder a goal within its phase.
    ///
    /// `position` can be:
    /// - `"top"` / `"bottom"` — move to start/end
    /// - `"before:<text_id>"` / `"after:<text_id>"` — relative to a sibling
    /// - `"0"`, `"1"`, ... — 0-indexed numeric position (deprecated)
    pub fn reorder_goal(&self, text_id: &str, position: &str) -> Result<()> {
        let conn = self.db.connection();
        let phase_id: i64 = conn
            .query_row(
                "SELECT phase_id FROM goals_data WHERE text_id = ?",
                [text_id],
                |row| row.get(0),
            )
            .with_context(|| format!("Goal not found: {text_id}"))?;

        reorder_entity(
            conn,
            text_id,
            position,
            "goals_data",
            "goals",
            "phase_id",
            phase_id,
        )
    }

    /// Move a goal to a different phase.
    pub fn move_goal_to_phase(&self, goal_text_id: &str, target_phase_text_id: &str) -> Result<()> {
        self.move_goal_to_phase_position(goal_text_id, target_phase_text_id, None)
    }

    /// Move a goal to a different phase and optional target position.
    ///
    /// This preserves the goal's status, completion fields, tasks, logs, and inbox links by
    /// changing only the containing phase and sort key.
    pub fn move_goal_to_phase_position(
        &self,
        goal_text_id: &str,
        target_phase_text_id: &str,
        position: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let target_phase_id: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = ?",
                [target_phase_text_id],
                |row| row.get(0),
            )
            .with_context(|| format!("Target phase not found: {target_phase_text_id}"))?;

        move_entity_to_parent(
            conn,
            goal_text_id,
            position,
            "goals_data",
            "goals",
            "phase_id",
            target_phase_id,
            "Goal",
        )
    }

    /// Reorder a task within its goal.
    ///
    /// Same position syntax as `reorder_goal`.
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    pub fn reorder_task(&self, text_id: &str, position: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, text_id)?;
        self.reorder_task_by_row_id(task_row_id, position)
    }

    /// Reorder a task that has already been resolved to its stable row identity.
    pub fn reorder_task_by_row_id(&self, task_row_id: i64, position: &str) -> Result<()> {
        let conn = self.db.connection();
        let (text_id, goal_id): (String, i64) = conn
            .query_row(
                "SELECT text_id, goal_id FROM tasks_data WHERE id = ?1",
                [task_row_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .with_context(|| format!("Task row not found: {task_row_id}"))?;

        reorder_entity(
            conn,
            &text_id,
            position,
            "tasks_data",
            "tasks",
            "goal_id",
            goal_id,
        )
    }

    /// Reorder a phase within its epoch.
    ///
    /// Same position syntax as `reorder_goal`.
    pub fn reorder_phase(&self, text_id: &str, position: &str) -> Result<()> {
        let conn = self.db.connection();
        let epoch_id: i64 = conn
            .query_row(
                "SELECT epoch_id FROM phases_data WHERE text_id = ?",
                [text_id],
                |row| row.get(0),
            )
            .with_context(|| format!("Phase not found: {text_id}"))?;

        reorder_entity(
            conn,
            text_id,
            position,
            "phases_data",
            "phases",
            "epoch_id",
            epoch_id,
        )
    }

    /// Move a phase to a different epoch and optional target position.
    ///
    /// This preserves the phase's stable ID, status, nested goals/tasks, RFC links, and active
    /// phase pin because the phase row itself is retained.
    pub fn move_phase_to_epoch(
        &self,
        phase_text_id: &str,
        target_epoch_text_id: &str,
        position: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let target_epoch_id: i64 = conn
            .query_row(
                "SELECT id FROM epochs_data WHERE text_id = ?",
                [target_epoch_text_id],
                |row| row.get(0),
            )
            .with_context(|| format!("Target epoch not found: {target_epoch_text_id}"))?;

        move_entity_to_parent(
            conn,
            phase_text_id,
            position,
            "phases_data",
            "phases",
            "epoch_id",
            target_epoch_id,
            "Phase",
        )
    }

    /// Reorder an epoch globally.
    ///
    /// Same position syntax as `reorder_goal`.
    pub fn reorder_epoch(&self, text_id: &str, position: &str) -> Result<()> {
        let conn = self.db.connection();
        // Verify epoch exists
        conn.query_row(
            "SELECT id FROM epochs_data WHERE text_id = ?",
            [text_id],
            |row| row.get::<_, i64>(0),
        )
        .with_context(|| format!("Epoch not found: {text_id}"))?;

        // Epochs have no parent scope — load all epochs as siblings
        let mut stmt = conn
            .prepare(
                "SELECT text_id, sort_key FROM epochs_data
                 ORDER BY sort_key, id",
            )
            .context("Failed to prepare epoch sibling query")?;
        let siblings: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("Failed to query epoch siblings")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read epoch sibling rows")?;

        let others: Vec<&(String, String)> =
            siblings.iter().filter(|(id, _)| id != text_id).collect();

        let new_key = compute_position(position, &others)?;

        conn.execute(
            "UPDATE epochs SET sort_key = ?1 WHERE text_id = ?2",
            (new_key.to_string(), text_id),
        )
        .with_context(|| format!("Failed to update sort_key for epoch {text_id}"))?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Tasks
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new task to a goal. Returns the `text_id`.
    pub fn add_task(
        &self,
        goal_text_id: &str,
        task_id: &str,
        title: &str,
        notes: Option<&str>,
    ) -> Result<String> {
        let conn = self.db.connection();
        let goal_id = resolve_id(conn, "goals_data", goal_text_id)?;
        if task_handle_conflicts(conn, goal_id, task_id, None)? {
            return Err(invalid_task_handle(format!(
                "Task handle '{task_id}' conflicts with an existing canonical, alias, or goal-qualified task reference."
            )));
        }
        let sort_key = append_sort_key(conn, "tasks_data", "goal_id", goal_id)?;
        conn.execute(
            "INSERT INTO tasks (text_id, title, status, goal_id, sort_key, notes)
             VALUES (?1, ?2, 'pending', ?3, ?4, ?5)",
            (task_id, title, goal_id, &sort_key, notes),
        )
        .context("Failed to insert task")?;
        Ok(task_id.to_string())
    }

    /// Resolve a task reference through canonical IDs, aliases, or a
    /// goal-qualified display ID.
    pub fn resolve_task_reference(&self, reference: &str) -> Result<Option<ResolvedTaskReference>> {
        resolve_task_reference(self.db.connection(), reference)
    }

    /// Resolve a goal ID or alias to its canonical ID.
    pub fn resolve_goal_reference(&self, reference: &str) -> Result<Option<String>> {
        let conn = self.db.connection();
        resolve_goal_row_id(conn, reference)?.map_or(Ok(None), |row_id| {
            conn.query_row(
                "SELECT text_id FROM goals_data WHERE id = ?1",
                [row_id],
                |row| row.get(0),
            )
            .map(Some)
            .context("Failed to read canonical goal ID")
        })
    }

    /// Return whether a canonical task handle or any goal-qualified form is
    /// owned by a row other than the supplied task.
    pub fn task_handle_conflicts_for_goal(
        &self,
        goal_reference: &str,
        handle: &str,
        task_row_id: i64,
    ) -> Result<bool> {
        let conn = self.db.connection();
        let goal_row_id = resolve_goal_row_id(conn, goal_reference)?
            .ok_or_else(|| anyhow!("Goal not found: {goal_reference}"))?;
        task_handle_conflicts(conn, goal_row_id, handle, Some(task_row_id))
    }

    /// Rename a task after command-level validation has resolved the canonical
    /// task and checked the new handle for conflicts.
    pub fn rename_task_handle(
        &self,
        task_row_id: i64,
        old_task_id: &str,
        new_task_id: &str,
    ) -> Result<()> {
        let conn = self.db.connection();
        let (goal_row_id, canonical_goal_id): (i64, String) = conn
            .query_row(
                "SELECT g.id, g.text_id
                 FROM tasks_data t
                 JOIN goals_data g ON t.goal_id = g.id
                 WHERE t.id = ?1",
                [task_row_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("Failed to read task goal before rename")?;
        let mut task_handles = entity_aliases_for_row(conn, "task", task_row_id)?;
        task_handles.push(old_task_id.to_string());
        let mut goal_handles = entity_aliases_for_row(conn, "goal", goal_row_id)?;
        goal_handles.push(canonical_goal_id);
        let mut legacy_references = BTreeSet::new();
        for task_handle in &task_handles {
            legacy_references.insert(task_handle.clone());
            for goal_handle in &goal_handles {
                legacy_references.insert(format!("{goal_handle}::{task_handle}"));
            }
        }
        let mut exclusively_owned_references = BTreeSet::new();
        for legacy_reference in legacy_references {
            let owners = task_handle_owners(conn, &legacy_reference)?;
            if !owners.is_empty() && owners.iter().all(|owner| *owner == task_row_id) {
                exclusively_owned_references.insert(legacy_reference);
            }
        }
        let tx = conn
            .unchecked_transaction()
            .context("Failed to start task rename transaction")?;

        tx.execute(
            "DELETE FROM entity_aliases
             WHERE entity_type = 'task' AND entity_id = ?1 AND alias = ?2",
            (task_row_id, new_task_id),
        )
        .with_context(|| format!("Failed to promote task alias '{new_task_id}'"))?;

        let renamed = tx
            .execute(
                "UPDATE tasks SET text_id = ?1 WHERE id = ?2 AND text_id = ?3",
                (new_task_id, task_row_id, old_task_id),
            )
            .with_context(|| format!("Failed to rename task '{old_task_id}'"))?;
        if renamed != 1 {
            return Err(anyhow!(
                "Task '{old_task_id}' changed before its handle could be renamed"
            ));
        }

        for legacy_reference in &exclusively_owned_references {
            tx.execute(
                "UPDATE inbox SET entity_id = ?1
                 WHERE entity_type = 'task' AND entity_id = ?2",
                (new_task_id, legacy_reference),
            )
            .with_context(|| {
                format!("Failed to update task inbox reference '{legacy_reference}'")
            })?;

            tx.execute(
                "UPDATE agent_events SET entity_id = ?1
                 WHERE entity_type = 'task' AND entity_id = ?2",
                (new_task_id, legacy_reference),
            )
            .with_context(|| {
                format!("Failed to update task event reference '{legacy_reference}'")
            })?;
        }

        tx.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
             VALUES ('task', ?1, ?2)",
            (task_row_id, old_task_id),
        )
        .with_context(|| format!("Failed to preserve task alias '{old_task_id}'"))?;

        tx.commit().context("Failed to commit task rename")?;
        Ok(())
    }

    /// Remove a task by `text_id`.
    ///
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    pub fn remove_task(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, text_id)?;
        self.remove_task_by_row_id(task_row_id)
    }

    /// Remove a task that has already been resolved to its stable row identity.
    pub fn remove_task_by_row_id(&self, task_row_id: i64) -> Result<()> {
        let conn = self.db.connection();
        delete_aliases_for_rowid(conn, "task", task_row_id)?;
        let rows = conn
            .execute("DELETE FROM tasks WHERE id = ?1", [task_row_id])
            .context("Failed to delete task")?;
        if rows == 0 {
            return Err(anyhow!("Task row not found: {task_row_id}"));
        }
        Ok(())
    }

    /// Update a task's status. If transitioning to `in-progress`, also sets `started_at`.
    ///
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    pub fn update_task_status(&self, text_id: &str, status: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, text_id)?;
        self.update_task_status_by_row_id(task_row_id, status)
    }

    /// Update a task that has already been resolved to its stable row identity.
    pub fn update_task_status_by_row_id(&self, task_row_id: i64, status: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = if status == "in-progress" {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE tasks SET status = ?1, started_at = COALESCE(started_at, ?2) WHERE id = ?3",
                (status, &now, task_row_id),
            )
            .context("Failed to update task status")?
        } else {
            conn.execute(
                "UPDATE tasks SET status = ?1 WHERE id = ?2",
                (status, task_row_id),
            )
            .context("Failed to update task status")?
        };
        if rows == 0 {
            return Err(anyhow!("Task row not found: {task_row_id}"));
        }
        Ok(())
    }

    /// Update a task's title.
    ///
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    pub fn update_task_title(&self, text_id: &str, title: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, text_id)?;
        self.update_task_title_by_row_id(task_row_id, title)
    }

    /// Update the title of a task that has already been resolved.
    pub fn update_task_title_by_row_id(&self, task_row_id: i64, title: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE tasks SET title = ?1 WHERE id = ?2",
                (title, task_row_id),
            )
            .context("Failed to update task title")?;
        if rows == 0 {
            return Err(anyhow!("Task row not found: {task_row_id}"));
        }
        Ok(())
    }

    /// Complete a task with a log message.
    ///
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    /// For composite IDs, resolves the task within the specified goal.
    pub fn complete_task(&self, text_id: &str, log: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, text_id)?;
        self.complete_task_by_row_id(task_row_id, log)
    }

    /// Complete a task that has already been resolved to its stable row identity.
    pub fn complete_task_by_row_id(&self, task_row_id: i64, log: &str) -> Result<()> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE tasks SET status = 'completed', completion_log = ?1, completed_at = ?2
                 WHERE id = ?3",
                (log, &now, task_row_id),
            )
            .context("Failed to complete task")?;
        if rows == 0 {
            return Err(anyhow!("Task row not found: {task_row_id}"));
        }
        Ok(())
    }

    /// Add a log entry to a task.
    ///
    /// Accepts both plain task IDs (`"task-1"`) and composite IDs (`"goal-1::task-1"`).
    pub fn add_task_log(&self, task_text_id: &str, kind: &str, message: &str) -> Result<()> {
        let conn = self.db.connection();
        let task_row_id = resolve_task_row_id(conn, task_text_id)?;
        self.add_task_log_by_row_id(task_row_id, kind, message)
    }

    /// Add a log entry to a task that has already been resolved.
    pub fn add_task_log_by_row_id(
        &self,
        task_row_id: i64,
        kind: &str,
        message: &str,
    ) -> Result<()> {
        let conn = self.db.connection();
        conn.execute(
            "INSERT INTO task_logs (task_id, kind, message) VALUES (?1, ?2, ?3)",
            (task_row_id, kind, message),
        )
        .context("Failed to insert task log")?;
        Ok(())
    }

    /// Add a verification result to a task.
    pub fn add_task_verification(
        &self,
        task_text_id: &str,
        kind: &str,
        command: Option<&str>,
        result: &str,
        details: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let task_id = resolve_task_row_id(conn, task_text_id)?;
        conn.execute(
            "INSERT INTO task_verifications (task_id, kind, command, result, details)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (task_id, kind, command, result, details),
        )
        .context("Failed to insert task verification")?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Ideas
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new idea. Returns the generated `text_id` (UUID v4).
    pub fn add_idea(
        &self,
        title: &str,
        description: Option<&str>,
        tags: &[String],
    ) -> Result<String> {
        let conn = self.db.connection();
        let text_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO ideas (text_id, title, description, status, created_at, source)
             VALUES (?1, ?2, ?3, 'new', ?4, 'user')",
            (&text_id, title, description, &now),
        )
        .context("Failed to insert idea")?;

        let idea_id = conn.last_insert_rowid();
        for tag in tags {
            conn.execute(
                "INSERT INTO idea_tags (idea_id, tag) VALUES (?1, ?2)",
                (idea_id, tag.as_str()),
            )
            .with_context(|| format!("Failed to insert tag '{tag}'"))?;
        }

        Ok(text_id)
    }

    /// Add an axiom to the database.
    #[allow(clippy::too_many_arguments)]
    pub fn add_axiom(
        &self,
        text_id: &str,
        scope: &str,
        principle: &str,
        rationale: Option<&str>,
        notes: Option<&str>,
        implications: &[String],
        tags: &[String],
    ) -> Result<()> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO axioms (text_id, scope, principle, rationale, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (text_id, scope, principle, rationale, notes, &now),
        )
        .with_context(|| format!("Failed to insert axiom '{text_id}'"))?;

        let axiom_id = conn.last_insert_rowid();
        for (i, imp) in implications.iter().enumerate() {
            conn.execute(
                "INSERT INTO axiom_implications (axiom_id, implication, sort_key) VALUES (?1, ?2, ?3)",
                (axiom_id, imp.as_str(), i64::try_from(i).unwrap_or(0)),
            )
            .with_context(|| format!("Failed to insert implication for axiom '{text_id}'"))?;
        }
        for tag in tags {
            conn.execute(
                "INSERT INTO axiom_tags (axiom_id, tag) VALUES (?1, ?2)",
                (axiom_id, tag.as_str()),
            )
            .with_context(|| format!("Failed to insert tag '{tag}' for axiom '{text_id}'"))?;
        }

        Ok(())
    }

    /// Remove an axiom by `text_id`.
    pub fn remove_axiom(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute("DELETE FROM axioms WHERE text_id = ?", [text_id])
            .context("Failed to remove axiom")?;
        if rows == 0 {
            return Err(anyhow!("Axiom not found: {text_id}"));
        }
        Ok(())
    }

    /// Archive an idea.
    pub fn archive_idea(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "UPDATE ideas SET status = 'archived' WHERE text_id = ?",
                [text_id],
            )
            .context("Failed to archive idea")?;
        if rows == 0 {
            return Err(anyhow!("Idea not found: {text_id}"));
        }
        Ok(())
    }

    /// Add a task reference to an idea (e.g., "rfc:0042").
    pub fn add_idea_task_ref(&self, text_id: &str, task_ref: &str) -> Result<()> {
        let conn = self.db.connection();

        let mut stmt = conn
            .prepare("SELECT id FROM ideas_data WHERE text_id = ?")
            .context("Failed to prepare idea query")?;

        let ids: Vec<i64> = stmt
            .query_map([text_id], |row| row.get(0))
            .context("Failed to query idea")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read idea id")?;

        let idea_id = ids
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Idea not found: {text_id}"))?;

        conn.execute(
            "INSERT INTO idea_task_refs (idea_id, task_ref) VALUES (?1, ?2)",
            (idea_id, task_ref),
        )
        .context("Failed to insert idea task ref")?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Inbox
    // ─────────────────────────────────────────────────────────────────────

    /// Add a new inbox item. Returns the generated `text_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_inbox_item(
        &self,
        entity_type: &str,
        entity_id: Option<&str>,
        source: &str,
        intent: &str,
        priority: &str,
        confidence: Option<&str>,
        agent_id: Option<&str>,
        subject: &str,
        body: &str,
        action_json: Option<&str>,
    ) -> Result<String> {
        let conn = self.db.connection();
        let text_id = format!("intent-{}", ulid::Ulid::new().to_string().to_lowercase());
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO inbox
             (text_id, status, entity_type, entity_id, source, intent, priority, confidence,
              agent_id, subject, body, action_json, created_at, updated_at)
             VALUES (?1, 'pending', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            (
                &text_id,
                entity_type,
                entity_id,
                source,
                intent,
                priority,
                confidence,
                agent_id,
                subject,
                body,
                action_json,
                &now,
            ),
        )
        .context("Failed to insert inbox item")?;
        Ok(text_id)
    }

    /// Update an inbox item's status.
    pub fn update_inbox_status(
        &self,
        text_id: &str,
        status: &str,
        resolution: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE inbox SET status = ?1, resolution = ?2, updated_at = ?3
                 WHERE text_id = ?4",
                (status, resolution, &now, text_id),
            )
            .context("Failed to update inbox status")?;
        if rows == 0 {
            return Err(anyhow!("Inbox item not found: {text_id}"));
        }
        Ok(())
    }

    /// Archive all resolved inbox items. Returns count archived.
    pub fn archive_resolved_inbox(&self) -> Result<u64> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE inbox SET status = 'archived', updated_at = ?1
                 WHERE status = 'resolved'",
                [&now],
            )
            .context("Failed to archive resolved inbox items")?;
        Ok(rows as u64)
    }

    /// Delete archived inbox items older than `days`. Returns count deleted.
    pub fn gc_old_archived_inbox(&self, days: u32) -> Result<u64> {
        let conn = self.db.connection();
        let rows = conn
            .execute(
                "DELETE FROM inbox
                 WHERE status = 'archived'
                 AND updated_at < datetime('now', ?1)",
                [format!("-{days} days")],
            )
            .context("Failed to gc archived inbox items")?;
        Ok(rows as u64)
    }
    // ═══════════════════════════════════════════════════════════════════════
    // RFC metadata
    // ═══════════════════════════════════════════════════════════════════════

    /// Upsert an RFC metadata record. Inserts if new, updates if `text_id` exists.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_rfc(
        &self,
        text_id: &str,
        rfc_number: i64,
        title: &str,
        stage: u8,
        status: &str,
        feature: Option<&str>,
        slug: &str,
        file_path: &str,
        superseded_by: Option<&str>,
        supersedes: Option<&str>,
        withdrawal_reason: Option<&str>,
        archived_reason: Option<&str>,
        consolidated_into: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE rfcs
                 SET rfc_number = ?2,
                     title = ?3,
                     stage = ?4,
                     status = ?5,
                     feature = ?6,
                     slug = ?7,
                     file_path = ?8,
                     superseded_by = ?9,
                     supersedes = ?10,
                     withdrawal_reason = ?11,
                     archived_reason = ?12,
                     consolidated_into = ?13,
                     updated_at = ?14
                 WHERE text_id = ?1",
                (
                    text_id,
                    rfc_number,
                    title,
                    stage,
                    status,
                    feature,
                    slug,
                    file_path,
                    superseded_by,
                    supersedes,
                    withdrawal_reason,
                    archived_reason,
                    consolidated_into,
                    &now,
                ),
            )
            .context("Failed to update RFC metadata")?;

        if rows == 0 {
            let inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO rfcs (
                    text_id, rfc_number, title, stage, status, feature, slug, file_path,
                    superseded_by, supersedes, withdrawal_reason, archived_reason,
                    consolidated_into, created_at, updated_at
                )
                SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14
                WHERE NOT EXISTS (
                    SELECT 1 FROM rfcs_data WHERE text_id = ?1
                )",
                    (
                        text_id,
                        rfc_number,
                        title,
                        stage,
                        status,
                        feature,
                        slug,
                        file_path,
                        superseded_by,
                        supersedes,
                        withdrawal_reason,
                        archived_reason,
                        consolidated_into,
                        &now,
                    ),
                )
                .context("Failed to insert RFC metadata")?;

            if inserted == 0 {
                let rows = conn
                    .execute(
                        "UPDATE rfcs
                         SET rfc_number = ?2,
                             title = ?3,
                             stage = ?4,
                             status = ?5,
                             feature = ?6,
                             slug = ?7,
                             file_path = ?8,
                             superseded_by = ?9,
                             supersedes = ?10,
                             withdrawal_reason = ?11,
                             archived_reason = ?12,
                             consolidated_into = ?13,
                             updated_at = ?14
                         WHERE text_id = ?1",
                        (
                            text_id,
                            rfc_number,
                            title,
                            stage,
                            status,
                            feature,
                            slug,
                            file_path,
                            superseded_by,
                            supersedes,
                            withdrawal_reason,
                            archived_reason,
                            consolidated_into,
                            &now,
                        ),
                    )
                    .context("Failed to update RFC metadata after concurrent insert")?;
                if rows == 0 {
                    return Err(anyhow!("RFC metadata could not be upserted for {text_id}"));
                }
            }
        }

        Ok(())
    }

    /// Update just the stage and `file_path` for an RFC (used by rfc promote).
    pub fn update_rfc_stage(&self, text_id: &str, stage: u8, file_path: &str) -> Result<()> {
        let conn = self.db.connection();
        let now = Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE rfcs SET stage = ?1, file_path = ?2, updated_at = ?3
                 WHERE text_id = ?4",
                (stage, file_path, &now, text_id),
            )
            .context("Failed to update RFC stage")?;
        if rows == 0 {
            return Err(anyhow::anyhow!("RFC not found: {text_id}"));
        }
        Ok(())
    }

    /// Delete an RFC metadata row by `text_id`.
    pub fn delete_rfc(&self, text_id: &str) -> Result<()> {
        let conn = self.db.connection();
        let rows = conn
            .execute("DELETE FROM rfcs WHERE text_id = ?1", [text_id])
            .context("Failed to delete RFC metadata")?;
        if rows == 0 {
            return Err(anyhow!("RFC not found: {text_id}"));
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────

/// Resolve a `text_id` to an integer rowid.
fn resolve_id(conn: &Connection, table: &str, text_id: &str) -> Result<i64> {
    conn.query_row(
        &format!("SELECT id FROM {table} WHERE text_id = ?"),
        [text_id],
        |row| row.get(0),
    )
    .with_context(|| format!("Entity not found in {table}: {text_id}"))
}

/// Delete `entity_aliases` rows for the entity identified by `text_id` in
/// `table`, **and for every descendant** that the entity's `ON DELETE CASCADE`
/// will remove, before that entity row is deleted.
///
/// `entity_aliases.entity_id` references the entity's rowid (not its `text_id`)
/// and has no foreign-key cascade, so the database's `ON DELETE CASCADE` on the
/// entity tables removes the descendant rows but leaves their alias rows behind.
/// Orphaned aliases break the git-friendly SQL projection (`dump_tables` cannot
/// resolve their rowid to a `text_id`). See RFC 10189.
///
/// The cascade hierarchy is epoch → phases → goals → tasks. Cleaning an epoch
/// must therefore also clean the aliases of every phase, goal, and task beneath
/// it; cleaning a phase must also clean its goals and tasks; and so on.
///
/// If the entity row does not exist (so its rowid cannot be resolved) this is a
/// no-op. Any other SQLite failure is propagated rather than treated as "not
/// found", so real errors (I/O, corruption) are not hidden behind silent alias
/// retention.
fn delete_entity_aliases(
    conn: &Connection,
    entity_type: &str,
    table: &str,
    text_id: &str,
) -> Result<()> {
    let rowid = match conn.query_row(
        &format!("SELECT id FROM {table} WHERE text_id = ?"),
        [text_id],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(rowid) => rowid,
        // Entity row doesn't exist — nothing to clean up.
        Err(exosuit_storage::rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to resolve {entity_type} rowid for alias cleanup: {text_id}")
            });
        }
    };

    // Delete aliases for the entity itself, then for each descendant level that
    // the database cascade will delete. Each query selects descendant rowids via
    // the same parent chain the FK cascade follows.
    delete_aliases_for_rowid(conn, entity_type, rowid)?;
    match entity_type {
        "epoch" => {
            delete_descendant_aliases(
                conn,
                "phase",
                "SELECT id FROM phases_data WHERE epoch_id = ?1",
                rowid,
            )?;
            delete_descendant_aliases(
                conn,
                "goal",
                "SELECT g.id FROM goals_data g
                 JOIN phases_data p ON g.phase_id = p.id
                 WHERE p.epoch_id = ?1",
                rowid,
            )?;
            delete_descendant_aliases(
                conn,
                "task",
                "SELECT t.id FROM tasks_data t
                 JOIN goals_data g ON t.goal_id = g.id
                 JOIN phases_data p ON g.phase_id = p.id
                 WHERE p.epoch_id = ?1",
                rowid,
            )?;
        }
        "phase" => {
            delete_descendant_aliases(
                conn,
                "goal",
                "SELECT id FROM goals_data WHERE phase_id = ?1",
                rowid,
            )?;
            delete_descendant_aliases(
                conn,
                "task",
                "SELECT t.id FROM tasks_data t
                 JOIN goals_data g ON t.goal_id = g.id
                 WHERE g.phase_id = ?1",
                rowid,
            )?;
        }
        "goal" => {
            delete_descendant_aliases(
                conn,
                "task",
                "SELECT id FROM tasks_data WHERE goal_id = ?1",
                rowid,
            )?;
        }
        // Tasks have no descendants.
        _ => {}
    }
    Ok(())
}

/// Delete every `entity_aliases` row of `entity_type` whose `entity_id` is
/// returned by `descendant_query` (bound with `parent_rowid` as `?1`).
fn delete_descendant_aliases(
    conn: &Connection,
    entity_type: &str,
    descendant_query: &str,
    parent_rowid: i64,
) -> Result<()> {
    let sql = format!(
        "DELETE FROM entity_aliases
         WHERE entity_type = '{entity_type}'
           AND entity_id IN ({descendant_query})"
    );
    conn.execute(&sql, [parent_rowid])
        .with_context(|| format!("Failed to delete cascaded {entity_type} aliases"))?;
    Ok(())
}

/// Delete the `entity_aliases` rows for a single entity rowid.
fn delete_aliases_for_rowid(conn: &Connection, entity_type: &str, rowid: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM entity_aliases WHERE entity_type = ?1 AND entity_id = ?2",
        (entity_type, rowid),
    )
    .with_context(|| format!("Failed to delete aliases for {entity_type} rowid {rowid}"))?;
    Ok(())
}

fn resolve_task_row_id(conn: &Connection, text_id: &str) -> Result<i64> {
    resolve_task_reference(conn, text_id)?
        .map(|task| task.row_id)
        .ok_or_else(|| anyhow!("Task not found: {text_id}"))
}

fn resolve_task_reference(
    conn: &Connection,
    reference: &str,
) -> Result<Option<ResolvedTaskReference>> {
    if let Some((goal_reference, task_reference)) = reference.split_once("::")
        && let Some(goal_row_id) = resolve_goal_row_id(conn, goal_reference)?
    {
        if let Some(task) = query_task_reference(conn, task_reference, Some(goal_row_id), false)? {
            return Ok(Some(task));
        }
        if let Some(task) = query_task_reference(conn, task_reference, Some(goal_row_id), true)? {
            return Ok(Some(task));
        }
    }

    if let Some(task) = query_task_reference(conn, reference, None, false)? {
        return Ok(Some(task));
    }
    query_task_reference(conn, reference, None, true)
}

fn query_task_reference(
    conn: &Connection,
    reference: &str,
    goal_row_id: Option<i64>,
    alias: bool,
) -> Result<Option<ResolvedTaskReference>> {
    let (join, predicate) = if alias {
        (
            "JOIN entity_aliases a ON a.entity_type = 'task' AND a.entity_id = t.id",
            "a.alias = ?1",
        )
    } else {
        ("", "t.text_id = ?1")
    };
    let goal_predicate = if goal_row_id.is_some() {
        " AND g.id = ?2"
    } else {
        ""
    };
    let sql = format!(
        "SELECT t.id, t.text_id, t.title, g.text_id, p.text_id, p.status
         FROM tasks_data t
         JOIN goals_data g ON t.goal_id = g.id
         JOIN phases_data p ON g.phase_id = p.id
         {join}
         WHERE {predicate}{goal_predicate}"
    );
    let read = |row: &exosuit_storage::Row<'_>| {
        Ok(ResolvedTaskReference {
            row_id: row.get(0)?,
            task_id: row.get(1)?,
            title: row.get(2)?,
            goal_id: row.get(3)?,
            phase_id: row.get(4)?,
            phase_status: row.get(5)?,
        })
    };

    match goal_row_id {
        Some(goal_row_id) => conn
            .query_row(&sql, (reference, goal_row_id), read)
            .optional()
            .context("Failed to resolve qualified task reference"),
        None => conn
            .query_row(&sql, [reference], read)
            .optional()
            .context("Failed to resolve task reference"),
    }
}

fn resolve_goal_row_id(conn: &Connection, reference: &str) -> Result<Option<i64>> {
    if let Some(row_id) = conn
        .query_row(
            "SELECT id FROM goals_data WHERE text_id = ?1",
            [reference],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to resolve canonical goal reference")?
    {
        return Ok(Some(row_id));
    }

    conn.query_row(
        "SELECT entity_id FROM entity_aliases
         WHERE entity_type = 'goal' AND alias = ?1",
        [reference],
        |row| row.get(0),
    )
    .optional()
    .context("Failed to resolve goal alias")
}

fn task_handle_conflicts(
    conn: &Connection,
    goal_row_id: i64,
    handle: &str,
    allowed_owner: Option<i64>,
) -> Result<bool> {
    let mut references = BTreeSet::from([handle.to_string()]);
    for goal_handle in goal_reference_handles(conn, goal_row_id)? {
        references.insert(format!("{goal_handle}::{handle}"));
    }

    for reference in references {
        if task_handle_owners(conn, &reference)?
            .into_iter()
            .any(|owner| Some(owner) != allowed_owner)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn task_handle_owners(conn: &Connection, handle: &str) -> Result<Vec<i64>> {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM tasks_data WHERE text_id = ?1
             UNION
             SELECT entity_id FROM entity_aliases
             WHERE entity_type = 'task' AND alias = ?1
             UNION
             SELECT t.id
             FROM tasks_data t
             JOIN goals_data g ON t.goal_id = g.id
             WHERE g.text_id || '::' || t.text_id = ?1
             UNION
             SELECT t.id
             FROM tasks_data t
             JOIN entity_aliases goal_alias
               ON goal_alias.entity_type = 'goal' AND goal_alias.entity_id = t.goal_id
             WHERE goal_alias.alias || '::' || t.text_id = ?1
             UNION
             SELECT t.id
             FROM tasks_data t
             JOIN goals_data g ON t.goal_id = g.id
             JOIN entity_aliases task_alias
               ON task_alias.entity_type = 'task' AND task_alias.entity_id = t.id
             WHERE g.text_id || '::' || task_alias.alias = ?1
             UNION
             SELECT t.id
             FROM tasks_data t
             JOIN entity_aliases goal_alias
               ON goal_alias.entity_type = 'goal' AND goal_alias.entity_id = t.goal_id
             JOIN entity_aliases task_alias
               ON task_alias.entity_type = 'task' AND task_alias.entity_id = t.id
             WHERE goal_alias.alias || '::' || task_alias.alias = ?1",
        )
        .context("Failed to prepare task handle ownership query")?;
    stmt.query_map([handle], |row| row.get(0))
        .context("Failed to inspect task handle ownership")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read task handle owners")
}

fn invalid_task_handle(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ExoFailure::new(
        ErrorCode::InvalidInput,
        message.into(),
        ExoFailure::orienting_steering(Vec::new()),
    ))
}

fn goal_reference_handles(conn: &Connection, goal_row_id: i64) -> Result<Vec<String>> {
    let mut handles = entity_aliases_for_row(conn, "goal", goal_row_id)?;
    handles.push(
        conn.query_row(
            "SELECT text_id FROM goals_data WHERE id = ?1",
            [goal_row_id],
            |row| row.get(0),
        )
        .context("Failed to read canonical goal handle")?,
    );
    Ok(handles)
}

fn entity_aliases_for_row(
    conn: &Connection,
    entity_type: &str,
    entity_row_id: i64,
) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT alias FROM entity_aliases
             WHERE entity_type = ?1 AND entity_id = ?2",
        )
        .with_context(|| format!("Failed to prepare {entity_type} alias query"))?;
    stmt.query_map((entity_type, entity_row_id), |row| row.get(0))
        .with_context(|| format!("Failed to query {entity_type} aliases"))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to read {entity_type} aliases"))
}

/// Compute a `sort_key` for appending to a parent scope.
fn append_sort_key(
    conn: &Connection,
    table: &str,
    parent_col: &str,
    parent_id: i64,
) -> Result<String> {
    let sql = format!(
        "SELECT sort_key FROM {table} WHERE {parent_col} = ? AND sort_key IS NOT NULL
         ORDER BY sort_key DESC LIMIT 1"
    );
    let last_key: Option<String> = conn.query_row(&sql, [parent_id], |row| row.get(0)).ok();

    let key = match last_key {
        Some(ref k) => {
            let prev = FractionalIndex::from_string(k)
                .map_err(|e| anyhow!("Invalid sort_key '{k}': {e}"))?;
            FractionalIndex::new_after(&prev)
        }
        None => FractionalIndex::default(),
    };
    Ok(key.to_string())
}

/// Generic reorder operation for any ordered entity.
///
/// Supports position syntax:
/// - `"top"` / `"bottom"` — move to start/end
/// - `"before:<text_id>"` / `"after:<text_id>"` — relative to a named sibling
/// - `"0"`, `"1"`, ... — 0-indexed numeric position
fn reorder_entity(
    conn: &Connection,
    text_id: &str,
    position: &str,
    read_table: &str,
    write_table: &str,
    parent_col: &str,
    parent_id: i64,
) -> Result<()> {
    // Load all siblings with sort_keys
    let sql = format!(
        "SELECT text_id, sort_key FROM {read_table}
         WHERE {parent_col} = ? AND sort_key IS NOT NULL
         ORDER BY sort_key, id"
    );
    let mut stmt = conn
        .prepare(&sql)
        .context("Failed to prepare sibling query")?;
    let siblings: Vec<(String, String)> = stmt
        .query_map([parent_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .context("Failed to query siblings")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read sibling rows")?;

    // Filter out the item being moved
    let others: Vec<&(String, String)> = siblings.iter().filter(|(id, _)| id != text_id).collect();

    let new_key = compute_position(position, &others)?;

    conn.execute(
        &format!("UPDATE {write_table} SET sort_key = ?1 WHERE text_id = ?2"),
        (new_key.to_string(), text_id),
    )
    .with_context(|| format!("Failed to update sort_key for {text_id}"))?;

    Ok(())
}

fn move_entity_to_parent(
    conn: &Connection,
    text_id: &str,
    position: Option<&str>,
    read_table: &str,
    write_table: &str,
    parent_col: &str,
    parent_id: i64,
    label: &str,
) -> Result<()> {
    conn.query_row(
        &format!("SELECT id FROM {read_table} WHERE text_id = ?"),
        [text_id],
        |row| row.get::<_, i64>(0),
    )
    .with_context(|| format!("{label} not found: {text_id}"))?;

    let sql = format!(
        "SELECT text_id, sort_key FROM {read_table}
         WHERE {parent_col} = ? AND sort_key IS NOT NULL
         ORDER BY sort_key, id"
    );
    let mut stmt = conn
        .prepare(&sql)
        .context("Failed to prepare target sibling query")?;
    let siblings: Vec<(String, String)> = stmt
        .query_map([parent_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .context("Failed to query target siblings")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read target sibling rows")?;

    let others: Vec<&(String, String)> = siblings.iter().filter(|(id, _)| id != text_id).collect();
    let new_key = compute_position(position.unwrap_or("bottom"), &others)?;

    let rows = conn
        .execute(
            &format!(
                "UPDATE {write_table} SET {parent_col} = ?1, sort_key = ?2 WHERE text_id = ?3"
            ),
            (parent_id, new_key.to_string(), text_id),
        )
        .with_context(|| format!("Failed to move {label} {text_id}"))?;

    if rows == 0 {
        return Err(anyhow!("{label} not found: {text_id}"));
    }

    Ok(())
}

/// Compute a `FractionalIndex` for a given position among siblings.
///
/// `others` is the list of `(text_id, sort_key)` pairs for all siblings
/// *excluding* the item being positioned, in sort order.
fn compute_position(position: &str, others: &[&(String, String)]) -> Result<FractionalIndex> {
    // Handle before:/after: syntax
    if let Some(anchor_id) = position.strip_prefix("before:") {
        let anchor_idx = others
            .iter()
            .position(|(id, _)| id == anchor_id)
            .ok_or_else(|| anyhow!("Anchor item not found: {anchor_id}"))?;
        return if anchor_idx == 0 {
            parse_key(&others[0].1).map(|k| FractionalIndex::new_before(&k))
        } else {
            let before = parse_key(&others[anchor_idx - 1].1)?;
            let after = parse_key(&others[anchor_idx].1)?;
            FractionalIndex::new_between(&before, &after)
                .ok_or_else(|| anyhow!("Cannot compute sort_key between adjacent items"))
        };
    }

    if let Some(anchor_id) = position.strip_prefix("after:") {
        let anchor_idx = others
            .iter()
            .position(|(id, _)| id == anchor_id)
            .ok_or_else(|| anyhow!("Anchor item not found: {anchor_id}"))?;
        return if anchor_idx == others.len() - 1 {
            parse_key(&others[anchor_idx].1).map(|k| FractionalIndex::new_after(&k))
        } else {
            let before = parse_key(&others[anchor_idx].1)?;
            let after = parse_key(&others[anchor_idx + 1].1)?;
            FractionalIndex::new_between(&before, &after)
                .ok_or_else(|| anyhow!("Cannot compute sort_key between adjacent items"))
        };
    }

    match position {
        "top" => {
            if let Some((_, first_key)) = others.first() {
                parse_key(first_key).map(|k| FractionalIndex::new_before(&k))
            } else {
                Ok(FractionalIndex::default())
            }
        }
        "bottom" => {
            if let Some((_, last_key)) = others.last() {
                parse_key(last_key).map(|k| FractionalIndex::new_after(&k))
            } else {
                Ok(FractionalIndex::default())
            }
        }
        idx_str => {
            // Numeric positions are deprecated — prefer top/bottom/before:/after:
            eprintln!(
                "⚠️  Numeric position '{idx_str}' is deprecated. Use top, bottom, before:<id>, or after:<id>."
            );
            let idx: usize = idx_str
                .parse()
                .with_context(|| format!("Invalid position: '{idx_str}'. Use top, bottom, before:<id>, after:<id>, or a number."))?;

            if others.is_empty() {
                Ok(FractionalIndex::default())
            } else if idx == 0 {
                parse_key(&others[0].1).map(|k| FractionalIndex::new_before(&k))
            } else if idx >= others.len() {
                parse_key(&others[others.len() - 1].1).map(|k| FractionalIndex::new_after(&k))
            } else {
                let before = parse_key(&others[idx - 1].1)?;
                let after = parse_key(&others[idx].1)?;
                FractionalIndex::new_between(&before, &after)
                    .ok_or_else(|| anyhow!("Cannot compute sort_key between adjacent items"))
            }
        }
    }
}

/// Parse a `sort_key` string into a `FractionalIndex`.
fn parse_key(s: &str) -> Result<FractionalIndex> {
    FractionalIndex::from_string(s).map_err(|e| anyhow!("Invalid sort_key '{s}': {e}"))
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rowset_counter(conn: &exosuit_storage::Connection, table: &str) -> Result<i64> {
        conn.query_row(
            "SELECT counter FROM rowset_revisions WHERE table_name = ?1",
            [table],
            |row| row.get(0),
        )
        .with_context(|| format!("Failed to read rowset counter for {table}"))
    }

    fn digest_count(conn: &exosuit_storage::Connection, table: &str) -> Result<i64> {
        let rev_table = table.replace("_data", "_rev");
        conn.query_row(&format!("SELECT COUNT(*) FROM {rev_table}"), [], |row| {
            row.get(0)
        })
        .with_context(|| format!("Failed to read digest count for {rev_table}"))
    }

    fn assert_reactive_write(
        conn: &exosuit_storage::Connection,
        table: &str,
        expected_counter: i64,
        expected_digests: i64,
    ) -> Result<()> {
        assert_eq!(
            rowset_counter(conn, table)?,
            expected_counter,
            "{table} rowset counter should match"
        );
        assert_eq!(
            digest_count(conn, table)?,
            expected_digests,
            "{table} digest count should match"
        );
        Ok(())
    }

    #[test]
    fn test_add_and_remove_epoch() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let id = w.add_epoch("Test Epoch", None, &[])?;
        let title: String = conn.query_row(
            "SELECT title FROM epochs_data WHERE text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(title, "Test Epoch");

        w.remove_epoch(&id)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM epochs_data WHERE text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    fn writer_methods_update_reactive_revision_metadata() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_id = w.add_epoch("E1", Some("e1"), &[])?;
        assert_reactive_write(conn, "epochs_data", 1, 1)?;

        let phase_id = w.add_phase(&epoch_id, "P1", "regular", Some("p1"), &[])?;
        assert_reactive_write(conn, "phases_data", 1, 1)?;

        w.replace_phase_rfcs(&phase_id, &["10165".to_string()])?;
        assert_reactive_write(conn, "phase_rfcs_data", 1, 1)?;

        w.set_workspace_active_phase("/tmp/exo-reactive-test", &phase_id)?;
        assert_reactive_write(conn, "workspace_active_phase_data", 1, 1)?;

        w.set_phase_owner(
            &phase_id,
            "branch",
            "wycats/rfc-10165-reactive-sqlite",
            Some("workspace-id"),
            Some("/tmp/exo-reactive-test"),
        )?;
        assert_reactive_write(conn, "phase_ownership_data", 1, 1)?;

        let claimed_unowned = w.claim_phase_owner_if_current(
            &phase_id,
            "branch",
            "other-branch",
            Some("workspace-id"),
            Some("/tmp/exo-reactive-test"),
            None,
        )?;
        assert!(!claimed_unowned);
        assert_reactive_write(conn, "phase_ownership_data", 1, 1)?;

        let claimed_existing = w.claim_phase_owner_if_current(
            &phase_id,
            "workspace",
            "workspace-owner",
            Some("workspace-id"),
            Some("/tmp/exo-reactive-test"),
            Some(("branch", "wycats/rfc-10165-reactive-sqlite")),
        )?;
        assert!(claimed_existing);
        assert_reactive_write(conn, "phase_ownership_data", 2, 1)?;

        w.add_goal(
            &phase_id,
            "g1",
            "Goal 1",
            None,
            None,
            None,
            None,
            None,
            Some("g1"),
            &[],
        )?;
        assert_reactive_write(conn, "goals_data", 1, 1)?;

        w.update_goal_status("g1", "in-progress")?;
        assert_reactive_write(conn, "goals_data", 2, 1)?;
        let goal_label: String =
            conn.query_row("SELECT label FROM goals WHERE text_id = 'g1'", [], |row| {
                row.get(0)
            })?;
        assert_eq!(goal_label, "Goal 1");

        w.add_task("g1", "t1", "Task 1", None)?;
        assert_reactive_write(conn, "tasks_data", 1, 1)?;

        w.complete_task("t1", "done")?;
        assert_reactive_write(conn, "tasks_data", 2, 1)?;
        let task_title: String =
            conn.query_row("SELECT title FROM tasks WHERE text_id = 't1'", [], |row| {
                row.get(0)
            })?;
        assert_eq!(task_title, "Task 1");

        let inbox_id = w.add_inbox_item(
            "project",
            None,
            "user-feedback",
            "fyi",
            "next-touch",
            None,
            None,
            "Subject",
            "Body",
            None,
        )?;
        assert_reactive_write(conn, "inbox_data", 1, 1)?;

        w.update_inbox_status(&inbox_id, "resolved", Some("handled"))?;
        assert_reactive_write(conn, "inbox_data", 2, 1)?;

        w.upsert_rfc(
            "rfc-row",
            10165,
            "Reactive SQLite",
            3,
            "active",
            None,
            "reactive-sqlite",
            "docs/rfcs/stage-3/10165-reactive-sqlite.md",
            None,
            None,
            None,
            None,
            None,
        )?;
        assert_reactive_write(conn, "rfcs_data", 1, 1)?;

        w.update_rfc_stage("rfc-row", 4, "docs/rfcs/stage-4/10165-reactive-sqlite.md")?;
        assert_reactive_write(conn, "rfcs_data", 2, 1)?;

        w.remove_epoch(&epoch_id)?;
        assert_reactive_write(conn, "epochs_data", 2, 0)?;
        assert_reactive_write(conn, "phases_data", 2, 0)?;
        assert_reactive_write(conn, "phase_rfcs_data", 2, 0)?;
        assert_reactive_write(conn, "workspace_active_phase_data", 2, 0)?;
        assert_reactive_write(conn, "phase_ownership_data", 3, 0)?;
        assert_reactive_write(conn, "goals_data", 3, 0)?;
        assert_reactive_write(conn, "tasks_data", 3, 0)?;

        Ok(())
    }

    #[test]
    fn reorder_and_move_helpers_update_reactive_revision_metadata() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_a = w.add_epoch("Epoch A", Some("epoch-a"), &[])?;
        let epoch_b = w.add_epoch("Epoch B", Some("epoch-b"), &[])?;
        let phase_a = w.add_phase(&epoch_a, "Phase A", "regular", Some("phase-a"), &[])?;
        let phase_b = w.add_phase(&epoch_a, "Phase B", "regular", Some("phase-b"), &[])?;

        let phase_counter = rowset_counter(conn, "phases_data")?;
        w.reorder_phase(&phase_b, "top")?;
        assert_reactive_write(conn, "phases_data", phase_counter + 1, 2)?;

        let phase_counter = rowset_counter(conn, "phases_data")?;
        w.move_phase_to_epoch(&phase_b, &epoch_b, None)?;
        assert_reactive_write(conn, "phases_data", phase_counter + 1, 2)?;

        w.add_goal(
            &phase_a,
            "g1",
            "Goal 1",
            None,
            None,
            None,
            None,
            None,
            Some("g1"),
            &[],
        )?;
        w.add_goal(
            &phase_a,
            "g2",
            "Goal 2",
            None,
            None,
            None,
            None,
            None,
            Some("g2"),
            &[],
        )?;

        let goal_counter = rowset_counter(conn, "goals_data")?;
        w.reorder_goal("g2", "top")?;
        assert_reactive_write(conn, "goals_data", goal_counter + 1, 2)?;

        let goal_counter = rowset_counter(conn, "goals_data")?;
        w.move_goal_to_phase_position("g2", &phase_b, None)?;
        assert_reactive_write(conn, "goals_data", goal_counter + 1, 2)?;

        w.add_task("g1", "t1", "Task 1", None)?;
        w.add_task("g1", "t2", "Task 2", None)?;

        let task_counter = rowset_counter(conn, "tasks_data")?;
        w.reorder_task("t2", "top")?;
        assert_reactive_write(conn, "tasks_data", task_counter + 1, 2)?;

        Ok(())
    }

    #[test]
    fn test_remove_epoch_cleans_cascaded_descendant_aliases() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        // Build a full epoch → phase → goal → task hierarchy, each with an
        // alias. Deleting the epoch cascades the entity rows (ON DELETE
        // CASCADE), but entity_aliases has no FK cascade — so without explicit
        // cleanup the descendants' aliases would orphan and break the dump.
        let epoch_id = w.add_epoch("E1", None, &["e-alias".to_string()])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &["p-alias".to_string()])?;
        w.add_goal(
            &phase_id,
            "g1",
            "G1",
            None,
            None,
            None,
            None,
            None,
            None,
            &["g-alias".to_string()],
        )?;
        w.add_task("g1", "t1", "T1", None)?;
        conn.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 SELECT 'task', id, 't-alias' FROM tasks_data WHERE text_id = 't1'",
            [],
        )?;

        let aliases_before: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))?;
        assert_eq!(aliases_before, 4, "all four aliases should be present");

        w.remove_epoch(&epoch_id)?;

        // Every descendant entity row is gone via cascade...
        let entity_rows: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM epochs_data)
                  + (SELECT COUNT(*) FROM phases_data)
                  + (SELECT COUNT(*) FROM goals_data)
                  + (SELECT COUNT(*) FROM tasks_data)",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(entity_rows, 0, "cascade should delete all descendant rows");

        // ...and no orphaned aliases are left behind.
        let aliases_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))?;
        assert_eq!(
            aliases_after, 0,
            "all descendant aliases should be cleaned up, leaving no orphans"
        );
        Ok(())
    }

    #[test]
    fn test_remove_phase_cleans_cascaded_descendant_aliases() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        // Exercises the `phase` arm of delete_entity_aliases: removing a phase
        // must clean the phase's own alias plus its descendant goal/task
        // aliases (which have no FK cascade of their own).
        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &["p-alias".to_string()])?;
        w.add_goal(
            &phase_id,
            "g1",
            "G1",
            None,
            None,
            None,
            None,
            None,
            None,
            &["g-alias".to_string()],
        )?;
        w.add_task("g1", "t1", "T1", None)?;
        conn.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 SELECT 'task', id, 't-alias' FROM tasks_data WHERE text_id = 't1'",
            [],
        )?;

        w.remove_phase(&phase_id)?;

        // Phase/goal/task rows gone via cascade; epoch (the non-deleted parent)
        // remains.
        let descendant_rows: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM phases_data)
                  + (SELECT COUNT(*) FROM goals_data)
                  + (SELECT COUNT(*) FROM tasks_data)",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            descendant_rows, 0,
            "cascade should delete phase + descendants"
        );

        // No phase/goal/task aliases survive (the epoch alias would, but we
        // added none).
        let aliases_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))?;
        assert_eq!(
            aliases_after, 0,
            "phase delete should clean phase, goal, and task aliases"
        );
        Ok(())
    }

    #[test]
    fn test_remove_goal_cleans_cascaded_descendant_aliases() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        // Exercises the `goal` arm of delete_entity_aliases: removing a goal
        // must clean the goal's own alias plus its descendant task aliases.
        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(
            &phase_id,
            "g1",
            "G1",
            None,
            None,
            None,
            None,
            None,
            None,
            &["g-alias".to_string()],
        )?;
        w.add_task("g1", "t1", "T1", None)?;
        conn.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 SELECT 'task', id, 't-alias' FROM tasks_data WHERE text_id = 't1'",
            [],
        )?;

        let aliases_before: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))?;
        assert_eq!(aliases_before, 2, "goal + task aliases should be present");

        w.remove_goal("g1")?;

        // Goal and its task are gone via cascade; phase/epoch remain.
        let goal_task_rows: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM goals_data)
                  + (SELECT COUNT(*) FROM tasks_data)",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(goal_task_rows, 0, "cascade should delete goal + task");

        let aliases_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))?;
        assert_eq!(
            aliases_after, 0,
            "goal delete should clean goal and task aliases"
        );
        Ok(())
    }

    #[test]
    fn test_add_phase_with_sort_key() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_id = w.add_epoch("E1", None, &[])?;
        let p1 = w.add_phase(&epoch_id, "Phase 1", "regular", None, &[])?;
        let p2 = w.add_phase(&epoch_id, "Phase 2", "regular", None, &[])?;

        let k1: String = conn.query_row(
            "SELECT sort_key FROM phases_data WHERE text_id = ?",
            [&p1],
            |row| row.get(0),
        )?;
        let k2: String = conn.query_row(
            "SELECT sort_key FROM phases_data WHERE text_id = ?",
            [&p2],
            |row| row.get(0),
        )?;
        assert!(k1 < k2, "sort_key should be ordered: {} < {}", k1, k2);
        Ok(())
    }

    #[test]
    fn test_add_and_complete_goal() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(
            &phase_id,
            "my-goal",
            "My Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;

        w.update_goal_status("my-goal", "in-progress")?;
        let status: String = conn.query_row(
            "SELECT status FROM goals_data WHERE text_id = 'my-goal'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(status, "in-progress");

        w.update_goal_completion_log("my-goal", "Done!")?;
        w.update_goal_status("my-goal", "completed")?;
        let log: String = conn.query_row(
            "SELECT completion_log FROM goals_data WHERE text_id = 'my-goal'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(log, "Done!");
        Ok(())
    }

    #[test]
    fn test_reorder_goal() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(
            &phase_id,
            "g1",
            "Goal 1",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;
        w.add_goal(
            &phase_id,
            "g2",
            "Goal 2",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;
        w.add_goal(
            &phase_id,
            "g3",
            "Goal 3",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;

        // Move g3 to top
        w.reorder_goal("g3", "top")?;

        let mut stmt = conn.prepare(
            "SELECT text_id FROM goals_data WHERE phase_id = (
                SELECT id FROM phases_data WHERE text_id = ?
             ) ORDER BY sort_key, id",
        )?;
        let order: Vec<String> = stmt
            .query_map([&phase_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        assert_eq!(order, vec!["g3", "g1", "g2"]);
        Ok(())
    }

    #[test]
    fn test_add_task_with_log_and_verification() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(
            &phase_id,
            "g1",
            "Goal 1",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;
        w.add_task("g1", "t1", "Task 1", None)?;

        w.add_task_log("t1", "note", "Started working")?;
        w.add_task_verification("t1", "test", Some("cargo test"), "pass", None)?;

        let log_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_logs tl
             JOIN tasks_data td ON tl.task_id = td.id
             WHERE td.text_id = 't1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(log_count, 1);

        let ver_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_verifications tv
             JOIN tasks_data td ON tv.task_id = td.id
             WHERE td.text_id = 't1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(ver_count, 1);

        w.complete_task("t1", "All done")?;
        let status: String = conn.query_row(
            "SELECT status FROM tasks_data WHERE text_id = 't1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(status, "completed");
        Ok(())
    }

    #[test]
    fn test_add_idea_with_tags() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let id = w.add_idea(
            "My Idea",
            Some("A description"),
            &["cli".into(), "ux".into()],
        )?;

        let title: String = conn.query_row(
            "SELECT title FROM ideas_data WHERE text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(title, "My Idea");

        let tag_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM idea_tags it
             JOIN ideas_data i ON it.idea_id = i.id
             WHERE i.text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(tag_count, 2);
        Ok(())
    }

    #[test]
    fn test_inbox_lifecycle() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let conn = w.database().connection();

        let id = w.add_inbox_item(
            "project",
            None,
            "user-feedback",
            "fyi",
            "next-touch",
            None,
            None,
            "Something happened",
            "",
            None,
        )?;
        assert!(id.starts_with("intent-"));

        // Acknowledge
        w.update_inbox_status(&id, "acknowledged", None)?;
        let status: String = conn.query_row(
            "SELECT status FROM inbox_data WHERE text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(status, "acknowledged");

        // Resolve
        w.update_inbox_status(&id, "resolved", Some("Fixed it"))?;

        // Archive resolved
        let archived = w.archive_resolved_inbox()?;
        assert_eq!(archived, 1);

        let final_status: String = conn.query_row(
            "SELECT status FROM inbox_data WHERE text_id = ?",
            [&id],
            |row| row.get(0),
        )?;
        assert_eq!(final_status, "archived");
        Ok(())
    }

    // ── Position engine tests ────────────────────────────────────────

    fn goal_order(w: &SqliteWriter, phase_id: &str) -> Vec<String> {
        let conn = w.database().connection();
        let mut stmt = conn
            .prepare(
                "SELECT text_id FROM goals_data WHERE phase_id = (
                    SELECT id FROM phases_data WHERE text_id = ?
                 ) ORDER BY sort_key, id",
            )
            .unwrap();
        stmt.query_map([phase_id], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn test_reorder_before_after() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(&phase_id, "a", "A", None, None, None, None, None, None, &[])?;
        w.add_goal(&phase_id, "b", "B", None, None, None, None, None, None, &[])?;
        w.add_goal(&phase_id, "c", "C", None, None, None, None, None, None, &[])?;
        w.add_goal(&phase_id, "d", "D", None, None, None, None, None, None, &[])?;

        // Move d before b → a, d, b, c
        w.reorder_goal("d", "before:b")?;
        assert_eq!(goal_order(&w, &phase_id), vec!["a", "d", "b", "c"]);

        // Move a after c → d, b, c, a
        w.reorder_goal("a", "after:c")?;
        assert_eq!(goal_order(&w, &phase_id), vec!["d", "b", "c", "a"]);

        // Move a before d (first position) → a, d, b, c
        w.reorder_goal("a", "before:d")?;
        assert_eq!(goal_order(&w, &phase_id), vec!["a", "d", "b", "c"]);

        // Move b after c (last position) → a, d, c, b
        w.reorder_goal("b", "after:c")?;
        assert_eq!(goal_order(&w, &phase_id), vec!["a", "d", "c", "b"]);

        Ok(())
    }

    #[test]
    fn test_reorder_task() -> Result<()> {
        let w = SqliteWriter::open_memory()?;
        let epoch_id = w.add_epoch("E1", None, &[])?;
        let phase_id = w.add_phase(&epoch_id, "P1", "regular", None, &[])?;
        w.add_goal(
            &phase_id,
            "g1",
            "Goal 1",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )?;
        w.add_task("g1", "t1", "Task 1", None)?;
        w.add_task("g1", "t2", "Task 2", None)?;
        w.add_task("g1", "t3", "Task 3", None)?;

        let conn = w.database().connection();
        let task_order = |conn: &Connection| -> Vec<String> {
            let mut stmt = conn
                .prepare(
                    "SELECT text_id FROM tasks_data WHERE goal_id = (
                        SELECT id FROM goals_data WHERE text_id = 'g1'
                     ) ORDER BY sort_key, id",
                )
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };

        // Move t3 to top
        w.reorder_task("t3", "top")?;
        assert_eq!(task_order(conn), vec!["t3", "t1", "t2"]);

        // Move t1 after t2
        w.reorder_task("t1", "after:t2")?;
        assert_eq!(task_order(conn), vec!["t3", "t2", "t1"]);

        Ok(())
    }
}
