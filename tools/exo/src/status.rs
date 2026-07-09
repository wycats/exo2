//! Bootstrap/orientation command for Exosuit projects.
//!
//! `exo status` is the canonical "where am I?" command - the first thing to run
//! when entering a session. It provides:
//! - Current phase and epoch information
//! - Repository health (git status, change summary)
//! - Progress mode (Discovery/Execution/Verification/Review)
//! - Steering suggestions (what to do next)
//!
//! Compare with `exo map` which is more detailed and navigation-focused.

use crate::ExoResult;
use crate::command::sidecar::SidecarRepoSyncStatus;
use crate::context::AgentContext;
use crate::phase_owner::{self, CurrentOwnerView, PhaseOwnerView};
use crate::steering::{self, SteeringBlock};
use crate::upgrade::UpgradeRegistry;
use crate::world_state::WorldState;
use serde::Serialize;

/// JSON output for `exo status`
#[derive(Debug, Serialize)]
pub struct StatusJson {
    /// Current phase ID, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    /// Current phase title, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_title: Option<String>,
    /// Current epoch title, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch_title: Option<String>,
    /// Ownership signal for the focused phase, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_owner: Option<PhaseOwnerView>,
    /// How this workspace would claim phase ownership.
    pub current_owner: CurrentOwnerView,
    /// Whether the git working tree is dirty
    pub git_dirty: bool,
    /// Summary of git changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_summary: Option<GitSummary>,
    /// Sidecar git repository sync health, when this project uses sidecar state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar_sync: Option<SidecarRepoSyncStatus>,
    /// Current progress mode
    pub progress_mode: steering::ProgressMode,
    /// Between-phases context data (RFC 00187: context-aware `BetweenPhases`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub between_phases_context: Option<BetweenPhasesContext>,
    /// Full steering block
    pub steering: SteeringBlock,
    /// Count of pending goals in current phase
    #[serde(alias = "pending_tasks")]
    pub pending_goals: usize,
    /// Count of completed goals in current phase
    #[serde(alias = "completed_tasks")]
    pub completed_goals: usize,
    /// Session boundary detection
    pub session_boundary: crate::session_boundary::BoundaryDetection,
}

/// Git change summary for status output
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GitSummary {
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub untracked: usize,
}

/// Next phase preview for between-phases context.
#[derive(Debug, Clone, Serialize)]
pub struct NextPhasePreview {
    pub id: String,
    pub title: String,
    pub goal_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rfcs: Vec<String>,
}

/// Completed phase context for between-phases mode.
/// Per RFC 00187: shows the most recently completed phase.
#[derive(Debug, Clone, Serialize)]
pub struct CompletedPhaseContext {
    pub phase_id: String,
    pub phase_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_log: Option<String>,
    pub goal_count: usize,
    pub completed_goals: usize,
}

/// Context data for between-phases mode.
///
/// Per RFC 00187: `BetweenPhases` is context-aware, showing both
/// the completed phase (if any) and the next phase preview.
#[derive(Debug, Clone, Serialize)]
pub struct BetweenPhasesContext {
    /// The most recently completed phase in the active epoch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_phase: Option<CompletedPhaseContext>,

    /// The next pending phase to start
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_phase: Option<NextPhasePreview>,

    /// Current epoch info
    pub epoch_id: String,
    pub epoch_title: String,

    /// Whether this is the last phase in the epoch
    pub is_epoch_finale: bool,
}

/// Compute context for between-phases mode (RFC 00187).
///
/// Shows the most recently completed phase and the next pending phase.
fn compute_between_phases_context(context: &AgentContext) -> Option<BetweenPhasesContext> {
    let active_epoch = context.find_workspace_active_epoch().ok().flatten()?;

    // Find the most recently completed phase in this epoch
    let completed_phase = active_epoch
        .phases
        .iter()
        .rfind(|p| p.status == "completed")
        .map(|phase| {
            let goal_count = phase.goals.len();
            let completed_goals = phase
                .goals
                .iter()
                .filter(|t| t.status == "completed")
                .count();

            // Aggregate completion logs from goals (phase doesn't have its own completion_log)
            let completion_log = phase
                .goals
                .iter()
                .filter_map(|t| t.completion_log.as_ref())
                .next_back()
                .cloned();

            CompletedPhaseContext {
                phase_id: phase.id.clone(),
                phase_title: phase.title.clone(),
                completion_log,
                goal_count,
                completed_goals,
            }
        });

    // Find the next pending phase
    let next_phase = active_epoch
        .phases
        .iter()
        .find(|p| p.status == "pending")
        .map(|phase| NextPhasePreview {
            id: phase.id.clone(),
            title: phase.title.clone(),
            goal_count: phase.goals.len(),
            rfcs: phase.rfcs.iter().map(|r| r.id.clone()).collect(),
        });

    let is_epoch_finale = next_phase.is_none();

    Some(BetweenPhasesContext {
        completed_phase,
        next_phase,
        epoch_id: active_epoch.id.clone(),
        epoch_title: active_epoch.title.clone(),
        is_epoch_finale,
    })
}

