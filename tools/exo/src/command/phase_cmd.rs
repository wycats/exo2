//! Phase namespace commands.
//!
//! - `phase start`: Start a new phase (Write)
//! - `phase status`: Get current phase status (Pure)
//! - `phase finish`: Complete the current phase (Write)
//! - `phase history`: Show history of completed phases (Pure)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::{AgentContext, PhaseKind, SqliteLoader, SqliteWriter};
use crate::failure::ExoFailure;
use crate::phase;
use crate::phase_owner::{self, CurrentOwnerView, PhaseOwnerView};
use crate::steering::{self, SteeringBlock, SuggestedAction, WorkIntent};
use crate::world_state::{SnapshotFileStatus, WorldState};
use anyhow::{Context, Result as ExoResult};

use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

/// Default steering for phase namespace
#[allow(dead_code)] // Scaffolding for future use
fn default_phase_steering() -> Vec<SuggestedAction> {
    vec![
        SuggestedAction {
            label: "Show phase status".to_string(),
            command: "exo phase status".to_string(),
            rationale: "View current phase information and tasks".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.8),
        },
        SuggestedAction {
            label: "List available phases".to_string(),
            command: "exo plan read".to_string(),
            rationale: "See all phases in the plan".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.7),
        },
    ]
}

// ============================================================================
// ExoSpec definition — single source of truth for the phase namespace
// ============================================================================

/// Phase namespace command specification.
///
/// This enum is the authoritative definition of the phase namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `PhaseCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "phase", description = "Phase lifecycle commands")]
pub enum PhaseCommands {
    #[exo(effect = "write", upgrade_gate, description = "Start a pending phase")]
    Start {
        #[exo(positional, optional, description = "Phase ID to start")]
        id: Option<String>,
        #[exo(flag, description = "Explicitly replace an existing phase owner")]
        take_over: bool,
    },

    #[exo(
        effect = "write",
        description = "Focus a phase in this workspace without claiming ownership"
    )]
    Focus {
        #[exo(positional, description = "Phase ID to focus")]
        id: String,
    },

    #[exo(
        effect = "write",
        description = "Release this workspace's or a stale phase owner"
    )]
    Release {
        #[exo(positional, description = "Phase ID whose owner should be released")]
        id: String,
    },

    #[exo(effect = "pure", description = "Get the status of the current phase")]
    Status {
        #[exo(
            flag,
            description = "Show full details even if a surgical strike is active"
        )]
        full: bool,
    },

    #[exo(
        effect = "pure",
        operation = "execution.tasks",
        description = "List phase execution tasks"
    )]
    ExecutionTasks {
        #[exo(long, optional, description = "Pagination cursor")]
        cursor: Option<String>,
        #[exo(long, optional, description = "Pagination limit")]
        limit: Option<i64>,
    },

    #[exo(
        effect = "pure",
        operation = "read-goals",
        description = "Read goals for a phase (defaults to active phase)"
    )]
    ReadGoals {
        #[exo(
            positional,
            optional,
            description = "Phase ID (defaults to active phase)"
        )]
        id: Option<String>,
    },

    #[exo(
        effect = "pure",
        operation = "read-tasks",
        description = "Read tasks for a phase (defaults to active phase)"
    )]
    ReadTasks {
        #[exo(
            positional,
            optional,
            description = "Phase ID (defaults to active phase)"
        )]
        id: Option<String>,
    },

    #[exo(
        effect = "pure",
        operation = "read-details",
        description = "Read canonical details for a phase (defaults to active phase)"
    )]
    ReadDetails {
        #[exo(
            positional,
            optional,
            description = "Phase ID (defaults to active phase)"
        )]
        id: Option<String>,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Add a new phase to an epoch (defaults to active epoch)"
    )]
    Add {
        #[exo(long, short = 't', description = "The phase title")]
        title: String,
        #[exo(
            long,
            optional,
            description = "The parent epoch ID (defaults to active epoch)"
        )]
        epoch: Option<String>,
        #[exo(long, optional, description = "Insert after this phase ID")]
        after: Option<String>,
        #[exo(long, optional, description = "Insert before this phase ID")]
        before: Option<String>,
        #[exo(flag, description = "Insert at the beginning of the epoch")]
        first: bool,
        #[exo(long, optional, description = "Comma-separated RFC IDs")]
        rfcs: Option<String>,
        #[exo(
            long,
            default = "regular",
            description = "The kind of work (regular, chore)"
        )]
        kind: String,
    },

    #[exo(
        effect = "pure",
        operation = "list",
        description = "List phases in the active epoch"
    )]
    List {
        #[exo(long, optional, description = "Epoch ID (defaults to active epoch)")]
        epoch: Option<String>,
    },

    #[exo(effect = "write", upgrade_gate, description = "Remove a phase")]
    Remove {
        #[exo(positional, description = "The phase ID to remove")]
        id: String,
    },

    #[exo(effect = "write", description = "Update phase metadata (title, RFCs)")]
    Update {
        #[exo(positional, description = "The ID of the phase to update")]
        id: String,
        #[exo(long, optional, description = "New title for the phase")]
        title: Option<String>,
        #[exo(
            long,
            optional,
            description = "Comma-separated RFC IDs to associate with the phase"
        )]
        rfcs: Option<String>,
    },

    #[exo(effect = "write", description = "Reorder a phase within its epoch")]
    Reorder {
        #[exo(positional, description = "The phase ID to reorder")]
        id: String,
        #[exo(
            positional,
            description = "Target position: top, bottom, before:<id>, or after:<id>"
        )]
        position: String,
    },

    #[exo(effect = "write", description = "Move a phase to another epoch")]
    Move {
        #[exo(positional, description = "The phase ID to move")]
        id: String,
        #[exo(long, description = "Target epoch ID")]
        epoch: String,
        #[exo(
            long,
            optional,
            description = "Target position: top, bottom, before:<id>, or after:<id>"
        )]
        position: Option<String>,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Finish the current phase"
    )]
    Finish {
        #[exo(long, short = 'm', optional, description = "The commit message to use")]
        message: Option<String>,
    },

    #[exo(effect = "pure", description = "Show history of completed phases")]
    History {
        #[exo(long, optional, description = "Maximum number of phases to show")]
        limit: Option<i64>,
    },
}

