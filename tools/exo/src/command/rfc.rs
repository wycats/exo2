//! RFC namespace commands.
//!
//! - `rfc list`: List RFCs with optional stage filter (Pure)
//! - `rfc show`: Show RFC details by ID (Pure)
//! - `rfc status`: Show RFC status grouped by stage (Pure)
//! - `rfc pipeline`: Show RFC pipeline for the active phase (Pure)
//! - `rfc create`: Create a new RFC (Write)
//! - `rfc edit`: Edit an existing RFC (Write)
//! - `rfc rename`: Rename RFC file to match title slug (Write)
//! - `rfc repair`: Repair RFC filename identity and metadata path drift (Write)
//! - `rfc promote`: Promote RFC to the specified next stage (Write)
//! - `rfc supersede`: Mark RFC as superseded (Write)
//! - `rfc withdraw`: Withdraw an RFC (Write)

use std::path::Path;

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::AgentContext;
use crate::context::sqlite_loader::{RfcRecord, SqliteLoader};
use crate::failure::ExoFailure;
use crate::rfc;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for RFC commands.
fn default_rfc_steering() -> Vec<SuggestedAction> {
    vec![
        SuggestedAction {
            label: "List RFCs".to_string(),
            command: "exo rfc list".to_string(),
            rationale: "View all RFCs in the project.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.6),
        },
        SuggestedAction {
            label: "Show RFC status".to_string(),
            command: "exo rfc status".to_string(),
            rationale: "See RFCs grouped by stage.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.5),
        },
    ]
}

/// Helper to get RFC root path
fn rfc_root(root: &Path) -> std::path::PathBuf {
    root.join("docs/rfcs")
}

fn format_rfc_number(number: i64) -> String {
    format!("{number:05}")
}

fn parse_rfc_number(id: &str) -> ExoResult<i64> {
    parse_rfc_number_with_failure(id, invalid_rfc_id_failure)
}

fn parse_rfc_number_with_failure(
    id: &str,
    invalid_failure: fn(&str) -> anyhow::Error,
) -> ExoResult<i64> {
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(invalid_failure(id));
    }
    id.parse::<i64>().map_err(|_| invalid_failure(id))
}

fn rfc_list_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "List RFCs".to_string(),
        command: "exo rfc list".to_string(),
        rationale: "Find a real RFC ID before attempting RFC promotion.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(1.0),
    }]
}

fn rfc_promote_error_details(id: &str) -> serde_json::Value {
    serde_json::json!({
        "operation": "rfc.promote",
        "rfc_id": id,
        "mutation_performed": false,
        "safe_next": "exo rfc list"
    })
}

fn invalid_rfc_id_failure(id: &str) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::InvalidInput,
            format!("Invalid RFC ID '{id}'. Expected a numeric RFC number."),
            ExoFailure::orienting_steering(rfc_list_steering()),
        )
        .with_details(rfc_promote_error_details(id)),
    )
}

fn invalid_rfc_supersede_id_failure(id: &str) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::InvalidInput,
            format!("Invalid RFC ID '{id}'. Expected a numeric RFC number."),
            ExoFailure::orienting_steering(rfc_list_steering()),
        )
        .with_details(serde_json::json!({
            "operation": "rfc.supersede",
            "superseded_by": id,
            "mutation_performed": false,
            "safe_next": "exo rfc list"
        })),
    )
}

fn rfc_not_found_failure(id: &str) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::NotFound,
            format!("RFC {id} not found. Use `exo rfc list` to see available RFCs."),
            ExoFailure::orienting_steering(rfc_list_steering()),
        )
        .with_details(rfc_promote_error_details(id)),
    )
}

fn rfc_promote_stage_mismatch_failure(
    id: &str,
    target_stage: u8,
    new_stage: u8,
    old_stage: u8,
) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Refusing to promote RFC {id}: target stage {target_stage} does not match next stage {new_stage} (current stage {old_stage}). Re-read the RFC status and retry only after explicit approval."
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Check RFC status".to_string(),
                command: "exo rfc status".to_string(),
                rationale: "Confirm the RFC's current stage before retrying promotion.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(1.0),
            }]),
        )
        .with_details(serde_json::json!({
            "operation": "rfc.promote",
            "rfc_id": id,
            "target_stage": target_stage,
            "expected_next_stage": new_stage,
            "current_stage": old_stage,
            "mutation_performed": false,
            "safe_next": "exo rfc status"
        })),
    )
}

fn rfc_promote_already_stable_failure(id: &str, old_stage: u8) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!("Refusing to promote RFC {id}: RFC is already at Stage 4 (Stable)."),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Check RFC status".to_string(),
                command: "exo rfc status".to_string(),
                rationale: "Confirm the RFC's current stage before retrying promotion.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(1.0),
            }]),
        )
        .with_details(serde_json::json!({
            "operation": "rfc.promote",
            "rfc_id": id,
            "current_stage": old_stage,
            "mutation_performed": false,
            "safe_next": "exo rfc status"
        })),
    )
}

fn rfc_promote_lifecycle_failure(id: &str, read_status: &str) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Refusing to promote RFC {id}: RFC is {} and is not active stage work.",
                status_label(read_status)
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Check RFC status".to_string(),
                command: "exo rfc status".to_string(),
                rationale: "Confirm the RFC lifecycle state before changing stages.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(1.0),
            }]),
        )
        .with_details(serde_json::json!({
            "operation": "rfc.promote",
            "rfc_id": id,
            "status": read_status,
            "mutation_performed": false,
            "safe_next": "exo rfc status"
        })),
    )
}

