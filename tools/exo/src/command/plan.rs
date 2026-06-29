//! Plan namespace commands.
//!
//! - `plan review`: Review plan structure (Pure, --fix for enhanced output)
//! - `plan update-status`: Update status of epoch/phase/goal (Write)
//!
//! Removed commands:
//! - `plan health` — merged into `plan review` / `exo status`
//! - `plan linearize` — one-time migration tool, no longer needed
//! - `plan migrate-ids` — one-time migration tool (internal API kept for upgrade plugin)
//! - `plan add-task`, `plan remove-task` — use `goal add` / `goal remove`
//! - `plan add-epoch`, `plan remove-epoch` — use `epoch add` / `epoch remove`
//! - `plan bankrupt` — use `epoch bankrupt`

use std::path::Path;

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::{AgentContext, Goal, SqliteWriter};
use crate::phase_owner;
use crate::plan;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::{Context, Result as ExoResult};
use serde::Serialize;

/// Find a goal by ID in the plan.
/// Returns None if the ID doesn't match any goal (might be an epoch or phase).
fn find_goal_by_id(root: &Path, id: &str) -> ExoResult<Option<Goal>> {
    let ctx = AgentContext::load(root.to_path_buf())?;

    for epoch in &ctx.plan.epochs {
        for phase in &epoch.phases {
            if let Some(goal) = phase.goals.iter().find(|g| g.id == id) {
                return Ok(Some(goal.clone()));
            }
        }
    }
    Ok(None)
}

fn find_phase_for_status_target(context: &AgentContext, id: &str) -> Option<String> {
    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.goals.iter().any(|goal| goal.id == id) {
                return Some(phase.id.clone());
            }
        }
    }

    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.id == id {
                return Some(phase.id.clone());
            }
        }
    }
    None
}

fn ensure_phase_write_allowed(ctx: &MutableCommandContext, phase_id: &str) -> ExoResult<()> {
    phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), phase_id)
}

/// Default steering for plan commands.
fn default_plan_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "Review plan".to_string(),
        command: "exo plan review".to_string(),
        rationale: "Inspect the plan to confirm IDs and state.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.6),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the plan namespace
// ============================================================================

/// Plan namespace command specification.
///
/// This enum is the authoritative definition of the plan namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `PlanCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "plan", description = "Plan management commands")]
pub enum PlanCommands {
    #[exo(
        effect = "pure",
        description = "Review the plan for health and progress"
    )]
    Review {
        #[exo(flag, description = "Enable enhanced output with fixes")]
        fix: bool,
    },

    #[exo(
        effect = "pure",
        description = "Return the full plan state as JSON (for extension consumption)"
    )]
    Snapshot,

    #[exo(
        effect = "write",
        operation = "update-status",
        upgrade_gate,
        description = "Update the status of an item"
    )]
    UpdateStatus {
        #[exo(positional, description = "The item ID (epoch/phase/goal)")]
        id: String,
        #[exo(positional, description = "The new status value")]
        status: String,
    },

    #[exo(
        effect = "pure",
        operation = "read",
        description = "Read the full plan state as JSON"
    )]
    Read,

    #[exo(
        effect = "write",
        operation = "move-goals",
        upgrade_gate,
        description = "Move goals to a different phase"
    )]
    MoveGoals {
        #[exo(positional, description = "The source phase ID")]
        source_phase_id: String,
        #[exo(positional, description = "The target phase ID")]
        target_phase_id: String,
        #[exo(positional, description = "Comma-separated goal IDs to move")]
        goal_ids: String,
    },
}

impl PlanCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Review { fix } => CommandBox::pure(PlanReview::new(fix)),
            Self::Snapshot => CommandBox::pure(PlanSnapshot),
            Self::Read => CommandBox::pure(PlanRead),
            Self::UpdateStatus { id, status } => {
                CommandBox::mutable(PlanUpdateStatus::new(id, status))
            }
            Self::MoveGoals {
                source_phase_id,
                target_phase_id,
                goal_ids,
            } => CommandBox::mutable(PlanMoveGoals::new(
                source_phase_id,
                target_phase_id,
                goal_ids
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .collect(),
            )),
        })
    }
}

