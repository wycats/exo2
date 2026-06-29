use crate::api::protocol::{NextCall, NextCallKind, Steering};
use crate::command_reference::ExoCommandReference;
use crate::context::{Goal, PhaseKind, SqliteLoader};
use crate::shell_ops::ShellOperatorHit;
use crate::world_state::WorldState;
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionOutcomeDigestItem {
    pub id: String,
    pub status: String,
    pub source: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionOutcomeDigestSummary {
    pub entity_type: String,
    pub entity_id: String,
    pub count: usize,
    pub claims: Vec<CompletionOutcomeDigestItem>,
    pub drill_in: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkIntent {
    Orient,
    Plan,
    Execute,
    Record,
    Verify,
    Ship,
}

impl WorkIntent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Orient => "orient",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Record => "record",
            Self::Verify => "verify",
            Self::Ship => "ship",
        }
    }
}

/// Progress mode heuristic for the current workflow state.
///
/// This is a 7-state machine that guides agents through the complete workflow:
///
/// - `RoadmapRevision`: RFCs define new epochs, major pivot, roadmap staleness
/// - `BetweenEpochs`: No active epoch, or current epoch complete
/// - `BetweenPhases`: In an epoch, but no active phase (context-aware per RFC 00187)
/// - Planning: Phase started, defining scope and tasks
/// - Executing: Active execution, implementing planned changes
/// - Verifying: Tests are red, need to validate and fix
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressMode {
    /// Roadmap revision: RFCs define new work, need to update plan
    RoadmapRevision,
    /// Between epochs: No active epoch, decide which to start
    BetweenEpochs,
    /// Between phases: In an epoch, decide which phase to start (context-aware per RFC 00187)
    BetweenPhases,
    /// Planning: Phase started, defining scope and tasks
    Planning,
    /// Executing: Active execution, implementing planned changes
    Executing,
    /// Verifying: Tests are red, need to validate and fix
    Verifying,
}

impl ProgressMode {
    /// Returns the string representation of the mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RoadmapRevision => "roadmap-revision",
            Self::BetweenEpochs => "between-epochs",
            Self::BetweenPhases => "between-phases",
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
        }
    }

    /// Get a confidence multiplier for suggestions in this mode.
    ///
    /// The multipliers guide agents toward appropriate actions:
    /// - `RoadmapRevision`: Orient + Plan focused
    /// - BetweenEpochs/BetweenPhases: Orient + Plan focused
    /// - Planning: Plan + Orient focused
    /// - Executing: Execute + Record focused
    /// - Verifying: Verify + Execute focused
    pub const fn confidence_multiplier_for_intent(self, intent: WorkIntent) -> f32 {
        match (self, intent) {
            // RoadmapRevision: Focus on orienting and planning at roadmap level
            (Self::RoadmapRevision, WorkIntent::Orient) => 1.15,
            (Self::RoadmapRevision, WorkIntent::Plan) => 1.1,
            (Self::RoadmapRevision, _) => 0.8,

            // BetweenEpochs: Focus on orienting (review epoch) and planning (start epoch)
            (Self::BetweenEpochs, WorkIntent::Orient) => 1.15,
            (Self::BetweenEpochs, WorkIntent::Plan) => 1.05,
            (Self::BetweenEpochs, WorkIntent::Record) => 1.0, // epoch review
            (Self::BetweenEpochs, _) => 0.85,

            // BetweenPhases: Focus on orienting and planning
            (Self::BetweenPhases, WorkIntent::Orient) => 1.1,
            (Self::BetweenPhases, WorkIntent::Plan) => 1.1,
            (Self::BetweenPhases, _) => 0.85,

            // Planning: Boost Plan intent, still allow Orient
            (Self::Planning, WorkIntent::Plan) => 1.2,
            (Self::Planning, WorkIntent::Orient) => 1.05,
            (Self::Planning, _) => 0.85,

            // Executing: Boost Execute and Record intents
            (Self::Executing, WorkIntent::Execute) => 1.15,
            (Self::Executing, WorkIntent::Record) => 1.1,
            (Self::Executing, _) => 0.95,

            // Verifying: Boost Verify intent
            (Self::Verifying, WorkIntent::Verify) => 1.2,
            (Self::Verifying, WorkIntent::Execute) => 1.05,
            (Self::Verifying, _) => 0.9,
        }
    }

    /// Returns true if this mode is a "between" state (no active work item).
    pub const fn is_between_state(self) -> bool {
        matches!(
            self,
            Self::RoadmapRevision | Self::BetweenEpochs | Self::BetweenPhases
        )
    }

    /// Returns true if this mode is a phase-active state.
    pub const fn is_phase_active(self) -> bool {
        matches!(self, Self::Planning | Self::Executing | Self::Verifying)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SuggestedAction {
    pub label: String,
    pub command: String,
    pub rationale: String,
    pub intent: WorkIntent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl SuggestedAction {
    pub fn exo(
        label: impl Into<String>,
        reference: ExoCommandReference,
        rationale: impl Into<String>,
        intent: WorkIntent,
        confidence: Option<f32>,
    ) -> Self {
        debug_assert!(
            reference.validate_against_default_spec().is_ok(),
            "Exo suggested action must validate against CommandSpec: {}",
            reference.render_cli()
        );

        Self {
            label: label.into(),
            command: reference.render_cli(),
            rationale: rationale.into(),
            intent,
            confidence,
        }
    }

    pub fn human_action(
        label: impl Into<String>,
        action: impl Into<String>,
        rationale: impl Into<String>,
        intent: WorkIntent,
        confidence: Option<f32>,
    ) -> Self {
        Self {
            label: label.into(),
            command: action.into(),
            rationale: rationale.into(),
            intent,
            confidence,
        }
    }

    pub fn external_shell(
        label: impl Into<String>,
        command: impl Into<String>,
        rationale: impl Into<String>,
        intent: WorkIntent,
        confidence: Option<f32>,
    ) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            rationale: rationale.into(),
            intent,
            confidence,
        }
    }

    pub fn legacy_exo_surface(
        label: impl Into<String>,
        command: impl Into<String>,
        rationale: impl Into<String>,
        intent: WorkIntent,
        confidence: Option<f32>,
    ) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            rationale: rationale.into(),
            intent,
            confidence,
        }
    }

    fn tool_suggestion(&self) -> Option<(String, serde_json::Value)> {
        tool_suggestion_for_command(&self.command)
    }
}

impl Serialize for SuggestedAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("label", &self.label)?;
        map.serialize_entry("command", &self.command)?;
        map.serialize_entry("rationale", &self.rationale)?;
        map.serialize_entry("intent", &self.intent)?;
        if let Some(confidence) = self.confidence {
            map.serialize_entry("confidence", &confidence)?;
        }
        if let Some((tool, tool_args)) = self.tool_suggestion() {
            map.serialize_entry("tool", &tool)?;
            map.serialize_entry("tool_args", &tool_args)?;
        }
        map.end()
    }
}

fn tool_suggestion_for_command(command: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "exo" {
        return None;
    }

    match parts.as_slice() {
        ["exo", "status"] => Some(("exo-status".to_string(), serde_json::json!({}))),
        ["exo", "plan", "show"] => Some(("exo-plan".to_string(), serde_json::json!({}))),
        ["exo", "phase", "start", phase_id] => {
            if is_placeholder(phase_id) {
                None
            } else {
                Some((
                    "exo-phase-start".to_string(),
                    serde_json::json!({ "id": phase_id }),
                ))
            }
        }
        ["exo", "task", "complete", task_id] => {
            if is_placeholder(task_id) {
                None
            } else {
                Some((
                    "exo-task-complete".to_string(),
                    serde_json::json!({ "id": task_id }),
                ))
            }
        }
        ["exo", "phase", "finish", ..] => {
            let message = extract_message_arg(&parts[3..]);
            match message {
                Some(msg) if !is_placeholder(&msg) => Some((
                    "exo-phase-finish".to_string(),
                    serde_json::json!({ "message": msg }),
                )),
                _ => Some(("exo-phase-finish".to_string(), serde_json::json!({}))),
            }
        }
        _ => None,
    }
}

fn extract_message_arg(tokens: &[&str]) -> Option<String> {
    let mut iter = tokens.iter();
    while let Some(token) = iter.next() {
        if *token == "--message" || *token == "-m" {
            if let Some(value) = iter.next() {
                return Some(strip_wrapping_quotes(value));
            }
        } else if let Some(value) = token.strip_prefix("--message=") {
            return Some(strip_wrapping_quotes(value));
        }
    }
    None
}

fn strip_wrapping_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn is_placeholder(value: &str) -> bool {
    matches!(
        value,
        "<id>" | "..." | "\"...\"" | "<message>" | "\"<message>\""
    )
}

/// Summary of an RFC being advanced by the current phase.
/// Always present in steering output when the phase has RFC linkage,
/// so agents know what RFC they're working toward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfcSteeringContext {
    pub id: String,
    pub title: String,
    pub current_stage: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_stage: Option<u8>,
    /// Whether this phase is driving (advancing) this RFC vs just referencing it.
    pub is_driving: bool,
    /// Human-readable requirement for the next stage promotion (if driving).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_requirement: Option<String>,
    /// Derived implementation status: "in-flight" when attached to the active phase,
    /// "in-progress" when attached to an in-progress goal within the phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionSummary {
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub count: usize,
    pub highest_priority: String,
    pub sample_subject: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    pub drill_in: String,
}

