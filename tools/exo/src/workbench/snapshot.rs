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

const MAX_TASK_PROGRESS_ENTRIES: usize = 8;
const MAX_TASK_PROGRESS_BYTES: usize = 16 * 1024;

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

pub(super) fn registered_git(registered: &WorkspaceRegistration) -> GitSnapshot {
    GitSnapshot {
        detached: registered.branch.is_none() && registered.head.is_some(),
        branch: registered.branch.clone(),
        head: registered.head.clone(),
        dirty: false,
    }
}

#[cfg(test)]
pub(super) fn build(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
) -> Result<WorkbenchSnapshot> {
    let git = sample_git(&registered.root);
    build_with_git(project, registered, revision, daemon_instance_id, git)
}

pub(super) fn build_with_git(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    git: GitSnapshot,
) -> Result<WorkbenchSnapshot> {
    build_with_git_and_after_state_hook(
        project,
        registered,
        revision,
        daemon_instance_id,
        git,
        || {},
    )
}

#[cfg(test)]
pub(super) fn build_with_after_state_hook(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    after_state: impl FnOnce(),
) -> Result<WorkbenchSnapshot> {
    let git = sample_git(&registered.root);
    build_with_git_and_after_state_hook(
        project,
        registered,
        revision,
        daemon_instance_id,
        git,
        after_state,
    )
}

fn build_with_git_and_after_state_hook(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    git: GitSnapshot,
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
                        .map(|task| {
                            let (progress, progress_truncated) =
                                bounded_task_progress(task.logs.iter().filter_map(|log| {
                                    (log.kind == "progress")
                                        .then_some((log.message.as_str(), log.created_at.as_str()))
                                }));
                            WorkbenchTask {
                                id: task.id.clone(),
                                title: task.title.clone(),
                                status: task.status.clone(),
                                progress,
                                progress_truncated,
                            }
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

fn bounded_task_progress<'a>(
    mut logs: impl DoubleEndedIterator<Item = (&'a str, &'a str)>,
) -> (Vec<WorkbenchTaskProgress>, bool) {
    let mut progress = Vec::with_capacity(MAX_TASK_PROGRESS_ENTRIES);
    let mut bytes = 0;
    let mut truncated = false;

    while let Some((message, created_at)) = logs.next_back() {
        if progress.len() == MAX_TASK_PROGRESS_ENTRIES {
            truncated = true;
            break;
        }

        let remaining = MAX_TASK_PROGRESS_BYTES.saturating_sub(bytes);
        if remaining == 0 {
            truncated = true;
            break;
        }
        let (message, message_truncated) = bounded_message(message, remaining);
        bytes += message.len();
        progress.push(WorkbenchTaskProgress {
            message,
            created_at: created_at.to_string(),
        });
        if message_truncated {
            truncated = true;
            break;
        }
    }

    progress.reverse();
    (progress, truncated)
}

fn bounded_message(message: &str, max_bytes: usize) -> (String, bool) {
    if message.len() <= max_bytes {
        return (message.to_string(), false);
    }
    if max_bytes <= 3 {
        return (".".repeat(max_bytes), true);
    }

    let mut end = max_bytes - 3;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}...", &message[..end]), true)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_progress_is_byte_bounded_on_utf8_boundaries() {
        let oversized = "é".repeat(MAX_TASK_PROGRESS_BYTES);
        let (progress, truncated) = bounded_task_progress(
            [
                ("Older progress.", "2026-07-30T00:00:00Z"),
                (oversized.as_str(), "2026-07-30T00:01:00Z"),
            ]
            .into_iter(),
        );

        assert!(truncated);
        assert_eq!(progress.len(), 1);
        assert!(progress[0].message.len() <= MAX_TASK_PROGRESS_BYTES);
        assert!(progress[0].message.ends_with("..."));
        assert_eq!(progress[0].created_at, "2026-07-30T00:01:00Z");
    }
}