fn rfc_promote_repair_required_failure(
    id: &str,
    candidate: &rfc::RfcRepairCandidate,
) -> anyhow::Error {
    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Refusing to promote RFC {id}: RFC identity repair is required first. Run `exo rfc repair {}` before retrying promotion.",
                candidate.id
            ),
            ExoFailure::orienting_steering(vec![
                SuggestedAction {
                    label: "Repair RFC identity".to_string(),
                    command: format!("exo rfc repair {}", candidate.id),
                    rationale:
                        "Repair the RFC filename/metadata identity before applying lifecycle writes."
                            .to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(1.0),
                },
                SuggestedAction {
                    label: "Check RFC status".to_string(),
                    command: "exo rfc status".to_string(),
                    rationale: "Review outstanding RFC repair debt.".to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.9),
                },
            ]),
        )
        .with_details(serde_json::json!({
            "operation": "rfc.promote",
            "rfc_id": id,
            "repair_id": candidate.id,
            "current_path": candidate.current_path,
            "expected_path": candidate.expected_path,
            "reasons": &candidate.reasons,
            "mutation_performed": false,
            "safe_next": format!("exo rfc repair {}", candidate.id)
        })),
    )
}

fn display_feature(feature: Option<&str>) -> String {
    feature.unwrap_or("Unknown").to_string()
}

fn display_filename(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(file_path)
        .to_string()
}

fn rfc_list_entry_from_record(record: &RfcRecord) -> RfcListEntry {
    RfcListEntry {
        id: format_rfc_number(record.rfc_number),
        title: record.title.clone(),
        stage: record.stage,
        status: rfc_read_status(record).to_string(),
        feature: display_feature(record.feature.as_deref()),
        filename: display_filename(&record.file_path),
        document_source: "canonical".to_string(),
        workspace_presence: "unknown".to_string(),
        canonical_presence: "present".to_string(),
        differs_from_canonical: false,
    }
}

fn rfc_list_entry_from_effective(record: &rfc::EffectiveRfcRecord) -> RfcListEntry {
    let mut entry = rfc_list_entry_from_record(&record.record);
    entry.document_source = record.provenance.document_source.clone();
    entry.workspace_presence = record.provenance.workspace_presence.clone();
    entry.canonical_presence = record.provenance.canonical_presence.clone();
    entry.differs_from_canonical = record.provenance.differs_from_canonical;
    entry
}

fn status_label(status: &str) -> String {
    let mut chars = status.chars();
    let Some(first) = chars.next() else {
        return "Unknown".to_string();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn is_active_rfc(record: &RfcRecord) -> bool {
    record.status == "active"
}

fn is_active_stage_rfc(record: &RfcRecord) -> bool {
    is_active_rfc(record) && record.superseded_by.is_none()
}

fn rfc_read_status(record: &RfcRecord) -> &str {
    if is_active_rfc(record) && record.superseded_by.is_some() {
        "superseded"
    } else {
        &record.status
    }
}

fn rfc_matches_stage_filter(record: &RfcRecord, filter: Option<u8>) -> bool {
    match filter {
        Some(stage) => is_active_stage_rfc(record) && record.stage == stage,
        None => true,
    }
}

// ============================================================================
// ExoSpec definition — single source of truth for the rfc namespace
// ============================================================================

/// RFC namespace command specification.
///
/// This enum is the authoritative definition of the rfc namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `RfcCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "rfc", description = "RFC management commands")]
pub enum RfcCommands {
    #[exo(effect = "pure", description = "List RFCs with optional stage filter")]
    List {
        #[exo(long, optional, description = "Filter by RFC stage (0-4)")]
        stage: Option<i64>,
    },

    #[exo(effect = "pure", description = "Show RFC details by ID")]
    Show {
        #[exo(positional, description = "The RFC ID to show")]
        id: String,
    },

    #[exo(effect = "pure", description = "Show RFC status grouped by stage")]
    Status,

    #[exo(effect = "write", upgrade_gate, description = "Create a new RFC")]
    Create {
        #[exo(positional, description = "The RFC title")]
        title: String,
        #[exo(
            long,
            optional,
            description = "Custom RFC ID (auto-generated if omitted)"
        )]
        id: Option<String>,
        #[exo(long, default = "unspecified", description = "Feature category")]
        feature: String,
        #[exo(long, default = "0", description = "Initial stage (0-4)")]
        stage: i64,
        #[exo(long, optional, description = "RFC body content")]
        body: Option<String>,
        #[exo(flag, description = "Open the RFC in an editor after creation")]
        open: bool,
    },

    #[exo(effect = "write", upgrade_gate, description = "Edit an existing RFC")]
    Edit {
        #[exo(positional, optional, description = "RFC ID to edit")]
        id: Option<String>,
        #[exo(long, optional, description = "RFC file path to edit")]
        path: Option<String>,
        #[exo(long, optional, description = "New title")]
        title: Option<String>,
        #[exo(long, optional, description = "New feature category")]
        feature: Option<String>,
        #[exo(long, optional, description = "New stage (0-4)")]
        stage: Option<i64>,
        #[exo(long, optional, description = "New body content")]
        body: Option<String>,
        #[exo(long, optional, description = "Read body content from file")]
        body_file: Option<String>,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Rename RFC file to match title slug"
    )]
    Rename {
        #[exo(positional, description = "The RFC ID to rename")]
        id: String,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Repair RFC filename identity and metadata path drift"
    )]
    Repair {
        #[exo(positional, description = "The RFC ID to repair")]
        id: String,
        #[exo(long, optional, description = "RFC file path to repair")]
        path: Option<String>,
        #[exo(long, optional, description = "Assign a new numeric RFC ID")]
        renumber_to: Option<String>,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Promote RFC to the specified next stage"
    )]
    Promote {
        #[exo(positional, description = "The RFC ID to promote")]
        id: String,
        #[exo(
            long,
            description = "Required target stage. Must equal the RFC's current stage plus one."
        )]
        stage: i64,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Mark RFC as superseded by another"
    )]
    Supersede {
        #[exo(positional, optional, description = "The RFC ID to supersede")]
        id: Option<String>,
        #[exo(long, description = "The RFC ID that supersedes this one")]
        by: String,
        #[exo(long, optional, description = "The RFC file path to supersede")]
        path: Option<String>,
    },

    #[exo(effect = "write", upgrade_gate, description = "Withdraw an RFC")]
    Withdraw {
        #[exo(positional, description = "The RFC ID to withdraw")]
        id: String,
        #[exo(long, optional, description = "Optional reason for withdrawal")]
        reason: Option<String>,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Archive a shipped-then-superseded RFC"
    )]
    Archive {
        #[exo(positional, description = "The RFC ID to archive")]
        id: String,
        #[exo(
            long,
            optional,
            description = "Optional reason for archiving (e.g., superseded by RFC XXXXX)"
        )]
        reason: Option<String>,
    },

    #[exo(
        effect = "pure",
        description = "Show RFC pipeline for the active phase"
    )]
    Pipeline,
}

