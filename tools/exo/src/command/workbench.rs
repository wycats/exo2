//! Local lane workbench commands.

use super::traits::{Command, CommandBox, CommandContext, CommandOutput};
use crate::workbench::daemon_required_failure;
use anyhow::Result as ExoResult;

#[derive(Debug, Clone, Copy, exospec::ExoSpec)]
#[exo(namespace = "workbench", description = "Local lane workbench commands")]
pub enum WorkbenchCommands {
    #[exo(effect = "pure", description = "Launch the local lane workbench")]
    Launch,

    #[exo(
        effect = "pure",
        description = "Read the current lane workbench snapshot"
    )]
    Snapshot,
}

impl WorkbenchCommands {
    pub fn to_command_box(self, _root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Launch => CommandBox::pure(WorkbenchLaunch),
            Self::Snapshot => CommandBox::pure(WorkbenchSnapshot),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WorkbenchLaunch;

impl Command for WorkbenchLaunch {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "launch"
    }

    fn execute(&self, ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        let result = services.launch(ctx.root)?;
        let message = format!(
            "Open the Exo workbench for {}:\n{}\n\nThis link expires in five minutes.",
            result.workspace.label, result.url
        );
        Ok(CommandOutput::new(result, message))
    }

    fn description(&self) -> &'static str {
        "Launch the local lane workbench"
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WorkbenchSnapshot;

impl Command for WorkbenchSnapshot {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "snapshot"
    }

    fn execute(&self, ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(services.snapshot(ctx.root)?))
    }

    fn description(&self) -> &'static str {
        "Read the current lane workbench snapshot"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::{Effect, ErrorCode, RecoveryClass};
    use crate::command::command_spec::CommandSpec;
    use crate::command::registry::default_registry;
    use crate::command::traits::{CommandContext, OutputFormat};
    use crate::failure::ExoFailure;
    use std::path::Path;

    fn direct_context() -> CommandContext<'static> {
        CommandContext {
            root: Path::new("."),
            project: None,
            format: OutputFormat::Json,
            agent_id: None,
            workflow_confirmation: None,
            input_content: None,
            runtime_services: None,
        }
    }

    #[test]
    fn registered_workbench_commands_are_pure_replayable_reads() {
        let spec = CommandSpec::from_registry(&default_registry());
        for operation in ["launch", "snapshot"] {
            let operation = spec
                .operation("workbench", operation)
                .expect("registered workbench operation");
            assert_eq!(operation.effect, Effect::Pure);
            assert_eq!(operation.recovery_class, RecoveryClass::ReplayableRead);
        }
    }

    #[test]
    fn direct_workbench_commands_require_daemon_runtime_services() {
        for error in [
            WorkbenchLaunch
                .execute(&direct_context())
                .expect_err("direct launch must fail"),
            WorkbenchSnapshot
                .execute(&direct_context())
                .expect_err("direct snapshot must fail"),
        ] {
            let failure = error
                .downcast_ref::<ExoFailure>()
                .expect("structured workbench failure");
            assert_eq!(failure.error.code, ErrorCode::PreconditionFailed);
            assert_eq!(
                failure.error.details.as_ref().expect("details")["kind"],
                "workbench.daemon_required"
            );
        }
    }
}
