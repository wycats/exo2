#![allow(clippy::redundant_pub_crate)]

use crate::ExoResult;
use crate::context::{ActivePhaseInfo, ExoState};
use serde::Serialize;

// ─────────────────────────────────────────────────────────────────────────────
// Plan Health Metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Health status classification for plan metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HealthStatus {
    /// Plan is healthy (within thresholds)
    Healthy,
    /// Plan shows signs of degradation (warning thresholds exceeded)
    Degraded,
    /// Plan is critically unhealthy (critical thresholds exceeded)
    Critical,
}
/// Thresholds for health status computation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HealthThresholds {
    /// Pending goals above this triggers Degraded status
    pub pending_tasks_degraded: usize,
    /// Pending goals above this triggers Critical status
    pub pending_tasks_critical: usize,
    /// Stale phases above this triggers warnings
    #[allow(dead_code)]
    pub stale_phases_warning: usize,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            pending_tasks_degraded: 10,
            pending_tasks_critical: 15,
            stale_phases_warning: 3,
        }
    }
}
#[derive(Serialize)]
struct ActivePhaseJson {
    epoch_id: String,
    epoch_title: String,
    phase_id: String,
    phase_title: String,
}

#[derive(Serialize)]
struct NamedPhaseJson {
    epoch_id: String,
    epoch_title: String,
    phase_id: String,
    phase_title: String,
}