/// Entity ancestry context for entity-scoped steering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityContext {
    pub entity_type: String,
    pub entity_id: String,
    pub ancestors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringBlock {
    pub primary_intent: WorkIntent,
    pub progress_mode: ProgressMode,
    /// A short, assertive sentence framing the current context.
    ///
    /// This is the most important field for agent alignment. In executing mode,
    /// it makes clear that a plan exists and the job is to execute it — competing
    /// with the system prompt's ambient "be helpful, check in" pressure.
    pub situation: String,
    pub next_actions: Vec<SuggestedAction>,
    pub repair_actions: Vec<SuggestedAction>,
    /// Grouped perception summaries that should be surfaced to the agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perception_summaries: Vec<PerceptionSummary>,
    /// Active completion claims with subject/body preserved for review surfaces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_digests: Vec<CompletionOutcomeDigestSummary>,
    /// RFCs linked to the current phase. Always populated when RFC linkage exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rfc_context: Vec<RfcSteeringContext>,
    /// Session boundary detection: what kind of session start is this?
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_boundary: Option<crate::session_boundary::BoundaryDetection>,
    /// Entity context for entity-scoped steering (task/goal commands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_context: Option<EntityContext>,
}

/// Create steering that blocks on critical upgrades.
/// This is returned when critical upgrades must be applied before any other operation.
pub fn upgrade_required_steering(
    critical_upgrades: &[crate::upgrade::UpgradeNeeded],
) -> SteeringBlock {
    let reasons = critical_upgrades
        .iter()
        .map(|u| format!("  • {}", u.reason))
        .collect::<Vec<_>>()
        .join("\n");

    SteeringBlock {
        primary_intent: WorkIntent::Orient,
        progress_mode: ProgressMode::BetweenPhases, // Upgrade required is a between-state
        situation: format!(
            "Critical upgrade required. {} migration(s) must run before any work can proceed.",
            critical_upgrades.len()
        ),
        next_actions: vec![SuggestedAction::exo(
            "Run upgrade migrations",
            ExoCommandReference::new(&["update"]),
            format!(
                "Project requires {} critical upgrade(s) before operations can proceed:\n\n{}",
                critical_upgrades.len(),
                reasons
            ),
            WorkIntent::Orient,
            Some(1.0), // Blocking - no alternative
        )],
        repair_actions: vec![], // No alternative path
        perception_summaries: vec![],
        completion_digests: vec![],
        rfc_context: vec![],
        session_boundary: None,
        entity_context: None,
    }
}

/// Steering for when no active phase exists (between phases or between epochs).
///
/// Implements Orient-First behavior from RFC 0107:
/// - First explains the current situation (orient)
/// - Then offers appropriate actions based on context
pub fn no_active_phase_steering(world: &crate::world_state::WorldState) -> SteeringBlock {
    let next_phase = world.next_phase.as_ref();
    let epoch_state = &world.epoch_state;

    // Determine which state we're in based on epoch boundary
    // BetweenEpochs if: no epochs, all epochs complete, or current epoch is complete
    let progress_mode =
        if !epoch_state.has_epochs || epoch_state.all_epochs_complete || epoch_state.epoch_complete
        {
            ProgressMode::BetweenEpochs
        } else {
            ProgressMode::BetweenPhases
        };

    let mut next_actions = Vec::new();
    let mut repair_actions = Vec::new();

    // Orient-first: Build context-aware rationale
    let situation = build_situation_summary(world, progress_mode);

    match progress_mode {
        ProgressMode::BetweenEpochs => {
            // Between epochs - focus on epoch-level decisions
            next_actions.push(SuggestedAction::exo(
                "Review roadmap",
                ExoCommandReference::new(&["plan", "review"]),
                format!(
                    "{situation} Review the roadmap to understand strategic objectives and plan the next epoch."
                ),
                WorkIntent::Orient,
                Some(0.85),
            ));

            if let Some(np) = next_phase {
                next_actions.push(SuggestedAction::exo(
                    "Start next epoch's first phase",
                    ExoCommandReference::new(&["phase", "start"]).positional(np.id.clone()),
                    format!(
                        "Ready to begin Epoch '{}'. First phase: '{}' ({}).",
                        np.epoch_title, np.title, np.id
                    ),
                    WorkIntent::Plan,
                    Some(0.75),
                ));
            }

            // Suggest epoch review if there are unreviewed epochs
            if !world.unreviewed_epochs.is_empty() {
                let epoch = &world.unreviewed_epochs[0];
                next_actions.push(SuggestedAction::exo(
                    format!("Review completed epoch: {}", epoch.title),
                    ExoCommandReference::new(&["epoch", "review"]).positional(epoch.id.clone()),
                    format!(
                        "Epoch '{}' completed but not reviewed. Capture accomplishments before moving on.",
                        epoch.title
                    ),
                    WorkIntent::Record,
                    Some(0.8),
                ));
            }
        }
        ProgressMode::BetweenPhases => {
            // Between phases within an epoch - focus on phase selection
            if let Some(np) = next_phase {
                next_actions.push(SuggestedAction::exo(
                    "Start next phase",
                    ExoCommandReference::new(&["phase", "start"]).positional(np.id.clone()),
                    format!(
                        "{} Next scheduled phase: '{}' in Epoch '{}'.",
                        situation, np.title, np.epoch_title
                    ),
                    WorkIntent::Plan,
                    Some(0.85),
                ));
            }

            next_actions.push(SuggestedAction::exo(
                "Review phase options",
                ExoCommandReference::new(&["plan", "review"]),
                format!("{situation} Review available phases before deciding which to start."),
                WorkIntent::Orient,
                Some(0.7),
            ));

            repair_actions.push(SuggestedAction::exo(
                "Check status",
                ExoCommandReference::new(&["status"]),
                "Get a full status overview before proceeding.",
                WorkIntent::Orient,
                Some(0.5),
            ));
        }
        _ => {
            // Fallback for other modes (shouldn't happen in no_active_phase context)
            if let Some(np) = next_phase {
                next_actions.push(SuggestedAction::exo(
                    "Start a phase",
                    ExoCommandReference::new(&["phase", "start"]).positional(np.id.clone()),
                    format!("No active phase. Next: '{}' ({})", np.title, np.epoch_title),
                    WorkIntent::Plan,
                    Some(0.8),
                ));
            } else {
                next_actions.push(SuggestedAction::exo(
                    "Review plan",
                    ExoCommandReference::new(&["plan", "review"]),
                    "No active phase and no pending phases found. Review the plan.",
                    WorkIntent::Orient,
                    Some(0.7),
                ));
            }
        }
    }

    SteeringBlock {
        primary_intent: WorkIntent::Orient,
        progress_mode,
        situation,
        next_actions,
        repair_actions,
        perception_summaries: vec![],
        completion_digests: vec![],
        rfc_context: vec![],
        session_boundary: None,
        entity_context: None,
    }
}

/// Build a situation summary for orient-first steering.
fn build_situation_summary(
    world: &crate::world_state::WorldState,
    progress_mode: ProgressMode,
) -> String {
    let epoch_state = &world.epoch_state;

    match progress_mode {
        ProgressMode::BetweenEpochs => {
            if !epoch_state.has_epochs {
                "No epochs defined in the roadmap.".to_string()
            } else if epoch_state.all_epochs_complete {
                "All epochs complete. Time to plan new strategic objectives.".to_string()
            } else if let Some(ref active) = epoch_state.active_epoch {
                if active.status == "completed" {
                    format!("Epoch '{}' complete.", active.title)
                } else {
                    format!("Between epochs. Current context: '{}'.", active.title)
                }
            } else {
                "Between epochs.".to_string()
            }
        }
        ProgressMode::BetweenPhases => {
            if let Some(ref active) = epoch_state.active_epoch {
                format!("In Epoch '{}', no active phase.", active.title)
            } else {
                "Between phases.".to_string()
            }
        }
        _ => "Current situation unclear.".to_string(),
    }
}