impl PhaseCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        fn parse_limit(limit: Option<i64>) -> Option<usize> {
            limit.and_then(|value| usize::try_from(value).ok())
        }

        fn split_csv(value: Option<String>) -> Option<Vec<String>> {
            let raw = value?;
            let items: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
            if items.is_empty() { None } else { Some(items) }
        }

        Ok(match self {
            Self::Start { id, take_over } => CommandBox::mutable(PhaseStart::new(id, take_over)),
            Self::Focus { id } => CommandBox::mutable(PhaseFocus::new(id)),
            Self::Release { id } => CommandBox::mutable(PhaseRelease::new(id)),
            Self::Status { full } => CommandBox::pure(PhaseStatus::new(full)),
            Self::ExecutionTasks { cursor, limit } => {
                CommandBox::pure(PhaseExecutionTasks::new(cursor, parse_limit(limit)))
            }
            Self::ReadGoals { id } => CommandBox::pure(PhaseReadGoals::new(id)),
            Self::ReadTasks { id } => CommandBox::pure(PhaseReadTasks::new(id)),
            Self::ReadDetails { id } => CommandBox::pure(PhaseReadDetails::new(id)),
            Self::Add {
                title,
                epoch,
                after,
                before,
                first,
                rfcs,
                kind,
            } => {
                let kind = kind
                    .parse::<PhaseKind>()
                    .map_err(|err| anyhow::anyhow!("Invalid value for argument 'kind': {err}"))?;
                let mut cmd = PhaseAdd::new(title, after, before, first, split_csv(rfcs), kind);
                cmd = cmd.with_epoch(epoch);
                CommandBox::mutable(cmd)
            }
            Self::List { epoch } => CommandBox::pure(PhaseList::new(epoch)),
            Self::Remove { id } => CommandBox::mutable(PhaseRemove::new(id)),
            Self::Update { id, title, rfcs } => {
                CommandBox::mutable(PhaseUpdate::new(id, title, split_csv(rfcs)))
            }
            Self::Reorder { id, position } => CommandBox::mutable(PhaseReorder::new(id, position)),
            Self::Move {
                id,
                epoch,
                position,
            } => CommandBox::mutable(PhaseMove::new(id, epoch, position)),
            Self::Finish { message } => CommandBox::mutable(PhaseFinish::new(message)),
            Self::History { limit } => CommandBox::pure(PhaseHistory::new(parse_limit(limit))),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PhaseReadGoals {
    id: Option<String>,
}

impl PhaseReadGoals {
    pub const fn new(id: Option<String>) -> Self {
        Self { id }
    }
}

impl Command for PhaseReadGoals {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "read-goals"
    }

    fn description(&self) -> &'static str {
        "Read goals for a phase (defaults to active phase)"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let phase = match resolve_phase_details_or_error(ctx, self.id.as_deref())? {
            Some(phase) => phase,
            None => return Ok(CommandOutput::data(Vec::<serde_json::Value>::new())),
        };

        let goals: Vec<serde_json::Value> = phase
            .goals
            .iter()
            .map(|goal| {
                json!({
                    "id": goal.id,
                    "title": goal.title,
                    "description": goal.description,
                    "status": normalize_phase_read_status(&goal.status),
                    "kind": goal.kind,
                    "completionLog": goal.completion_log,
                })
            })
            .collect();

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(goals)),
            OutputFormat::Human => Ok(CommandOutput::new(
                &goals,
                serde_json::to_string_pretty(&goals)?,
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhaseReadTasks {
    id: Option<String>,
}

impl PhaseReadTasks {
    pub const fn new(id: Option<String>) -> Self {
        Self { id }
    }
}

impl Command for PhaseReadTasks {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "read-tasks"
    }

    fn description(&self) -> &'static str {
        "Read tasks for a phase (defaults to active phase)"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let tasks: Vec<serde_json::Value> =
            match resolve_phase_details_or_error(ctx, self.id.as_deref())? {
                Some(phase) => phase
                    .goals
                    .iter()
                    .flat_map(|goal| {
                        goal.tasks.iter().map(move |task| {
                            json!({
                                "id": format!("{}::{}", goal.id, task.id),
                                "taskId": task.id,
                                "title": task.title,
                                "status": normalize_phase_read_status(&task.status),
                                "goalId": goal.id,
                                "goalTitle": goal.title,
                            })
                        })
                    })
                    .collect(),
                None => Vec::new(),
            };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(tasks)),
            OutputFormat::Human => Ok(CommandOutput::new(
                &tasks,
                serde_json::to_string_pretty(&tasks)?,
            )),
        }
    }
}

fn normalize_phase_read_status(status: &str) -> &str {
    match status {
        "pending" => "todo",
        "completed" => "done",
        "in_progress" => "in-progress",
        other => other,
    }
}

fn resolve_phase_details_or_error(
    ctx: &CommandContext,
    phase_id: Option<&str>,
) -> ExoResult<Option<crate::context::sqlite_loader::PhaseDetailsData>> {
    let details = resolve_phase_details(ctx, phase_id)?;
    if details.is_none()
        && let Some(phase_id) = phase_id
    {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::InvalidInput,
            format!(
                "Phase '{phase_id}' not found. Use `exo phase list` or `exo plan read` to see available phases."
            ),
            ExoFailure::orienting_steering(vec![
                SuggestedAction {
                    label: "List phases".to_string(),
                    command: "exo phase list".to_string(),
                    rationale: "Find the phase ID before reading phase-scoped goals or tasks."
                        .to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.7),
                },
                SuggestedAction {
                    label: "Read plan".to_string(),
                    command: "exo plan read".to_string(),
                    rationale: "Review the current epoch and phases before choosing a phase."
                        .to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.6),
                },
            ]),
        )));
    }
    Ok(details)
}

fn resolve_phase_details(
    ctx: &CommandContext,
    phase_id: Option<&str>,
) -> ExoResult<Option<crate::context::sqlite_loader::PhaseDetailsData>> {
    let loader = SqliteLoader::open(ctx.db_path())?;
    if let Some(phase_id) = phase_id {
        return loader.load_phase_details_by_id(phase_id);
    }

    let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
    let workspace_root = agent_ctx.workspace_root_key();
    loader.load_active_phase_details_for_workspace(workspace_root.as_deref())
}

#[derive(Debug, Clone)]
pub struct PhaseList {
    epoch: Option<String>,
}

impl PhaseList {
    pub const fn new(epoch: Option<String>) -> Self {
        Self { epoch }
    }
}

#[derive(Debug, Clone, Serialize)]
struct PhaseListEntry {
    id: String,
    title: String,
    status: String,
    epoch_id: String,
    epoch_title: String,
    position: usize,
    goal_count: usize,
    focused_here: bool,
    owned_here: bool,
    owned_elsewhere: bool,
    stale_owner: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<PhaseOwnerView>,
}

#[derive(Debug, Serialize)]
struct PhaseListOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    epoch_title: String,
    phases: Vec<PhaseListEntry>,
}

