#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)] // CLI tool uses blocking I/O
#![allow(clippy::case_sensitive_file_extension_comparisons)] // We control file extensions
#![allow(clippy::match_same_arms)] // Sometimes clarity > deduplication
#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]
#![cfg_attr(
    test,
    allow(
        clippy::doc_markdown,
        clippy::expect_used,
        clippy::panic,
        clippy::similar_names,
        clippy::unwrap_used
    )
)]

use exo::api::protocol::{
    Address, Auth, Effect, ErrorBody, ErrorCode, HelpOperation, HelpResult, PROTOCOL_VERSION,
    RequestEnvelope, ResponseEnvelope, Status, WorkflowConfirmationDecision,
    WorkflowConfirmationInput,
};
use exo::command::run::TASK_DIRECT_MODE_ENV;
use exo::command::transport::{CliTransport, ticket_for_exec_call};
use exo::failure::ExoFailure;
use exo::project::Project;
use exo::{command, context::AgentContext};
use serde::Deserialize;
use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Json,
    Compact,
    Grouped,
    Jsonl,
}

fn parse_output_format(s: &str) -> OutputFormat {
    match s {
        "json" => OutputFormat::Json,
        "compact" => OutputFormat::Compact,
        "grouped" => OutputFormat::Grouped,
        "jsonl" => OutputFormat::Jsonl,
        _ => OutputFormat::Human,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergeDriverKind {
    Toml,
}

impl MergeDriverKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidateColor {
    Auto,
    Always,
    Never,
}

impl ValidateColor {
    fn parse(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Auto,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

fn extract_format(raw_args: &[String]) -> (OutputFormat, Vec<String>) {
    let mut format = OutputFormat::Human;
    let mut args = Vec::with_capacity(raw_args.len());
    let mut i = 0;

    while i < raw_args.len() {
        let arg = &raw_args[i];
        if arg == "--format"
            && let Some(value) = raw_args.get(i + 1)
        {
            format = parse_output_format(value);
            i += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--format=") {
            format = parse_output_format(value);
            i += 1;
            continue;
        }

        args.push(arg.clone());
        i += 1;
    }

    (format, args)
}

/// Parse --workspace flag for daemon command.
/// Falls back to current directory if not specified.
fn parse_daemon_workspace(args: &[String], cwd: &Path) -> PathBuf {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--workspace"
            && let Some(value) = args.get(i + 1)
        {
            return PathBuf::from(value);
        }
        if let Some(value) = arg.strip_prefix("--workspace=") {
            return PathBuf::from(value);
        }
        i += 1;
    }
    cwd.to_path_buf()
}

/// Parse --timeout flag for daemon command (in seconds).
fn parse_daemon_timeout(args: &[String]) -> Option<u64> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--timeout"
            && let Some(value) = args.get(i + 1)
        {
            return value.parse().ok();
        }
        if let Some(value) = arg.strip_prefix("--timeout=") {
            return value.parse().ok();
        }
        i += 1;
    }
    None
}

fn parse_daemon_diagnostics(
    args: &[String],
) -> Option<exo::daemon_diagnostics::DaemonDiagnosticsConfig> {
    let mut enabled = false;
    let mut path = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--diagnostics-enabled" {
            enabled = true;
            i += 1;
            continue;
        }
        if arg == "--diagnostics-path" {
            if let Some(value) = args.get(i + 1) {
                path = Some(PathBuf::from(value));
            }
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--diagnostics-path=") {
            path = Some(PathBuf::from(value));
        }
        i += 1;
    }

    enabled.then_some(exo::daemon_diagnostics::DaemonDiagnosticsConfig { enabled, path })
}

/// Extract --direct flag from arguments.
///
/// When present, CLI commands bypass the daemon and execute directly.
/// This is useful for debugging or when the daemon is unavailable.
fn extract_direct_flag(raw_args: &[String]) -> (bool, Vec<String>) {
    let mut is_direct = false;
    let mut args = Vec::with_capacity(raw_args.len());

    for arg in raw_args {
        if arg == "--direct" {
            is_direct = true;
        } else {
            args.push(arg.clone());
        }
    }

    (is_direct, args)
}

fn extract_workflow_confirmation_flag(
    raw_args: &[String],
) -> Result<(Option<WorkflowConfirmationInput>, Vec<String>), String> {
    let mut workflow_confirmation = None;
    let mut args = Vec::with_capacity(raw_args.len());
    let mut i = 0;

    while i < raw_args.len() {
        let arg = &raw_args[i];
        if arg == "--workflow-confirmation-json" {
            let Some(value) = raw_args.get(i + 1) else {
                return Err("--workflow-confirmation-json requires a JSON value".to_string());
            };
            workflow_confirmation = Some(parse_workflow_confirmation_json(value)?);
            i += 2;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--workflow-confirmation-json=") {
            workflow_confirmation = Some(parse_workflow_confirmation_json(value)?);
            i += 1;
            continue;
        }

        args.push(arg.clone());
        i += 1;
    }

    Ok((workflow_confirmation, args))
}

fn parse_workflow_confirmation_json(value: &str) -> Result<WorkflowConfirmationInput, String> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)
        .map_err(|error| format!("invalid workflow confirmation JSON: {error}"))?;

    let candidate = parsed
        .get("completion_input")
        .or_else(|| {
            parsed
                .get("workflow_confirmation")
                .and_then(|workflow| workflow.get("completion_input").or(Some(workflow)))
        })
        .or_else(|| {
            parsed
                .get("workflowConfirmation")
                .and_then(|workflow| workflow.get("completion_input").or(Some(workflow)))
        })
        .unwrap_or(&parsed);

    let mut candidate = candidate.clone();
    if let Some(object) = candidate.as_object_mut() {
        if !object.contains_key("entity_type")
            && let Some(value) = object.get("entityType").cloned()
        {
            object.insert("entity_type".to_string(), value);
        }
        if !object.contains_key("entity_id")
            && let Some(value) = object.get("entityId").cloned()
        {
            object.insert("entity_id".to_string(), value);
        }
    }

    serde_json::from_value::<WorkflowConfirmationInput>(candidate)
        .map_err(|error| format!("invalid workflow confirmation payload: {error}"))
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct CliCompletionReview {
    header: String,
    question: String,
    message: String,
    proposed_outcome: String,
    completion_input: WorkflowConfirmationInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliReviewChoice {
    Approve,
    Revise,
    KeepWorking,
    Discuss,
}

#[derive(Debug, Clone, PartialEq)]
enum CliReviewDecision {
    Approve(WorkflowConfirmationInput),
    Revise(String),
    KeepWorking,
    Discuss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliReviewTransition {
    Redispatch,
    KeepWorking,
    Discuss,
}

trait CompletionReviewPrompter {
    fn prompt(&mut self, review: &CliCompletionReview) -> std::io::Result<CliReviewDecision>;
}

struct InteractiveCompletionReviewPrompter;

impl CompletionReviewPrompter for InteractiveCompletionReviewPrompter {
    fn prompt(&mut self, review: &CliCompletionReview) -> std::io::Result<CliReviewDecision> {
        let _theme = exo::ui::ThemeGuard::install_compact();
        let _ = cliclack::intro(&review.header);
        if !review.message.trim().is_empty() {
            let _ = cliclack::log::remark(&review.message);
        }

        let choice = cliclack::select(&review.question)
            .item(
                CliReviewChoice::Approve,
                "Approve outcome",
                "Record this outcome and complete the work.",
            )
            .item(
                CliReviewChoice::Revise,
                "Revise outcome",
                "Edit the outcome before reviewing it again.",
            )
            .item(
                CliReviewChoice::KeepWorking,
                "Keep working",
                "Leave the work pending.",
            )
            .item(
                CliReviewChoice::Discuss,
                "Discuss first",
                "Leave the outcome pending for discussion.",
            )
            .interact()?;

        match choice {
            CliReviewChoice::Approve => {
                Ok(CliReviewDecision::Approve(review.completion_input.clone()))
            }
            CliReviewChoice::Revise => {
                let revised = dialoguer::Input::<String>::new()
                    .with_prompt("Revised outcome")
                    .with_initial_text(review.proposed_outcome.clone())
                    .allow_empty(false)
                    .interact_text()
                    .map_err(std::io::Error::from)?;
                Ok(CliReviewDecision::Revise(revised))
            }
            CliReviewChoice::KeepWorking => Ok(CliReviewDecision::KeepWorking),
            CliReviewChoice::Discuss => Ok(CliReviewDecision::Discuss),
        }
    }
}

fn completion_review_is_interactive(
    format: OutputFormat,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
) -> bool {
    format == OutputFormat::Human && stdin_is_terminal && stdout_is_terminal && stderr_is_terminal
}

fn cli_can_prompt_for_completion_review(format: OutputFormat) -> bool {
    completion_review_is_interactive(
        format,
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    )
}

fn completion_review_from_value(value: &serde_json::Value) -> Option<CliCompletionReview> {
    [
        "/error/details/workflow_confirmation",
        "/error/details/details/workflow_confirmation",
        "/details/workflow_confirmation",
        "/workflow_confirmation",
    ]
    .into_iter()
    .find_map(|pointer| value.pointer(pointer))
    .and_then(|review| serde_json::from_value(review.clone()).ok())
    .filter(valid_cli_completion_review)
}

fn valid_cli_completion_review(review: &CliCompletionReview) -> bool {
    !review.header.trim().is_empty()
        && !review.question.trim().is_empty()
        && matches!(
            review.completion_input.kind.as_str(),
            "workflow_completion_confirmation" | "outcome_review"
        )
        && !review.completion_input.entity_type.trim().is_empty()
        && !review.completion_input.entity_id.trim().is_empty()
        && matches!(
            review.completion_input.decision,
            WorkflowConfirmationDecision::YesComplete
        )
        && review.completion_input.outcome == review.proposed_outcome
}

fn completion_review_from_response(response: &ResponseEnvelope) -> Option<CliCompletionReview> {
    let value = serde_json::to_value(response).ok()?;
    completion_review_from_value(&value)
}

fn replace_log_arg(args: &mut Vec<String>, revised_outcome: String) {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--log" {
            if let Some(value) = args.get_mut(index + 1) {
                *value = revised_outcome;
            } else {
                args.push(revised_outcome);
            }
            return;
        }
        if args[index].starts_with("--log=") {
            args[index] = format!("--log={revised_outcome}");
            return;
        }
        index += 1;
    }
    args.push("--log".to_string());
    args.push(revised_outcome);
}