pub fn derive_phase_steering(
    tasks: &[(String, String, String)],
    goals: &[Goal],
    phase_kind: PhaseKind,
) -> SteeringBlock {
    let mut next_actions = Vec::new();
    let mut repair_actions = Vec::new();

    // Task-level TDD no longer uses goal.status = red/green.
    let any_red = false;
    let any_pending_tasks = tasks.iter().any(|(_, _, s)| s == "pending");
    // Find the next pending goal, returning both the name and the goal itself
    // so we can check for promotion metadata (rfc + target_stage)
    let next_pending_goal_data = goals.iter().find_map(|goal| {
        if goal.is_terminal() {
            return None;
        }

        let name = if goal.label.is_empty() {
            goal.id.as_str()
        } else {
            goal.label.as_str()
        };

        Some((name.to_string(), goal))
    });
    let next_pending_goal = next_pending_goal_data
        .as_ref()
        .map(|(name, _)| name.clone());
    let any_pending_goals = next_pending_goal.is_some();

    let primary_intent = if any_red {
        WorkIntent::Verify
    } else if any_pending_tasks || any_pending_goals {
        WorkIntent::Execute
    } else {
        WorkIntent::Record
    };

    // Note: active TDD steering (red/green) is injected at world-steering level.

    if let Some((next_goal_name, goal)) = &next_pending_goal_data {
        // Check if this is a promotion goal (has rfc + target_stage)
        if let (Some(rfc_id), Some(target)) = (&goal.rfc, goal.target_stage) {
            // Promotion goals: suggest reviewing the RFC for promotion readiness
            let requirement = promotion_requirements(target.saturating_sub(1), target);
            next_actions.push(SuggestedAction::exo(
                format!("Review RFC {rfc_id} for Stage {target} promotion"),
                ExoCommandReference::new(&["rfc", "show"]).positional(rfc_id.clone()),
                format!(
                    "Goal '{next_goal_name}' is advancing RFC {rfc_id} to Stage {target}. {requirement}. Review the RFC for promotion readiness."
                ),
                WorkIntent::Execute,
                Some(0.9),
            ));
        } else if phase_kind == PhaseKind::Chore {
            // Chore phases: suggest starting the goal directly without TDD ceremony
            next_actions.push(SuggestedAction::exo(
                "Start next goal",
                ExoCommandReference::new(&["task", "start"]).positional(next_goal_name.clone()),
                "A pending goal exists. This is a chore phase — start working directly.",
                WorkIntent::Execute,
                Some(0.85),
            ));
        } else {
            // With task-level TDD, we can only start TDD once a concrete task exists.
            // If tasks exist, suggest starting TDD for the first pending task.
            if let Some((task_id, task_label, _)) = tasks.iter().find(|(_, _, s)| s == "pending") {
                next_actions.push(SuggestedAction::legacy_exo_surface(
                    "Start TDD cycle for next task",
                    format!("exo tdd new -n \"{task_id}\" -t <test-file>"),
                    format!(
                        "Goal '{next_goal_name}' is pending. Task '{task_label}' exists — start TDD at the task level.",
                    ),
                    WorkIntent::Execute,
                    Some(0.85),
                ));
            }
        }
    }

    if any_pending_tasks {
        next_actions.push(SuggestedAction::exo(
            "Complete a task",
            ExoCommandReference::new(&["task", "complete"])
                .positional_placeholder("id", "sample-task"),
            "There are pending tasks in the active phase; close one when done.",
            WorkIntent::Record,
            Some(0.6),
        ));
    }

    next_actions.push(SuggestedAction::exo(
        "Add a task",
        ExoCommandReference::new(&["task", "add"])
            .positional_placeholder("title", "sample-title")
            .option_placeholder("id", "id", "sample-task"),
        "Break work into a small, verifiable next step.",
        WorkIntent::Plan,
        Some(0.5),
    ));

    next_actions.push(SuggestedAction::exo(
        "Finish the phase",
        ExoCommandReference::new(&["phase", "finish"]),
        "When tasks and verification are complete, finish the phase and update the plan.",
        WorkIntent::Ship,
        Some(0.4),
    ));

    repair_actions.push(SuggestedAction::exo(
        "Re-orient",
        ExoCommandReference::new(&["phase", "status"]),
        "If unsure what state you're in, re-check phase status.",
        WorkIntent::Orient,
        Some(0.4),
    ));

    // Determine progress mode based on phase state
    // RFC 00187: No Transitioning mode - all work done = still Executing
    let progress_mode = ProgressMode::Executing;

    // Build assertive execution-mode situation framing.
    // This is the primary signal that competes with the system prompt's ambient
    // "be helpful, check in" pressure. It must be unmistakably clear that a plan
    // exists and the job is to execute it.
    let pending_count = goals.iter().filter(|g| !g.is_terminal()).count();
    let completed_count = goals.iter().filter(|g| g.is_terminal()).count();
    let total_tasks = tasks.len();
    let pending_tasks = tasks.iter().filter(|(_, _, s)| s == "pending").count();
    let in_progress_tasks = tasks.iter().filter(|(_, _, s)| s == "in-progress").count();

    let situation = if total_tasks == 0 && pending_count == 0 {
        "Phase active but empty. Add goals and tasks to define the plan.".to_string()
    } else if pending_count == 0 && pending_tasks == 0 && in_progress_tasks == 0 {
        format!("All {completed_count} goals complete. Verify work and finish the phase.")
    } else {
        let mut parts = Vec::new();
        parts.push(format!(
            "Executing phase plan: {completed_count}/{} goals complete",
            completed_count + pending_count
        ));
        if total_tasks > 0 {
            let done_tasks = tasks.iter().filter(|(_, _, s)| s == "completed").count();
            parts.push(format!("{done_tasks}/{total_tasks} tasks done"));
        }
        parts.push("Continue executing — the plan is set.".to_string());
        parts.join(", ")
    };

    SteeringBlock {
        primary_intent,
        progress_mode,
        situation,
        next_actions,
        repair_actions,
        perception_summaries: vec![],
        completion_digests: vec![],
        rfc_context: vec![],
        session_boundary: None,
        entity_context: None,
    }
}

const fn priority_rank(priority: crate::inbox::InboxPriority) -> u8 {
    match priority {
        crate::inbox::InboxPriority::Immediate => 3,
        crate::inbox::InboxPriority::NextTouch => 2,
        crate::inbox::InboxPriority::WhenRelevant => 1,
    }
}

/// Rank intents by communicative weight: claim > concern > inquiry > fyi.
/// A claim is someone asserting something about the entity — most decision-relevant.
const fn intent_rank(intent: crate::inbox::InboxIntent) -> u8 {
    match intent {
        crate::inbox::InboxIntent::Claim => 4,
        crate::inbox::InboxIntent::Concern => 3,
        crate::inbox::InboxIntent::Inquiry => 2,
        crate::inbox::InboxIntent::Fyi => 1,
    }
}

pub(crate) fn summarize_surfaced_intents(
    intents: Vec<crate::inbox::SurfacedIntent>,
) -> Vec<PerceptionSummary> {
    let mut order: Vec<(String, Option<String>)> = Vec::new();
    let mut grouped: HashMap<(String, Option<String>), Vec<crate::inbox::SurfacedIntent>> =
        HashMap::new();

    for intent in intents {
        let key = (intent.entity_type.clone(), intent.entity_id.clone());
        if !grouped.contains_key(&key) {
            order.push(key.clone());
        }
        grouped.entry(key).or_default().push(intent);
    }

    let mut summaries = Vec::with_capacity(order.len());
    for key in order {
        let Some(group) = grouped.remove(&key) else {
            continue;
        };
        let highest_priority = group
            .iter()
            .max_by_key(|i| priority_rank(i.priority))
            .map_or_else(
                || {
                    crate::inbox::InboxPriority::WhenRelevant
                        .as_str()
                        .to_string()
                },
                |i| i.priority.as_str().to_string(),
            );

        // Pick the most communicative item: intent rank > priority rank > first
        let representative = group
            .iter()
            .max_by_key(|i| (intent_rank(i.intent), priority_rank(i.priority)))
            .unwrap_or(&group[0]);

        let sample_subject = if group.len() > 1 {
            format!("{} (+{} more)", representative.subject, group.len() - 1)
        } else {
            representative.subject.clone()
        };
        let subjects = group.iter().map(|intent| intent.subject.clone()).collect();

        let mut drill_in = format!(
            "exo inbox list --entity-type {}",
            representative.entity_type
        );
        if let Some(entity_id) = &representative.entity_id {
            drill_in.push_str(&format!(" --entity-id {entity_id}"));
        }

        summaries.push(PerceptionSummary {
            entity_type: representative.entity_type.clone(),
            entity_id: representative.entity_id.clone(),
            count: group.len(),
            highest_priority,
            sample_subject,
            subjects,
            drill_in,
        });
    }

    summaries
}

/// Derive entity-scoped steering for task/goal commands.
///
/// This is the lightweight steering path — it opens a `SqliteLoader` directly
/// (no `AgentContext`, no git, no RFC pipeline) and collects inbox items
/// scoped to the entity and its ancestors.
///
/// If anything fails, returns a default empty `SteeringBlock`. Entity steering
/// is advisory — it should never block command execution.
pub fn derive_entity_steering(
    root: &Path,
    entity_type: &str,
    entity_id: &str,
    agent_id: Option<&str>,
    activity: Option<&crate::activity::ActivityContext>,
) -> SteeringBlock {
    match derive_entity_steering_inner(root, entity_type, entity_id, agent_id, activity) {
        Ok(block) => block,
        Err(_) => SteeringBlock {
            primary_intent: WorkIntent::Execute,
            progress_mode: ProgressMode::Executing,
            situation: String::new(),
            next_actions: vec![],
            repair_actions: vec![],
            perception_summaries: vec![],
            completion_digests: vec![],
            rfc_context: vec![],
            session_boundary: None,
            entity_context: None,
        },
    }
}

