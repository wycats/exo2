//! Command trait architecture per RFC 0085.
//!
//! This module provides the trait-based command abstraction that enables:
//! - Format-agnostic command execution (JSON/Human output)
//! - Centralized error handling with steering suggestions
//! - Capability tree generation for RFC 0125

use crate::api::protocol::{Effect, ErrorCode, RecoveryClass, WorkflowConfirmationInput};
use crate::command::command_spec::CommandSpec;
use crate::command::registry::{build_command_from_invocation, default_registry};
use crate::command::router::Invocation;
use crate::command::transport::{
    CommandError, ConfirmResult, SteeringOutput, TransportContext, TransportOutput,
};
use crate::command::unified_diagnostics::IntoDiagnosticSteering;
use crate::failure::ExoFailure;
use crate::project::Project;
use crate::steering::SuggestedAction;
use anyhow::Result as ExoResult;
use exosuit_storage::TraceScope;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Output format for command results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

/// Context for read-only command execution.
#[derive(Debug)]
pub struct CommandContext<'a> {
    pub root: &'a Path,
    pub project: Option<&'a Project>,
    pub format: OutputFormat,
    /// Agent session identity from request envelope (None = CLI/sidebar).
    pub agent_id: Option<String>,
    pub workflow_confirmation: Option<WorkflowConfirmationInput>,
}

/// Context for mutable command execution.
#[derive(Debug)]
pub struct MutableCommandContext<'a> {
    pub root: &'a Path,
    pub project: Option<&'a Project>,
    pub format: OutputFormat,
    /// Agent session identity from request envelope (None = CLI/sidebar).
    pub agent_id: Option<String>,
    pub workflow_confirmation: Option<WorkflowConfirmationInput>,
}

impl CommandContext<'_> {
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        crate::context::db_path(self.root, self.project)
    }
}

impl MutableCommandContext<'_> {
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        crate::context::db_path(self.root, self.project)
    }
}

/// Structured output from command execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Structured data for JSON output.
    pub data: serde_json::Value,
    /// Human-readable message (if different from auto-formatting data).
    pub human_message: Option<String>,
}

/// Result of invoking a command through the transport layer.
///
/// Carries both the JSON result and optional display metadata
/// extracted from the command's `human_message`.
#[derive(Debug, Clone)]
pub struct CommandInvokeResult {
    /// The JSON result data.
    pub data: serde_json::Value,
    /// Human-readable message from the command, if available.
    /// Used by the machine channel handler to generate display metadata.
    pub human_message: Option<String>,
    /// The command's declared effect (pure, write, exec).
    pub effect: Effect,
    /// Reactive trace captured during command execution.
    /// Contains `(cell, revision)` tuples recorded by `SQLite` vtab callbacks.
    pub trace: exosuit_storage::Trace,
}

impl CommandOutput {
    /// Create output with just data.
    pub fn data<T: Serialize>(data: T) -> Self {
        Self {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            human_message: None,
        }
    }

    /// Create output with explicit human message.
    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.human_message = Some(msg.into());
        self
    }

    /// Create output with just a message (data is null).
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            data: serde_json::Value::Null,
            human_message: Some(msg.into()),
        }
    }

    /// Create output with both data and message.
    pub fn new<T: Serialize>(data: T, msg: impl Into<String>) -> Self {
        Self {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            human_message: Some(msg.into()),
        }
    }
}

// ============================================================================
// CommandBox - unified dispatch for Pure and Mutable commands
// ============================================================================

/// A boxed command that can be either pure (read-only) or mutable.
///
/// This enum provides a unified dispatch mechanism for commands,
/// handling both `Command` and `MutableCommand` traits.
pub enum CommandBox {
    /// A pure (read-only) command.
    Pure(Box<dyn Command>),
    /// A mutable command that modifies project state.
    Mutable(Box<dyn MutableCommand>),
}

