use super::{
    WorkbenchBetweenPhasesContext, WorkbenchCompletedPhaseSummary, WorkbenchDaemonIdentity,
    WorkbenchDiagnostic, WorkbenchGoal, WorkbenchLaneDetails, WorkbenchLaneInspection,
    WorkbenchLaneSummary, WorkbenchNextPhasePreview, WorkbenchPhase, WorkbenchProjectIdentity,
    WorkbenchProjectWorkspaceSummary, WorkbenchSnapshot, WorkbenchSnapshotWorkspace,
    WorkbenchSteering, WorkbenchSuggestedAction, WorkbenchTask, WorkbenchTaskProgress,
    WorkbenchWorkspaceLaneSummary, WorkbenchWorkspacePhaseSummary, WorkspaceProjection,
    WorkspaceRegistration,
};
use crate::api::protocol::ErrorCode;
use crate::context::sqlite_loader::PhaseDetailsData;
use crate::context::{Epoch, ExoState, Phase, SqliteLoader, WorkbenchLaneData};
use crate::failure::ExoFailure;
use crate::phase_owner::PhaseOwnerViewContext;
use crate::project::Project;
use crate::status::between_phases_context_for_epoch;
use crate::steering::derive_phase_steering;
use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_TASK_PROGRESS_ENTRIES: usize = 8;
const MAX_TASK_PROGRESS_BYTES: usize = 16 * 1024;
const MAX_OUTCOME_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub(super) struct GitSnapshot {
    pub(super) branch: Option<String>,
    pub(super) head: Option<String>,
    pub(super) detached: bool,
    pub(super) dirty: Option<bool>,
}