fn handle_completion_review(
    review: &CliCompletionReview,
    prompter: &mut dyn CompletionReviewPrompter,
) -> Result<CliReviewDecision, i32> {
    match prompter.prompt(review) {
        Ok(decision) => Ok(decision),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Err(130),
        Err(error) => {
            eprintln!("Unable to review outcome: {error}");
            Err(1)
        }
    }
}

fn apply_completion_review_decision(
    args: &mut Vec<String>,
    workflow_confirmation: &mut Option<WorkflowConfirmationInput>,
    decision: CliReviewDecision,
) -> CliReviewTransition {
    match decision {
        CliReviewDecision::Approve(confirmation) => {
            *workflow_confirmation = Some(confirmation);
            CliReviewTransition::Redispatch
        }
        CliReviewDecision::Revise(outcome) => {
            replace_log_arg(args, outcome);
            *workflow_confirmation = None;
            CliReviewTransition::Redispatch
        }
        CliReviewDecision::KeepWorking => CliReviewTransition::KeepWorking,
        CliReviewDecision::Discuss => CliReviewTransition::Discuss,
    }
}

fn render_pending_review_transition(transition: CliReviewTransition) -> i32 {
    match transition {
        CliReviewTransition::KeepWorking => {
            println!("Outcome remains pending. Continue working when ready.");
            0
        }
        CliReviewTransition::Discuss => {
            println!("Outcome remains pending for discussion.");
            0
        }
        CliReviewTransition::Redispatch => 1,
    }
}

/// Dispatch a command through the daemon.
///
/// This function:
/// 1. Parses CLI args to an Invocation
/// 2. Converts the Invocation to a protocol Op
/// 3. Sends the request to the daemon
/// 4. Renders the response
///
/// Returns the exit code.
#[allow(dead_code)]
fn dispatch_via_daemon(
    args: &[String],
    cwd: &Path,
    format: OutputFormat,
    workflow_confirmation: Option<WorkflowConfirmationInput>,
) -> i32 {
    let mut prompter = InteractiveCompletionReviewPrompter;
    dispatch_via_daemon_with_prompter(
        args,
        cwd,
        format,
        workflow_confirmation,
        cli_can_prompt_for_completion_review(format),
        &mut prompter,
    )
}

fn dispatch_via_daemon_with_prompter(
    args: &[String],
    cwd: &Path,
    format: OutputFormat,
    workflow_confirmation: Option<WorkflowConfirmationInput>,
    can_prompt: bool,
    prompter: &mut dyn CompletionReviewPrompter,
) -> i32 {
    use exo::api::protocol::{CallParams, Op, RequestEnvelope};
    use exo::daemon::resolve_daemon_workspace;
    use exo::daemon_client::generate_request_id;

    let mut args = args.to_vec();
    let mut workflow_confirmation = workflow_confirmation;
    let spec = exo::command::command_spec::CommandSpec::from_registry(
        &exo::command::registry::default_registry(),
    );
    loop {
        let compilation = exo::router::compile_argv(&spec, &args);
        let Some(invocation) = compilation.invocation else {
            return render_compilation_errors(format, &args, &compilation);
        };
        let daemon_workspace = match resolve_daemon_workspace(cwd) {
            Ok(workspace) => workspace,
            Err(e) => {
                eprintln!("Failed to resolve daemon workspace: {e}");
                return 1;
            }
        };

        let address = invocation_to_address(&invocation);
        let input = invocation.to_json_input();
        let op = Op::Call(CallParams {
            address: address.clone(),
            input: input.clone(),
        });
        let effect = spec
            .operation(invocation.namespace(), invocation.operation())
            .map_or(Effect::Pure, |operation| operation.effect);
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: generate_request_id(),
            op,
            workspace_root: Some(daemon_workspace.clone()),
            auth: (effect == Effect::Exec).then(|| Auth {
                ticket: ticket_for_exec_call(&address, &input),
                confirm: true,
            }),
            workflow_confirmation: workflow_confirmation.clone(),
            agent_id: None,
        };

        let response = match send_daemon_request_with_recovery(&daemon_workspace, &request, effect)
        {
            Ok(response) => response,
            Err(error) => return render_daemon_dispatch_error(&error, format),
        };

        let Some(review) = can_prompt
            .then(|| completion_review_from_response(&response))
            .flatten()
        else {
            return render_daemon_response(&response, format);
        };

        let decision = match handle_completion_review(&review, prompter) {
            Ok(decision) => decision,
            Err(exit_code) => return exit_code,
        };
        let transition =
            apply_completion_review_decision(&mut args, &mut workflow_confirmation, decision);
        if transition != CliReviewTransition::Redispatch {
            return render_pending_review_transition(transition);
        }
    }
}

fn send_daemon_request_with_recovery(
    daemon_workspace: &Path,
    request: &RequestEnvelope,
    effect: Effect,
) -> Result<ResponseEnvelope, DaemonDispatchError> {
    use exo::daemon_client::{connect_or_spawn, send_request};

    send_daemon_request_with_recovery_using(
        request,
        effect,
        || connect_or_spawn(daemon_workspace),
        send_request,
    )
}

fn should_retry_daemon_request(effect: Effect, err: &std::io::Error) -> bool {
    let _ = effect;
    exo::daemon_client::is_reconnectable_daemon_error(err)
}

fn send_daemon_request_with_recovery_using<Stream, Connect, Send>(
    request: &RequestEnvelope,
    effect: Effect,
    mut connect: Connect,
    mut send: Send,
) -> Result<ResponseEnvelope, DaemonDispatchError>
where
    Connect: FnMut() -> std::io::Result<Stream>,
    Send: FnMut(&mut Stream, &RequestEnvelope) -> std::io::Result<ResponseEnvelope>,
{
    let mut stream = connect()
        .map_err(|source| DaemonDispatchError::new(request, effect, source, false, false, false))?;
    match send(&mut stream, request) {
        Ok(response) => Ok(response),
        Err(err) if exo::daemon_client::is_reconnectable_daemon_error(&err) => match connect() {
            Ok(mut retry_stream) => {
                if should_retry_daemon_request(effect, &err) {
                    send(&mut retry_stream, request).map_err(|source| {
                        DaemonDispatchError::new(
                            request,
                            effect,
                            source,
                            true,
                            true,
                            effect != Effect::Pure,
                        )
                    })
                } else {
                    Err(DaemonDispatchError::new(
                        request, effect, err, true, false, true,
                    ))
                }
            }
            Err(repair_error) => Err(DaemonDispatchError::with_repair_error(
                request,
                effect,
                err,
                repair_error,
            )),
        },
        Err(source) => Err(DaemonDispatchError::new(
            request, effect, source, false, false, false,
        )),
    }
}

#[derive(Debug)]
struct DaemonDispatchError {
    source: std::io::Error,
    effect: Effect,
    request_id: String,
    request_summary: serde_json::Value,
    daemon_repaired: bool,
    replayed: bool,
    ambiguous_outcome: bool,
    repair_error: Option<String>,
}

impl DaemonDispatchError {
    fn new(
        request: &RequestEnvelope,
        effect: Effect,
        source: std::io::Error,
        daemon_repaired: bool,
        replayed: bool,
        ambiguous_outcome: bool,
    ) -> Self {
        Self {
            source,
            effect,
            request_id: request.id.clone(),
            request_summary: daemon_request_summary(request),
            daemon_repaired,
            replayed,
            ambiguous_outcome,
            repair_error: None,
        }
    }

    fn with_repair_error(
        request: &RequestEnvelope,
        effect: Effect,
        source: std::io::Error,
        repair_error: std::io::Error,
    ) -> Self {
        Self {
            source,
            effect,
            request_id: request.id.clone(),
            request_summary: daemon_request_summary(request),
            daemon_repaired: false,
            replayed: false,
            ambiguous_outcome: effect != Effect::Pure,
            repair_error: Some(repair_error.to_string()),
        }
    }

    fn detail_code(&self) -> &'static str {
        if self.ambiguous_outcome {
            "exo.daemon_outcome_ambiguous"
        } else {
            "exo.daemon_transport_unavailable"
        }
    }

    fn message(&self) -> String {
        if self.ambiguous_outcome && self.daemon_repaired {
            format!(
                "Daemon connection was repaired automatically, but the {} request outcome is unknown.",
                effect_label(self.effect)
            )
        } else if self.daemon_repaired && self.replayed {
            format!(
                "Daemon connection was repaired automatically, but the retried {} request failed: {}",
                effect_label(self.effect),
                self.source
            )
        } else if let Some(repair_error) = &self.repair_error {
            format!(
                "Daemon closed the connection before responding, and automatic repair failed: {repair_error}"
            )
        } else {
            format!("Failed to communicate with daemon: {}", self.source)
        }
    }
}

fn render_daemon_dispatch_error(error: &DaemonDispatchError, format: OutputFormat) -> i32 {
    if format == OutputFormat::Json {
        let response = daemon_dispatch_error_response(error);
        let json = serde_json::to_string_pretty(&response).unwrap_or_default();
        println!("{json}");
        return 2;
    }

    for line in daemon_dispatch_error_lines(error) {
        eprintln!("{line}");
    }
    1
}