impl std::fmt::Debug for CommandBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pure(cmd) => f
                .debug_tuple("Pure")
                .field(&format!("{}.{}", cmd.namespace(), cmd.operation()))
                .finish(),
            Self::Mutable(cmd) => f
                .debug_tuple("Mutable")
                .field(&format!("{}.{}", cmd.namespace(), cmd.operation()))
                .finish(),
        }
    }
}

impl CommandBox {
    /// Dispatch the command, executing it with the appropriate context.
    pub fn dispatch(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        match self {
            Self::Pure(cmd) => cmd.execute(ctx),
            Self::Mutable(cmd) => {
                let mut mutable_ctx = MutableCommandContext {
                    root: ctx.root,
                    project: ctx.project,
                    format: ctx.format,
                    agent_id: ctx.agent_id.clone(),
                    workflow_confirmation: ctx.workflow_confirmation.clone(),
                };
                cmd.execute_mut(&mut mutable_ctx)
            }
        }
    }

    /// Returns the effect of the underlying command.
    pub fn effect(&self) -> Effect {
        match self {
            Self::Pure(cmd) => cmd.effect(),
            Self::Mutable(cmd) => cmd.effect(),
        }
    }

    /// Returns the daemon recovery class of the underlying command.
    pub fn recovery_class(&self) -> RecoveryClass {
        match self {
            Self::Pure(cmd) => cmd.recovery_class(),
            Self::Mutable(cmd) => cmd.recovery_class(),
        }
    }

    /// Returns the description of the underlying command.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Pure(cmd) => cmd.description(),
            Self::Mutable(cmd) => cmd.description(),
        }
    }

    /// Returns the default steering suggestions for the underlying command.
    pub fn default_steering(&self) -> Vec<SuggestedAction> {
        match self {
            Self::Pure(cmd) => cmd.default_steering(),
            Self::Mutable(cmd) => cmd.default_steering(),
        }
    }

    /// Wrap a pure command.
    pub fn pure<C: Command + 'static>(cmd: C) -> Self {
        Self::Pure(Box::new(cmd))
    }

    /// Wrap a mutable command.
    pub fn mutable<C: MutableCommand + 'static>(cmd: C) -> Self {
        Self::Mutable(Box::new(cmd))
    }
}

fn command_spec_from_registry() -> CommandSpec {
    let registry = default_registry();
    CommandSpec::from_registry(&registry)
}

fn transport_output_to_json(output: TransportOutput) -> serde_json::Value {
    match output {
        TransportOutput::Json(value) => value,
        TransportOutput::Text(text) => serde_json::json!({ "text": text }),
        TransportOutput::Bytes(bytes) => serde_json::json!({ "bytes": bytes }),
    }
}

fn steering_output_to_json(output: SteeringOutput) -> serde_json::Value {
    match output {
        SteeringOutput::Json(value) => value,
        SteeringOutput::Text(text) => serde_json::json!({ "text": text }),
        SteeringOutput::Bytes(bytes) => serde_json::json!({ "bytes": bytes }),
    }
}

fn format_error_response(
    transport: &dyn TransportContext,
    error: CommandError,
    steering: Vec<SuggestedAction>,
) -> serde_json::Value {
    let mut response = transport_output_to_json(transport.format_error(error));
    if !steering.is_empty()
        && let Some(obj) = response.as_object_mut()
    {
        obj.insert(
            "steering".to_string(),
            steering_output_to_json(transport.render_steering(steering)),
        );
    }
    response
}