/// Build JSON output for `exo status`
pub fn build_status_json(
    context: &AgentContext,
    rfc_view: &crate::rfc::EffectiveRfcView,
    agent_id: Option<&str>,
) -> ExoResult<serde_json::Value> {
    let owner_context =
        phase_owner::PhaseOwnerViewContext::new(&context.root, context.project.as_ref());
    let current_owner = owner_context.current_owner_view();

    // Check for critical upgrades first
    let registry = UpgradeRegistry::new();
    let upgrade_check = registry.check_all(context)?;

    if upgrade_check.has_blocking() {
        let steering = steering::upgrade_required_steering(&upgrade_check.critical);
        let output = StatusJson {
            phase_id: None,
            phase_title: None,
            epoch_title: None,
            phase_owner: None,
            current_owner,
            git_dirty: false,
            git_summary: None,
            sidecar_sync: None,
            progress_mode: steering::ProgressMode::BetweenPhases,
            between_phases_context: None,
            steering,
            pending_goals: 0,
            completed_goals: 0,
            session_boundary: crate::session_boundary::BoundaryDetection {
                boundary_type: crate::session_boundary::BoundaryType::Session,
                confidence: 0.5,
                rationale: "Upgrade required — boundary detection deferred.".to_string(),
                previous_session: None,
            },
        };
        return Ok(serde_json::to_value(&output)?);
    }

    let world = WorldState::probe_with_rfc_view(context, rfc_view)?;
    let steering = steering::derive_world_steering(&world, agent_id);
    let progress_mode = steering::derive_progress_mode(&world);

    // RFC 00187: Compute between-phases context when in BetweenPhases mode
    let between_phases_context = if progress_mode == steering::ProgressMode::BetweenPhases {
        compute_between_phases_context(context)
    } else {
        None
    };

    // Count goals
    let pending_goals = world
        .goals
        .iter()
        .filter(|goal| goal.status == "pending" || goal.status == "in-progress")
        .count();
    let completed_goals = world
        .goals
        .iter()
        .filter(|goal| goal.status == "completed")
        .count();

    // Build git summary if dirty
    let git_summary = world.git_changes.as_ref().map(|changes| GitSummary {
        modified: changes.modified,
        added: changes.added,
        deleted: changes.deleted,
        untracked: changes.untracked,
    });
    let phase_owner = match world.active_phase.as_ref() {
        Some(phase) => owner_context.owner_view_for_phase(
            &crate::context::db_path(&context.root, context.project.as_ref()),
            &phase.id,
        )?,
        None => None,
    };

    let output = StatusJson {
        phase_id: world.active_phase.as_ref().map(|p| p.id.clone()),
        phase_title: world.active_phase.as_ref().map(|p| p.title.clone()),
        epoch_title: world.active_phase.as_ref().map(|p| p.epoch_title.clone()),
        phase_owner,
        current_owner,
        git_dirty: world.git_dirty,
        git_summary,
        sidecar_sync: world.sidecar_sync.clone(),
        progress_mode,
        between_phases_context,
        steering,
        pending_goals,
        completed_goals,
        session_boundary: world.session_boundary,
    };

    Ok(serde_json::to_value(&output)?)
}

