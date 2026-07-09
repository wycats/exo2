//! SQLite-based loader for `ExoState`.
//!
//! Loads the project state from `SQLite` database instead of TOML files.
//! This is the read path for the reactive storage layer (RFC 10165).
//!
//! # Architecture
//!
//! The loader reads from shadow tables (`*_data`) which are wrapped by
//! reactive virtual tables. Tests insert directly into shadow tables;
//! production code can use either (virtual tables add trace recording).
//!
//! # Alias Resolution
//!
//! The `entity_aliases` table uses INTEGER rowids (not `text_id`) to reference
//! entities. This means we must query aliases using the rowid returned from
//! the main entity query, not the `text_id`.

use crate::context::{Epoch, ExoState, Goal, Meta, Phase, PhaseRfc};
use crate::idea::Idea;
use crate::inbox::{
    InboxConfidence, InboxIntent, InboxItem, InboxItemStatus, InboxPriority, InboxSource,
};
use crate::ulid_util::parse_ulid;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use exosuit_storage::rusqlite::config::DbConfig;
use exosuit_storage::{
    Connection, Database, OptionalExtension, Row, open_database, open_memory_database,
};
use fractional_index::FractionalIndex;
use std::collections::HashMap;
use std::path::Path;

const LEGACY_COMPLETION_APPROVAL_SUBJECT: &str = "Workflow confirmation accepted";
const COMPLETION_APPROVAL_SUBJECT: &str = "Outcome approved";

fn display_inbox_subject(subject: String) -> String {
    if subject == LEGACY_COMPLETION_APPROVAL_SUBJECT {
        COMPLETION_APPROVAL_SUBJECT.to_string()
    } else {
        subject
    }
}

/// Result of checking whether a completion claim exists for an entity.
///
/// Used by the completion guard to enforce shared-perception completion:
/// - Human claims pass immediately (the human already validated)
/// - Agent claims require human acknowledgment before the agent can close
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionClaimStatus {
    /// No active claim found for this entity.
    NoClaim,
    /// A human created the claim (`agent_id` IS NULL).
    HumanClaim,
    /// An agent created the claim but it hasn't been acknowledged yet.
    AgentClaimPending,
    /// An agent created the claim and a human acknowledged it.
    AgentClaimAcknowledged,
}

/// A derived completion claim digest for review surfaces.
///
/// This is read from canonical inbox state. It is not persisted as a separate
/// projection table.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOutcomeDigest {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub status: String,
    pub source: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub created: String,
}

/// Loader for `ExoState` from `SQLite` database.
///
/// This provides an alternative to the TOML-based loader, reading from
/// the `SQLite` storage layer. Both loaders produce identical `ExoState`
/// output, enabling gradual migration.
#[derive(Debug)]
pub struct SqliteLoader {
    db: Database,
}

struct DefensiveModeGuard<'conn> {
    conn: &'conn Connection,
    restore: bool,
}

impl Drop for DefensiveModeGuard<'_> {
    fn drop(&mut self) {
        let _ = self
            .conn
            .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, self.restore);
    }
}