impl Command for PhaseList {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List phases in the active epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let epoch = if let Some(epoch_id) = &self.epoch {
            agent_ctx
                .plan
                .epochs
                .iter()
                .find(|epoch| {
                    epoch.id == *epoch_id || epoch.aliases.iter().any(|id| id == epoch_id)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Epoch '{epoch_id}' not found. Use `exo plan read` to see available epochs."
                    )
                })?
        } else {
            let Some(active_epoch) = agent_ctx.find_workspace_active_epoch()? else {
                anyhow::bail!("No active epoch found. Use `exo plan read` to see available epochs.")
            };
            active_epoch
        };

        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path)?;
        let focused_phase = agent_ctx.workspace_active_phase_pin()?;
        let started_active_phase_id = if let Some(active) =
            agent_ctx.find_workspace_active_phase()?
            && phase_has_started_work(&loader, active.phase)?
        {
            Some(active.phase.id.clone())
        } else {
            None
        };
        let owner_records = loader.load_phase_owners()?;
        let owner_view_context = epoch
            .phases
            .iter()
            .any(|phase| owner_records.contains_key(&phase.id))
            .then(|| phase_owner::PhaseOwnerViewContext::new(ctx.root, ctx.project));
        let mut phases = Vec::with_capacity(epoch.phases.len());
        for (position, phase) in epoch.phases.iter().enumerate() {
            let owner = owner_records.get(&phase.id).map(|owner| {
                owner_view_context
                    .as_ref()
                    .expect("owner view context exists when an owner record matches")
                    .owner_view(owner)
            });
            phases.push(PhaseListEntry {
                id: phase.id.clone(),
                title: phase.title.clone(),
                status: phase.status.clone(),
                epoch_id: epoch.id.clone(),
                epoch_title: epoch.title.clone(),
                position,
                goal_count: phase.goals.len(),
                focused_here: focused_phase.as_deref() == Some(phase.id.as_str()),
                owned_here: owner.as_ref().is_some_and(|owner| owner.owned_here),
                owned_elsewhere: owner.as_ref().is_some_and(|owner| owner.owned_elsewhere),
                stale_owner: owner.as_ref().is_some_and(|owner| owner.stale),
                owner,
            });
        }

        let output = PhaseListOutput {
            kind: "phase.list",
            ok: true,
            epoch_id: epoch.id.clone(),
            epoch_title: epoch.title.clone(),
            phases: phases.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if phases.is_empty() {
                    Ok(CommandOutput::new(output, "No phases found."))
                } else {
                    let mut msg = format!("Phases in {}\n", epoch.title);
                    msg.push_str("| Position | Title | Status | Owner | Goals | Action |\n");
                    msg.push_str("| ---: | :--- | :--- | :--- | ---: | :--- |\n");
                    for phase in &phases {
                        let owner_signal = phase_owner_signal(phase);
                        let action = if matches!(phase.status.as_str(), "pending" | "in-progress") {
                            if started_active_phase_id.as_deref() == Some(phase.id.as_str()) {
                                if phase.owned_elsewhere {
                                    format!("`exo phase start {} --take-over`", phase.id)
                                } else {
                                    "`exo phase finish`".to_string()
                                }
                            } else if started_active_phase_id.is_some() {
                                String::new()
                            } else if phase.owned_elsewhere {
                                format!("`exo phase start {} --take-over`", phase.id)
                            } else {
                                format!("`exo phase start {}`", phase.id)
                            }
                        } else {
                            String::new()
                        };
                        msg.push_str(&format!(
                            "| {} | {} | {} | {} | {} | {} |\n",
                            phase.position,
                            phase.title,
                            phase.status,
                            owner_signal,
                            phase.goal_count,
                            action
                        ));
                    }
                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

fn phase_owner_signal(phase: &PhaseListEntry) -> String {
    let focus = if phase.focused_here { "focused" } else { "" };
    let owner = if phase.stale_owner {
        "stale owner".to_string()
    } else if phase.owned_here {
        "owned here".to_string()
    } else if let Some(owner) = &phase.owner {
        format!("owned by {}", owner.label)
    } else {
        "unowned".to_string()
    };

    if focus.is_empty() {
        owner
    } else {
        format!("{focus}, {owner}")
    }
}

#[derive(Debug, Clone)]
pub struct PhaseReadDetails {
    id: Option<String>,
}

impl PhaseReadDetails {
    pub const fn new(id: Option<String>) -> Self {
        Self { id }
    }
}

impl Command for PhaseReadDetails {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "read-details"
    }

    fn description(&self) -> &'static str {
        "Read canonical details for a phase (defaults to active phase)"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path)?;
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let workspace_root = agent_ctx.workspace_root_key();
        let details = match &self.id {
            Some(phase_id) => loader.load_phase_details_by_id(phase_id)?,
            None => loader.load_active_phase_details_for_workspace(workspace_root.as_deref())?,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(details)),
            OutputFormat::Human => Ok(CommandOutput::new(
                &details,
                serde_json::to_string_pretty(&details)?,
            )),
        }
    }
}

// ===== phase start =====

/// Start a pending phase.
#[derive(Debug, Clone, Default)]
pub struct PhaseStart {
    id: Option<String>,
    take_over: bool,
}

impl PhaseStart {
    pub const fn new(id: Option<String>, take_over: bool) -> Self {
        Self { id, take_over }
    }
}