fn find_exo_failure<'a>(mut e: &'a (dyn std::error::Error + 'static)) -> Option<&'a ExoFailure> {
    loop {
        if let Some(f) = e.downcast_ref::<ExoFailure>() {
            return Some(f);
        }
        e = e.source()?;
    }
}

fn is_cli_json_completion_confirmation_failure(
    transport: &dyn TransportContext,
    failure: &ExoFailure,
) -> bool {
    if transport.output_format() != OutputFormat::Json
        || failure.error.code != ErrorCode::PreconditionFailed
    {
        return false;
    }

    failure
        .error
        .details
        .as_ref()
        .is_some_and(|details| details.get("blocked_state").is_some())
}

fn is_completion_confirmation_failure(failure: &ExoFailure) -> bool {
    failure.error.code == ErrorCode::PreconditionFailed
        && failure
            .error
            .details
            .as_ref()
            .is_some_and(|details| details.get("blocked_state").is_some())
}

fn completion_review_human_message(failure: &ExoFailure) -> String {
    let Some(workflow) = failure
        .error
        .details
        .as_ref()
        .and_then(|details| details.get("workflow_confirmation"))
    else {
        return failure.error.message.clone();
    };

    let header = workflow
        .get("header")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Outcome ready for review");
    let question = workflow
        .get("question")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Approve this outcome?");
    let message = workflow
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let proposed = workflow
        .get("proposed_outcome")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let mut lines = vec![
        failure.error.message.clone(),
        String::new(),
        header.to_string(),
    ];
    if !message.trim().is_empty() {
        lines.push(String::new());
        lines.push(message.to_string());
    } else if !proposed.trim().is_empty() {
        lines.push(String::new());
        lines.push("Outcome:".to_string());
        lines.push(proposed.to_string());
    }

    lines.push(String::new());
    lines.push(question.to_string());

    if let Some(options) = workflow
        .get("options")
        .and_then(serde_json::Value::as_array)
        && !options.is_empty()
    {
        lines.push(String::new());
        lines.push("Choices:".to_string());
        for option in options {
            let Some(label) = option.get("label").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let description = option
                .get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if description.is_empty() {
                lines.push(format!("- {label}"));
            } else {
                lines.push(format!("- {label}: {description}"));
            }
        }
    }

    lines.join("\n")
}

fn is_cli_json_sidecar_checkpoint_failure(
    transport: &dyn TransportContext,
    failure: &ExoFailure,
) -> bool {
    if transport.output_format() != OutputFormat::Json
        || failure.error.code != ErrorCode::PreconditionFailed
    {
        return false;
    }

    is_sidecar_checkpoint_failure(failure)
}

fn is_cli_json_sidecar_bootstrap_git_required_failure(
    transport: &dyn TransportContext,
    failure: &ExoFailure,
) -> bool {
    if transport.output_format() != OutputFormat::Json
        || failure.error.code != ErrorCode::PreconditionFailed
    {
        return false;
    }

    failure.error.details.as_ref().is_some_and(|details| {
        details
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| kind == "sidecar.bootstrap")
            && details
                .get("requires_git_repo")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    })
}

fn is_sidecar_checkpoint_failure(failure: &ExoFailure) -> bool {
    failure.error.details.as_ref().is_some_and(|details| {
        details
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| kind == "sidecar.local_checkpoint")
    })
}

fn merge_error_details(
    details: Option<serde_json::Value>,
    steering: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (details, steering) {
        (None, None) => None,
        (Some(details), None) => Some(details),
        (None, Some(steering)) => Some(serde_json::json!({ "steering": steering })),
        (Some(details), Some(steering)) => Some(serde_json::json!({
            "details": details,
            "steering": steering,
        })),
    }
}