fn daemon_dispatch_error_response(error: &DaemonDispatchError) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: error.request_id.clone(),
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code: if error.ambiguous_outcome {
                ErrorCode::PreconditionFailed
            } else {
                ErrorCode::Internal
            },
            message: error.message(),
            details: Some(serde_json::json!({
                "code": error.detail_code(),
                "effect": effect_label(error.effect),
                "daemon_repaired": error.daemon_repaired,
                "replayed": error.replayed,
                "ambiguous_outcome": error.ambiguous_outcome,
                "request_summary": error.request_summary,
                "repair_error": error.repair_error,
                "guidance": daemon_recovery_guidance(error),
            })),
        }),
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: Some(error.effect),
        trace: None,
    }
}

fn daemon_dispatch_error_lines(error: &DaemonDispatchError) -> Vec<String> {
    let mut lines = vec![format!(
        "Failed to communicate with daemon: {}",
        error.source
    )];
    if error.source.kind() == std::io::ErrorKind::UnexpectedEof {
        if error.daemon_repaired {
            lines.push("The daemon connection was repaired automatically.".to_string());
        } else {
            lines.push(
                "Exo attempted to repair the daemon connection automatically, but repair failed."
                    .to_string(),
            );
        }
    }
    lines.push(daemon_recovery_guidance(error));
    lines
}

fn daemon_recovery_guidance(error: &DaemonDispatchError) -> String {
    if error.repair_error.is_some() && error.ambiguous_outcome {
        "Exo could not repair the daemon transport automatically. Refresh Exo state before retrying; the interrupted write/exec request may already have completed.".to_string()
    } else if error.repair_error.is_some() {
        "Exo could not repair the daemon transport automatically. Retry after the transport is available again.".to_string()
    } else if error.ambiguous_outcome {
        "Refresh Exo state before retrying; the interrupted write/exec request may already have completed.".to_string()
    } else if error.daemon_repaired && error.replayed {
        "The daemon was repaired and the read was retried, but the retry failed. Refresh Exo state and retry if needed.".to_string()
    } else if error.daemon_repaired {
        "The daemon transport was repaired automatically. Refresh Exo state and retry if needed."
            .to_string()
    } else {
        "The daemon transport is unavailable. Retry after the transport is available again."
            .to_string()
    }
}

fn daemon_request_summary(request: &RequestEnvelope) -> serde_json::Value {
    serde_json::json!({
        "request_id": request.id.clone(),
        "op_path": daemon_request_op_path(request),
        "has_auth": request.auth.is_some(),
        "has_workflow_confirmation": request.workflow_confirmation.is_some(),
    })
}

fn daemon_request_op_path(request: &RequestEnvelope) -> Vec<String> {
    match &request.op {
        exo::api::protocol::Op::Call(params) | exo::api::protocol::Op::Preview(params) => {
            match &params.address {
                Address::Root => vec![],
                Address::Namespace { path } | Address::Operation { path } => path.clone(),
            }
        }
        exo::api::protocol::Op::List(params) => {
            let mut path = match &params.address {
                Address::Root => vec![],
                Address::Namespace { path } | Address::Operation { path } => path.clone(),
            };
            path.push(params.kind.clone());
            path
        }
        exo::api::protocol::Op::Help(params) => match &params.address {
            Address::Root => vec![],
            Address::Namespace { path } | Address::Operation { path } => path.clone(),
        },
    }
}

const fn effect_label(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "pure",
        Effect::Write => "write",
        Effect::Exec => "exec",
    }
}

/// Convert an Invocation to a protocol Address.
#[allow(dead_code)]
fn invocation_to_address(invocation: &exo::command::router::Invocation) -> Address {
    let path = &invocation.path;
    if path.namespace.is_empty() {
        Address::Operation {
            path: vec![path.operation.clone()],
        }
    } else {
        Address::Operation {
            path: vec![path.namespace.clone(), path.operation.clone()],
        }
    }
}

/// Render a daemon response to stdout/stderr.
///
/// Returns the exit code.
#[allow(dead_code)]
fn render_daemon_response(response: &ResponseEnvelope, format: OutputFormat) -> i32 {
    match response.status {
        Status::Ok => {
            if format == OutputFormat::Json {
                // In JSON mode, output the full response
                let json = serde_json::to_string_pretty(&response).unwrap_or_default();
                println!("{json}");
            } else if let Some(display) = &response.display {
                // Use display metadata if available
                if let Some(body) = &display.body {
                    println!("{body}");
                } else {
                    println!("{}", display.summary);
                }
            } else if let Some(result) = &response.result {
                // Fall back to raw result
                let json = serde_json::to_string_pretty(result).unwrap_or_default();
                println!("{json}");
            }
            0
        }
        Status::Error => {
            if format == OutputFormat::Json {
                let json = serde_json::to_string_pretty(&response).unwrap_or_default();
                println!("{json}");
                2
            } else if let Some(error) = &response.error {
                eprintln!("Error: {}", error.message);
                1
            } else {
                eprintln!("Unknown error");
                1
            }
        }
        Status::NeedsInput | Status::ConfirmRequired => {
            // These statuses require interactive handling
            // For now, just report them
            if format == OutputFormat::Json {
                let json = serde_json::to_string_pretty(&response).unwrap_or_default();
                println!("{json}");
            } else {
                eprintln!("Operation requires confirmation or input");
                if let Some(error) = &response.error {
                    eprintln!("{}", error.message);
                }
            }
            1
        }
    }
}

fn handle_help(args: &[String], format: OutputFormat) {
    let parsed = exo::command_text::parse_argv(args);
    let tokens = parsed.help_target().unwrap_or(&[]);

    let spec = exo::command::command_spec::CommandSpec::from_registry(
        &exo::command::registry::default_registry(),
    );

    if let Some(result) = special_help_for_tokens(tokens) {
        match format {
            OutputFormat::Json => match serde_json::to_string_pretty(&result) {
                Ok(json) => println!("{json}"),
                Err(err) => eprintln!("Failed to serialize help output: {err}"),
            },
            _ => print!("{}", render_help_text(&result)),
        }
        return;
    }

    let address = match tokens.len() {
        0 => Address::Root,
        1 if spec.namespaces.contains_key(&tokens[0]) => Address::Namespace {
            path: vec![tokens[0].clone()],
        },
        1 => Address::Operation {
            path: vec![tokens[0].clone()],
        },
        _ => Address::Operation {
            path: vec![tokens[0].clone(), tokens[1..].join(".")],
        },
    };

    let result = exo::api::handler::help_for_address(&spec, &address)
        .or_else(|| exo::api::handler::help_for_address(&spec, &Address::Root))
        .unwrap_or_else(|| HelpResult {
            title: "help".to_string(),
            summary: "No help available.".to_string(),
            namespaces: vec![],
            operations: vec![],
            next_calls: vec![],
        });

    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(&result) {
            Ok(json) => println!("{json}"),
            Err(err) => eprintln!("Failed to serialize help output: {err}"),
        },
        _ => {
            print!("{}", render_help_text(&result));
        }
    }
}

fn special_help_for_tokens(tokens: &[String]) -> Option<HelpResult> {
    let parts: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["init"] => Some(special_operation_help(
            "init",
            Effect::Write,
            "Initialize Exosuit project in the current directory",
            vec![exo::command::command_spec::ArgSpec::flag(
                "defaults",
                "Use non-interactive default values",
            )],
        )),
        ["daemon", "ensure"] => Some(special_operation_help(
            "daemon ensure",
            Effect::Exec,
            "Ensure the workspace daemon is running",
            vec![
                exo::command::command_spec::ArgSpec::option(
                    "workspace",
                    "Workspace path to resolve before starting the daemon",
                    exo::command::command_spec::ValueType::Path,
                )
                .optional(),
            ],
        )),
        ["daemon", "restart"] => Some(special_operation_help(
            "daemon restart",
            Effect::Exec,
            "Force-restart the workspace daemon",
            vec![
                exo::command::command_spec::ArgSpec::option(
                    "workspace",
                    "Workspace path to resolve before restarting the daemon",
                    exo::command::command_spec::ValueType::Path,
                )
                .optional(),
            ],
        )),
        ["daemon", "status"] => Some(special_operation_help(
            "daemon status",
            Effect::Pure,
            "Inspect the workspace daemon without starting or restarting it",
            vec![
                exo::command::command_spec::ArgSpec::option(
                    "workspace",
                    "Workspace path to resolve before inspecting the daemon",
                    exo::command::command_spec::ValueType::Path,
                )
                .optional(),
            ],
        )),
        ["daemon", "run"] => Some(special_operation_help(
            "daemon run",
            Effect::Exec,
            "Run the workspace daemon",
            vec![
                exo::command::command_spec::ArgSpec::option(
                    "workspace",
                    "Workspace path for the daemon",
                    exo::command::command_spec::ValueType::Path,
                )
                .optional(),
                exo::command::command_spec::ArgSpec::option(
                    "timeout",
                    "Idle timeout in seconds",
                    exo::command::command_spec::ValueType::Int,
                )
                .optional(),
            ],
        )),
        ["mcp", "serve"] => Some(special_operation_help(
            "mcp serve",
            Effect::Exec,
            "Run the Exo MCP stdio server",
            vec![],
        )),
        ["mcp", "worker"] => Some(special_operation_help(
            "mcp worker",
            Effect::Exec,
            "Run the Exo MCP worker stdio server",
            vec![],
        )),
        ["json", "server"] => Some(special_operation_help(
            "json server",
            Effect::Exec,
            "Run the machine-channel JSON stdio server",
            vec![],
        )),
        ["merge-driver"] => Some(special_operation_help(
            "merge-driver",
            Effect::Exec,
            "Run an Exo merge driver helper",
            vec![
                exo::command::command_spec::ArgSpec::positional(
                    "kind",
                    "Merge driver kind",
                    exo::command::command_spec::ValueType::String,
                ),
                exo::command::command_spec::ArgSpec::positional(
                    "base",
                    "Base file path",
                    exo::command::command_spec::ValueType::Path,
                ),
                exo::command::command_spec::ArgSpec::positional(
                    "current",
                    "Current file path",
                    exo::command::command_spec::ValueType::Path,
                ),
                exo::command::command_spec::ArgSpec::positional(
                    "other",
                    "Other file path",
                    exo::command::command_spec::ValueType::Path,
                ),
                exo::command::command_spec::ArgSpec::positional(
                    "path",
                    "Merged file path",
                    exo::command::command_spec::ValueType::Path,
                )
                .optional(),
            ],
        )),
        ["validate"] => Some(special_operation_help(
            "validate",
            Effect::Exec,
            "Run Exohook validation",
            vec![
                exo::command::command_spec::ArgSpec::positional(
                    "name",
                    "Optional validation name",
                    exo::command::command_spec::ValueType::String,
                )
                .optional(),
                exo::command::command_spec::ArgSpec::flag("verbose", "Show verbose output"),
                exo::command::command_spec::ArgSpec::flag("dry-run", "Preview validation work"),
                exo::command::command_spec::ArgSpec::option(
                    "color",
                    "Color output mode",
                    exo::command::command_spec::ValueType::String,
                )
                .optional(),
            ],
        )),
        _ => None,
    }
}