impl RfcCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    ///
    /// Takes `root` to resolve file-based arguments (e.g., `--body-file`).
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::List { stage } => {
                let stage = stage.and_then(|value| u8::try_from(value).ok());
                CommandBox::pure(RfcList::new(stage))
            }
            Self::Show { id } => CommandBox::pure(RfcShow::new(id)),
            Self::Status => CommandBox::pure(RfcStatus::new()),
            Self::Create {
                title,
                id,
                feature,
                stage,
                body,
                open,
            } => {
                let stage = u8::try_from(stage).unwrap_or(0);
                CommandBox::mutable(RfcCreate::new(title, id, feature, stage, body, open))
            }
            Self::Edit {
                id,
                path,
                title,
                feature,
                stage,
                body,
                body_file,
            } => {
                // Resolve body from file if provided, otherwise use direct value
                let body = if let Some(file_path) = body_file {
                    Some(crate::utils::read_text_input(root, &file_path)?)
                } else {
                    body
                };
                let stage = stage.and_then(|value| u8::try_from(value).ok());
                CommandBox::mutable(RfcEdit::new(id, path, title, feature, stage, body))
            }
            Self::Rename { id } => CommandBox::mutable(RfcRename::new(id)),
            Self::Repair {
                id,
                path,
                renumber_to,
            } => CommandBox::mutable(RfcRepair::with_options(id, path, renumber_to)),
            Self::Promote { id, stage } => {
                let target_stage = u8::try_from(stage).map_err(|_| {
                    anyhow::anyhow!("Invalid target stage '{stage}'. Expected 0-4.")
                })?;
                CommandBox::mutable(RfcPromote::new(id, target_stage))
            }
            Self::Supersede { id, by, path } => {
                CommandBox::mutable(RfcSupersede::new(id, by, path))
            }
            Self::Withdraw { id, reason } => CommandBox::mutable(RfcWithdraw::new(id, reason)),
            Self::Archive { id, reason } => CommandBox::mutable(RfcArchive::new(id, reason)),
            Self::Pipeline => CommandBox::pure(RfcPipeline),
        })
    }
}

// ============================================================================
// rfc list
// ============================================================================

/// List RFCs with optional stage filter.
#[derive(Debug, Clone, Copy)]
pub struct RfcList {
    pub stage: Option<u8>,
}

