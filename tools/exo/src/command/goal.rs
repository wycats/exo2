//! Goal namespace commands.
//!
//! - `goal add`: Add a new goal (Write)
//! - `goal list`: List goals in the active phase (Pure)
//! - `goal reorder`: Reorder goals in the active phase (Write)
//! - `goal complete`: Record a goal's completed outcome (Write)
//! - `goal abandon`: Abandon a goal with a log message (Write)
//! - `goal remove`: Remove a goal from the active phase (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::{AgentContext, SqliteLoader, SqliteWriter};
use crate::phase_owner;
use crate::steering::{SuggestedAction, WorkIntent};
use crate::task;
use anyhow::Result as ExoResult;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Default steering for goal commands.
fn default_goal_steering() -> Vec<SuggestedAction> {
    vec![
        SuggestedAction {
            label: "List goals".to_string(),
            command: "exo goal list".to_string(),
            rationale: "View all goals in the active phase.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.6),
        },
        SuggestedAction {
            label: "Show phase status".to_string(),
            command: "exo phase status".to_string(),
            rationale: "See goals alongside execution context.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.5),
        },
    ]
}

// ============================================================================
// ExoSpec definition — single source of truth for the goal namespace
// ============================================================================

/// Goal namespace command specification.
///
/// This enum is the authoritative definition of the goal namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `GoalCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "goal", description = "Goal management commands")]
pub enum GoalCommands {
    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Add a new goal to a phase (defaults to active phase)"
    )]
    Add {
        #[exo(
            positional,
            description = "The goal label (ID auto-generated from label if --id omitted)"
        )]
        label: String,
        #[exo(
            long,
            optional,
            description = "Read the label from a file (or '-' for stdin)"
        )]
        label_file: Option<String>,
        #[exo(
            long,
            optional,
            description = "Explicit goal ID (auto-generated from label if omitted)"
        )]
        id: Option<String>,
        #[exo(
            long,
            optional,
            description = "RFC ID this goal is advancing (enables pipeline-aware steering)"
        )]
        rfc: Option<String>,
        #[exo(
            long,
            optional,
            description = "Target stage for RFC promotion (e.g., 2 for Stage 1→2)"
        )]
        target_stage: Option<i64>,
        #[exo(
            long,
            optional,
            description = "Target phase ID (defaults to active phase)"
        )]
        phase: Option<String>,
    },

    #[exo(effect = "pure", description = "List goals in the active phase")]
    List,

    #[exo(effect = "write", description = "Reorder a goal within its phase")]
    Reorder {
        #[exo(positional, description = "The goal ID to reorder")]
        id: String,
        #[exo(
            positional,
            description = "Target position: top, bottom, before:<id>, or after:<id>"
        )]
        position: String,
    },

    #[exo(effect = "write", description = "Move a goal to another phase")]
    Move {
        #[exo(positional, description = "The goal ID to move")]
        id: String,
        #[exo(long, description = "Target phase ID")]
        phase: String,
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
        description = "Abandon a goal with a log message"
    )]
    Abandon {
        #[exo(positional, description = "The goal ID to abandon")]
        id: String,
        #[exo(long, description = "Abandonment log message (required)")]
        log: String,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Record a goal's completed outcome"
    )]
    Complete {
        #[exo(positional, description = "The goal ID to complete")]
        id: String,
        #[exo(
            long,
            default = "Completed",
            description = "Completed outcome summary (stored as the completion log; defaults to 'Completed' if omitted)"
        )]
        log: String,
    },

    #[exo(
        effect = "write",
        upgrade_gate,
        description = "Remove a goal from a phase (defaults to active phase)"
    )]
    Remove {
        #[exo(positional, description = "The goal ID to remove")]
        id: String,
        #[exo(
            long,
            optional,
            description = "Target phase ID (defaults to active phase)"
        )]
        phase: Option<String>,
    },

    #[exo(effect = "write", description = "Update a goal's label")]
    Update {
        #[exo(positional, description = "The goal ID to update")]
        id: String,
        #[exo(positional, description = "The new label for the goal")]
        label: String,
    },
}

