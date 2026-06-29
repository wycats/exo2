//! Command registry for discovering and looking up commands.
//!
//! The registry provides:
//! - Registration of Command implementers
//! - Iteration over all commands for capability tree generation (RFC 0125)
//! - Lookup by namespace + operation
//! - Metadata access for each command

// Allow use statements near their related registrations for organizational clarity
#![allow(clippy::items_after_statements)]

use super::router::Invocation;
use super::traits::{Command, CommandBox};
use crate::api::protocol::Effect;
use crate::context::PhaseKind;
use anyhow::Result as ExoResult;
use std::collections::HashMap;

/// Metadata about a registered command.
///
/// This provides a snapshot of command metadata for capability tree
/// generation and introspection without needing to hold the command.
#[derive(Debug, Clone, Copy)]
pub struct CommandMetadata {
    pub namespace: &'static str,
    pub operation: &'static str,
    pub effect: Effect,
    pub description: &'static str,
    pub needs_upgrade_gate: bool,
}

impl CommandMetadata {
    /// Create metadata from a Command trait object.
    pub fn from_command(cmd: &dyn Command) -> Self {
        Self {
            namespace: cmd.namespace(),
            operation: cmd.operation(),
            effect: cmd.effect(),
            description: cmd.description(),
            needs_upgrade_gate: false,
        }
    }

    /// Returns the fully qualified command name (namespace.operation).
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.namespace, self.operation)
    }
}

/// A registry of available commands.
///
/// The registry stores boxed trait objects and provides lookup by
/// namespace and operation. Commands are stateless, so we can store
/// representative instances for metadata and capability enumeration.
///
/// # Design Notes
///
/// - Commands with parameters (like `JsonRead`) are registered with default
///   instances. The registry is for discovery and metadata, not execution.
/// - For execution, commands are typically constructed from Clap args.
/// - The `Default` implementation registers all Wave 1 commands.
#[derive(Default)]
pub struct CommandRegistry {
    /// Commands indexed by (namespace, operation) for fast lookup.
    commands: HashMap<(&'static str, &'static str), Box<dyn Command>>,
    /// Ordered list of commands for stable iteration.
    order: Vec<(&'static str, &'static str)>,
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("commands", &self.order)
            .field("len", &self.commands.len())
            .finish()
    }
}

impl CommandRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Register a command.
    ///
    /// If a command with the same namespace and operation already exists,
    /// it will be replaced.
    pub fn register(&mut self, cmd: Box<dyn Command>) {
        let key = (cmd.namespace(), cmd.operation());

        // Only add to order if this is a new command
        if !self.commands.contains_key(&key) {
            self.order.push(key);
        }

        self.commands.insert(key, cmd);
    }

    /// Look up a command by namespace and operation.
    pub fn find(&self, namespace: &str, operation: &str) -> Option<&dyn Command> {
        // We need to iterate because the lookup keys are &'static str
        // but we're given &str. HashMap lookup requires exact key types.
        self.commands
            .iter()
            .find(|((ns, op), _)| *ns == namespace && *op == operation)
            .map(|(_, cmd)| cmd.as_ref())
    }

    /// Iterate over all registered commands in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Command> {
        self.order
            .iter()
            .filter_map(|key| self.commands.get(key).map(std::convert::AsRef::as_ref))
    }

    /// Get the number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Get metadata for all registered commands.
    pub fn metadata(&self) -> Vec<CommandMetadata> {
        self.iter().map(CommandMetadata::from_command).collect()
    }

    /// Find a command and return its metadata.
    pub fn find_metadata(&self, namespace: &str, operation: &str) -> Option<CommandMetadata> {
        self.find(namespace, operation)
            .map(CommandMetadata::from_command)
    }

    /// Get all commands in a namespace.
    pub fn commands_in_namespace(&self, namespace: &str) -> Vec<&dyn Command> {
        self.iter()
            .filter(|cmd| cmd.namespace() == namespace)
            .collect()
    }

    /// Get all unique namespaces.
    pub fn namespaces(&self) -> Vec<&'static str> {
        let mut namespaces: Vec<&'static str> = self
            .order
            .iter()
            .map(|(ns, _)| *ns)
            .filter(|ns| !ns.is_empty())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        namespaces.sort_unstable();
        namespaces
    }
}

