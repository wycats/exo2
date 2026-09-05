#![allow(clippy::redundant_pub_crate)]

use crate::ExoResult;
use crate::api::protocol::ErrorCode;
use crate::context::ExoState;
use crate::failure::ExoFailure;
use crate::process_spawn::CommandSpawnExt as _;
use crate::steering::{SuggestedAction, WorkIntent};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

const DETAILS_TRUNCATE_LIMIT: usize = 4096;

fn truncate_for_details(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    // Keep the beginning; it's usually where Git prints the actionable line.
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push('…');
    out
}

fn run_git_capture(root: &Path, args: &[&str]) -> ExoResult<Output> {
    Ok(Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output_with_configured_stdio_guarded()?)
}

fn escape_commit_message_for_cmd(msg: &str) -> String {
    msg.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

fn suggested_show_git_status() -> SuggestedAction {
    SuggestedAction {
        label: "Show git status".to_string(),
        command: "git status".to_string(),
        rationale: "Inspect what is staged and what remains unstaged.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.7),
    }
}

fn suggested_retry_add() -> SuggestedAction {
    SuggestedAction {
        label: "Retry add".to_string(),
        command: "git add .".to_string(),
        rationale: "Run git directly to see full error output.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }
}

fn suggested_retry_commit(msg: &str) -> SuggestedAction {
    SuggestedAction {
        label: "Retry commit".to_string(),
        command: format!(
            "git commit -S -m \"{}\"",
            escape_commit_message_for_cmd(msg)
        ),
        rationale: "Run git directly to see full error output.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }
}

/// Result of a successful phase finish, containing information for the caller.
#[derive(Debug, Serialize)]
pub(crate) struct PhaseFinishResult {
    /// The ID of the phase that was finished.
    pub phase_id: String,
    /// RFC promotion suggestions for phase-attached RFCs.
    pub rfc_suggestions: Vec<RfcSuggestion>,
    /// Diagnostics produced while resolving canonical campaign RFC objectives.
    pub rfc_diagnostics: Vec<String>,
    /// Info about the next pending phase in the epoch, if any.
    pub next_phase: Option<NextPhaseInfo>,
}

/// A suggestion about an RFC attached to the completed phase.
#[derive(Debug, Serialize)]
pub(crate) struct RfcSuggestion {
    pub rfc_id: String,
    pub rfc_ulid: String,
    pub title: String,
    pub current_stage: Option<u8>,
    pub lifecycle: Option<String>,
    pub superseded_by: Option<String>,
    pub target_stage: Option<u8>,
    pub motion: crate::project_flow::RfcObjectiveMotion,
    pub suggestion: String,
    pub is_driving: bool,
}

/// Information about the next pending phase in the epoch.
#[derive(Debug, Serialize)]
pub(crate) struct NextPhaseInfo {
    pub phase_id: String,
    pub phase_title: String,
    pub epoch_title: String,
    pub rfc_ids: Vec<String>,
}

pub(crate) fn finish_phase(
    root: &Path,
    db_path: &Path,
    plan: &ExoState,
    active_phase_id: Option<String>,
    message: Option<String>,
    emit_output: bool,
) -> ExoResult<PhaseFinishResult> {
    // 1. Find active phase
    let Some(active_phase_id) = active_phase_id else {
        let failure = ExoFailure::new(
            ErrorCode::NotFound,
            "No active phase found to finish.",
            ExoFailure::orienting_steering(vec![
                SuggestedAction {
                    label: "Show phase status".to_string(),
                    command: "exo phase status --full".to_string(),
                    rationale: "Confirm whether a phase is currently active.".to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.8),
                },
                SuggestedAction {
                    label: "Review plan".to_string(),
                    command: "exo plan review".to_string(),
                    rationale: "Find a phase ID to start or continue.".to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.7),
                },
                SuggestedAction {
                    label: "Start a phase".to_string(),
                    command: "exo phase start <id>".to_string(),
                    rationale: "Activate the phase you want to work on.".to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.6),
                },
            ]),
        )
        .with_details(serde_json::json!({
            "command": "phase.finish"
        }));

        return Err(failure.into());
    };

    // 2. Check for uncommitted changes
    let output = run_git_capture(root, &["status", "--porcelain"])?;
    if !output.status.success() {
        let failure = ExoFailure::new(
            ErrorCode::Internal,
            "Failed to check git status. Phase finish aborted.",
            ExoFailure::orienting_steering(vec![suggested_show_git_status()]),
        )
        .with_details(serde_json::json!({
            "command": "git status --porcelain",
            "exit_code": output.status.code(),
            "stdout": truncate_for_details(&String::from_utf8_lossy(&output.stdout), DETAILS_TRUNCATE_LIMIT),
            "stderr": truncate_for_details(&String::from_utf8_lossy(&output.stderr), DETAILS_TRUNCATE_LIMIT),
        }));
        return Err(failure.into());
    }

    let is_dirty = !output.stdout.is_empty();

    if is_dirty {
        if let Some(msg) = message {
            if emit_output {
                println!("Uncommitted changes detected. Committing...");
            }

            // Always capture git output so stdout remains deterministic and JSON mode
            // stays a single JSON value.
            let add = run_git_capture(root, &["add", "."])?;
            if !add.status.success() {
                let steering = if emit_output {
                    ExoFailure::orienting_steering(vec![
                        suggested_show_git_status(),
                        suggested_retry_add(),
                    ])
                } else {
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "Show git status".to_string(),
                        command: "git status".to_string(),
                        rationale: "Inspect the working tree and staged changes.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.7),
                    }])
                };

                let failure = ExoFailure::new(
                    ErrorCode::Internal,
                    "Failed to add changes to index. Phase finish aborted.",
                    steering,
                )
                .with_details(serde_json::json!({
                    "command": "git add .",
                    "exit_code": add.status.code(),
                    "stdout": truncate_for_details(&String::from_utf8_lossy(&add.stdout), DETAILS_TRUNCATE_LIMIT),
                    "stderr": truncate_for_details(&String::from_utf8_lossy(&add.stderr), DETAILS_TRUNCATE_LIMIT),
                }));
                return Err(failure.into());
            }

            let commit = run_git_capture(root, &["commit", "-S", "-m", &msg])?;
            if !commit.status.success() {
                let failure = ExoFailure::new(
                    ErrorCode::Internal,
                    "Failed to commit changes. Phase finish aborted.",
                    ExoFailure::orienting_steering(vec![
                        SuggestedAction {
                            label: "Show git status".to_string(),
                            command: "git status".to_string(),
                            rationale: "Confirm what is staged and what remains uncommitted.".to_string(),
                            intent: WorkIntent::Orient,
                            confidence: Some(0.7),
                        },
                        suggested_retry_commit(&msg),
                    ]),
                )
                .with_details(serde_json::json!({
                    "command": "git commit -S -m <message>",
                    "exit_code": commit.status.code(),
                    "stdout": truncate_for_details(&String::from_utf8_lossy(&commit.stdout), DETAILS_TRUNCATE_LIMIT),
                    "stderr": truncate_for_details(&String::from_utf8_lossy(&commit.stderr), DETAILS_TRUNCATE_LIMIT),
                }));
                return Err(failure.into());
            }
        } else {
            let failure = ExoFailure::new(
                ErrorCode::InvalidInput,
                "Working directory is dirty. Please commit your changes or use --message to commit automatically.",
                ExoFailure::orienting_steering(vec![
                    SuggestedAction {
                        label: "Show git status".to_string(),
                        command: "git status".to_string(),
                        rationale: "See what is uncommitted before finishing the phase.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.8),
                    },
                    SuggestedAction {
                        label: "Finish phase with message".to_string(),
                        command: "exo phase finish --message \"...\"".to_string(),
                        rationale: "Let exo commit for you and then complete the phase.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.7),
                    },
                    SuggestedAction {
                        label: "Commit manually".to_string(),
                        command: "git commit -S -m \"...\"".to_string(),
                        rationale: "Commit your changes, then rerun `exo phase finish`.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.6),
                    },
                ]),
            )
            .with_details(serde_json::json!({
                "command": "phase.finish",
                "phase_id": active_phase_id,
                "dirty": true,
                "requires_message": true,
            }));

            return Err(failure.into());
        }
    } else if message.is_some() && emit_output {
        println!("Working directory clean. Nothing to commit.");
    }

    // 3. Collect RFC info for the completed phase before marking it done
    let rfc_info = collect_phase_rfc_info(root, &active_phase_id);

    // 4. Update status to completed
    {
        let writer = crate::context::SqliteWriter::open(db_path)?;
        writer.complete_phase_and_clear_lane_focus(&active_phase_id)?;
    }
    if emit_output {
        println!("Marked phase '{active_phase_id}' as completed.");
    }

    // 5. Find next phase in the SAME epoch (for informational output only — no auto-activation)
    let mut next_phase_in_epoch: Option<(&crate::context::Phase, &str)> = None;
    'scan: for epoch in &plan.epochs {
        let mut found_active = false;
        for phase in &epoch.phases {
            if phase.id == active_phase_id {
                found_active = true;
                continue;
            }
            if found_active && phase.status == "pending" {
                next_phase_in_epoch = Some((phase, &epoch.title));
                break 'scan;
            }
        }
        if found_active {
            break;
        }
    }

    // Build the result
    let next_phase = next_phase_in_epoch.map(|(next, epoch_title)| NextPhaseInfo {
        phase_id: next.id.clone(),
        phase_title: next.title.clone(),
        epoch_title: epoch_title.to_string(),
        rfc_ids: next.rfcs.iter().map(|r| r.id.clone()).collect(),
    });

    if emit_output {
        // Print RFC suggestions
        for diagnostic in &rfc_info.diagnostics {
            println!("\nRFC objective diagnostic: {diagnostic}");
        }
        for suggestion in &rfc_info.suggestions {
            let stage = suggestion
                .current_stage
                .map_or_else(|| "unavailable".to_string(), |stage| stage.to_string());
            println!(
                "\nRFC {}: {} (Stage {})",
                suggestion.rfc_id, suggestion.title, stage
            );
            println!("  → {}", suggestion.suggestion);
        }

        // Print next phase info
        if let Some(ref next) = next_phase {
            println!("\n--------------------------------------------------");
            println!("Epoch: {}", next.epoch_title);
            println!("Next Phase: {} ({})", next.phase_title, next.phase_id);
            if !next.rfc_ids.is_empty() {
                println!("RFCs: {}", next.rfc_ids.join(", "));
            }
            println!("--------------------------------------------------");
            println!("Run `exo phase start` to begin the next phase.");
        } else {
            println!("\nNo pending phases found in this epoch. Time to plan or start a new epoch!");
        }
    }

    Ok(PhaseFinishResult {
        phase_id: active_phase_id,
        rfc_suggestions: rfc_info.suggestions,
        rfc_diagnostics: rfc_info.diagnostics,
        next_phase,
    })
}

