//! Transport-specific formatting and confirmation contracts.
//!
//! This module defines the `TransportContext` trait used by command dispatch
//! to format output and errors for different transports (CLI, JSON, protocol).

use std::path::{Path, PathBuf};

use crate::api::protocol::{Address, Auth, ErrorCode, WorkflowConfirmationInput};
use crate::command::traits::{CommandOutput, OutputFormat};
use crate::project::Project;
use crate::steering::SuggestedAction;
use serde_json::Value as JsonValue;

/// Result of an execution confirmation check.
///
/// This is transport-agnostic. The transport decides how to represent
/// confirmation to the caller (CLI: prompt, Machine: ticket in response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmResult {
    /// Confirmation granted, continue execution.
    Proceed,
    /// Confirmation required; caller should request user confirmation.
    /// The ticket is an opaque string for resuming execution after confirmation.
    NeedConfirm {
        /// Ticket to resume execution once confirmed.
        ticket: String,
    },
    /// Confirmation denied or failed.
    Denied(String),
}

/// Transport-specific output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportOutput {
    /// Human-readable output.
    Text(String),
    /// Structured JSON output.
    Json(serde_json::Value),
    /// Binary output.
    Bytes(Vec<u8>),
}

/// Transport-specific steering output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringOutput {
    /// Human-readable steering output.
    Text(String),
    /// Structured JSON steering output.
    Json(serde_json::Value),
    /// Binary steering output.
    Bytes(Vec<u8>),
}

/// Error type for command execution formatted by transports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    /// Error code for machine transport.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
}

impl CommandError {
    /// Create a new `CommandError` with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: message.into(),
        }
    }

    /// Create a new `CommandError` with the given code and message.
    pub fn with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Context interface for transport-specific formatting and confirmations.
pub trait TransportContext {
    /// Get the workspace root path.
    fn workspace_root(&self) -> Option<&Path>;
    /// Get the resolved project context, if this transport boundary has one.
    fn project(&self) -> Option<&Project> {
        None
    }
    /// Check whether execution should proceed.
    fn confirm_exec(&self, action: &str) -> ConfirmResult;
    /// Output format preference for this transport.
    fn output_format(&self) -> OutputFormat {
        OutputFormat::Json
    }
    /// Agent session identity from the request envelope (None = CLI/sidebar).
    fn agent_id(&self) -> Option<&str> {
        None
    }
    fn workflow_confirmation(&self) -> Option<&WorkflowConfirmationInput> {
        None
    }
    /// Whether semantic command error codes should be exposed to the caller.
    fn preserves_error_codes(&self) -> bool {
        false
    }
    /// Format command output for the transport.
    fn format_output(&self, output: CommandOutput) -> TransportOutput;
    /// Format command error for the transport.
    fn format_error(&self, error: CommandError) -> TransportOutput;
    /// Render steering suggestions for the transport.
    fn render_steering(&self, suggestions: Vec<SuggestedAction>) -> SteeringOutput;
}

/// Generate a confirmation ticket for an exec-effect call.
pub fn ticket_for_exec_call(address: &Address, input: &JsonValue) -> String {
    let payload = serde_json::json!({
        "address": address,
        "input": input,
    });

    let digest = blake3::hash(payload.to_string().as_bytes());
    format!("blake3:{digest}")
}

fn generate_exec_ticket(action: &str) -> String {
    let digest = blake3::hash(action.as_bytes());
    format!("blake3:{digest}")
}

/// Transport context for machine-channel requests.
#[derive(Debug, Clone)]
pub struct MachineChannelTransport<'a> {
    /// Workspace root path.
    pub workspace_root: &'a Path,
    /// Resolved project context for this request, if available.
    pub project: Option<&'a Project>,
    /// Request ID for correlation.
    pub request_id: String,
    /// Authentication/confirmation state.
    pub auth: Option<Auth>,
    /// Expected confirmation ticket for exec calls.
    pub expected_ticket: Option<String>,
    /// Agent session identity from request envelope (None = CLI/sidebar).
    pub agent_id: Option<String>,
    pub workflow_confirmation: Option<WorkflowConfirmationInput>,
}

/// Transport context for CLI requests.
#[derive(Debug, Clone)]
pub struct CliTransport {
    /// Output format preference.
    pub format: OutputFormat,
    /// Explicit workspace root for direct CLI dispatch.
    pub workspace_root: Option<PathBuf>,
    /// Resolved project context for direct CLI dispatch, if available.
    pub project: Option<Project>,
    /// Machine-only completion confirmation carried by recovery CLI calls.
    pub workflow_confirmation: Option<WorkflowConfirmationInput>,
}