fn special_operation_help(
    path: &str,
    effect: Effect,
    summary: &str,
    args: Vec<exo::command::command_spec::ArgSpec>,
) -> HelpResult {
    HelpResult {
        title: path.to_string(),
        summary: summary.to_string(),
        namespaces: vec![],
        operations: vec![HelpOperation {
            path: path.to_string(),
            effect,
            summary: summary.to_string(),
            args,
        }],
        next_calls: vec![],
    }
}

fn render_help_text(result: &HelpResult) -> String {
    let mut out = String::new();

    if !result.title.is_empty() {
        out.push_str(&result.title);
        out.push('\n');
    }

    if !result.summary.is_empty() {
        out.push_str(&result.summary);
        out.push('\n');
    }

    let is_operation = result.namespaces.is_empty()
        && result.operations.len() == 1
        && !result.operations[0].args.is_empty();

    if is_operation {
        out.push('\n');
        out.push_str(&render_operation_help(&result.operations[0]));
        return out;
    }

    if !result.namespaces.is_empty() {
        out.push('\n');
        out.push_str("Namespaces:\n");
        for ns in &result.namespaces {
            let _ = writeln!(out, "  {} - {}", ns.path.join(" "), ns.summary);
        }
    }

    if !result.operations.is_empty() {
        out.push('\n');
        out.push_str("Operations:\n");
        for op in &result.operations {
            let _ = writeln!(
                out,
                "  {} {} - {}",
                op.path,
                effect_tag(op.effect),
                op.summary
            );
        }
    }

    out
}

fn render_operation_help(op: &exo::api::protocol::HelpOperation) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} - {}",
        op.path,
        effect_tag(op.effect),
        op.summary
    );

    if op.args.is_empty() {
        return out;
    }

    let formatted: Vec<(String, String)> = op.args.iter().map(format_arg_spec).collect();
    let max_width = formatted
        .iter()
        .map(|(left, _)| left.len())
        .max()
        .unwrap_or(0);

    out.push('\n');
    out.push_str("Args:\n");
    for (left, right) in formatted {
        let _ = writeln!(out, "  {left:max_width$}    {right}");
    }

    out
}

fn format_arg_spec(arg: &exo::command::command_spec::ArgSpec) -> (String, String) {
    use exo::command::command_spec::ArgKind;

    let type_label = value_type_label(&arg.value_type);
    let left = match arg.kind {
        ArgKind::Flag => arg.short.map_or_else(
            || format!("--{}", arg.name),
            |short| format!("--{}, -{}", arg.name, short),
        ),
        ArgKind::Option => {
            let mut display = arg.short.map_or_else(
                || format!("--{}", arg.name),
                |short| format!("--{}, -{}", arg.name, short),
            );
            let _ = write!(display, " <{type_label}>");
            display
        }
        ArgKind::Positional => format!("<{}> <{}>", arg.name, type_label),
    };

    let mut meta_parts = Vec::new();
    meta_parts.push(if arg.optional { "optional" } else { "required" }.to_string());
    if let Some(default) = &arg.default {
        meta_parts.push(format!("default: {default}"));
    }
    if arg.repeatable {
        meta_parts.push("repeatable".to_string());
    }

    let right = if meta_parts.is_empty() {
        arg.description.clone()
    } else {
        format!("{} ({})", arg.description, meta_parts.join(", "))
    };

    (left, right)
}

fn value_type_label(value_type: &exo::command::command_spec::ValueType) -> String {
    use exo::command::command_spec::ValueType;

    match value_type {
        ValueType::Bool => "bool".to_string(),
        ValueType::Int => "int".to_string(),
        ValueType::Float => "float".to_string(),
        ValueType::String => "string".to_string(),
        ValueType::Path => "path".to_string(),
        ValueType::Json => "json".to_string(),
        ValueType::Enum(variants) => format!("enum({})", variants.join("|")),
    }
}

const fn effect_tag(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "[pure]",
        Effect::Write => "[write]",
        Effect::Exec => "[exec]",
    }
}

fn normalize_run_shorthand(args: &mut Vec<String>) {
    if args.first().map(String::as_str) != Some("run") {
        return;
    }

    if args.len() == 1 {
        args.push("tasks".to_string());
        return;
    }

    if let Some(second) = args.get(1)
        && (second == "task" || second == "tasks" || second.starts_with('-'))
    {
        return;
    }

    let task_id = args.get(1).cloned();
    if let Some(task_id) = task_id {
        let mut rewritten = Vec::with_capacity(args.len() + 1);
        rewritten.push("run".to_string());
        rewritten.push("task".to_string());
        rewritten.push(task_id);
        if args.len() > 2 {
            rewritten.extend_from_slice(&args[2..]);
        }
        *args = rewritten;
    }
}

fn normalize_verify_shorthand(args: &mut Vec<String>) {
    if args.len() == 1 && args.first().map(String::as_str) == Some("verify") {
        args.push("run".to_string());
    }
}

fn normalize_project_repair_apply_shorthand(args: &mut Vec<String>) {
    if args.first().map(String::as_str) != Some("project")
        || args.get(1).map(String::as_str) != Some("repair")
        || !args.iter().any(|arg| arg == "--apply")
    {
        return;
    }

    args[1] = "repair-apply".to_string();
    args.retain(|arg| arg != "--apply");
}

fn is_project_bootstrap_read(args: &[String]) -> bool {
    matches!(
        (
            args.first().map(String::as_str),
            args.get(1).map(String::as_str)
        ),
        (
            Some("project"),
            Some("resolve" | "list" | "snapshot" | "repair" | "repair-apply" | "move-root")
        )
    )
}

fn is_sidecar_bootstrap_context_command(args: &[String]) -> bool {
    matches!(
        (
            args.first().map(String::as_str),
            args.get(1).map(String::as_str)
        ),
        (
            Some("sidecar"),
            Some("bootstrap" | "discover" | "init" | "link" | "setup" | "status" | "unlink")
        )
    )
}

fn is_update_command(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("update"))
}

fn command_loads_request_context(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("status"))
        || matches!(
            (
                args.first().map(String::as_str),
                args.get(1).map(String::as_str)
            ),
            (Some("task"), Some("list"))
        )
}

fn attach_post_write_report(
    value: &mut serde_json::Value,
    report: &exo::post_write::PostWritePersistenceReport,
) {
    let Ok(report_value) = serde_json::to_value(report) else {
        return;
    };
    if let Some(obj) = value.as_object_mut() {
        obj.insert("post_write".to_string(), report_value);
    }
}

fn post_write_issue(report: &exo::post_write::PostWritePersistenceReport) -> Option<&str> {
    report
        .sidecar_auto_persist
        .as_ref()
        .filter(|auto_persist| !auto_persist.ok)
        .and_then(|auto_persist| auto_persist.issue.as_deref())
}

fn lightweight_context(cwd: PathBuf) -> AgentContext {
    let project = Project::resolve(&cwd).ok();
    AgentContext {
        root: cwd,
        project,
        plan: exo::context::ExoState::default(),
    }
}

fn load_context_or_exit(
    format: OutputFormat,
    is_machine_protocol: bool,
    cwd: PathBuf,
) -> AgentContext {
    match AgentContext::load(cwd) {
        Ok(context) => context,
        Err(e) => {
            let original_command = original_command_for_guidance();
            let preload_guidance =
                exo::preload_guidance::classify_context_load_error(&e, &original_command);

            if is_machine_protocol {
                let response = ResponseEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    id: "unknown".to_string(),
                    status: Status::Error,
                    result: None,
                    error: Some(preload_guidance.as_ref().map_or_else(
                        || ErrorBody {
                            code: ErrorCode::Internal,
                            message: format!("Failed to load agent context: {e}"),
                            details: None,
                        },
                        |guidance| ErrorBody {
                            code: guidance.error_code,
                            message: guidance.message(),
                            details: Some(guidance.details()),
                        },
                    )),
                    ticket: None,
                    steering: Some(preload_guidance.as_ref().map_or_else(
                        protocol_help_root_steering,
                        exo::preload_guidance::PreloadGuidance::to_steering,
                    )),
                    reminders: None,
                    display: None,
                    preview: None,
                    effect: None,
                    trace: None,
                };

                match serde_json::to_string(&response) {
                    Ok(s) => println!("{s}"),
                    Err(err) => {
                        eprintln!("Failed to serialize protocol error envelope: {err}");
                    }
                }
                std::process::exit(2);
            }

            if format == OutputFormat::Json {
                let response = ResponseEnvelope {
                    protocol_version: PROTOCOL_VERSION,
                    id: "cli".to_string(),
                    status: Status::Error,
                    result: None,
                    error: Some(preload_guidance.as_ref().map_or_else(
                        || ErrorBody {
                            code: ErrorCode::Internal,
                            message: format!("Failed to load agent context: {e}"),
                            details: None,
                        },
                        |guidance| ErrorBody {
                            code: guidance.error_code,
                            message: guidance.message(),
                            details: Some(guidance.details()),
                        },
                    )),
                    ticket: None,
                    steering: Some(preload_guidance.as_ref().map_or_else(
                        protocol_help_root_steering,
                        exo::preload_guidance::PreloadGuidance::to_steering,
                    )),
                    reminders: None,
                    display: None,
                    preview: None,
                    effect: None,
                    trace: None,
                };

                match serde_json::to_string_pretty(&response) {
                    Ok(s) => println!("{s}"),
                    Err(err) => eprintln!("Failed to serialize protocol error envelope: {err}"),
                }

                std::process::exit(2);
            }

            if let Some(guidance) = preload_guidance {
                eprintln!("{}", guidance.message());
                eprintln!("\nUnderlying error: {e}");
            } else {
                eprintln!("Failed to load agent context: {e}");
                eprintln!("\n[Next]\n- exo init\n- exo update");
            }
            std::process::exit(1);
        }
    }
}