struct PhaseRfcInfo {
    suggestions: Vec<RfcSuggestion>,
    diagnostics: Vec<String>,
}

/// Project phase suggestions from the canonical typed-first campaign resolver.
fn collect_phase_rfc_info(root: &Path, phase_id: &str) -> PhaseRfcInfo {
    let result = crate::project::Project::resolve(root).and_then(|project| {
        let effective_rfcs = crate::rfc::load_effective_rfcs(root, Some(&project))?;
        crate::project_flow::campaign_rfc_objectives_with_effective_rfcs(
            &project.db_path(),
            phase_id,
            &effective_rfcs,
        )
    });
    let (objectives, diagnostics) = match result {
        Ok(resolved) => resolved,
        Err(error) => {
            return PhaseRfcInfo {
                suggestions: Vec::new(),
                diagnostics: vec![format!("project_flow.objective_resolution_failed: {error}")],
            };
        }
    };
    phase_rfc_info_for_workspace(root, objectives, diagnostics)
}

fn phase_rfc_info_for_workspace(
    root: &Path,
    objectives: Vec<crate::project_flow::RfcObjectiveView>,
    diagnostics: Vec<String>,
) -> PhaseRfcInfo {
    let unavailable_numbers = objectives
        .iter()
        .filter(|objective| {
            crate::rfc::find_rfc_file(&root.join("docs/rfcs"), &objective.rfc_number.to_string())
                .ok()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|content| crate::rfc::extract_anchor_ulid(&content))
                .is_none_or(|ulid| ulid != objective.rfc_ulid)
        })
        .map(|objective| objective.rfc_number)
        .collect::<HashSet<_>>();
    phase_rfc_info_from_objectives(objectives, diagnostics, &unavailable_numbers)
}