fn derive_entity_steering_inner(
    root: &Path,
    entity_type: &str,
    entity_id: &str,
    agent_id: Option<&str>,
    activity: Option<&crate::activity::ActivityContext>,
) -> anyhow::Result<SteeringBlock> {
    let db_path = root.join(crate::context::SQLITE_DB_PATH);
    let loader = SqliteLoader::open(&db_path)?;

    let ancestors = loader.resolve_entity_tree(entity_type, entity_id)?;

    // Build entity list: self + ancestors
    let mut entity_list = vec![(entity_type.to_string(), entity_id.to_string())];
    entity_list.extend(ancestors.clone());

    // Collect inbox items for all entities in the tree
    let mut all_intents: Vec<crate::inbox::SurfacedIntent> = Vec::new();
    let mut repair_actions: Vec<SuggestedAction> = Vec::new();
    let mut completion_digests = Vec::new();
    for (etype, eid) in &entity_list {
        if matches!(etype.as_str(), "goal" | "task")
            && let Ok(digest) = loader.load_completion_outcome_digest(etype, eid)
            && !digest.claims.is_empty()
        {
            completion_digests.push(completion_outcome_digest_summary_from_loader(digest));
        }

        if let Ok(items) = loader.load_inbox_filtered(None, Some(etype), Some(eid), None, None) {
            for item in &items {
                if !item.is_active() {
                    continue;
                }
                // Agent self-suppression: skip items created by this agent
                if let (Some(item_agent), Some(suppress)) = (item.agent_id.as_deref(), agent_id)
                    && item_agent == suppress
                {
                    continue;
                }

                // Concern on a completed entity → repair action
                if item.intent == crate::inbox::InboxIntent::Concern
                    && let Some(ref eid_val) = item.entity_id
                    && let Ok(Some(status)) = loader.load_entity_status(&item.entity_type, eid_val)
                    && (status == "completed" || status == "green")
                {
                    repair_actions.push(SuggestedAction::exo(
                        format!(
                            "Review concern on completed {} '{}'",
                            item.entity_type, eid_val
                        ),
                        ExoCommandReference::new(&["inbox", "list"])
                            .option("entity-type", item.entity_type.clone())
                            .option("entity-id", eid_val.clone()),
                        format!(
                            "User raised a concern on completed {} '{}' — review before proceeding",
                            item.entity_type, eid_val
                        ),
                        WorkIntent::Orient,
                        Some(0.85),
                    ));
                }

                all_intents.push(crate::inbox::SurfacedIntent::from(item));
            }
        }
    }

    // Limit to 3 summaries
    all_intents.truncate(30); // keep enough raw items for grouping, summaries will be ≤3 groups
    let mut perception_summaries = summarize_surfaced_intents(all_intents);
    perception_summaries.truncate(3);

    // Enrich with activity data when available
    let mut situation = String::new();
    let mut enriched_ancestors = ancestors;

    if let Some(ctx) = activity {
        // Add session info to situation
        if let Some(ref session) = ctx.session {
            situation = format!(
                "Session: {}min, {} events",
                session.duration_minutes, session.event_count
            );
        }

        // If active_entity differs from the explicit entity, add it to ancestors
        // so the steering consumer knows about cross-entity activity
        if let Some(ref active) = ctx.active_entity
            && (active.entity_type != entity_type || active.entity_id != entity_id)
        {
            let pair = (active.entity_type.clone(), active.entity_id.clone());
            if !enriched_ancestors.contains(&pair) {
                enriched_ancestors.push(pair);
            }
        }

        // Drift detection: compare recent file areas against session scope
        let scope = crate::activity::infer_entity_scope(root);
        if let Some(drift) = crate::activity::detect_drift(&ctx.recent_files, &scope) {
            repair_actions.push(SuggestedAction::human_action(
                "Review: file edits outside usual scope",
                "",
                format!(
                    "Recent file edits in {} are outside the established scope for this session",
                    drift.outside_dirs.join(", ")
                ),
                WorkIntent::Orient,
                Some(0.5),
            ));
        }
    }

    Ok(SteeringBlock {
        primary_intent: WorkIntent::Execute,
        progress_mode: ProgressMode::Executing,
        situation,
        next_actions: vec![],
        repair_actions,
        perception_summaries,
        completion_digests,
        rfc_context: vec![],
        session_boundary: None,
        entity_context: Some(EntityContext {
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            ancestors: enriched_ancestors,
        }),
    })
}

/// Derive progress mode from world state.
///
/// This implements the 6-state machine (RFC 00187 collapsed Transitioning into `BetweenPhases`).
/// Currently implemented states:
/// - `BetweenEpochs`: No active epoch, or current epoch complete
/// - `BetweenPhases`: In an epoch, but no active phase (context-aware per RFC 00187)
/// - `Planning`: Phase started but no tasks defined
/// - `Executing`: Active phase with pending tasks/steps (or all work done)
/// - `Verifying`: Tests are red, need to fix
///
/// Future state (not yet implemented):
/// - `RoadmapRevision`: RFCs define new work that needs integration into SQLite-backed state
pub fn derive_progress_mode(world: &WorldState) -> ProgressMode {
    // Check for active TDD red (failing tests) - highest priority
    // Check if we have an active phase
    if world.active_phase.is_none() {
        // Use epoch boundary state to distinguish BetweenEpochs vs BetweenPhases
        let epoch_state = &world.epoch_state;

        // If no epochs exist, or all epochs are complete, we're between epochs
        if !epoch_state.has_epochs || epoch_state.all_epochs_complete {
            return ProgressMode::BetweenEpochs;
        }

        // If we have an active epoch that's not complete, we're between phases
        if epoch_state.active_epoch.is_some() && !epoch_state.epoch_complete {
            return ProgressMode::BetweenPhases;
        }

        // If current epoch is complete, we're between epochs
        if epoch_state.epoch_complete {
            return ProgressMode::BetweenEpochs;
        }

        // Default to BetweenPhases (most common case)
        return ProgressMode::BetweenPhases;
    }

    // Active phase exists - check for planning state (no goals defined)
    if world.tasks.is_empty() && world.goals.is_empty() {
        return ProgressMode::Planning;
    }

    // Check for pending work
    let any_pending_tasks = world.tasks.iter().any(|(_, _, s)| s == "pending");
    let any_pending_goals = world
        .goals
        .iter()
        .any(|goal| goal.status == "pending" || goal.status == "in-progress");
    let any_in_progress_tasks = world.tasks.iter().any(|(_, _, s)| s == "in-progress");

    // If there's pending or in-progress work, we're executing
    if any_pending_tasks || any_pending_goals || any_in_progress_tasks {
        return ProgressMode::Executing;
    }

    // Check if all work is completed
    let all_tasks_completed =
        !world.tasks.is_empty() && world.tasks.iter().all(|(_, _, s)| s == "completed");
    let all_goals_completed = !world.goals.is_empty()
        && world
            .goals
            .iter()
            .all(|goal| goal.status == "completed" || goal.status == "green");

    if (all_tasks_completed || world.tasks.is_empty())
        && (all_goals_completed || world.goals.is_empty())
    {
        // RFC 00187: All work done with active phase = still Executing
        // The "ready to ship" state is shown via context, not a separate mode
        // User runs `exo phase finish` to move to BetweenPhases
        return ProgressMode::Executing;
    }

    // Default to executing if we have an active phase
    ProgressMode::Executing
}

/// Apply progress mode confidence adjustments to a list of actions.
fn adjust_confidence_for_mode(actions: &mut [SuggestedAction], mode: ProgressMode) {
    for action in actions {
        if let Some(conf) = action.confidence {
            let multiplier = mode.confidence_multiplier_for_intent(action.intent);
            action.confidence = Some((conf * multiplier).clamp(0.0, 1.0));
        }
    }
}

pub fn derive_world_steering(world: &WorldState, agent_id: Option<&str>) -> SteeringBlock {
    // Derive progress mode from world state
    let progress_mode = derive_progress_mode(world);

    let mut steering = if world.active_phase.is_none() {
        no_active_phase_steering(world)
    } else {
        let phase_kind = world
            .active_phase
            .as_ref()
            .map(|p| p.kind)
            .unwrap_or_default();
        derive_phase_steering(&world.tasks, &world.goals, phase_kind)
    };

    // Override progress mode with world-derived value (may differ from phase-only derivation)
    steering.progress_mode = progress_mode;

    // Apply confidence adjustments based on progress mode
    adjust_confidence_for_mode(&mut steering.next_actions, progress_mode);
    adjust_confidence_for_mode(&mut steering.repair_actions, progress_mode);

    add_world_repairs(world, &mut steering.repair_actions);
    add_health_repairs(world, progress_mode, &mut steering.repair_actions);
    add_epoch_review_suggestions(world, &mut steering.repair_actions);
    add_goal_centric_nudges(world, progress_mode, &mut steering.repair_actions);
    add_concern_on_completed_repairs(world, agent_id, &mut steering.repair_actions);
    add_goal_completion_log_nudges(world, &mut steering.next_actions);
    add_phase_completion_nudge(world, &mut steering.next_actions);

    // Build top-level RFC context from pipeline data
    let rfc_context = build_rfc_steering_context(world);
    if !rfc_context.is_empty() {
        // Enrich primary next_actions with RFC context so agents always know
        // what RFC they're advancing, not just "a pending goal exists".
        let rfc_summary = format_rfc_summary(&rfc_context);
        for action in &mut steering.next_actions {
            if matches!(action.intent, WorkIntent::Execute | WorkIntent::Verify) {
                action.rationale = format!("{} {}", rfc_summary, action.rationale);
            }
        }
    }
    steering.rfc_context = rfc_context;

    // Populate grouped perception summaries from inbox (before boundary enrichment so we can
    // include intent counts in the situation).
    let active_phase_id = world.active_phase.as_ref().map(|p| p.id.as_str());
    match crate::inbox::get_surfaced_intents(&world.root, active_phase_id, agent_id) {
        Ok(intents) => {
            steering.perception_summaries = summarize_surfaced_intents(intents);
        }
        Err(err) => {
            eprintln!(
                "Warning: failed to load inbox intents for root {}, active phase {:?}: {:?}",
                world.root.display(),
                active_phase_id,
                err
            );
        }
    }

    match load_world_completion_digests(&world.root, active_phase_id) {
        Ok(digests) => {
            steering.completion_digests = digests;
        }
        Err(err) => {
            eprintln!(
                "Warning: failed to load completion digests for root {}, active phase {:?}: {:?}",
                world.root.display(),
                active_phase_id,
                err
            );
        }
    }

    // Populate session boundary detection and enrich situation with boundary context.
    // The situation field was set by derive_phase_steering with execution framing.
    // Here we prepend boundary-specific context so the agent knows *how* it arrived,
    // and whether there are priority items needing user input.
    steering.session_boundary = Some(world.session_boundary.clone());
    let priority_count = steering
        .perception_summaries
        .iter()
        .filter(|summary| {
            summary.highest_priority == crate::inbox::InboxPriority::Immediate.as_str()
        })
        .count();
    enrich_situation_for_boundary(
        &mut steering.situation,
        &world.session_boundary,
        priority_count,
    );

    // Implicit entity scoping: if the event log shows recent activity on a
    // specific entity, merge entity-scoped perception into world steering.
    if let Some(ae) = crate::activity::active_entity(&world.root) {
        let activity = crate::activity::ActivityContext::collect(&world.root);
        let entity_steering = derive_entity_steering(
            &world.root,
            &ae.entity_type,
            &ae.entity_id,
            agent_id,
            Some(&activity),
        );
        if entity_steering.entity_context.is_some() {
            steering.entity_context = entity_steering.entity_context;
        }
        for summary in entity_steering.perception_summaries {
            if !steering
                .perception_summaries
                .iter()
                .any(|s| s.entity_type == summary.entity_type && s.entity_id == summary.entity_id)
            {
                steering.perception_summaries.push(summary);
            }
        }
        for digest in entity_steering.completion_digests {
            if !steering
                .completion_digests
                .iter()
                .any(|d| d.entity_type == digest.entity_type && d.entity_id == digest.entity_id)
            {
                steering.completion_digests.push(digest);
            }
        }
        steering
            .repair_actions
            .extend(entity_steering.repair_actions);
    }

    steering
}