// ============================================================================
// plan review (Pure)
// ============================================================================

/// Review the plan for health and progress.
#[derive(Debug, Clone, Copy)]
pub struct PlanReview {
    pub fix: bool,
}

impl Default for PlanReview {
    fn default() -> Self {
        Self::new(false)
    }
}

impl PlanReview {
    pub const fn new(fix: bool) -> Self {
        Self { fix }
    }
}

impl Command for PlanReview {
    fn namespace(&self) -> &'static str {
        "plan"
    }

    fn operation(&self) -> &'static str {
        "review"
    }

    fn description(&self) -> &'static str {
        "Review the plan for health and progress"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_plan_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        match ctx.format {
            OutputFormat::Json => {
                let json = plan::build_plan_review_json_from_context(&agent_ctx, self.fix)?;
                Ok(CommandOutput::data(json))
            }
            OutputFormat::Human => {
                plan::show_plan_review_human_from_context(&agent_ctx, self.fix)?;
                Ok(CommandOutput::message(""))
            }
        }
    }
}

// ============================================================================
// plan snapshot (Pure)
// ============================================================================

/// Return the full plan state as JSON for extension consumption.
///
/// This serializes the entire `ExoState` (all epochs, phases, goals) as JSON.
/// The output shape matches what the extension's `PlanSchema` Zod validator expects,
/// enabling the extension to consume plan data from the daemon instead of reading
/// TOML files from disk.
#[derive(Debug, Clone, Copy)]
pub struct PlanSnapshot;

impl Command for PlanSnapshot {
    fn namespace(&self) -> &'static str {
        "plan"
    }

    fn operation(&self) -> &'static str {
        "snapshot"
    }

    fn description(&self) -> &'static str {
        "Return the full plan state as JSON (for extension consumption)"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_plan_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let json = serde_json::to_value(&agent_ctx.plan)
            .with_context(|| "Failed to serialize plan state")?;

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(json)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&json)?;
                Ok(CommandOutput::new(json, pretty))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlanRead;

impl Command for PlanRead {
    fn namespace(&self) -> &'static str {
        "plan"
    }

    fn operation(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read the full plan state as JSON"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_plan_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        PlanSnapshot.execute(ctx)
    }
}

// ============================================================================
// plan update-status (Write)
// ============================================================================

/// Update the status of an item.
#[derive(Debug, Clone)]
pub struct PlanUpdateStatus {
    pub id: String,
    pub status: String,
}

impl PlanUpdateStatus {
    pub fn new(id: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: status.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct PlanUpdateStatusOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    status: String,
}

impl Command for PlanUpdateStatus {
    fn namespace(&self) -> &'static str {
        "plan"
    }

    fn operation(&self) -> &'static str {
        "update-status"
    }

    fn description(&self) -> &'static str {
        "Update the status of an item"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_plan_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PlanUpdateStatus should be dispatched via execute_mut")
    }
}

impl MutableCommand for PlanUpdateStatus {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        // RFC 00229: Prevent setting goals to "completed" without a completion log.
        // Goals must be completed via `exo goal complete --log` to ensure the human
        // synthesizes what was accomplished.
        if self.status == "completed"
            && let Some(goal) = find_goal_by_id(ctx.root, &self.id)?
            && goal.completion_log.is_none()
        {
            return Err(anyhow::anyhow!(
                "Cannot set goal '{}' to 'completed' directly.\n\n\
                 [Next Step]\n\
                 Use: exo goal complete {} --log '<what was accomplished>'\n\n\
                 The completion log captures what was learned or delivered—it's the final deliverable, not bookkeeping.",
                self.id,
                self.id
            ));
        }
        if let Some(phase_id) = find_phase_for_status_target(&agent_ctx, &self.id) {
            ensure_phase_write_allowed(ctx, &phase_id)?;
        }