fn phase_rfc_info_from_objectives(
    objectives: Vec<crate::project_flow::RfcObjectiveView>,
    diagnostics: Vec<String>,
    ambiguous_rfc_numbers: &HashSet<i64>,
) -> PhaseRfcInfo {
    let suggestions = objectives
        .into_iter()
        .map(|objective| {
            let motion = objective.motion();
            let suggestion = match motion {
                crate::project_flow::RfcObjectiveMotion::IdentityMissing => {
                    "Canonical RFC identity is unavailable. Repair the project-flow relationship before promotion."
                        .to_string()
                }
                crate::project_flow::RfcObjectiveMotion::Terminal => {
                    let lifecycle = objective.lifecycle.as_deref().unwrap_or("terminal");
                    match objective.superseded_by.as_deref() {
                        Some(successor) => format!(
                            "RFC is {lifecycle} and superseded by {successor}. No promotion is available."
                        ),
                        None => format!("RFC is {lifecycle}. No promotion is available."),
                    }
                }
                crate::project_flow::RfcObjectiveMotion::TargetReached => format!(
                    "Already at Stage {} (target was {}). No promotion needed.",
                    objective.current_stage.expect("reached target has a current stage"),
                    objective.target_stage.expect("reached target has a target stage")
                ),
                crate::project_flow::RfcObjectiveMotion::Advancing => {
                    let current_stage = objective
                        .current_stage
                        .expect("advancing objective has a current stage");
                    let target_stage = objective
                        .target_stage
                        .expect("advancing objective has a target stage");
                    if ambiguous_rfc_numbers.contains(&objective.rfc_number) {
                        format!(
                            "Currently Stage {current_stage}, target Stage {target_stage}. RFC number {} does not select a unique workspace document with the attached identity; repair the workspace document before promotion (attached RFC {}).",
                            objective.rfc_number, objective.rfc_ulid
                        )
                    } else {
                        format!(
                            "Currently Stage {current_stage}, target Stage {target_stage}. Consider: `exo rfc promote {} --stage {}`",
                            objective.rfc_number, current_stage + 1
                        )
                    }
                }
                crate::project_flow::RfcObjectiveMotion::Associated => {
                    let current_stage = objective
                        .current_stage
                        .expect("associated objective has a current stage");
                    if current_stage == 4 {
                        "Stable (Stage 4). No action needed.".to_string()
                    } else {
                        format!(
                            "Associated RFC at Stage {current_stage}. No targeted promotion is pending."
                        )
                    }
                }
            };
            RfcSuggestion {
                rfc_id: format!("{:05}", objective.rfc_number),
                rfc_ulid: objective.rfc_ulid,
                title: objective.title,
                current_stage: objective.current_stage,
                lifecycle: objective.lifecycle,
                superseded_by: objective.superseded_by,
                target_stage: objective.target_stage,
                motion,
                suggestion,
                is_driving: matches!(objective.relation.as_str(), "drives" | "driving"),
            }
        })
        .collect();
    PhaseRfcInfo {
        suggestions,
        diagnostics,
    }
}