impl Command for PhaseStart {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "start"
    }

    fn description(&self) -> &'static str {
        "Start a pending phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Check current phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "Verify if there's an active phase that needs to be finished first"
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Review plan".to_string(),
                command: "exo plan review".to_string(),
                rationale: "See the plan structure and phase ordering".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseStart should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let (next_phase, next_phase_title, owner_transition) = {
            let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
            let (next_phase, next_phase_title, next_phase_status) = match self.id.as_deref() {
                Some(id) => find_startable_phase(&agent_ctx, id).ok_or_else(|| {
                    crate::failure::ExoFailure::new(
                        crate::api::protocol::ErrorCode::PreconditionFailed,
                        format!("Phase '{id}' is not pending, in-progress, or does not exist"),
                        crate::failure::ExoFailure::orienting_steering(vec![SuggestedAction {
                            label: "Review phase options".to_string(),
                            command: "exo plan review".to_string(),
                            rationale: "Check available pending phases before starting one."
                                .to_string(),
                            intent: WorkIntent::Orient,
                            confidence: Some(0.9),
                        }]),
                    )
                })?,
                None => find_next_pending_phase(&agent_ctx).ok_or_else(|| {
                    crate::failure::ExoFailure::new(
                        crate::api::protocol::ErrorCode::NotFound,
                        "No pending phase found to start".to_string(),
                        crate::failure::ExoFailure::orienting_steering(vec![SuggestedAction {
                            label: "Review plan".to_string(),
                            command: "exo plan review".to_string(),
                            rationale: "Check the plan to see available phases".to_string(),
                            intent: WorkIntent::Orient,
                            confidence: Some(0.9),
                        }]),
                    )
                })?,
            };

            let writer = SqliteWriter::open(ctx.db_path())?;
            let loader = SqliteLoader::open(ctx.db_path())?;
            let active_phase = agent_ctx.find_workspace_active_phase()?;
            let active_phase_to_demote = if let Some(active_phase) = active_phase.as_ref()
                && active_phase.phase.id != next_phase
            {
                let active_owner = phase_owner::owner_view_for_phase(
                    ctx.root,
                    ctx.project,
                    &ctx.db_path(),
                    &active_phase.phase.id,
                )?;
                if active_owner
                    .as_ref()
                    .is_some_and(|owner| owner.owned_elsewhere)
                {
                    None
                } else if !phase_has_started_work(&loader, active_phase.phase)? {
                    Some(active_phase.phase.id.clone())
                } else {
                    return Err(anyhow::Error::new(crate::failure::ExoFailure::new(
                        crate::api::protocol::ErrorCode::PreconditionFailed,
                        format!(
                            "Cannot start phase '{}' while '{}' has started work",
                            next_phase_title, active_phase.phase.title
                        ),
                        crate::failure::ExoFailure::orienting_steering(vec![SuggestedAction {
                            label: "Finish current phase".to_string(),
                            command: "exo phase finish".to_string(),
                            rationale: "Started phases must be finished before switching phases."
                                .to_string(),
                            intent: WorkIntent::Ship,
                            confidence: Some(0.9),
                        }]),
                    )));
                }
            } else {
                None
            };

            let owner_transition = phase_owner::claim_phase_for_current_owner(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                &next_phase,
                self.take_over,
            )?;

            if let Some(active_phase_id) = active_phase_to_demote
                && phase_owner::ensure_phase_write_allowed(
                    ctx.root,
                    ctx.project,
                    &ctx.db_path(),
                    &active_phase_id,
                )
                .is_ok()
            {
                writer.update_phase_status(&active_phase_id, "pending")?;
            }

            if next_phase_status != "in-progress" {
                writer.update_phase_status(&next_phase, "in-progress")?;
            }
            if let Some(workspace_root) = agent_ctx.workspace_root_key() {
                writer.set_workspace_active_phase(&workspace_root, &next_phase)?;
            }

            (next_phase, next_phase_title, owner_transition)
        };

        #[derive(Serialize)]
        struct PhaseStartOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
            title: String,
            owner: PhaseOwnerView,
            took_over: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            previous_owner: Option<PhaseOwnerView>,
        }

        let output = PhaseStartOutput {
            kind: "phase.start",
            ok: true,
            phase_id: next_phase.clone(),
            title: next_phase_title.clone(),
            owner: owner_transition.owner,
            took_over: owner_transition.took_over,
            previous_owner: owner_transition.previous_owner,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = if output.took_over {
                    format!("Started phase \"{next_phase_title}\" (took ownership)")
                } else {
                    format!("Started phase \"{next_phase_title}\"")
                };
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

fn find_startable_phase(context: &AgentContext, id: &str) -> Option<(String, String, String)> {
    context
        .plan
        .epochs
        .iter()
        .flat_map(|epoch| &epoch.phases)
        .find(|phase| phase.id == id && matches!(phase.status.as_str(), "pending" | "in-progress"))
        .map(|phase| (phase.id.clone(), phase.title.clone(), phase.status.clone()))
}

pub(crate) fn phase_has_started_work(
    loader: &SqliteLoader,
    phase: &crate::context::Phase,
) -> ExoResult<bool> {
    if phase.goals.iter().any(|goal| {
        goal.status != "pending" || goal.started_at.is_some() || goal.completion_log.is_some()
    }) {
        return Ok(true);
    }

    loader
        .database()
        .connection()
        .query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM tasks_data t
                 JOIN goals_data g ON t.goal_id = g.id
                 JOIN phases_data p ON g.phase_id = p.id
                 WHERE p.text_id = ?1
                   AND (
                       t.status != 'pending'
                       OR t.started_at IS NOT NULL
                       OR t.completion_log IS NOT NULL
                       OR EXISTS (SELECT 1 FROM task_logs l WHERE l.task_id = t.id)
                       OR EXISTS (SELECT 1 FROM task_verifications v WHERE v.task_id = t.id)
                   )
             )",
            [&phase.id],
            |row| row.get(0),
        )
        .with_context(|| format!("Failed to inspect task activity for phase '{}'", phase.id))
}

/// Find the next phase to start: prefer the workspace-active phase, then the first pending phase.
fn find_next_pending_phase(context: &AgentContext) -> Option<(String, String, String)> {
    if let Ok(Some(active)) = context.find_workspace_active_phase() {
        return Some((
            active.phase.id.clone(),
            active.phase.title.clone(),
            active.phase.status.clone(),
        ));
    }

    // Then, look for the first pending phase
    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.status == "pending" {
                return Some((phase.id.clone(), phase.title.clone(), phase.status.clone()));
            }
        }
    }
    None
}

// ===== phase focus/release =====

#[derive(Debug, Clone)]
pub struct PhaseFocus {
    phase_id: String,
}

impl PhaseFocus {
    pub fn new(phase_id: impl Into<String>) -> Self {
        Self {
            phase_id: phase_id.into(),
        }
    }
}

impl Command for PhaseFocus {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "focus"
    }

    fn description(&self) -> &'static str {
        "Focus a phase without claiming ownership"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseFocus should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseFocus {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let phase_title = agent_ctx
            .plan
            .epochs
            .iter()
            .flat_map(|epoch| epoch.phases.iter())
            .find(|phase| phase.id == self.phase_id)
            .map(|phase| phase.title.clone())
            .ok_or_else(|| anyhow::anyhow!("Phase not found: {}", self.phase_id))?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        if let Some(workspace_root) = agent_ctx.workspace_root_key() {
            writer.set_workspace_active_phase(&workspace_root, &self.phase_id)?;
        }

        #[derive(Serialize)]
        struct PhaseFocusOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
            title: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            owner: Option<PhaseOwnerView>,
        }

        let owner = phase_owner::owner_view_for_phase(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &self.phase_id,
        )?;
        let output = PhaseFocusOutput {
            kind: "phase.focus",
            ok: true,
            phase_id: self.phase_id.clone(),
            title: phase_title.clone(),
            owner,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Focused phase \"{phase_title}\" (read-only unless owned here)"),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhaseRelease {
    phase_id: String,
}

impl PhaseRelease {
    pub fn new(phase_id: impl Into<String>) -> Self {
        Self {
            phase_id: phase_id.into(),
        }
    }
}

impl Command for PhaseRelease {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "release"
    }

    fn description(&self) -> &'static str {
        "Release this workspace's or a stale phase owner"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseRelease should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseRelease {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let phase_title = AgentContext::load(ctx.root.to_path_buf())?
            .plan
            .epochs
            .iter()
            .flat_map(|epoch| epoch.phases.iter())
            .find(|phase| phase.id == self.phase_id)
            .map(|phase| phase.title.clone())
            .ok_or_else(|| anyhow::anyhow!("Phase not found: {}", self.phase_id))?;
        let released = phase_owner::release_phase_owner(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &self.phase_id,
        )?;

        #[derive(Serialize)]
        struct PhaseReleaseOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
            title: String,
            released: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            previous_owner: Option<PhaseOwnerView>,
        }

        let output = PhaseReleaseOutput {
            kind: "phase.release",
            ok: true,
            phase_id: self.phase_id.clone(),
            title: phase_title.clone(),
            released: released.is_some(),
            previous_owner: released,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Released phase owner for \"{phase_title}\""),
            )),
        }
    }
}

fn read_last_executed_phase_id(_root: &Path, context: &AgentContext) -> Option<String> {
    if let Ok(anchor) = context.workspace_anchor_phase_id()
        && anchor.is_some()
    {
        return anchor;
    }

    // Find the last completed phase in plan order.
    // This is the most reliable anchor — works on both TOML and SQLite backends.
    let mut last_completed = None;
    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.status == "completed" {
                last_completed = Some(phase.id.clone());
            }
        }
    }
    last_completed
}