impl CliTransport {
    pub const fn new(format: OutputFormat) -> Self {
        Self {
            format,
            workspace_root: None,
            project: None,
            workflow_confirmation: None,
        }
    }

    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    pub fn with_project(mut self, project: Option<Project>) -> Self {
        self.project = project;
        self
    }

    pub fn with_workflow_confirmation(
        mut self,
        workflow_confirmation: Option<WorkflowConfirmationInput>,
    ) -> Self {
        self.workflow_confirmation = workflow_confirmation;
        self
    }

    pub fn render_value(&self, value: &serde_json::Value) -> String {
        match self.format {
            OutputFormat::Json => serde_json::to_string_pretty(value).unwrap_or_default(),
            OutputFormat::Human => render_human_value(value),
        }
    }
}

fn render_human_value(value: &serde_json::Value) -> String {
    if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
        let mut out = text.to_string();

        if let Some(steering_text) = value
            .get("steering")
            .and_then(|steering| steering.get("text"))
            .and_then(serde_json::Value::as_str)
            && !steering_text.trim().is_empty()
        {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(steering_text);
        }

        return out;
    }

    serde_json::to_string_pretty(value).unwrap_or_default()
}

impl TransportContext for MachineChannelTransport<'_> {
    fn workspace_root(&self) -> Option<&Path> {
        Some(self.workspace_root)
    }

    fn project(&self) -> Option<&Project> {
        self.project
    }

    fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    fn workflow_confirmation(&self) -> Option<&WorkflowConfirmationInput> {
        self.workflow_confirmation.as_ref()
    }

    fn preserves_error_codes(&self) -> bool {
        true
    }

    fn confirm_exec(&self, action: &str) -> ConfirmResult {
        let ticket = self
            .expected_ticket
            .clone()
            .unwrap_or_else(|| generate_exec_ticket(action));

        match &self.auth {
            Some(auth) if auth.confirm => {
                if auth.ticket == ticket {
                    ConfirmResult::Proceed
                } else {
                    ConfirmResult::Denied("Invalid confirmation ticket".to_string())
                }
            }
            _ => ConfirmResult::NeedConfirm { ticket },
        }
    }

    fn format_output(&self, output: CommandOutput) -> TransportOutput {
        TransportOutput::Json(output.data)
    }

    fn format_error(&self, error: CommandError) -> TransportOutput {
        TransportOutput::Json(serde_json::json!({
            "error": {
                "code": error.code(),
                "message": error.message(),
            }
        }))
    }

    fn render_steering(&self, suggestions: Vec<SuggestedAction>) -> SteeringOutput {
        let rendered = suggestions
            .into_iter()
            .filter_map(|suggestion| {
                let value = serde_json::to_value(&suggestion).ok()?;
                let tool = value.get("tool")?.clone();
                let tool_args = value.get("tool_args")?.clone();
                Some(serde_json::json!({
                    "tool": tool,
                    "tool_args": tool_args,
                }))
            })
            .collect::<Vec<_>>();

        SteeringOutput::Json(serde_json::json!({
            "suggestions": rendered,
        }))
    }
}

impl TransportContext for CliTransport {
    fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    fn workflow_confirmation(&self) -> Option<&WorkflowConfirmationInput> {
        self.workflow_confirmation.as_ref()
    }

    fn preserves_error_codes(&self) -> bool {
        self.format == OutputFormat::Json
    }

    fn confirm_exec(&self, _action: &str) -> ConfirmResult {
        ConfirmResult::Proceed
    }

    fn output_format(&self) -> OutputFormat {
        self.format
    }