/// Execute an already-built `CommandBox` directly via the transport abstraction.
///
/// This is the execution path for CLI when the command has already been built.
/// Unlike `invoke_json` on a Command trait, this doesn't rebuild the command,
/// which is important when stdin has already been consumed during construction.
///
/// Returns `CommandInvokeResult` which includes both the JSON data and
/// the human-readable message (if the command produced one). The handler
/// uses the human message to generate display metadata for UI rendering.
pub fn invoke_command_box_json(
    cmd: &CommandBox,
    transport: &dyn TransportContext,
) -> Result<CommandInvokeResult, serde_json::Value> {
    if cmd.effect() == Effect::Exec {
        let action = cmd.description();

        match transport.confirm_exec(action) {
            ConfirmResult::Proceed => {}
            ConfirmResult::NeedConfirm { ticket } => {
                return Err(serde_json::json!({
                    "status": "confirm_required",
                    "ticket": ticket,
                    "action": action
                }));
            }
            ConfirmResult::Denied(reason) => {
                let error = CommandError::with_code(ErrorCode::ConfirmRequired, reason);
                return Err(format_error_response(
                    transport,
                    error,
                    cmd.default_steering(),
                ));
            }
        }
    }

    let root = match transport.workspace_root() {
        Some(root) => root.to_path_buf(),
        None => match std::env::current_dir() {
            Ok(root) => root,
            Err(err) => {
                let error = CommandError::with_code(
                    ErrorCode::Internal,
                    format!("Failed to determine current directory: {err}"),
                );
                return Err(format_error_response(
                    transport,
                    error,
                    cmd.default_steering(),
                ));
            }
        },
    };

    let ctx = CommandContext {
        root: &root,
        project: transport.project(),
        format: transport.output_format(),
        agent_id: transport.agent_id().map(String::from),
        workflow_confirmation: transport.workflow_confirmation().cloned(),
    };

    let effect = cmd.effect();
    let (dispatch_result, trace) = TraceScope::run(|| cmd.dispatch(&ctx));
    let output = match dispatch_result {
        Ok(output) => output,
        Err(err) => {
            if let Some(failure) = find_exo_failure(err.as_ref()) {
                let error_code = if transport.preserves_error_codes()
                    || is_cli_json_completion_confirmation_failure(transport, failure)
                    || is_cli_json_sidecar_checkpoint_failure(transport, failure)
                    || is_cli_json_sidecar_bootstrap_git_required_failure(transport, failure)
                {
                    failure.error.code
                } else {
                    ErrorCode::Internal
                };
                let message = if transport.output_format() == OutputFormat::Human
                    && is_completion_confirmation_failure(failure)
                {
                    completion_review_human_message(failure)
                } else {
                    failure.error.message.clone()
                };
                let error = CommandError::with_code(error_code, message);
                let steering = if is_sidecar_checkpoint_failure(failure)
                    || is_completion_confirmation_failure(failure)
                    || is_cli_json_sidecar_bootstrap_git_required_failure(transport, failure)
                {
                    failure.steering.next_actions.clone()
                } else {
                    cmd.default_steering()
                };
                let mut response = format_error_response(transport, error, steering);

                if let Some(obj) = response.as_object_mut() {
                    let details = if is_sidecar_checkpoint_failure(failure) {
                        merge_error_details(
                            failure.error.details.clone(),
                            Some(serde_json::json!(failure.steering.clone())),
                        )
                    } else {
                        failure.error.details.clone()
                    };
                    if let Some(details) = details {
                        if let Some(error_obj) =
                            obj.get_mut("error").and_then(|value| value.as_object_mut())
                        {
                            error_obj.insert("details".to_string(), details);
                        } else {
                            obj.insert(
                                "error".to_string(),
                                serde_json::json!({
                                    "code": failure.error.code,
                                    "message": failure.error.message,
                                    "details": details,
                                }),
                            );
                        }
                    }
                }

                return Err(response);
            }

            let error = CommandError::with_code(ErrorCode::Internal, err.to_string());
            return Err(format_error_response(
                transport,
                error,
                cmd.default_steering(),
            ));
        }
    };

    let human_message = output.human_message.clone();
    let data = transport_output_to_json(transport.format_output(output));
    Ok(CommandInvokeResult {
        data,
        human_message,
        effect,
        trace,
    })
}

/// A single CLI operation that can be executed.
///
/// Each subcommand (e.g., `epoch list`, `phase start`) implements this trait.
/// The trait is designed to be:
/// - **Format-agnostic**: Handlers return structured data, not println output
/// - **Testable**: Commands can be constructed and executed without Clap
/// - **Discoverable**: The registry can enumerate all commands for capability tree
pub trait Command: Send + Sync {
    /// The namespace for this command (e.g., "epoch", "phase").
    fn namespace(&self) -> &'static str;

