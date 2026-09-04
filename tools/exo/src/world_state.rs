use crate::ExoResult;
use crate::command::sidecar::SidecarRepoSyncStatus;
use crate::context::{ActivePhaseData, AgentContext, Goal};
use crate::process_spawn::CommandSpawnExt as _;
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
    pub current_stage: Option<u8>,
    pub lifecycle: Option<String>,
    pub superseded_by: Option<String>,
    pub target_stage: Option<u8>,
    pub title: String,
    pub is_driving: bool,
    pub motion: crate::project_flow::RfcObjectiveMotion,
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
    pub rfc_objective_diagnostics: Vec<String>,
    /// Epochs that are completed but not yet reviewed.
    pub unreviewed_epochs: Vec<UnreviewedEpoch>,
    /// Session boundary detection result.
    pub session_boundary: crate::session_boundary::BoundaryDetection,
}

impl WorldState {
    pub fn probe(context: &AgentContext) -> ExoResult<Self> {
        let rfc_view =
            crate::rfc::load_effective_rfc_view(&context.root, context.project.as_ref())?;
        Self::probe_with_rfc_view(context, &rfc_view)
    }

    pub fn probe_with_rfc_view(
        context: &AgentContext,
        rfc_view: &crate::rfc::EffectiveRfcView,
    ) -> ExoResult<Self> {
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

        let (rfc_pipeline, rfc_objective_diagnostics) =
            build_rfc_pipeline(&db_path, active_phase.as_ref(), &rfc_view.records);

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
            rfc_objective_diagnostics,
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
    db_path: &Path,
    active_phase: Option<&ActivePhaseData>,
    effective_rfcs: &[crate::rfc::EffectiveRfcRecord],
) -> (HashMap<String, RfcPipelineEntry>, Vec<String>) {
    let mut pipeline = HashMap::new();

    let Some(phase) = active_phase else {
        return (pipeline, Vec::new());
    };

    match crate::project_flow::campaign_rfc_objectives(db_path, &phase.id) {
        Ok((mut objectives, mut diagnostics)) => {
            let effective = effective_rfcs
                .iter()
                .map(|effective| (effective.record.text_id.as_str(), &effective.record))
                .collect::<HashMap<_, _>>();
            for objective in &mut objectives {
                let Some(record) = effective.get(objective.rfc_ulid.as_str()) else {
                    continue;
                };
                let (lifecycle, superseded_by) =
                    crate::project_flow::effective_rfc_lifecycle(record);
                objective.rfc_number = record.rfc_number;
                objective.title.clone_from(&record.title);
                objective.current_stage = Some(record.stage);
                objective.lifecycle = Some(lifecycle);
                objective.superseded_by = superseded_by;
                objective.diagnostic = None;
            }
            diagnostics.retain(|diagnostic| {
                !objectives.iter().any(|objective| {
                    objective.current_stage.is_some()
                        && diagnostic
                            == &format!("project_flow.rfc_identity_missing: {}", objective.rfc_ulid)
                })
            });
            for objective in objectives {
                let id = format!("{:05}", objective.rfc_number);
                let motion = objective.motion();
                pipeline.insert(
                    objective.rfc_ulid,
                    RfcPipelineEntry {
                        id,
                        current_stage: objective.current_stage,
                        lifecycle: objective.lifecycle,
                        superseded_by: objective.superseded_by,
                        target_stage: objective.target_stage,
                        title: objective.title,
                        is_driving: matches!(objective.relation.as_str(), "drives" | "driving"),
                        motion,
                    },
                );
            }
            (pipeline, diagnostics)
        }
        Err(error) => (
            pipeline,
            vec![format!("project_flow.objective_resolution_failed: {error}")],
        ),
    }
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

#[cfg(test)]
mod project_flow_pipeline_tests {
    use super::*;
    use crate::context::sqlite_loader::RfcRecord;
    use crate::context::{PhaseKind, SqliteWriter};
    use crate::project_flow::{RfcRelation, attach_rfc};
    use crate::rfc::{EffectiveRfcRecord, RfcViewProvenance};

    fn fixture() -> (tempfile::TempDir, PathBuf, ActivePhaseData) {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("exo.db");
        let writer = SqliteWriter::open(&db_path).unwrap();
        let epoch = writer.add_epoch("Epoch", Some("epoch"), &[]).unwrap();
        let phase = writer
            .add_phase(&epoch, "Campaign", "regular", Some("campaign"), &[])
            .unwrap();
        writer
            .database()
            .connection()
            .execute(
                "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
             VALUES('01rfc000000000000000000001', 10207, 'Project flow', 2, 'active',
                    'project-flow', 'docs/rfcs/stage-2/10207.md')",
                [],
            )
            .unwrap();
        (
            temp,
            db_path,
            ActivePhaseData {
                id: phase,
                title: "Campaign".to_string(),
                epoch_id: epoch,
                epoch_title: "Epoch".to_string(),
                rfcs: Vec::new(),
                kind: PhaseKind::Regular,
            },
        )
    }

    #[test]
    fn typed_only_objective_is_visible_in_pipeline() {
        let (_temp, db_path, phase) = fixture();
        attach_rfc(
            &db_path,
            &phase.id,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();

        let (pipeline, diagnostics) = build_rfc_pipeline(&db_path, Some(&phase), &[]);
        let objective = pipeline
            .get("01rfc000000000000000000001")
            .expect("typed objective in pipeline");
        assert_eq!(objective.current_stage, Some(2));
        assert_eq!(objective.lifecycle.as_deref(), Some("active"));
        assert_eq!(objective.superseded_by, None);
        assert_eq!(objective.target_stage, Some(3));
        assert_eq!(
            objective.motion,
            crate::project_flow::RfcObjectiveMotion::Advancing
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn pipeline_uses_the_supplied_effective_rfc_view() {
        let (_temp, db_path, phase) = fixture();
        attach_rfc(
            &db_path,
            &phase.id,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        let effective = EffectiveRfcRecord {
            record: RfcRecord {
                text_id: "01rfc000000000000000000001".to_string(),
                rfc_number: 10207,
                title: "Workspace project flow".to_string(),
                stage: 3,
                status: "active".to_string(),
                feature: None,
                slug: "project-flow".to_string(),
                file_path: "docs/rfcs/stage-3/10207.md".to_string(),
                superseded_by: None,
                supersedes: None,
                withdrawal_reason: None,
                archived_reason: None,
                consolidated_into: None,
            },
            provenance: RfcViewProvenance {
                document_source: "workspace".to_string(),
                workspace_presence: "present".to_string(),
                canonical_presence: "present".to_string(),
                workspace_branch: Some("wycats/project-flow".to_string()),
                workspace_head: Some("abc123".to_string()),
                canonical_ref: Some("refs/heads/main".to_string()),
                canonical_head: Some("def456".to_string()),
                differs_from_canonical: true,
            },
        };

        let (pipeline, diagnostics) = build_rfc_pipeline(&db_path, Some(&phase), &[effective]);
        let objective = &pipeline["01rfc000000000000000000001"];
        assert_eq!(objective.title, "Workspace project flow");
        assert_eq!(objective.current_stage, Some(3));
        assert_eq!(
            objective.motion,
            crate::project_flow::RfcObjectiveMotion::TargetReached
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn world_pipeline_marks_reached_and_stable_objectives_as_non_advancing() {
        let (_temp, db_path, phase) = fixture();
        attach_rfc(
            &db_path,
            &phase.id,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        SqliteWriter::open(&db_path)
            .unwrap()
            .database()
            .connection()
            .execute("UPDATE rfcs SET stage = 3", [])
            .unwrap();
        let (pipeline, _) = build_rfc_pipeline(&db_path, Some(&phase), &[]);
        assert_eq!(
            pipeline["01rfc000000000000000000001"].motion,
            crate::project_flow::RfcObjectiveMotion::TargetReached
        );

        let (_temp, db_path, phase) = fixture();
        attach_rfc(
            &db_path,
            &phase.id,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            None,
        )
        .unwrap();
        SqliteWriter::open(&db_path)
            .unwrap()
            .database()
            .connection()
            .execute("UPDATE rfcs SET stage = 4", [])
            .unwrap();
        let (pipeline, _) = build_rfc_pipeline(&db_path, Some(&phase), &[]);
        assert_eq!(
            pipeline["01rfc000000000000000000001"].motion,
            crate::project_flow::RfcObjectiveMotion::Associated
        );
    }

    #[test]
    fn missing_typed_identity_is_not_replaced_or_fabricated_at_stage_zero() {
        let (_temp, db_path, phase) = fixture();
        attach_rfc(
            &db_path,
            &phase.id,
            "01rfc000000000000000000001",
            RfcRelation::Drives,
            Some(3),
        )
        .unwrap();
        let writer = SqliteWriter::open(&db_path).unwrap();
        writer
            .database()
            .connection()
            .execute(
                "DELETE FROM rfcs WHERE text_id = '01rfc000000000000000000001'",
                [],
            )
            .unwrap();
        writer
            .database()
            .connection()
            .execute(
                "INSERT INTO rfcs(text_id, rfc_number, title, stage, status, slug, file_path)
             VALUES('01rfc000000000000000000002', 10207, 'Different RFC', 4, 'active',
                    'different', 'docs/rfcs/stage-4/10207-different.md')",
                [],
            )
            .unwrap();
        writer
            .replace_phase_rfcs(&phase.id, &["10207".to_string()])
            .unwrap();

        let (pipeline, diagnostics) = build_rfc_pipeline(&db_path, Some(&phase), &[]);
        let objective = pipeline
            .get("01rfc000000000000000000001")
            .expect("disconnected typed objective remains visible");
        assert_eq!(objective.id, "10207");
        assert_eq!(objective.title, "Project flow");
        assert_eq!(objective.current_stage, None);
        assert!(
            !pipeline.contains_key("01rfc000000000000000000002"),
            "a same-number RFC must not replace the stored typed identity"
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic == "project_flow.rfc_identity_missing: 01rfc000000000000000000001"
        }));
    }
}

fn git_status_porcelain(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output_guarded()
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