impl GoalCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    ///
    /// Takes `root` to resolve file-based arguments (e.g., `--label-file`).
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Add {
                label,
                label_file,
                id,
                rfc,
                target_stage,
                phase,
            } => {
                // Resolve label from file if provided, otherwise use direct value
                let label = if let Some(file_path) = label_file {
                    crate::utils::read_text_input(root, &file_path)?
                } else {
                    label
                };
                let id = id.unwrap_or_else(|| {
                    let slug = crate::utils::slugify(&label);
                    if slug.is_empty() {
                        "untitled".to_string()
                    } else {
                        slug
                    }
                });
                let target_stage = target_stage.and_then(|value| u8::try_from(value).ok());
                CommandBox::mutable(GoalAdd::new(id, label, rfc, target_stage).with_phase(phase))
            }
            Self::List => CommandBox::pure(GoalList::new()),
            Self::Reorder { id, position } => CommandBox::mutable(GoalReorder::new(id, position)),
            Self::Move {
                id,
                phase,
                position,
            } => CommandBox::mutable(GoalMove::new(id, phase, position)),
            Self::Abandon { id, log } => CommandBox::mutable(GoalAbandon::new(id, log)),
            Self::Complete { id, log } => CommandBox::mutable(GoalComplete::new(id, log)),
            Self::Remove { id, phase } => {
                CommandBox::mutable(GoalRemove::new(id).with_phase(phase))
            }
            Self::Update { id, label } => CommandBox::mutable(GoalUpdate::new(id, label)),
        })
    }
}

fn active_phase_id(ctx: &AgentContext) -> Option<String> {
    ctx.find_workspace_active_phase_id().ok().flatten()
}

fn ensure_phase_write_allowed(ctx: &MutableCommandContext, phase_id: &str) -> ExoResult<()> {
    phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), phase_id)
}

fn require_goal_phase_id(ctx: &MutableCommandContext, goal_id: &str) -> ExoResult<String> {
    let loader = SqliteLoader::open(ctx.db_path())?;
    loader
        .resolve_entity_tree("goal", goal_id)?
        .into_iter()
        .find_map(|(entity_type, entity_id)| (entity_type == "phase").then_some(entity_id))
        .ok_or_else(|| anyhow::anyhow!("Goal not found: {goal_id}"))
}

fn ensure_goal_requested_phase(
    goal_id: &str,
    actual_phase: &str,
    requested_phase: &str,
) -> ExoResult<()> {
    if actual_phase != requested_phase {
        anyhow::bail!(
            "Goal '{goal_id}' belongs to phase '{actual_phase}', not phase '{requested_phase}'."
        );
    }
    Ok(())
}

fn load_impl_goal_task_counts(
    root: &Path,
    project: Option<&crate::project::Project>,
) -> ExoResult<HashMap<String, usize>> {
    use crate::context::SqliteLoader;
    let db_path = crate::context::db_path(root, project);
    if !db_path.exists() {
        return Ok(HashMap::new());
    }
    let loader = SqliteLoader::open(&db_path)?;
    let workspace_root = project
        .and_then(|project| project.workspace_root.as_ref())
        .map(|root| root.to_string_lossy().into_owned());
    loader.count_tasks_per_goal_for_workspace(workspace_root.as_deref())
}

// Dead TOML fallback removed (~40 lines). Was unreachable: StorageBackend is always Sqlite.

fn load_goal_task_progress(root: &Path) -> ExoResult<HashMap<String, (usize, usize)>> {
    let tasks = task::list_tasks(root)?;
    let mut progress: HashMap<String, (usize, usize)> = HashMap::new();

    for (task_id, _, status) in tasks {
        let Some((goal_id, _)) = task_id.split_once("::") else {
            continue;
        };

        let entry = progress.entry(goal_id.to_string()).or_insert((0, 0));

        entry.0 += 1;
        if status == "completed" {
            entry.1 += 1;
        }
    }

    Ok(progress)
}

// ============================================================================
// goal add (Write)
// ============================================================================

/// Add a new goal to a phase (defaults to active phase).
#[derive(Debug, Clone)]
pub struct GoalAdd {
    pub id: String,
    pub label: String,
    /// RFC this goal is advancing (stored on the goal itself for pipeline awareness).
    pub rfc: Option<String>,
    /// Target stage for RFC promotion (e.g., 2 means "promote to Stage 2").
    pub target_stage: Option<u8>,
    /// Explicit phase ID (defaults to active phase if omitted).
    pub phase: Option<String>,
}

