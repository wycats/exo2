//! Phase State Machine
//!
//! Implements the phase state machine as defined in RFC 0064.
//! The state machine provides deterministic "where are we at?" answers
//! derived from canonical artifacts.
//!
//! # Primary States
//!
//! 1. **`NoActivePhase`** - No phase is currently active
//! 2. **`ActivePhaseNeedsUpgrade`** - Deprecated projections detected
//! 3. **`ActivePhaseUnprepared`** - Active phase lacks execution plan
//! 4. **`ActivePhaseExecuting`** - Active phase with work in progress
//! 5. **`PreparingNextPhase`** - Preparing the next pending phase
//! 6. **`PreparingNextEpoch`** - No pending phases, need next epoch
//!
//! Additionally, **`StrikeActive`** is an overlay state for surgical strikes.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::steering::{SuggestedAction, WorkIntent};
use serde::Serialize;
/// The primary states of the phase state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimaryState {
    /// No phase is currently active.
    NoActivePhase,
    /// Active phase exists but deprecated projections detected.
    ActivePhaseNeedsUpgrade,
    /// Active phase exists but lacks meaningful execution plan.
    ActivePhaseUnprepared,
    /// Active phase with work in progress.
    ActivePhaseExecuting,
    /// No active phase, preparing the next pending phase.
    PreparingNextPhase,
    /// No active phase, no pending phases - need next epoch.
    PreparingNextEpoch,
    /// Surgical strike in progress (overlay state).
    StrikeActive(String),
}

impl PrimaryState {
    /// Get a human-readable name for this state.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::NoActivePhase => "No Active Phase",
            Self::ActivePhaseNeedsUpgrade => "Active Phase (Needs Upgrade)",
            Self::ActivePhaseUnprepared => "Active Phase (Unprepared)",
            Self::ActivePhaseExecuting => "Active Phase (Executing)",
            Self::PreparingNextPhase => "Preparing Next Phase",
            Self::PreparingNextEpoch => "Preparing Next Epoch",
            Self::StrikeActive(_) => "Strike Active",
        }
    }

    /// Get the primary intent for this state.
    #[must_use]
    pub const fn primary_intent(&self) -> WorkIntent {
        match self {
            Self::NoActivePhase => WorkIntent::Orient,
            Self::ActivePhaseNeedsUpgrade => WorkIntent::Orient,
            Self::ActivePhaseUnprepared => WorkIntent::Plan,
            Self::ActivePhaseExecuting => WorkIntent::Execute,
            Self::PreparingNextPhase => WorkIntent::Plan,
            Self::PreparingNextEpoch => WorkIntent::Plan,
            Self::StrikeActive(_) => WorkIntent::Execute,
        }
    }

    /// Check if this is an active phase state (any variant).
    #[must_use]
    pub const fn is_active_phase(&self) -> bool {
        matches!(
            self,
            Self::ActivePhaseNeedsUpgrade
                | Self::ActivePhaseUnprepared
                | Self::ActivePhaseExecuting
        )
    }
}

/// Operations that can be gated by state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Start a new phase
    PhaseStart,
    /// Finish the current phase
    PhaseFinish,
    /// Add a task
    TaskAdd,
    /// Start a task (mark in-progress)
    TaskStart,
    /// Complete a task
    TaskComplete,
    /// Remove a task
    TaskRemove,
    /// Reorder a task
    TaskReorder,
    /// Update a task
    TaskUpdate,
    /// Modify the implementation plan
    ImplPlanMutate,
    /// View status (always allowed)
    StatusView,
    /// Run upgrade migration
    UpgradeMigrate,
}

/// Detect if there's an active surgical strike.
///
/// Returns the strike ID if a strike is active.
pub fn detect_active_strike(context: &AgentContext) -> ExoResult<Option<String>> {
    let Some(active_phase) = context.find_workspace_active_phase()? else {
        return Ok(None);
    };

    for goal in &active_phase.phase.goals {
        if goal.kind.as_deref() == Some("strike") && goal.status == "in-progress" {
            return Ok(Some(goal.id.clone()));
        }
    }

    Ok(None)
}

