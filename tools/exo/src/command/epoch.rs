//! Epoch namespace commands.
//!
//! - `epoch list`: List all epochs with status (Pure)
//! - `epoch status`: Show status of a specific epoch (Pure)
//! - `epoch start`: Start an epoch by activating its first pending phase (Write)
//! - `epoch finish`: Finish the current active epoch (Write)
//! - `epoch review`: Mark an epoch as reviewed (Write)

use super::phase_cmd::phase_has_started_work;
use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::{AgentContext, SqliteLoader, SqliteWriter};
use crate::phase_owner::{self, PhaseOwnerView};
use crate::steering::SuggestedAction;
use crate::steering::WorkIntent;
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for epoch commands.
fn default_epoch_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "Show map".to_string(),
        command: "exo map".to_string(),
        rationale: "Use map to orient and get suggested next actions.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the epoch namespace
// ============================================================================

/// Epoch namespace command specification.
///
/// This enum is the authoritative definition of the epoch namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `EpochCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "epoch", description = "Epoch lifecycle commands")]
pub enum EpochCommands {
    #[exo(effect = "pure", description = "List all epochs with status")]
    List,

    #[exo(effect = "pure", description = "Show status of an epoch")]
    Status {
        #[exo(
            positional,
            description = "The epoch ID (defaults to active epoch)",
            optional
        )]
        id: Option<String>,
    },

    #[exo(effect = "write", upgrade_gate, description = "Start an epoch")]
    Start {
        #[exo(positional, description = "The epoch ID to start")]
        id: String,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Finish the current active epoch"
    )]
    Finish,

    #[exo(effect = "write", description = "Mark an epoch as reviewed")]
    Review {
        #[exo(positional, description = "The epoch ID to mark as reviewed")]
        id: String,
    },

    #[exo(effect = "write", upgrade_gate, description = "Add a new epoch")]
    Add {
        #[exo(long, short = 't', description = "The epoch title")]
        title: String,
        #[exo(long, description = "Insert after this epoch ID", optional)]
        after: Option<String>,
    },

    #[exo(effect = "write", description = "Update epoch metadata")]
    Update {
        #[exo(positional, description = "The epoch ID to update")]
        id: String,
        #[exo(long, description = "New title for the epoch")]
        title: String,
    },

    #[exo(effect = "write", description = "Reorder an epoch")]
    Reorder {
        #[exo(positional, description = "The epoch ID to reorder")]
        id: String,
        #[exo(
            positional,
            description = "Target position: top, bottom, before:<id>, or after:<id>"
        )]
        position: String,
    },

    #[exo(effect = "write", upgrade_gate, description = "Remove an epoch")]
    Remove {
        #[exo(positional, description = "The epoch ID to remove")]
        id: String,
    },

    #[exo(effect = "write", upgrade_gate, description = "Bankrupt an epoch")]
    Bankrupt {
        #[exo(positional, description = "The epoch ID to bankrupt")]
        id: String,
    },
}

impl EpochCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::List => CommandBox::pure(EpochList),
            Self::Status { id } => CommandBox::pure(EpochStatus::new(id)),
            Self::Start { id } => CommandBox::mutable(EpochStart::new(id)),
            Self::Finish => CommandBox::mutable(EpochFinish),
            Self::Review { id } => CommandBox::mutable(EpochReview::new(id)),
            Self::Add { title, after } => CommandBox::mutable(EpochAdd::new(title, after)),
            Self::Update { id, title } => CommandBox::mutable(EpochUpdate::new(id, title)),
            Self::Reorder { id, position } => CommandBox::mutable(EpochReorder::new(id, position)),
            Self::Remove { id } => CommandBox::mutable(EpochRemove::new(id)),
            Self::Bankrupt { id } => CommandBox::mutable(EpochBankrupt::new(id)),
        })
    }
}

// ============================================================================
// epoch list
// ============================================================================

/// List all epochs with their status.
#[derive(Debug, Clone, Copy)]
pub struct EpochList;