impl RfcList {
    pub const fn new(stage: Option<u8>) -> Self {
        Self { stage }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RfcListEntry {
    id: String,
    title: String,
    stage: u8,
    status: String,
    feature: String,
    filename: String,
    document_source: String,
    workspace_presence: String,
    canonical_presence: String,
    differs_from_canonical: bool,
}

#[derive(Debug, Serialize)]
struct RfcListOutput {
    kind: &'static str,
    ok: bool,
    rfcs: Vec<RfcListEntry>,
    count: usize,
}

impl Command for RfcList {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List RFCs with optional stage filter"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let all_rfcs = rfc::observe_effective_rfcs(ctx.root, ctx.project)?;

        let filtered: Vec<_> = match self.stage {
            Some(s) => all_rfcs
                .into_iter()
                .filter(|r| rfc_matches_stage_filter(&r.record, Some(s)))
                .collect(),
            None => all_rfcs,
        };

        let entries: Vec<RfcListEntry> =
            filtered.iter().map(rfc_list_entry_from_effective).collect();

        let count = entries.len();
        let output = RfcListOutput {
            kind: "rfc.list",
            ok: true,
            rfcs: entries.clone(),
            count,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if entries.is_empty() {
                    let msg = match self.stage {
                        Some(s) => format!("No RFCs found at stage {s}."),
                        None => "No RFCs found.".to_string(),
                    };
                    Ok(CommandOutput::new(output, msg))
                } else {
                    let mut msg = String::from("| Stage | ID | Title | Feature |\n");
                    msg.push_str("| :---: | :--- | :--- | :--- |\n");
                    for entry in &entries {
                        msg.push_str(&format!(
                            "| {} | {} | {} | {} |\n",
                            if entry.status == "active" {
                                render_stage_dots(entry.stage)
                            } else {
                                status_label(&entry.status)
                            },
                            entry.id,
                            entry.title,
                            entry.feature
                        ));
                    }
                    msg.push_str(&format!("\n*{count} RFC(s) found.*"));
                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

// ============================================================================
// rfc show
// ============================================================================

/// Show RFC details by ID.
#[derive(Debug, Clone)]
pub struct RfcShow {
    pub id: String,
}

impl RfcShow {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct RfcShowOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    title: String,
    stage: u8,
    status: String,
    feature: String,
    filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    supersedes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    withdrawal_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    archived_reason: Option<String>,
    document_source: String,
    workspace_presence: String,
    canonical_presence: String,
    differs_from_canonical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_head: Option<String>,
}

impl Command for RfcShow {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "show"
    }

    fn description(&self) -> &'static str {
        "Show RFC details by ID"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "List RFCs".to_string(),
                command: "exo rfc list".to_string(),
                rationale: "View all RFCs to find valid IDs.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Show RFC status".to_string(),
                command: "exo rfc status".to_string(),
                rationale: "See RFCs grouped by stage.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.5),
            },
        ]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let rfc_number = parse_rfc_number(&self.id)?;
        let effective = rfc::observe_effective_rfc_by_number(ctx.root, ctx.project, rfc_number)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "RFC {} not found. Use `exo rfc list` to see available RFCs.",
                    self.id
                )
            })?;
        let rfc_info = &effective.record;

        let feature = display_feature(rfc_info.feature.as_deref());
        let filename = display_filename(&rfc_info.file_path);
        let formatted_id = format_rfc_number(rfc_info.rfc_number);
        let read_status = rfc_read_status(rfc_info).to_string();

        let output = RfcShowOutput {
            kind: "rfc.show",
            ok: true,
            id: formatted_id.clone(),
            title: rfc_info.title.clone(),
            stage: rfc_info.stage,
            status: read_status.clone(),
            feature: feature.clone(),
            filename: filename.clone(),
            superseded_by: rfc_info.superseded_by.clone(),
            supersedes: rfc_info.supersedes.clone(),
            withdrawal_reason: rfc_info.withdrawal_reason.clone(),
            archived_reason: rfc_info.archived_reason.clone(),
            document_source: effective.provenance.document_source.clone(),
            workspace_presence: effective.provenance.workspace_presence.clone(),
            canonical_presence: effective.provenance.canonical_presence.clone(),
            differs_from_canonical: effective.provenance.differs_from_canonical,
            workspace_branch: effective.provenance.workspace_branch.clone(),
            workspace_head: effective.provenance.workspace_head.clone(),
            canonical_ref: effective.provenance.canonical_ref.clone(),
            canonical_head: effective.provenance.canonical_head.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let feature_str = if feature.is_empty() {
                    "(none)"
                } else {
                    &feature
                };
                let lifecycle = if read_status == "active" {
                    let dots = render_stage_dots(rfc_info.stage);
                    let stage_label = stage_name(rfc_info.stage);
                    format!("**Stage**: {dots} {} ({stage_label})", rfc_info.stage)
                } else {
                    let mut lines = vec![format!("**Status**: {}", status_label(&read_status))];
                    if let Some(reason) = rfc_info
                        .archived_reason
                        .as_deref()
                        .or(rfc_info.withdrawal_reason.as_deref())
                    {
                        lines.push(format!("**Reason**: {reason}"));
                    }
                    if let Some(by) = rfc_info.superseded_by.as_deref() {
                        lines.push(format!("**Superseded by**: RFC {by}"));
                    }
                    lines.join("\n")
                };
                let view = match (
                    effective.provenance.document_source.as_str(),
                    effective.provenance.workspace_presence.as_str(),
                ) {
                    ("workspace", _) if effective.provenance.differs_from_canonical => {
                        "Workspace overlay (differs from canonical)"
                    }
                    ("workspace", _) => "Workspace document",
                    (_, "absent") => "Canonical document (absent from this workspace)",
                    _ => "Canonical document",
                };
                let msg = format!(
                    "# RFC {}: {}\n\n{}\n**Feature**: {}\n**File**: {}\n**View**: {}",
                    formatted_id, rfc_info.title, lifecycle, feature_str, filename, view
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc status
// ============================================================================

/// Show RFC status grouped by stage.
#[derive(Debug, Clone, Copy, Default)]
pub struct RfcStatus;

impl RfcStatus {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Serialize)]
struct RfcStageGroup {
    stage: u8,
    stage_name: &'static str,
    rfcs: Vec<RfcListEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct RfcLifecycleGroup {
    status: String,
    status_name: String,
    rfcs: Vec<RfcListEntry>,
}

#[derive(Debug, Serialize)]
struct RfcStatusOutput {
    kind: &'static str,
    ok: bool,
    stages: Vec<RfcStageGroup>,
    lifecycle: Vec<RfcLifecycleGroup>,
    repairs: Vec<rfc::RfcRepairCandidate>,
    workspace_diagnostics: Vec<crate::context::sqlite_loader::RfcWorkspaceDiagnostic>,
    total: usize,
}

const fn stage_name(stage: u8) -> &'static str {
    match stage {
        0 => "Idea",
        1 => "Proposal",
        2 => "Draft",
        3 => "Candidate",
        4 => "Stable",
        _ => "Unknown",
    }
}

// Stage dot glyphs matching rfcDisplay.ts (RFC 10172)
const GLYPH_COMPLETED: char = '●'; // U+25CF BLACK CIRCLE
const GLYPH_FUTURE: char = '○'; // U+25CB WHITE CIRCLE

/// Render stage dots as a 4-character string.
/// Matches the shared rfcDisplay.ts implementation.
///
/// Examples:
/// - Stage 0: ○○○○
/// - Stage 1: ●○○○
/// - Stage 2: ●●○○
/// - Stage 3: ●●●○
/// - Stage 4: ●●●●
fn render_stage_dots(stage: u8) -> String {
    (1..=4)
        .map(|pos| {
            if pos <= stage {
                GLYPH_COMPLETED
            } else {
                GLYPH_FUTURE
            }
        })
        .collect()
}

impl Command for RfcStatus {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "Show RFC status grouped by stage"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let view = rfc::observe_effective_rfc_view_with_project(ctx.root, ctx.project)?.1;
        let repairs =
            rfc::detect_rfc_repair_candidates_with_records(ctx.root, &view.repair_records)?;
        let workspace_diagnostics = view.workspace_diagnostics;
        let all_rfcs = view.records;
        let total = all_rfcs.len();

        // Group active RFCs by stage; archived and withdrawn RFCs are grouped
        // separately so they do not appear as ordinary Stage 0 ideas.
        let mut stages: Vec<RfcStageGroup> = (0..=4)
            .map(|s| RfcStageGroup {
                stage: s,
                stage_name: stage_name(s),
                rfcs: vec![],
            })
            .collect();
        let mut lifecycle = Vec::<RfcLifecycleGroup>::new();

        for r in all_rfcs {
            if is_active_stage_rfc(&r.record) && (r.record.stage as usize) < stages.len() {
                stages[r.record.stage as usize]
                    .rfcs
                    .push(rfc_list_entry_from_effective(&r));
            } else {
                let entry = rfc_list_entry_from_effective(&r);
                let read_status = rfc_read_status(&r.record);
                if let Some(group) = lifecycle
                    .iter_mut()
                    .find(|group| group.status == read_status)
                {
                    group.rfcs.push(entry);
                } else {
                    lifecycle.push(RfcLifecycleGroup {
                        status: read_status.to_string(),
                        status_name: status_label(read_status),
                        rfcs: vec![entry],
                    });
                }
            }
        }
        lifecycle.sort_by(|a, b| a.status.cmp(&b.status));
        let output = RfcStatusOutput {
            kind: "rfc.status",
            ok: true,
            stages: stages.clone(),
            lifecycle: lifecycle.clone(),
            repairs: repairs.clone(),
            workspace_diagnostics: workspace_diagnostics.clone(),
            total,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = format!("# RFC Status ({total} total)\n\n");
                for group in &stages {
                    if group.rfcs.is_empty() {
                        continue;
                    }
                    let dots = render_stage_dots(group.stage);
                    msg.push_str(&format!(
                        "## {} Stage {}: {} ({} RFCs)\n\n",
                        dots,
                        group.stage,
                        group.stage_name,
                        group.rfcs.len()
                    ));
                    for rfc in &group.rfcs {
                        msg.push_str(&format!("- {} **{}**: {}\n", dots, rfc.id, rfc.title));
                    }
                    msg.push('\n');
                }
                for group in &lifecycle {
                    if group.rfcs.is_empty() {
                        continue;
                    }
                    msg.push_str(&format!(
                        "## {} RFCs ({} RFCs)\n\n",
                        group.status_name,
                        group.rfcs.len()
                    ));
                    for rfc in &group.rfcs {
                        msg.push_str(&format!("- **{}**: {}\n", rfc.id, rfc.title));
                    }
                    msg.push('\n');
                }
                if !repairs.is_empty() {
                    msg.push_str("## RFC Identity Repairs\n\n");
                    for repair in &repairs {
                        msg.push_str(&format!(
                            "- RFC {}: {} → {}\n  Reason: {}\n  Next: exo rfc repair {}\n",
                            repair.id,
                            repair.current_path,
                            repair.expected_path,
                            repair.reasons.join(", "),
                            repair.id
                        ));
                    }
                    msg.push('\n');
                }
                if !workspace_diagnostics.is_empty() {
                    msg.push_str("## Workspace RFC Diagnostics\n\n");
                    for diagnostic in &workspace_diagnostics {
                        msg.push_str(&format!(
                            "- {}: {} ({})\n",
                            diagnostic.file_path, diagnostic.message, diagnostic.diagnostic_code
                        ));
                    }
                    msg.push('\n');
                }
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc create
// ============================================================================

/// Create a new RFC.
#[derive(Debug, Clone)]
pub struct RfcCreate {
    pub title: String,
    pub id: Option<String>,
    pub feature: String,
    pub stage: u8,
    pub body: Option<String>,
    // Note: 'open' flag is ignored in trait-based implementation
    // Editor opening is a side effect that doesn't fit CommandOutput model
    pub open: bool,
}

impl RfcCreate {
    pub fn new(
        title: impl Into<String>,
        id: Option<String>,
        feature: impl Into<String>,
        stage: u8,
        body: Option<String>,
        open: bool,
    ) -> Self {
        Self {
            title: title.into(),
            id,
            feature: feature.into(),
            stage,
            body,
            open,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcCreateOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    title: String,
    stage: u8,
    path: String,
}

impl Command for RfcCreate {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "create"
    }

    fn description(&self) -> &'static str {
        "Create a new RFC"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcCreate should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcCreate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let created_path = rfc::create(
            ctx.root,
            &self.title,
            self.id.as_deref(),
            &self.feature,
            self.stage,
            self.body.as_deref(),
        )?;

        // Extract ID from filename (e.g. "10072-my-rfc.md" -> "0086")
        let id = created_path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('-').next())
            .unwrap_or("unknown")
            .to_string();

        let output = RfcCreateOutput {
            kind: "rfc.create",
            ok: true,
            id: id.clone(),
            title: self.title.clone(),
            stage: self.stage,
            path: created_path.display().to_string(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!(
                    "Created RFC {}: {}\nPath: {}\n→ Next: exo rfc edit {} --body \"...\" or exo rfc promote {} --stage {}",
                    id,
                    self.title,
                    created_path.display(),
                    id,
                    id,
                    self.stage.saturating_add(1)
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc edit
// ============================================================================

/// Edit an existing RFC.
#[derive(Debug, Clone)]
pub struct RfcEdit {
    pub id: Option<String>,
    pub path: Option<String>,
    pub title: Option<String>,
    pub feature: Option<String>,
    pub stage: Option<u8>,
    pub body: Option<String>,
}

impl RfcEdit {
    pub const fn new(
        id: Option<String>,
        path: Option<String>,
        title: Option<String>,
        feature: Option<String>,
        stage: Option<u8>,
        body: Option<String>,
    ) -> Self {
        Self {
            id,
            path,
            title,
            feature,
            stage,
            body,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcEditOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    path: String,
}

impl Command for RfcEdit {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Edit an existing RFC"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcEdit should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcEdit {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let (id, result_path) = if let Some(ref explicit_path) = self.path {
            // Edit by explicit path
            let edited_path = rfc::edit_by_path(
                ctx.root,
                explicit_path,
                self.id.as_deref(),
                self.title.as_deref(),
                self.feature.as_deref(),
                self.stage,
                self.body.as_deref(),
            )?;
            // Extract ID from filename or use provided id
            let id = self.id.clone().unwrap_or_else(|| {
                edited_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.split('-').next())
                    .unwrap_or("unknown")
                    .to_string()
            });
            (id, edited_path.display().to_string())
        } else if let Some(ref id) = self.id {
            // Edit by ID
            let edited_path = rfc::edit(
                ctx.root,
                id,
                self.title.as_deref(),
                self.feature.as_deref(),
                self.stage,
                self.body.as_deref(),
            )?;
            (id.clone(), edited_path.display().to_string())
        } else {
            anyhow::bail!("Either --id or --path must be provided")
        };

        let output = RfcEditOutput {
            kind: "rfc.edit",
            ok: true,
            id: id.clone(),
            path: result_path.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Edited RFC: {id}\nPath: {result_path}");
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc rename
// ============================================================================

/// Rename RFC file to match title slug.
#[derive(Debug, Clone)]
pub struct RfcRename {
    pub id: String,
}

impl RfcRename {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct RfcRenameOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    old_path: String,
    new_path: String,
}

impl Command for RfcRename {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "rename"
    }

    fn description(&self) -> &'static str {
        "Rename RFC file to match title slug"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcRename should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcRename {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let (old_path, new_path) = rfc::rename(ctx.root, &self.id)?;

        let output = RfcRenameOutput {
            kind: "rfc.rename",
            ok: true,
            id: self.id.clone(),
            old_path: old_path.display().to_string(),
            new_path: new_path.display().to_string(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!(
                    "Renamed RFC {}\n  {} → {}",
                    self.id,
                    old_path.display(),
                    new_path.display()
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc repair
// ============================================================================

/// Repair RFC filename identity and metadata path drift.
#[derive(Debug, Clone)]
pub struct RfcRepair {
    pub id: String,
    pub path: Option<String>,
    pub renumber_to: Option<String>,
}

impl RfcRepair {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            path: None,
            renumber_to: None,
        }
    }

    pub fn with_options(
        id: impl Into<String>,
        path: Option<String>,
        renumber_to: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            path,
            renumber_to,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcRepairOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    old_path: String,
    new_path: String,
    title: String,
    reasons: Vec<String>,
    repaired: bool,
    renumbered_to: Option<String>,
}

impl Command for RfcRepair {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "repair"
    }

    fn description(&self) -> &'static str {
        "Repair RFC filename identity and metadata path drift"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcRepair should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcRepair {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let outcome = rfc::repair_with_options(
            ctx.root,
            &self.id,
            self.path.as_deref(),
            self.renumber_to.as_deref(),
        )?;

        let output = RfcRepairOutput {
            kind: "rfc.repair",
            ok: true,
            id: outcome.id.clone(),
            old_path: outcome.old_path.clone(),
            new_path: outcome.new_path.clone(),
            title: outcome.title.clone(),
            reasons: outcome.reasons.clone(),
            repaired: outcome.repaired,
            renumbered_to: outcome.renumbered_to.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = if outcome.repaired {
                    let reasons = if outcome.reasons.is_empty() {
                        "metadata relink".to_string()
                    } else {
                        outcome.reasons.join(", ")
                    };
                    format!(
                        "Repaired RFC {}\n  {} → {}\nReason: {}",
                        outcome.id, outcome.old_path, outcome.new_path, reasons
                    )
                } else {
                    format!(
                        "RFC {} already matches the RFC filename policy.\nPath: {}",
                        outcome.id, outcome.new_path
                    )
                };
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc promote
// ============================================================================

/// Promote RFC to the specified next stage.
#[derive(Debug, Clone)]
pub struct RfcPromote {
    pub id: String,
    pub target_stage: u8,
}

impl RfcPromote {
    pub fn new(id: impl Into<String>, target_stage: u8) -> Self {
        Self {
            id: id.into(),
            target_stage,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcPromoteOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    old_stage: u8,
    new_stage: u8,
}

impl Command for RfcPromote {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "promote"
    }

    fn description(&self) -> &'static str {
        "Promote RFC to the specified next stage"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcPromote should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcPromote {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let rfc_path = rfc_root(ctx.root);
        parse_rfc_number(&self.id)?;

        // Get current stage before promotion
        let current = rfc::workspace_rfc_record(ctx.root, &self.id)?
            .ok_or_else(|| rfc_not_found_failure(&self.id))?;
        let read_status = rfc_read_status(&current).to_string();
        let old_stage = current.stage;
        let title = current.title.clone();

        if let Some(candidate) =
            rfc::detect_rfc_repair_candidate_for_text_id(ctx.root, &current.text_id)?
            && rfc::is_blocking_rfc_promote_candidate(&candidate)
        {
            return Err(rfc_promote_repair_required_failure(&self.id, &candidate));
        }

        if read_status != "active" {
            return Err(rfc_promote_lifecycle_failure(&self.id, &read_status));
        }

        if old_stage >= 4 {
            return Err(rfc_promote_already_stable_failure(&self.id, old_stage));
        }

        let new_stage = old_stage + 1;

        if self.target_stage != new_stage {
            return Err(rfc_promote_stage_mismatch_failure(
                &self.id,
                self.target_stage,
                new_stage,
                old_stage,
            ));
        }

        // Perform promotion
        rfc::promote(&rfc_path, &self.id)?;

        let output = RfcPromoteOutput {
            kind: "rfc.promote",
            ok: true,
            id: self.id.clone(),
            old_stage,
            new_stage,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = format!(
                    "Promoted RFC {} ({}) from stage {} ({}) to stage {} ({})",
                    self.id,
                    title,
                    old_stage,
                    stage_name(old_stage),
                    new_stage,
                    stage_name(new_stage)
                );

                // Stage 2→3 transition warning
                if old_stage == 2 && new_stage == 3 {
                    msg.push_str("\n\n⚠️  Stage 3 (Candidate) represents implemented reality. Ensure the RFC content accurately reflects what was built.");
                }

                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc supersede
// ============================================================================

/// Mark RFC as superseded by another.
#[derive(Debug, Clone)]
pub struct RfcSupersede {
    pub id: Option<String>,
    pub by: String,
    pub path: Option<String>,
}

impl RfcSupersede {
    pub fn new(id: Option<String>, by: impl Into<String>, path: Option<String>) -> Self {
        Self {
            id,
            by: by.into(),
            path,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcSupersedeOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    superseded_by: String,
    symmetric_update_applied: bool,
}

impl Command for RfcSupersede {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "supersede"
    }

    fn description(&self) -> &'static str {
        "Mark RFC as superseded by another"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcSupersede should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcSupersede {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let _superseding_number =
            parse_rfc_number_with_failure(&self.by, invalid_rfc_supersede_id_failure)?;
        let mut missing_symmetric_update = false;
        let mut symmetric_update_applied = false;
        let id = if let Some(ref explicit_path) = self.path {
            // Supersede by explicit path
            let outcome = rfc::supersede_file(ctx.root, explicit_path, &self.by)?;
            // Extract ID from filename or use provided id
            self.id.clone().unwrap_or_else(|| {
                outcome
                    .superseded_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.split('-').next())
                    .unwrap_or("unknown")
                    .to_string()
            })
        } else if let Some(ref id) = self.id {
            // Supersede by ID
            let outcome = rfc::supersede(ctx.root, id, &self.by)?;
            symmetric_update_applied = outcome.superseding_path.is_some();
            missing_symmetric_update = !symmetric_update_applied;
            id.clone()
        } else {
            anyhow::bail!("Either --id or --path must be provided")
        };

        let output = RfcSupersedeOutput {
            kind: "rfc.supersede",
            ok: true,
            id: id.clone(),
            superseded_by: self.by.clone(),
            symmetric_update_applied,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = format!("RFC {id} is now superseded by RFC {}", self.by);
                if missing_symmetric_update {
                    msg.push_str(&format!(
                        "\nRFC {} was not found; symmetric supersedes metadata was not updated.",
                        self.by
                    ));
                }
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc withdraw
// ============================================================================

/// Withdraw an RFC (move to withdrawn folder).
#[derive(Debug, Clone)]
pub struct RfcWithdraw {
    pub id: String,
    pub reason: Option<String>,
}

impl RfcWithdraw {
    pub fn new(id: impl Into<String>, reason: Option<String>) -> Self {
        Self {
            id: id.into(),
            reason,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcWithdrawOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    path: String,
}

impl Command for RfcWithdraw {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "withdraw"
    }

    fn description(&self) -> &'static str {
        "Withdraw an RFC"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcWithdraw should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcWithdraw {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let rfc_path = rfc_root(ctx.root);
        let new_path = rfc::withdraw(&rfc_path, &self.id, self.reason.as_deref())?;

        let output = RfcWithdrawOutput {
            kind: "rfc.withdraw",
            ok: true,
            id: self.id.clone(),
            path: new_path.display().to_string(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Withdrew RFC {}\nPath: {}", self.id, output.path);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc archive
// ============================================================================

/// Archive an RFC (move to archive folder for shipped-then-superseded RFCs).
///
/// Unlike withdraw, archive is for RFCs that were implemented (Stage 3+)
/// but are now superseded by newer work.
#[derive(Debug, Clone)]
pub struct RfcArchive {
    pub id: String,
    pub reason: Option<String>,
}

impl RfcArchive {
    pub fn new(id: impl Into<String>, reason: Option<String>) -> Self {
        Self {
            id: id.into(),
            reason,
        }
    }
}

#[derive(Debug, Serialize)]
struct RfcArchiveOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    path: String,
}

impl Command for RfcArchive {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "archive"
    }

    fn description(&self) -> &'static str {
        "Archive a shipped-then-superseded RFC"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_rfc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("RfcArchive should be dispatched via execute_mut")
    }
}

impl MutableCommand for RfcArchive {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let rfc_path = rfc_root(ctx.root);
        let new_path = rfc::archive(&rfc_path, &self.id, self.reason.as_deref())?;

        let output = RfcArchiveOutput {
            kind: "rfc.archive",
            ok: true,
            id: self.id.clone(),
            path: new_path.display().to_string(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Archived RFC {}\nPath: {}", self.id, output.path);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// rfc pipeline
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct RfcPipeline;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PipelineEntry {
    id: String,
    title: String,
    current_stage: Option<u8>,
    target_stage: Option<u8>,
    role: String,
    promotion_requirement: Option<String>,
    is_in_motion: bool,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PipelineOutput {
    phase_id: Option<String>,
    phase_title: Option<String>,
    entries: Vec<PipelineEntry>,
}

fn promotion_requirement(current: u8, target: u8) -> Option<String> {
    if current >= target {
        return None;
    }
    // Walk through each stage transition needed
    let steps: Vec<String> = (current..target)
        .map(|s| match s {
            0 => "Idea → Proposal: needs user approval".to_string(),
            1 => "Proposal → Draft: needs detailed spec".to_string(),
            2 => "Draft → Candidate: needs implementation".to_string(),
            3 => "Candidate → Stable: needs shipping".to_string(),
            _ => format!("Stage {s} → Stage {}: advance", s + 1),
        })
        .collect();
    Some(steps.join("; "))
}

impl Command for RfcPipeline {
    fn namespace(&self) -> &'static str {
        "rfc"
    }

    fn operation(&self) -> &'static str {
        "pipeline"
    }

    fn description(&self) -> &'static str {
        "Show RFC pipeline for the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path)?;
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let workspace_root = agent_ctx.workspace_root_key();
        let details = loader.load_active_phase_details_for_workspace(workspace_root.as_deref())?;

        let Some(details) = details else {
            let output = PipelineOutput {
                phase_id: None,
                phase_title: None,
                entries: vec![],
            };
            return match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(output, "No active phase.")),
            };
        };

        let rfc_index = rfc::load_effective_rfcs(ctx.root, ctx.project)?
            .into_iter()
            .map(|record| (format_rfc_number(record.record.rfc_number), record.record))
            .collect::<std::collections::HashMap<_, _>>();

        // Load phase RFC attachments (with target/relation)
        let phase_rfcs = loader
            .load_phase_rfcs_for_active_phase_for_workspace(workspace_root.as_deref())
            .unwrap_or_default();

        let entries: Vec<PipelineEntry> = phase_rfcs
            .iter()
            .map(|pr| {
                let record = rfc_index.get(&pr.id);
                let current_stage = record.map(|r| r.stage);
                let role = pr.relation.clone();
                let is_in_motion = role == "driving";
                let promotion_req = match (current_stage, pr.target) {
                    (Some(current), Some(target)) => promotion_requirement(current, target),
                    _ => None,
                };
                let path = record.map(|r| r.file_path.clone());

                PipelineEntry {
                    id: pr.id.clone(),
                    title: record.map_or_else(|| format!("RFC {}", pr.id), |r| r.title.clone()),
                    current_stage,
                    target_stage: pr.target,
                    role,
                    promotion_requirement: promotion_req,
                    is_in_motion,
                    path,
                }
            })
            .collect();

        let output = PipelineOutput {
            phase_id: Some(details.phase_id.clone()),
            phase_title: Some(details.phase_title.clone()),
            entries,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if output.entries.is_empty() {
                    Ok(CommandOutput::new(
                        output,
                        "No RFCs linked to the active phase.",
                    ))
                } else {
                    let mut msg = format!(
                        "RFC Pipeline for: {} ({})\n\n",
                        details.phase_title, details.phase_id
                    );
                    for entry in &output.entries {
                        let stage = entry
                            .current_stage
                            .map_or_else(|| "?".to_string(), |s| s.to_string());
                        let target = entry
                            .target_stage
                            .map_or_else(|| "-".to_string(), |s| s.to_string());
                        msg.push_str(&format!(
                            "  {} [{}] Stage {} → {} ({})\n",
                            entry.id, entry.role, stage, target, entry.title
                        ));
                    }
                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc_list_metadata() {
        let cmd = RfcList::new(None);
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_rfc_list_with_stage_filter() {
        let cmd = RfcList::new(Some(2));
        assert_eq!(cmd.stage, Some(2));
    }

    #[test]
    fn test_rfc_stage_filter_matches_active_rfcs_only() {
        let mut record = RfcRecord {
            text_id: "01test".to_string(),
            rfc_number: 1,
            title: "Test".to_string(),
            stage: 0,
            status: "active".to_string(),
            feature: None,
            slug: "test".to_string(),
            file_path: "docs/rfcs/stage-0/00001-test.md".to_string(),
            superseded_by: None,
            supersedes: None,
            withdrawal_reason: None,
            archived_reason: None,
            consolidated_into: None,
        };

        assert!(rfc_matches_stage_filter(&record, Some(0)));
        record.status = "archived".to_string();
        assert!(!rfc_matches_stage_filter(&record, Some(0)));
        record.status = "withdrawn".to_string();
        assert!(!rfc_matches_stage_filter(&record, Some(0)));
        assert!(rfc_matches_stage_filter(&record, None));
    }

    #[test]
    fn test_rfc_stage_filter_excludes_superseded_active_rfcs() {
        let record = RfcRecord {
            text_id: "01test".to_string(),
            rfc_number: 1,
            title: "Test".to_string(),
            stage: 1,
            status: "active".to_string(),
            feature: None,
            slug: "test".to_string(),
            file_path: "docs/rfcs/stage-1/00001-test.md".to_string(),
            superseded_by: Some("00002".to_string()),
            supersedes: None,
            withdrawal_reason: None,
            archived_reason: None,
            consolidated_into: None,
        };

        assert!(!rfc_matches_stage_filter(&record, Some(1)));
        assert_eq!(rfc_read_status(&record), "superseded");
    }

    #[test]
    fn test_rfc_show_metadata() {
        let cmd = RfcShow::new("0085");
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "show");
        assert_eq!(cmd.effect(), Effect::Pure);
        assert_eq!(cmd.id, "0085");
    }

    #[test]
    fn test_rfc_status_metadata() {
        let cmd = RfcStatus::new();
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "status");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_rfc_create_metadata() {
        let cmd = RfcCreate::new("Test RFC", None, "General", 0, None, false);
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "create");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.title, "Test RFC");
        assert_eq!(cmd.feature, "General");
        assert_eq!(cmd.stage, 0);
    }

    #[test]
    fn test_rfc_edit_metadata() {
        let cmd = RfcEdit::new(
            Some("0085".to_string()),
            None,
            Some("New Title".to_string()),
            None,
            None,
            None,
        );
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "edit");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_rfc_rename_metadata() {
        let cmd = RfcRename::new("0085");
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "rename");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_rfc_repair_metadata() {
        let cmd = RfcRepair::new("0085");
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "repair");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_rfc_promote_metadata() {
        let cmd = RfcPromote::new("0085", 1);
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "promote");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_rfc_supersede_metadata() {
        let cmd = RfcSupersede::new(Some("10070".to_string()), "0085", None);
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "supersede");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.by, "0085");
    }

    #[test]
    fn test_rfc_withdraw_metadata() {
        let cmd = RfcWithdraw::new("0086", Some("no longer needed".to_string()));
        assert_eq!(cmd.namespace(), "rfc");
        assert_eq!(cmd.operation(), "withdraw");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_stage_name() {
        assert_eq!(stage_name(0), "Idea");
        assert_eq!(stage_name(1), "Proposal");
        assert_eq!(stage_name(2), "Draft");
        assert_eq!(stage_name(3), "Candidate");
        assert_eq!(stage_name(4), "Stable");
        assert_eq!(stage_name(5), "Unknown");
    }
}
