//! Command dispatcher for routing and executing commands.
//!
//! The dispatcher handles:
//! - Format conversion (JSON/Human output)
//! - Upgrade gate checks
//! - Error boxing with steering suggestions
//!
//! # Spec-Driven Dispatch (RFC 0132)
//!
//! The dispatcher can work with:
//! - Clap-parsed commands via `CommandBox`
//! - Spec-routed commands via `Invocation`

use super::traits::{
    Command, CommandContext, CommandOutput, MutableCommand, MutableCommandContext, OutputFormat,
};
use crate::api::protocol::{Effect, PROTOCOL_VERSION, ResponseEnvelope, Status};
use crate::boundary;
use anyhow::Result as ExoResult;

/// Dispatches commands with centralized handling.
#[derive(Debug)]
pub struct CommandDispatcher<'a> {
    ctx: CommandContext<'a>,
}

impl<'a> CommandDispatcher<'a> {
    pub const fn new(ctx: CommandContext<'a>) -> Self {
        Self { ctx }
    }

    /// Execute a read-only command.
    pub fn dispatch(&self, cmd: &dyn Command) -> ExoResult<()> {
        let output = cmd.execute(&self.ctx)?;
        self.render(output)?;
        Ok(())
    }

    /// Execute a mutable command.
    pub fn dispatch_mut(&self, cmd: &dyn MutableCommand) -> ExoResult<()> {
        let mut mutable_ctx = MutableCommandContext {
            root: self.ctx.root,
            project: self.ctx.project,
            format: self.ctx.format,
            agent_id: self.ctx.agent_id.clone(),
            workflow_confirmation: self.ctx.workflow_confirmation.clone(),
        };
        let output = cmd.execute_mut(&mut mutable_ctx)?;
        self.render(output)?;
        Ok(())
    }

    /// Render command output based on format.
    fn render(&self, output: CommandOutput) -> ExoResult<()> {
        match self.ctx.format {
            OutputFormat::Json => {
                // Wrap in ResponseEnvelope for protocol consistency
                let envelope = ResponseEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    id: "cli".to_string(),
                    status: Status::Ok,
                    result: Some(output.data),
                    error: None,
                    ticket: None,
                    steering: None,
                    reminders: None,
                    display: None,
                    preview: None,
                    effect: None,
                    trace: None,
                };
                let json = serde_json::to_string_pretty(&envelope)?;
                println!("{json}");
            }
            OutputFormat::Human => {
                // Prefer human message, fall back to formatted data
                if let Some(msg) = output.human_message {
                    println!("{msg}");
                } else if !output.data.is_null() {
                    // Format data as human-readable
                    let json = serde_json::to_string_pretty(&output.data)?;
                    println!("{json}");
                }
            }
        }
        Ok(())
    }
}

/// Extension trait for wrapping command errors with steering.
pub trait CommandResultExt<T> {
    /// Box errors with steering suggestions for the given command.
    fn box_for_command(self, cmd: &dyn Command) -> ExoResult<T>;
}

impl<T> CommandResultExt<T> for ExoResult<T> {
    fn box_for_command(self, cmd: &dyn Command) -> Self {
        self.map_err(|e| {
            let actions = cmd.default_steering();
            if actions.is_empty() {
                e
            } else {
                // Convert Box<dyn Error> back to anyhow::Error
                let boxed = boundary::box_anyhow_internal_with_actions(e, actions);
                anyhow::anyhow!("{boxed}")
            }
        })
    }
}

/// Check if a command effect requires upgrade gate validation.
pub const fn requires_upgrade_gate(effect: Effect, has_upgrade_gate: bool) -> bool {
    matches!(effect, Effect::Write | Effect::Exec) && has_upgrade_gate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    struct DummyCommand;

    impl Command for DummyCommand {
        fn namespace(&self) -> &'static str {
            "test"
        }
        fn operation(&self) -> &'static str {
            "dummy"
        }
        fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
            Ok(CommandOutput::message("Dummy executed"))
        }
    }

    #[test]
    fn test_upgrade_gate_logic() {
        assert!(!requires_upgrade_gate(Effect::Pure, false));
        assert!(!requires_upgrade_gate(Effect::Pure, true));
        assert!(!requires_upgrade_gate(Effect::Write, false));
        assert!(requires_upgrade_gate(Effect::Write, true));
        assert!(requires_upgrade_gate(Effect::Exec, true));
    }
}