pub(super) fn sample_git(root: &Path) -> GitSnapshot {
    let branch = git_stdout(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    let dirty = git_stdout(root, &["status", "--porcelain=v1"]).map(|output| !output.is_empty());
    GitSnapshot {
        detached: branch.is_none() && head.is_some(),
        branch,
        head,
        dirty,
    }
}

pub(super) fn sample_git_identity(root: &Path) -> GitSnapshot {
    let branch = git_stdout(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    GitSnapshot {
        detached: branch.is_none() && head.is_some(),
        branch,
        head,
        dirty: None,
    }
}

impl GitSnapshot {
    pub(super) const fn unavailable() -> Self {
        Self {
            branch: None,
            head: None,
            detached: false,
            dirty: None,
        }
    }
}

pub(super) fn registered_git(registered: &WorkspaceRegistration) -> GitSnapshot {
    GitSnapshot {
        detached: registered.branch.is_none() && registered.head.is_some(),
        branch: registered.branch.clone(),
        head: registered.head.clone(),
        dirty: registered.dirty,
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
    let project_workspaces = vec![WorkspaceProjection {
        registration: registered.clone(),
        availability: "live",
        current: true,
    }];
    build_with_git_and_workspaces(
        project,
        registered,
        revision,
        daemon_instance_id,
        git,
        project_workspaces,
    )
}

pub(super) fn build_with_git_and_workspaces(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    git: GitSnapshot,
    project_workspaces: Vec<WorkspaceProjection>,
) -> Result<WorkbenchSnapshot> {
    build_with_git_and_after_state_hook(
        project,
        registered,
        revision,
        daemon_instance_id,
        git,
        project_workspaces,
        || {},
    )
}

pub(super) fn inspect_with_git(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    lane_id: &str,
    git: GitSnapshot,
) -> Result<WorkbenchLaneInspection> {
    let loader = SqliteLoader::open(project.db_path())?;
    let transaction = loader
        .database()
        .connection()
        .unchecked_transaction()
        .context("Failed to begin lane inspection read transaction")?;
    let plan = loader.load_state()?;
    let lane = loader
        .load_workbench_lane(lane_id)?
        .ok_or_else(|| anyhow::Error::new(lane_inspection_not_found(lane_id)))?;
    let workspace_root = registered.root.to_string_lossy();
    let focused_lane_id = loader
        .load_workspace_lane_focus(&workspace_root)?
        .map(|focus| focus.lane_id);
    let summary = lane_summary(&plan, &lane, focused_lane_id.as_deref())?;
    let phase = phase_for_id(&plan, &lane.execution_phase_id)
        .ok_or_else(|| anyhow::anyhow!("workbench lane references a missing phase"))?;
    let details = loader
        .load_phase_details_by_id_with_bounded_history(
            &phase.id,
            MAX_TASK_PROGRESS_ENTRIES,
            MAX_TASK_PROGRESS_BYTES,
            MAX_OUTCOME_BYTES,
        )?
        .ok_or_else(|| anyhow::anyhow!("lane phase details are missing"))?;
    let relationship = match phase.status.as_str() {
        "in-progress" if summary.focused_here => "focused_here",
        "in-progress" => "focusable_here",
        "pending" => "prepared",
        _ => "historical",
    };
    let can_focus_here = relationship == "focusable_here";
    let phase = workbench_phase(phase, &details, false, true);
    transaction
        .commit()
        .context("Failed to finish lane inspection read transaction")?;

    Ok(WorkbenchLaneInspection {
        kind: "workbench.lane_inspection",
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
        workspace: workspace_snapshot(registered, git),
        relationship: relationship.to_string(),
        can_focus_here,
        lane: WorkbenchLaneDetails {
            summary,
            intent: lane.intent,
            created_at: lane.created_at,
            updated_at: lane.updated_at,
        },
        phase,
    })
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
    let project_workspaces = vec![WorkspaceProjection {
        registration: registered.clone(),
        availability: "live",
        current: true,
    }];
    build_with_git_and_after_state_hook(
        project,
        registered,
        revision,
        daemon_instance_id,
        git,
        project_workspaces,
        after_state,
    )
}

fn build_with_git_and_after_state_hook(
    project: &Project,
    registered: &WorkspaceRegistration,
    revision: u64,
    daemon_instance_id: &str,
    git: GitSnapshot,
    project_workspaces: Vec<WorkspaceProjection>,
    after_state: impl FnOnce(),
) -> Result<WorkbenchSnapshot> {
    let mut workspace_project = project.clone();
    workspace_project.workspace_root = Some(registered.root.clone());
    let phase_owner_context =
        PhaseOwnerViewContext::new(&registered.root, Some(&workspace_project));
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
    let workspace_phase_id = loader.load_workspace_active_phase(&workspace_root)?;

    let lane_summaries = lanes
        .iter()
        .map(|lane| lane_summary(&plan, lane, focused_lane_id.as_deref()))
        .collect::<Result<Vec<_>>>()?;
    let project_workspaces =
        project_workspace_summaries(&loader, &plan, &lanes, project_workspaces)?;
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
    let planning_available = focused_phase
        .map(|phase| {
            loader.load_phase_owner(&phase.id).map(|owner| {
                owner
                    .as_ref()
                    .is_none_or(|owner| phase_owner_context.owner_view(owner).owned_here)
            })
        })
        .transpose()?
        .unwrap_or(false);
    let phase = focused_phase
        .zip(focused_phase_details.as_ref())
        .map(|(phase, details)| workbench_phase(phase, details, planning_available, false));
    let between_phases_context =
        between_phases_epoch(&plan, workspace_phase_id.as_deref()).map(|epoch| {
            let context = between_phases_context_for_epoch(epoch);
            WorkbenchBetweenPhasesContext {
                epoch_id: context.epoch_id,
                epoch_title: context.epoch_title,
                completed_phase: context.completed_phase.map(|phase| {
                    WorkbenchCompletedPhaseSummary {
                        id: phase.phase_id,
                        title: phase.phase_title,
                        completed_at: phase.completed_at,
                        goal_count: phase.goal_count,
                        completed_goals: phase.completed_goals,
                    }
                }),
                next_phase: context.next_phase.map(|phase| WorkbenchNextPhasePreview {
                    id: phase.id,
                    title: phase.title,
                    goal_count: phase.goal_count,
                    rfc_count: phase.rfcs.len(),
                }),
                pending_phases: epoch
                    .phases
                    .iter()
                    .filter(|phase| phase.status == "pending")
                    .count(),
            }
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
    let diagnostics = focus_diagnostics(focused_lane.as_ref(), workspace_phase_id.as_deref());
    transaction
        .commit()
        .context("Failed to finish workbench snapshot read transaction")?;

    Ok(WorkbenchSnapshot {
        kind: "workbench.snapshot",
        ok: true,
        schema_version: 3,
        observed_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        revision,
        project: WorkbenchProjectIdentity {
            id: project.id.to_string(),
        },
        daemon: WorkbenchDaemonIdentity {
            instance_id: daemon_instance_id.to_string(),
        },
        workspace: workspace_snapshot(registered, git),
        project_workspaces,
        lanes: lane_summaries,
        focused_lane,
        phase,
        between_phases_context,
        steering,
        diagnostics,
    })
}

fn workbench_phase(
    phase: &Phase,
    details: &PhaseDetailsData,
    planning_available: bool,
    include_outcomes: bool,
) -> WorkbenchPhase {
    WorkbenchPhase {
        planning_available,
        id: phase.id.clone(),
        title: phase.title.clone(),
        status: phase.status.clone(),
        goals: details
            .goals
            .iter()
            .map(|goal| {
                let (outcome, outcome_truncated) = if include_outcomes {
                    bounded_outcome(goal.completion_log.as_deref())
                } else {
                    (None, false)
                };
                WorkbenchGoal {
                    id: goal.id.clone(),
                    title: goal.title.clone(),
                    status: goal.status.clone(),
                    outcome,
                    outcome_truncated,
                    tasks: goal
                        .tasks
                        .iter()
                        .map(|task| {
                            let (progress, progress_truncated) =
                                bounded_task_progress(task.logs.iter().filter_map(|log| {
                                    (log.kind == "progress")
                                        .then_some((log.message.as_str(), log.created_at.as_str()))
                                }));
                            let (outcome, outcome_truncated) = if include_outcomes {
                                bounded_outcome(task.completion_log.as_deref())
                            } else {
                                (None, false)
                            };
                            WorkbenchTask {
                                id: task.id.clone(),
                                title: task.title.clone(),
                                status: task.status.clone(),
                                outcome,
                                outcome_truncated,
                                progress,
                                progress_truncated,
                            }
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

fn workspace_snapshot(
    registered: &WorkspaceRegistration,
    git: GitSnapshot,
) -> WorkbenchSnapshotWorkspace {
    let label = git
        .branch
        .clone()
        .or_else(|| {
            git.head
                .as_deref()
                .map(|head| format!("detached@{}", &head[..head.len().min(8)]))
        })
        .unwrap_or_else(|| registered.label.clone());
    WorkbenchSnapshotWorkspace {
        key: registered.key.clone(),
        label,
        branch: git.branch,
        head: git.head,
        detached: git.detached,
        dirty: git.dirty.unwrap_or(true),
    }
}

fn project_workspace_summaries(
    loader: &SqliteLoader,
    plan: &ExoState,
    lanes: &[WorkbenchLaneData],
    projections: Vec<WorkspaceProjection>,
) -> Result<Vec<WorkbenchProjectWorkspaceSummary>> {
    projections
        .into_iter()
        .map(|projection| {
            let workspace_root = projection.registration.root.to_string_lossy();
            let focused_lane =
                loader
                    .load_workspace_lane_focus(&workspace_root)?
                    .and_then(|focus| {
                        lanes
                            .iter()
                            .find(|lane| lane.text_id == focus.lane_id)
                            .and_then(|lane| {
                                let phase = phase_for_id(plan, &lane.execution_phase_id)?;
                                Some(WorkbenchWorkspaceLaneSummary {
                                    id: lane.text_id.clone(),
                                    title: lane.title.clone(),
                                    state: lane.state.clone(),
                                    phase_id: phase.id.clone(),
                                    phase_title: phase.title.clone(),
                                    phase_status: phase.status.clone(),
                                })
                            })
                    });
            let active_phase = loader
                .load_workspace_active_phase(&workspace_root)?
                .and_then(|phase_id| phase_for_id(plan, &phase_id))
                .map(|phase| WorkbenchWorkspacePhaseSummary {
                    id: phase.id.clone(),
                    title: phase.title.clone(),
                    status: phase.status.clone(),
                });
            let registration = projection.registration;
            let detached = registration.branch.is_none() && registration.head.is_some();
            Ok(WorkbenchProjectWorkspaceSummary {
                key: registration.key,
                label: registration.label,
                current: projection.current,
                availability: projection.availability.to_string(),
                observed_at: registration
                    .observed_at
                    .map(super::timestamp_for_unix_seconds),
                branch: registration.branch,
                head: registration.head,
                detached,
                dirty: registration.dirty,
                focused_lane,
                active_phase,
            })
        })
        .collect()
}

fn bounded_outcome(outcome: Option<&str>) -> (Option<String>, bool) {
    match outcome {
        Some(outcome) => {
            let (outcome, truncated) = bounded_message(outcome, MAX_OUTCOME_BYTES);
            (Some(outcome), truncated)
        }
        None => (None, false),
    }
}

fn lane_inspection_not_found(lane_id: &str) -> ExoFailure {
    ExoFailure::new(
        ErrorCode::NotFound,
        format!("Workbench lane not found: {lane_id}"),
        ExoFailure::orienting_steering(vec![]),
    )
    .with_details(serde_json::json!({
        "kind": "workbench.lane_not_found",
        "lane_id": lane_id,
    }))
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

fn between_phases_epoch<'a>(
    plan: &'a ExoState,
    workspace_phase_id: Option<&str>,
) -> Option<&'a Epoch> {
    let phase = plan.find_phase_by_id(workspace_phase_id?)?;
    (phase.phase.status != "in-progress" && phase.epoch.derived_status() == "in-progress")
        .then_some(phase.epoch)
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

    #[test]
    fn recorded_outcomes_are_byte_bounded_on_utf8_boundaries() {
        let oversized = "é".repeat(MAX_OUTCOME_BYTES);
        let (outcome, truncated) = bounded_outcome(Some(&oversized));
        let outcome = outcome.expect("bounded outcome");

        assert!(truncated);
        assert!(outcome.len() <= MAX_OUTCOME_BYTES);
        assert!(outcome.ends_with("..."));
    }
}