fn suggest_next_phase(context: &AgentContext) -> Option<(String, String, String)> {
    let anchor = read_last_executed_phase_id(&context.root, context);

    // Pass 1: if we have an anchor, find the first pending phase after it.
    if let Some(anchor_id) = anchor {
        let mut seen_anchor = false;
        for epoch in &context.plan.epochs {
            for phase in &epoch.phases {
                if !seen_anchor {
                    if phase.id == anchor_id {
                        seen_anchor = true;
                    }
                    continue;
                }

                if phase.status == "pending" {
                    return Some((phase.id.clone(), phase.title.clone(), epoch.title.clone()));
                }
            }
        }
    }

    // Pass 2: fall back to the first pending phase anywhere in plan order.
    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if phase.status == "pending" {
                return Some((phase.id.clone(), phase.title.clone(), epoch.title.clone()));
            }
        }
    }

    None
}

// ===== phase status =====

/// Get current phase status
#[derive(Debug, Clone, Copy)]
pub struct PhaseStatus {
    #[allow(dead_code)] // Placeholder for future filtering
    show_main: bool,
}

impl PhaseStatus {
    pub const fn new(show_main: bool) -> Self {
        Self { show_main }
    }
}

impl Command for PhaseStatus {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "Get the status of the current phase"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "List phase tasks".to_string(),
                command: "exo task list".to_string(),
                rationale: "View all tasks in the current phase".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Show phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View goals and tasks for the current phase".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Check upgrade status".to_string(),
                command: "exo upgrade check".to_string(),
                rationale: "Verify if any upgrades are blocking phase operations".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let context = AgentContext::load(ctx.root.to_path_buf())?;

        let active_info = context.find_workspace_active_phase()?;
        let strike_goal = active_info.as_ref().and_then(|info| {
            info.phase
                .goals
                .iter()
                .find(|task| task.kind.as_deref() == Some("strike") && task.status == "in-progress")
        });

        let show_strike_overlay =
            ctx.format == OutputFormat::Human && !self.show_main && strike_goal.is_some();