#[derive(Serialize)]
struct ProgressHeuristicJson {
    mode: String,
    reason: String,
    pending_phases_in_active_epoch: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct PlanReviewJson {
    kind: String,
    active_phase: Option<ActivePhaseJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    in_progress_phases: Vec<NamedPhaseJson>,
    non_linearity: Vec<NamedPhaseJson>,
    stale_pending_phases: Vec<NamedPhaseJson>,
    progress_heuristic: ProgressHeuristicJson,
    fix_mode: bool,
}

pub(crate) fn compute_plan_review(
    plan: &ExoState,
    active_info: Option<ActivePhaseInfo<'_>>,
    fix: bool,
) -> PlanReviewJson {
    let active_phase_json = active_info.as_ref().map(|info| ActivePhaseJson {
        epoch_id: info.epoch.id.clone(),
        epoch_title: info.epoch.title.clone(),
        phase_id: info.phase.id.clone(),
        phase_title: info.phase.title.clone(),
    });

    let mut in_progress_phases = vec![];
    for epoch in &plan.epochs {
        for phase in &epoch.phases {
            if phase.status == "in-progress" {
                in_progress_phases.push(NamedPhaseJson {
                    epoch_id: epoch.id.clone(),
                    epoch_title: epoch.title.clone(),
                    phase_id: phase.id.clone(),
                    phase_title: phase.title.clone(),
                });
            }
        }
    }

    let diagnostic_anchor = plan
        .epochs
        .iter()
        .enumerate()
        .find_map(|(epoch_idx, epoch)| {
            epoch
                .phases
                .iter()
                .enumerate()
                .find(|(_, phase)| phase.status == "in-progress")
                .map(|(phase_idx, _)| (epoch_idx, phase_idx))
        });

    // 2. Check for Non-Linearity (completed phases after the whole-plan
    // diagnostic anchor). Workspace pins orient the current workspace; plan
    // diagnostics still describe plan shape across all in-progress phases.
    let mut non_linear_json: Vec<NamedPhaseJson> = vec![];
    if let Some((e_idx, p_idx)) = diagnostic_anchor {
        for (curr_e_idx, epoch) in plan.epochs.iter().enumerate() {
            if curr_e_idx < e_idx {
                continue;
            } // Past epochs are fine

            for (curr_p_idx, phase) in epoch.phases.iter().enumerate() {
                if curr_e_idx == e_idx && curr_p_idx <= p_idx {
                    continue;
                } // Past phases in current epoch are fine

                if phase.status == "completed" {
                    non_linear_json.push(NamedPhaseJson {
                        epoch_id: epoch.id.clone(),
                        epoch_title: epoch.title.clone(),
                        phase_id: phase.id.clone(),
                        phase_title: phase.title.clone(),
                    });
                }
            }
        }
    }

    // 3. Check for Stale Pending Phases (in past epochs)
    let mut stale_phases_json: Vec<NamedPhaseJson> = vec![];
    if let Some((anchor_epoch_idx, _)) = diagnostic_anchor {
        for (curr_e_idx, epoch) in plan.epochs.iter().enumerate() {
            if curr_e_idx >= anchor_epoch_idx {
                break;
            } // Only check past epochs

            for phase in &epoch.phases {
                if phase.status == "pending" {
                    stale_phases_json.push(NamedPhaseJson {
                        epoch_id: epoch.id.clone(),
                        epoch_title: epoch.title.clone(),
                        phase_id: phase.id.clone(),
                        phase_title: phase.title.clone(),
                    });
                }
            }
        }
    }

    // 4. Progress Heuristic (Explore vs Exploit)
    let pending_current = active_info.as_ref().and_then(|info| {
        plan.epochs.get(info.epoch_idx).map(|epoch| {
            epoch
                .phases
                .iter()
                .filter(|p| p.status == "pending")
                .count()
        })
    });

    let (mode, reason) = match pending_current {
        Some(pending) if pending > 3 => (
            "LEVERAGE".to_string(),
            ">3 pending phases in current epoch. Focus on execution.".to_string(),
        ),
        Some(_) => (
            "DISCOVERY".to_string(),
            "Few pending phases. Time to plan the next steps.".to_string(),
        ),
        None => (
            "ORIENT".to_string(),
            "No current phase is selected for this workspace. Review plan diagnostics before choosing the next phase.".to_string(),
        ),
    };

    let progress = ProgressHeuristicJson {
        mode,
        reason,
        pending_phases_in_active_epoch: pending_current,
    };

    PlanReviewJson {
        kind: "plan.review".to_string(),
        active_phase: active_phase_json,
        in_progress_phases,
        non_linearity: non_linear_json,
        stale_pending_phases: stale_phases_json,
        progress_heuristic: progress,
        fix_mode: fix,
    }
}
pub(crate) fn build_plan_review_json_from_context(
    context: &crate::context::AgentContext,
    fix: bool,
) -> ExoResult<serde_json::Value> {
    let active_info = context.find_workspace_active_phase()?;
    let review = compute_plan_review(&context.plan, active_info, fix);
    Ok(serde_json::to_value(&review)?)
}
pub(crate) fn show_plan_review_human_from_context(
    context: &crate::context::AgentContext,
    fix: bool,
) -> ExoResult<()> {
    let active_info = context.find_workspace_active_phase()?;
    let review = compute_plan_review(&context.plan, active_info, fix);

    println!("\n# Strategic Plan Review\n");
    if let Some(active) = &review.active_phase {
        println!(
            "**Active Phase**: {} (Epoch: {})",
            active.phase_title, active.epoch_title
        );
    } else {
        println!("**Active Phase**: None");
    }

    let visible_in_progress_phases: Vec<_> = review
        .in_progress_phases
        .iter()
        .filter(|phase| {
            review
                .active_phase
                .as_ref()
                .is_none_or(|active| active.phase_id != phase.phase_id)
        })
        .collect();

    if !visible_in_progress_phases.is_empty() {
        println!("\n## In-Progress Phases");
        if review.active_phase.is_some() {
            println!("Plan state also contains other in-progress phases:");
        } else {
            println!(
                "Plan state contains in-progress phases, but none is current for this workspace:"
            );
        }
        for item in visible_in_progress_phases {
            println!("- [{}] {}", item.epoch_title, item.phase_title);
        }
    }

    if !review.non_linearity.is_empty() {
        println!("\n## ⚠️  Non-Linearity Detected");
        println!("The following phases are marked 'completed' but appear AFTER the active phase:");
        for item in &review.non_linearity {
            println!("- [{}] {}", item.epoch_title, item.phase_title);
        }
        if fix {
            println!("> Fix: Consider 'Bankrupting' these epochs if they were skipped.");
        }
    }

    if !review.stale_pending_phases.is_empty() {
        println!("\n## 🧟 Stale Phases Detected");
        println!("The following phases are 'pending' in past epochs:");
        for item in &review.stale_pending_phases {
            println!("- [{}] {}", item.epoch_title, item.phase_title);
        }
    }

    println!("\n## 🧭 Progress Heuristic");
    println!("**Mode**: {}", review.progress_heuristic.mode);
    println!("Reason: {}", review.progress_heuristic.reason);

    Ok(())
}
// (ULID Migration block removed — migrate command deleted)
