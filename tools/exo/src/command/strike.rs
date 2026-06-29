//! Strike namespace commands.
//!
//! - `strike start`: Start a surgical strike (Exec with upgrade gate)
//! - `strike finish`: Finish the current strike (Exec)
//! - `strike abort`: Abort the current strike (Exec)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::{AgentContext, SqliteWriter};
use crate::failure::ExoFailure;
use crate::phase_owner;
use crate::state_machine;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use chrono::Utc;
use serde::Serialize;

/// Default steering for strike commands.
fn default_strike_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "Show map".to_string(),
        command: "exo map".to_string(),
        rationale: "Use map to orient and get suggested next actions.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the strike namespace
// ============================================================================

/// Strike namespace command specification.
///
/// This enum is the authoritative definition of the strike namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `StrikeCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "strike", description = "Surgical strike commands")]
pub enum StrikeCommands {
    #[exo(
        effect = "exec",
        upgrade_gate,
        description = "Start a new surgical strike"
    )]
    Start {
        #[exo(long, short = 'n', description = "The strike name")]
        name: String,
        #[exo(long, short = 'g', description = "The strike goal")]
        goal: String,
    },

    #[exo(effect = "exec", description = "Finish the current surgical strike")]
    Finish,

    #[exo(effect = "exec", description = "Abort the current surgical strike")]
    Abort,
}

impl StrikeCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Start { name, goal } => CommandBox::mutable(StrikeStart::new(name, goal)),
            Self::Finish => CommandBox::mutable(StrikeFinish::new()),
            Self::Abort => CommandBox::mutable(StrikeAbort::new()),
        })
    }
}

// ============================================================================
// strike start
// ============================================================================

/// Start a new surgical strike.
///
/// This command has an **upgrade gate**: it blocks when deprecated projections
/// exist, requiring migration before new strikes can be started.
#[derive(Debug, Clone)]
pub struct StrikeStart {
    pub name: String,
    pub goal: String,
}

impl StrikeStart {
    pub fn new(name: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            goal: goal.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct StrikeStartOutput {
    kind: &'static str,
    ok: bool,
    strike_id: String,
}

impl Command for StrikeStart {
    fn namespace(&self) -> &'static str {
        "strike"
    }

    fn operation(&self) -> &'static str {
        "start"
    }

    fn description(&self) -> &'static str {
        "Start a new surgical strike"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_strike_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("StrikeStart should be dispatched via execute_mut")
    }
}

impl MutableCommand for StrikeStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        // Check upgrade gate first - create a temporary AgentContext for the check
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        state_machine::check_upgrade_gate(&agent_ctx)?;

        let strike_id = {
            let has_active_strike = agent_ctx.plan.epochs.iter().any(|epoch| {
                epoch.phases.iter().any(|phase| {
                    phase.goals.iter().any(|goal| {
                        goal.kind.as_deref() == Some("strike") && goal.status == "in-progress"
                    })
                })
            });

            if has_active_strike {
                let failure = ExoFailure::new(
                    ErrorCode::InvalidInput,
                    "An active strike is already in progress.".to_string(),
                    ExoFailure::orienting_steering(vec![
                        SuggestedAction {
                            label: "Finish strike".to_string(),
                            command: "exo strike finish".to_string(),
                            rationale: "Complete the active strike before starting a new one."
                                .to_string(),
                            intent: WorkIntent::Ship,
                            confidence: Some(0.7),
                        },
                        SuggestedAction {
                            label: "Abort strike".to_string(),
                            command: "exo strike abort".to_string(),
                            rationale: "Abort the active strike if it is no longer needed."
                                .to_string(),
                            intent: WorkIntent::Ship,
                            confidence: Some(0.6),
                        },
                    ]),
                )
                .with_details(serde_json::json!({
                    "command": "strike.start",
                }));

                return Err(failure.into());
            }

            let Some(active_phase) = agent_ctx.find_workspace_active_phase()? else {
                let failure = ExoFailure::new(
                    ErrorCode::NotFound,
                    "No active phase found to attach a strike.".to_string(),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "Show phase status".to_string(),
                        command: "exo phase status".to_string(),
                        rationale: "Confirm the active phase before starting a strike.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.7),
                    }]),
                )
                .with_details(serde_json::json!({
                    "command": "strike.start",
                }));

                return Err(failure.into());
            };

            phase_owner::ensure_phase_write_allowed(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                &active_phase.phase.id,
            )?;

            let strike_id = format!("strike-{}", Utc::now().timestamp());
            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.add_strike_goal(&active_phase.phase.id, &strike_id, &self.name, &self.goal)?;
            strike_id
        };

        let output = StrikeStartOutput {
            kind: "strike.start",
            ok: true,
            strike_id,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Started surgical strike.".to_string(),
            )),
        }
    }
}

// ============================================================================
// strike finish
// ============================================================================

/// Finish the current surgical strike.
#[derive(Debug, Clone, Copy)]
pub struct StrikeFinish;

impl StrikeFinish {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for StrikeFinish {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
struct StrikeFinishOutput {
    kind: &'static str,
    ok: bool,
    strike_id: String,
}

impl Command for StrikeFinish {
    fn namespace(&self) -> &'static str {
        "strike"
    }

    fn operation(&self) -> &'static str {
        "finish"
    }

    fn description(&self) -> &'static str {
        "Finish the current surgical strike"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_strike_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("StrikeFinish should be dispatched via execute_mut")
    }
}