fn defensive_mode_disabled(conn: &Connection) -> Result<DefensiveModeGuard<'_>> {
    let restore = conn
        .db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE)
        .context("Failed to read SQLite defensive mode")?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
        .context("Failed to disable SQLite defensive mode")?;
    Ok(DefensiveModeGuard { conn, restore })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLog {
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseDetailsTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub notes: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub logs: Vec<TaskLog>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseDetailsGoal {
    pub id: String,
    pub title: String,
    pub status: String,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub started_at: Option<String>,
    pub completion_log: Option<String>,
    pub tasks: Vec<PhaseDetailsTask>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiblingPhase {
    pub id: String,
    pub title: String,
    pub status: String,
    pub goal_count: i64,
    pub completed_goals: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextEpoch {
    pub title: String,
    pub phase_count: usize,
    pub phase_titles: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpochContext {
    pub epoch_id: String,
    pub epoch_title: String,
    pub phase_index: usize,
    pub total_phases: usize,
    pub sibling_phases: Vec<SiblingPhase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_epoch: Option<NextEpoch>,
}

/// An RFC record from `SQLite` (reactive metadata).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RfcRecord {
    pub text_id: String,
    pub rfc_number: i64,
    pub title: String,
    pub stage: u8,
    pub status: String,
    pub feature: Option<String>,
    pub slug: String,
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdrawal_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consolidated_into: Option<String>,
}

/// Machine-local identity for one workspace RFC document snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfcWorkspaceSnapshot {
    pub workspace_root: String,
    pub branch_name: Option<String>,
    pub head_oid: String,
    pub document_digest: Vec<u8>,
    pub canonical_ref: Option<String>,
    pub canonical_oid: Option<String>,
    pub observed_at: String,
}

/// Parsed RFC document observed in one workspace snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfcWorkspaceObservation {
    pub workspace_root: String,
    pub text_id: String,
    pub rfc_number: i64,
    pub title: String,
    pub stage: u8,
    pub stage_source: String,
    pub status: String,
    pub feature: Option<String>,
    pub feature_declared: bool,
    pub slug: String,
    pub file_path: String,
    pub superseded_by: Option<String>,
    pub superseded_by_declared: bool,
    pub supersedes: Option<String>,
    pub supersedes_declared: bool,
    pub withdrawal_reason: Option<String>,
    pub withdrawal_reason_declared: bool,
    pub archived_reason: Option<String>,
    pub archived_reason_declared: bool,
    pub consolidated_into: Option<String>,
    pub consolidated_into_declared: bool,
    pub branch_name: Option<String>,
    pub head_oid: String,
    pub observed_at: String,
}

/// Parse or identity diagnostic retained with one workspace RFC snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RfcWorkspaceDiagnostic {
    pub workspace_root: String,
    pub file_path: String,
    pub diagnostic_code: String,
    pub text_id: Option<String>,
    pub rfc_number: Option<i64>,
    pub message: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseProgress {
    pub mode: String,
    pub goals_completed: usize,
    pub goals_total: usize,
    pub tasks_completed: usize,
    pub tasks_total: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseInboxItem {
    pub id: String,
    pub subject: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    pub source: String,
    pub intent: String,
    pub priority: String,
    pub status: String,
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseCompletionDigest {
    pub entity_type: String,
    pub entity_id: String,
    pub claims: Vec<CompletionOutcomeDigest>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseOwnerData {
    pub owner_kind: String,
    pub owner_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by_workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by_workspace_root: Option<String>,
    pub claimed_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseDetailsData {
    pub phase_id: String,
    pub phase_title: String,
    pub epoch_id: String,
    pub epoch_title: String,
    pub rfcs: Vec<String>,
    pub kind: String,
    pub goals: Vec<PhaseDetailsGoal>,
    pub progress: PhaseProgress,
    pub epoch_context: EpochContext,
    pub inbox_items: Vec<PhaseInboxItem>,
    pub completion_digests: Vec<PhaseCompletionDigest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<PhaseOwnerData>,
}

impl SqliteLoader {
    /// Open a database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = open_database(path.as_ref())
            .with_context(|| format!("Failed to open database at {}", path.as_ref().display()))?;
        Ok(Self { db })
    }

    /// Create an in-memory database for testing.
    pub fn open_memory() -> Result<Self> {
        let db = open_memory_database().context("Failed to create in-memory database")?;
        #[cfg(test)]
        db.connection()
            .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .context("Failed to disable SQLite defensive mode for test fixtures")?;
        Ok(Self { db })
    }

    /// Get a reference to the underlying database.
    pub const fn database(&self) -> &Database {
        &self.db
    }

    /// Load the phase `text_id` pinned for a workspace root.
    pub fn load_workspace_active_phase(&self, workspace_root: &str) -> Result<Option<String>> {
        let conn = self.db.connection();
        conn.query_row(
            "SELECT p.text_id
             FROM workspace_active_phase wap
             JOIN phases p ON p.id = wap.phase_id
             WHERE wap.workspace_root = ?1
             LIMIT 1",
            [workspace_root],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to load workspace active phase")
    }

    /// List workspace root keys that have an active phase pin.
    pub fn list_workspace_active_phase_roots(&self) -> Result<Vec<String>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT workspace_root
                 FROM workspace_active_phase
                 ORDER BY updated_at DESC, workspace_root ASC",
            )
            .context("Failed to prepare workspace active phase roots query")?;

        stmt.query_map([], |row| row.get::<_, String>(0))
            .context("Failed to query workspace active phase roots")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read workspace active phase roots")
    }

    /// Load the ownership claim for a phase by text id.
    pub fn load_phase_owner(&self, phase_text_id: &str) -> Result<Option<PhaseOwnerData>> {
        let conn = self.db.connection();
        conn.query_row(
            "SELECT po.owner_kind,
                    po.owner_id,
                    po.claimed_by_workspace_id,
                    po.claimed_by_workspace_root,
                    po.claimed_at,
                    po.updated_at
             FROM phase_ownership po
             JOIN phases p ON p.id = po.phase_id
             WHERE p.text_id = ?1
             LIMIT 1",
            [phase_text_id],
            |row| {
                Ok(PhaseOwnerData {
                    owner_kind: row.get(0)?,
                    owner_id: row.get(1)?,
                    claimed_by_workspace_id: row.get(2)?,
                    claimed_by_workspace_root: row.get(3)?,
                    claimed_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .context("Failed to load phase owner")
    }

    /// Load all phase ownership claims keyed by phase text id.
    pub fn load_phase_owners(&self) -> Result<HashMap<String, PhaseOwnerData>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT p.text_id,
                        po.owner_kind,
                        po.owner_id,
                        po.claimed_by_workspace_id,
                        po.claimed_by_workspace_root,
                        po.claimed_at,
                        po.updated_at
                 FROM phase_ownership po
                 JOIN phases p ON p.id = po.phase_id",
            )
            .context("Failed to prepare phase owners query")?;

        let rows: Vec<(String, PhaseOwnerData)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    PhaseOwnerData {
                        owner_kind: row.get(1)?,
                        owner_id: row.get(2)?,
                        claimed_by_workspace_id: row.get(3)?,
                        claimed_by_workspace_root: row.get(4)?,
                        claimed_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    },
                ))
            })
            .context("Failed to query phase owners")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read phase owners")?;

        Ok(rows.into_iter().collect())
    }

    fn resolve_active_phase_rowid(&self, workspace_root: Option<&str>) -> Result<Option<i64>> {
        let conn = self.db.connection();

        if let Some(workspace_root) = workspace_root {
            let pinned: Option<(i64, String)> = conn
                .query_row(
                    "SELECT p.id, p.status
                     FROM workspace_active_phase wap
                     JOIN phases p ON p.id = wap.phase_id
                     WHERE wap.workspace_root = ?1
                     LIMIT 1",
                    [workspace_root],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .context("Failed to resolve workspace active phase")?;

            if let Some((phase_rowid, status)) = pinned {
                return Ok((status == "in-progress").then_some(phase_rowid));
            }
        }

        let mut stmt = conn
            .prepare("SELECT id FROM phases WHERE status = 'in-progress' LIMIT 2")
            .context("Failed to prepare active phase fallback query")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, i64>(0))
            .context("Failed to query active phase fallback")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read active phase fallback")?;

        Ok(if rows.len() == 1 { Some(rows[0]) } else { None })
    }

    fn resolve_active_phase_text_id(&self, workspace_root: Option<&str>) -> Result<Option<String>> {
        let Some(phase_rowid) = self.resolve_active_phase_rowid(workspace_root)? else {
            return Ok(None);
        };

        let conn = self.db.connection();
        conn.query_row(
            "SELECT text_id FROM phases WHERE id = ?1",
            [phase_rowid],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to resolve active phase text_id")
    }

    /// List axioms from the database, optionally filtered by scope.
    pub fn list_axioms(&self, scope: Option<&str>) -> Result<Vec<crate::axiom::Axiom>> {
        let conn = self.db.connection();
        let axioms = if let Some(scope) = scope {
            let mut stmt = conn.prepare(
                "SELECT id, text_id, scope, principle, rationale, notes FROM axioms WHERE scope = ? ORDER BY text_id"
            )?;
            stmt.query_map([scope], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, text_id, scope, principle, rationale, notes FROM axioms ORDER BY text_id"
            )?;
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let mut result = Vec::new();
        for (rowid, text_id, _scope, principle, rationale, notes) in axioms {
            let mut imp_stmt = conn.prepare(
                "SELECT implication FROM axiom_implications WHERE axiom_id = ? ORDER BY sort_key",
            )?;
            let implications: Vec<String> = imp_stmt
                .query_map([rowid], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            let mut tag_stmt =
                conn.prepare("SELECT tag FROM axiom_tags WHERE axiom_id = ? ORDER BY tag")?;
            let tags: Vec<String> = tag_stmt
                .query_map([rowid], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            result.push(crate::axiom::Axiom {
                id: text_id,
                principle,
                rationale,
                implications,
                notes,
                tags,
            });
        }

        Ok(result)
    }

    /// Load the full `ExoState` from the database.
    pub fn load_state(&self) -> Result<ExoState> {
        let epochs = self.load_epochs()?;
        Ok(ExoState {
            meta: Some(Meta::current()),
            epochs,
        })
    }

    #[allow(clippy::type_complexity)]
    pub fn load_active_phase_details(&self) -> Result<Option<PhaseDetailsData>> {
        self.load_active_phase_details_for_workspace(None)
    }

    #[allow(clippy::type_complexity)]
    pub fn load_active_phase_details_for_workspace(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<Option<PhaseDetailsData>> {
        let Some(active_phase_id) = self.resolve_active_phase_text_id(workspace_root)? else {
            return Ok(None);
        };
        self.load_phase_details_impl(&active_phase_id)
    }

    pub fn load_phase_details_by_id(&self, text_id: &str) -> Result<Option<PhaseDetailsData>> {
        self.load_phase_details_impl(text_id)
    }

    /// Shared implementation: loads phase details for a specific phase.
    fn load_phase_details_impl(&self, text_id: &str) -> Result<Option<PhaseDetailsData>> {
        let conn = self.db.connection();

        // Fetch phase + parent epoch (including epoch rowid and sort_key
        // for sibling/next-epoch queries).
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.text_id, e.title, e.sort_key,
                        p.id, p.text_id, p.title, p.kind
                 FROM phases p
                 JOIN epochs e ON p.epoch_id = e.id
                 WHERE p.text_id = ?1
                 LIMIT 1",
            )
            .context("Failed to prepare phase details query")?;
        let phase_row = stmt
            .query_row([text_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .optional()
            .context("Failed to query phase details")?;

        let Some((
            epoch_rowid,
            epoch_id,
            epoch_title,
            epoch_sort_key,
            phase_rowid,
            phase_id,
            phase_title,
            phase_kind,
        )) = phase_row
        else {
            return Ok(None);
        };

        let rfcs = self
            .load_phase_rfcs(phase_rowid)
            .context("Failed to load phase RFCs")?
            .into_iter()
            .map(|rfc| rfc.id)
            .collect();

        let conn = self.db.connection();
        let mut goal_stmt = conn
            .prepare(
                "SELECT id, text_id, label, status, kind, started_at, description, completion_log
                 FROM goals
                 WHERE phase_id = ?
                 ORDER BY sort_key NULLS LAST, id",
            )
            .context("Failed to prepare active phase goal details query")?;

        type GoalDetailRow = (
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );

        type TaskDetailRow = (
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        );

        let goal_rows: Vec<GoalDetailRow> = goal_stmt
            .query_map([phase_rowid], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })
            .context("Failed to query active phase goals")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read active phase goals")?;

        let mut task_stmt = conn
            .prepare(
                "SELECT id, text_id, title, status, notes, started_at, completed_at
                 FROM tasks
                 WHERE goal_id = ?
                 ORDER BY sort_key NULLS LAST, id",
            )
            .context("Failed to prepare active phase task details query")?;

        let mut task_log_stmt = conn
            .prepare(
                "SELECT kind, message, created_at
                 FROM task_logs
                 WHERE task_id = ?
                 ORDER BY id",
            )
            .context("Failed to prepare task logs query")?;

        let mut goals = Vec::with_capacity(goal_rows.len());
        for (
            goal_rowid,
            goal_id,
            goal_title,
            goal_status,
            goal_kind,
            goal_started_at,
            goal_description,
            goal_completion_log,
        ) in goal_rows
        {
            let task_rows: Vec<TaskDetailRow> = task_stmt
                .query_map([goal_rowid], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                })
                .context("Failed to query active phase tasks")?
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read active phase tasks")?;

            let mut tasks = Vec::with_capacity(task_rows.len());
            for (
                task_rowid,
                task_id,
                task_title,
                task_status,
                task_notes,
                task_started_at,
                task_completed_at,
            ) in task_rows
            {
                let logs: Vec<TaskLog> = task_log_stmt
                    .query_map([task_rowid], |row| {
                        Ok(TaskLog {
                            kind: row.get(0)?,
                            message: row.get(1)?,
                            created_at: row.get(2)?,
                        })
                    })
                    .context("Failed to query task logs")?
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read task logs")?;

                tasks.push(PhaseDetailsTask {
                    id: task_id,
                    title: task_title,
                    status: task_status,
                    notes: task_notes,
                    started_at: task_started_at,
                    completed_at: task_completed_at,
                    logs,
                });
            }

            goals.push(PhaseDetailsGoal {
                id: goal_id,
                title: goal_title,
                status: goal_status,
                description: goal_description,
                kind: goal_kind,
                started_at: goal_started_at,
                completion_log: goal_completion_log,
                tasks,
            });
        }

        // Compute progress from goals/tasks
        let goals_total = goals.len();
        let goals_completed = goals.iter().filter(|g| g.status == "completed").count();
        let tasks_total: usize = goals.iter().map(|g| g.tasks.len()).sum();
        let tasks_completed: usize = goals
            .iter()
            .flat_map(|g| &g.tasks)
            .filter(|t| t.status == "completed")
            .count();

        let progress = PhaseProgress {
            mode: "executing".to_string(),
            goals_completed,
            goals_total,
            tasks_completed,
            tasks_total,
        };

        // Sibling phases in the same epoch (with goal counts)
        let mut sibling_stmt = conn
            .prepare(
                "SELECT p.text_id, p.title, p.status,
                        COALESCE(g.goal_count, 0),
                        COALESCE(g.completed_goals, 0)
                 FROM phases p
                 LEFT JOIN (
                     SELECT phase_id,
                            COUNT(*) AS goal_count,
                            SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed_goals
                     FROM goals
                     GROUP BY phase_id
                 ) g ON g.phase_id = p.id
                 WHERE p.epoch_id = ?
                 ORDER BY p.sort_key NULLS LAST, p.id",
            )
            .context("Failed to prepare sibling phases query")?;

        let sibling_phases: Vec<SiblingPhase> = sibling_stmt
            .query_map([epoch_rowid], |row| {
                Ok(SiblingPhase {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    status: row.get(2)?,
                    goal_count: row.get(3)?,
                    completed_goals: row.get(4)?,
                })
            })
            .context("Failed to query sibling phases")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read sibling phases")?;

        let total_phases = sibling_phases.len();
        let phase_index = sibling_phases
            .iter()
            .position(|p| p.id == phase_id)
            .unwrap_or(0);

        // Next non-completed epoch (first epoch after the current one that
        // has at least one phase not yet completed).
        let mut next_epoch_stmt = conn
            .prepare(
                "SELECT e.id, e.title
                 FROM epochs e
                 WHERE (e.sort_key > ?1 OR (e.sort_key = ?1 AND e.id > ?2))
                   AND EXISTS (
                       SELECT 1 FROM phases p
                       WHERE p.epoch_id = e.id AND p.status != 'completed'
                   )
                 ORDER BY e.sort_key, e.id
                 LIMIT 1",
            )
            .context("Failed to prepare next epoch query")?;

        let next_epoch_row: Option<(i64, String)> = next_epoch_stmt
            .query_row(
                exosuit_storage::params![&epoch_sort_key, epoch_rowid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("Failed to query next epoch")?;

        let next_epoch = if let Some((next_epoch_rowid, next_epoch_title)) = next_epoch_row {
            let mut phase_titles_stmt = conn
                .prepare(
                    "SELECT title FROM phases
                     WHERE epoch_id = ?
                     ORDER BY sort_key NULLS LAST, id",
                )
                .context("Failed to prepare next epoch phases query")?;

            let phase_titles: Vec<String> = phase_titles_stmt
                .query_map([next_epoch_rowid], |row| row.get(0))
                .context("Failed to query next epoch phases")?
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read next epoch phases")?;

            Some(NextEpoch {
                title: next_epoch_title,
                phase_count: phase_titles.len(),
                phase_titles,
            })
        } else {
            None
        };

        let epoch_context = EpochContext {
            epoch_id: epoch_id.clone(),
            epoch_title: epoch_title.clone(),
            phase_index,
            total_phases,
            sibling_phases,
            next_epoch,
        };

        // Active inbox items relevant to this phase: scoped to the phase,
        // or referencing any goal/task within it.
        let goal_ids: Vec<String> = goals.iter().map(|g| g.id.clone()).collect();
        let task_ids: Vec<String> = goals
            .iter()
            .flat_map(|g| g.tasks.iter().map(|t| t.id.clone()))
            .collect();

        let mut inbox_stmt = conn
            .prepare(
                "SELECT text_id, subject, entity_type, entity_id, source, intent, priority, status, created_at, agent_id
                 FROM inbox
                 WHERE status = 'pending'
                                     AND intent != 'claim'
                 ORDER BY created_at DESC",
            )
            .context("Failed to prepare inbox query")?;

        let all_active_inbox: Vec<PhaseInboxItem> = inbox_stmt
            .query_map([], |row| {
                Ok(PhaseInboxItem {
                    id: row.get(0)?,
                    subject: display_inbox_subject(row.get(1)?),
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    source: row.get(4)?,
                    intent: row.get(5)?,
                    priority: row.get(6)?,
                    status: row.get(7)?,
                    created: row.get(8)?,
                    agent_id: row.get(9)?,
                })
            })
            .context("Failed to query inbox")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read inbox")?;

        // Filter to items relevant to this phase
        let inbox_items: Vec<PhaseInboxItem> = all_active_inbox
            .into_iter()
            .filter(
                |item| match (item.entity_type.as_str(), item.entity_id.as_deref()) {
                    ("phase", Some(id)) => id == phase_id,
                    ("goal", Some(id)) => goal_ids.iter().any(|g| g == id),
                    ("task", Some(id)) => task_ids.iter().any(|t| t == id),
                    ("project", _) => true,
                    _ => false,
                },
            )
            .collect();

        let phase_entity_ids = goal_ids
            .iter()
            .map(|id| ("goal", id.as_str()))
            .chain(task_ids.iter().map(|id| ("task", id.as_str())))
            .collect::<Vec<_>>();
        let completion_digests = self
            .load_completion_outcome_digests_for_entities(&phase_entity_ids)
            .context("Failed to load phase completion digests")?
            .into_iter()
            .filter(|digest| !digest.claims.is_empty())
            .collect();

        let owner = self
            .load_phase_owner(&phase_id)
            .context("Failed to load phase owner")?;

        Ok(Some(PhaseDetailsData {
            phase_id,
            phase_title,
            epoch_id,
            epoch_title,
            rfcs,
            kind: phase_kind,
            goals,
            progress,
            epoch_context,
            inbox_items,
            completion_digests,
            owner,
        }))
    }

    fn load_epochs(&self) -> Result<Vec<Epoch>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare("SELECT id, text_id, title, slug, reviewed FROM epochs ORDER BY sort_key, id")
            .context("Failed to prepare epochs query")?;

        let epoch_rows: Vec<(i64, String, String, Option<String>, bool)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, i32>(4)? != 0,
                ))
            })
            .context("Failed to execute epochs query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read epoch rows")?;

        let mut epochs = Vec::with_capacity(epoch_rows.len());
        for (rowid, text_id, title, slug, reviewed) in epoch_rows {
            let phases = self
                .load_phases(rowid)
                .with_context(|| format!("Failed to load phases for epoch '{text_id}'"))?;
            let aliases = self
                .load_aliases("epoch", rowid)
                .with_context(|| format!("Failed to load aliases for epoch '{text_id}'"))?;
            let ulid = parse_ulid(&text_id);

            epochs.push(Epoch {
                id: text_id,
                title,
                phases,
                ulid,
                slug,
                aliases,
                reviewed,
            });
        }

        Ok(epochs)
    }

    fn load_phases(&self, epoch_id: i64) -> Result<Vec<Phase>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, text_id, title, status, kind, slug
                 FROM phases WHERE epoch_id = ? ORDER BY sort_key NULLS LAST, id",
            )
            .context("Failed to prepare phases query")?;

        let phase_rows: Vec<(i64, String, String, String, String, Option<String>)> = stmt
            .query_map([epoch_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .context("Failed to execute phases query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read phase rows")?;

        let mut phases = Vec::with_capacity(phase_rows.len());
        for (rowid, text_id, title, status, kind, slug) in phase_rows {
            let goals = self
                .load_goals(rowid)
                .with_context(|| format!("Failed to load goals for phase '{text_id}'"))?;
            let rfcs = self
                .load_phase_rfcs(rowid)
                .with_context(|| format!("Failed to load RFCs for phase '{text_id}'"))?;
            let aliases = self
                .load_aliases("phase", rowid)
                .with_context(|| format!("Failed to load aliases for phase '{text_id}'"))?;
            let ulid = parse_ulid(&text_id);
            let kind = kind.parse().unwrap_or_default();

            phases.push(Phase {
                id: text_id,
                title,
                status,
                goals,
                rfcs,
                kind,
                ulid,
                slug,
                aliases,
            });
        }

        Ok(phases)
    }

    #[allow(clippy::type_complexity)]
    fn load_goals(&self, phase_id: i64) -> Result<Vec<Goal>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, text_id, label, status, kind, rfc, target_stage,
                        started_at, description, completion_log, slug
                 FROM goals WHERE phase_id = ? ORDER BY sort_key NULLS LAST, id",
            )
            .context("Failed to prepare goals query")?;

        let goal_rows = stmt
            .query_map([phase_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,             // rowid
                    row.get::<_, String>(1)?,          // text_id
                    row.get::<_, String>(2)?,          // label
                    row.get::<_, String>(3)?,          // status
                    row.get::<_, Option<String>>(4)?,  // kind
                    row.get::<_, Option<String>>(5)?,  // rfc
                    row.get::<_, Option<i32>>(6)?,     // target_stage
                    row.get::<_, Option<String>>(7)?,  // started_at
                    row.get::<_, Option<String>>(8)?,  // description
                    row.get::<_, Option<String>>(9)?,  // completion_log
                    row.get::<_, Option<String>>(10)?, // slug
                ))
            })
            .context("Failed to execute goals query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read goal rows")?;

        let mut goals = Vec::with_capacity(goal_rows.len());
        for row in goal_rows {
            let (
                rowid,
                text_id,
                label,
                status,
                kind,
                rfc,
                target_stage,
                started_at_str,
                description,
                completion_log,
                slug,
            ) = row;

            let aliases = self
                .load_aliases("goal", rowid)
                .with_context(|| format!("Failed to load aliases for goal '{text_id}'"))?;
            let ulid = parse_ulid(&text_id);
            let started_at = started_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            let target_stage = target_stage.map(|v| v as u8);

            goals.push(Goal {
                id: text_id,
                label,
                status,
                kind,
                started_at,
                description,
                completion_log,
                ulid,
                slug,
                aliases,
                rfc,
                target_stage,
            });
        }

        Ok(goals)
    }

    /// Load phase RFC attachments for the active (in-progress) phase.
    pub fn load_phase_rfcs_for_active_phase(&self) -> Result<Vec<PhaseRfc>> {
        self.load_phase_rfcs_for_active_phase_for_workspace(None)
    }

    /// Load phase RFC attachments for the workspace-scoped active phase.
    pub fn load_phase_rfcs_for_active_phase_for_workspace(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<Vec<PhaseRfc>> {
        match self.resolve_active_phase_rowid(workspace_root)? {
            Some(rowid) => self.load_phase_rfcs(rowid),
            None => Ok(Vec::new()),
        }
    }

    fn load_phase_rfcs(&self, phase_id: i64) -> Result<Vec<PhaseRfc>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT rfc_id, target, relation
                 FROM phase_rfcs WHERE phase_id = ? ORDER BY id",
            )
            .context("Failed to prepare phase_rfcs query")?;

        let rfcs = stmt
            .query_map([phase_id], |row| {
                Ok(PhaseRfc {
                    id: row.get(0)?,
                    target: row.get::<_, Option<i32>>(1)?.map(|v| v as u8),
                    relation: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "related".to_string()),
                })
            })
            .context("Failed to execute phase_rfcs query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read phase_rfc rows")?;

        Ok(rfcs)
    }

    /// Load aliases for an entity from the `entity_aliases` table.
    ///
    /// Note: `entity_id` is the INTEGER rowid from the entity's shadow table,
    /// not the `text_id`. This is because `entity_aliases` uses rowid for efficient
    /// foreign key relationships.
    fn load_aliases(&self, entity_type: &str, entity_id: i64) -> Result<Vec<String>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT alias FROM entity_aliases
                 WHERE entity_type = ? AND entity_id = ?",
            )
            .context("Failed to prepare aliases query")?;

        let aliases = stmt
            .query_map([entity_type, &entity_id.to_string()], |row| row.get(0))
            .context("Failed to execute aliases query")?
            .collect::<Result<Vec<String>, _>>()
            .context("Failed to read alias rows")?;

        Ok(aliases)
    }

    /// Load all ideas from the database.
    #[allow(clippy::type_complexity)]
    pub fn load_ideas(&self) -> Result<Vec<Idea>> {
        let conn = self.db.connection();

        // Query all ideas
        let mut stmt = conn
            .prepare(
                "SELECT id, text_id, title, description, status, created_at, source
                 FROM ideas ORDER BY created_at DESC",
            )
            .context("Failed to prepare ideas query")?;

        let idea_rows: Vec<(i64, String, String, Option<String>, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .context("Failed to execute ideas query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read idea rows")?;

        let mut ideas = Vec::with_capacity(idea_rows.len());
        for (rowid, text_id, title, description, status, created_at, source) in idea_rows {
            // Load tags for this idea
            let tags = self.load_idea_tags(rowid)?;
            // Load related tasks for this idea
            let related_tasks = self.load_idea_task_refs(rowid)?;

            ideas.push(Idea {
                id: text_id,
                title,
                description: description.unwrap_or_default(),
                status,
                created_at,
                source,
                tags,
                related_tasks,
            });
        }

        Ok(ideas)
    }

    /// Load a single idea by its `text_id`.
    #[allow(clippy::type_complexity)]
    pub fn load_idea_by_id(&self, text_id: &str) -> Result<Option<Idea>> {
        let conn = self.db.connection();

        let mut stmt = conn
            .prepare(
                "SELECT id, text_id, title, description, status, created_at, source
                 FROM ideas WHERE text_id = ?",
            )
            .context("Failed to prepare idea query")?;

        let row = stmt
            .query_row([text_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .optional()
            .context("Failed to query idea")?;

        match row {
            None => Ok(None),
            Some((rowid, id, title, description, status, created_at, source)) => {
                let tags = self.load_idea_tags(rowid)?;
                let related_tasks = self.load_idea_task_refs(rowid)?;
                Ok(Some(Idea {
                    id,
                    title,
                    description: description.unwrap_or_default(),
                    status,
                    created_at,
                    source,
                    tags,
                    related_tasks,
                }))
            }
        }
    }

    /// Load tags for an idea from the junction table.
    fn load_idea_tags(&self, idea_id: i64) -> Result<Vec<String>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare("SELECT tag FROM idea_tags WHERE idea_id = ?")
            .context("Failed to prepare idea_tags query")?;

        let tags = stmt
            .query_map([idea_id], |row| row.get(0))
            .context("Failed to execute idea_tags query")?
            .collect::<Result<Vec<String>, _>>()
            .context("Failed to read idea_tags rows")?;

        Ok(tags)
    }

    /// Load related task references for an idea from the junction table.
    fn load_idea_task_refs(&self, idea_id: i64) -> Result<Vec<String>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare("SELECT task_ref FROM idea_task_refs WHERE idea_id = ?")
            .context("Failed to prepare idea_task_refs query")?;

        let refs = stmt
            .query_map([idea_id], |row| row.get(0))
            .context("Failed to execute idea_task_refs query")?
            .collect::<Result<Vec<String>, _>>()
            .context("Failed to read idea_task_refs rows")?;

        Ok(refs)
    }

    /// List tasks for the active phase as (id, title, status) tuples.
    ///
    /// Returns tasks in the format expected by `list_tasks`: `goal_id::task_id`.
    pub fn list_active_phase_tasks(&self) -> Result<Vec<(String, String, String)>> {
        self.list_active_phase_tasks_for_workspace(None)
    }

    /// List tasks for the workspace-scoped active phase.
    pub fn list_active_phase_tasks_for_workspace(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<Vec<(String, String, String)>> {
        let conn = self.db.connection();

        let Some(phase_id) = self.resolve_active_phase_rowid(workspace_root)? else {
            return Ok(Vec::new());
        };

        // Load goals for this phase
        let mut goal_stmt = conn
            .prepare(
                "SELECT g.id, g.text_id FROM goals g WHERE g.phase_id = ? ORDER BY g.sort_key NULLS LAST, g.id",
            )
            .context("Failed to prepare goals query")?;

        let goals: Vec<(i64, String)> = goal_stmt
            .query_map([phase_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("Failed to query goals")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read goals")?;

        // Load tasks for each goal
        let mut task_stmt = conn
            .prepare(
                "SELECT t.text_id, t.title, t.status FROM tasks t WHERE t.goal_id = ? ORDER BY t.sort_key NULLS LAST, t.id",
            )
            .context("Failed to prepare tasks query")?;

        let mut result = Vec::new();
        for (goal_rowid, goal_text_id) in &goals {
            let tasks: Vec<(String, String, String)> = task_stmt
                .query_map([goal_rowid], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .context("Failed to query tasks")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to read tasks")?;

            for (task_id, title, status) in tasks {
                let composite_id = if !goal_text_id.is_empty() && !task_id.is_empty() {
                    format!("{goal_text_id}::{task_id}")
                } else if !task_id.is_empty() {
                    task_id
                } else {
                    title.clone()
                };
                result.push((composite_id, title, status));
            }
        }

        Ok(result)
    }

    /// Count tasks per goal for the active phase.
    pub fn count_tasks_per_goal(&self) -> Result<std::collections::HashMap<String, usize>> {
        self.count_tasks_per_goal_for_workspace(None)
    }

    /// Count tasks per goal for the workspace-scoped active phase.
    pub fn count_tasks_per_goal_for_workspace(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<std::collections::HashMap<String, usize>> {
        let conn = self.db.connection();

        let Some(phase_id) = self.resolve_active_phase_rowid(workspace_root)? else {
            return Ok(std::collections::HashMap::new());
        };

        let mut stmt = conn
            .prepare(
                "SELECT g.text_id, COUNT(t.id)
                 FROM goals g
                 LEFT JOIN tasks t ON t.goal_id = g.id
                 WHERE g.phase_id = ?1
                 GROUP BY g.id",
            )
            .context("Failed to prepare task count query")?;

        let rows: Vec<(String, usize)> = stmt
            .query_map([phase_id], |row| {
                Ok((row.get(0)?, row.get::<_, i64>(1)? as usize))
            })
            .context("Failed to query task counts")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read task counts")?;

        Ok(rows.into_iter().collect())
    }

    /// Collect all entity IDs (goal, task, rfc) belonging to the active phase.
    ///
    /// Returns a set of `(entity_type, entity_id)` pairs for relevance scoring.
    pub fn collect_active_phase_entity_ids(
        &self,
    ) -> Result<std::collections::HashSet<(String, String)>> {
        self.collect_active_phase_entity_ids_for_workspace(None)
    }

    /// Collect all entity IDs belonging to the workspace-scoped active phase.
    pub fn collect_active_phase_entity_ids_for_workspace(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<std::collections::HashSet<(String, String)>> {
        let conn = self.db.connection();

        let Some(phase_rowid) = self.resolve_active_phase_rowid(workspace_root)? else {
            return Ok(std::collections::HashSet::new());
        };

        let mut ids = std::collections::HashSet::new();

        // Goals
        let mut goal_stmt = conn
            .prepare("SELECT id, text_id FROM goals WHERE phase_id = ?")
            .context("Failed to prepare goals query")?;
        let goals: Vec<(i64, String)> = goal_stmt
            .query_map([phase_rowid], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("Failed to query goals")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read goals")?;

        for (goal_rowid, goal_text_id) in &goals {
            ids.insert(("goal".to_string(), goal_text_id.clone()));

            // Tasks for this goal
            let mut task_stmt = conn
                .prepare("SELECT text_id FROM tasks WHERE goal_id = ?")
                .context("Failed to prepare tasks query")?;
            let tasks: Vec<String> = task_stmt
                .query_map([goal_rowid], |row| row.get(0))
                .context("Failed to query tasks")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to read tasks")?;

            for task_id in tasks {
                ids.insert(("task".to_string(), task_id));
            }
        }

        // Phase RFCs
        let rfcs = self.load_phase_rfcs(phase_rowid)?;
        for rfc in rfcs {
            ids.insert(("rfc".to_string(), rfc.id));
        }

        Ok(ids)
    }

    /// Resolve the ancestor chain for an entity (task → goal → phase).
    ///
    /// Returns `(entity_type, entity_id)` pairs from immediate parent upward.
    /// - task: `[(goal, goal_id), (phase, phase_id)]`
    /// - goal: `[(phase, phase_id)]`
    /// - phase: `[]`
    /// - unknown: `[]`
    pub fn resolve_entity_tree(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Vec<(String, String)>> {
        let conn = self.db.connection();
        match entity_type {
            "task" => {
                let mut stmt = conn.prepare(
                    "SELECT g.text_id, p.text_id
                     FROM tasks t
                     JOIN goals g ON t.goal_id = g.id
                     JOIN phases p ON g.phase_id = p.id
                     WHERE t.text_id = ?",
                )?;
                let result: Option<(String, String)> = stmt
                    .query_row([entity_id], |row| Ok((row.get(0)?, row.get(1)?)))
                    .optional()?;
                Ok(match result {
                    Some((goal_id, phase_id)) => {
                        vec![
                            ("goal".to_string(), goal_id),
                            ("phase".to_string(), phase_id),
                        ]
                    }
                    None => vec![],
                })
            }
            "goal" => {
                let mut stmt = conn.prepare(
                    "SELECT p.text_id
                     FROM goals g
                     JOIN phases p ON g.phase_id = p.id
                     WHERE g.text_id = ?",
                )?;
                let result: Option<String> =
                    stmt.query_row([entity_id], |row| row.get(0)).optional()?;
                Ok(match result {
                    Some(phase_id) => vec![("phase".to_string(), phase_id)],
                    None => vec![],
                })
            }
            _ => Ok(vec![]),
        }
    }

    /// Load the status of an entity (goal or task) by its `text_id`.
    ///
    /// Returns `Some(status_string)` if the entity exists, `None` otherwise.
    pub fn load_entity_status(&self, entity_type: &str, entity_id: &str) -> Result<Option<String>> {
        let conn = self.db.connection();
        let query = match entity_type {
            "goal" => "SELECT status FROM goals WHERE text_id = ?",
            "task" => "SELECT status FROM tasks WHERE text_id = ?",
            _ => return Ok(None),
        };
        conn.query_row(query, [entity_id], |row| row.get(0))
            .optional()
            .context("Failed to query entity status")
    }

    /// Load full task details by `text_id`.
    ///
    /// Returns (`text_id`, title, status, notes, `started_at`, `completed_at`, `completion_log`).
    #[allow(clippy::type_complexity)]
    pub fn load_task_details(
        &self,
        text_id: &str,
    ) -> Result<
        Option<(
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>,
    > {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT text_id, title, status, notes, started_at, completed_at, completion_log
                 FROM tasks WHERE text_id = ?",
            )
            .context("Failed to prepare task details query")?;

        stmt.query_row([text_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .optional()
        .context("Failed to query task details")
    }

    /// Load log entries for a task.
    pub fn load_task_logs(&self, task_text_id: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.db.connection();
        let Some(task_id) = conn
            .query_row(
                "SELECT id FROM tasks WHERE text_id = ?",
                [task_text_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("Failed to query task")?
        else {
            return Ok(Vec::new());
        };

        let mut stmt = conn
            .prepare(
                "SELECT kind, message, created_at FROM task_logs
                 WHERE task_id = ? ORDER BY id",
            )
            .context("Failed to prepare task logs query")?;

        let logs = stmt
            .query_map([task_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .context("Failed to query task logs")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read task logs")?;

        Ok(logs)
    }

    /// Load verification results for a task.
    #[allow(clippy::type_complexity)]
    pub fn load_task_verifications(
        &self,
        task_text_id: &str,
    ) -> Result<Vec<(String, Option<String>, String, Option<String>, String)>> {
        let conn = self.db.connection();
        let Some(task_id) = conn
            .query_row(
                "SELECT id FROM tasks WHERE text_id = ?",
                [task_text_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("Failed to query task")?
        else {
            return Ok(Vec::new());
        };

        let mut stmt = conn
            .prepare(
                "SELECT kind, command, result, details, created_at FROM task_verifications
                 WHERE task_id = ? ORDER BY id",
            )
            .context("Failed to prepare task verifications query")?;

        let verifications = stmt
            .query_map([task_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .context("Failed to query task verifications")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read task verifications")?;

        Ok(verifications)
    }

    /// Load all inbox items from the database.
    pub fn load_inbox(&self) -> Result<Vec<InboxItem>> {
        let conn = self.db.connection();

        let mut stmt = conn
            .prepare(
                "SELECT text_id, created_at, updated_at, status,
                        entity_type, entity_id, source, intent, priority, confidence,
                        agent_id, subject, body, action_json, resolution
                 FROM inbox ORDER BY created_at DESC",
            )
            .context("Failed to prepare inbox query")?;

        let items = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,          // text_id
                    row.get::<_, String>(1)?,          // created_at
                    row.get::<_, Option<String>>(2)?,  // updated_at
                    row.get::<_, String>(3)?,          // status
                    row.get::<_, String>(4)?,          // entity_type
                    row.get::<_, Option<String>>(5)?,  // entity_id
                    row.get::<_, String>(6)?,          // source
                    row.get::<_, String>(7)?,          // intent
                    row.get::<_, String>(8)?,          // priority
                    row.get::<_, Option<String>>(9)?,  // confidence
                    row.get::<_, Option<String>>(10)?, // agent_id
                    row.get::<_, String>(11)?,         // subject
                    row.get::<_, String>(12)?,         // body
                    row.get::<_, Option<String>>(13)?, // action_json
                    row.get::<_, Option<String>>(14)?, // resolution
                ))
            })
            .context("Failed to execute inbox query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read inbox rows")?;

        let mut result = Vec::with_capacity(items.len());
        for (
            text_id,
            created_at,
            updated_at,
            status_str,
            entity_type,
            entity_id,
            source_str,
            intent_str,
            priority_str,
            confidence_str,
            agent_id,
            subject,
            body,
            action_json,
            resolution,
        ) in items
        {
            let status: InboxItemStatus = status_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let source: InboxSource = source_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let intent: InboxIntent = intent_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let priority: InboxPriority =
                priority_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let confidence: Option<InboxConfidence> = confidence_str
                .map(|s| s.parse().map_err(|e| anyhow::anyhow!("{e}")))
                .transpose()?;
            let action = action_json
                .map(|raw| serde_json::from_str(&raw).with_context(|| "Invalid inbox action_json"))
                .transpose()?;

            result.push(InboxItem {
                id: text_id,
                created: created_at,
                status,
                entity_type,
                entity_id,
                source,
                intent,
                priority,
                confidence,
                agent_id,
                subject: display_inbox_subject(subject),
                body,
                action,
                updated: updated_at,
                resolution,
            });
        }

        Ok(result)
    }

    /// Load inbox items from the database with optional SQL-side filtering.
    pub fn load_inbox_filtered(
        &self,
        status: Option<&str>,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        source: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<InboxItem>> {
        let conn = self.db.connection();

        let mut query = String::from(
            "SELECT text_id, created_at, updated_at, status,
                    entity_type, entity_id, source, intent, priority, confidence,
                    agent_id, subject, body, action_json, resolution
             FROM inbox",
        );
        let mut where_clauses = Vec::new();
        let mut params = Vec::new();

        if let Some(status) = status {
            where_clauses.push("status = ?");
            params.push(exosuit_storage::rusqlite::types::Value::Text(
                status.to_string(),
            ));
        }

        if let Some(entity_type) = entity_type {
            where_clauses.push("entity_type = ?");
            params.push(exosuit_storage::rusqlite::types::Value::Text(
                entity_type.to_string(),
            ));
        }

        if let Some(entity_id) = entity_id {
            where_clauses.push("entity_id = ?");
            params.push(exosuit_storage::rusqlite::types::Value::Text(
                entity_id.to_string(),
            ));
        }

        if let Some(source) = source {
            where_clauses.push("source = ?");
            params.push(exosuit_storage::rusqlite::types::Value::Text(
                source.to_string(),
            ));
        }

        if !where_clauses.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&where_clauses.join(" AND "));
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = limit {
            query.push_str(" LIMIT ?");
            params.push(exosuit_storage::rusqlite::types::Value::Integer(
                i64::try_from(limit).context("Inbox limit exceeds i64")?,
            ));
        }

        let mut stmt = conn
            .prepare(&query)
            .context("Failed to prepare filtered inbox query")?;

        let items = stmt
            .query_map(exosuit_storage::rusqlite::params_from_iter(params), |row| {
                Ok((
                    row.get::<_, String>(0)?,          // text_id
                    row.get::<_, String>(1)?,          // created_at
                    row.get::<_, Option<String>>(2)?,  // updated_at
                    row.get::<_, String>(3)?,          // status
                    row.get::<_, String>(4)?,          // entity_type
                    row.get::<_, Option<String>>(5)?,  // entity_id
                    row.get::<_, String>(6)?,          // source
                    row.get::<_, String>(7)?,          // intent
                    row.get::<_, String>(8)?,          // priority
                    row.get::<_, Option<String>>(9)?,  // confidence
                    row.get::<_, Option<String>>(10)?, // agent_id
                    row.get::<_, String>(11)?,         // subject
                    row.get::<_, String>(12)?,         // body
                    row.get::<_, Option<String>>(13)?, // action_json
                    row.get::<_, Option<String>>(14)?, // resolution
                ))
            })
            .context("Failed to execute filtered inbox query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read filtered inbox rows")?;

        let mut result = Vec::with_capacity(items.len());
        for (
            text_id,
            created_at,
            updated_at,
            status_str,
            entity_type,
            entity_id,
            source_str,
            intent_str,
            priority_str,
            confidence_str,
            agent_id,
            subject,
            body,
            action_json,
            resolution,
        ) in items
        {
            let status: InboxItemStatus = status_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let source: InboxSource = source_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let intent: InboxIntent = intent_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let priority: InboxPriority =
                priority_str.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
            let confidence: Option<InboxConfidence> = confidence_str
                .map(|s| s.parse().map_err(|e| anyhow::anyhow!("{e}")))
                .transpose()?;
            let action = action_json
                .map(|raw| serde_json::from_str(&raw).with_context(|| "Invalid inbox action_json"))
                .transpose()?;

            result.push(InboxItem {
                id: text_id,
                created: created_at,
                status,
                entity_type,
                entity_id,
                source,
                intent,
                priority,
                confidence,
                agent_id,
                subject: display_inbox_subject(subject),
                body,
                action,
                updated: updated_at,
                resolution,
            });
        }

        Ok(result)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Completion guard
    // ═══════════════════════════════════════════════════════════════════════

    /// Check whether a completion claim exists for an entity.
    ///
    /// Returns the claim status, distinguishing between human and agent claims.
    /// Human claims pass the completion gate immediately; agent claims require
    /// human acknowledgment first.
    pub fn has_completion_claim(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<CompletionClaimStatus> {
        let conn = self.db.connection();
        let row: Option<(Option<String>, String)> = conn
            .query_row(
                "SELECT agent_id, status FROM inbox \
                 WHERE entity_type = ?1 AND entity_id = ?2 \
                   AND intent = 'claim' \
                                     AND status IN ('pending', 'acknowledged') \
                                 ORDER BY created_at DESC \
                 LIMIT 1",
                [entity_type, entity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        Ok(match row {
            None => CompletionClaimStatus::NoClaim,
            Some((None, _)) => CompletionClaimStatus::HumanClaim,
            Some((Some(_), status)) if status == "acknowledged" => {
                CompletionClaimStatus::AgentClaimAcknowledged
            }
            Some((Some(_), _)) => CompletionClaimStatus::AgentClaimPending,
        })
    }

    /// Load active completion claims for a single entity as a derived digest.
    pub fn load_completion_outcome_digest(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<PhaseCompletionDigest> {
        let mut digests =
            self.load_completion_outcome_digests_for_entities(&[(entity_type, entity_id)])?;
        Ok(digests.pop().unwrap_or_else(|| PhaseCompletionDigest {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            claims: vec![],
        }))
    }

    /// Load active completion claims for entities as a derived read over inbox rows.
    pub fn load_completion_outcome_digests_for_entities(
        &self,
        entities: &[(&str, &str)],
    ) -> Result<Vec<PhaseCompletionDigest>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT text_id, entity_type, entity_id, status, source, priority, confidence,
                        agent_id, subject, body, created_at
                 FROM inbox
                 WHERE entity_type = ?1 AND entity_id = ?2
                   AND intent = 'claim'
                   AND status IN ('pending', 'acknowledged')
                 ORDER BY created_at DESC, id DESC",
            )
            .context("Failed to prepare completion digest query")?;

        let mut digests = Vec::with_capacity(entities.len());
        for (entity_type, entity_id) in entities {
            let claims = stmt
                .query_map([*entity_type, *entity_id], |row| {
                    Ok(CompletionOutcomeDigest {
                        id: row.get(0)?,
                        entity_type: row.get(1)?,
                        entity_id: row.get(2)?,
                        status: row.get(3)?,
                        source: row.get(4)?,
                        priority: row.get(5)?,
                        confidence: row.get(6)?,
                        agent_id: row.get(7)?,
                        subject: display_inbox_subject(row.get(8)?),
                        body: row.get(9)?,
                        created: row.get(10)?,
                    })
                })
                .context("Failed to query completion digest")?
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read completion digest rows")?;

            digests.push(PhaseCompletionDigest {
                entity_type: (*entity_type).to_string(),
                entity_id: (*entity_id).to_string(),
                claims,
            });
        }

        Ok(digests)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // RFC metadata
    // ═══════════════════════════════════════════════════════════════════════

    /// Load all RFC metadata records from the database.
    pub fn load_rfcs(&self) -> Result<Vec<RfcRecord>> {
        let conn = self.db.connection();
        let mut stmt = conn
            .prepare(
                "SELECT text_id, rfc_number, title, stage, status, feature, slug, file_path,
                        superseded_by, supersedes, withdrawal_reason, archived_reason,
                        consolidated_into
                 FROM rfcs ORDER BY rfc_number ASC",
            )
            .context("Failed to prepare rfcs query")?;

        let records = stmt
            .query_map([], |row| {
                Ok(RfcRecord {
                    text_id: row.get(0)?,
                    rfc_number: row.get(1)?,
                    title: row.get(2)?,
                    stage: row.get(3)?,
                    status: row.get(4)?,
                    feature: row.get(5)?,
                    slug: row.get(6)?,
                    file_path: row.get(7)?,
                    superseded_by: row.get(8)?,
                    supersedes: row.get(9)?,
                    withdrawal_reason: row.get(10)?,
                    archived_reason: row.get(11)?,
                    consolidated_into: row.get(12)?,
                })
            })
            .context("Failed to query rfcs")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read rfcs rows")?;

        Ok(records)
    }

    /// Load all RFC metadata records in legacy display order.
    ///
    /// This matches the old filesystem scan ordering used by `get_rfcs()`:
    /// stage descending, then RFC number ascending.
    pub fn load_rfcs_for_display(&self) -> Result<Vec<RfcRecord>> {
        let mut records = self.load_rfcs()?;
        records.sort_by(|a, b| b.stage.cmp(&a.stage).then(a.rfc_number.cmp(&b.rfc_number)));
        Ok(records)
    }

    /// Find an RFC by its numeric ID.
    pub fn load_rfc_by_number(&self, rfc_number: i64) -> Result<Option<RfcRecord>> {
        let conn = self.db.connection();
        let result = conn
            .query_row(
                "SELECT text_id, rfc_number, title, stage, status, feature, slug, file_path,
                        superseded_by, supersedes, withdrawal_reason, archived_reason,
                        consolidated_into
                 FROM rfcs WHERE rfc_number = ?",
                [rfc_number],
                |row| {
                    Ok(RfcRecord {
                        text_id: row.get(0)?,
                        rfc_number: row.get(1)?,
                        title: row.get(2)?,
                        stage: row.get(3)?,
                        status: row.get(4)?,
                        feature: row.get(5)?,
                        slug: row.get(6)?,
                        file_path: row.get(7)?,
                        superseded_by: row.get(8)?,
                        supersedes: row.get(9)?,
                        withdrawal_reason: row.get(10)?,
                        archived_reason: row.get(11)?,
                        consolidated_into: row.get(12)?,
                    })
                },
            )
            .optional()
            .context("Failed to query rfc by number")?;

        Ok(result)
    }

    /// Load the latest RFC document snapshot for one workspace.
    pub fn load_rfc_workspace_snapshot(
        &self,
        workspace_root: &str,
    ) -> Result<Option<RfcWorkspaceSnapshot>> {
        self.db
            .connection()
            .query_row(
                "SELECT workspace_root, branch_name, head_oid, document_digest,
                        canonical_ref, canonical_oid, observed_at
                 FROM rfc_workspace_snapshots
                 WHERE workspace_root = ?1",
                [workspace_root],
                |row| {
                    Ok(RfcWorkspaceSnapshot {
                        workspace_root: row.get(0)?,
                        branch_name: row.get(1)?,
                        head_oid: row.get(2)?,
                        document_digest: row.get(3)?,
                        canonical_ref: row.get(4)?,
                        canonical_oid: row.get(5)?,
                        observed_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .context("Failed to load RFC workspace snapshot")
    }

    /// Load parsed RFC observations for one workspace snapshot.
    pub fn load_rfc_workspace_observations(
        &self,
        workspace_root: &str,
    ) -> Result<Vec<RfcWorkspaceObservation>> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT workspace_root, text_id, rfc_number, title, stage, stage_source,
                        status, feature, feature_declared, slug, file_path,
                        superseded_by, superseded_by_declared, supersedes,
                        supersedes_declared, withdrawal_reason,
                        withdrawal_reason_declared, archived_reason,
                        archived_reason_declared, consolidated_into,
                        consolidated_into_declared, branch_name, head_oid, observed_at
                 FROM rfc_workspace_observations
                 WHERE workspace_root = ?1
                 ORDER BY rfc_number ASC, file_path ASC",
            )
            .context("Failed to prepare RFC workspace observations query")?;

        stmt.query_map([workspace_root], |row| {
            Ok(RfcWorkspaceObservation {
                workspace_root: row.get(0)?,
                text_id: row.get(1)?,
                rfc_number: row.get(2)?,
                title: row.get(3)?,
                stage: row.get(4)?,
                stage_source: row.get(5)?,
                status: row.get(6)?,
                feature: row.get(7)?,
                feature_declared: row.get(8)?,
                slug: row.get(9)?,
                file_path: row.get(10)?,
                superseded_by: row.get(11)?,
                superseded_by_declared: row.get(12)?,
                supersedes: row.get(13)?,
                supersedes_declared: row.get(14)?,
                withdrawal_reason: row.get(15)?,
                withdrawal_reason_declared: row.get(16)?,
                archived_reason: row.get(17)?,
                archived_reason_declared: row.get(18)?,
                consolidated_into: row.get(19)?,
                consolidated_into_declared: row.get(20)?,
                branch_name: row.get(21)?,
                head_oid: row.get(22)?,
                observed_at: row.get(23)?,
            })
        })
        .context("Failed to query RFC workspace observations")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read RFC workspace observations")
    }

    /// Load persisted RFC diagnostics for one workspace snapshot.
    pub fn load_rfc_workspace_diagnostics(
        &self,
        workspace_root: &str,
    ) -> Result<Vec<RfcWorkspaceDiagnostic>> {
        let mut stmt = self
            .db
            .connection()
            .prepare(
                "SELECT workspace_root, file_path, diagnostic_code, text_id,
                        rfc_number, message, observed_at
                 FROM rfc_workspace_diagnostics
                 WHERE workspace_root = ?1
                 ORDER BY file_path ASC, diagnostic_code ASC",
            )
            .context("Failed to prepare RFC workspace diagnostics query")?;

        stmt.query_map([workspace_root], |row| {
            Ok(RfcWorkspaceDiagnostic {
                workspace_root: row.get(0)?,
                file_path: row.get(1)?,
                diagnostic_code: row.get(2)?,
                text_id: row.get(3)?,
                rfc_number: row.get(4)?,
                message: row.get(5)?,
                observed_at: row.get(6)?,
            })
        })
        .context("Failed to query RFC workspace diagnostics")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read RFC workspace diagnostics")
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Import methods: TOML → SQLite migration
    // ═══════════════════════════════════════════════════════════════════════

    /// Import ideas from a parsed `IdeasFile` into the database.
    ///
    /// This is used for one-time migration from TOML to `SQLite`.
    /// Existing ideas with the same `text_id` will be skipped (upsert semantics).
    pub fn import_ideas(&self, ideas: &[Idea]) -> Result<ImportResult> {
        let conn = self.db.connection();
        let _defensive_guard = defensive_mode_disabled(conn)?;
        let mut imported = 0;
        let mut skipped = 0;

        for idea in ideas {
            // Check if idea already exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM ideas_data WHERE text_id = ?",
                    [&idea.id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                skipped += 1;
                continue;
            }

            // Insert the idea
            let description: Option<&str> = if idea.description.is_empty() {
                None
            } else {
                Some(&idea.description)
            };
            conn.execute(
                "INSERT INTO ideas_data (text_id, title, description, status, created_at, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (
                    &idea.id,
                    &idea.title,
                    description,
                    &idea.status,
                    &idea.created_at,
                    &idea.source,
                ),
            )
            .with_context(|| format!("Failed to insert idea '{}'", idea.id))?;

            // Get the rowid for junction tables
            let rowid: i64 = conn.last_insert_rowid();

            // Insert tags
            for tag in &idea.tags {
                conn.execute(
                    "INSERT INTO idea_tags (idea_id, tag) VALUES (?1, ?2)",
                    (rowid, tag.as_str()),
                )
                .with_context(|| {
                    format!("Failed to insert tag '{}' for idea '{}'", tag, idea.id)
                })?;
            }

            // Insert related task refs
            for task_ref in &idea.related_tasks {
                conn.execute(
                    "INSERT INTO idea_task_refs (idea_id, task_ref) VALUES (?1, ?2)",
                    (rowid, task_ref.as_str()),
                )
                .with_context(|| {
                    format!(
                        "Failed to insert task_ref '{}' for idea '{}'",
                        task_ref, idea.id
                    )
                })?;
            }

            imported += 1;
        }

        Ok(ImportResult { imported, skipped })
    }

    /// Import inbox items from a parsed `InboxFile` into the database.
    ///
    /// This is used for one-time migration from TOML to `SQLite`.
    /// Existing items with the same `text_id` will be skipped (upsert semantics).
    pub fn import_inbox(&self, items: &[InboxItem]) -> Result<ImportResult> {
        let conn = self.db.connection();
        let _defensive_guard = defensive_mode_disabled(conn)?;
        let mut imported = 0;
        let mut skipped = 0;

        for item in items {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM inbox_data WHERE text_id = ?",
                    [&item.id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                skipped += 1;
                continue;
            }

            conn.execute(
                "INSERT INTO inbox_data (
                    text_id, created_at, updated_at, status,
                    entity_type, entity_id, source, intent, priority, confidence,
                    agent_id, subject, body, action_json, resolution
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                (
                    &item.id,
                    &item.created,
                    item.updated.as_deref(),
                    item.status.as_str(),
                    &item.entity_type,
                    item.entity_id.as_deref(),
                    item.source.as_str(),
                    item.intent.as_str(),
                    item.priority.as_str(),
                    item.confidence.map(|c| c.as_str()),
                    item.agent_id.as_deref(),
                    &item.subject,
                    &item.body,
                    item.action.as_ref().map(serde_json::Value::to_string),
                    item.resolution.as_deref(),
                ),
            )
            .with_context(|| format!("Failed to insert inbox item '{}'", item.id))?;

            imported += 1;
        }

        Ok(ImportResult { imported, skipped })
    }

    /// Normalize a status string to canonical form.
    ///
    /// Legacy TOML files used inconsistent status names. This maps them
    /// to the canonical values expected by the `SQLite` CHECK constraints.
    fn normalize_status(status: &str) -> &str {
        match status {
            "active" | "in_progress" => "in-progress",
            "complete" => "completed",
            "bankrupt" => "abandoned",
            _ => status,
        }
    }

    /// Import the execution hierarchy (epochs → phases → goals → tasks) from `ExoState`.
    ///
    /// This is used for one-time migration from TOML to `SQLite`.
    /// Existing entities with the same `text_id` will be skipped.
    ///
    /// Returns the total count of imported entities across all levels.
    pub fn import_plan(&self, state: &ExoState) -> Result<PlanImportResult> {
        let conn = self.db.connection();
        let _defensive_guard = defensive_mode_disabled(conn)?;
        let mut result = PlanImportResult::default();

        let sort_keys = generate_sort_keys(state.epochs.len());
        for (i, epoch) in state.epochs.iter().enumerate() {
            // Check if epoch already exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM epochs_data WHERE text_id = ?",
                    [&epoch.id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                result.epochs_skipped += 1;
                // Backfill sort_key if it's the zero-padded migration default
                let epoch_rowid: i64 = conn.query_row(
                    "SELECT id FROM epochs_data WHERE text_id = ?",
                    [&epoch.id],
                    |row: &Row| row.get(0),
                )?;
                conn.execute(
                    "UPDATE epochs_data SET sort_key = ?1 WHERE id = ?2 AND sort_key LIKE '0%'",
                    (&sort_keys[i], epoch_rowid),
                )?;
                // Still need to check phases/goals/tasks in case they're new
                self.import_phases(conn, epoch_rowid, &epoch.phases, &mut result)?;
                continue;
            }

            // Insert epoch
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug, reviewed, sort_key)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    &epoch.id,
                    &epoch.title,
                    epoch.slug.as_deref(),
                    i32::from(epoch.reviewed),
                    &sort_keys[i],
                ),
            )
            .with_context(|| format!("Failed to insert epoch '{}'", epoch.id))?;

            let epoch_rowid = conn.last_insert_rowid();

            // Insert aliases
            for alias in &epoch.aliases {
                conn.execute(
                    "INSERT OR IGNORE INTO entity_aliases (entity_type, entity_id, alias)
                     VALUES ('epoch', ?1, ?2)",
                    (epoch_rowid, alias.as_str()),
                )
                .with_context(|| {
                    format!(
                        "Failed to insert alias '{}' for epoch '{}'",
                        alias, epoch.id
                    )
                })?;
            }

            result.epochs_imported += 1;

            // Import phases for this epoch
            self.import_phases(conn, epoch_rowid, &epoch.phases, &mut result)?;
        }

        Ok(result)
    }

    fn import_phases(
        &self,
        conn: &Connection,
        epoch_rowid: i64,
        phases: &[Phase],
        result: &mut PlanImportResult,
    ) -> Result<()> {
        let sort_keys = generate_sort_keys(phases.len());
        for (i, phase) in phases.iter().enumerate() {
            // Check if phase already exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM phases_data WHERE text_id = ?",
                    [&phase.id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                result.phases_skipped += 1;
                // Backfill sort_key if NULL (from pre-V007 data or prior import)
                let phase_rowid: i64 = conn.query_row(
                    "SELECT id FROM phases_data WHERE text_id = ?",
                    [&phase.id],
                    |row: &Row| row.get(0),
                )?;
                conn.execute(
                    "UPDATE phases_data SET sort_key = ?1 WHERE id = ?2 AND sort_key IS NULL",
                    (&sort_keys[i], phase_rowid),
                )?;
                // Still need to check goals/tasks
                self.import_goals(conn, phase_rowid, &phase.goals, result)?;
                self.import_phase_rfcs(conn, phase_rowid, &phase.rfcs)?;
                continue;
            }

            // Insert phase (normalize status: active→in-progress, bankrupt→abandoned)
            let normalized_status = Self::normalize_status(&phase.status);
            conn.execute(
                "INSERT INTO phases_data (text_id, title, status, epoch_id, kind, slug, sort_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    &phase.id,
                    &phase.title,
                    normalized_status,
                    epoch_rowid,
                    phase.kind.as_str(),
                    phase.slug.as_deref(),
                    &sort_keys[i],
                ),
            )
            .with_context(|| format!("Failed to insert phase '{}'", phase.id))?;

            let phase_rowid = conn.last_insert_rowid();

            // Insert aliases
            for alias in &phase.aliases {
                conn.execute(
                    "INSERT OR IGNORE INTO entity_aliases (entity_type, entity_id, alias)
                     VALUES ('phase', ?1, ?2)",
                    (phase_rowid, alias.as_str()),
                )
                .with_context(|| {
                    format!(
                        "Failed to insert alias '{}' for phase '{}'",
                        alias, phase.id
                    )
                })?;
            }

            result.phases_imported += 1;

            // Import goals for this phase
            self.import_goals(conn, phase_rowid, &phase.goals, result)?;

            // Import phase RFCs
            self.import_phase_rfcs(conn, phase_rowid, &phase.rfcs)?;
        }

        Ok(())
    }

    fn import_goals(
        &self,
        conn: &Connection,
        phase_rowid: i64,
        goals: &[Goal],
        result: &mut PlanImportResult,
    ) -> Result<()> {
        let sort_keys = generate_sort_keys(goals.len());
        for (i, goal) in goals.iter().enumerate() {
            // Check if goal already exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM goals_data WHERE text_id = ?",
                    [&goal.id],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                result.goals_skipped += 1;
                // Backfill sort_key if NULL (from pre-V007 data or prior import)
                let goal_rowid: i64 = conn.query_row(
                    "SELECT id FROM goals_data WHERE text_id = ?",
                    [&goal.id],
                    |row: &Row| row.get(0),
                )?;
                conn.execute(
                    "UPDATE goals_data SET sort_key = ?1 WHERE id = ?2 AND sort_key IS NULL",
                    (&sort_keys[i], goal_rowid),
                )?;
                // Still need to check tasks
                self.import_tasks(conn, goal_rowid, goal, result)?;
                continue;
            }

            // Insert goal (normalize status: active→in-progress, bankrupt→abandoned)
            let normalized_status = Self::normalize_status(&goal.status);
            conn.execute(
                "INSERT INTO goals_data (
                    text_id, label, status, phase_id, kind, rfc, target_stage,
                    started_at, description, completion_log, slug, sort_key
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                (
                    &goal.id,
                    &goal.label,
                    normalized_status,
                    phase_rowid,
                    goal.kind.as_deref(),
                    goal.rfc.as_deref(),
                    goal.target_stage,
                    goal.started_at.map(|dt| dt.to_rfc3339()),
                    goal.description.as_deref(),
                    goal.completion_log.as_deref(),
                    goal.slug.as_deref(),
                    &sort_keys[i],
                ),
            )
            .with_context(|| format!("Failed to insert goal '{}'", goal.id))?;

            let goal_rowid = conn.last_insert_rowid();

            // Insert aliases
            for alias in &goal.aliases {
                conn.execute(
                    "INSERT OR IGNORE INTO entity_aliases (entity_type, entity_id, alias)
                     VALUES ('goal', ?1, ?2)",
                    (goal_rowid, alias.as_str()),
                )
                .with_context(|| {
                    format!("Failed to insert alias '{}' for goal '{}'", alias, goal.id)
                })?;
            }

            result.goals_imported += 1;

            // Import tasks for this goal
            self.import_tasks(conn, goal_rowid, goal, result)?;
        }

        Ok(())
    }

    const fn import_tasks(
        &self,
        conn: &Connection,
        goal_rowid: i64,
        _goal: &Goal,
        result: &mut PlanImportResult,
    ) -> Result<()> {
        // Tasks are loaded from SQLite task tables.
        // The Goal struct doesn't contain tasks directly.
        // This method is currently a no-op because task import is handled elsewhere.
        //
        // For now, we just update the result to indicate no tasks were imported.
        let _ = (conn, goal_rowid, result);
        Ok(())
    }

    fn import_phase_rfcs(
        &self,
        conn: &Connection,
        phase_rowid: i64,
        rfcs: &[PhaseRfc],
    ) -> Result<()> {
        for rfc in rfcs {
            // Check if this phase-rfc association already exists
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM phase_rfcs_data WHERE phase_id = ? AND rfc_id = ?",
                    (phase_rowid, &rfc.id),
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if exists {
                continue;
            }

            // Determine relation type
            let relation = if rfc.target.is_some() {
                "driving"
            } else {
                "related"
            };

            conn.execute(
                "INSERT INTO phase_rfcs_data (phase_id, rfc_id, target, relation)
                 VALUES (?1, ?2, ?3, ?4)",
                (phase_rowid, &rfc.id, rfc.target, relation),
            )
            .with_context(|| {
                format!(
                    "Failed to insert phase_rfc for phase {} and RFC {}",
                    phase_rowid, rfc.id
                )
            })?;
        }

        Ok(())
    }
}

/// Generate sort keys for an ordered list of items.
///
/// Produces lexicographically sortable strings using fractional indexing,
/// preserving the original array order when stored in `SQLite`.
fn generate_sort_keys(count: usize) -> Vec<String> {
    let mut keys = Vec::with_capacity(count);
    let mut prev: Option<FractionalIndex> = None;
    for _ in 0..count {
        let key = match &prev {
            None => FractionalIndex::default(),
            Some(p) => FractionalIndex::new_after(p),
        };
        keys.push(key.to_string());
        prev = Some(key);
    }
    keys
}

/// Result of a plan import operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanImportResult {
    pub epochs_imported: usize,
    pub epochs_skipped: usize,
    pub phases_imported: usize,
    pub phases_skipped: usize,
    pub goals_imported: usize,
    pub goals_skipped: usize,
    pub tasks_imported: usize,
    pub tasks_skipped: usize,
}

impl PlanImportResult {
    /// Total number of entities imported.
    pub const fn total_imported(&self) -> usize {
        self.epochs_imported + self.phases_imported + self.goals_imported + self.tasks_imported
    }

    /// Total number of entities skipped.
    pub const fn total_skipped(&self) -> usize {
        self.epochs_skipped + self.phases_skipped + self.goals_skipped + self.tasks_skipped
    }
}

/// Result of an import operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportResult {
    /// Number of items successfully imported.
    pub imported: usize,
    /// Number of items skipped (already exist).
    pub skipped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_loader_round_trip() -> Result<()> {
        // 1. Create in-memory database via SqliteLoader
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        // 2. Insert test data: 1 epoch, 1 active phase, 1 goal with alias
        conn.execute(
            "INSERT INTO epochs_data (text_id, title, slug, reviewed)
             VALUES ('01ABC123', 'Test Epoch', 'test-epoch', 0)",
            [],
        )?;

        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind, slug)
             VALUES ('01DEF456', 'Test Phase', 'in-progress', 1, 'regular', 'test-phase')",
            [],
        )?;

        conn.execute(
            "INSERT INTO goals_data (text_id, label, status, phase_id, slug)
             VALUES ('01GHI789', 'Test Goal', 'pending', 1, 'test-goal')",
            [],
        )?;

        // Add an alias for the goal
        conn.execute(
            "INSERT INTO entity_aliases (entity_type, entity_id, alias)
             VALUES ('goal', 1, 'my-alias')",
            [],
        )?;

        // 3. Load via SqliteLoader
        let state = loader.load_state()?;

        // 4. Assert find_active_phase() returns correct phase
        let active = state.find_active_phase();
        assert!(active.is_some());
        let active = active.unwrap();
        assert_eq!(active.phase.id, "01DEF456");
        assert_eq!(active.phase.status, "in-progress");

        // 5. Assert find_goal_by_id("my-alias") resolves via alias
        let goal_info = state.find_goal_by_id("my-alias");
        assert!(goal_info.is_some());
        let goal_info = goal_info.unwrap();
        assert_eq!(goal_info.goal.id, "01GHI789");
        assert_eq!(goal_info.goal.label, "Test Goal");

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_loads_phase_rfcs() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'pending', 1, 'regular')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phase_rfcs_data (phase_id, rfc_id, target, relation)
             VALUES (1, '00238', 2, 'driving')",
            [],
        )?;

        let state = loader.load_state()?;
        let phase = &state.epochs[0].phases[0];

        assert_eq!(phase.rfcs.len(), 1);
        assert_eq!(phase.rfcs[0].id, "00238");
        assert_eq!(phase.rfcs[0].target, Some(2));
        assert_eq!(phase.rfcs[0].relation, "driving");

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_loads_multiple_aliases() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'pending', 1, 'regular')",
            [],
        )?;
        conn.execute(
            "INSERT INTO goals_data (text_id, label, status, phase_id)
             VALUES ('g1', 'Goal', 'pending', 1)",
            [],
        )?;

        // Add multiple aliases
        conn.execute(
            "INSERT INTO entity_aliases (entity_type, entity_id, alias)
             VALUES ('goal', 1, 'alias-one')",
            [],
        )?;
        conn.execute(
            "INSERT INTO entity_aliases (entity_type, entity_id, alias)
             VALUES ('goal', 1, 'alias-two')",
            [],
        )?;

        let state = loader.load_state()?;
        let goal = &state.epochs[0].phases[0].goals[0];

        assert_eq!(goal.aliases.len(), 2);
        assert!(goal.aliases.contains(&"alias-one".to_string()));
        assert!(goal.aliases.contains(&"alias-two".to_string()));

        // Both aliases should resolve
        assert!(state.find_goal_by_id("alias-one").is_some());
        assert!(state.find_goal_by_id("alias-two").is_some());

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_empty_database() -> Result<()> {
        // An empty database should return an empty ExoState, not an error
        let loader = SqliteLoader::open_memory()?;
        let state = loader.load_state()?;

        assert!(state.epochs.is_empty());
        assert!(state.meta.is_some()); // Meta should still be populated

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_epoch_without_phases() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        // Insert epoch with no phases
        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Empty Epoch')",
            [],
        )?;

        let state = loader.load_state()?;

        assert_eq!(state.epochs.len(), 1);
        assert_eq!(state.epochs[0].title, "Empty Epoch");
        assert!(state.epochs[0].phases.is_empty());

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_phase_without_goals() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Empty Phase', 'pending', 1, 'regular')",
            [],
        )?;

        let state = loader.load_state()?;

        assert_eq!(state.epochs[0].phases.len(), 1);
        assert_eq!(state.epochs[0].phases[0].title, "Empty Phase");
        assert!(state.epochs[0].phases[0].goals.is_empty());

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_goal_with_all_optional_fields() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'pending', 1, 'regular')",
            [],
        )?;
        conn.execute(
            "INSERT INTO goals_data (text_id, label, status, phase_id, kind, rfc, 
                    target_stage, started_at, description, completion_log, slug)
             VALUES ('g1', 'Full Goal', 'completed', 1, 'strike', '00238', 2,
                    '2025-02-21T10:00:00Z', 'A description', 'Done!', 'full-goal')",
            [],
        )?;

        let state = loader.load_state()?;
        let goal = &state.epochs[0].phases[0].goals[0];

        assert_eq!(goal.label, "Full Goal");
        assert_eq!(goal.status, "completed");
        assert_eq!(goal.kind, Some("strike".to_string()));
        assert_eq!(goal.rfc, Some("00238".to_string()));
        assert_eq!(goal.target_stage, Some(2));
        assert!(goal.started_at.is_some());
        assert_eq!(goal.description, Some("A description".to_string()));
        assert_eq!(goal.completion_log, Some("Done!".to_string()));
        assert_eq!(goal.slug, Some("full-goal".to_string()));

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_ulid_parsing() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        // Valid ULID format
        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('01HZVY3X4M5N6P7Q8R9S0TABC1', 'ULID Epoch')",
            [],
        )?;
        // Legacy ID (not a ULID)
        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('legacy-epoch', 'Legacy Epoch')",
            [],
        )?;

        let state = loader.load_state()?;

        // First epoch should have parsed ULID
        assert!(state.epochs[0].ulid.is_some());
        // Second epoch should have None for ulid (legacy ID)
        assert!(state.epochs[1].ulid.is_none());

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_phase_rfc_uses_schema_default() -> Result<()> {
        // Test that relation uses schema default when not explicitly set
        // Schema: relation TEXT NOT NULL DEFAULT 'related'
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'pending', 1, 'regular')",
            [],
        )?;
        // Insert RFC without specifying relation (uses schema default)
        conn.execute(
            "INSERT INTO phase_rfcs_data (phase_id, rfc_id, target)
             VALUES (1, '00100', NULL)",
            [],
        )?;

        let state = loader.load_state()?;
        let rfc = &state.epochs[0].phases[0].rfcs[0];

        assert_eq!(rfc.id, "00100");
        assert_eq!(rfc.target, None);
        assert_eq!(rfc.relation, "related"); // Schema default

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_multiple_epochs_ordering() -> Result<()> {
        // Verify epochs are returned in insertion order (by rowid)
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'First')",
            [],
        )?;
        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e2', 'Second')",
            [],
        )?;
        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e3', 'Third')",
            [],
        )?;

        let state = loader.load_state()?;

        assert_eq!(state.epochs.len(), 3);
        assert_eq!(state.epochs[0].title, "First");
        assert_eq!(state.epochs[1].title, "Second");
        assert_eq!(state.epochs[2].title, "Third");

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_ideas() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        // Insert an idea with tags and task refs
        conn.execute(
            "INSERT INTO ideas_data (text_id, title, description, status, created_at, source)
             VALUES ('idea-1', 'Test Idea', 'A description', 'new', '2024-01-15T10:00:00Z', 'user')",
            [],
        )?;

        // Add tags
        conn.execute(
            "INSERT INTO idea_tags (idea_id, tag) VALUES (1, 'sqlite')",
            [],
        )?;
        conn.execute(
            "INSERT INTO idea_tags (idea_id, tag) VALUES (1, 'migration')",
            [],
        )?;

        // Add task refs
        conn.execute(
            "INSERT INTO idea_task_refs (idea_id, task_ref) VALUES (1, 'task-abc')",
            [],
        )?;

        let ideas = loader.load_ideas()?;

        assert_eq!(ideas.len(), 1);
        assert_eq!(ideas[0].id, "idea-1");
        assert_eq!(ideas[0].title, "Test Idea");
        assert_eq!(ideas[0].description, "A description");
        assert_eq!(ideas[0].status, "new");
        assert_eq!(ideas[0].source, "user");
        let mut tags = ideas[0].tags.clone();
        tags.sort();
        assert_eq!(tags, vec!["migration", "sqlite"]);
        assert_eq!(ideas[0].related_tasks, vec!["task-abc"]);

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_inbox() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO inbox_data (
                text_id, created_at, updated_at, status,
                entity_type, entity_id, source, intent, priority, confidence,
                subject, body, resolution
            ) VALUES (
                'inbox-1', '2024-01-15T10:00:00Z', NULL, 'pending',
                'goal', 'my-goal', 'user-feedback', 'concern', 'next-touch', NULL,
                'Test Subject', 'Test body content', NULL
            )",
            [],
        )?;

        let items = loader.load_inbox()?;

        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "inbox-1");
        assert_eq!(item.subject, "Test Subject");
        assert_eq!(item.body, "Test body content");
        assert_eq!(item.entity_type, "goal");
        assert_eq!(item.entity_id, Some("my-goal".to_string()));
        assert_eq!(item.intent, InboxIntent::Concern);

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_inbox_project_scope() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO inbox_data (
                text_id, created_at, status,
                entity_type, source, intent, priority,
                subject, body
            ) VALUES (
                'inbox-2', '2024-01-15T10:00:00Z', 'pending',
                'project', 'user-feedback', 'inquiry', 'immediate',
                'Global Item', ''
            )",
            [],
        )?;

        let items = loader.load_inbox()?;

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entity_type, "project");
        assert_eq!(items[0].entity_id, None);
        assert_eq!(items[0].intent, InboxIntent::Inquiry);

        Ok(())
    }

    #[test]
    fn test_sqlite_loader_inbox_filtered() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute_batch(
            "INSERT INTO inbox_data (
                text_id, created_at, status,
                entity_type, entity_id, source, intent, priority,
                subject, body
            ) VALUES
                ('inbox-1', '2024-01-15T10:00:00Z', 'pending',
                 'goal', 'goal-1', 'user-feedback', 'concern', 'next-touch',
                 'Goal concern', 'Body 1'),
                ('inbox-2', '2024-01-15T11:00:00Z', 'pending',
                 'task', 'task-1', 'system-observation', 'claim', 'immediate',
                 'Task claim', 'Body 2'),
                ('inbox-3', '2024-01-15T12:00:00Z', 'resolved',
                 'goal', 'goal-2', 'plan-mutation', 'fyi', 'when-relevant',
                 'Goal resolved', 'Body 3'),
                ('inbox-4', '2024-01-15T13:00:00Z', 'acknowledged',
                 'goal', 'goal-1', 'user-feedback', 'inquiry', 'next-touch',
                 'Goal inquiry', 'Body 4');",
        )?;

        let goal_items = loader.load_inbox_filtered(None, Some("goal"), None, None, None)?;
        assert_eq!(goal_items.len(), 3);
        assert_eq!(
            goal_items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["inbox-4", "inbox-3", "inbox-1"]
        );

        let goal_entity_items =
            loader.load_inbox_filtered(None, Some("goal"), Some("goal-1"), None, None)?;
        assert_eq!(goal_entity_items.len(), 2);
        assert_eq!(
            goal_entity_items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["inbox-4", "inbox-1"]
        );

        let user_items =
            loader.load_inbox_filtered(None, None, None, Some("user-feedback"), None)?;
        assert_eq!(user_items.len(), 2);
        assert_eq!(
            user_items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["inbox-4", "inbox-1"]
        );

        let pending_goal_items =
            loader.load_inbox_filtered(Some("pending"), Some("goal"), None, None, None)?;
        assert_eq!(pending_goal_items.len(), 1);
        assert_eq!(pending_goal_items[0].id, "inbox-1");

        let limited_goal_items =
            loader.load_inbox_filtered(None, Some("goal"), None, None, Some(1))?;
        assert_eq!(limited_goal_items.len(), 1);
        assert_eq!(limited_goal_items[0].id, "inbox-4");

        Ok(())
    }

    #[test]
    fn test_has_completion_claim_no_claim() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::NoClaim
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_human_pending() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::HumanClaim
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_human_acknowledged() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'acknowledged', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::HumanClaim
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_agent_pending() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body, agent_id)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '', 'agent-session-1')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::AgentClaimPending
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_agent_acknowledged() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body, agent_id)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'acknowledged', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '', 'agent-session-1')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::AgentClaimAcknowledged
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_newer_human_claim_wins_over_older_agent_pending() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body, agent_id)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '', 'agent-session-1')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c2', '2024-01-15T10:01:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Confirmed done', '')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::HumanClaim
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_newer_agent_pending_wins_over_older_human_claim() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '')",
            [],
        )?;
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body, agent_id)
             VALUES ('c2', '2024-01-15T10:01:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Agent says done', '', 'agent-session-1')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::AgentClaimPending
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_ignores_resolved() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'resolved', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Done', '')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::NoClaim
        );
        Ok(())
    }

    #[test]
    fn test_has_completion_claim_ignores_non_claim_intent() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
             VALUES ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'fyi', 'immediate', 'Note', '')",
            [],
        )?;
        assert_eq!(
            loader.has_completion_claim("goal", "goal-1")?,
            CompletionClaimStatus::NoClaim
        );
        Ok(())
    }

    #[test]
    fn test_completion_outcome_digest_preserves_multiple_claim_subjects_and_bodies() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();
        conn.execute_batch(
            "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body, agent_id)
             VALUES
             ('c1', '2024-01-15T10:00:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'claim', 'next-touch', 'First real subject', 'First body with detail', NULL),
             ('c2', '2024-01-15T10:01:00Z', 'acknowledged', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Second real subject', 'Second body with detail', 'agent-session-1'),
             ('c3', '2024-01-15T10:02:00Z', 'resolved', 'goal', 'goal-1', 'user-feedback', 'claim', 'immediate', 'Resolved subject', 'Resolved body', NULL),
             ('c4', '2024-01-15T10:03:00Z', 'pending', 'goal', 'goal-1', 'user-feedback', 'concern', 'immediate', 'Concern subject', 'Concern body', NULL);",
        )?;

        let digest = loader.load_completion_outcome_digest("goal", "goal-1")?;

        assert_eq!(digest.entity_type, "goal");
        assert_eq!(digest.entity_id, "goal-1");
        assert_eq!(digest.claims.len(), 2);
        assert_eq!(digest.claims[0].subject, "Second real subject");
        assert_eq!(digest.claims[0].body, "Second body with detail");
        assert_eq!(
            digest.claims[0].agent_id.as_deref(),
            Some("agent-session-1")
        );
        assert_eq!(digest.claims[1].subject, "First real subject");
        assert_eq!(digest.claims[1].body, "First body with detail");

        Ok(())
    }

    #[test]
    fn test_import_ideas() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;

        let ideas = vec![
            Idea {
                id: "idea-1".to_string(),
                title: "First Idea".to_string(),
                description: "Description of first idea".to_string(),
                status: "new".to_string(),
                created_at: "2024-01-15T10:00:00Z".to_string(),
                source: "user".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                related_tasks: vec!["task-1".to_string()],
            },
            Idea {
                id: "idea-2".to_string(),
                title: "Second Idea".to_string(),
                description: String::new(), // empty description
                status: "archived".to_string(),
                created_at: "2024-01-16T10:00:00Z".to_string(),
                source: "agent".to_string(),
                tags: vec![],
                related_tasks: vec![],
            },
        ];

        // Import ideas
        let result = loader.import_ideas(&ideas)?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);

        // Verify they can be loaded back
        let loaded = loader.load_ideas()?;
        assert_eq!(loaded.len(), 2);

        // Find idea-1 and verify its data
        let idea1 = loaded.iter().find(|i| i.id == "idea-1").unwrap();
        assert_eq!(idea1.title, "First Idea");
        assert_eq!(idea1.description, "Description of first idea");
        assert_eq!(idea1.tags, vec!["tag1", "tag2"]);
        assert_eq!(idea1.related_tasks, vec!["task-1"]);

        // Find idea-2 and verify empty description becomes empty string
        let idea2 = loaded.iter().find(|i| i.id == "idea-2").unwrap();
        assert_eq!(idea2.title, "Second Idea");
        assert!(idea2.description.is_empty());
        assert!(idea2.tags.is_empty());

        // Import again - should skip existing
        let result2 = loader.import_ideas(&ideas)?;
        assert_eq!(result2.imported, 0);
        assert_eq!(result2.skipped, 2);

        Ok(())
    }

    #[test]
    fn test_import_inbox() -> Result<()> {
        use crate::inbox::{InboxIntent, InboxPriority, InboxSource};

        let loader = SqliteLoader::open_memory()?;

        let items = vec![
            InboxItem {
                id: "inbox-1".to_string(),
                created: "2024-01-15T10:00:00Z".to_string(),
                status: InboxItemStatus::Pending,
                entity_type: "goal".to_string(),
                entity_id: Some("my-goal".to_string()),
                source: InboxSource::UserFeedback,
                intent: InboxIntent::Fyi,
                priority: InboxPriority::NextTouch,
                confidence: None,
                agent_id: None,
                subject: "Test Subject".to_string(),
                body: "Test body content".to_string(),
                action: None,
                updated: None,
                resolution: None,
            },
            InboxItem {
                id: "inbox-2".to_string(),
                created: "2024-01-16T10:00:00Z".to_string(),
                status: InboxItemStatus::Resolved,
                entity_type: "project".to_string(),
                entity_id: None,
                source: InboxSource::UserFeedback,
                intent: InboxIntent::Concern,
                priority: InboxPriority::Immediate,
                confidence: None,
                agent_id: None,
                subject: "Global Item".to_string(),
                body: String::new(),
                action: None,
                updated: Some("2024-01-17T10:00:00Z".to_string()),
                resolution: Some("Fixed it".to_string()),
            },
        ];

        // Import inbox items
        let result = loader.import_inbox(&items)?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);

        // Verify they can be loaded back
        let loaded = loader.load_inbox()?;
        assert_eq!(loaded.len(), 2);

        // Find inbox-1 and verify its data
        let item1 = loaded.iter().find(|i| i.id == "inbox-1").unwrap();
        assert_eq!(item1.subject, "Test Subject");
        assert_eq!(item1.entity_type, "goal");
        assert_eq!(item1.entity_id, Some("my-goal".to_string()));
        assert_eq!(item1.intent, InboxIntent::Fyi);

        // Find inbox-2 and verify project scope
        let item2 = loaded.iter().find(|i| i.id == "inbox-2").unwrap();
        assert_eq!(item2.entity_type, "project");
        assert_eq!(item2.entity_id, None);
        assert_eq!(item2.resolution, Some("Fixed it".to_string()));

        // Import again - should skip existing
        let result2 = loader.import_inbox(&items)?;
        assert_eq!(result2.imported, 0);
        assert_eq!(result2.skipped, 2);

        Ok(())
    }

    #[test]
    fn test_import_plan() -> Result<()> {
        use crate::context::{PhaseKind, PhaseRfc};

        let loader = SqliteLoader::open_memory()?;

        // Create a minimal ExoState with one epoch, one phase, one goal
        let state = ExoState {
            meta: Some(Meta::current()),
            epochs: vec![Epoch {
                id: "epoch-1".to_string(),
                title: "Test Epoch".to_string(),
                phases: vec![Phase {
                    id: "phase-1".to_string(),
                    title: "Test Phase".to_string(),
                    status: "in-progress".to_string(),
                    goals: vec![Goal {
                        id: "goal-1".to_string(),
                        label: "Test Goal".to_string(),
                        status: "pending".to_string(),
                        kind: None,
                        started_at: None,
                        description: None,
                        completion_log: None,
                        ulid: None,
                        slug: Some("test-goal".to_string()),
                        aliases: vec!["my-goal".to_string()],
                        rfc: None,
                        target_stage: None,
                    }],
                    rfcs: vec![PhaseRfc {
                        id: "00238".to_string(),
                        target: Some(2),
                        relation: "driving".to_string(),
                    }],
                    kind: PhaseKind::Regular,
                    ulid: None,
                    slug: Some("test-phase".to_string()),
                    aliases: vec![],
                }],
                ulid: None,
                slug: Some("test-epoch".to_string()),
                aliases: vec!["my-epoch".to_string()],
                reviewed: false,
            }],
        };

        // Import the plan
        let result = loader.import_plan(&state)?;
        assert_eq!(result.epochs_imported, 1);
        assert_eq!(result.phases_imported, 1);
        assert_eq!(result.goals_imported, 1);
        assert_eq!(result.epochs_skipped, 0);

        // Verify it can be loaded back
        let loaded = loader.load_state()?;
        assert_eq!(loaded.epochs.len(), 1);

        let epoch = &loaded.epochs[0];
        assert_eq!(epoch.id, "epoch-1");
        assert_eq!(epoch.title, "Test Epoch");
        assert_eq!(epoch.aliases, vec!["my-epoch"]);

        let phase = &epoch.phases[0];
        assert_eq!(phase.id, "phase-1");
        assert_eq!(phase.status, "in-progress");
        assert_eq!(phase.rfcs.len(), 1);
        assert_eq!(phase.rfcs[0].id, "00238");
        assert_eq!(phase.rfcs[0].target, Some(2));

        let goal = &phase.goals[0];
        assert_eq!(goal.id, "goal-1");
        assert_eq!(goal.label, "Test Goal");
        assert_eq!(goal.aliases, vec!["my-goal"]);

        // Import again - should skip existing
        let result2 = loader.import_plan(&state)?;
        assert_eq!(result2.epochs_skipped, 1);
        assert_eq!(result2.phases_skipped, 1);
        assert_eq!(result2.goals_skipped, 1);
        assert_eq!(result2.total_imported(), 0);

        Ok(())
    }

    #[test]
    fn test_resolve_entity_tree_task() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'in-progress', 1, 'regular')",
            [],
        )?;
        conn.execute(
            "INSERT INTO goals_data (text_id, label, status, phase_id)
             VALUES ('g1', 'Goal', 'in-progress', 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO tasks_data (text_id, title, status, goal_id)
             VALUES ('t1', 'Task', 'pending', 1)",
            [],
        )?;

        let ancestors = loader.resolve_entity_tree("task", "t1")?;
        assert_eq!(
            ancestors,
            vec![
                ("goal".to_string(), "g1".to_string()),
                ("phase".to_string(), "p1".to_string()),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_resolve_entity_tree_goal() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;
        let conn = loader.database().connection();

        conn.execute(
            "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch')",
            [],
        )?;
        conn.execute(
            "INSERT INTO phases_data (text_id, title, status, epoch_id, kind)
             VALUES ('p1', 'Phase', 'in-progress', 1, 'regular')",
            [],
        )?;
        conn.execute(
            "INSERT INTO goals_data (text_id, label, status, phase_id)
             VALUES ('g1', 'Goal', 'in-progress', 1)",
            [],
        )?;

        let ancestors = loader.resolve_entity_tree("goal", "g1")?;
        assert_eq!(ancestors, vec![("phase".to_string(), "p1".to_string())]);

        Ok(())
    }

    #[test]
    fn test_resolve_entity_tree_unknown() -> Result<()> {
        let loader = SqliteLoader::open_memory()?;

        let ancestors = loader.resolve_entity_tree("task", "nonexistent")?;
        assert!(ancestors.is_empty());

        let ancestors = loader.resolve_entity_tree("phase", "anything")?;
        assert!(ancestors.is_empty());

        Ok(())
    }
}