fn original_command_for_guidance() -> String {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (_, args_after_format) = extract_format(&raw_args);
    let (_, args) = extract_direct_flag(&args_after_format);
    let command = args.join(" ");
    if command.is_empty() {
        "exo".to_string()
    } else {
        format!("exo {command}")
    }
}

fn render_compilation_errors(
    format: OutputFormat,
    argv: &[String],
    compilation: &exo::router::Compilation,
) -> i32 {
    if format == OutputFormat::Json {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "cli".to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: ErrorCode::InvalidInput,
                message: "Invalid command invocation".to_string(),
                details: Some(serde_json::json!({
                    "argv": argv,
                    "diagnostics": compilation.diagnostics,
                })),
            }),
            ticket: None,
            steering: Some(
                compilation
                    .steering
                    .clone()
                    .unwrap_or_else(protocol_help_root_steering),
            ),
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };

        match serde_json::to_string_pretty(&response) {
            Ok(s) => println!("{s}"),
            Err(err) => eprintln!("Failed to serialize protocol error envelope: {err}"),
        }

        return 2;
    }

    if compilation.diagnostics.is_empty() {
        eprintln!("error: failed to parse command");
        return 1;
    }

    for diagnostic in &compilation.diagnostics {
        eprintln!("error: {}", diagnostic.message);
        if let Some(span) = &diagnostic.span {
            if let Some(arg) = argv.get(span.arg_index) {
                eprintln!("  at arg {}: {}", span.arg_index, arg);
            } else {
                eprintln!("  at arg {}", span.arg_index);
            }
        }
        if !diagnostic.suggestions.is_empty() {
            eprintln!("  suggestions:");
            for suggestion in &diagnostic.suggestions {
                eprintln!("    - {}: {}", suggestion.label, suggestion.replacement);
            }
        }
    }

    1
}

fn render_daemon_ensure_error(format: OutputFormat, error: &std::io::Error) -> i32 {
    if format == OutputFormat::Json {
        let code = if error.kind() == std::io::ErrorKind::InvalidInput {
            ErrorCode::InvalidInput
        } else {
            ErrorCode::Internal
        };
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "cli".to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code,
                message: error.to_string(),
                details: None,
            }),
            ticket: None,
            steering: Some(protocol_help_root_steering()),
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
        match serde_json::to_string_pretty(&response) {
            Ok(json) => println!("{json}"),
            Err(err) => eprintln!("Failed to serialize daemon ensure error: {err}"),
        }
        2
    } else {
        eprintln!("exo daemon ensure: {error}");
        1
    }
}

fn render_daemon_ensure_report(
    format: OutputFormat,
    report: exo::daemon::DaemonEnsureReport,
) -> i32 {
    if format == OutputFormat::Json {
        let result = match serde_json::to_value(report) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("Failed to serialize daemon ensure report: {error}");
                return 1;
            }
        };
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "cli".to_string(),
            status: Status::Ok,
            result: Some(result),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
        match serde_json::to_string_pretty(&response) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("Failed to serialize daemon ensure response: {error}");
                return 1;
            }
        }
    } else {
        println!("daemon ready: {}", report.endpoint);
    }
    0
}

fn render_daemon_status_report(
    format: OutputFormat,
    report: exo::daemon::DaemonStatusReport,
) -> i32 {
    if format == OutputFormat::Json {
        let result = match serde_json::to_value(report) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("Failed to serialize daemon status report: {error}");
                return 1;
            }
        };
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "cli".to_string(),
            status: Status::Ok,
            result: Some(result),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
        match serde_json::to_string_pretty(&response) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("Failed to serialize daemon status response: {error}");
                return 1;
            }
        }
    } else {
        println!("daemon status: {:?}", report.state);
        if let Some(socket_path) = report.socket_path {
            println!("socket: {}", socket_path.display());
        }
        if let Some(endpoint) = report.endpoint {
            println!("endpoint: {endpoint}");
        }
        if let Some(pid) = report.pid {
            println!("pid: {pid}");
        }
        if let Some(issue) = report.issue {
            println!("issue: {issue}");
        }
    }
    0
}