#[derive(Debug, Serialize)]
struct EpochListEntry {
    id: String,
    title: String,
    status: String,
    reviewed: bool,
    needs_review: bool,
    phase_count: usize,
}

#[derive(Debug, Serialize)]
struct EpochListOutput {
    kind: &'static str,
    ok: bool,
    epochs: Vec<EpochListEntry>,
}

impl Command for EpochList {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List all epochs with status"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        let epochs: Vec<EpochListEntry> = agent_ctx
            .plan
            .epochs
            .iter()
            .map(|e| EpochListEntry {
                id: e.id.clone(),
                title: e.title.clone(),
                status: e.derived_status().to_string(),
                reviewed: e.reviewed,
                needs_review: e.needs_review(),
                phase_count: e.phases.len(),
            })
            .collect();

        let output = EpochListOutput {
            kind: "epoch.list",
            ok: true,
            epochs,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let loader = SqliteLoader::open(ctx.db_path())?;
                let owner_records = loader.load_phase_owners()?;
                let owner_view_context =
                    phase_owner::PhaseOwnerViewContext::new(ctx.root, ctx.project);
                let active_phase_has_started_work =
                    if let Some(active) = agent_ctx.find_workspace_active_phase()? {
                        phase_has_started_work(&loader, active.phase)?
                    } else {
                        false
                    };
                let mut msg = String::new();
                msg.push_str("\n# Epochs\n\n");
                msg.push_str("| Title | Status | Reviewed | Phases | Action |\n");
                msg.push_str("| :--- | :--- | :--- | :--- | :--- |\n");

                for epoch in &agent_ctx.plan.epochs {
                    let status = epoch.derived_status();
                    let reviewed = if epoch.reviewed {
                        "✅"
                    } else if epoch.needs_review() {
                        "⚠️ needs review"
                    } else {
                        "-"
                    };
                    let startable_here = epoch
                        .phases
                        .iter()
                        .filter(|phase| matches!(phase.status.as_str(), "pending" | "in-progress"))
                        .any(|phase| {
                            owner_records
                                .get(&phase.id)
                                .is_none_or(|owner| owner_view_context.owner_view(owner).owned_here)
                        });
                    let action = if status == "completed" && !epoch.reviewed {
                        format!("`exo epoch review {}`", epoch.id)
                    } else if status != "completed"
                        && startable_here
                        && !active_phase_has_started_work
                    {
                        format!("`exo epoch start {}`", epoch.id)
                    } else {
                        String::new()
                    };

                    msg.push_str(&format!(
                        "| {} | {} | {} | {} | {} |\n",
                        epoch.title,
                        status,
                        reviewed,
                        epoch.phases.len(),
                        action
                    ));
                }

                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// epoch review
// ============================================================================

/// Mark an epoch as reviewed.
#[derive(Debug, Clone)]
pub struct EpochReview {
    pub epoch_id: String,
}

impl EpochReview {
    pub fn new(epoch_id: impl Into<String>) -> Self {
        Self {
            epoch_id: epoch_id.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EpochReviewOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    already_reviewed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reviewed: Option<bool>,
    message: String,
}

impl Command for EpochReview {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "review"
    }

    fn description(&self) -> &'static str {
        "Mark an epoch as reviewed"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        // Delegate to execute_mut since this is a mutable command
        unreachable!("EpochReview should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochReview {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        // Find the epoch
        let epoch = agent_ctx
            .plan
            .find_epoch_by_id(&self.epoch_id)
            .ok_or_else(|| anyhow::anyhow!("Epoch not found: {}", self.epoch_id))?;

        let status = epoch.derived_status();
        let epoch_title = epoch.title.clone();
        let epoch_id = epoch.id.clone();

        // Check if already reviewed
        if epoch.reviewed {
            let output = EpochReviewOutput {
                kind: "epoch.review",
                ok: true,
                epoch_id,
                already_reviewed: Some(true),
                reviewed: None,
                message: format!("Epoch '{epoch_title}' has already been reviewed."),
            };
            return match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(
                    output,
                    format!("✓ Epoch '{epoch_title}' has already been reviewed."),
                )),
            };
        }

        // Check if epoch is completed
        if status != "completed" {
            anyhow::bail!(
                "Epoch '{epoch_title}' is not yet completed (status: {status}). Only completed epochs can be reviewed."
            );
        }

        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_epoch_reviewed(&epoch_id, true)?;

        let output = EpochReviewOutput {
            kind: "epoch.review",
            ok: true,
            epoch_id: epoch_id.clone(),
            already_reviewed: None,
            reviewed: Some(true),
            message: format!("Epoch '{epoch_title}' marked as reviewed."),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = String::new();
                msg.push_str("\n# Epoch Review Complete\n\n");
                msg.push_str(&format!("**Epoch**: {epoch_title}\n"));
                msg.push_str("**Status**: ✅ Reviewed\n\n");
                msg.push_str("The epoch has been marked as reviewed. Consider:\n");
                msg.push_str("- Updating the project changelog\n");
                msg.push_str("- Promoting any Stage 2 RFCs to Stage 3 (Candidate)\n");
                msg.push_str("- Archiving completed work artifacts\n");
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// epoch status
// ============================================================================

/// Show status of a specific epoch.
#[derive(Debug, Clone)]
pub struct EpochStatus {
    pub epoch_id: Option<String>,
}

impl EpochStatus {
    pub const fn new(epoch_id: Option<String>) -> Self {
        Self { epoch_id }
    }
}

#[derive(Debug, Serialize)]
struct EpochStatusPhase {
    id: String,
    title: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct EpochStatusOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    title: String,
    status: String,
    reviewed: bool,
    needs_review: bool,
    phases: Vec<EpochStatusPhase>,
}

impl Command for EpochStatus {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "Show status of an epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        // Find the epoch - either by ID or the active one
        let epoch = if let Some(ref id) = self.epoch_id {
            agent_ctx
                .plan
                .find_epoch_by_id(id)
                .ok_or_else(|| anyhow::anyhow!("Epoch not found: {id}"))?
        } else {
            agent_ctx
                .find_workspace_active_epoch()?
                .ok_or_else(|| anyhow::anyhow!("No active epoch found. Specify an epoch ID."))?
        };

        let phases: Vec<EpochStatusPhase> = epoch
            .phases
            .iter()
            .map(|p| EpochStatusPhase {
                id: p.id.clone(),
                title: p.title.clone(),
                status: p.status.clone(),
            })
            .collect();

        let output = EpochStatusOutput {
            kind: "epoch.status",
            ok: true,
            epoch_id: epoch.id.clone(),
            title: epoch.title.clone(),
            status: epoch.derived_status().to_string(),
            reviewed: epoch.reviewed,
            needs_review: epoch.needs_review(),
            phases,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = String::new();
                msg.push_str(&format!("\n# Epoch: {}\n\n", epoch.title));
                msg.push_str(&format!("**Status**: {}\n", epoch.derived_status()));
                if epoch.reviewed {
                    msg.push_str("**Reviewed**: ✅ Yes\n");
                } else if epoch.needs_review() {
                    msg.push_str("**Reviewed**: ⚠️ Needs review\n");
                }
                msg.push_str("\n## Phases\n\n");
                msg.push_str("| Title | Status |\n");
                msg.push_str("| :--- | :--- |\n");
                for phase in &epoch.phases {
                    let status_icon = match phase.status.as_str() {
                        "completed" => "✅",
                        "in-progress" => "🔄",
                        "pending" => "⏳",
                        _ => "❓",
                    };
                    msg.push_str(&format!(
                        "| {} | {} {} |\n",
                        phase.title, status_icon, phase.status
                    ));
                }
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// epoch start
// ============================================================================

/// Start an epoch by activating its first pending phase.
#[derive(Debug, Clone)]
pub struct EpochStart {
    pub epoch_id: String,
}

impl EpochStart {
    pub fn new(epoch_id: impl Into<String>) -> Self {
        Self {
            epoch_id: epoch_id.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EpochStartOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    title: String,
    first_phase_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<PhaseOwnerView>,
    message: String,
}

impl Command for EpochStart {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "start"
    }

    fn description(&self) -> &'static str {
        "Start an epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochStart should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        // Find the epoch
        let epoch = agent_ctx
            .plan
            .find_epoch_by_id(&self.epoch_id)
            .ok_or_else(|| anyhow::anyhow!("Epoch not found: {}", self.epoch_id))?;

        let epoch_title = epoch.title.clone();
        let epoch_id = epoch.id.clone();

        let status = epoch.derived_status();
        if status == "completed" {
            anyhow::bail!("Epoch '{epoch_title}' is already completed and cannot be started.");
        }

        let db_path = ctx.db_path();
        let mut existing_active = None;
        for phase in epoch.phases.iter().filter(|p| p.status == "in-progress") {
            let owner =
                phase_owner::owner_view_for_phase(ctx.root, ctx.project, &db_path, &phase.id)?;
            if owner.as_ref().map_or(true, |owner| owner.owned_here) {
                existing_active = Some(phase);
                break;
            }
        }

        let mut first_pending = None;
        for phase in epoch.phases.iter().filter(|p| p.status == "pending") {
            let owner =
                phase_owner::owner_view_for_phase(ctx.root, ctx.project, &db_path, &phase.id)?;
            if owner.as_ref().map_or(true, |owner| owner.owned_here) {
                first_pending = Some(phase);
                break;
            }
        }

        let first_suitable = existing_active.or(first_pending).ok_or_else(|| {
            anyhow::anyhow!("No startable phases found in epoch '{epoch_title}'.")
        })?;

        let first_phase_id = first_suitable.id.clone();
        let first_phase_title = first_suitable.title.clone();
        let should_mark_started = first_suitable.status != "in-progress";

        let writer = SqliteWriter::open(&db_path)?;

        let owner = phase_owner::claim_phase_for_current_owner(
            ctx.root,
            ctx.project,
            &db_path,
            &first_phase_id,
            false,
        )?
        .owner;

        if should_mark_started {
            writer.update_phase_status(&first_phase_id, "in-progress")?;
        }
        if let Some(workspace_root) = agent_ctx.workspace_root_key() {
            writer.set_workspace_active_phase(&workspace_root, &first_phase_id)?;
        }

        let message = if should_mark_started {
            format!(
                "Started epoch '{epoch_title}'. First phase \"{first_phase_title}\" is now active."
            )
        } else {
            format!("Epoch '{epoch_title}' is active in this workspace.")
        };

        let output = EpochStartOutput {
            kind: "epoch.start",
            ok: true,
            epoch_id: epoch_id.clone(),
            title: epoch_title.clone(),
            first_phase_id: Some(first_phase_id.clone()),
            owner: Some(owner),
            message,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = String::new();
                msg.push_str(&format!("\n# Epoch Started: {epoch_title}\n\n"));
                msg.push_str(&format!("**Active Phase**: {first_phase_title}\n\n"));
                msg.push_str("Next steps:\n");
                msg.push_str("- Run `exo phase status` to see phase details\n");
                msg.push_str("- Begin implementation according to the phase plan\n");
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// epoch finish
// ============================================================================

/// Finish the current active epoch.
#[derive(Debug, Clone, Copy)]
pub struct EpochFinish;

#[derive(Debug, Serialize)]
struct EpochFinishOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    title: String,
    message: String,
}

impl Command for EpochFinish {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "finish"
    }

    fn description(&self) -> &'static str {
        "Finish the current active epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochFinish should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochFinish {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        let (output, epoch_title, epoch_id, phases_len, reviewed) = {
            let epoch = agent_ctx
                .find_workspace_active_epoch()?
                .ok_or_else(|| anyhow::anyhow!("No active epoch found."))?;

            let epoch_title = epoch.title.clone();
            let epoch_id = epoch.id.clone();

            // Check if all phases are completed
            let incomplete_phases: Vec<_> = epoch
                .phases
                .iter()
                .filter(|p| p.status != "completed")
                .map(|p| p.title.clone())
                .collect();

            if !incomplete_phases.is_empty() {
                anyhow::bail!(
                    "Cannot finish epoch '{epoch_title}'. The following phases are not completed: {}",
                    incomplete_phases.join(", ")
                );
            }

            // Epoch is already effectively completed since all phases are done
            // Just provide feedback about the status
            (
                EpochFinishOutput {
                    kind: "epoch.finish",
                    ok: true,
                    epoch_id: epoch_id.clone(),
                    title: epoch_title.clone(),
                    message: format!(
                        "Epoch '{epoch_title}' is complete! All {} phases finished.",
                        epoch.phases.len()
                    ),
                },
                epoch_title,
                epoch_id,
                epoch.phases.len(),
                epoch.reviewed,
            )
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = String::new();
                msg.push_str(&format!("\n# Epoch Complete: {epoch_title}\n\n"));
                msg.push_str(&format!("**Phases Completed**: {phases_len}\n\n"));
                if !reviewed {
                    msg.push_str("⚠️ This epoch has not been reviewed yet.\n\n");
                    msg.push_str("Next steps:\n");
                    msg.push_str(&format!(
                        "- Run `exo epoch review {epoch_id}` to mark as reviewed\n"
                    ));
                } else {
                    msg.push_str("✅ This epoch has been reviewed.\n\n");
                }
                msg.push_str("- Update the project changelog\n");
                msg.push_str("- Consider promoting Stage 2 RFCs to Stage 3\n");
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ===== epoch add =====

/// Add a new epoch.
#[derive(Debug, Clone)]
pub struct EpochAdd {
    pub title: String,
    pub after: Option<String>,
}

impl EpochAdd {
    pub fn new(title: impl Into<String>, after: Option<String>) -> Self {
        Self {
            title: title.into(),
            after,
        }
    }
}

#[derive(Debug, Serialize)]
struct EpochAddOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    title: String,
}

impl Command for EpochAdd {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let id = {
            let writer = SqliteWriter::open(ctx.db_path())?;

            // Validate anchor exists before creating the epoch
            if let Some(ref after_id) = self.after {
                let loader = crate::context::SqliteLoader::open(ctx.db_path())?;
                let state = loader.load_state()?;
                if !state.epochs.iter().any(|e| e.id == *after_id) {
                    return Err(anyhow::Error::new(
                        crate::failure::ExoFailure::plan_anchor_not_found(after_id, "Epoch"),
                    ));
                }
            }

            let id = writer.add_epoch(&self.title, None, &[])?;

            // Apply ordering
            if let Some(ref after_id) = self.after {
                writer.reorder_epoch(&id, &format!("after:{after_id}"))?;
            }

            id
        };

        let output = EpochAddOutput {
            kind: "epoch.add",
            ok: true,
            id: id.clone(),
            title: self.title.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Added epoch \"{}\"", self.title);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ===== epoch update =====

/// Update an epoch.
#[derive(Debug, Clone)]
pub struct EpochUpdate {
    pub id: String,
    pub title: String,
}

impl EpochUpdate {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EpochUpdateOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    title: String,
}

impl Command for EpochUpdate {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "update"
    }

    fn description(&self) -> &'static str {
        "Update epoch metadata"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochUpdate should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochUpdate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_epoch_title(&self.id, &self.title)?;

        let output = EpochUpdateOutput {
            kind: "epoch.update",
            ok: true,
            id: self.id.clone(),
            title: self.title.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Updated epoch title to \"{}\"", self.title),
            )),
        }
    }
}

// ===== epoch reorder =====

/// Reorder an epoch.
#[derive(Debug, Clone)]
pub struct EpochReorder {
    pub id: String,
    pub position: String,
}

impl EpochReorder {
    pub fn new(id: impl Into<String>, position: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            position: position.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EpochReorderOutput {
    kind: &'static str,
    ok: bool,
    id: String,
    position: String,
}

impl Command for EpochReorder {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "reorder"
    }

    fn description(&self) -> &'static str {
        "Reorder an epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochReorder should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochReorder {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let epoch_title = AgentContext::load(ctx.root.to_path_buf())?
            .plan
            .find_epoch_by_id(&self.id)
            .map(|epoch| epoch.title.clone())
            .ok_or_else(|| anyhow::anyhow!("Epoch not found: {}", self.id))?;
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.reorder_epoch(&self.id, &self.position)?;

        let output = EpochReorderOutput {
            kind: "epoch.reorder",
            ok: true,
            id: self.id.clone(),
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Moved epoch \"{epoch_title}\" to {}", self.position),
            )),
        }
    }
}