    /// The operation name (e.g., "list", "start").
    fn operation(&self) -> &'static str;

    /// Execute the command, returning structured output.
    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput>;

    /// Execute a command from JSON input via the transport abstraction.
    ///
    /// This is the unified execution path for both CLI and machine channel.
    /// The transport handles confirmation, output formatting, and error rendering.
    fn invoke_json(
        &self,
        input: &serde_json::Value,
        transport: &dyn TransportContext,
    ) -> Result<serde_json::Value, serde_json::Value> {
        let namespace = self.namespace();
        let operation = self.operation();

        let spec = command_spec_from_registry();

        if spec.operation(namespace, operation).is_none() {
            let error = CommandError::with_code(
                ErrorCode::UnknownAddress,
                format!("Unknown command: '{namespace} {operation}'"),
            );
            return Err(format_error_response(
                transport,
                error,
                self.default_steering(),
            ));
        }

        let invocation = match Invocation::from_json(input, namespace, operation, &spec) {
            Ok(invocation) => invocation,
            Err(diagnostic) => {
                let error =
                    CommandError::with_code(ErrorCode::InvalidInput, diagnostic.format_plain());
                return Err(format_error_response(
                    transport,
                    error,
                    self.default_steering(),
                ));
            }
        };

        let root = match transport.workspace_root() {
            Some(root) => root.to_path_buf(),
            None => match std::env::current_dir() {
                Ok(root) => root,
                Err(err) => {
                    let error = CommandError::with_code(
                        ErrorCode::Internal,
                        format!("Failed to determine current directory: {err}"),
                    );
                    return Err(format_error_response(
                        transport,
                        error,
                        self.default_steering(),
                    ));
                }
            },
        };

        match build_command_from_invocation(&invocation, &root) {
            Ok(Some(cmd)) => return invoke_command_box_json(&cmd, transport).map(|r| r.data),
            Ok(None) => {}
            Err(err) => {
                let error = CommandError::with_code(ErrorCode::InvalidInput, err.to_string());
                return Err(format_error_response(
                    transport,
                    error,
                    self.default_steering(),
                ));
            }
        }

        if self.effect() == Effect::Exec {
            let action = self.description();
            match transport.confirm_exec(action) {
                ConfirmResult::Proceed => {}
                ConfirmResult::NeedConfirm { ticket } => {
                    return Err(serde_json::json!({
                        "status": "confirm_required",
                        "ticket": ticket,
                        "action": action
                    }));
                }
                ConfirmResult::Denied(reason) => {
                    let error = CommandError::with_code(ErrorCode::ConfirmRequired, reason);
                    return Err(format_error_response(
                        transport,
                        error,
                        self.default_steering(),
                    ));
                }
            }
        }

        let ctx = CommandContext {
            root: &root,
            project: transport.project(),
            format: transport.output_format(),
            agent_id: transport.agent_id().map(String::from),
            workflow_confirmation: transport.workflow_confirmation().cloned(),
        };

        let output = match self.execute(&ctx) {
            Ok(output) => output,
            Err(err) => {
                let error = CommandError::with_code(ErrorCode::Internal, err.to_string());
                return Err(format_error_response(
                    transport,
                    error,
                    self.default_steering(),
                ));
            }
        };

        Ok(transport_output_to_json(transport.format_output(output)))
    }

    /// The effect classification for this command.
    fn effect(&self) -> Effect {
        Effect::Pure
    }

    /// The recovery boundary used when a daemon disappears mid-request.
    fn recovery_class(&self) -> RecoveryClass {
        recovery_class_for_command(self.namespace(), self.operation(), self.effect())
    }

    /// Human-readable description for help and capability tree.
    fn description(&self) -> &'static str {
        ""
    }

    /// Default steering suggestions for errors.
    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![]
    }
}