pub(crate) fn completion_outcome_digest_summary_from_loader(
    digest: crate::context::sqlite_loader::PhaseCompletionDigest,
) -> CompletionOutcomeDigestSummary {
    let drill_in = format!(
        "exo inbox list --entity-type {} --entity-id {}",
        digest.entity_type, digest.entity_id
    );
    let claims = digest
        .claims
        .into_iter()
        .map(|claim| CompletionOutcomeDigestItem {
            id: claim.id,
            status: claim.status,
            source: claim.source,
            priority: claim.priority,
            confidence: claim.confidence,
            agent_id: claim.agent_id,
            subject: claim.subject,
            body: claim.body,
            created: claim.created,
        })
        .collect::<Vec<_>>();

    CompletionOutcomeDigestSummary {
        entity_type: digest.entity_type,
        entity_id: digest.entity_id,
        count: claims.len(),
        claims,
        drill_in,
    }
}

fn load_world_completion_digests(
    root: &Path,
    active_phase_id: Option<&str>,
) -> anyhow::Result<Vec<CompletionOutcomeDigestSummary>> {
    let Some(_active_phase_id) = active_phase_id else {
        return Ok(vec![]);
    };

    let agent_ctx = crate::context::AgentContext::load(root.to_path_buf()).ok();
    let workspace_root = agent_ctx
        .as_ref()
        .and_then(crate::context::AgentContext::workspace_root_key);
    let db_path = crate::context::db_path(
        root,
        agent_ctx.as_ref().and_then(|ctx| ctx.project.as_ref()),
    );
    if !db_path.exists() {
        return Ok(vec![]);
    }

    let loader = SqliteLoader::open(&db_path)?;
    let active_entities =
        loader.collect_active_phase_entity_ids_for_workspace(workspace_root.as_deref())?;
    let mut entities = active_entities
        .iter()
        .filter_map(|(entity_type, entity_id)| {
            matches!(entity_type.as_str(), "goal" | "task")
                .then_some((entity_type.as_str(), entity_id.as_str()))
        })
        .collect::<Vec<_>>();
    entities.sort_unstable();

    Ok(loader
        .load_completion_outcome_digests_for_entities(&entities)?
        .into_iter()
        .filter(|digest| !digest.claims.is_empty())
        .map(completion_outcome_digest_summary_from_loader)
        .collect())
}

/// Enrich the situation string with boundary-type-specific context.
///
/// The base situation (from `derive_phase_steering`) says *what* to do.
/// This adds *how you got here* so the agent understands its orientation:
/// - **Compaction**: "Context was compacted mid-session." — strongest "hold the plan" signal
/// - **Session**: "New session." — lighter touch, plan is still primary
/// - **`BrandNew`**: "First session." — onboarding context
///
/// At session/brand-new boundaries (not compaction), priority intents are
/// mentioned so the agent knows to surface them collaboratively with the user.
/// Compaction boundaries suppress this — the agent should hold the plan, not
/// introduce new topics.
fn enrich_situation_for_boundary(
    situation: &mut String,
    boundary: &crate::session_boundary::BoundaryDetection,
    priority_intent_count: usize,
) {
    use crate::session_boundary::BoundaryType;

    let prefix = match boundary.boundary_type {
        BoundaryType::Compaction => {
            "Context was compacted mid-session. Prior work is in progress — pick up where you left off.".to_string()
        }
        BoundaryType::Session => {
            let mut s = String::new();
            if let Some(ref prev) = boundary.previous_session {
                if let Some((ref et, ref ei)) = prev.primary_entity {
                    s.push_str(&format!(
                        "Previous session: {} events over {}min, focused on {}/{}. ",
                        prev.event_count, prev.duration_minutes, et, ei
                    ));
                } else {
                    s.push_str(&format!(
                        "Previous session: {} events over {}min. ",
                        prev.event_count, prev.duration_minutes
                    ));
                }
            }
            s.push_str("New session. A plan exists from prior work.");
            if priority_intent_count > 0 {
                s.push_str(&format!(
                    " {priority_intent_count} priority item(s) in the inbox — check with the user before acting on them."
                ));
            }
            s
        }
        BoundaryType::BrandNew => "First session on this project.".to_string(),
    };

    *situation = format!("{prefix} {situation}");
}

pub const fn world_needs_repair(world: &WorldState) -> bool {
    if world.git_dirty {
        return true;
    }

    if let Some(sidecar_sync) = &world.sidecar_sync
        && (!sidecar_sync.ok || !sidecar_sync.repo_clean)
    {
        return true;
    }

    if let Some(sidecar_sync) = &world.sidecar_sync
        && !sidecar_sync.foreign_checkpoint_debt.is_empty()
    {
        return true;
    }

    false
}

fn add_world_repairs(world: &WorldState, repair_actions: &mut Vec<SuggestedAction>) {
    if let Some(sidecar_sync) = &world.sidecar_sync
        && (!sidecar_sync.ok || !sidecar_sync.repo_clean)
    {
        let (label, reference) = if sidecar_sync.issue_kind == Some("unrelated_history") {
            (
                "Inspect sidecar repo recovery",
                ExoCommandReference::new(&["sidecar", "repo", "status"]),
            )
        } else if !sidecar_sync.has_remote {
            (
                "Add sidecar remote",
                ExoCommandReference::new(&["sidecar", "repo", "remote"]).option_placeholder(
                    "url",
                    "url",
                    "https://github.com/example/sidecar.git",
                ),
            )
        } else if sidecar_sync.clean && sidecar_sync.repo_clean {
            (
                "Resolve sidecar sync",
                ExoCommandReference::new(&["sidecar", "repo", "push"]),
            )
        } else {
            (
                "Resolve sidecar sync",
                ExoCommandReference::new(&["sidecar", "repo", "status"]),
            )
        };
        repair_actions.insert(
            0,
            SuggestedAction::exo(
                label,
                reference,
                sidecar_sync.issue.clone().unwrap_or_else(|| {
                    "Sidecar repository needs attention before shared state is fully synced."
                        .to_string()
                }),
                WorkIntent::Execute,
                Some(0.9),
            ),
        );
    }

    // Prefer deterministic, non-destructive “inspect + repair” commands.
    if world.git_dirty {
        use std::fmt::Write as _;

        let any_red = world.goals.iter().any(|goal| goal.status == "red");
        let any_pending_tasks = world.tasks.iter().any(|(_, _, s)| s == "pending");
        let any_pending_goals = world
            .goals
            .iter()
            .any(|goal| goal.status == "pending" || goal.status == "in-progress");
        let phase_suggests_active_work = any_red || any_pending_tasks || any_pending_goals;

        let changes = world.git_changes.as_ref();
        let touches_non_generated = changes.is_some_and(|c| c.source + c.context + c.other > 0);
        let only_generatedish = changes.is_some_and(|c| c.total > 0 && c.total == c.generatedish);
        let only_agent_context = changes.is_some_and(|c| c.total > 0 && c.total == c.agent_context);

        let mut rationale = String::new();
        if let Some(phase) = &world.active_phase {
            let _ = write!(
                rationale,
                "Repo is dirty during active phase {} ({}) — start by checking phase status, then classify changes. ",
                phase.id, phase.title
            );
        } else {
            rationale.push_str(
                "Repo is dirty — start by checking phase status (to understand workflow context), then classify changes. ",
            );
        }

        if let Some(c) = changes {
            let _ = write!(
                rationale,
                "Summary: total={} (modified={}, added={}, deleted={}, renamed={}, untracked={}); buckets: source={}, context={}, agent-context={}, generated-ish={}, other={}. ",
                c.total,
                c.modified,
                c.added,
                c.deleted,
                c.renamed,
                c.untracked,
                c.source,
                c.context,
                c.agent_context,
                c.generatedish,
                c.other
            );

            if !c.sample_source.is_empty() {
                let _ = write!(
                    rationale,
                    "Example source: {}. ",
                    c.sample_source.join(", ")
                );
            }
            if !c.sample_context.is_empty() {
                let _ = write!(
                    rationale,
                    "Example context: {}. ",
                    c.sample_context.join(", ")
                );
            }
            if !c.sample_agent_context.is_empty() {
                let _ = write!(
                    rationale,
                    "Example agent-context: {}. ",
                    c.sample_agent_context.join(", ")
                );
            }
            if !c.sample_generatedish.is_empty() {
                let _ = write!(
                    rationale,
                    "Example generated-ish: {}. ",
                    c.sample_generatedish.join(", ")
                );
            }
        }

        if phase_suggests_active_work && touches_non_generated {
            rationale.push_str(
                "This looks like real phase work: recommend committing and opening a PR (deleting files is exceptional).",
            );
        } else if only_generatedish {
            rationale.push_str(
                "These look mostly generated-ish; confirm whether they are intended outputs before deciding to commit. Prefer commit+PR over deletion if they’re tracked/meaningful.",
            );
        } else if only_agent_context {
            rationale.push_str(
                "Only agent-context files are dirty; keep going and let the phase boundary commit capture these.",
            );
        } else {
            rationale.push_str(
                "After reviewing phase status + diff, decide whether to commit+PR (usual) or revert unintended changes. Deleting files is exceptional.",
            );
        }

        // Insert in reverse priority order so higher-priority items end up first.
        if !only_agent_context {
            repair_actions.insert(
                0,
                SuggestedAction::external_shell(
                    "Recommend: Commit + open PR (after review)",
                    "git add -A && git commit -m \"<message>\" && git push -u origin HEAD && gh pr create --fill",
                    rationale,
                    WorkIntent::Ship,
                    Some(if phase_suggests_active_work && touches_non_generated {
                        0.85
                    } else if only_generatedish {
                        0.55
                    } else {
                        0.7
                    }),
                ),
            );
        }

        repair_actions.insert(
            0,
            SuggestedAction::external_shell(
                "Review diff",
                "git diff",
                "Review the actual changes before deciding to commit, revert, or regenerate.",
                WorkIntent::Orient,
                Some(0.8),
            ),
        );

        repair_actions.insert(
            0,
            SuggestedAction::external_shell(
                "Inspect working tree",
                "git status --porcelain",
                "Collect the concrete list of modified/untracked files before acting.",
                WorkIntent::Orient,
                Some(0.9),
            ),
        );

        repair_actions.insert(
            0,
            SuggestedAction::exo(
                "Check phase status",
                ExoCommandReference::new(&["phase", "status"]),
                "Phase status gives context: which phase you’re in and what work is pending.",
                WorkIntent::Orient,
                Some(0.95),
            ),
        );
    }

    if let Some(sidecar_sync) = &world.sidecar_sync {
        for debt in sidecar_sync.foreign_checkpoint_debt.iter().rev() {
            for action in debt.next_actions.iter().rev() {
                repair_actions.insert(0, action.clone());
            }
        }
    }
}