        if show_strike_overlay && let Some(strike) = strike_goal {
            let mut output = String::new();
            output.push_str(&format!(
                "🔴 SURGICAL STRIKE: {}\n========================================\n",
                strike.label
            ));
            let strike_goal = strike.description.as_deref().unwrap_or("(no goal)");
            let started = strike
                .started_at
                .as_ref()
                .map_or_else(|| "(unknown)".to_string(), chrono::DateTime::to_rfc3339);
            output.push_str(&format!("Goal: {strike_goal}\nStarted: {started}\n\n"));

            output.push_str("## Tasks\n\n");
            let strike_tasks: Vec<_> = active_info
                .as_ref()
                .map(|info| {
                    info.phase
                        .goals
                        .iter()
                        .filter(|task| task.id != strike.id)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if strike_tasks.is_empty() {
                output.push_str("No goals defined.\n");
            } else {
                output.push_str("| ID | Label | Status |\n");
                output.push_str("| :--- | :--- | :--- |\n");
                for task in strike_tasks {
                    output.push_str(&format!(
                        "| {} | {} | {} |\n",
                        task.id, task.label, task.status
                    ));
                }
            }

            output.push_str("\n----------------------------------------\n");
            if let Some(info) = active_info {
                output.push_str(&format!("Main Phase: {} (Background)\n", info.phase.title));
            } else {
                output.push_str("Main Phase: None\n");
            }
            output.push_str("(Use --full to view main phase context)\n");
            return Ok(CommandOutput::message(output));
        }

        let active_info = context.find_workspace_active_phase()?;

        if let Some(info) = active_info {
            let phase = info.phase;
            let active_epoch = Some(info.epoch);

            #[derive(Serialize)]
            struct TaskRow {
                id: String,
                label: String,
                status: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                derived_reason: Option<String>,
            }

            #[derive(Serialize, Clone)]
            struct GoalRow {
                name: String,
                #[serde(rename = "type")]
                type_: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                status: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                display_status: Option<String>,
            }

            #[derive(Serialize)]
            struct PhaseStatusJson {
                phase_id: String,
                phase_title: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                epoch_title: Option<String>,
                focused_here: bool,
                owned_here: bool,
                owned_elsewhere: bool,
                stale_owner: bool,
                #[serde(skip_serializing_if = "Option::is_none")]
                owner: Option<PhaseOwnerView>,
                current_owner: CurrentOwnerView,
                #[serde(default, skip_serializing_if = "Vec::is_empty")]
                rfcs: Vec<String>,
                git_dirty: bool,
                snapshot_policy: SnapshotPolicyJson,
                snapshots: Vec<SnapshotFileStatus>,
                tasks: Vec<TaskRow>,
                goals: Vec<GoalRow>,
                steering: SteeringBlock,
            }

            #[derive(Serialize)]
            struct SnapshotPolicyJson {
                expected_disk_state: String,
                rationale: String,
                repair_command: String,
            }

            let world = WorldState::probe(&context)?;
            let steering = steering::derive_world_steering(&world, ctx.agent_id.as_deref());
            let owner = phase_owner::owner_view_for_phase(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                &phase.id,
            )?;
            let current_owner = phase_owner::current_owner_view(ctx.root, ctx.project);

            let tasks_json = world
                .tasks
                .iter()
                .map(|(id, label, status)| TaskRow {
                    id: id.clone(),
                    label: label.clone(),
                    status: status.clone(),
                    derived_reason: None,
                })
                .collect::<Vec<_>>();

            let mut goal_task_progress: HashMap<String, (usize, usize)> = HashMap::new();
            for (task_id, _, status) in &world.tasks {
                let Some((goal_id, _)) = task_id.split_once("::") else {
                    continue;
                };

                let entry = goal_task_progress
                    .entry(goal_id.to_string())
                    .or_insert((0, 0));
                entry.0 += 1;
                if status == "completed" {
                    entry.1 += 1;
                }
            }

            let goals_json = world
                .goals
                .iter()
                .map(|goal| GoalRow {
                    name: if goal.label.is_empty() {
                        goal.id.clone()
                    } else {
                        goal.label.clone()
                    },
                    type_: goal.kind.as_deref().unwrap_or("regular").to_string(),
                    status: if goal.status.is_empty() {
                        None
                    } else {
                        Some(goal.status.clone())
                    },
                    display_status: goal_task_progress.get(&goal.id).and_then(
                        |(total, completed)| {
                            (goal.status == "pending" && *total > 0 && *total == *completed)
                                .then_some("done?".to_string())
                        },
                    ),
                })
                .collect::<Vec<_>>();

            let out = PhaseStatusJson {
                phase_id: phase.id.clone(),
                phase_title: phase.title.clone(),
                epoch_title: active_epoch.map(|e| e.title.clone()),
                focused_here: true,
                owned_here: owner.as_ref().is_some_and(|owner| owner.owned_here),
                owned_elsewhere: owner.as_ref().is_some_and(|owner| owner.owned_elsewhere),
                stale_owner: owner.as_ref().is_some_and(|owner| owner.stale),
                owner,
                current_owner,
                rfcs: phase.rfcs.iter().map(|rfc| rfc.id.clone()).collect(),
                git_dirty: world.git_dirty,
                snapshot_policy: SnapshotPolicyJson {
                    expected_disk_state: "read-only".to_string(),
                    rationale: "CLI-managed context files are intentionally read-only on disk; use `exo` commands to update them.".to_string(),
                    repair_command: "exo update".to_string(),
                },
                snapshots: world.current_snapshots,
                tasks: tasks_json,
                goals: goals_json,
                steering,
            };

            if ctx.format == OutputFormat::Json {
                return Ok(CommandOutput::data(out));
            }

            let mut output = String::new();
            output.push_str(&format!("# Phase Status: {}\n\n", phase.title));
            if let Some(epoch) = active_epoch {
                output.push_str(&format!("**Epoch**: {}\n\n", epoch.title));
            }
            if let Some(owner) = &out.owner {
                let owner_state = if owner.stale {
                    "stale owner"
                } else if owner.owned_here {
                    "owned here"
                } else {
                    "owned elsewhere"
                };
                let owner_basis = phase_owner::current_owner_basis_label(&out.current_owner);
                output.push_str(&format!(
                    "**Ownership**: {} ({owner_state}; basis: {owner_basis})\n\n",
                    owner.label
                ));
            } else {
                let owner_basis = phase_owner::current_owner_basis_label(&out.current_owner);
                output.push_str(&format!(
                    "**Ownership**: unowned (basis: {owner_basis})\n\n"
                ));
            }
            if !phase.rfcs.is_empty() {
                let rfcs = phase
                    .rfcs
                    .iter()
                    .map(|rfc| rfc.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!("**RFCs**: {rfcs}\n\n"));
            }

            // Execution tasks from SQLite
            output.push_str("## Tasks\n\n");

            if world.tasks.is_empty() {
                output.push_str("No tasks defined.\n\n");
            } else {
                for (id, label, status) in &world.tasks {
                    let task_icon = match status.as_str() {
                        "completed" => "✓",
                        "in-progress" => "→",
                        "skipped" => "⏭",
                        _ => "○",
                    };
                    output.push_str(&format!("- {task_icon} {label} ({id}) — *{status}*\n"));
                }
                output.push('\n');
            }

            // Goals from SQLite
            output.push_str("## Goals\n\n");

            if world.goals.is_empty() {
                output.push_str("No goals defined.\n\n");
                output.push_str("*Hint: Use `exo goal add` to define goals for this phase.*\n");
            } else {
                for goal in &world.goals {
                    let goal_name = if goal.label.is_empty() {
                        goal.id.as_str()
                    } else {
                        goal.label.as_str()
                    };
                    let display_done =
                        goal_task_progress
                            .get(&goal.id)
                            .is_some_and(|(total, completed)| {
                                goal.status == "pending" && *total > 0 && *total == *completed
                            });
                    let goal_status = if display_done {
                        "done?"
                    } else {
                        goal.status.as_str()
                    };
                    let goal_kind = goal.kind.as_deref();

                    let status_icon = match goal_status {
                        "done?" => "🟡",
                        "completed" => "✅",
                        "in-progress" => "🔄",
                        "skipped" => "⏭️",
                        "aborted" => "⛔",
                        _ => "○",
                    };

                    let kind_badge = if goal_kind == Some("strike") {
                        "⚡ "
                    } else {
                        ""
                    };

                    output.push_str(&format!(
                        "### {status_icon} {kind_badge}{goal_name} — *{goal_status}*\n\n"
                    ));

                    if let Some(desc) = goal.description.as_deref() {
                        output.push_str(&format!("{desc}\n\n"));
                    }
                }
            }

            output.push_str("\n## Context Check\n\n");
            output.push_str("| Document | Status | Action Required |\n");
            output.push_str("| :--- | :--- | :--- |\n");

            return Ok(CommandOutput::new(out, output));
        } else if ctx.format == OutputFormat::Json {
            #[derive(Serialize)]
            struct PhaseStatusJson {
                phase_id: Option<String>,
                phase_title: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                next_phase_id: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                next_phase_title: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                next_epoch_title: Option<String>,
                current_owner: CurrentOwnerView,
                git_dirty: bool,
                snapshot_policy: SnapshotPolicyJson,
                snapshots: Vec<SnapshotFileStatus>,
                steering: SteeringBlock,
            }

            #[derive(Serialize)]
            struct SnapshotPolicyJson {
                expected_disk_state: String,
                rationale: String,
                repair_command: String,
            }

            let world = WorldState::probe(&context)?;

            let steering = steering::derive_world_steering(&world, ctx.agent_id.as_deref());

            let (next_phase_id, next_phase_title, next_epoch_title) = suggest_next_phase(&context)
                .map_or((None, None, None), |(pid, ptitle, etitle)| {
                    (Some(pid), Some(ptitle), Some(etitle))
                });

            let out = PhaseStatusJson {
                phase_id: None,
                phase_title: None,
                next_phase_id,
                next_phase_title,
                next_epoch_title,
                current_owner: phase_owner::current_owner_view(ctx.root, ctx.project),
                git_dirty: world.git_dirty,
                snapshot_policy: SnapshotPolicyJson {
                    expected_disk_state: "read-only".to_string(),
                    rationale: "CLI-managed context files are intentionally read-only on disk; use `exo` commands to update them.".to_string(),
                    repair_command: "exo update".to_string(),
                },
                snapshots: world.current_snapshots,
                steering,
            };

            return Ok(CommandOutput::data(out));
        }

        let mut output = String::new();
        output.push_str("No active phase found.\n");

        if let Some((phase_id, phase_title, epoch_title)) = suggest_next_phase(&context) {
            output.push_str(&format!(
                "Next phase to start: [{phase_id}] {phase_title} (Epoch: {epoch_title})\n"
            ));
            output.push_str(&format!("Run: exo phase start {phase_id}\n"));
        } else {
            output.push_str("No pending phases found in plan.\n");
            output.push_str("Run: exo plan review\n");
        }

        Ok(CommandOutput::message(output))
    }
}

// Phase status is implemented inline in main.rs due to its complexity
// and deep integration with world_state, steering, and derived modules.
// It doesn't have a simple phase module function to delegate to.
// This is a pure read operation, so it implements Command but not MutableCommand.

// ===== phase execution tasks =====

/// List phase execution tasks.
#[derive(Debug, Clone, Default)]
pub struct PhaseExecutionTasks {
    cursor: Option<String>,
    limit: Option<usize>,
}

impl PhaseExecutionTasks {
    pub const fn new(cursor: Option<String>, limit: Option<usize>) -> Self {
        Self { cursor, limit }
    }
}

impl Command for PhaseExecutionTasks {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "execution.tasks"
    }