/// A command that mutates project state.
///
/// This is a sub-trait of Command for operations that need mutable context.
pub trait MutableCommand: Command {
    /// Execute with mutable access to context.
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput>;
}

/// Derive the recovery contract from the registered command surface.
///
/// The atomic class is deliberately narrow: these commands mutate canonical
/// project SQLite state without owning Git, filesystem, process, or other
/// external effects. The registry regression test keeps this list aligned
/// with the 42-operation contract approved for RFC 10195.
#[must_use]
pub fn recovery_class_for_command(
    namespace: &str,
    operation: &str,
    effect: Effect,
) -> RecoveryClass {
    if effect == Effect::Pure {
        return RecoveryClass::ReplayableRead;
    }

    let atomic_project_state = matches!(
        (namespace, operation),
        ("axiom", "add" | "remove")
            | (
                "epoch",
                "add"
                    | "bankrupt"
                    | "finish"
                    | "remove"
                    | "reorder"
                    | "review"
                    | "start"
                    | "update"
            )
            | (
                "goal",
                "abandon" | "add" | "complete" | "move" | "remove" | "reorder" | "update"
            )
            | ("idea", "add" | "archive")
            | ("inbox", "ack" | "add" | "archive" | "resolve")
            | (
                "phase",
                "add" | "focus" | "move" | "release" | "remove" | "reorder" | "start" | "update"
            )
            | ("plan", "move-goals" | "update-status")
            | (
                "task",
                "add" | "complete" | "log" | "remove" | "rename" | "reorder" | "start" | "update"
            )
            | ("gc", "inbox")
    );

    if atomic_project_state {
        RecoveryClass::AtomicProjectState
    } else {
        RecoveryClass::ExternalAtMostOnce
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::toml::TomlRead;
    use std::path::PathBuf;
    use std::process::Command as ProcessCommand;

    #[test]
    fn completion_failure_predicate_requires_precondition_error() {
        let failure = ExoFailure::new(
            ErrorCode::Internal,
            "unrelated blocked state",
            ExoFailure::orienting_steering(vec![]),
        )
        .with_details(serde_json::json!({
            "blocked_state": "unrelated"
        }));

        assert!(!is_completion_confirmation_failure(&failure));
    }

    struct TestCommand;

    struct ProjectTransport<'a> {
        root: &'a Path,
        project: Option<&'a Project>,
    }

    impl TransportContext for ProjectTransport<'_> {
        fn workspace_root(&self) -> Option<&std::path::Path> {
            Some(self.root)
        }

        fn project(&self) -> Option<&Project> {
            self.project
        }

        fn confirm_exec(&self, _action: &str) -> ConfirmResult {
            ConfirmResult::Proceed
        }

        fn format_output(&self, output: CommandOutput) -> TransportOutput {
            TransportOutput::Json(output.data)
        }

        fn format_error(&self, error: CommandError) -> TransportOutput {
            TransportOutput::Json(serde_json::json!({
                "error": error.message(),
            }))
        }

        fn render_steering(&self, _suggestions: Vec<SuggestedAction>) -> SteeringOutput {
            SteeringOutput::Json(serde_json::json!({ "suggestions": [] }))
        }
    }

    struct ProjectEchoCommand;

    impl Command for ProjectEchoCommand {
        fn namespace(&self) -> &'static str {
            "test"
        }

        fn operation(&self) -> &'static str {
            "project-echo"
        }

        fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
            let project = ctx.project.expect("project context is present");
            Ok(CommandOutput::data(serde_json::json!({
                "project_id": project.id.as_str(),
                "db_path": project.db_path(),
                "root": ctx.root,
            })))
        }
    }

    struct MutableProjectEchoCommand;

    impl Command for MutableProjectEchoCommand {
        fn namespace(&self) -> &'static str {
            "test"
        }

        fn operation(&self) -> &'static str {
            "mutable-project-echo"
        }

        fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
            unreachable!("mutable command should dispatch through execute_mut")
        }
    }

    impl MutableCommand for MutableProjectEchoCommand {
        fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
            let project = ctx.project.expect("project context is present");
            Ok(CommandOutput::data(serde_json::json!({
                "project_id": project.id.as_str(),
                "db_path": project.db_path(),
                "root": ctx.root,
            })))
        }
    }

    fn git_init(root: &Path) {
        let output = ProcessCommand::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .expect("run git init");

        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn json_path(value: &serde_json::Value, key: &str) -> PathBuf {
        value[key].as_str().map(PathBuf::from).expect("path string")
    }

    impl Command for TestCommand {
        fn namespace(&self) -> &'static str {
            "test"
        }

        fn operation(&self) -> &'static str {
            "example"
        }

        fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
            Ok(CommandOutput::message("Test executed"))
        }
    }

    #[test]
    fn test_command_output_message() {
        let output = CommandOutput::message("hello");
        assert_eq!(output.human_message, Some("hello".to_string()));
        assert_eq!(output.data, serde_json::Value::Null);
    }

    #[test]
    fn test_command_output_data() {
        let output = CommandOutput::data(serde_json::json!({"key": "value"}));
        assert!(output.human_message.is_none());
        assert_eq!(output.data["key"], "value");
    }

    #[test]
    fn test_command_trait() {
        let cmd = TestCommand;
        assert_eq!(cmd.namespace(), "test");
        assert_eq!(cmd.operation(), "example");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn pure_command_context_receives_resolved_project() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        git_init(root);
        let project = Project::resolve(root).expect("resolve project");
        let transport = ProjectTransport {
            root,
            project: Some(&project),
        };
        let cmd = CommandBox::pure(ProjectEchoCommand);

        let result = invoke_command_box_json(&cmd, &transport).expect("invoke command");

        assert_eq!(result.data["project_id"], project.id.as_str());
        assert_eq!(json_path(&result.data, "db_path"), project.db_path());
    }

    #[test]
    fn mutable_command_context_receives_resolved_project() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        git_init(root);
        let project = Project::resolve(root).expect("resolve project");
        let ctx = CommandContext {
            root,
            project: Some(&project),
            format: OutputFormat::Json,
            agent_id: None,
            workflow_confirmation: None,
        };
        let cmd = CommandBox::mutable(MutableProjectEchoCommand);

        let output = cmd.dispatch(&ctx).expect("dispatch mutable command");

        assert_eq!(output.data["project_id"], project.id.as_str());
        assert_eq!(json_path(&output.data, "db_path"), project.db_path());
    }

    #[test]
    fn test_invoke_json_binds_args() {
        struct JsonTransport;

        impl TransportContext for JsonTransport {
            fn workspace_root(&self) -> Option<&std::path::Path> {
                None
            }

            fn confirm_exec(&self, _action: &str) -> ConfirmResult {
                ConfirmResult::Proceed
            }

            fn format_output(&self, output: CommandOutput) -> TransportOutput {
                TransportOutput::Json(output.data)
            }

            fn format_error(&self, error: CommandError) -> TransportOutput {
                TransportOutput::Json(serde_json::json!({
                    "error": error.message(),
                }))
            }

            fn render_steering(&self, _suggestions: Vec<SuggestedAction>) -> SteeringOutput {
                SteeringOutput::Json(serde_json::json!({ "suggestions": [] }))
            }
        }

        // Test that invoke_json correctly routes errors through the
        // transport's format_error path, producing structured JSON.
        let cmd = TomlRead::new("does-not-exist.toml", None);
        let input = serde_json::json!({
            "path": "nonexistent.toml",
            "key": "anything"
        });
        let transport = JsonTransport;

        let err = cmd
            .invoke_json(&input, &transport)
            .expect_err("reading a nonexistent file should produce an error");

        let error_msg = err.get("error").and_then(|v| v.as_str());
        assert!(
            error_msg.is_some_and(|m| m.contains("Failed to read TOML file")),
            "expected TOML read error, got: {err}"
        );
    }
}