/// Create a registry with all Wave 1 and Wave 2 commands.
///
/// Wave 1 commands are stateless or have sensible defaults for metadata:
/// - `epoch list`: List epochs (Pure)
/// - `epoch review`: Review epoch (Write) - registered with placeholder `epoch_id`
/// - `json read`: Read JSON (Pure) - registered with empty path
/// - `json write`: Write JSON (Write) - registered with empty path
/// - `json schema`: Generate schema (Pure)
/// - `toml read`: Read TOML (Pure) - registered with empty path
/// - `toml write`: Write TOML (Write) - registered with empty path
/// - `ai prompt`: AI prompt (Pure)
///
/// Wave 2 simple commands:
/// - `axiom add`: Add axiom (Write) - registered with placeholder values
/// - `axiom list`: List axioms (Pure)
/// - `axiom remove`: Remove axiom (Write) - registered with placeholder values
/// - `idea add`: Add idea (Write) - registered with placeholder values
/// - `idea list`: List ideas (Pure)
/// - `context restore`: Restore context (Pure)
/// - `context paths`: Dump context paths (Pure)
///
/// Wave 2 medium commands:
/// - `inbox list`: List inbox items (Pure)
/// - `inbox add`: Add inbox item (Write)
/// - `inbox ack`: Acknowledge inbox item (Write)
/// - `inbox resolve`: Resolve inbox item (Write)
/// - `strike start`: Start surgical strike (Exec with upgrade gate)
/// - `strike finish`: Finish surgical strike (Exec)
/// - `strike abort`: Abort surgical strike (Exec)
pub fn default_registry() -> CommandRegistry {
    use super::ai::{AiChatHistory, AiContext, AiPrompt};
    use super::axiom::{AxiomAdd, AxiomList, AxiomRemove};
    use super::docs::{DocsLinksCheckCommand, DocsLinksFixCommand};
    use super::dogfood::{DogfoodReceipt, DogfoodRepair, DogfoodRestartRuntimes, DogfoodVerify};
    use super::epoch::{
        EpochAdd, EpochBankrupt, EpochFinish, EpochList, EpochRemove, EpochReorder, EpochReview,
        EpochStart, EpochStatus, EpochUpdate,
    };
    use super::idea::{IdeaAdd, IdeaArchive, IdeaList, IdeaToRfc};
    use super::json::{
        JsonArtifact, JsonLmTools, JsonPackageTools, JsonRead, JsonSchema, JsonSpec, JsonWrite,
    };
    use super::project::{
        ProjectList, ProjectMoveRoot, ProjectRepair, ProjectResolve, ProjectSnapshot,
    };
    use super::root::{MapCommand, StatusCommand};
    use super::sidecar::{
        SidecarBootstrap, SidecarInit, SidecarLink, SidecarRepo, SidecarStatus, SidecarUnlink,
    };
    use super::storage::StorageMaintain;
    use super::toml::{TomlRead, TomlWrite};
    use super::update::UpdateCommand;
    use super::write::Write;
    use crate::axiom::AxiomScope;
    use std::path::PathBuf;

    let mut registry = CommandRegistry::new();

    // Root commands
    registry.register(Box::new(StatusCommand::new()));
    registry.register(Box::new(MapCommand::new(false, None)));
    registry.register(Box::new(Write::new("placeholder", false)));
    registry.register(Box::new(UpdateCommand::new()));

    // Docs namespace
    registry.register(Box::new(DocsLinksCheckCommand::default()));
    registry.register(Box::new(DocsLinksFixCommand::default()));

    // Dogfood namespace
    registry.register(Box::new(DogfoodVerify::default()));
    registry.register(Box::new(DogfoodRepair::default()));
    registry.register(Box::new(DogfoodRestartRuntimes));
    registry.register(Box::new(DogfoodReceipt::default()));

    // Epoch namespace
    registry.register(Box::new(EpochList));
    registry.register(Box::new(EpochStatus::new(None)));
    registry.register(Box::new(EpochStart::new("placeholder")));
    registry.register(Box::new(EpochFinish));
    registry.register(Box::new(EpochReview::new("placeholder")));
    registry.register(Box::new(EpochAdd::new("placeholder", None)));
    registry.register(Box::new(EpochUpdate::new("placeholder", "placeholder")));
    registry.register(Box::new(EpochReorder::new("placeholder", "bottom")));
    registry.register(Box::new(EpochRemove::new("placeholder")));
    registry.register(Box::new(EpochBankrupt::new("placeholder")));

    // JSON namespace
    registry.register(Box::new(JsonRead::new(PathBuf::new(), None)));
    registry.register(Box::new(JsonWrite::new(PathBuf::new(), "", "")));
    registry.register(Box::new(JsonSchema::full()));
    registry.register(Box::new(JsonSpec));
    registry.register(Box::new(JsonArtifact::new(None)));
    registry.register(Box::new(JsonLmTools::new(None)));
    registry.register(Box::new(JsonPackageTools));

    // TOML namespace
    registry.register(Box::new(TomlRead::new(PathBuf::new(), None)));
    registry.register(Box::new(TomlWrite::new(PathBuf::new(), "", "")));

    // AI namespace
    registry.register(Box::new(AiContext::new(None, false)));
    registry.register(Box::new(AiPrompt::new("")));
    registry.register(Box::new(AiChatHistory::new(
        10, None, None, false, false, false,
    )));

    // Axiom namespace (Wave 2)
    registry.register(Box::new(AxiomAdd::new(AxiomScope::Workflow, "placeholder")));
    registry.register(Box::new(AxiomList::new(AxiomScope::Workflow)));
    registry.register(Box::new(AxiomRemove::new(
        AxiomScope::Workflow,
        "placeholder",
    )));

    // Idea namespace (Wave 2)
    registry.register(Box::new(IdeaAdd::new("placeholder", "", vec![])));
    registry.register(Box::new(IdeaList::new(None, None)));
    registry.register(Box::new(IdeaArchive::new("placeholder")));
    registry.register(Box::new(IdeaToRfc::new("placeholder", None)));

    // Context namespace (Wave 2)
    use super::context::{ContextPaths, ContextRestore};
    registry.register(Box::new(ContextRestore::new()));
    registry.register(Box::new(ContextPaths::new()));

    // Inbox namespace (Wave 2 medium)
    use super::inbox::{InboxAck, InboxAdd, InboxArchive, InboxList, InboxResolve};
    registry.register(Box::new(InboxList::new(None, None, None, None, None)));
    registry.register(Box::new(InboxAdd::new(
        "placeholder",
        "project",
        None,
        "user-feedback",
        "fyi",
        "next-touch",
        None,
        "",
        None,
    )));
    registry.register(Box::new(InboxAck::new("placeholder")));
    registry.register(Box::new(InboxResolve::new(
        "placeholder",
        Some("Resolved".to_string()),
        None,
    )));
    registry.register(Box::new(InboxArchive::new("placeholder")));

    // GC namespace (Wave 2 medium)
    use super::gc::GcInbox;
    registry.register(Box::new(GcInbox::new(30)));

    // Strike namespace (Wave 2 medium)
    use super::strike::{StrikeAbort, StrikeFinish, StrikeStart};
    registry.register(Box::new(StrikeStart::new("placeholder", "")));
    registry.register(Box::new(StrikeFinish::new()));
    registry.register(Box::new(StrikeAbort::new()));

    // Task namespace (Wave 3)
    use super::task::{
        TaskAdd, TaskComplete, TaskList, TaskLog, TaskRemove, TaskRename, TaskReorder, TaskStart,
        TaskUpdate,
    };
    registry.register(Box::new(TaskAdd::new("placeholder", "placeholder")));
    registry.register(Box::new(TaskList::new()));
    registry.register(Box::new(TaskStart::new("placeholder")));
    registry.register(Box::new(TaskComplete::new("placeholder", "placeholder")));
    registry.register(Box::new(TaskLog::new("placeholder", "placeholder")));
    registry.register(Box::new(TaskRemove::new("placeholder")));
    registry.register(Box::new(TaskReorder::new("placeholder", "0")));
    registry.register(Box::new(TaskRename::new("placeholder", "renamed")));
    registry.register(Box::new(TaskUpdate::new("placeholder", "placeholder")));

    // Goal namespace (Wave 3.5 - transitional)
    use super::goal::{
        GoalAbandon, GoalAdd, GoalComplete, GoalList, GoalMove, GoalRemove, GoalReorder, GoalUpdate,
    };
    registry.register(Box::new(GoalList::new()));
    registry.register(Box::new(GoalAdd::new(
        "placeholder",
        "placeholder",
        None,
        None,
    )));
    registry.register(Box::new(GoalReorder::new("placeholder", "0")));
    registry.register(Box::new(GoalMove::new("placeholder", "placeholder", None)));
    registry.register(Box::new(GoalComplete::new("placeholder", "placeholder")));
    registry.register(Box::new(GoalAbandon::new("placeholder", "placeholder")));
    registry.register(Box::new(GoalRemove::new("placeholder")));
    registry.register(Box::new(GoalUpdate::new("placeholder", "placeholder")));

    // Run namespace (Wave 3)
    use super::run::{RunTaskCommand, RunTasksCommand};
    registry.register(Box::new(RunTasksCommand::new(None, None)));
    registry.register(Box::new(RunTaskCommand::new("placeholder".to_string())));

    // RFC namespace (Wave 4)
    use super::rfc::{
        RfcArchive, RfcCreate, RfcEdit, RfcList, RfcPipeline, RfcPromote, RfcRename, RfcRepair,
        RfcShow, RfcStatus, RfcSupersede, RfcWithdraw,
    };
    registry.register(Box::new(RfcList::new(None)));
    registry.register(Box::new(RfcShow::new("placeholder")));
    registry.register(Box::new(RfcStatus::new()));
    registry.register(Box::new(RfcPipeline));
    registry.register(Box::new(RfcCreate::new(
        "placeholder",
        None,
        "General",
        0,
        None,
        false,
    )));
    registry.register(Box::new(RfcEdit::new(
        Some("placeholder".to_string()),
        None,
        None,
        None,
        None,
        None,
    )));
    registry.register(Box::new(RfcRename::new("placeholder")));
    registry.register(Box::new(RfcRepair::new("placeholder")));
    registry.register(Box::new(RfcPromote::new("placeholder", 1)));
    registry.register(Box::new(RfcSupersede::new(
        Some("placeholder".to_string()),
        "placeholder",
        None,
    )));
    registry.register(Box::new(RfcWithdraw::new(
        "placeholder",
        Some("placeholder".to_string()),
    )));
    registry.register(Box::new(RfcArchive::new(
        "placeholder",
        Some("placeholder".to_string()),
    )));

    // Plan namespace (Wave 5)
    use super::plan::{PlanMoveGoals, PlanRead, PlanReview, PlanSnapshot, PlanUpdateStatus};
    registry.register(Box::new(PlanReview::new(false)));
    registry.register(Box::new(PlanSnapshot));
    registry.register(Box::new(PlanRead));
    registry.register(Box::new(PlanUpdateStatus::new(
        "placeholder",
        "placeholder",
    )));
    registry.register(Box::new(PlanMoveGoals::new(
        "placeholder",
        "placeholder",
        vec!["placeholder".to_string()],
    )));

    // Project namespace
    registry.register(Box::new(ProjectResolve::new()));
    registry.register(Box::new(ProjectList::new()));
    registry.register(Box::new(ProjectSnapshot::new("project-id")));
    registry.register(Box::new(ProjectRepair::new(true, false)));
    registry.register(Box::new(ProjectRepair::new(true, true)));
    registry.register(Box::new(ProjectMoveRoot::new(
        "sidecar-key",
        PathBuf::from("workspace"),
        true,
    )));

    // Sidecar namespace
    registry.register(Box::new(SidecarBootstrap::new(
        None, None, false, false, None, false,
    )));
    registry.register(Box::new(SidecarInit::new(None, None, false)));
    registry.register(Box::new(SidecarLink::new("placeholder", PathBuf::new())));
    registry.register(Box::new(SidecarStatus::new(None)));
    registry.register(Box::new(SidecarRepo::new(
        "status", None, None, None, None, false,
    )));
    registry.register(Box::new(SidecarUnlink::new()));

    // Storage namespace
    registry.register(Box::new(StorageMaintain::new(
        false,
        exosuit_storage::DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET,
    )));

    // Phase namespace (Wave 7)
    use super::phase_cmd::{
        PhaseAdd, PhaseExecutionTasks, PhaseFinish, PhaseFocus, PhaseHistory, PhaseList, PhaseMove,
        PhaseReadDetails, PhaseReadGoals, PhaseReadTasks, PhaseRelease, PhaseRemove, PhaseReorder,
        PhaseStart, PhaseStatus, PhaseUpdate,
    };
    registry.register(Box::new(PhaseStart::new(None, false)));
    registry.register(Box::new(PhaseFocus::new("placeholder")));
    registry.register(Box::new(PhaseRelease::new("placeholder")));
    registry.register(Box::new(PhaseStatus::new(false)));
    registry.register(Box::new(PhaseFinish::new(None)));
    registry.register(Box::new(PhaseHistory::new(None)));
    registry.register(Box::new(PhaseExecutionTasks::new(None, None)));
    registry.register(Box::new(PhaseReadGoals::new(None)));
    registry.register(Box::new(PhaseReadTasks::new(None)));
    registry.register(Box::new(PhaseReadDetails::new(None)));
    registry.register(Box::new(PhaseList::new(None)));
    registry.register(Box::new(PhaseUpdate::new(
        "placeholder".to_string(),
        None,
        None,
    )));
    registry.register(Box::new(PhaseReorder::new("placeholder", "bottom")));
    registry.register(Box::new(PhaseMove::new("placeholder", "placeholder", None)));
    registry.register(Box::new(PhaseAdd::new(
        "placeholder",
        None,
        None,
        false,
        None,
        PhaseKind::default(),
    )));
    registry.register(Box::new(PhaseRemove::new("placeholder")));

    // Verify namespace (Wave 8)
    use super::verify::VerifyRun;
    registry.register(Box::new(VerifyRun::new()));

    // Commit namespace (Wave 9 - Phase D)
    use super::commit::{Commit, CommitStatus};
    registry.register(Box::new(CommitStatus::new()));
    registry.register(Box::new(Commit::new("placeholder")));

    registry
}