    fn description(&self) -> &'static str {
        "List phase execution tasks"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let tasks = crate::task::list_execution_tasks(ctx.root)?;
        let result = paginate_items(
            &tasks,
            self.cursor.as_deref(),
            self.limit,
            |(id, label, status)| {
                json!({
                    "id": id,
                    "title": label,
                    "status": status,
                })
            },
        );

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&result)?;
                Ok(CommandOutput::new(result, pretty))
            }
        }
    }
}

fn paginate_items<T, F>(
    items: &[T],
    cursor: Option<&str>,
    limit: Option<usize>,
    map_fn: F,
) -> serde_json::Value
where
    F: Fn(&T) -> serde_json::Value,
{
    const DEFAULT_LIMIT: usize = 20;

    let start = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let end = (start + limit).min(items.len());
    let has_more = end < items.len();
    let next_cursor = if has_more {
        Some(end.to_string())
    } else {
        None
    };

    let mapped: Vec<serde_json::Value> = (start..end).map(|i| map_fn(&items[i])).collect();

    json!({
        "items": mapped,
        "next_cursor": next_cursor,
        "has_more": has_more,
    })
}

// ===== phase add =====

/// Add a new phase to an epoch (defaults to active epoch).
#[derive(Debug, Clone)]
pub struct PhaseAdd {
    pub title: String,
    pub epoch: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub first: bool,
    pub rfcs: Option<Vec<String>>,
    pub kind: PhaseKind,
}

impl PhaseAdd {
    pub fn new(
        title: impl Into<String>,
        after: Option<String>,
        before: Option<String>,
        first: bool,
        rfcs: Option<Vec<String>>,
        kind: PhaseKind,
    ) -> Self {
        Self {
            title: title.into(),
            epoch: None,
            after,
            before,
            first,
            rfcs,
            kind,
        }
    }

    pub fn with_epoch(mut self, epoch: Option<String>) -> Self {
        self.epoch = epoch;
        self
    }
}

#[derive(Debug, Serialize)]
struct PhaseAddOutput {
    kind: &'static str,
    ok: bool,
    epoch_id: String,
    id: String,
    title: String,
    status: &'static str,
    position: String,
}

impl Command for PhaseAdd {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new phase to an epoch (defaults to active epoch)"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_phase_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let epoch_id = if let Some(ref explicit) = self.epoch {
            explicit.clone()
        } else {
            let Some(epoch) = agent_ctx.find_workspace_active_epoch()? else {
                anyhow::bail!("No active epoch found. Use --epoch <id> to specify an epoch.")
            };
            epoch.id.clone()
        };

        let id = {
            let writer = SqliteWriter::open(ctx.db_path())?;
            let id = writer.add_phase(&epoch_id, &self.title, self.kind.as_str(), None, &[])?;

            // Apply ordering if specified
            if self.first {
                writer.reorder_phase(&id, "top")?;
            } else if let Some(ref after_id) = self.after {
                writer.reorder_phase(&id, &format!("after:{after_id}"))?;
            } else if let Some(ref before_id) = self.before {
                writer.reorder_phase(&id, &format!("before:{before_id}"))?;
            }

            id
        };

        let output = PhaseAddOutput {
            kind: "phase.add",
            ok: true,
            epoch_id: epoch_id.clone(),
            id: id.clone(),
            title: self.title.clone(),
            status: "pending",
            position: phase_add_position(self.first, self.after.as_deref(), self.before.as_deref()),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!(
                    "Added phase '{}': {} (to epoch {}, status pending, position {})\n→ Next: exo goal add \"Goal label\" --phase {}",
                    id, self.title, epoch_id, output.position, id
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

fn phase_add_position(first: bool, after: Option<&str>, before: Option<&str>) -> String {
    if first {
        "top".to_string()
    } else if let Some(after_id) = after {
        format!("after:{after_id}")
    } else if let Some(before_id) = before {
        format!("before:{before_id}")
    } else {
        "bottom".to_string()
    }
}

// ===== phase remove =====

/// Remove a phase.
#[derive(Debug, Clone)]
pub struct PhaseRemove {
    pub id: String,
}

impl PhaseRemove {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct PhaseRemoveOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for PhaseRemove {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove a phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_phase_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseRemove should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), &self.id)?;
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.remove_phase(&self.id)?;

        let output = PhaseRemoveOutput {
            kind: "phase.remove",
            ok: true,
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Removed phase '{}'", self.id);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ===== phase update =====

/// Update phase metadata (title, RFCs)
#[derive(Debug, Clone)]
pub struct PhaseUpdate {
    phase_id: String,
    title: Option<String>,
    rfcs: Option<Vec<String>>,
}

impl PhaseUpdate {
    pub const fn new(phase_id: String, title: Option<String>, rfcs: Option<Vec<String>>) -> Self {
        Self {
            phase_id,
            title,
            rfcs,
        }
    }
}

impl Command for PhaseUpdate {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "update"
    }

    fn description(&self) -> &'static str {
        "Update phase metadata (title, RFCs)"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "View phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View current phase information before updating".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "List phases".to_string(),
                command: "exo plan read".to_string(),
                rationale: "See all phases to find the correct phase ID".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseUpdate should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseUpdate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        {
            let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
            let phase_exists = agent_ctx
                .plan
                .epochs
                .iter()
                .any(|epoch| epoch.phases.iter().any(|phase| phase.id == self.phase_id));

            if !phase_exists {
                anyhow::bail!("Phase not found: {}", self.phase_id);
            }
            phase_owner::ensure_phase_write_allowed(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                &self.phase_id,
            )?;

            let writer = SqliteWriter::open(ctx.db_path())?;
            if let Some(title) = self.title.as_deref() {
                writer.update_phase_title(&self.phase_id, title)?;
            }
            if let Some(rfcs) = self.rfcs.as_ref() {
                writer.replace_phase_rfcs(&self.phase_id, rfcs)?;
            }
        }

        #[derive(Serialize)]
        struct PhaseUpdateOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
        }

        let output = PhaseUpdateOutput {
            kind: "phase.update",
            ok: true,
            phase_id: self.phase_id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Updated phase: {}", self.phase_id);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ===== phase reorder =====

/// Reorder a phase within its epoch.
#[derive(Debug, Clone)]
pub struct PhaseReorder {
    phase_id: String,
    position: String,
}

impl PhaseReorder {
    pub fn new(phase_id: impl Into<String>, position: impl Into<String>) -> Self {
        Self {
            phase_id: phase_id.into(),
            position: position.into(),
        }
    }
}

impl Command for PhaseReorder {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "reorder"
    }

    fn description(&self) -> &'static str {
        "Reorder a phase within its epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_phase_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseReorder should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseReorder {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        phase_owner::ensure_phase_write_allowed(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &self.phase_id,
        )?;
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.reorder_phase(&self.phase_id, &self.position)?;

        #[derive(Serialize)]
        struct PhaseReorderOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
            position: String,
        }

        let output = PhaseReorderOutput {
            kind: "phase.reorder",
            ok: true,
            phase_id: self.phase_id.clone(),
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Moved phase '{}' to {}", self.phase_id, self.position),
            )),
        }
    }
}