#[cfg(test)]
mod project_flow_tests {
    use super::*;
    use crate::project_flow::RfcObjectiveView;

    #[test]
    fn phase_projection_retains_disconnected_typed_objective() {
        let info = phase_rfc_info_from_objectives(
            vec![RfcObjectiveView {
                rfc_ulid: "01rfc000000000000000000001".to_string(),
                rfc_number: 10207,
                title: "Stored project-flow title".to_string(),
                observed_stage: Some(2),
                current_stage: None,
                lifecycle: None,
                superseded_by: None,
                target_stage: Some(3),
                relation: "drives".to_string(),
                source: "typed".to_string(),
                diagnostic: Some(
                    "project_flow.rfc_identity_missing: 01rfc000000000000000000001".to_string(),
                ),
            }],
            vec!["project_flow.rfc_identity_missing: 01rfc000000000000000000001".to_string()],
            &HashSet::new(),
        );

        assert_eq!(info.suggestions.len(), 1);
        assert_eq!(info.suggestions[0].rfc_id, "10207");
        assert_eq!(info.suggestions[0].rfc_ulid, "01rfc000000000000000000001");
        assert_eq!(info.suggestions[0].title, "Stored project-flow title");
        assert_eq!(info.suggestions[0].current_stage, None);
        assert!(
            info.suggestions[0]
                .suggestion
                .contains("identity is unavailable")
        );
        assert_eq!(info.diagnostics.len(), 1);
    }

