use crate::ExoResult;
use crate::command::sidecar::SidecarRepoSyncStatus;
use crate::context::rfc::{self, RfcIndexEntry};
use crate::context::{ActivePhaseData, AgentContext, Goal};
use crate::task;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Re-export for backward compatibility.
pub type ActivePhase = ActivePhaseData;

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotGuidance {
    pub command: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotFileStatus {
    pub path: String,
    pub exists: bool,
    // Legacy alias (kept for one release): true when file is read-only on disk (intended).
    pub read_only: bool,
    // Legacy alias (kept for one release): true when file is directly writable on disk.
    pub writable: bool,

    // New names (preferred).
    pub disk_read_only: bool,
    pub direct_writable: bool,

    // ok | missing | unexpectedly-writable
    pub status: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<SnapshotGuidance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitChangeSummary {
    pub total: usize,
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub renamed: usize,
    pub untracked: usize,

    pub generatedish: usize,
    pub agent_context: usize,
    pub context: usize,
    pub source: usize,
    pub other: usize,

    pub sample_generatedish: Vec<String>,
    pub sample_agent_context: Vec<String>,
    pub sample_context: Vec<String>,
    pub sample_source: Vec<String>,
    pub sample_other: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RfcPipelineEntry {
    pub id: String,
    pub current_stage: u8,
    pub target_stage: Option<u8>,
    pub title: String,
    pub is_driving: bool,
}

/// Info about an epoch that needs review.
#[derive(Debug, Clone)]
pub struct UnreviewedEpoch {
    pub id: String,
    pub title: String,
}

/// Summary of the next phase to start when no phase is active.
#[derive(Debug, Clone)]
pub struct NextPhase {
    pub id: String,
    pub title: String,
    pub epoch_title: String,
}

/// Information about the active epoch.
#[derive(Debug, Clone)]
pub struct ActiveEpoch {
    pub id: String,
    pub title: String,
    pub status: String,
}

/// Epoch boundary state for multi-level steering.
#[derive(Debug, Clone)]
pub struct EpochBoundaryState {
    /// The currently active epoch (if any).
    pub active_epoch: Option<ActiveEpoch>,
    /// Whether the current epoch is complete (all phases done).
    pub epoch_complete: bool,
    /// Whether there are any epochs defined.
    pub has_epochs: bool,
    /// Whether all epochs are complete.
    pub all_epochs_complete: bool,
}

#[derive(Debug)]
pub struct WorldState {
    pub root: PathBuf,
    pub db_path: PathBuf,
    pub workspace_root_key: Option<String>,
    pub active_phase: Option<ActivePhase>,
    /// Next phase to start (when no active phase)
    pub next_phase: Option<NextPhase>,
    /// Epoch boundary state for multi-level steering
    pub epoch_state: EpochBoundaryState,
    pub git_dirty: bool,
    pub git_changes: Option<GitChangeSummary>,
    pub sidecar_sync: Option<SidecarRepoSyncStatus>,
    pub current_snapshots: Vec<SnapshotFileStatus>,
    pub tasks: Vec<(String, String, String)>,
    pub goals: Vec<Goal>,
    pub rfc_pipeline: HashMap<String, RfcPipelineEntry>,
    /// Epochs that are completed but not yet reviewed.
    pub unreviewed_epochs: Vec<UnreviewedEpoch>,
    /// Session boundary detection result.
    pub session_boundary: crate::session_boundary::BoundaryDetection,
}

impl WorldState {
    pub fn probe(context: &AgentContext) -> ExoResult<Self> {
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let workspace_root_key = context.workspace_root_key();
        let active_phase = context
            .find_workspace_active_phase()?
            .map(|info| info.to_owned_data());

        // Find next phase to start (only if no active phase)
        let next_phase = if active_phase.is_none() {
            Self::find_next_phase(context)
        } else {
            None
        };

        // Compute epoch boundary state for multi-level steering
        let epoch_state = Self::compute_epoch_state(context);

        let tasks = task::list_tasks_for_context(context).unwrap_or_default();
        let goals = if let Some(phase_info) = context.find_workspace_active_phase()? {
            phase_info.phase.goals.clone()
        } else {
            Vec::new()
        };

        let (git_porcelain, sidecar_sync) = std::thread::scope(|scope| {
            let git_status = scope.spawn(|| git_status_porcelain(&context.root));
            let sidecar_status = scope.spawn(|| {
                crate::command::sidecar::sidecar_repo_sync_status_with_project(
                    &context.root,
                    context.project.as_ref(),
                )
            });
            (
                git_status.join().unwrap_or_default(),
                sidecar_status.join().unwrap_or_default(),
            )
        });
        let git_dirty = git_porcelain
            .as_ref()
            .is_some_and(|stdout| !stdout.trim().is_empty());
        let git_changes = git_porcelain.as_deref().map(summarize_git_porcelain);
        let current_snapshots = snapshot_statuses(&context.root);

        let rfc_index = rfc::index_rfcs_with_project(&context.root, context.project.as_ref())?;
        let rfc_pipeline = build_rfc_pipeline(active_phase.as_ref(), &rfc_index);

        // Find unreviewed completed epochs
        let unreviewed_epochs = context
            .plan
            .find_unreviewed_epochs()
            .into_iter()
            .map(|e| UnreviewedEpoch {
                id: e.id.clone(),
                title: e.title.clone(),
            })
            .collect();

        // Detect session boundary type.
        // We build a partial WorldState to pass to the detector, then move fields into the final struct.
        // Instead, we inline the detection here since it needs the same fields.
        let partial = Self {
            root: context.root.clone(),
            db_path,
            workspace_root_key,
            active_phase,
            next_phase,
            epoch_state,
            git_dirty,
            git_changes,
            sidecar_sync,
            current_snapshots,
            tasks,
            goals,
            rfc_pipeline,
            unreviewed_epochs,
            session_boundary: crate::session_boundary::BoundaryDetection {
                boundary_type: crate::session_boundary::BoundaryType::Session,
                confidence: 0.0,
                rationale: String::new(),
                previous_session: None,
            },
        };
        let session_boundary = crate::session_boundary::detect_boundary(&partial);

        Ok(Self {
            session_boundary,
            ..partial
        })
    }

    /// Compute epoch boundary state for multi-level steering.
    ///
    /// This determines:
    /// - Whether we're between epochs (no active epoch, or current epoch complete)
    /// - Whether we're between phases (in an epoch, but no active phase)
    fn compute_epoch_state(context: &AgentContext) -> EpochBoundaryState {
        let has_epochs = !context.plan.epochs.is_empty();

        if !has_epochs {
            return EpochBoundaryState {
                active_epoch: None,
                epoch_complete: false,
                has_epochs: false,
                all_epochs_complete: true, // vacuously true
            };
        }

        // Check if all epochs are complete
        let all_epochs_complete = context
            .plan
            .epochs
            .iter()
            .all(|e| e.derived_status() == "completed");

        let active_epoch = context
            .find_workspace_active_epoch()
            .ok()
            .flatten()
            .map(|e| ActiveEpoch {
                id: e.id.clone(),
                title: e.title.clone(),
                status: e.derived_status().to_string(),
            });

        // Epoch is complete if the active epoch (or all epochs) is complete
        let epoch_complete = active_epoch
            .as_ref()
            .map_or(all_epochs_complete, |e| e.status == "completed");

        EpochBoundaryState {
            active_epoch,
            epoch_complete,
            has_epochs,
            all_epochs_complete,
        }
    }

    /// Find the next phase to start.
    /// Uses anchor heuristic: prefer phases after the last executed phase.
    fn find_next_phase(context: &AgentContext) -> Option<NextPhase> {
        let anchor = read_last_executed_phase_id(&context.root, context);

        // Pass 1: if we have an anchor, find the first pending phase after it.
        if let Some(anchor_id) = anchor {
            let mut seen_anchor = false;
            for epoch in &context.plan.epochs {
                for phase in &epoch.phases {
                    if !seen_anchor {
                        if phase.id == anchor_id {
                            seen_anchor = true;
                        }
                        continue;
                    }

                    if phase.status == "pending" {
                        return Some(NextPhase {
                            id: phase.id.clone(),
                            title: phase.title.clone(),
                            epoch_title: epoch.title.clone(),
                        });
                    }
                }
            }
        }

        // Pass 2: fall back to the first pending phase anywhere in plan order.
        for epoch in &context.plan.epochs {
            for phase in &epoch.phases {
                if phase.status == "pending" {
                    return Some(NextPhase {
                        id: phase.id.clone(),
                        title: phase.title.clone(),
                        epoch_title: epoch.title.clone(),
                    });
                }
            }
        }

        None
    }
}

fn build_rfc_pipeline(
    active_phase: Option<&ActivePhaseData>,
    rfc_index: &HashMap<String, RfcIndexEntry>,
) -> HashMap<String, RfcPipelineEntry> {
    let mut pipeline = HashMap::new();

    let Some(phase) = active_phase else {
        return pipeline;
    };

    for rfc in &phase.rfcs {
        let (current_stage, title) = match rfc_index.get(&rfc.id) {
            Some(entry) => (entry.stage, entry.title.clone()),
            None => (0, format!("RFC {}", rfc.id)),
        };

        pipeline.insert(
            rfc.id.clone(),
            RfcPipelineEntry {
                id: rfc.id.clone(),
                current_stage,
                target_stage: rfc.target,
                title,
                is_driving: rfc.is_driving(),
            },
        );
    }

    pipeline
}

/// Read the phase id from the last completed phase in SQLite-backed state.
fn read_last_executed_phase_id(_root: &Path, context: &AgentContext) -> Option<String> {
    if let Ok(anchor) = context.workspace_anchor_phase_id()
        && anchor.is_some()
    {
        return anchor;
    }

    // Strategy: find the last completed phase in plan order.
    // This is the most reliable anchor and doesn't depend on deprecated snapshot files.
    let mut last_completed = None;
    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.status == "completed" {
                last_completed = Some(phase.id.clone());
            }
        }
    }
    last_completed
}

fn git_status_porcelain(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn summarize_git_porcelain(stdout: &str) -> GitChangeSummary {
    fn push_sample(samples: &mut Vec<String>, path: &str) {
        const MAX: usize = 5;
        if samples.len() < MAX {
            samples.push(path.to_string());
        }
    }

    fn classify_path(path: &str) -> &'static str {
        let normalized = path.trim_start_matches("./");

        // Context-managed content: usually intentional edits.
        if normalized.starts_with("docs/agent-context/") {
            return "agent_context";
        }
        if normalized.starts_with("docs/rfcs/") {
            return "context";
        }

        // Common generated-ish dirs (often safe to regenerate).
        if normalized.starts_with("target/")
            || normalized.starts_with("node_modules/")
            || normalized.starts_with(".debug/")
            || normalized.starts_with("dist/")
            || normalized.contains("/out/")
            || normalized.starts_with("out/")
        {
            return "generatedish";
        }

        // Source-ish content.
        if normalized.starts_with("crates/")
            || normalized.starts_with("tools/")
            || normalized.starts_with("packages/")
            || normalized.starts_with("scripts/")
            || normalized.starts_with("src/")
        {
            return "source";
        }

        "other"
    }

    let mut summary = GitChangeSummary {
        total: 0,
        modified: 0,
        added: 0,
        deleted: 0,
        renamed: 0,
        untracked: 0,
        generatedish: 0,
        agent_context: 0,
        context: 0,
        source: 0,
        other: 0,
        sample_generatedish: Vec::new(),
        sample_agent_context: Vec::new(),
        sample_context: Vec::new(),
        sample_source: Vec::new(),
        sample_other: Vec::new(),
    };

    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        // Porcelain v1 is: XY<space>PATH, or ??<space>PATH, or R<space>OLD -> NEW.
        if line.len() < 3 {
            continue;
        }

        let status = &line[0..2];
        let mut path = line[3..].trim();
        if let Some((_, new)) = path.split_once(" -> ") {
            path = new.trim();
        }

        summary.total += 1;

        if status == "??" {
            summary.untracked += 1;
        } else {
            if status.contains('M') {
                summary.modified += 1;
            }
            if status.contains('A') {
                summary.added += 1;
            }
            if status.contains('D') {
                summary.deleted += 1;
            }
            if status.contains('R') {
                summary.renamed += 1;
            }
        }

        match classify_path(path) {
            "generatedish" => {
                summary.generatedish += 1;
                push_sample(&mut summary.sample_generatedish, path);
            }
            "agent_context" => {
                summary.agent_context += 1;
                push_sample(&mut summary.sample_agent_context, path);
            }
            "context" => {
                summary.context += 1;
                push_sample(&mut summary.sample_context, path);
            }
            "source" => {
                summary.source += 1;
                push_sample(&mut summary.sample_source, path);
            }
            _ => {
                summary.other += 1;
                push_sample(&mut summary.sample_other, path);
            }
        }
    }

    summary
}

const fn snapshot_statuses(root: &Path) -> Vec<SnapshotFileStatus> {
    let _ = root;
    Vec::new()
}