    fn format_output(&self, output: CommandOutput) -> TransportOutput {
        match self.format {
            OutputFormat::Json => {
                let envelope = crate::api::protocol::ResponseEnvelope {
                    protocol_version: crate::api::protocol::PROTOCOL_VERSION,
                    id: "cli".to_string(),
                    status: crate::api::protocol::Status::Ok,
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
                TransportOutput::Json(serde_json::to_value(envelope).unwrap_or_default())
            }
            OutputFormat::Human => {
                if let Some(msg) = output.human_message {
                    TransportOutput::Text(msg)
                } else if !output.data.is_null() {
                    TransportOutput::Text(
                        serde_json::to_string_pretty(&output.data).unwrap_or_default(),
                    )
                } else {
                    TransportOutput::Text(String::new())
                }
            }
        }
    }

    fn format_error(&self, error: CommandError) -> TransportOutput {
        match self.format {
            OutputFormat::Json => {
                let envelope = crate::api::protocol::ResponseEnvelope {
                    protocol_version: crate::api::protocol::PROTOCOL_VERSION,
                    id: "cli".to_string(),
                    status: crate::api::protocol::Status::Error,
                    result: None,
                    error: Some(crate::api::protocol::ErrorBody {
                        code: error.code(),
                        message: error.message,
                        details: None,
                    }),
                    ticket: None,
                    steering: None,
                    reminders: None,
                    display: None,
                    preview: None,
                    effect: None,
                    trace: None,
                };
                TransportOutput::Json(serde_json::to_value(envelope).unwrap_or_default())
            }
            OutputFormat::Human => TransportOutput::Text(error.message),
        }
    }

    fn render_steering(&self, suggestions: Vec<SuggestedAction>) -> SteeringOutput {
        match self.format {
            OutputFormat::Json => {
                SteeringOutput::Json(serde_json::to_value(suggestions).unwrap_or_default())
            }
            OutputFormat::Human => {
                if suggestions.is_empty() {
                    return SteeringOutput::Text(String::new());
                }

                let mut out = String::from("\n[Next]\n");
                for suggestion in suggestions.iter().take(4) {
                    out.push_str(&format!(
                        "- {}: {}\n  {}\n",
                        suggestion.label, suggestion.command, suggestion.rationale
                    ));
                }

                SteeringOutput::Text(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Auth;

    struct TestTransport;

    impl TransportContext for TestTransport {
        fn workspace_root(&self) -> Option<&Path> {
            None
        }

        fn confirm_exec(&self, action: &str) -> ConfirmResult {
            ConfirmResult::NeedConfirm {
                ticket: format!("ticket-{action}"),
            }
        }

        fn format_output(&self, output: CommandOutput) -> TransportOutput {
            TransportOutput::Json(output.data)
        }

        fn format_error(&self, error: CommandError) -> TransportOutput {
            TransportOutput::Text(error.message)
        }

        fn render_steering(&self, suggestions: Vec<SuggestedAction>) -> SteeringOutput {
            SteeringOutput::Json(serde_json::to_value(suggestions).unwrap())
        }
    }

    #[test]
    fn transport_context_contract() {
        let transport = TestTransport;

        // Test confirm_exec returns ticket for confirmation
        let confirm = transport.confirm_exec("run");
        match confirm {
            ConfirmResult::NeedConfirm { ticket } => {
                assert_eq!(ticket, "ticket-run");
            }
            _ => panic!("expected NeedConfirm"),
        }

        // Test Proceed variant
        assert_eq!(ConfirmResult::Proceed, ConfirmResult::Proceed);

        // Test Denied variant
        assert_eq!(
            ConfirmResult::Denied("nope".to_string()),
            ConfirmResult::Denied("nope".to_string())
        );

        // Test format_output
        let output = CommandOutput::message("hello");
        assert_eq!(
            transport.format_output(output),
            TransportOutput::Json(serde_json::Value::Null)
        );

        // Test format_error
        let error = CommandError::new("boom");
        assert_eq!(
            transport.format_error(error),
            TransportOutput::Text("boom".to_string())
        );

        // Test render_steering
        let steering_output = transport.render_steering(vec![]);
        assert_eq!(steering_output, SteeringOutput::Json(serde_json::json!([])));
    }

    #[test]
    fn machine_channel_confirm_exec_checks_ticket() {
        let action = "run";
        let expected_ticket = generate_exec_ticket(action);
        let test_root = std::path::PathBuf::from("/test");

        let transport = MachineChannelTransport {
            workspace_root: &test_root,
            project: None,
            request_id: "req-1".to_string(),
            auth: Some(Auth {
                ticket: expected_ticket.clone(),
                confirm: true,
            }),
            expected_ticket: None,
            agent_id: None,
            workflow_confirmation: None,
        };

        assert_eq!(transport.confirm_exec(action), ConfirmResult::Proceed);

        let transport = MachineChannelTransport {
            workspace_root: &test_root,
            project: None,
            request_id: "req-2".to_string(),
            auth: Some(Auth {
                ticket: "bad-ticket".to_string(),
                confirm: true,
            }),
            expected_ticket: Some(expected_ticket.clone()),
            agent_id: None,
            workflow_confirmation: None,
        };

        assert_eq!(
            transport.confirm_exec(action),
            ConfirmResult::Denied("Invalid confirmation ticket".to_string())
        );

        let transport = MachineChannelTransport {
            workspace_root: &test_root,
            project: None,
            request_id: "req-3".to_string(),
            auth: Some(Auth {
                ticket: expected_ticket,
                confirm: false,
            }),
            expected_ticket: None,
            agent_id: None,
            workflow_confirmation: None,
        };

        assert_eq!(
            transport.confirm_exec(action),
            ConfirmResult::NeedConfirm {
                ticket: generate_exec_ticket(action)
            }
        );
    }
}
