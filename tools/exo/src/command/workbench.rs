//! Local lane workbench commands.

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
};
use crate::api::protocol::Effect;
use crate::workbench::daemon_required_failure;
use anyhow::Result as ExoResult;

fn mutable_command_unreachable(name: &str) -> ! {
    unreachable!("{name} should be dispatched via execute_mut")
}

#[derive(Debug, Clone, exospec::ExoSpec)]
#[exo(namespace = "workbench", description = "Local lane workbench commands")]
pub enum WorkbenchCommands {
    #[exo(effect = "pure", description = "Launch the local lane workbench")]
    Launch,

    #[exo(
        effect = "pure",
        description = "Read the current lane workbench snapshot"
    )]
    Snapshot,

    #[exo(
        effect = "pure",
        description = "Inspect a workbench lane without focusing it"
    )]
    Inspect {
        #[exo(positional, description = "Lane ID")]
        id: String,
    },

    #[exo(
        operation = "pairing.list",
        effect = "pure",
        description = "List durable browser pairings"
    )]
    PairingList,

    #[exo(
        operation = "pairing.revoke",
        effect = "write",
        description = "Revoke a durable browser pairing"
    )]
    PairingRevoke {
        #[exo(positional, description = "Pairing selector")]
        selector: String,
    },

    #[exo(
        operation = "pairing.forget",
        effect = "write",
        description = "Delete a retained browser pairing record"
    )]
    PairingForget {
        #[exo(positional, description = "Pairing selector")]
        selector: String,
    },

    #[exo(
        operation = "pairing.rename",
        effect = "write",
        description = "Name a durable browser pairing"
    )]
    PairingRename {
        #[exo(positional, description = "Pairing selector")]
        selector: String,
        #[exo(positional, description = "Pairing nickname")]
        nickname: String,
    },
}

impl WorkbenchCommands {
    pub fn to_command_box(self, _root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Launch => CommandBox::pure(WorkbenchLaunch),
            Self::Snapshot => CommandBox::pure(WorkbenchSnapshot),
            Self::Inspect { id } => CommandBox::pure(WorkbenchInspect::new(id)),
            Self::PairingList => CommandBox::pure(WorkbenchPairingList),
            Self::PairingRevoke { selector } => {
                CommandBox::mutable(WorkbenchPairingRevoke::new(selector))
            }
            Self::PairingForget { selector } => {
                CommandBox::mutable(WorkbenchPairingForget::new(selector))
            }
            Self::PairingRename { selector, nickname } => {
                CommandBox::mutable(WorkbenchPairingRename::new(selector, nickname))
            }
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WorkbenchPairingList;

impl Command for WorkbenchPairingList {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "pairing.list"
    }

    fn execute(&self, ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(services.pairings()?))
    }

    fn description(&self) -> &'static str {
        "List durable browser pairings"
    }
}

#[derive(Debug, Clone)]
pub struct WorkbenchPairingRevoke {
    selector: String,
}

impl WorkbenchPairingRevoke {
    pub fn new(selector: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
        }
    }
}

impl Command for WorkbenchPairingRevoke {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "pairing.revoke"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("WorkbenchPairingRevoke")
    }

    fn description(&self) -> &'static str {
        "Revoke a durable browser pairing"
    }
}

impl MutableCommand for WorkbenchPairingRevoke {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(
            services.revoke_pairing(&self.selector)?,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct WorkbenchPairingForget {
    selector: String,
}

impl WorkbenchPairingForget {
    pub fn new(selector: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
        }
    }
}

impl Command for WorkbenchPairingForget {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "pairing.forget"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("WorkbenchPairingForget")
    }

    fn description(&self) -> &'static str {
        "Delete a retained browser pairing record"
    }
}

impl MutableCommand for WorkbenchPairingForget {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(
            services.forget_pairing(&self.selector)?,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct WorkbenchPairingRename {
    selector: String,
    nickname: String,
}

impl WorkbenchPairingRename {
    pub fn new(selector: impl Into<String>, nickname: impl Into<String>) -> Self {
        Self {
            selector: selector.into(),
            nickname: nickname.into(),
        }
    }
}

impl Command for WorkbenchPairingRename {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "pairing.rename"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("WorkbenchPairingRename")
    }

    fn description(&self) -> &'static str {
        "Name a durable browser pairing"
    }
}

impl MutableCommand for WorkbenchPairingRename {
    fn execute_mut(&self, ctx: &mut MutableCommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(
            services.rename_pairing(&self.selector, &self.nickname)?,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct WorkbenchInspect {
    id: String,
}

impl WorkbenchInspect {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Command for WorkbenchInspect {
    fn namespace(&self) -> &'static str {
        "workbench"
    }

    fn operation(&self) -> &'static str {
        "inspect"
    }

    fn execute(&self, ctx: &CommandContext<'_>) -> ExoResult<CommandOutput> {
        let services = ctx
            .runtime_services
            .ok_or_else(|| anyhow::Error::new(daemon_required_failure()))?;
        Ok(CommandOutput::data(services.inspect(ctx.root, &self.id)?))
    }

    fn description(&self) -> &'static str {
        "Inspect a workbench lane without focusing it"
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
            "Open the Exo workbench for {}:\n{}\n\nThis one-time browser enrollment link expires in one hour.",
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
        for operation in ["launch", "snapshot", "inspect", "pairing.list"] {
            let operation = spec
                .operation("workbench", operation)
                .expect("registered workbench operation");
            assert_eq!(operation.effect, Effect::Pure);
            assert_eq!(operation.recovery_class, RecoveryClass::ReplayableRead);
        }
    }

    #[test]
    fn pairing_mutations_are_external_at_most_once_writes() {
        let spec = CommandSpec::from_registry(&default_registry());
        for operation in ["pairing.revoke", "pairing.forget", "pairing.rename"] {
            let operation = spec
                .operation("workbench", operation)
                .expect("registered pairing operation");
            assert_eq!(operation.effect, Effect::Write);
            assert_eq!(operation.recovery_class, RecoveryClass::ExternalAtMostOnce);
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
            WorkbenchInspect::new("lane-history")
                .execute(&direct_context())
                .expect_err("direct inspection must fail"),
            WorkbenchPairingList
                .execute(&direct_context())
                .expect_err("direct pairing list must fail"),
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