// ===== epoch remove =====

/// Remove an epoch.
#[derive(Debug, Clone)]
pub struct EpochRemove {
    pub id: String,
}

impl EpochRemove {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct EpochRemoveOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for EpochRemove {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove an epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochRemove should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let epoch_title = AgentContext::load(ctx.root.to_path_buf())?
            .plan
            .find_epoch_by_id(&self.id)
            .map(|epoch| epoch.title.clone())
            .ok_or_else(|| anyhow::anyhow!("Epoch not found: {}", self.id))?;
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.remove_epoch(&self.id)?;

        let output = EpochRemoveOutput {
            kind: "epoch.remove",
            ok: true,
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Removed epoch \"{epoch_title}\"");
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

/// Bankrupt an epoch (mark all pending items as bankrupt).
#[derive(Debug, Clone)]
pub struct EpochBankrupt {
    pub id: String,
}

impl EpochBankrupt {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct EpochBankruptOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for EpochBankrupt {
    fn namespace(&self) -> &'static str {
        "epoch"
    }

    fn operation(&self) -> &'static str {
        "bankrupt"
    }

    fn description(&self) -> &'static str {
        "Bankrupt an epoch (mark all pending items as bankrupt)"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_epoch_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("EpochBankrupt should be dispatched via execute_mut")
    }
}