/// Resolve the current primary state from canonical artifacts.
///
/// This is the main entry point for the state machine. It examines the project
/// artifacts and returns the current state in a deterministic manner.
pub fn resolve_primary_state(context: &AgentContext) -> ExoResult<PrimaryState> {
    // 1. Check for active strike (overlay state)
    if let Some(strike_id) = detect_active_strike(context)? {
        return Ok(PrimaryState::StrikeActive(strike_id));
    }

    // 2. Check for active phase
    let active_phase_id = context.find_workspace_active_phase_id()?;

    // 3. If active phase exists, determine sub-state
    if active_phase_id.is_some() {
        // 3a. Check for upgrade gate (critical upgrades block)
        let registry = UpgradeRegistry::new();
        if let Ok(check) = registry.check_all(context)
            && check.has_blocking()
        {
            return Ok(PrimaryState::ActivePhaseNeedsUpgrade);
        }

        // 3b. Check if phase has executable work defined in SQLite state
        let phase_info = context
            .plan
            .find_phase_by_id(active_phase_id.as_deref().unwrap_or_default());
        let has_goals = phase_info.is_some_and(|info| !info.phase.goals.is_empty());
        if !has_goals {
            return Ok(PrimaryState::ActivePhaseUnprepared);
        }

        // 3c. Phase is prepared and active — executing
        return Ok(PrimaryState::ActivePhaseExecuting);
    }

    // 4. No active phase - check for pending phases
    if context.plan.find_next_pending_phase(None).is_some() {
        return Ok(PrimaryState::PreparingNextPhase);
    }

    // 5. No pending phases - check for active epochs with pending phases
    for epoch in &context.plan.epochs {
        let status = epoch.derived_status();
        if (status == "in-progress" || status == "pending")
            && epoch.phases.iter().any(|p| p.status == "pending")
        {
            return Ok(PrimaryState::PreparingNextPhase);
        }
    }

    // 6. No pending phases anywhere - need next epoch
    Ok(PrimaryState::PreparingNextEpoch)
}

/// Check if an operation is allowed in the current state.
#[must_use]
pub const fn is_operation_allowed(state: &PrimaryState, operation: Operation) -> bool {
    match operation {
        // Status viewing is always allowed
        Operation::StatusView => true,

        // Upgrade migration is only allowed in NeedsUpgrade state
        Operation::UpgradeMigrate => matches!(state, PrimaryState::ActivePhaseNeedsUpgrade),

        // Phase start requires no active phase
        Operation::PhaseStart => matches!(
            state,
            PrimaryState::NoActivePhase
                | PrimaryState::PreparingNextPhase
                | PrimaryState::PreparingNextEpoch
        ),

        // Phase finish requires an active, executing phase
        Operation::PhaseFinish => {
            matches!(state, PrimaryState::ActivePhaseExecuting)
        }

        // Task operations require active phase (not in upgrade state)
        Operation::TaskAdd
        | Operation::TaskStart
        | Operation::TaskComplete
        | Operation::TaskRemove
        | Operation::TaskReorder
        | Operation::TaskUpdate
        | Operation::ImplPlanMutate => {
            matches!(
                state,
                PrimaryState::ActivePhaseUnprepared
                    | PrimaryState::ActivePhaseExecuting
                    | PrimaryState::StrikeActive(_)
            )
        }
    }
}