impl GoalAdd {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        rfc: Option<String>,
        target_stage: Option<u8>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            rfc,
            target_stage,
            phase: None,
        }
    }

    pub fn with_phase(mut self, phase: Option<String>) -> Self {
        self.phase = phase;
        self
    }
}

#[derive(Debug, Serialize)]
struct GoalAddOutput {
    kind: &'static str,
    ok: bool,
    phase_id: String,
    goal_id: String,
}

impl Command for GoalAdd {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new goal to a phase (defaults to active phase)"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let phase_id = if let Some(ref explicit) = self.phase {
            explicit.clone()
        } else {
            let Some(id) = active_phase_id(&agent_ctx) else {
                anyhow::bail!("No active phase. Start a phase or use --phase <id>.")
            };
            id
        };
        ensure_phase_write_allowed(ctx, &phase_id)?;

        // Add goal with optional RFC linkage (stored on the goal itself for pipeline awareness)
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.add_goal(
            &phase_id,
            &self.id,
            &self.label,
            self.rfc.as_deref(),
            self.target_stage,
            None, // kind (regular by default)
            None, // description
            None, // started_at
            None, // slug
            &[],  // aliases
        )?;

        let output = GoalAddOutput {
            kind: "goal.add",
            ok: true,
            phase_id,
            goal_id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = format!("Added goal: {}\n", self.id);
                if let (Some(rfc), Some(target)) = (&self.rfc, self.target_stage) {
                    msg.push_str(&format!("📋 Promotion goal: RFC {rfc} → Stage {target}\n"));
                } else if self.rfc.is_none() {
                    msg.push_str("⚠️ Warning: No RFC linked. Goal should be self-documenting.\n");
                }
                msg.push_str(&format!(
                    "→ Next: exo task add \"Task label\" --goal {}\n",
                    self.id
                ));
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// goal list (Pure)
// ============================================================================

/// List goals in the active phase.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoalList;

impl GoalList {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Serialize)]
struct GoalListEntry {
    id: String,
    label: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_log: Option<String>,
    rfc: Option<String>,
    task_count: usize,
    source: String,
}

#[derive(Debug, Serialize)]
struct GoalListOutput {
    kind: &'static str,
    ok: bool,
    goals: Vec<GoalListEntry>,
}

impl Command for GoalList {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List goals in the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let Some(phase_id) = active_phase_id(&agent_ctx) else {
            let output = GoalListOutput {
                kind: "goal.list",
                ok: true,
                goals: Vec::new(),
            };
            return match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(
                    output,
                    "No active phase. Start a phase to list goals.",
                )),
            };
        };

        let phase = agent_ctx
            .plan
            .epochs
            .iter()
            .flat_map(|e| e.phases.iter())
            .find(|p| p.id == phase_id)
            .ok_or_else(|| anyhow::anyhow!("Active phase '{phase_id}' not found. Use `exo plan read` to see available phases."))?;

        let task_counts = load_impl_goal_task_counts(ctx.root, ctx.project)?;
        let task_progress = load_goal_task_progress(ctx.root)?;
        let rfc_link = if phase.rfcs.is_empty() {
            None
        } else {
            let ids: Vec<_> = phase.rfcs.iter().map(|r| r.id.as_str()).collect();
            Some(ids.join(", "))
        };

        let mut goals = Vec::new();
        for goal in &phase.goals {
            let display_status = task_progress.get(&goal.id).and_then(|(total, completed)| {
                (goal.status == "pending" && *total > 0 && *total == *completed)
                    .then_some("done?".to_string())
            });

            // Prefer goal-level RFC; fall back to phase-level RFC link
            let goal_rfc = goal.rfc.clone().or_else(|| rfc_link.clone());

            goals.push(GoalListEntry {
                id: goal.id.clone(),
                label: goal.label.clone(),
                status: goal.status.clone(),
                display_status,
                completion_log: goal.completion_log.clone(),
                rfc: goal_rfc,
                task_count: task_counts.get(&goal.id).copied().unwrap_or(0),
                source: "sqlite".to_string(),
            });
        }

        let output = GoalListOutput {
            kind: "goal.list",
            ok: true,
            goals: goals.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if goals.is_empty() {
                    Ok(CommandOutput::new(
                        output,
                        "No goals found in active phase.",
                    ))
                } else {
                    let mut msg = String::from("| ID | Label | RFC | Tasks | Status | Source |\n");
                    msg.push_str("| :--- | :--- | :--- | ---: | :--- | :--- |\n");
                    for g in &goals {
                        let rfc = g.rfc.as_deref().unwrap_or("-");
                        let shown_status = g.display_status.as_deref().unwrap_or(&g.status);
                        msg.push_str(&format!(
                            "| {} | {} | {} | {} | {} | {} |\n",
                            g.id, g.label, rfc, g.task_count, shown_status, g.source
                        ));
                    }
                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

// ============================================================================
// goal reorder (Write)
// ============================================================================

/// Reorder a goal within the active phase.
#[derive(Debug, Clone)]
pub struct GoalReorder {
    pub id: String,
    pub position: String,
}

impl GoalReorder {
    pub fn new(id: impl Into<String>, position: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            position: position.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GoalReorderOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
    position: String,
}

impl Command for GoalReorder {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "reorder"
    }

    fn description(&self) -> &'static str {
        "Reorder a goal within the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalReorder should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalReorder {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let Some(active_phase_id) = active_phase_id(&agent_ctx) else {
            anyhow::bail!("No active phase. Start a phase before reordering goals.")
        };
        let phase_id = require_goal_phase_id(ctx, &self.id)?;
        ensure_phase_write_allowed(ctx, &phase_id)?;
        ensure_goal_requested_phase(&self.id, &phase_id, &active_phase_id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.reorder_goal(&self.id, &self.position)?;

        let output = GoalReorderOutput {
            kind: "goal.reorder",
            ok: true,
            goal_id: self.id.clone(),
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Goal '{}' moved to position {}", self.id, self.position),
            )),
        }
    }
}