        let writer = SqliteWriter::open(ctx.db_path())?;
        // update_status can target phases or goals — try goal first, fall back to phase
        if writer.update_goal_status(&self.id, &self.status).is_err() {
            writer.update_phase_status(&self.id, &self.status)?;
        }

        let output = PlanUpdateStatusOutput {
            kind: "plan.update-status",
            ok: true,
            id: self.id.clone(),
            status: self.status.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Updated status of '{}' to '{}'", self.id, self.status);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanMoveGoals {
    pub source_phase_id: String,
    pub target_phase_id: String,
    pub goal_ids: Vec<String>,
}

impl PlanMoveGoals {
    pub fn new(
        source_phase_id: impl Into<String>,
        target_phase_id: impl Into<String>,
        goal_ids: Vec<String>,
    ) -> Self {
        Self {
            source_phase_id: source_phase_id.into(),
            target_phase_id: target_phase_id.into(),
            goal_ids,
        }
    }
}

impl Command for PlanMoveGoals {
    fn namespace(&self) -> &'static str {
        "plan"
    }

    fn operation(&self) -> &'static str {
        "move-goals"
    }

    fn description(&self) -> &'static str {
        "Move goals to a different phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PlanMoveGoals should be dispatched via execute_mut")
    }
}

impl MutableCommand for PlanMoveGoals {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let mut source_phase_found = false;
        let mut target_phase_found = false;
        let mut source_goal_ids = std::collections::HashSet::new();

        for epoch in &agent_ctx.plan.epochs {
            for phase in &epoch.phases {
                if phase.id == self.source_phase_id {
                    source_phase_found = true;
                    source_goal_ids.extend(phase.goals.iter().map(|goal| goal.id.clone()));
                }
                if phase.id == self.target_phase_id {
                    target_phase_found = true;
                }
            }
        }

        if !source_phase_found {
            anyhow::bail!("Source phase not found: {}", self.source_phase_id);
        }
        if !target_phase_found {
            anyhow::bail!("Target phase not found: {}", self.target_phase_id);
        }
        for goal_id in &self.goal_ids {
            if !source_goal_ids.contains(goal_id) {
                anyhow::bail!(
                    "Goal '{}' not found in source phase '{}'",
                    goal_id,
                    self.source_phase_id
                );
            }
        }
        ensure_phase_write_allowed(ctx, &self.source_phase_id)?;
        ensure_phase_write_allowed(ctx, &self.target_phase_id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        for goal_id in &self.goal_ids {
            writer.move_goal_to_phase(goal_id, &self.target_phase_id)?;
        }

        let result = serde_json::json!({
            "kind": "plan.move-goals",
            "ok": true,
            "source_phase_id": self.source_phase_id,
            "target_phase_id": self.target_phase_id,
            "goal_ids": self.goal_ids,
        });

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => Ok(CommandOutput::new(
                result,
                format!(
                    "Moved {} goal(s) from '{}' to '{}'",
                    self.goal_ids.len(),
                    self.source_phase_id,
                    self.target_phase_id
                ),
            )),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Pure operation tests

    #[test]
    fn test_plan_review_metadata() {
        let cmd = PlanReview::new(false);
        assert_eq!(cmd.namespace(), "plan");
        assert_eq!(cmd.operation(), "review");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_plan_review_with_fix() {
        let cmd = PlanReview::new(true);
        assert!(cmd.fix);
    }

    // Write operation tests

    #[test]
    fn test_plan_update_status_metadata() {
        let cmd = PlanUpdateStatus::new("p1", "completed");
        assert_eq!(cmd.namespace(), "plan");
        assert_eq!(cmd.operation(), "update-status");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "p1");
        assert_eq!(cmd.status, "completed");
    }
}