/// Show human-readable status output
pub fn show_status_human(
    context: &AgentContext,
    rfc_view: &crate::rfc::EffectiveRfcView,
    agent_id: Option<&str>,
) -> ExoResult<()> {
    let owner_context =
        phase_owner::PhaseOwnerViewContext::new(&context.root, context.project.as_ref());
    // Check for critical upgrades first
    let registry = UpgradeRegistry::new();
    let upgrade_check = registry.check_all(context)?;

    if upgrade_check.has_blocking() {
        println!("# ⚠️  Exosuit Status: Upgrade Required\n");
        for upgrade in &upgrade_check.critical {
            println!("- {}", upgrade.reason);
        }
        println!();
        println!(
            "Run `exo update` to apply {} critical upgrade(s).",
            upgrade_check.critical.len()
        );
        return Ok(());
    }

    let world = WorldState::probe_with_rfc_view(context, rfc_view)?;
    let steering = steering::derive_world_steering(&world, agent_id);
    let progress_mode = steering::derive_progress_mode(&world);
    let current_owner = owner_context.current_owner_view();
    let owner_basis = phase_owner::current_owner_basis_label(&current_owner);

    // Header with phase info
    if let Some(phase) = &world.active_phase {
        println!("# Exosuit Status\n");
        println!("**Phase**: {}", phase.title);
        println!("**Epoch**: {}", phase.epoch_title);
        if let Some(owner) = owner_context.owner_view_for_phase(
            &crate::context::db_path(&context.root, context.project.as_ref()),
            &phase.id,
        )? {
            let state = if owner.stale {
                "stale owner"
            } else if owner.owned_here {
                "owned here"
            } else {
                "owned elsewhere"
            };
            println!("**Owner**: {} ({state}; basis: {owner_basis})", owner.label);
        } else {
            println!("**Owner**: unowned (basis: {owner_basis})");
        }
        println!("**Mode**: {}", format_progress_mode(progress_mode));
    } else {
        println!("# Exosuit Status\n");
        println!("**Phase**: None (no active phase)");
        println!("**Mode**: {}", format_progress_mode(progress_mode));
    }

    // Goal summary
    let pending = world
        .goals
        .iter()
        .filter(|goal| goal.status == "pending" || goal.status == "in-progress")
        .count();
    let completed = world
        .goals
        .iter()
        .filter(|goal| goal.status == "completed")
        .count();
    let total = world.goals.len();

    if total > 0 {
        println!("**Goals**: {completed}/{total} completed ({pending} pending)");
    }

    println!();

    // Repository health
    println!("## Repository\n");
    if let Some(summary) = &world.git_changes {
        let parts: Vec<String> = [
            (summary.modified, "modified"),
            (summary.added, "added"),
            (summary.deleted, "deleted"),
            (summary.untracked, "untracked"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| format!("{n} {label}"))
        .collect();

        if !parts.is_empty() {
            println!("⚠️  Working tree is dirty: {}", parts.join(", "));
        }

        // Show categorization if available
        if summary.total > 0 {
            let categories: Vec<String> = [
                (summary.source, "source"),
                (summary.context, "context"),
                (summary.agent_context, "agent-context"),
                (summary.generatedish, "generated"),
                (summary.other, "other"),
            ]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, label)| format!("{n} {label}"))
            .collect();

            if !categories.is_empty() {
                println!("   Categories: {}", categories.join(", "));
            }
        }
    } else {
        println!("✅ Working tree is clean");
    }
    if let Some(sidecar_sync) = &world.sidecar_sync {
        if sidecar_sync.ok {
            if sidecar_sync.foreign_checkpoint_debt.is_empty() {
                println!("✅ Sidecar repo is synced");
            } else {
                println!("✅ Current sidecar project is checkpointed");
                let count = sidecar_sync.foreign_checkpoint_debt.len();
                println!("⚠️  {count} other sidecar project(s) have checkpoint debt");
                for debt in sidecar_sync.foreign_checkpoint_debt.iter().take(3) {
                    println!("   {}: {} file(s)", debt.project, debt.files.len());
                }
            }
        } else if let Some(issue) = &sidecar_sync.issue {
            println!("⚠️  Sidecar repo sync issue: {issue}");
        }
    }
    println!();

    // Steering suggestions
    println!("## What's Next\n");
    println!("**Intent**: {}\n", steering.primary_intent.as_str());

    if !steering.next_actions.is_empty() {
        for action in steering.next_actions.iter().take(3) {
            let confidence = action.confidence.unwrap_or(0.5);
            let bar = confidence_bar(confidence);
            println!("→ `{}` {}", action.command, bar);
            println!("  {}\n", action.rationale);
        }
    }

    // Repair actions if any
    if !steering.repair_actions.is_empty() {
        println!("### Repairs Suggested\n");
        for action in steering.repair_actions.iter().take(2) {
            println!("⚠️  `{}`", action.command);
            println!("   {}\n", action.rationale);
        }
    }

    Ok(())
}

const fn format_progress_mode(mode: steering::ProgressMode) -> &'static str {
    match mode {
        steering::ProgressMode::RoadmapRevision => "🗺️  Roadmap Revision (updating plan)",
        steering::ProgressMode::BetweenEpochs => "📊 Between Epochs (choosing next epoch)",
        steering::ProgressMode::BetweenPhases => "🔍 Between Phases (choosing next phase)",
        steering::ProgressMode::Planning => "📋 Planning (defining scope)",
        steering::ProgressMode::Executing => "⚡ Executing (implementing)",
        steering::ProgressMode::Verifying => "🧪 Verifying (tests need attention)",
    }
}

fn confidence_bar(confidence: f32) -> String {
    let filled = (confidence * 5.0).round() as usize;
    let empty = 5 - filled.min(5);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}