// ============================================================================
// goal move (Write)
// ============================================================================

/// Move a goal to another phase.
#[derive(Debug, Clone)]
pub struct GoalMove {
    pub id: String,
    pub phase_id: String,
    pub position: Option<String>,
}

impl GoalMove {
    pub fn new(
        id: impl Into<String>,
        phase_id: impl Into<String>,
        position: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            phase_id: phase_id.into(),
            position,
        }
    }
}

#[derive(Debug, Serialize)]
struct GoalMoveOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
    phase_id: String,
    position: Option<String>,
}

impl Command for GoalMove {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "move"
    }

    fn description(&self) -> &'static str {
        "Move a goal to another phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalMove should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalMove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let source_phase_id = require_goal_phase_id(ctx, &self.id)?;
        ensure_phase_write_allowed(ctx, &source_phase_id)?;
        ensure_phase_write_allowed(ctx, &self.phase_id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.move_goal_to_phase_position(&self.id, &self.phase_id, self.position.as_deref())?;

        let output = GoalMoveOutput {
            kind: "goal.move",
            ok: true,
            goal_id: self.id.clone(),
            phase_id: self.phase_id.clone(),
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Moved goal '{}' to phase '{}'", self.id, self.phase_id),
            )),
        }
    }
}

// ============================================================================
// goal abandon (Write)
// ============================================================================

/// Abandon a goal with a required log message.
#[derive(Debug, Clone)]
pub struct GoalAbandon {
    pub id: String,
    pub log: String,
}

impl GoalAbandon {
    pub fn new(id: impl Into<String>, log: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            log: log.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GoalAbandonOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
    message: String,
}

impl Command for GoalAbandon {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "abandon"
    }

