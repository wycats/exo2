use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    NeedsInput,
    ConfirmRequired,
    Error,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Pure,
    Write,
    Exec,
}

/// Recovery behavior for a built command after daemon replacement.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryClass {
    /// The request can be executed again because it cannot mutate state.
    ReplayableRead,
    /// State and the replayable core response commit in one project transaction.
    AtomicProjectState,
    /// The request may perform effects outside the canonical SQLite transaction.
    ExternalAtMostOnce,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownAddress,
    UnknownListKind,
    InvalidInput,
    /// Unknown namespace or operation in a call.
    UnknownCommand,
    /// A required argument was not provided.
    MissingArg,
    /// An argument value has the wrong type (e.g., string where int expected).
    TypeMismatch,
    MissingTicket,
    TicketInvalid,
    ConfirmRequired,
    NotFound,
    Internal,
    VersionMismatch,
    /// Operation blocked because a precondition is not met (e.g., upgrade required)
    PreconditionFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Address {
    Root,
    Namespace { path: Vec<String> },
    Operation { path: Vec<String> },
}

impl Address {
    pub const fn root() -> Self {
        Self::Root
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    pub ticket: String,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowConfirmationDecision {
    YesComplete,
    ReviseOutcome,
    NotCompleteYet,
    Discuss,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowConfirmationInput {
    /// Workflow confirmation kind.
    ///
    /// Canonical value: `workflow_completion_confirmation`.
    /// The legacy alias `outcome_review` is accepted defensively by command
    /// matchers, but producers must emit the canonical kind.
    pub kind: String,
    pub entity_type: String,
    pub entity_id: String,
    pub decision: WorkflowConfirmationDecision,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "params", rename_all = "snake_case")]
pub enum Op {
    Help(HelpParams),
    List(ListParams),
    Call(CallParams),
    Preview(CallParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelpParams {
    pub address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    pub address: Address,
    pub kind: String,
    pub page: Page,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallParams {
    pub address: Address,
    pub input: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

const fn default_limit() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub protocol_version: u32,
    pub id: String,
    pub op: Op,
    /// Canonical workspace root for the caller issuing this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_confirmation: Option<WorkflowConfirmationInput>,
    /// Agent session identity (chatSessionResource URI). NULL for sidebar/CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextCall {
    pub kind: NextCallKind,
    pub params: JsonValue,
}

/// Priority level for steering suggestions.
/// Higher priority actions should be addressed before lower priority ones.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Operation must complete before any other action (e.g., critical upgrades)
    Blocking,
    /// High priority but not blocking
    High,
    /// Normal priority
    Normal,
    /// Low priority
    Low,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NextCallKind {
    Help,
    List,
    Call,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Steering {
    pub next_call: NextCall,
    /// Priority level for this steering suggestion
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Confidence level (0.0-1.0) for this suggestion
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Additional context explaining the steering suggestion
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_note: Option<String>,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    /// Stable identifier for the reminder/verifier producing this message.
    pub kind: String,
    pub severity: ReminderSeverity,
    /// Human-readable guidance intended to be understandable to both humans and AIs.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<JsonValue>,
}

/// Display metadata for UI rendering.
///
/// When present, UI consumers should use these fields instead of
/// attempting to format `result` themselves. The server generates
/// display metadata because it has full knowledge of the command,
/// its arguments, and the result shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Display {
    /// Short message shown while the operation is running
    /// (e.g., "Listing tasks in active phase...")
    pub invocation_message: String,
    /// One-line summary of the result (e.g., "3 tasks in active phase")
    pub summary: String,
    /// Full human-readable body (markdown). If absent, summary is the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub protocol_version: u32,
    pub id: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steering: Option<Steering>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminders: Option<Vec<Reminder>>,
    /// Display metadata for UI rendering. Present on successful responses
    /// from the machine channel. Absent on errors and non-machine transports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    /// Preview-only display metadata. Present on `Op::Preview` responses.
    /// Contains just the invocation message (pre-execution title).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewDisplay>,
    /// The command's declared effect. Lets the extension distinguish
    /// reads from writes for reactive invalidation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    /// Reactive trace captured during command execution.
    /// The extension holds this as an opaque token and sends it back
    /// to `validate_trace` to check if cached data is still current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<JsonValue>,
}

/// Preview-specific display metadata returned by `Op::Preview`.
///
/// Contains only pre-execution information (no summary/body which
/// depend on execution results).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewDisplay {
    /// Message shown while the operation is running
    /// (e.g., "Completing task 'fix-bug' (Fix the parser edge case)")
    pub invocation_message: String,
    /// Past-tense message shown after the operation completes.
    /// (e.g., "Completed task 'fix-bug' (Fix the parser edge case)")
    ///
    /// Used for the `pastTenseMessage` field in VS Code's `PreparedToolInvocation`.
    /// This is behind the `chatParticipantPrivate` proposed API — only available
    /// in VS Code Insiders with the proposed API flag enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub past_tense_message: Option<String>,
    /// Optional confirmation dialog for destructive operations.
    /// When present, VS Code shows a confirmation dialog before executing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationInfo>,
}

/// Confirmation dialog metadata for destructive operations.
///
/// When returned in a preview response, VS Code shows a confirmation dialog
/// with the title and message. If the user cancels, the command is not executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationInfo {
    /// Short title for the confirmation dialog
    pub title: String,
    /// Longer explanation of what this action does and why confirmation is needed
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelpNamespace {
    pub path: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelpOperation {
    pub path: String,
    pub effect: Effect,
    pub summary: String,
    /// Argument specifications for this operation (RFC 10169 help system).
    /// Empty for namespace-level help; populated for operation-level help.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<crate::command::command_spec::ArgSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelpResult {
    pub title: String,
    pub summary: String,
    pub namespaces: Vec<HelpNamespace>,
    pub operations: Vec<HelpOperation>,
    pub next_calls: Vec<NextCall>,
}
