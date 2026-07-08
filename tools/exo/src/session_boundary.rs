//! Session boundary detection.
//!
//! Detects what kind of session boundary the agent is at:
//! - **brand-new**: First session ever (no prior context, no history)
//! - **session**: Normal session start (prior work exists, agent is fresh)
//! - **compaction**: Mid-session context loss (agent's context was compacted/summarized)
//!
//! Primary detection uses `last_event_at` from the `agent_events` table.
//! Falls back to git-based heuristics when the event table is empty or
//! the database is unavailable.
//!
//! The boundary type drives different steering guidance:
//! - Brand-new → full onboarding orientation
//! - Session → plan-aware reorientation ("where were we?")
//! - Compaction → minimal reorientation ("hold the plan, keep going")

use crate::world_state::WorldState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// The type of session boundary detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryType {
    /// First session ever — no epochs, no phases, no history.
    BrandNew,
    /// Normal session start — prior work exists, agent is fresh.
    Session,
    /// Mid-session context loss — recent activity suggests the agent was
    /// working and lost context (compaction, window overflow, crash).
    Compaction,
}

impl std::fmt::Display for BoundaryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BrandNew => write!(f, "brand-new"),
            Self::Session => write!(f, "session"),
            Self::Compaction => write!(f, "compaction"),
        }
    }
}

/// Result of boundary detection with confidence score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryDetection {
    pub boundary_type: BoundaryType,
    /// Confidence in the detection (0.0–1.0).
    pub confidence: f32,
    /// Human-readable explanation of why this boundary type was detected.
    pub rationale: String,
    /// Summary of the session before the current gap, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_session: Option<crate::activity::PreviousSessionSummary>,
}

/// Detect the session boundary type from world state and event history.
///
/// Detection priority:
///
/// 1. **Brand-new**: No epochs, no tasks, no goals → first session ever.
///
/// 2. **Event-based**: Query `last_event_at` from `agent_events` table.
///    - Gap > 30 min → `Session`
///    - Gap ≤ 30 min + in-progress tasks → `Compaction`
///    - Gap ≤ 30 min + no in-progress tasks → `Session` (lower confidence)
///
/// 3. **Fallback**: Git-based heuristics when event data is unavailable.
pub fn detect_boundary(world: &WorldState) -> BoundaryDetection {
    // === Brand-new: no project history at all ===
    if !world.epoch_state.has_epochs && world.tasks.is_empty() && world.goals.is_empty() {
        return BoundaryDetection {
            boundary_type: BoundaryType::BrandNew,
            confidence: 0.95,
            rationale: "No epochs, tasks, or goals found — this appears to be a brand-new project."
                .to_string(),
            previous_session: None,
        };
    }

    // === Event-based detection (primary) ===
    if let Some(last_event) = last_event_at(&world.db_path) {
        let now = Utc::now();
        let gap = now.signed_duration_since(last_event);
        let gap_minutes = gap.num_minutes();
        let has_in_progress = world.tasks.iter().any(|(_, _, s)| s == "in-progress");

        if gap_minutes > 30 {
            return BoundaryDetection {
                boundary_type: BoundaryType::Session,
                confidence: 0.90,
                rationale: format!(
                    "No agent activity for {gap_minutes} minutes — likely a new session."
                ),
                previous_session: crate::activity::previous_session_summary_from_db(&world.db_path),
            };
        }

        if has_in_progress {
            return BoundaryDetection {
                boundary_type: BoundaryType::Compaction,
                confidence: 0.95,
                rationale: format!(
                    "Recent activity ({gap_minutes} minutes ago) with in-progress tasks — \
                     likely context compaction."
                ),
                previous_session: None,
            };
        }

        return BoundaryDetection {
            boundary_type: BoundaryType::Session,
            confidence: 0.75,
            rationale: format!(
                "Recent activity ({gap_minutes} minutes ago) but no in-progress tasks — \
                 session continuing or just ended."
            ),
            previous_session: None,
        };
    }

    // === Fallback: git-based heuristics (no event data available) ===
    detect_boundary_git_fallback(world)
}