    #[test]
    fn phase_projection_reports_terminal_objective_without_promotion() {
        let info = phase_rfc_info_from_objectives(
            vec![RfcObjectiveView {
                rfc_ulid: "01rfc000000000000000000001".to_string(),
                rfc_number: 10207,
                title: "Stored project-flow title".to_string(),
                observed_stage: Some(2),
                current_stage: Some(2),
                lifecycle: Some("superseded".to_string()),
                superseded_by: Some("10208".to_string()),
                target_stage: Some(3),
                relation: "drives".to_string(),
                source: "typed".to_string(),
                diagnostic: None,
            }],
            Vec::new(),
            &HashSet::new(),
        );

        assert!(
            info.suggestions[0]
                .suggestion
                .contains("superseded by 10208")
        );
        assert!(!info.suggestions[0].suggestion.contains("exo rfc promote"));
        assert_eq!(info.suggestions[0].lifecycle.as_deref(), Some("superseded"));
    }

    #[test]
    fn phase_projection_only_prompts_for_a_strictly_future_target() {
        let objective = |current_stage, target_stage| RfcObjectiveView {
            rfc_ulid: format!("01rfc{current_stage}{target_stage:?}"),
            rfc_number: 10207,
            title: "Project flow".to_string(),
            observed_stage: Some(current_stage),
            current_stage: Some(current_stage),
            lifecycle: Some("active".to_string()),
            superseded_by: None,
            target_stage,
            relation: "drives".to_string(),
            source: "typed".to_string(),
            diagnostic: None,
        };
        let info = phase_rfc_info_from_objectives(
            vec![
                objective(3, Some(3)),
                objective(4, None),
                objective(2, Some(4)),
            ],
            Vec::new(),
            &HashSet::new(),
        );

        assert_eq!(
            info.suggestions[0].motion,
            crate::project_flow::RfcObjectiveMotion::TargetReached
        );
        assert!(!info.suggestions[0].suggestion.contains("exo rfc promote"));
        assert_eq!(
            info.suggestions[1].motion,
            crate::project_flow::RfcObjectiveMotion::Associated
        );
        assert!(!info.suggestions[1].suggestion.contains("exo rfc promote"));
        assert_eq!(
            info.suggestions[2].motion,
            crate::project_flow::RfcObjectiveMotion::Advancing
        );
        assert!(info.suggestions[2].suggestion.contains("exo rfc promote"));
        assert!(info.suggestions[2].suggestion.contains("target Stage 4"));
        assert!(info.suggestions[2].suggestion.contains("--stage 3"));
    }

    #[test]
    fn phase_projection_does_not_suggest_an_ambiguous_numeric_promotion() {
        let temp = tempfile::tempdir().unwrap();
        let stage = temp.path().join("docs/rfcs/stage-2");
        std::fs::create_dir_all(&stage).unwrap();
        for name in ["10207-first.md", "10207-duplicate.md"] {
            std::fs::write(
                stage.join(name),
                "<!-- exo:10207 ulid:01rfc000000000000000000001 -->\n# Project flow\n",
            )
            .unwrap();
        }
        let info = phase_rfc_info_for_workspace(
            temp.path(),
            vec![RfcObjectiveView {
                rfc_ulid: "01rfc000000000000000000001".to_string(),
                rfc_number: 10207,
                title: "Project flow".to_string(),
                observed_stage: Some(2),
                current_stage: Some(2),
                lifecycle: Some("active".to_string()),
                superseded_by: None,
                target_stage: Some(3),
                relation: "drives".to_string(),
                source: "typed".to_string(),
                diagnostic: None,
            }],
            Vec::new(),
        );

        assert!(
            info.suggestions[0]
                .suggestion
                .contains("does not select a unique workspace document")
        );
        assert!(
            info.suggestions[0]
                .suggestion
                .contains("01rfc000000000000000000001")
        );
        assert!(!info.suggestions[0].suggestion.contains("exo rfc promote"));
    }
}