impl MutableCommand for EpochBankrupt {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let epoch = agent_ctx
            .plan
            .epochs
            .iter()
            .find(|epoch| epoch.id == self.id)
            .ok_or_else(|| anyhow::anyhow!("Epoch not found: {}", self.id))?;
        for phase in &epoch.phases {
            phase_owner::ensure_phase_write_allowed(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                &phase.id,
            )?;
        }

        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.bankrupt_epoch(&self.id)?;

        let output = EpochBankruptOutput {
            kind: "epoch.bankrupt",
            ok: true,
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!(
                    "Bankrupted epoch \"{}\" - all pending items marked bankrupt",
                    epoch.title
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_list_metadata() {
        let cmd = EpochList;
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_epoch_review_metadata() {
        let cmd = EpochReview::new("test-epoch");
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "review");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_epoch_status_metadata() {
        let cmd = EpochStatus::new(Some("test-epoch".to_string()));
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "status");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_epoch_start_metadata() {
        let cmd = EpochStart::new("test-epoch");
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "start");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_epoch_finish_metadata() {
        let cmd = EpochFinish;
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "finish");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_epoch_add_metadata() {
        let cmd = EpochAdd::new("Epoch 1", None);
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.title, "Epoch 1");
    }

    #[test]
    fn test_epoch_add_with_after() {
        let cmd = EpochAdd::new("Epoch 2", Some("e1".to_string()));
        assert_eq!(cmd.after, Some("e1".to_string()));
    }

    #[test]
    fn test_epoch_remove_metadata() {
        let cmd = EpochRemove::new("e1");
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "remove");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_epoch_bankrupt_metadata() {
        let cmd = EpochBankrupt::new("e1");
        assert_eq!(cmd.namespace(), "epoch");
        assert_eq!(cmd.operation(), "bankrupt");
        assert_eq!(cmd.effect(), Effect::Write);
    }
}