    fn description(&self) -> &'static str {
        "Abandon a goal with a log message"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalAbandon should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalAbandon {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let Some(phase_id) = active_phase_id(&agent_ctx) else {
            anyhow::bail!("No active phase. Start a phase before abandoning goals.")
        };
        ensure_phase_write_allowed(ctx, &phase_id)?;

        // Verify the goal exists in the active phase
        let phase = agent_ctx
            .plan
            .epochs
            .iter()
            .flat_map(|e| e.phases.iter())
            .find(|p| p.id == phase_id)
            .ok_or_else(|| anyhow::anyhow!("Active phase '{phase_id}' not found. Use `exo plan read` to see available phases."))?;

        let goal = phase
            .goals
            .iter()
            .find(|t| t.id == self.id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Goal '{}' not found in active phase. Use `exo goal list` to see available goals.",
                    self.id
                )
            })?;

        if goal.status == "abandoned" {
            anyhow::bail!("Goal '{}' is already abandoned.", self.id);
        }

        // Note: We allow abandoning a completed goal (e.g., if the work is later invalidated).
        // Per RFC 00229: "A completed goal can also be abandoned later if the work is invalidated."

        // Mark goal as abandoned in SQLite (canonical source per RFC 00177)
        // Use phase-scoped update to avoid updating duplicate goal IDs in other phases
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_goal_status(&self.id, "abandoned")?;
        writer.update_goal_completion_log(&self.id, &self.log)?;

        let output = GoalAbandonOutput {
            kind: "goal.abandon",
            ok: true,
            goal_id: self.id.clone(),
            message: self.log.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Abandoned goal: {}\nLog: {}", self.id, self.log);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// goal complete (Write)
// ============================================================================

/// Record a goal's completed outcome.
#[derive(Debug, Clone)]
pub struct GoalComplete {
    pub id: String,
    pub log: String,
}

impl GoalComplete {
    pub fn new(id: impl Into<String>, log: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            log: log.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GoalCompleteOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
    message: String,
    outcome: String,
    steering: crate::steering::SteeringBlock,
}

impl Command for GoalComplete {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "complete"
    }

    fn description(&self) -> &'static str {
        "Record a goal's completed outcome"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalComplete should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalComplete {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let Some(phase_id) = active_phase_id(&agent_ctx) else {
            anyhow::bail!("No active phase. Start a phase before completing goals.")
        };
        ensure_phase_write_allowed(ctx, &phase_id)?;

        // Verify the goal exists in the active phase
        let phase = agent_ctx
            .plan
            .epochs
            .iter()
            .flat_map(|e| e.phases.iter())
            .find(|p| p.id == phase_id)
            .ok_or_else(|| anyhow::anyhow!("Active phase '{phase_id}' not found. Use `exo plan read` to see available phases."))?;

        let goal = phase
            .goals
            .iter()
            .find(|t| t.id == self.id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Goal '{}' not found in active phase. Use `exo goal list` to see available goals.",
                    self.id
                )
            })?;

        if goal.status == "completed" {
            anyhow::bail!("Goal '{}' is already completed.", self.id);
        }

        if goal.status == "abandoned" {
            anyhow::bail!("Goal '{}' is already abandoned.", self.id);
        }

        let tasks = task::list_active_phase_tasks_only(ctx.root)?;
        let goal_tasks: Vec<_> = tasks
            .iter()
            .filter(|(task_id, _, _)| {
                task_id
                    .split("::")
                    .next()
                    .is_some_and(|goal_part| goal_part == goal.id)
            })
            .collect();

        if !goal_tasks.is_empty() {
            let pending_count = goal_tasks
                .iter()
                .filter(|(_, _, status)| status != "completed")
                .count();

            if pending_count > 0 {
                anyhow::bail!(
                    "Cannot complete goal '{}': {} task(s) still pending. Complete tasks first or use `exo goal abandon {} --log '...'`",
                    self.id,
                    pending_count,
                    self.id
                );
            }
        }

        let workflow_evidence_recorded =
            crate::command::completion_confirmation::record_workflow_completion_evidence(
                ctx, "goal", &self.id, &self.log,
            )?;

        // Completion guard: require outcome approval before recording completion.
        {
            use crate::command::completion_confirmation::{
                completion_confirmation_failure_with_workflow, goal_workflow_confirmation,
            };
            use crate::context::sqlite_loader::SqliteLoader;
            use crate::steering::completion_outcome_digest_summary_from_loader;
            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path)?;
            let completion_digest = loader
                .load_completion_outcome_digest("goal", &self.id)
                .ok()
                .filter(|digest| !digest.claims.is_empty())
                .map(completion_outcome_digest_summary_from_loader);
            let workflow_confirmation = goal_workflow_confirmation(
                &self.id,
                &goal.label,
                &self.log,
                goal_tasks.len(),
                workflow_evidence_recorded,
                completion_digest,
            );
            if let Some(failure) = completion_confirmation_failure_with_workflow(
                "goal",
                &self.id,
                loader.has_completion_claim("goal", &self.id)?,
                Some(workflow_confirmation),
            ) {
                return Err(anyhow::Error::new(failure));
            }
        }

        // Mark goal as completed in SQLite (canonical source per RFC 00177)
        // Use phase-scoped update to avoid updating duplicate goal IDs in other phases
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_goal_status(&self.id, "completed")?;
        writer.update_goal_completion_log(&self.id, &self.log)?;
        // NOTE: completion_log is stored in SQLite per RFC 00177.

        let steering = crate::steering::derive_entity_steering(
            ctx.root,
            "goal",
            &self.id,
            ctx.agent_id.as_deref(),
            None,
        );

        let output = GoalCompleteOutput {
            kind: "goal.complete",
            ok: true,
            goal_id: self.id.clone(),
            message: self.log.clone(),
            outcome: self.log.clone(),
            steering,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!(
                    "Completed goal: {}\nOutcome: {}\n→ Next: exo goal list (check remaining goals)",
                    self.id, self.log
                );
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// goal remove (Write)
// ============================================================================

/// Remove a goal from a phase (defaults to active phase).
#[derive(Debug, Clone)]
pub struct GoalRemove {
    pub id: String,
    /// Explicit phase ID (defaults to active phase if omitted).
    pub phase: Option<String>,
}

impl GoalRemove {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            phase: None,
        }
    }

    pub fn with_phase(mut self, phase: Option<String>) -> Self {
        self.phase = phase;
        self
    }
}