/// Machine-channel steering for argv patterns that look like shell syntax.
///
/// This is a suggestion hook only (no rewrite). Keep it minimal and stable.
pub fn steering_for_shell_operators(hits: &[ShellOperatorHit]) -> Option<Steering> {
    if hits.is_empty() {
        return None;
    }

    Some(Steering {
        next_call: NextCall {
            kind: NextCallKind::Help,
            params: serde_json::json!({ "address": { "kind": "root" } }),
        },
        priority: None,
        confidence: None,
        context_note: None,
    })
}

/// Add health-related repair actions when plan is degraded or critical.
///
/// Only warns during Executing/Verifying modes - suppressed in between-states
/// per RFC 0107 guidance (health warnings don't apply when exploring).
fn add_health_repairs(
    world: &WorldState,
    progress_mode: ProgressMode,
    repair_actions: &mut Vec<SuggestedAction>,
) {
    use crate::plan::{HealthStatus, HealthThresholds};

    // Only warn during Executing/Verifying modes (suppress in between-states)
    if progress_mode.is_between_state() {
        return;
    }

    // Need an active phase to compute health
    if world.active_phase.is_none() {
        return;
    }

    // Count pending/completed from world state (already probed)
    let pending_count = world
        .tasks
        .iter()
        .filter(|(_, _, status)| status != "completed")
        .count();
    let _completed_count = world
        .tasks
        .iter()
        .filter(|(_, _, status)| status == "completed")
        .count();

    let thresholds = HealthThresholds::default();

    // Quick check without loading plan again
    let status = if pending_count >= thresholds.pending_tasks_critical {
        HealthStatus::Critical
    } else if pending_count >= thresholds.pending_tasks_degraded {
        HealthStatus::Degraded
    } else {
        HealthStatus::Healthy
    };

    if status == HealthStatus::Healthy {
        return;
    }

    let severity_label = match status {
        HealthStatus::Critical => "CRITICAL",
        HealthStatus::Degraded => "DEGRADED",
        HealthStatus::Healthy => unreachable!(),
    };

    let rationale = format!(
        "Plan health: {}. {} pending tasks (threshold: {}). Consider: completing tasks, deferring low-priority work, or reviewing plan health.",
        severity_label, pending_count, thresholds.pending_tasks_degraded
    );

    let confidence = match status {
        HealthStatus::Critical => 0.8,
        HealthStatus::Degraded => 0.6,
        HealthStatus::Healthy => unreachable!(),
    };

    repair_actions.push(SuggestedAction::exo(
        "Review plan",
        ExoCommandReference::new(&["plan", "review"]),
        rationale,
        WorkIntent::Orient,
        Some(confidence),
    ));
}

/// Add steering suggestions for unreviewed completed epochs.
///
/// When an epoch is completed but not yet reviewed, suggest running
/// `exo epoch review <id>` to capture accomplishments and formally close it out.
fn add_epoch_review_suggestions(world: &WorldState, repair_actions: &mut Vec<SuggestedAction>) {
    if world.unreviewed_epochs.is_empty() {
        return;
    }

    // Suggest reviewing each unreviewed epoch
    for epoch in &world.unreviewed_epochs {
        let rationale = format!(
            "Epoch '{}' is completed but not yet reviewed. Run review to capture accomplishments, update changelog, and formally close out the epoch.",
            epoch.title
        );

        repair_actions.push(SuggestedAction::exo(
            format!("Review completed epoch: {}", epoch.title),
            ExoCommandReference::new(&["epoch", "review"]).positional(epoch.id.clone()),
            rationale,
            WorkIntent::Record,
            Some(0.7), // Advisory, not blocking
        ));
    }
}
/// Add goal-centric workflow nudges.
///
/// Per RFC 00177 (Goals and Tasks unified model), these nudges guide agents
/// toward goal-centric thinking:
///
/// 1. Suggest goal creation when an active phase has no goals (tasks).
///    Only during Planning mode to avoid nagging during execution.
///
/// 2. Warn when goals exist but the phase lacks RFC linkage.
///    RFC linkage is strongly encouraged (though not required) to maintain
///    traceability between planning and implementation.
fn add_goal_centric_nudges(
    world: &WorldState,
    progress_mode: ProgressMode,
    repair_actions: &mut Vec<SuggestedAction>,
) {
    let Some(active_phase) = &world.active_phase else {
        return;
    };

    let rfc_context = rfc_context_for_goal_nudges(world);

    // Nudge 1: Suggest goal creation when none exist
    // Only suggest during Planning mode to avoid nagging during execution
    if progress_mode == ProgressMode::Planning && world.tasks.is_empty() {
        let (rationale, confidence) = if active_phase.kind == PhaseKind::Chore {
            // Chore phases: lighter ceremony, goals are optional
            (
                format!(
                    "Chore phase '{}' has no goals. Consider adding a quick checklist \
                     of items to track progress, or just start working.",
                    active_phase.title
                ),
                0.5, // Lower confidence — goals are optional for chores
            )
        } else {
            (
                format!(
                    "Phase '{}' has no goals defined. Goals provide high-level intent \
                     and help track progress. Add at least one goal to define the phase's scope.",
                    active_phase.title
                ),
                0.75, // Advisory, fairly important
            )
        };
        repair_actions.push(SuggestedAction::exo(
            "Add a goal to the phase",
            ExoCommandReference::new(&["goal", "add"])
                .positional_placeholder("title", "sample goal"),
            apply_rfc_context(rfc_context.as_ref(), rationale),
            WorkIntent::Plan,
            Some(confidence),
        ));
    }

    // Nudge 2: Warn when goals exist but phase lacks RFC linkage
    // This is an advisory - RFC linkage is strongly encouraged but not required
    // Skip this check for Chore phases.
    if !world.tasks.is_empty()
        && active_phase.rfcs.is_empty()
        && active_phase.kind == PhaseKind::Regular
    {
        let rationale = format!(
            "Phase '{}' has {} goal(s) but no RFC linkage. Consider linking to \
                 an RFC for traceability between planning decisions and implementation.",
            active_phase.title,
            world.tasks.len()
        );
        repair_actions.push(SuggestedAction::exo(
            "Link phase to RFC(s)",
            ExoCommandReference::new(&["phase", "update"])
                .positional(active_phase.id.clone())
                .option_placeholder("rfcs", "rfc-id", "10194"),
            apply_rfc_context(rfc_context.as_ref(), rationale),
            WorkIntent::Plan,
            Some(0.5), // Lower priority advisory
        ));
    }
}