/// Generate steering suggestions for the current state.
#[must_use]
pub fn generate_steering(state: &PrimaryState) -> Vec<SuggestedAction> {
    match state {
        PrimaryState::NoActivePhase => vec![SuggestedAction {
            label: "Start next phase".to_string(),
            command: "exo phase start".to_string(),
            rationale: "No active phase. Start the next pending phase.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.9),
        }],

        PrimaryState::ActivePhaseNeedsUpgrade => vec![
            SuggestedAction {
                label: "Migrate deprecated projections".to_string(),
                command: "exo update".to_string(),
                rationale: "Deprecated projections detected. Run exo update to migrate them."
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.95),
            },
            SuggestedAction {
                label: "Check phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View current phase status and blocking issues.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ],

        PrimaryState::ActivePhaseUnprepared => vec![
            SuggestedAction {
                label: "Add a task".to_string(),
                command: "exo task add <title> --id <id>".to_string(),
                rationale: "Phase needs work items. Add tasks to track progress.".to_string(),
                intent: WorkIntent::Plan,
                confidence: Some(0.9),
            },
            SuggestedAction {
                label: "Check phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View current phase details.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ],

        PrimaryState::ActivePhaseExecuting => vec![
            SuggestedAction {
                label: "Complete a task".to_string(),
                command: "exo task complete <id>".to_string(),
                rationale: "Work in progress. Complete tasks as you finish them.".to_string(),
                intent: WorkIntent::Record,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Check phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View current phase status and progress.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Finish phase".to_string(),
                command: "exo phase finish".to_string(),
                rationale: "All goals complete. Finish this phase.".to_string(),
                intent: WorkIntent::Ship,
                confidence: Some(0.6),
            },
        ],

        PrimaryState::PreparingNextPhase => vec![
            SuggestedAction {
                label: "Start next phase".to_string(),
                command: "exo phase start".to_string(),
                rationale: "Next phase is pending. Start it when ready.".to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(0.85),
            },
            SuggestedAction {
                label: "Review plan".to_string(),
                command: "exo plan review".to_string(),
                rationale: "Review the project plan before starting.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ],

        PrimaryState::PreparingNextEpoch => vec![
            SuggestedAction {
                label: "Add new epoch".to_string(),
                command: "exo epoch add --title <title>".to_string(),
                rationale: "No pending phases. Add a new epoch to continue.".to_string(),
                intent: WorkIntent::Plan,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Review plan".to_string(),
                command: "exo plan review".to_string(),
                rationale: "Review the project plan and completed work.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ],

        PrimaryState::StrikeActive(strike_id) => vec![
            SuggestedAction {
                label: "Finish strike".to_string(),
                command: "exo strike finish".to_string(),
                rationale: format!("Strike '{strike_id}' is active. Finish when done."),
                intent: WorkIntent::Ship,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Check strike status".to_string(),
                command: "exo strike status".to_string(),
                rationale: "View current strike details.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Upgrade Gate Enforcement
// ─────────────────────────────────────────────────────────────────────────────

use crate::api::protocol::ErrorCode;
use crate::failure::ExoFailure;
use crate::upgrade::UpgradeRegistry;

/// Check if the project requires an upgrade before mutations are allowed.
///
/// This function uses the `UpgradeRegistry` to check all upgrade plugins
/// and blocks only on `Critical` severity upgrades. Warning-level upgrades
/// are logged but do not block operations.
///
/// **Strike Bypass**: During an active surgical strike, the upgrade gate
/// is bypassed to allow urgent fixes to proceed.
///
/// Returns `Ok(())` if mutations are allowed, or an error with steering
/// if critical upgrades are needed.
///
/// This should be called at the start of any mutation command.
pub fn check_upgrade_gate(context: &AgentContext) -> ExoResult<()> {
    // Strike bypass: urgent fixes should not be blocked by upgrades
    if detect_active_strike(context)?.is_some() {
        return Ok(());
    }

    let registry = UpgradeRegistry::new();
    let check = registry.check_all(context)?;

    // Block on critical upgrades
    if !check.critical.is_empty() {
        let reasons = check
            .critical
            .iter()
            .map(|u| format!("  • {}", u.reason))
            .collect::<Vec<_>>()
            .join("\n");

        return Err(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Project requires critical upgrades before mutations are allowed:\n\
                 \n\
                 {reasons}\n\
                 \n\
                 Run `exo update` to apply all pending upgrades."
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Run upgrade migrations".to_string(),
                command: "exo update".to_string(),
                rationale: format!(
                    "{} critical upgrade(s) must be applied before proceeding.",
                    check.critical.len()
                ),
                intent: WorkIntent::Orient,
                confidence: Some(1.0), // Blocking - no alternative
            }]),
        )
        .into());
    }

    // Warnings are logged but do not block.
    // The caller can check for warnings separately via UpgradeRegistry::check_all()
    // if they want to display them (e.g., in `exo status` or `exo update --check`).

    Ok(())
}

/// Check if an operation is allowed given the current state.
///
/// This is a higher-level check that combines state resolution and operation gating.
/// Use this when you need both the state check and a specific operation check.
pub fn check_operation_allowed(context: &AgentContext, operation: Operation) -> ExoResult<()> {
    let state = resolve_primary_state(context)?;

    if !is_operation_allowed(&state, operation) {
        let action_name = match operation {
            Operation::PhaseStart => "start a phase",
            Operation::PhaseFinish => "finish the phase",
            Operation::TaskAdd => "add a task",
            Operation::TaskStart => "start a task",
            Operation::TaskComplete => "complete a task",
            Operation::TaskRemove => "remove a task",
            Operation::TaskReorder => "reorder a task",
            Operation::TaskUpdate => "update a task",
            Operation::ImplPlanMutate => "modify the implementation plan",
            Operation::StatusView => "view status",
            Operation::UpgradeMigrate => "run upgrade migration",
        };

        return Err(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Cannot {action_name} in current state: {}",
                state.display_name()
            ),
            ExoFailure::orienting_steering(generate_steering(&state)),
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primary_state_display_names() {
        assert_eq!(
            PrimaryState::NoActivePhase.display_name(),
            "No Active Phase"
        );
        assert_eq!(
            PrimaryState::ActivePhaseExecuting.display_name(),
            "Active Phase (Executing)"
        );
        assert_eq!(
            PrimaryState::StrikeActive("test".to_string()).display_name(),
            "Strike Active"
        );
    }

    #[test]
    fn test_primary_state_intents() {
        assert_eq!(
            PrimaryState::ActivePhaseExecuting.primary_intent(),
            WorkIntent::Execute
        );
        assert_eq!(
            PrimaryState::PreparingNextPhase.primary_intent(),
            WorkIntent::Plan
        );
    }

    #[test]
    fn test_is_active_phase() {
        assert!(!PrimaryState::NoActivePhase.is_active_phase());
        assert!(PrimaryState::ActivePhaseExecuting.is_active_phase());
        assert!(PrimaryState::ActivePhaseNeedsUpgrade.is_active_phase());
        assert!(!PrimaryState::PreparingNextPhase.is_active_phase());
        assert!(!PrimaryState::StrikeActive("test".to_string()).is_active_phase());
    }

    #[test]
    fn test_operation_allowed_status_always() {
        // StatusView should always be allowed
        assert!(is_operation_allowed(
            &PrimaryState::NoActivePhase,
            Operation::StatusView
        ));
        assert!(is_operation_allowed(
            &PrimaryState::ActivePhaseNeedsUpgrade,
            Operation::StatusView
        ));
        assert!(is_operation_allowed(
            &PrimaryState::ActivePhaseExecuting,
            Operation::StatusView
        ));
    }

    #[test]
    fn test_operation_allowed_upgrade_gate() {
        // UpgradeMigrate only in NeedsUpgrade
        assert!(is_operation_allowed(
            &PrimaryState::ActivePhaseNeedsUpgrade,
            Operation::UpgradeMigrate
        ));
        assert!(!is_operation_allowed(
            &PrimaryState::ActivePhaseExecuting,
            Operation::UpgradeMigrate
        ));
    }

    #[test]
    fn test_operation_allowed_phase_start() {
        assert!(is_operation_allowed(
            &PrimaryState::NoActivePhase,
            Operation::PhaseStart
        ));
        assert!(is_operation_allowed(
            &PrimaryState::PreparingNextPhase,
            Operation::PhaseStart
        ));
        assert!(!is_operation_allowed(
            &PrimaryState::ActivePhaseExecuting,
            Operation::PhaseStart
        ));
    }

    #[test]
    fn test_operation_blocked_in_needs_upgrade() {
        // Task operations blocked in NeedsUpgrade
        assert!(!is_operation_allowed(
            &PrimaryState::ActivePhaseNeedsUpgrade,
            Operation::TaskAdd
        ));
        assert!(!is_operation_allowed(
            &PrimaryState::ActivePhaseNeedsUpgrade,
            Operation::TaskComplete
        ));
        assert!(!is_operation_allowed(
            &PrimaryState::ActivePhaseNeedsUpgrade,
            Operation::ImplPlanMutate
        ));
    }

    #[test]
    fn test_steering_generation() {
        let steering = generate_steering(&PrimaryState::ActivePhaseExecuting);
        assert!(!steering.is_empty());
        assert!(steering.iter().any(|s| s.command.contains("phase finish")));

        let steering = generate_steering(&PrimaryState::ActivePhaseUnprepared);
        assert!(
            steering
                .iter()
                .any(|s| s.command == "exo task add <title> --id <id>")
        );
        assert!(!steering.iter().any(|s| s.command.contains("--label")));
    }
}