// ===== phase move =====

/// Move a phase to another epoch.
#[derive(Debug, Clone)]
pub struct PhaseMove {
    phase_id: String,
    epoch_id: String,
    position: Option<String>,
}

impl PhaseMove {
    pub fn new(
        phase_id: impl Into<String>,
        epoch_id: impl Into<String>,
        position: Option<String>,
    ) -> Self {
        Self {
            phase_id: phase_id.into(),
            epoch_id: epoch_id.into(),
            position,
        }
    }
}

impl Command for PhaseMove {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "move"
    }

    fn description(&self) -> &'static str {
        "Move a phase to another epoch"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_phase_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseMove should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseMove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        phase_owner::ensure_phase_write_allowed(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &self.phase_id,
        )?;
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.move_phase_to_epoch(&self.phase_id, &self.epoch_id, self.position.as_deref())?;

        #[derive(Serialize)]
        struct PhaseMoveOutput {
            kind: &'static str,
            ok: bool,
            phase_id: String,
            epoch_id: String,
            position: Option<String>,
        }

        let output = PhaseMoveOutput {
            kind: "phase.move",
            ok: true,
            phase_id: self.phase_id.clone(),
            epoch_id: self.epoch_id.clone(),
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "Moved phase '{}' to epoch '{}'",
                    self.phase_id, self.epoch_id
                ),
            )),
        }
    }
}

// ===== phase finish =====

/// Complete the current phase
#[derive(Debug, Clone)]
pub struct PhaseFinish {
    message: Option<String>,
}

impl PhaseFinish {
    pub const fn new(message: Option<String>) -> Self {
        Self { message }
    }
}

impl Command for PhaseFinish {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "finish"
    }

    fn description(&self) -> &'static str {
        "Finish the current phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Check phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "Review current phase completion status before finishing".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.9),
            },
            SuggestedAction {
                label: "Check git status".to_string(),
                command: "git status".to_string(),
                rationale: "Verify all changes are committed before finishing phase".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "List incomplete tasks".to_string(),
                command: "exo task list --status pending".to_string(),
                rationale: "Ensure all tasks are completed before finishing phase".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("PhaseFinish should be dispatched via execute_mut")
    }
}

impl MutableCommand for PhaseFinish {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let active_phase_id = agent_ctx.find_workspace_active_phase_id()?;
        if let Some(active_phase_id) = active_phase_id.as_deref() {
            phase_owner::ensure_phase_write_allowed(
                ctx.root,
                ctx.project,
                &ctx.db_path(),
                active_phase_id,
            )?;
        }

        // finish_phase is backend-agnostic: git check, RFC collection,
        // status update, next-phase scan.
        let result = phase::finish_phase(
            ctx.root,
            &ctx.db_path(),
            &agent_ctx.plan,
            active_phase_id.clone(),
            self.message.clone(),
            false,
        )?;
        if let Some(active_phase_id) = active_phase_id.as_deref() {
            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.clear_phase_owner(active_phase_id)?;
        }
        let (rfc_suggestions, next_phase) = (result.rfc_suggestions, result.next_phase);

        #[derive(Serialize)]
        struct PhaseFinishOutput {
            kind: &'static str,
            ok: bool,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            rfc_suggestions: Vec<phase::RfcSuggestion>,
            #[serde(skip_serializing_if = "Option::is_none")]
            next_phase: Option<phase::NextPhaseInfo>,
        }

        let output = PhaseFinishOutput {
            kind: "phase.finish",
            ok: true,
            rfc_suggestions,
            next_phase,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, "Phase finished successfully")),
        }
    }
}

// ===== phase history =====

/// Show history of completed phases
#[derive(Debug, Clone, Copy)]
pub struct PhaseHistory {
    limit: Option<usize>,
}

impl PhaseHistory {
    pub const fn new(limit: Option<usize>) -> Self {
        Self { limit }
    }
}

#[derive(Serialize)]
struct PhaseHistoryOutput {
    kind: &'static str,
    phases: Vec<PhaseHistoryEntry>,
}

#[derive(Serialize, Clone)]
struct PhaseHistoryEntry {
    id: String,
    title: String,
    epoch_id: String,
    epoch_title: String,
    status: String,
}

impl Command for PhaseHistory {
    fn namespace(&self) -> &'static str {
        "phase"
    }

    fn operation(&self) -> &'static str {
        "history"
    }

    fn description(&self) -> &'static str {
        "Show history of completed phases"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Show current phase".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View the current active phase".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "List all phases".to_string(),
                command: "exo plan read".to_string(),
                rationale: "See all phases in the plan".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        // Load plan through configured storage backend
        let context = AgentContext::load(ctx.root.to_path_buf())?;
        let plan = context.plan;

        // Collect completed phases from all epochs
        let mut phases: Vec<PhaseHistoryEntry> = Vec::new();
        for epoch in &plan.epochs {
            for phase in &epoch.phases {
                if phase.status == "completed" {
                    phases.push(PhaseHistoryEntry {
                        id: phase.id.clone(),
                        title: phase.title.clone(),
                        epoch_id: epoch.id.clone(),
                        epoch_title: epoch.title.clone(),
                        status: phase.status.clone(),
                    });
                }
            }
        }

        // Apply limit if specified
        if let Some(limit) = self.limit {
            phases.truncate(limit);
        }

        let output = PhaseHistoryOutput {
            kind: "phase.history",
            phases: phases.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut text = String::from("# Phase History\n\n");
                if phases.is_empty() {
                    text.push_str("No completed phases found.\n");
                } else {
                    text.push_str("| Phase | Title | Epoch |\n");
                    text.push_str("| :--- | :--- | :--- |\n");
                    for phase in &phases {
                        text.push_str(&format!(
                            "| {} | {} | {} |\n",
                            phase.id, phase.title, phase.epoch_title
                        ));
                    }
                }
                Ok(CommandOutput::new(output, text))
            }
        }
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_start_metadata() {
        let cmd = PhaseStart::new(None, false);
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "start");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_phase_status_metadata() {
        let cmd = PhaseStatus::new(false);
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "status");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_phase_finish_metadata() {
        let cmd = PhaseFinish::new(Some("Complete phase".to_string()));
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "finish");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_phase_add_metadata() {
        let cmd = PhaseAdd::new(
            "Test Phase".to_string(),
            None,
            None,
            false,
            None,
            PhaseKind::default(),
        );
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_phase_remove_metadata() {
        let cmd = PhaseRemove::new("test-phase");
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "remove");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_phase_update_metadata() {
        let cmd = PhaseUpdate::new(
            "test-phase".to_string(),
            Some("New Title".to_string()),
            None,
        );
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "update");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_phase_history_metadata() {
        let cmd = PhaseHistory::new(Some(10));
        assert_eq!(cmd.namespace(), "phase");
        assert_eq!(cmd.operation(), "history");
        assert_eq!(cmd.effect(), Effect::Pure);
    }
}