impl MutableCommand for StrikeFinish {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let strike_id = {
            let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
            let active_phase = agent_ctx.find_workspace_active_phase()?;
            if let Some(active_phase) = active_phase.as_ref() {
                phase_owner::ensure_phase_write_allowed(
                    ctx.root,
                    ctx.project,
                    &ctx.db_path(),
                    &active_phase.phase.id,
                )?;
            }
            let strike_goal = active_phase.and_then(|phase| {
                phase
                    .phase
                    .goals
                    .iter()
                    .find(|goal| {
                        goal.kind.as_deref() == Some("strike") && goal.status == "in-progress"
                    })
                    .map(|goal| goal.id.clone())
            });

            let Some(strike_id) = strike_goal else {
                let failure = ExoFailure::new(
                    ErrorCode::NotFound,
                    "No active surgical strike to finish.".to_string(),
                    ExoFailure::orienting_steering(vec![
                        SuggestedAction {
                            label: "Show phase status".to_string(),
                            command: "exo phase status --full".to_string(),
                            rationale: "Confirm whether a strike is currently active.".to_string(),
                            intent: WorkIntent::Orient,
                            confidence: Some(0.8),
                        },
                        SuggestedAction {
                            label: "Start a strike".to_string(),
                            command: "exo strike start <name> --goal \"...\"".to_string(),
                            rationale: "Begin a focused strike with a clear goal.".to_string(),
                            intent: WorkIntent::Execute,
                            confidence: Some(0.6),
                        },
                    ]),
                )
                .with_details(serde_json::json!({
                    "command": "strike.finish",
                }));

                return Err(failure.into());
            };

            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.update_goal_status(&strike_id, "completed")?;
            writer.update_goal_completion_log(&strike_id, "Finished surgical strike.")?;
            strike_id
        };

        let output = StrikeFinishOutput {
            kind: "strike.finish",
            ok: true,
            strike_id,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Finished surgical strike.".to_string(),
            )),
        }
    }
}

// ============================================================================
// strike abort
// ============================================================================

/// Abort the current surgical strike.
#[derive(Debug, Clone, Copy)]
pub struct StrikeAbort;

impl StrikeAbort {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for StrikeAbort {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
struct StrikeAbortOutput {
    kind: &'static str,
    ok: bool,
    strike_id: String,
}

impl Command for StrikeAbort {
    fn namespace(&self) -> &'static str {
        "strike"
    }

    fn operation(&self) -> &'static str {
        "abort"
    }

    fn description(&self) -> &'static str {
        "Abort the current surgical strike"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_strike_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("StrikeAbort should be dispatched via execute_mut")
    }
}

impl MutableCommand for StrikeAbort {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let strike_id = {
            let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
            let active_phase = agent_ctx.find_workspace_active_phase()?;
            if let Some(active_phase) = active_phase.as_ref() {
                phase_owner::ensure_phase_write_allowed(
                    ctx.root,
                    ctx.project,
                    &ctx.db_path(),
                    &active_phase.phase.id,
                )?;
            }
            let strike_goal = active_phase.and_then(|phase| {
                phase
                    .phase
                    .goals
                    .iter()
                    .find(|goal| {
                        goal.kind.as_deref() == Some("strike") && goal.status == "in-progress"
                    })
                    .map(|goal| goal.id.clone())
            });

            let Some(strike_id) = strike_goal else {
                let failure = ExoFailure::new(
                    ErrorCode::NotFound,
                    "No active surgical strike to abort.".to_string(),
                    ExoFailure::orienting_steering(vec![
                        SuggestedAction {
                            label: "Show phase status".to_string(),
                            command: "exo phase status --full".to_string(),
                            rationale: "Confirm whether a strike is currently active.".to_string(),
                            intent: WorkIntent::Orient,
                            confidence: Some(0.8),
                        },
                        SuggestedAction {
                            label: "Start a strike".to_string(),
                            command: "exo strike start <name> --goal \"...\"".to_string(),
                            rationale: "Begin a focused strike with a clear goal.".to_string(),
                            intent: WorkIntent::Execute,
                            confidence: Some(0.6),
                        },
                    ]),
                )
                .with_details(serde_json::json!({
                    "command": "strike.abort",
                }));

                return Err(failure.into());
            };

            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.update_goal_status(&strike_id, "abandoned")?;
            strike_id
        };

        let output = StrikeAbortOutput {
            kind: "strike.abort",
            ok: true,
            strike_id,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Aborted surgical strike.".to_string(),
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
    use crate::api::protocol::Effect;

    #[test]
    fn test_strike_start_metadata() {
        let cmd = StrikeStart::new("test-strike", "Fix a bug");
        assert_eq!(cmd.namespace(), "strike");
        assert_eq!(cmd.operation(), "start");
        assert_eq!(cmd.effect(), Effect::Exec);
    }

    #[test]
    fn test_strike_start_fields() {
        let cmd = StrikeStart::new("test-strike", "Fix a bug");
        assert_eq!(cmd.name, "test-strike");
        assert_eq!(cmd.goal, "Fix a bug");
    }

    #[test]
    fn test_strike_finish_metadata() {
        let cmd = StrikeFinish::new();
        assert_eq!(cmd.namespace(), "strike");
        assert_eq!(cmd.operation(), "finish");
        assert_eq!(cmd.effect(), Effect::Exec);
    }

    #[test]
    fn test_strike_abort_metadata() {
        let cmd = StrikeAbort::new();
        assert_eq!(cmd.namespace(), "strike");
        assert_eq!(cmd.operation(), "abort");
        assert_eq!(cmd.effect(), Effect::Exec);
    }

    #[test]
    fn test_strike_finish_default() {
        let cmd = StrikeFinish::default();
        assert_eq!(cmd.namespace(), "strike");
    }

    #[test]
    fn test_strike_abort_default() {
        let cmd = StrikeAbort::default();
        assert_eq!(cmd.namespace(), "strike");
    }
}