fn main() {
    exo_reexec::maybe_reexec();

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (mut format, args_after_format) = extract_format(&raw_args);
    let (mut workflow_confirmation, args_after_workflow_confirmation) =
        match extract_workflow_confirmation_flag(&args_after_format) {
            Ok(result) => result,
            Err(message) => {
                if format == OutputFormat::Json {
                    let value = serde_json::json!({
                        "id": "cli",
                        "protocol_version": PROTOCOL_VERSION,
                        "status": "error",
                        "error": {
                            "code": "invalid_input",
                            "message": message,
                        },
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
                    );
                    std::process::exit(2);
                }
                eprintln!("{message}");
                std::process::exit(1);
            }
        };
    let (has_direct_flag, mut args) = extract_direct_flag(&args_after_workflow_confirmation);
    normalize_project_repair_apply_shorthand(&mut args);
    let is_direct = has_direct_flag
        || std::env::var_os(TASK_DIRECT_MODE_ENV).is_some()
        || is_project_bootstrap_read(&args)
        || is_sidecar_bootstrap_context_command(&args);

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("exo {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if exo::command_text::parse_argv(&args).help_target().is_some() {
        handle_help(&args, format);
        return;
    }

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("Failed to determine current directory: {e}");
            std::process::exit(1);
        }
    };
    let update_project = is_update_command(&args).then(|| Project::resolve(&cwd));
    let is_direct = is_direct
        || update_project
            .as_ref()
            .is_some_and(std::result::Result::is_err);

    match args.first().map(String::as_str) {
        Some("json") if args.get(1).map(String::as_str) == Some("server") => {
            let context = load_context_or_exit(format, true, cwd);
            if let Err(e) = handle_json_server(&context, context.project.as_ref(), is_direct) {
                render_fatal_error(format, Some(&context), &*e);
                std::process::exit(if format == OutputFormat::Json { 2 } else { 1 });
            }
            return;
        }
        Some("mcp") if args.get(1).map(String::as_str) == Some("serve") => {
            let context = lightweight_context(cwd);
            if let Err(e) = exo::mcp::serve_stdio(&context.root, context.project.as_ref()) {
                render_fatal_error(format, Some(&context), &*e);
                std::process::exit(if format == OutputFormat::Json { 2 } else { 1 });
            }
            return;
        }
        Some("mcp") if args.get(1).map(String::as_str) == Some("worker") => {
            let context = lightweight_context(cwd);
            if let Err(e) = exo::mcp::serve_worker_stdio(&context.root, context.project.as_ref()) {
                render_fatal_error(format, Some(&context), &*e);
                std::process::exit(if format == OutputFormat::Json { 2 } else { 1 });
            }
            return;
        }
        Some("daemon") if args.get(1).map(String::as_str) == Some("run") => {
            // Parse --workspace flag
            let workspace = parse_daemon_workspace(&args, &cwd);
            let workspace = match exo::daemon::resolve_daemon_workspace(&workspace) {
                Ok(workspace) => workspace,
                Err(error) => {
                    eprintln!("exo daemon: failed to resolve project: {error}");
                    std::process::exit(1);
                }
            };
            let timeout = parse_daemon_timeout(&args);
            let diagnostics = parse_daemon_diagnostics(&args);

            // Run the async daemon
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|error| {
                eprintln!("Failed to create tokio runtime: {error}");
                std::process::exit(1);
            });
            rt.block_on(exo::daemon::run_daemon(workspace, timeout, diagnostics));
            return;
        }
        Some("daemon") if args.get(1).map(String::as_str) == Some("ensure") => {
            let workspace = parse_daemon_workspace(&args, &cwd);
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|error| {
                eprintln!("Failed to create tokio runtime: {error}");
                std::process::exit(1);
            });
            match rt.block_on(exo::daemon::ensure_daemon_with_report(&workspace)) {
                Ok(outcome) => {
                    std::process::exit(render_daemon_ensure_report(format, outcome.into_report()));
                }
                Err(error) => {
                    std::process::exit(render_daemon_ensure_error(format, &error));
                }
            }
        }
        Some("daemon") if args.get(1).map(String::as_str) == Some("restart") => {
            let workspace = parse_daemon_workspace(&args, &cwd);
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|error| {
                eprintln!("Failed to create tokio runtime: {error}");
                std::process::exit(1);
            });
            match rt.block_on(exo::daemon::restart_daemon_with_report(&workspace)) {
                Ok(outcome) => {
                    std::process::exit(render_daemon_ensure_report(format, outcome.into_report()));
                }
                Err(error) => {
                    std::process::exit(render_daemon_ensure_error(format, &error));
                }
            }
        }
        Some("daemon") if args.get(1).map(String::as_str) == Some("status") => {
            let workspace = parse_daemon_workspace(&args, &cwd);
            std::process::exit(render_daemon_status_report(
                format,
                exo::daemon::daemon_status(&workspace),
            ));
        }
        Some("init") => {
            let defaults = args.iter().any(|a| a == "--defaults");
            let project = Project::resolve(&cwd).ok();
            let context = AgentContext {
                root: cwd,
                project,
                plan: exo::context::ExoState {
                    meta: None,
                    epochs: vec![],
                },
            };

            if let Err(e) = command::init::run_init(&context, defaults) {
                render_fatal_error(format, Some(&context), &*e);
                std::process::exit(1);
            }
            return;
        }
        Some("merge-driver") => {
            let kind = args.get(1).and_then(|value| MergeDriverKind::parse(value));
            let Some(kind) = kind else {
                eprintln!("merge-driver requires a kind (toml)");
                std::process::exit(1);
            };
            let Some(base) = args.get(2) else {
                eprintln!("merge-driver requires base, current, other paths");
                std::process::exit(1);
            };
            let Some(current) = args.get(3) else {
                eprintln!("merge-driver requires base, current, other paths");
                std::process::exit(1);
            };
            let Some(other) = args.get(4) else {
                eprintln!("merge-driver requires base, current, other paths");
                std::process::exit(1);
            };
            let path = args.get(5).map(String::as_str);
            let kind = match kind {
                MergeDriverKind::Toml => exo::merge_driver::MergeDriverKind::Toml,
            };

            let code = exo::merge_driver::run(
                kind,
                Path::new(base),
                Path::new(current),
                Path::new(other),
                path,
            );
            std::process::exit(code);
        }
        Some("validate") => {
            let context = load_context_or_exit(format, false, cwd);
            let mut name: Option<&str> = None;
            let mut verbose = false;
            let mut dry_run = false;
            let mut color = ValidateColor::Auto;

            let mut i = 1;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--verbose" {
                    verbose = true;
                    i += 1;
                    continue;
                }
                if arg == "--dry-run" {
                    dry_run = true;
                    i += 1;
                    continue;
                }
                if arg == "--color"
                    && let Some(value) = args.get(i + 1)
                {
                    color = ValidateColor::parse(value);
                    i += 2;
                    continue;
                }
                if let Some(value) = arg.strip_prefix("--color=") {
                    color = ValidateColor::parse(value);
                    i += 1;
                    continue;
                }
                if !arg.starts_with('-') && name.is_none() {
                    name = Some(arg.as_str());
                }
                i += 1;
            }

            if let Err(e) = run_exohook_validate(&context, name, format, verbose, dry_run, color) {
                render_fatal_error(format, Some(&context), &*e);
                std::process::exit(if format == OutputFormat::Json { 2 } else { 1 });
            }
            return;
        }
        _ => {}
    }

    // Handle --json flag for map command before daemon dispatch
    if args.first().map(String::as_str) == Some("map") && args.iter().any(|a| a == "--json") {
        format = OutputFormat::Json;
        strip_flag(&mut args, "--json");
    }

    normalize_run_shorthand(&mut args);
    normalize_verify_shorthand(&mut args);
    normalize_project_repair_apply_shorthand(&mut args);

    // Daemon mode: dispatch unless the command requires trusted direct CLI handling.
    if !is_direct {
        let exit_code = dispatch_via_daemon(&args, &cwd, format, workflow_confirmation);
        std::process::exit(exit_code);
    }

    // Direct mode: load context and execute locally.
    // Project bootstrap operations tell callers where Exo state should live or
    // repair stale policy, so they must not require existing Exo state first.
    let context = if is_project_bootstrap_read(&args)
        || is_sidecar_bootstrap_context_command(&args)
        || is_update_command(&args)
        || command_loads_request_context(&args)
    {
        let project = if is_update_command(&args) {
            update_project.and_then(std::result::Result::ok)
        } else {
            Project::resolve(&cwd).ok()
        };
        AgentContext {
            root: cwd,
            project,
            plan: exo::context::ExoState {
                meta: None,
                epochs: vec![],
            },
        }
    } else {
        load_context_or_exit(format, false, cwd)
    };

    if format != OutputFormat::Json {
        let reminders = exo::verifiers::run_global_verifiers(&context.root);
        emit_verifier_reminders(&reminders);
    }

    let spec = exo::command::command_spec::CommandSpec::from_registry(
        &exo::command::registry::default_registry(),
    );
    let cmd_format = match format {
        OutputFormat::Json => exo::command::traits::OutputFormat::Json,
        _ => exo::command::traits::OutputFormat::Human,
    };
    let can_prompt = cli_can_prompt_for_completion_review(format);
    let mut prompter = InteractiveCompletionReviewPrompter;

    loop {
        let compilation = exo::router::compile_argv(&spec, &args);
        let Some(invocation) = compilation.invocation else {
            let exit_code = render_compilation_errors(format, &args, &compilation);
            std::process::exit(exit_code);
        };

        let Ok(Some(command_box)) =
            exo::command::registry::build_command_from_invocation(&invocation, &context.root)
        else {
            eprintln!("Unknown command.");
            std::process::exit(1);
        };

        let transport = CliTransport::new(cmd_format)
            .with_workspace_root(context.root.clone())
            .with_project(context.project.clone())
            .with_workflow_confirmation(workflow_confirmation.clone());

        let namespace = args.first().map_or("", String::as_str);
        let operation = args.get(1).map_or("", String::as_str);
        let effect = command_box.effect();
        if let Err(err) = exo::post_write::preflight_sidecar_post_write(
            context.project.as_ref(),
            namespace,
            operation,
            effect,
        ) {
            render_fatal_error(format, Some(&context), err.as_ref());
            std::process::exit(if cmd_format == exo::command::traits::OutputFormat::Json {
                2
            } else {
                1
            });
        }

        let result = exo::command::invoke_command_box_json(&command_box, &transport);
        match result {
            Ok(mut invoke_result) => {
                let post_write_report = if exo::post_write::should_persist_after_success(
                    context.project.as_ref(),
                    namespace,
                    operation,
                    invoke_result.effect,
                ) {
                    exo::post_write::with_sidecar_runtime_lock(context.project.as_ref(), || {
                        exo::post_write::persist_after_success(
                            &context.root,
                            context.project.as_ref(),
                            namespace,
                            operation,
                            invoke_result.effect,
                        )
                    })
                } else {
                    Ok(None)
                };
                let post_write_report = match post_write_report {
                    Ok(report) => report,
                    Err(error) => {
                        render_fatal_error(format, Some(&context), error.as_ref());
                        std::process::exit(
                            if cmd_format == exo::command::traits::OutputFormat::Json {
                                2
                            } else {
                                1
                            },
                        );
                    }
                };
                if let Some(report) = &post_write_report
                    && cmd_format == exo::command::traits::OutputFormat::Json
                {
                    attach_post_write_report(&mut invoke_result.data, report);
                }

                let value = invoke_result.data;
                let rendered = transport.render_value(&value);
                if !rendered.is_empty() {
                    print!("{rendered}");
                    if !rendered.ends_with('\n') {
                        println!();
                    }
                }
                if cmd_format == exo::command::traits::OutputFormat::Human
                    && let Some(report) = &post_write_report
                    && let Some(issue) = post_write_issue(report)
                {
                    eprintln!("Sidecar auto-persist did not complete: {issue}");
                }

                let exit_code = if cmd_format == exo::command::traits::OutputFormat::Json {
                    let structured_ok = value
                        .get("result")
                        .and_then(|v| v.get("ok"))
                        .and_then(serde_json::Value::as_bool);
                    if structured_ok == Some(false) { 2 } else { 0 }
                } else {
                    0
                };

                std::process::exit(exit_code);
            }
            Err(value) => {
                if can_prompt && let Some(review) = completion_review_from_value(&value) {
                    let decision = match handle_completion_review(&review, &mut prompter) {
                        Ok(decision) => decision,
                        Err(exit_code) => std::process::exit(exit_code),
                    };
                    let transition = apply_completion_review_decision(
                        &mut args,
                        &mut workflow_confirmation,
                        decision,
                    );
                    if transition == CliReviewTransition::Redispatch {
                        continue;
                    }
                    std::process::exit(render_pending_review_transition(transition));
                }

                let rendered = transport.render_value(&value);
                if !rendered.is_empty() {
                    if cmd_format == exo::command::traits::OutputFormat::Human {
                        eprint!("{rendered}");
                        if !rendered.ends_with('\n') {
                            eprintln!();
                        }
                    } else {
                        print!("{rendered}");
                        if !rendered.ends_with('\n') {
                            println!();
                        }
                    }
                }

                std::process::exit(if cmd_format == exo::command::traits::OutputFormat::Json {
                    2
                } else {
                    1
                });
            }
        }
    }
}

#[allow(dead_code)]
fn strip_flag_with_value(args: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            args.remove(i);
            if i < args.len() {
                args.remove(i);
            }
            continue;
        }

        let prefix = format!("{flag}=");
        if args[i].starts_with(&prefix) {
            args.remove(i);
            continue;
        }

        i += 1;
    }
}

fn strip_flag(args: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            args.remove(i);
        } else {
            i += 1;
        }
    }
}

enum RunOutcome {
    Human,
}

fn protocol_help_root_steering() -> exo::api::protocol::Steering {
    exo::api::protocol::Steering {
        next_call: exo::api::protocol::NextCall {
            kind: exo::api::protocol::NextCallKind::Help,
            params: serde_json::json!({ "address": { "kind": "root" } }),
        },
        priority: None,
        confidence: None,
        context_note: None,
    }
}

fn run_exohook_validate(
    context: &AgentContext,
    name: Option<&str>,
    format: OutputFormat,
    verbose: bool,
    dry_run: bool,
    color: ValidateColor,
) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    let mut cmd = resolve_exohook_command(&context.root)?;

    cmd.arg("validate");
    if let Some(name) = name {
        cmd.arg(name);
    }
    cmd.arg("--format").arg(exohook_format_arg(format));
    if verbose {
        cmd.arg("--verbose");
    }
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg("--color").arg(color.as_str());

    cmd.current_dir(&context.root);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd.status()?;
    if status.success() {
        Ok(RunOutcome::Human)
    } else {
        let code = status.code().unwrap_or(1);
        Err(format!("exohook validate failed (exit code {code})").into())
    }
}