/// Build top-level RFC context from the world's pipeline data.
/// This produces a structured summary of all RFCs linked to the current phase,
/// suitable for inclusion in steering output so agents always know what they're advancing.
fn build_rfc_steering_context(world: &WorldState) -> Vec<RfcSteeringContext> {
    // Collect RFC IDs that are attached to in-progress goals
    let goal_rfc_ids: std::collections::HashSet<&str> = world
        .goals
        .iter()
        .filter(|g| g.status == "in-progress")
        .filter_map(|g| g.rfc.as_deref())
        .collect();

    let mut entries: Vec<_> = world
        .rfc_pipeline
        .values()
        .map(|entry| {
            let promotion_requirement = if entry.is_driving {
                entry
                    .target_stage
                    .map(|target| promotion_requirements(entry.current_stage, target).to_string())
            } else {
                None
            };

            // Derive implementation status:
            // - "in-progress" if attached to an in-progress goal
            // - "in-flight" if attached to the active phase (but no goal is in-progress for it)
            let implementation_status = if goal_rfc_ids.contains(entry.id.as_str()) {
                Some("in-progress".to_string())
            } else {
                Some("in-flight".to_string())
            };

            RfcSteeringContext {
                id: entry.id.clone(),
                title: entry.title.clone(),
                current_stage: entry.current_stage,
                target_stage: entry.target_stage,
                is_driving: entry.is_driving,
                promotion_requirement,
                implementation_status,
            }
        })
        .collect();

    // Driving RFCs first, then by ID for stability
    entries.sort_by(|a, b| {
        b.is_driving
            .cmp(&a.is_driving)
            .then_with(|| a.id.cmp(&b.id))
    });

    entries
}

/// Format a one-line summary of driving RFCs for injecting into action rationales.
fn format_rfc_summary(context: &[RfcSteeringContext]) -> String {
    let driving: Vec<_> = context.iter().filter(|e| e.is_driving).collect();

    match driving.as_slice() {
        [] => String::new(),
        [single] => {
            let target = single
                .target_stage
                .map_or(String::new(), |t| format!("→{t}"));
            format!(
                "Phase is advancing RFC {} «{}» (Stage {}{}).",
                single.id, single.title, single.current_stage, target
            )
        }
        multiple => {
            let summaries: Vec<_> = multiple
                .iter()
                .map(|e| {
                    let target = e.target_stage.map_or(String::new(), |t| format!("→{t}"));
                    format!("RFC {} (Stage {}{})", e.id, e.current_stage, target)
                })
                .collect();
            format!("Phase is advancing {}.", summaries.join(", "))
        }
    }
}

fn rfc_context_for_goal_nudges(world: &WorldState) -> Option<String> {
    let mut driving: Vec<_> = world
        .rfc_pipeline
        .values()
        .filter(|entry| entry.is_driving)
        .collect();

    if driving.is_empty() {
        return None;
    }

    driving.sort_by_key(|entry| (entry.current_stage, entry.id.as_str()));
    let primary = driving.first()?;
    let target = primary.target_stage?;
    let requirement = promotion_requirements(primary.current_stage, target);

    Some(format!(
        "Phase is advancing RFC {} (Stage {}→{}). {}.",
        primary.id, primary.current_stage, target, requirement
    ))
}

fn apply_rfc_context(context: Option<&String>, rationale: String) -> String {
    match context {
        Some(context) => format!("{context} {rationale}"),
        None => rationale,
    }
}

/// Add nudges to record completion logs once all tasks are finished.
///
/// For each goal whose tasks are all completed but lacks a completion log,
/// suggest `exo goal complete <id> --log`. This fires per-goal, not waiting
/// for the entire phase to complete—the agent has the best context right
/// after finishing a goal's tasks.
fn task_log_lines_by_goal(root: &Path) -> HashMap<String, Vec<String>> {
    {
        // SQLite: Get active phase goals from ExoState, then query task logs
        let mut by_goal = HashMap::new();
        let context = match crate::context::AgentContext::load(root.to_path_buf()) {
            Ok(context) => context,
            Err(_) => return by_goal,
        };
        let workspace_root = context.workspace_root_key();
        let db_path = crate::context::db_path(root, context.project.as_ref());
        let loader = match SqliteLoader::open(&db_path) {
            Ok(loader) => loader,
            Err(_) => return by_goal,
        };

        // Find active phase and its goals
        let active_phase = context.find_workspace_active_phase().ok().flatten();
        let goals = match active_phase {
            Some(info) => &info.phase.goals,
            None => return by_goal,
        };

        // For each goal, get task logs from the tasks_data table
        // Note: In SQLite, tasks are stored under goals, and logs are in task_logs table
        for goal in goals {
            let goal_id = goal.id.clone();
            if goal_id.is_empty() {
                continue;
            }

            // Get tasks for this goal and their logs
            let tasks =
                match loader.list_active_phase_tasks_for_workspace(workspace_root.as_deref()) {
                    Ok(tasks) => tasks,
                    Err(_) => continue,
                };

            let mut lines = Vec::new();
            for (composite_id, title, _status) in &tasks {
                // composite_id is "goal_id::task_id" - check if it belongs to this goal
                if !composite_id.starts_with(&format!("{goal_id}::")) {
                    continue;
                }

                // Extract task_id from composite_id
                let task_id = composite_id
                    .strip_prefix(&format!("{goal_id}::"))
                    .unwrap_or(composite_id);

                if let Ok(logs) = loader.load_task_logs(task_id) {
                    for (kind, message, _created_at) in logs {
                        let message = message.trim();
                        let text = if message.is_empty() {
                            if title.is_empty() {
                                "Task update"
                            } else {
                                title.as_str()
                            }
                        } else {
                            message
                        };
                        lines.push(format!("- [{kind}] {text}"));
                    }
                }
            }

            if !lines.is_empty() {
                by_goal.insert(goal_id, lines);
            }
        }

        by_goal
    }
}

/// Surface concerns on completed goals/tasks as repair actions in world steering.
///
/// When a user raises a concern on something already marked done, the agent
/// should re-examine it before proceeding — this is a reopening signal.
fn add_concern_on_completed_repairs(
    world: &WorldState,
    agent_id: Option<&str>,
    repairs: &mut Vec<SuggestedAction>,
) {
    let db_path = world.root.join(crate::context::SQLITE_DB_PATH);
    let loader = match crate::context::sqlite_loader::SqliteLoader::open(&db_path) {
        Ok(l) => l,
        Err(_) => return,
    };
    let items = match loader.load_inbox_filtered(None, None, None, None, None) {
        Ok(items) => items,
        Err(_) => return,
    };

    // Collect completed goal/task IDs for fast lookup
    let completed_goals: std::collections::HashSet<&str> = world
        .goals
        .iter()
        .filter(|g| g.status == "completed")
        .map(|g| g.id.as_str())
        .collect();
    let completed_tasks: std::collections::HashSet<&str> = world
        .tasks
        .iter()
        .filter(|(_, _, status)| status == "completed")
        .map(|(id, _, _)| id.as_str())
        .collect();

    for item in &items {
        // Only active concerns
        if item.intent != crate::inbox::InboxIntent::Concern {
            continue;
        }
        if item.status != crate::inbox::InboxItemStatus::Pending
            && item.status != crate::inbox::InboxItemStatus::Acknowledged
        {
            continue;
        }
        // Self-suppression
        if let Some(ref item_agent) = item.agent_id
            && let Some(caller) = agent_id
            && item_agent == caller
        {
            continue;
        }

        let Some(ref eid) = item.entity_id else {
            continue;
        };

        let is_completed = match item.entity_type.as_str() {
            "goal" => completed_goals.contains(eid.as_str()),
            "task" => completed_tasks.contains(eid.as_str()),
            _ => false,
        };

        if is_completed {
            repairs.push(SuggestedAction::exo(
                format!("Review concern on completed {} '{}'", item.entity_type, eid),
                ExoCommandReference::new(&["inbox", "list"])
                    .option("entity-type", item.entity_type.clone())
                    .option("entity-id", eid.clone()),
                format!(
                    "User raised a concern on completed {} '{}' — review before proceeding",
                    item.entity_type, eid
                ),
                WorkIntent::Orient,
                Some(0.85),
            ));
        }
    }
}

fn add_goal_completion_log_nudges(world: &WorldState, next_actions: &mut Vec<SuggestedAction>) {
    let task_log_lines = task_log_lines_by_goal(&world.root);

    for goal in &world.goals {
        // Skip goals that already have a completion log
        if goal.completion_log.is_some() {
            continue;
        }

        // Skip abandoned/skipped goals — they don't need narrative logs
        if goal.status == "abandoned" || goal.status == "skipped" {
            continue;
        }

        // Check if this goal has tasks and all are completed
        // Task IDs are formatted as "goal-id::task-id", so extract the goal part
        let goal_tasks: Vec<_> = world
            .tasks
            .iter()
            .filter(|(task_id, _, _)| {
                task_id
                    .split("::")
                    .next()
                    .is_some_and(|goal_part| goal_part == goal.id)
            })
            .collect();

        // Zero-task goals are considered ready to log.
        // Goals with tasks require all tasks to be completed.
        let ready_to_log = goal_tasks.is_empty()
            || goal_tasks
                .iter()
                .all(|(_, _, status)| status == "completed");

        if ready_to_log {
            let goal_ref = goal.canonical_ref();
            let log_lines = task_log_lines
                .get(&goal.id)
                .or_else(|| task_log_lines.get(&goal.label));

            let mut rationale = if goal_tasks.is_empty() {
                format!("Goal '{}' has no tasks and is ready to log", goal.label)
            } else {
                format!(
                    "All {} tasks for '{}' are complete",
                    goal_tasks.len(),
                    goal.label
                )
            };

            if let Some(lines) = log_lines {
                if !lines.is_empty() {
                    rationale.push_str(":\n");
                    rationale.push_str(&lines.join("\n"));
                } else {
                    rationale.push('.');
                }
            } else {
                rationale.push('.');
            }

            if goal_tasks.is_empty() {
                rationale.push_str(
                    "\nRecord the completion log now while context is fresh. Use `exo phase` for full task log details.",
                );
            } else {
                rationale.push_str(
                    "\nThis goal is shown as `done?` in status views. Record the completion log now while context is fresh. Use `exo phase` for full task log details.",
                );
            }

            next_actions.push(SuggestedAction::exo(
                format!("Log completion for '{}'", goal.label),
                ExoCommandReference::new(&["goal", "complete"])
                    .positional(goal_ref)
                    .option_placeholder("log", "summary", "completed the goal"),
                rationale,
                WorkIntent::Record,
                Some(0.95),
            ));
        }
    }
}

