use super::{
    WorkbenchDaemonIdentity, WorkbenchDiagnostic, WorkbenchGoal, WorkbenchLaneDetails,
    WorkbenchLaneSummary, WorkbenchPhase, WorkbenchProjectIdentity, WorkbenchSnapshot,
    WorkbenchSnapshotWorkspace, WorkbenchSteering, WorkbenchSuggestedAction, WorkbenchTask,
    WorkbenchTaskProgress, WorkspaceRegistration,
};
use crate::context::{ExoState, Phase, SqliteLoader, WorkbenchLaneData};
use crate::project::Project;
use crate::steering::derive_phase_steering;
use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub(super) struct GitSnapshot {
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) detached: bool,
    pub(super) dirty: bool,
}

pub(super) fn sample_git(root: &Path) -> GitSnapshot {
    let branch = git_stdout(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    let dirty =
        git_stdout(root, &["status", "--porcelain=v1"]).is_some_and(|output| !output.is_empty());
    GitSnapshot {
        detached: branch.is_none() && head.is_some(),
        branch,
        head,
        dirty,
    }
}

pub(super) fn build(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
) -> Result<WorkbenchSnapshot> {
    build_with_after_state_hook(project, registered, revision, daemon_instance_id, || {})
}

pub(super) fn build_with_after_state_hook(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    after_state: impl FnOnce(),
) -> Result<WorkbenchSnapshot> {
    let loader = SqliteLoader::open(project.db_path())?;
    let transaction = loader
        .database()
        .connection()
        .unchecked_transaction()
        .context("Failed to begin workbench snapshot read transaction")?;
    let plan = loader.load_state()?;
    after_state();
    let lanes = loader.load_workbench_lanes()?;
    let workspace_root = registered.root.to_string_lossy();
    let focused_lane_id = loader
        .load_workspace_lane_focus(&workspace_root)?
        .map(|focus| focus.lane_id);
    let focused_phase_id = loader.load_workspace_active_phase(&workspace_root)?;

    let lane_summaries = lanes
        .iter()
        .map(|lane| lane_summary(&plan, lane, focused_lane_id.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    let focused_lane = focused_lane_id
        .as_deref()
        .and_then(|id| lanes.iter().find(|lane| lane.text_id == id))
        .map(|lane| {
            Ok::<WorkbenchLaneDetails, anyhow::Error>(WorkbenchLaneDetails {
                summary: lane_summary(&plan, lane, focused_lane_id.as_deref())?,
                intent: lane.intent.clone(),
                created_at: lane.created_at.clone(),
                updated_at: lane.updated_at.clone(),
            })
        })
        .transpose()?;
    let focused_phase = focused_lane
        .as_ref()
        .and_then(|lane| phase_for_id(&plan, &lane.summary.phase_id));
    let focused_phase_details = focused_phase
        .map(|phase| {
            loader
                .load_phase_details_by_id(&phase.id)?
                .ok_or_else(|| anyhow::anyhow!("focused lane phase details are missing"))
        })
        .transpose()?;
    let phase = focused_phase
        .zip(focused_phase_details.as_ref())
        .map(|(phase, details)| WorkbenchPhase {
            id: phase.id.clone(),
            title: phase.title.clone(),
            status: phase.status.clone(),
            goals: details
                .goals
                .iter()
                .map(|goal| WorkbenchGoal {
                    id: goal.id.clone(),
                    title: goal.title.clone(),
                    status: goal.status.clone(),
                    tasks: goal
                        .tasks
                        .iter()
                        .map(|task| WorkbenchTask {
                            id: task.id.clone(),
                            title: task.title.clone(),
                            status: task.status.clone(),
                            progress: task
                                .logs
                                .iter()
                                .filter(|log| log.kind == "progress")
                                .map(|log| WorkbenchTaskProgress {
                                    message: log.message.clone(),
                                    created_at: log.created_at.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        });
    let steering = focused_phase
        .zip(focused_phase_details.as_ref())
        .map_or_else(
            || WorkbenchSteering {
                situation: "No workbench lane is focused in this workspace.".to_string(),
                next_actions: vec![],
            },
            |(phase, details)| {
                let tasks = details
                    .goals
                    .iter()
                    .flat_map(|goal| goal.tasks.iter())
                    .map(|task| (task.id.clone(), task.title.clone(), task.status.clone()))
                    .collect::<Vec<_>>();
                let derived = derive_phase_steering(&tasks, &phase.goals, phase.kind);
                WorkbenchSteering {
                    situation: derived.situation,
                    next_actions: derived
                        .next_actions
                        .into_iter()
                        .map(|action| WorkbenchSuggestedAction {
                            label: action.label,
                            command: action.command,
                            rationale: action.rationale,
                            intent: action.intent.as_str().to_string(),
                            confidence: action.confidence.map(|confidence| {
                                (f64::from(confidence) * 1_000_000.0).round() / 1_000_000.0
                            }),
                        })
                        .collect(),
                }
            },
        );
    let diagnostics = focus_diagnostics(focused_lane.as_ref(), focused_phase_id.as_deref());
    transaction
        .commit()
        .context("Failed to finish workbench snapshot read transaction")?;

    let git = sample_git(&registered.root);
    let label = git
        .branch
        .clone()
        .or_else(|| {
            git.head
                .as_deref()
                .map(|head| format!("detached@{}", &head[..head.len().min(8)]))
        })
        .unwrap_or_else(|| registered.label.clone());

    Ok(WorkbenchSnapshot {
        kind: "workbench.snapshot",
        ok: true,
        schema_version: 1,
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        revision,
        project: WorkbenchProjectIdentity {
            id: project.id.to_string(),
        },
        daemon: WorkbenchDaemonIdentity {
            instance_id: daemon_instance_id.to_string(),
        },
        workspace: WorkbenchSnapshotWorkspace {
            key: registered.key.clone(),
            label,
            branch: git.branch,
            head: git.head,
            detached: git.detached,
            dirty: git.dirty,
        },
        lanes: lane_summaries,
        focused_lane,
        phase,
        steering,
        diagnostics,
    })
}

fn lane_summary(
    plan: &ExoState,
    lane: &WorkbenchLaneData,
    focused_lane_id: Option<&str>,
) -> Result<WorkbenchLaneSummary> {
    let phase = phase_for_id(plan, &lane.execution_phase_id)
        .ok_or_else(|| anyhow::anyhow!("workbench lane references a missing phase"))?;
    Ok(WorkbenchLaneSummary {
        id: lane.text_id.clone(),
        title: lane.title.clone(),
        state: lane.state.clone(),
        phase_id: phase.id.clone(),
        phase_title: phase.title.clone(),
        phase_status: phase.status.clone(),
        focused_here: focused_lane_id == Some(lane.text_id.as_str()),
    })
}

fn phase_for_id<'a>(plan: &'a ExoState, id: &str) -> Option<&'a Phase> {
    plan.find_phase_by_id(id).map(|info| info.phase)
}

fn focus_diagnostics(
    focused_lane: Option<&WorkbenchLaneDetails>,
    focused_phase_id: Option<&str>,
) -> Vec<WorkbenchDiagnostic> {
    let Some(lane) = focused_lane else {
        return vec![];
    };
    if focused_phase_id == Some(lane.summary.phase_id.as_str())
        && lane.summary.phase_status == "in-progress"
    {
        return vec![];
    }
    let message = if focused_phase_id == Some(lane.summary.phase_id.as_str()) {
        format!(
            "Focused lane '{}' belongs to phase '{}', but that phase is {} rather than in-progress",
            lane.summary.id, lane.summary.phase_id, lane.summary.phase_status
        )
    } else {
        format!(
            "Focused lane '{}' belongs to phase '{}', but this workspace's focused phase is {}",
            lane.summary.id,
            lane.summary.phase_id,
            focused_phase_id.unwrap_or("unset")
        )
    };
    vec![WorkbenchDiagnostic {
        code: "lane.phase_focus_mismatch".to_string(),
        severity: "warning".to_string(),
        message,
    }]
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