const fn exohook_format_arg(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Grouped => "grouped",
        OutputFormat::Jsonl => "jsonl",
        OutputFormat::Compact => "compact",
        OutputFormat::Json => "jsonl",
        OutputFormat::Human => "compact",
    }
}

fn resolve_exohook_command(root: &Path) -> Result<Command, Box<dyn std::error::Error>> {
    let release = root.join("target/release/exohook");
    if is_executable(&release) {
        return Ok(Command::new(release));
    }

    let debug = root.join("target/debug/exohook");
    if is_executable(&debug) {
        return Ok(Command::new(debug));
    }

    if find_in_path("exohook").is_some() {
        return Ok(Command::new("exohook"));
    }

    if find_in_path("cargo").is_some() {
        let mut cmd = Command::new("cargo");
        cmd.arg("run").arg("-q").arg("-p").arg("exohook").arg("--");
        return Ok(cmd);
    }

    Err("could not find exohook (or cargo)".into())
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for path in std::env::split_paths(&paths) {
        let candidate = path.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn find_exo_failure<'a>(mut e: &'a (dyn std::error::Error + 'static)) -> Option<&'a ExoFailure> {
    loop {
        if let Some(f) = e.downcast_ref::<ExoFailure>() {
            return Some(f);
        }
        e = e.source()?;
    }
}

fn render_fatal_error(
    format: OutputFormat,
    context: Option<&AgentContext>,
    e: &(dyn std::error::Error + 'static),
) {
    let failure = find_exo_failure(e);

    let causes: Vec<serde_json::Value> = {
        let mut causes = Vec::new();
        let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
        while let Some(err) = cur {
            causes.push(serde_json::json!({ "message": err.to_string() }));
            cur = err.source();
        }
        causes
    };

    match format {
        OutputFormat::Json => {
            let (status, error, steering) = failure.map_or_else(
                || {
                    (
                        Status::Error,
                        ErrorBody {
                            code: ErrorCode::Internal,
                            message: e.to_string(),
                            details: Some(serde_json::json!({ "causes": causes })),
                        },
                        Some(protocol_help_root_steering()),
                    )
                },
                |f| {
                    (
                        f.status,
                        ErrorBody {
                            code: f.error.code,
                            message: f.error.message.clone(),
                            details: merge_json_error_details(
                                f.error.details.clone(),
                                Some(serde_json::json!(f.steering.clone())),
                            ),
                        },
                        None,
                    )
                },
            );

            let response = ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: "cli".to_string(),
                status,
                result: None,
                error: Some(error),
                ticket: None,
                steering,
                reminders: None,
                display: None,
                preview: None,
                effect: None,
                trace: None,
            };

            match serde_json::to_string_pretty(&response) {
                Ok(s) => println!("{s}"),
                Err(err) => eprintln!("Failed to serialize protocol error envelope: {err}"),
            }
        }
        _ => {
            if let Some(failure) = failure {
                eprintln!("{}", failure.error.message);
                eprintln!("\n[Next]");
                for a in failure.steering.next_actions.iter().take(4) {
                    eprintln!("- {}: {}", a.label, a.command);
                    eprintln!("  {}", a.rationale);
                }
                if !failure.steering.repair_actions.is_empty() {
                    eprintln!("\n[Repair]");
                    for a in failure.steering.repair_actions.iter().take(4) {
                        eprintln!("- {}: {}", a.label, a.command);
                        eprintln!("  {}", a.rationale);
                    }
                }
            } else {
                eprintln!("{e}");
                if context.is_some() {
                    eprintln!(
                        "\n[Next]\n- exo map\n  Use map to orient and get suggested next actions."
                    );
                }
            }
        }
    }
}

fn merge_json_error_details(
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

fn emit_verifier_reminders(reminders: &[exo::api::protocol::Reminder]) {
    for r in reminders {
        eprintln!("EXOSUIT VERIFIER ({:?}): {}", r.severity, r.message);
        if let Some(details) = &r.details {
            eprintln!("  details: {details}");
        }
    }
}

fn handle_json_server(
    context: &AgentContext,
    project: Option<&Project>,
    is_direct: bool,
) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    use exo::api;
    use exo::api::protocol::{ErrorBody, ErrorCode, PROTOCOL_VERSION, ResponseEnvelope, Status};
    use std::io::{BufRead, BufReader, Write};

    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in reader.lines() {
        let input = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("json server: stdin read error: {e}");
                break;
            }
        };

        if input.trim().is_empty() {
            continue;
        }

        let reminders = exo::verifiers::run_global_verifiers(&context.root);

        let mut response: ResponseEnvelope =
            match serde_json::from_str::<api::protocol::RequestEnvelope>(&input) {
                Ok(request) if is_direct => {
                    api::handler::handle_request_with_project_and_diagnostics_as_writer(
                        &context.root,
                        project,
                        request,
                        &exo::daemon_diagnostics::DaemonDiagnostics::disabled(),
                    )
                }
                Ok(request) => {
                    api::handler::handle_request_with_project(&context.root, project, request)
                }
                Err(e) => {
                    let id = serde_json::from_str::<serde_json::Value>(&input)
                        .ok()
                        .and_then(|v| {
                            v.get("id")
                                .and_then(|x| x.as_str())
                                .map(std::string::ToString::to_string)
                        })
                        .unwrap_or_else(|| "unknown".to_string());

                    ResponseEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        id,
                        status: Status::Error,
                        result: None,
                        error: Some(ErrorBody {
                            code: ErrorCode::InvalidInput,
                            message: format!("Failed to parse request envelope: {e}"),
                            details: None,
                        }),
                        ticket: None,
                        steering: Some(exo::api::protocol::Steering {
                            next_call: exo::api::protocol::NextCall {
                                kind: exo::api::protocol::NextCallKind::Help,
                                params: serde_json::json!({ "address": { "kind": "root" } }),
                            },
                            priority: None,
                            confidence: None,
                            context_note: None,
                        }),
                        reminders: None,
                        display: None,
                        preview: None,
                        effect: None,
                        trace: None,
                    }
                }
            };

        if !reminders.is_empty() {
            response.reminders = Some(reminders);
        }

        let response_json = match serde_json::to_string(&response) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("json server: failed to serialize response: {e}");
                continue;
            }
        };
        writeln!(stdout_lock, "{response_json}")?;
        stdout_lock.flush()?;
    }

    Ok(RunOutcome::Human)
}

#[cfg(test)]
mod tests {
    use super::*;
    use exo::api::protocol::{CallParams, Op};
    use std::cell::Cell;
    use std::collections::VecDeque;

    fn test_completion_input(outcome: &str) -> WorkflowConfirmationInput {
        WorkflowConfirmationInput {
            kind: "workflow_completion_confirmation".to_string(),
            entity_type: "task".to_string(),
            entity_id: "goal::task".to_string(),
            decision: WorkflowConfirmationDecision::YesComplete,
            outcome: outcome.to_string(),
        }
    }

    fn test_completion_review(outcome: &str) -> CliCompletionReview {
        CliCompletionReview {
            header: "Outcome ready for review".to_string(),
            question: "Approve this outcome?".to_string(),
            message: format!("Proposed outcome: {outcome}"),
            proposed_outcome: outcome.to_string(),
            completion_input: test_completion_input(outcome),
        }
    }

    fn completion_review_json(outcome: &str) -> serde_json::Value {
        let review = test_completion_review(outcome);
        serde_json::json!({
            "header": review.header,
            "question": review.question,
            "message": review.message,
            "proposed_outcome": review.proposed_outcome,
            "completion_input": review.completion_input,
        })
    }