/// Construct a command from an invocation using ExoSpec-generated dispatch.
///
/// This function uses the `from_invocation()` + `to_command_box()` pattern
/// generated by the `#[derive(ExoSpec)]` macro on each namespace enum.
/// The macro generates type-safe argument extraction and the `to_command_box()`
/// method converts the parsed enum variant into a dispatchable `CommandBox`.
pub fn build_command_from_invocation(
    inv: &Invocation,
    root: &std::path::Path,
) -> ExoResult<Option<CommandBox>> {
    // Dispatch based on namespace, using ExoSpec-generated from_invocation()
    match inv.namespace() {
        "" => Ok(Some(
            super::root::RootCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "ai" => Ok(Some(
            super::ai::AiCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "axiom" => Ok(Some(
            super::axiom::AxiomCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "commit" => Ok(Some(
            super::commit::CommitCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "context" => Ok(Some(
            super::context::ContextCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "docs" => Ok(Some(
            super::docs::DocsCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "dogfood" => Ok(Some(
            super::dogfood::DogfoodCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "epoch" => Ok(Some(
            super::epoch::EpochCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "gc" => Ok(Some(
            super::gc::GcCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "goal" => Ok(Some(
            super::goal::GoalCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "idea" => Ok(Some(
            super::idea::IdeaCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "inbox" => Ok(Some(
            super::inbox::InboxCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "json" => Ok(Some(
            super::json::JsonCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "phase" => Ok(Some(
            super::phase_cmd::PhaseCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "plan" => Ok(Some(
            super::plan::PlanCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "project" => Ok(Some(
            super::project::ProjectCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "sidecar" => Ok(Some(
            super::sidecar::SidecarCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "rfc" => Ok(Some(
            super::rfc::RfcCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "run" => Ok(Some(
            super::run::RunCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "strike" => Ok(Some(
            super::strike::StrikeCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "storage" => Ok(Some(
            super::storage::StorageCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "task" => Ok(Some(
            super::task::TaskCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "toml" => Ok(Some(
            super::toml::TomlCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        "verify" => Ok(Some(
            super::verify::VerifyCommands::from_invocation(inv)?.to_command_box(root)?,
        )),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::traits::{CommandContext, CommandOutput};
    use anyhow::Result as ExoResult;

    /// A simple test command for testing registration.
    struct TestCommand {
        ns: &'static str,
        op: &'static str,
    }

    impl TestCommand {
        fn new(ns: &'static str, op: &'static str) -> Self {
            Self { ns, op }
        }
    }

    impl Command for TestCommand {
        fn namespace(&self) -> &'static str {
            self.ns
        }

        fn operation(&self) -> &'static str {
            self.op
        }

        fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
            Ok(CommandOutput::message("test"))
        }

        fn description(&self) -> &'static str {
            "Test command"
        }
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = CommandRegistry::new();
        assert!(registry.is_empty());

        registry.register(Box::new(TestCommand::new("test", "one")));
        assert_eq!(registry.len(), 1);

        registry.register(Box::new(TestCommand::new("test", "two")));
        assert_eq!(registry.len(), 2);

        // Duplicate registration replaces
        registry.register(Box::new(TestCommand::new("test", "one")));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_registry_iteration() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand::new("alpha", "first")));
        registry.register(Box::new(TestCommand::new("beta", "second")));
        registry.register(Box::new(TestCommand::new("alpha", "third")));

        let commands: Vec<_> = registry.iter().collect();
        assert_eq!(commands.len(), 3);

        // Verify iteration order matches registration order
        assert_eq!(commands[0].namespace(), "alpha");
        assert_eq!(commands[0].operation(), "first");
        assert_eq!(commands[1].namespace(), "beta");
        assert_eq!(commands[1].operation(), "second");
        assert_eq!(commands[2].namespace(), "alpha");
        assert_eq!(commands[2].operation(), "third");
    }

    #[test]
    fn test_registry_find() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand::new("foo", "bar")));
        registry.register(Box::new(TestCommand::new("foo", "baz")));
        registry.register(Box::new(TestCommand::new("qux", "bar")));

        // Find existing commands
        let cmd = registry.find("foo", "bar");
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().namespace(), "foo");
        assert_eq!(cmd.unwrap().operation(), "bar");

        let cmd = registry.find("qux", "bar");
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().namespace(), "qux");

        // Non-existent lookup
        assert!(registry.find("foo", "missing").is_none());
        assert!(registry.find("missing", "bar").is_none());
    }

    #[test]
    fn test_registry_metadata() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand::new("ns", "op")));

        let meta = registry.find_metadata("ns", "op");
        assert!(meta.is_some());

        let meta = meta.unwrap();
        assert_eq!(meta.namespace, "ns");
        assert_eq!(meta.operation, "op");
        assert_eq!(meta.full_name(), "ns.op");
        assert_eq!(meta.description, "Test command");
    }

    #[test]
    fn test_registry_namespaces() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand::new("beta", "one")));
        registry.register(Box::new(TestCommand::new("alpha", "two")));
        registry.register(Box::new(TestCommand::new("beta", "three")));

        let namespaces = registry.namespaces();
        assert_eq!(namespaces.len(), 2);
        // Should be sorted
        assert_eq!(namespaces[0], "alpha");
        assert_eq!(namespaces[1], "beta");
    }

    #[test]
    fn test_registry_commands_in_namespace() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCommand::new("ns1", "a")));
        registry.register(Box::new(TestCommand::new("ns2", "b")));
        registry.register(Box::new(TestCommand::new("ns1", "c")));

        let ns1_commands = registry.commands_in_namespace("ns1");
        assert_eq!(ns1_commands.len(), 2);

        let ns2_commands = registry.commands_in_namespace("ns2");
        assert_eq!(ns2_commands.len(), 1);

        let ns3_commands = registry.commands_in_namespace("ns3");
        assert_eq!(ns3_commands.len(), 0);
    }

    #[test]
    fn test_default_registry_contains_wave1_commands() {
        let registry = default_registry();

        // Should have all Wave 1 commands
        assert!(!registry.is_empty());

        // Check root namespace
        assert!(registry.find("", "status").is_some());
        assert!(registry.find("", "map").is_some());
        assert!(registry.find("", "write").is_some());
        assert!(registry.find("", "update").is_some());

        // Check dogfood namespace
        assert!(registry.find("dogfood", "verify").is_some());
        assert!(registry.find("dogfood", "repair").is_some());
        assert!(registry.find("dogfood", "restart").is_some());
        assert!(registry.find("dogfood", "receipt").is_some());

        // Check epoch namespace
        assert!(registry.find("epoch", "list").is_some());
        assert!(registry.find("epoch", "review").is_some());

        // Check json namespace
        assert!(registry.find("json", "read").is_some());
        assert!(registry.find("json", "write").is_some());
        assert!(registry.find("json", "schema").is_some());
        assert!(registry.find("json", "spec").is_some());

        // Check toml namespace
        assert!(registry.find("toml", "read").is_some());
        assert!(registry.find("toml", "write").is_some());

        // Check ai namespace
        assert!(registry.find("ai", "prompt").is_some());

        // Check axiom namespace (Wave 2)
        assert!(registry.find("axiom", "add").is_some());
        assert!(registry.find("axiom", "list").is_some());
        assert!(registry.find("axiom", "remove").is_some());

        // Check idea namespace (Wave 2)
        assert!(registry.find("idea", "add").is_some());
        assert!(registry.find("idea", "list").is_some());

        // Check context namespace (Wave 2)
        assert!(registry.find("context", "restore").is_some());
        assert!(registry.find("context", "paths").is_some());

        // Check inbox namespace (Wave 2 medium)
        assert!(registry.find("inbox", "list").is_some());
        assert!(registry.find("inbox", "add").is_some());
        assert!(registry.find("inbox", "ack").is_some());
        assert!(registry.find("inbox", "resolve").is_some());
        assert!(registry.find("inbox", "archive").is_some());

        // Check gc namespace (Wave 2 medium)
        assert!(registry.find("gc", "inbox").is_some());

        // Check strike namespace (Wave 2 medium)
        assert!(registry.find("strike", "start").is_some());
        assert!(registry.find("strike", "finish").is_some());
        assert!(registry.find("strike", "abort").is_some());

        // Check task namespace (Wave 3)
        assert!(registry.find("task", "add").is_some());
        assert!(registry.find("task", "list").is_some());
        assert!(registry.find("task", "start").is_some());
        assert!(registry.find("task", "complete").is_some());
        assert!(registry.find("task", "remove").is_some());
        assert!(registry.find("task", "reorder").is_some());
        assert!(registry.find("task", "rename").is_some());
        assert!(registry.find("task", "update").is_some());

        // Check RFC namespace (Wave 4)
        assert!(registry.find("rfc", "list").is_some());
        assert!(registry.find("rfc", "show").is_some());
        assert!(registry.find("rfc", "status").is_some());
        assert!(registry.find("rfc", "create").is_some());
        assert!(registry.find("rfc", "edit").is_some());
        assert!(registry.find("rfc", "rename").is_some());
        assert!(registry.find("rfc", "repair").is_some());
        assert!(registry.find("rfc", "promote").is_some());
        assert!(registry.find("rfc", "supersede").is_some());
        assert!(registry.find("rfc", "withdraw").is_some());

        // Check plan namespace (Wave 5)
        assert!(registry.find("plan", "review").is_some());
        assert!(registry.find("epoch", "add").is_some());
        assert!(registry.find("epoch", "update").is_some());
        assert!(registry.find("epoch", "reorder").is_some());
        assert!(registry.find("epoch", "remove").is_some());
        assert!(registry.find("plan", "update-status").is_some());
        assert!(registry.find("epoch", "bankrupt").is_some());

        // Check project namespace
        assert!(registry.find("project", "resolve").is_some());
        assert!(registry.find("project", "list").is_some());
        assert!(registry.find("project", "snapshot").is_some());
        assert!(registry.find("project", "repair").is_some());
        assert!(registry.find("project", "repair-apply").is_some());
        assert!(registry.find("project", "move-root").is_some());

        // Check sidecar namespace
        assert!(registry.find("sidecar", "bootstrap").is_some());
        assert!(registry.find("sidecar", "init").is_some());
        assert!(registry.find("sidecar", "link").is_some());
        assert!(registry.find("sidecar", "status").is_some());
        assert!(registry.find("sidecar", "repo").is_some());
        assert!(registry.find("sidecar", "unlink").is_some());

        // Check storage namespace
        assert!(registry.find("storage", "maintain").is_some());

        // Check goal namespace (Wave 3.5)
        assert!(registry.find("goal", "add").is_some());
        assert!(registry.find("goal", "list").is_some());
        assert!(registry.find("goal", "move").is_some());
        assert!(registry.find("goal", "complete").is_some());
        assert!(registry.find("goal", "abandon").is_some());

        // Check phase namespace (Wave 7)
        assert!(registry.find("phase", "start").is_some());
        assert!(registry.find("phase", "focus").is_some());
        assert!(registry.find("phase", "release").is_some());
        assert!(registry.find("phase", "status").is_some());
        assert!(registry.find("phase", "finish").is_some());
        assert!(registry.find("phase", "update").is_some());
        assert!(registry.find("phase", "reorder").is_some());
        assert!(registry.find("phase", "move").is_some());
        assert!(registry.find("phase", "add").is_some());
        assert!(registry.find("phase", "list").is_some());
        assert!(registry.find("phase", "remove").is_some());

        // Check verify namespace (Wave 8)
        assert!(registry.find("verify", "run").is_some());

        // Verify total count: registry.len() gives actual count - update as commands are added
        assert_eq!(registry.len(), 118);
    }

    #[test]
    fn test_default_registry_command_effects() {
        let registry = default_registry();

        // Pure commands
        let epoch_list = registry.find_metadata("epoch", "list").unwrap();
        assert_eq!(epoch_list.effect, Effect::Pure);

        let json_read = registry.find_metadata("json", "read").unwrap();
        assert_eq!(json_read.effect, Effect::Pure);

        // Write commands
        let epoch_review = registry.find_metadata("epoch", "review").unwrap();
        assert_eq!(epoch_review.effect, Effect::Write);

        let json_write = registry.find_metadata("json", "write").unwrap();
        assert_eq!(json_write.effect, Effect::Write);

        // Exec commands
        let strike_start = registry.find_metadata("strike", "start").unwrap();
        assert_eq!(strike_start.effect, Effect::Exec);

        let strike_finish = registry.find_metadata("strike", "finish").unwrap();
        assert_eq!(strike_finish.effect, Effect::Exec);
    }
}