#[derive(Debug, Serialize)]
struct GoalRemoveOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
}

impl Command for GoalRemove {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove a goal from a phase (defaults to active phase)"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalRemove should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let requested_phase_id = if let Some(phase_id) = &self.phase {
            phase_id.clone()
        } else {
            let Some(phase_id) = active_phase_id(&agent_ctx) else {
                anyhow::bail!("No active phase. Start a phase or use --phase <id>.")
            };
            phase_id
        };
        let phase_id = require_goal_phase_id(ctx, &self.id)?;
        ensure_phase_write_allowed(ctx, &phase_id)?;
        ensure_goal_requested_phase(&self.id, &phase_id, &requested_phase_id)?;

        // Remove goal from SQLite-backed state
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.remove_goal(&self.id)?;

        let output = GoalRemoveOutput {
            kind: "goal.remove",
            ok: true,
            goal_id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Removed goal: {}", self.id);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// goal update (Write)
// ============================================================================

/// Update a goal's label.
#[derive(Debug, Clone)]
pub struct GoalUpdate {
    pub id: String,
    pub label: String,
}

impl GoalUpdate {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct GoalUpdateOutput {
    kind: &'static str,
    ok: bool,
    goal_id: String,
    label: String,
}

impl Command for GoalUpdate {
    fn namespace(&self) -> &'static str {
        "goal"
    }

    fn operation(&self) -> &'static str {
        "update"
    }

    fn description(&self) -> &'static str {
        "Update a goal's label"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_goal_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GoalUpdate should be dispatched via execute_mut")
    }
}

impl MutableCommand for GoalUpdate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let Some(phase_id) = active_phase_id(&agent_ctx) else {
            anyhow::bail!("No active phase. Start a phase before updating goals.")
        };
        ensure_phase_write_allowed(ctx, &phase_id)?;

        // Verify the goal exists in the active phase
        let phase = agent_ctx
            .plan
            .epochs
            .iter()
            .flat_map(|e| e.phases.iter())
            .find(|p| p.id == phase_id)
            .ok_or_else(|| anyhow::anyhow!("Active phase '{phase_id}' not found. Use `exo plan read` to see available phases."))?;

        let _goal = phase
            .goals
            .iter()
            .find(|t| t.id == self.id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Goal '{}' not found in active phase. Use `exo goal list` to see available goals.",
                    self.id
                )
            })?;

        // Update the goal label in SQLite
        // Use phase-scoped update to avoid updating duplicate goal IDs in other phases
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_goal_label(&self.id, &self.label)?;

        let output = GoalUpdateOutput {
            kind: "goal.update",
            ok: true,
            goal_id: self.id.clone(),
            label: self.label.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Updated goal '{}' label to '{}'", self.id, self.label),
            )),
        }
    }
}