    #[test]
    fn completion_review_parser_accepts_daemon_and_direct_shapes() {
        let workflow = completion_review_json("Done");
        let direct = serde_json::json!({
            "text": "Outcome ready for review",
            "error": { "details": { "workflow_confirmation": workflow.clone() } },
        });
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "review".to_string(),
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: ErrorCode::PreconditionFailed,
                message: "Outcome ready for review".to_string(),
                details: Some(serde_json::json!({ "workflow_confirmation": workflow })),
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Write),
            trace: None,
        };

        assert_eq!(
            completion_review_from_value(&direct),
            completion_review_from_response(&response)
        );
        assert_eq!(
            completion_review_from_value(&direct)
                .expect("review")
                .proposed_outcome,
            "Done"
        );
    }

    #[test]
    fn completion_review_parser_rejects_unbound_or_nonapproval_payloads() {
        let mut wrong_outcome = completion_review_json("Done");
        wrong_outcome["completion_input"]["outcome"] = serde_json::json!("Different");
        let value = serde_json::json!({
            "error": { "details": { "workflow_confirmation": wrong_outcome } },
        });
        assert!(completion_review_from_value(&value).is_none());

        let mut wrong_decision = completion_review_json("Done");
        wrong_decision["completion_input"]["decision"] = serde_json::json!("discuss");
        let value = serde_json::json!({
            "error": { "details": { "workflow_confirmation": wrong_decision } },
        });
        assert!(completion_review_from_value(&value).is_none());
    }

    #[test]
    fn completion_review_requires_human_format_and_three_terminals() {
        assert!(completion_review_is_interactive(
            OutputFormat::Human,
            true,
            true,
            true
        ));
        for (stdin, stdout, stderr) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert!(!completion_review_is_interactive(
                OutputFormat::Human,
                stdin,
                stdout,
                stderr
            ));
        }
        assert!(!completion_review_is_interactive(
            OutputFormat::Json,
            true,
            true,
            true
        ));
    }

    #[test]
    fn replace_log_arg_handles_separate_inline_and_missing_values() {
        let mut separate = vec!["task", "complete", "task", "--log", "Old"]
            .into_iter()
            .map(String::from)
            .collect();
        replace_log_arg(&mut separate, "New".to_string());
        assert_eq!(separate[4], "New");

        let mut inline = vec!["goal", "complete", "goal", "--log=Old"]
            .into_iter()
            .map(String::from)
            .collect();
        replace_log_arg(&mut inline, "New".to_string());
        assert_eq!(inline[3], "--log=New");

        let mut missing = vec!["task", "complete", "task"]
            .into_iter()
            .map(String::from)
            .collect();
        replace_log_arg(&mut missing, "New".to_string());
        assert_eq!(&missing[3..], ["--log", "New"]);
    }

    #[test]
    fn completion_review_transitions_update_redispatch_state() {
        let mut args = vec!["task", "complete", "goal::task", "--log", "Old"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut confirmation = None;
        let approved = test_completion_input("Old");
        assert_eq!(
            apply_completion_review_decision(
                &mut args,
                &mut confirmation,
                CliReviewDecision::Approve(approved.clone())
            ),
            CliReviewTransition::Redispatch
        );
        assert_eq!(confirmation, Some(approved));

        assert_eq!(
            apply_completion_review_decision(
                &mut args,
                &mut confirmation,
                CliReviewDecision::Revise("Revised".to_string())
            ),
            CliReviewTransition::Redispatch
        );
        assert_eq!(args[4], "Revised");
        assert_eq!(confirmation, None);

        for (decision, expected) in [
            (
                CliReviewDecision::KeepWorking,
                CliReviewTransition::KeepWorking,
            ),
            (CliReviewDecision::Discuss, CliReviewTransition::Discuss),
        ] {
            assert_eq!(
                apply_completion_review_decision(&mut args, &mut confirmation, decision),
                expected
            );
            assert_eq!(confirmation, None);
        }
    }

    enum ScriptedReviewStep {
        Decision(CliReviewDecision),
        Interrupted,
    }

    struct ScriptedReviewPrompter {
        steps: VecDeque<ScriptedReviewStep>,
    }

    impl CompletionReviewPrompter for ScriptedReviewPrompter {
        fn prompt(&mut self, _review: &CliCompletionReview) -> std::io::Result<CliReviewDecision> {
            match self.steps.pop_front().expect("scripted review decision") {
                ScriptedReviewStep::Decision(decision) => Ok(decision),
                ScriptedReviewStep::Interrupted => {
                    Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
                }
            }
        }
    }

    #[test]
    fn scripted_review_supports_revision_then_approval_and_interruption() {
        let review = test_completion_review("Old");
        let mut prompter = ScriptedReviewPrompter {
            steps: VecDeque::from([
                ScriptedReviewStep::Decision(CliReviewDecision::Revise("New".to_string())),
                ScriptedReviewStep::Decision(CliReviewDecision::Approve(test_completion_input(
                    "New",
                ))),
                ScriptedReviewStep::Decision(CliReviewDecision::KeepWorking),
                ScriptedReviewStep::Decision(CliReviewDecision::Discuss),
            ]),
        };
        assert_eq!(
            handle_completion_review(&review, &mut prompter),
            Ok(CliReviewDecision::Revise("New".to_string()))
        );
        assert_eq!(
            handle_completion_review(&review, &mut prompter),
            Ok(CliReviewDecision::Approve(test_completion_input("New")))
        );
        assert_eq!(
            handle_completion_review(&review, &mut prompter),
            Ok(CliReviewDecision::KeepWorking)
        );
        assert_eq!(
            handle_completion_review(&review, &mut prompter),
            Ok(CliReviewDecision::Discuss)
        );

        let mut interrupted = ScriptedReviewPrompter {
            steps: VecDeque::from([ScriptedReviewStep::Interrupted]),
        };
        assert_eq!(
            handle_completion_review(&review, &mut interrupted),
            Err(130)
        );
    }

    fn test_daemon_request() -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "test-daemon-request".to_string(),
            op: Op::Call(CallParams {
                address: Address::Operation {
                    path: vec!["task".to_string(), "complete".to_string()],
                },
                input: serde_json::json!({ "id": "goal::task" }),
            }),
            workspace_root: None,
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        }
    }

    fn ok_daemon_response(id: &str) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: id.to_string(),
            status: Status::Ok,
            result: Some(serde_json::json!({ "ok": true })),
            error: None,
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: Some(Effect::Pure),
            trace: None,
        }
    }

    fn eof_error() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed")
    }

    #[test]
    fn daemon_retry_policy_replays_all_effects_with_durable_request_identity() {
        let eof = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed");
        assert!(should_retry_daemon_request(Effect::Pure, &eof));

        let write_eof = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed");
        assert!(should_retry_daemon_request(Effect::Write, &write_eof));

        let exec_eof = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed");
        assert!(should_retry_daemon_request(Effect::Exec, &exec_eof));

        let reset = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        assert!(should_retry_daemon_request(Effect::Pure, &reset));
    }

    #[test]
    fn daemon_eof_repair_retries_pure_requests_once() {
        let request = test_daemon_request();
        let connects = Cell::new(0);
        let sends = Cell::new(0);

        let response = send_daemon_request_with_recovery_using(
            &request,
            Effect::Pure,
            || {
                connects.set(connects.get() + 1);
                Ok(())
            },
            |_, request| {
                let attempt = sends.get();
                sends.set(attempt + 1);
                if attempt == 0 {
                    Err(eof_error())
                } else {
                    Ok(ok_daemon_response(&request.id))
                }
            },
        )
        .expect("pure request should retry after repair");

        assert_eq!(connects.get(), 2, "EOF should trigger daemon repair");
        assert_eq!(sends.get(), 2, "pure request should be replayed once");
        assert_eq!(response.status, Status::Ok);
    }

    #[test]
    fn daemon_eof_repair_replays_write_request_with_same_id() {
        let request = test_daemon_request();
        let connects = Cell::new(0);
        let sends = Cell::new(0);

        let response = send_daemon_request_with_recovery_using(
            &request,
            Effect::Write,
            || {
                connects.set(connects.get() + 1);
                Ok(())
            },
            |_, request| {
                let attempt = sends.get();
                sends.set(attempt + 1);
                if attempt == 0 {
                    Err(eof_error())
                } else {
                    Ok(ok_daemon_response(&request.id))
                }
            },
        )
        .expect("write response should be recovered from durable request id");

        assert_eq!(connects.get(), 2, "EOF should trigger daemon repair");
        assert_eq!(sends.get(), 2, "write request id should be retried once");
        assert_eq!(response.id, request.id);
    }

    #[test]
    fn daemon_eof_repair_replays_exec_request_with_same_id() {
        let request = test_daemon_request();
        let connects = Cell::new(0);
        let sends = Cell::new(0);

        let response = send_daemon_request_with_recovery_using(
            &request,
            Effect::Exec,
            || {
                connects.set(connects.get() + 1);
                Ok(())
            },
            |_, request| {
                let attempt = sends.get();
                sends.set(attempt + 1);
                if attempt == 0 {
                    Err(eof_error())
                } else {
                    Ok(ok_daemon_response(&request.id))
                }
            },
        )
        .expect("exec response should be recovered from durable request id");

        assert_eq!(connects.get(), 2, "EOF should trigger daemon repair");
        assert_eq!(sends.get(), 2, "exec request id should be retried once");
        assert_eq!(response.id, request.id);
    }

    #[test]
    fn daemon_eof_error_guidance_does_not_expose_manual_ensure() {
        let request = test_daemon_request();
        let err = send_daemon_request_with_recovery_using(
            &request,
            Effect::Write,
            || Ok(()),
            |_, _| Err(eof_error()),
        )
        .expect_err("write EOF should fail after repair");

        let lines = daemon_dispatch_error_lines(&err).join("\n");
        assert!(
            !lines.contains("daemon ensure"),
            "EOF guidance should not expose manual daemon lifecycle commands: {lines}"
        );
        assert!(lines.contains("repaired automatically"), "{lines}");
        assert!(lines.contains("Refresh Exo state"), "{lines}");
    }

    #[test]
    fn daemon_eof_error_reports_failed_automatic_repair() {
        let request = test_daemon_request();
        let connects = Cell::new(0);
        let err = send_daemon_request_with_recovery_using(
            &request,
            Effect::Write,
            || {
                let attempt = connects.get();
                connects.set(attempt + 1);
                if attempt == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::other("repair failed"))
                }
            },
            |_, _| Err(eof_error()),
        )
        .expect_err("EOF repair failure should report daemon dispatch error");

        assert_eq!(
            err.repair_error.as_deref(),
            Some("repair failed"),
            "failed automatic repair should be retained"
        );
        assert!(!err.daemon_repaired);
        assert!(err.ambiguous_outcome);
    }

    #[test]
    fn sidecar_bootstrap_context_predicate_is_operation_aware() {
        for operation in [
            "bootstrap",
            "discover",
            "init",
            "link",
            "setup",
            "status",
            "unlink",
        ] {
            assert!(is_sidecar_bootstrap_context_command(&[
                "sidecar".to_string(),
                operation.to_string(),
            ]));
        }

        for operation in ["repo", "unknown"] {
            assert!(!is_sidecar_bootstrap_context_command(&[
                "sidecar".to_string(),
                operation.to_string(),
            ]));
        }

        assert!(!is_sidecar_bootstrap_context_command(&[
            "sidecar".to_string()
        ]));
    }

    #[test]
    fn project_bootstrap_context_predicate_includes_move_root() {
        for operation in [
            "resolve",
            "list",
            "snapshot",
            "repair",
            "repair-apply",
            "move-root",
        ] {
            assert!(is_project_bootstrap_read(&[
                "project".to_string(),
                operation.to_string(),
            ]));
        }

        for operation in ["unknown", "status"] {
            assert!(!is_project_bootstrap_read(&[
                "project".to_string(),
                operation.to_string(),
            ]));
        }
    }

    #[test]
    fn request_context_commands_own_their_single_state_load() {
        assert!(command_loads_request_context(&["status".to_string()]));
        assert!(command_loads_request_context(&[
            "task".to_string(),
            "list".to_string(),
        ]));
        assert!(!command_loads_request_context(&[
            "task".to_string(),
            "start".to_string(),
        ]));
    }
}