/// Query the most recent event timestamp from the `agent_events` table.
///
/// Returns `None` if the database doesn't exist, the table is empty,
/// or any error occurs.
fn last_event_at(db_path: &Path) -> Option<DateTime<Utc>> {
    let ts = crate::event_db::with_event_db(db_path, |conn| {
        conn.query_row("SELECT MAX(timestamp) FROM agent_events", [], |row| {
            row.get::<_, Option<String>>(0)
        })
    })
    .flatten()?;
    DateTime::parse_from_rfc3339(&ts)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Git-based fallback heuristics for when event data is unavailable.
fn detect_boundary_git_fallback(world: &WorldState) -> BoundaryDetection {
    if world.active_phase.is_some() {
        let has_in_progress = world.tasks.iter().any(|(_, _, s)| s == "in-progress");
        let is_dirty = world.git_dirty;
        let recent_commit = has_recent_git_commit(&world.root, 30);

        // Strong compaction signal: dirty repo + in-progress tasks
        if has_in_progress && is_dirty {
            return BoundaryDetection {
                boundary_type: BoundaryType::Compaction,
                confidence: 0.85,
                rationale: "Active phase with in-progress tasks and uncommitted changes — \
                     likely mid-session context loss."
                    .to_string(),
                previous_session: None,
            };
        }

        // Medium compaction signal: in-progress tasks + recent commit
        if has_in_progress && recent_commit {
            return BoundaryDetection {
                boundary_type: BoundaryType::Compaction,
                confidence: 0.70,
                rationale:
                    "Active phase with in-progress tasks and a recent commit (within 30 min) — \
                     likely mid-session context loss after a commit."
                        .to_string(),
                previous_session: None,
            };
        }
    }

    let rationale = if world.active_phase.is_some() {
        "Active phase exists with no signs of recent mid-session activity — normal session start."
    } else {
        "Prior work exists but no active phase — normal session start."
    };

    BoundaryDetection {
        boundary_type: BoundaryType::Session,
        confidence: 0.60,
        rationale: rationale.to_string(),
        previous_session: None,
    }
}

/// Check if the most recent git commit was within `minutes` minutes.
fn has_recent_git_commit(root: &Path, minutes: u64) -> bool {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(root)
        .output();

    let Ok(output) = output else {
        return false;
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Ok(commit_epoch) = stdout.trim().parse::<u64>() else {
        return false;
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let elapsed_secs = now.saturating_sub(commit_epoch);
    elapsed_secs < minutes * 60
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_state::{EpochBoundaryState, WorldState};
    use std::collections::HashMap;

    /// Build a minimal `WorldState` for boundary detection tests.
    fn test_world(root: std::path::PathBuf) -> WorldState {
        WorldState {
            db_path: root.join(crate::context::SQLITE_DB_PATH),
            workspace_root_key: None,
            root,
            active_phase: None,
            next_phase: None,
            epoch_state: EpochBoundaryState {
                active_epoch: None,
                epoch_complete: false,
                has_epochs: true,
                all_epochs_complete: false,
            },
            git_dirty: false,
            git_changes: None,
            sidecar_sync: None,
            current_snapshots: Vec::new(),
            tasks: vec![("task-1".into(), "Some task".into(), "planned".into())],
            goals: vec![],
            rfc_pipeline: HashMap::new(),
            unreviewed_epochs: Vec::new(),
            // Placeholder — overwritten by detect_boundary itself
            session_boundary: BoundaryDetection {
                boundary_type: BoundaryType::Session,
                confidence: 0.0,
                rationale: String::new(),
                previous_session: None,
            },
        }
    }

    #[test]
    fn boundary_type_display() {
        assert_eq!(BoundaryType::BrandNew.to_string(), "brand-new");
        assert_eq!(BoundaryType::Session.to_string(), "session");
        assert_eq!(BoundaryType::Compaction.to_string(), "compaction");
    }

    #[test]
    fn boundary_type_serde_roundtrip() {
        let json = serde_json::to_string(&BoundaryType::Compaction).unwrap();
        assert_eq!(json, "\"compaction\"");
        let parsed: BoundaryType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BoundaryType::Compaction);
    }

    #[test]
    fn event_gap_over_30_min_detects_session() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        // Create the DB with migrations
        let db_path = root.join(crate::context::SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let db = exosuit_storage::open_database(&db_path).unwrap();

        // Insert an event from 1 hour ago (RFC3339 format for parser compatibility)
        db.connection()
            .execute(
                "INSERT INTO agent_events (text_id, timestamp, event_type, namespace, operation, summary)
                 VALUES ('evt-1', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hours'), 'command', 'goal', 'add', 'test')",
                [],
            )
            .unwrap();
        drop(db);

        let world = test_world(root.to_path_buf());
        let detection = detect_boundary(&world);
        assert_eq!(detection.boundary_type, BoundaryType::Session);
        assert!(detection.confidence >= 0.85);
    }

    #[test]
    fn recent_event_with_in_progress_task_detects_compaction() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        let db_path = root.join(crate::context::SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let db = exosuit_storage::open_database(&db_path).unwrap();

        // Insert an event from 1 minute ago (RFC3339 format for parser compatibility)
        db.connection()
            .execute(
                "INSERT INTO agent_events (text_id, timestamp, event_type, namespace, operation, summary)
                 VALUES ('evt-2', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 minutes'), 'command', 'task', 'start', 'test')",
                [],
            )
            .unwrap();
        drop(db);

        let mut world = test_world(root.to_path_buf());
        // Add an in-progress task
        world.tasks = vec![("task-1".into(), "Active task".into(), "in-progress".into())];

        let detection = detect_boundary(&world);
        assert_eq!(detection.boundary_type, BoundaryType::Compaction);
        assert!(detection.confidence >= 0.90);
    }

    #[test]
    fn recent_event_no_in_progress_detects_session_low_confidence() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        let db_path = root.join(crate::context::SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let db = exosuit_storage::open_database(&db_path).unwrap();

        // Insert an event from 5 minutes ago (RFC3339 format for parser compatibility)
        db.connection()
            .execute(
                "INSERT INTO agent_events (text_id, timestamp, event_type, namespace, operation, summary)
                 VALUES ('evt-3', strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 minutes'), 'command', 'goal', 'list', 'test')",
                [],
            )
            .unwrap();
        drop(db);

        let mut world = test_world(root.to_path_buf());
        world.tasks = vec![("task-1".into(), "Planned task".into(), "planned".into())];

        let detection = detect_boundary(&world);
        assert_eq!(detection.boundary_type, BoundaryType::Session);
        // Lower confidence when recent activity but no in-progress tasks
        assert!(
            detection.confidence <= 0.80,
            "expected confidence <= 0.80, got {}",
            detection.confidence
        );
    }
}