fn add_phase_completion_nudge(world: &WorldState, next_actions: &mut Vec<SuggestedAction>) {
    if world.goals.is_empty() {
        return;
    }

    let all_goals_logged = world.goals.iter().all(|goal| {
        goal.is_terminal()
            && goal
                .completion_log
                .as_ref()
                .is_some_and(|log| !log.is_empty())
    });

    if !all_goals_logged {
        return;
    }

    let goal_summaries: Vec<String> = world
        .goals
        .iter()
        .filter_map(|goal| {
            goal.completion_log.as_ref().map(|log| {
                let truncated = if log.chars().count() > 80 {
                    log.chars().take(80).collect::<String>()
                } else {
                    log.clone()
                };
                format!("- {}: {}", goal.label, truncated)
            })
        })
        .collect();

    next_actions.push(SuggestedAction::exo(
        format!("Finish the phase ({} goals complete)", world.goals.len()),
        ExoCommandReference::new(&["phase", "finish"]).option_placeholder(
            "message",
            "message",
            "completed phase",
        ),
        format!(
            "All {} goals are complete with logs:\n{}\nFinish the phase to archive and commit.",
            world.goals.len(),
            goal_summaries.join("\n")
        ),
        WorkIntent::Ship,
        Some(0.95),
    ));
}

#[allow(dead_code)]
const fn promotion_requirements(current: u8, target: u8) -> &'static str {
    match (current, target) {
        (0, 1) => "User approval required",
        (1, 2) => "Detailed spec required",
        (2, 3) => "Implementation must be complete",
        (3, 4) => "Manual must be updated",
        _ => "Advance to next stage",
    }
}

#[cfg(test)]
mod tests {
    use super::{promotion_requirements, summarize_surfaced_intents};
    use crate::context::{Goal, PhaseKind};
    use crate::inbox::{InboxIntent, InboxPriority, InboxSource, SurfacedIntent};

    #[test]
    fn promotion_requirements_for_stage_0_to_1() {
        assert_eq!(promotion_requirements(0, 1), "User approval required");
    }

    #[test]
    fn promotion_requirements_for_stage_1_to_2() {
        assert_eq!(promotion_requirements(1, 2), "Detailed spec required");
    }

    #[test]
    fn promotion_requirements_for_stage_2_to_3() {
        assert_eq!(
            promotion_requirements(2, 3),
            "Implementation must be complete"
        );
    }

    #[test]
    fn promotion_requirements_for_stage_3_to_4() {
        assert_eq!(promotion_requirements(3, 4), "Manual must be updated");
    }

    #[test]
    fn regular_phase_suggests_tdd_for_pending_task() {
        let goals = vec![Goal {
            id: "g1".to_string(),
            label: "Test goal".to_string(),
            status: "pending".to_string(),
            completion_log: None,
            kind: None,
            started_at: None,
            description: None,
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: None,
            target_stage: None,
        }];
        let tasks = vec![(
            "g1::t1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )];
        let block = super::derive_phase_steering(&tasks, &goals, PhaseKind::Regular);
        let tdd_action = block.next_actions.iter().find(|a| a.label.contains("TDD"));
        assert!(tdd_action.is_some(), "Regular phase should suggest TDD");
        assert!(
            block
                .next_actions
                .iter()
                .any(|a| a.command == "exo task add <title> --id <id>")
        );
        assert!(
            !block
                .next_actions
                .iter()
                .any(|a| a.command.contains("--label"))
        );
    }

    #[test]
    fn chore_phase_skips_tdd_for_pending_task() {
        let goals = vec![Goal {
            id: "g1".to_string(),
            label: "Cleanup task".to_string(),
            status: "pending".to_string(),
            completion_log: None,
            kind: None,
            started_at: None,
            description: None,
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: None,
            target_stage: None,
        }];
        let tasks = vec![(
            "g1::t1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )];
        let block = super::derive_phase_steering(&tasks, &goals, PhaseKind::Chore);
        let tdd_action = block.next_actions.iter().find(|a| a.label.contains("TDD"));
        assert!(tdd_action.is_none(), "Chore phase should not suggest TDD");
        let start_action = block
            .next_actions
            .iter()
            .find(|a| a.label.contains("Start next goal"));
        assert!(
            start_action.is_some(),
            "Chore phase should suggest starting directly"
        );
    }

    #[test]
    fn promotion_goal_suggests_review_not_tdd() {
        let goals = vec![Goal {
            id: "promote-rfc".to_string(),
            label: "Promote RFC 00238 to Stage 2".to_string(),
            status: "pending".to_string(),
            completion_log: None,
            kind: None,
            started_at: None,
            description: None,
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: Some("00238".to_string()),
            target_stage: Some(2),
        }];
        let block = super::derive_phase_steering(&[], &goals, PhaseKind::Regular);
        let tdd_action = block.next_actions.iter().find(|a| a.label.contains("TDD"));
        assert!(
            tdd_action.is_none(),
            "Promotion goal should not suggest TDD"
        );
        let review_action = block
            .next_actions
            .iter()
            .find(|a| a.label.contains("review") || a.label.contains("Review"));
        assert!(
            review_action.is_some(),
            "Promotion goal should suggest review"
        );
    }

    fn surfaced_intent(
        id: &str,
        entity_type: &str,
        entity_id: Option<&str>,
        priority: InboxPriority,
        subject: &str,
    ) -> SurfacedIntent {
        SurfacedIntent {
            id: id.to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.map(str::to_string),
            source: InboxSource::UserFeedback,
            intent: InboxIntent::Fyi,
            priority,
            agent_id: None,
            subject: subject.to_string(),
            relevance: 1.0,
        }
    }

    #[test]
    fn perception_summaries_group_by_entity() {
        let summaries = summarize_surfaced_intents(vec![
            surfaced_intent(
                "i1",
                "goal",
                Some("g1"),
                InboxPriority::NextTouch,
                "Goal one A",
            ),
            surfaced_intent(
                "i2",
                "goal",
                Some("g1"),
                InboxPriority::WhenRelevant,
                "Goal one B",
            ),
            surfaced_intent(
                "i3",
                "goal",
                Some("g1"),
                InboxPriority::Immediate,
                "Goal one C",
            ),
            surfaced_intent(
                "i4",
                "goal",
                Some("g2"),
                InboxPriority::NextTouch,
                "Goal two",
            ),
        ]);

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].entity_type, "goal");
        assert_eq!(summaries[0].entity_id.as_deref(), Some("g1"));
        assert_eq!(summaries[0].count, 3);
        // All items are fyi, so representative picks by priority: Immediate = "Goal one C"
        assert_eq!(summaries[0].sample_subject, "Goal one C (+2 more)");
        assert_eq!(
            summaries[0].drill_in,
            "exo inbox list --entity-type goal --entity-id g1"
        );
        assert_eq!(summaries[1].entity_id.as_deref(), Some("g2"));
        assert_eq!(summaries[1].count, 1);
        // Single item: no "(+N more)" suffix
        assert_eq!(summaries[1].sample_subject, "Goal two");
    }

    #[test]
    fn perception_summaries_choose_highest_priority() {
        let summaries = summarize_surfaced_intents(vec![
            surfaced_intent("i1", "goal", Some("g1"), InboxPriority::WhenRelevant, "One"),
            surfaced_intent("i2", "goal", Some("g1"), InboxPriority::Immediate, "Two"),
            surfaced_intent("i3", "goal", Some("g1"), InboxPriority::NextTouch, "Three"),
        ]);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].highest_priority, "immediate");
    }

    #[test]
    fn perception_summaries_prefer_most_communicative_intent() {
        // A claim at low priority beats an fyi at high priority for sample_subject
        let mut fyi_immediate = surfaced_intent(
            "i1",
            "goal",
            Some("g1"),
            InboxPriority::Immediate,
            "FYI: task added",
        );
        fyi_immediate.intent = InboxIntent::Fyi;

        let mut claim_next = surfaced_intent(
            "i2",
            "goal",
            Some("g1"),
            InboxPriority::NextTouch,
            "I think this is done",
        );
        claim_next.intent = InboxIntent::Claim;

        let mut concern_relevant = surfaced_intent(
            "i3",
            "goal",
            Some("g1"),
            InboxPriority::WhenRelevant,
            "Edge case concern",
        );
        concern_relevant.intent = InboxIntent::Concern;

        let summaries =
            summarize_surfaced_intents(vec![fyi_immediate, claim_next, concern_relevant]);

        assert_eq!(summaries.len(), 1);
        // Claim outranks concern outranks fyi, regardless of priority
        assert_eq!(
            summaries[0].sample_subject,
            "I think this is done (+2 more)"
        );
        // But highest_priority is still immediate (from the fyi)
        assert_eq!(summaries[0].highest_priority, "immediate");
    }
}
