#![allow(clippy::redundant_pub_crate)]

use crate::ExoResult;
use crate::api::protocol::ErrorCode;
use crate::context::{ExoState, SqliteLoader};
use crate::failure::ExoFailure;
use crate::steering::{SuggestedAction, WorkIntent};
use serde::Serialize;
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
        .output()?)
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
    /// Info about the next pending phase in the epoch, if any.
    pub next_phase: Option<NextPhaseInfo>,
}

/// A suggestion about an RFC attached to the completed phase.
#[derive(Debug, Serialize)]
pub(crate) struct RfcSuggestion {
    pub rfc_id: String,
    pub title: String,
    pub current_stage: u8,
    pub target_stage: Option<u8>,
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
    let rfc_suggestions = collect_phase_rfc_info(db_path, plan, &active_phase_id);

    // 4. Update status to completed
    {
        let writer = crate::context::SqliteWriter::open(db_path)?;
        writer.update_phase_status(&active_phase_id, "completed")?;
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
        for suggestion in &rfc_suggestions {
            println!(
                "\nRFC {}: {} (Stage {})",
                suggestion.rfc_id, suggestion.title, suggestion.current_stage
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
        rfc_suggestions,
        next_phase,
    })
}

/// Collect RFC information for a phase's attached RFCs.
///
/// Reads each attached RFC from disk to get its current stage and title,
/// then generates a promotion suggestion based on whether the RFC is
/// driving (has a target stage) or related.
fn collect_phase_rfc_info(db_path: &Path, plan: &ExoState, phase_id: &str) -> Vec<RfcSuggestion> {
    // Find the phase's RFC attachments
    let phase_rfcs: Vec<_> = plan
        .epochs
        .iter()
        .flat_map(|e| &e.phases)
        .find(|p| p.id == phase_id)
        .map(|p| p.rfcs.clone())
        .unwrap_or_default();

    if phase_rfcs.is_empty() {
        return Vec::new();
    }

    let loader = match SqliteLoader::open(db_path) {
        Ok(loader) => loader,
        Err(_) => return Vec::new(),
    };
    let rfc_index = match loader.load_rfcs() {
        Ok(rfcs) => rfcs
            .into_iter()
            .map(|rfc| (format!("{:05}", rfc.rfc_number), (rfc.title, rfc.stage)))
            .collect::<std::collections::HashMap<_, _>>(),
        Err(_) => return Vec::new(),
    };

    let mut suggestions = Vec::new();

    for phase_rfc in &phase_rfcs {
        let (title, current_stage) = match rfc_index.get(&phase_rfc.id) {
            Some((title, stage)) => (title.clone(), *stage),
            None => (format!("RFC {}", phase_rfc.id), 0),
        };

        let suggestion = if let Some(target) = phase_rfc.target {
            if current_stage >= target {
                format!(
                    "Already at Stage {current_stage} (target was {target}). No promotion needed."
                )
            } else {
                format!(
                    "Currently Stage {}, target Stage {}. Consider: `exo rfc promote {} --stage {}`",
                    current_stage, target, phase_rfc.id, target
                )
            }
        } else if current_stage < 4 {
            format!("Related RFC at Stage {current_stage}. Review if this work advances it.")
        } else {
            format!("Stable (Stage {current_stage}). No action needed.")
        };

        suggestions.push(RfcSuggestion {
            rfc_id: phase_rfc.id.clone(),
            title,
            current_stage,
            target_stage: phase_rfc.target,
            suggestion,
            is_driving: phase_rfc.is_driving(),
        });
    }

    suggestions
}
