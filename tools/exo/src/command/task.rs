//! Task namespace commands.
//!
//! - `task add`: Add a new task (Write)
//! - `task list`: List tasks (Pure)
//! - `task complete`: Mark task as completed (Write)
//! - `task rename`: Change a task's conversational handle (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::AgentContext;
use crate::context::Goal;
use crate::context::SqliteLoader;
use crate::context::SqliteWriter;
use crate::failure::ExoFailure;
use crate::phase_owner;
use crate::steering::{SuggestedAction, WorkIntent};
use crate::task;
use anyhow::{Context, Result as ExoResult};
use exosuit_storage::OptionalExtension;
use serde::Serialize;

/// Default steering for task commands.
fn default_task_steering() -> Vec<SuggestedAction> {
    vec![
        SuggestedAction {
            label: "List tasks".to_string(),
            command: "exo task list".to_string(),
            rationale: "View all tasks in the active phase.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.6),
        },
        SuggestedAction {
            label: "Show map".to_string(),
            command: "exo map".to_string(),
            rationale: "Get oriented with the project state.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.5),
        },
    ]
}

// ============================================================================
// ExoSpec definition — single source of truth for the task namespace
// ============================================================================

/// Task namespace command specification.
///
/// This enum is the authoritative definition of the task namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `TaskCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "task", description = "Task management commands")]
pub enum TaskCommands {
    #[exo(effect = "write", description = "Add a new task to the active phase")]
    Add {
        #[exo(
            positional,
            description = "The label of the task (ID auto-generated from label if --id omitted)"
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
            description = "Explicit task ID (auto-generated from label if omitted)"
        )]
        id: Option<String>,
        #[exo(
            long,
            optional,
            description = "Associate this task with the specified goal"
        )]
        goal: Option<String>,
    },

    #[exo(effect = "pure", description = "List tasks in the active phase")]
    List,

    #[exo(
        effect = "write",
        description = "Mark a task as completed with a log message"
    )]
    Complete {
        #[exo(positional, description = "The ID of the task")]
        id: String,
        #[exo(
            long,
            default = "Completed",
            description = "Completion log message (defaults to 'Completed' if omitted)"
        )]
        log: String,
    },

    #[exo(
        effect = "write",
        description = "Append a progress log entry to a task"
    )]
    Log {
        #[exo(positional, description = "The ID of the task")]
        id: String,
        #[exo(long, description = "Progress message to log")]
        message: String,
    },

    #[exo(effect = "write", description = "Mark a task as in-progress")]
    Start {
        #[exo(positional, description = "The ID of the task")]
        id: String,
    },

    #[exo(effect = "write", description = "Remove a task from the active phase")]
    Remove {
        #[exo(positional, description = "The ID of the task to remove")]
        id: String,
    },

    #[exo(
        effect = "write",
        description = "Reorder a task within the active phase"
    )]
    Reorder {
        #[exo(positional, description = "The ID of the task to reorder")]
        id: String,
        #[exo(
            positional,
            description = "Target position (0-indexed number, 'top', or 'bottom')"
        )]
        position: String,
    },

    #[exo(
        effect = "write",
        description = "Rename a task handle while preserving the old handle as an alias"
    )]
    Rename {
        #[exo(positional, description = "The current task ID or alias")]
        id: String,
        #[exo(long, description = "The new canonical task ID")]
        to: String,
    },

    #[exo(
        effect = "write",
        description = "Update a task's title in the active phase"
    )]
    Update {
        #[exo(positional, description = "The ID of the task to update")]
        id: String,
        #[exo(long, short = 't', description = "The new title for the task")]
        title: String,
    },
}

impl TaskCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    ///
    /// Takes `root` to resolve file-based arguments (e.g., `--label-file`).
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Add {
                label,
                label_file,
                id,
                goal,
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
                let mut cmd = TaskAdd::new(id, label);
                if let Some(goal) = goal {
                    cmd = cmd.with_goal(goal);
                }
                CommandBox::mutable(cmd)
            }
            Self::List => CommandBox::pure(TaskList::new()),
            Self::Complete { id, log } => CommandBox::mutable(TaskComplete::new(id, log)),
            Self::Log { id, message } => CommandBox::mutable(TaskLog::new(id, message)),
            Self::Start { id } => CommandBox::mutable(TaskStart::new(id)),
            Self::Remove { id } => CommandBox::mutable(TaskRemove::new(id)),
            Self::Reorder { id, position } => CommandBox::mutable(TaskReorder::new(id, position)),
            Self::Rename { id, to } => CommandBox::mutable(TaskRename::new(id, to)),
            Self::Update { id, title } => CommandBox::mutable(TaskUpdate::new(id, title)),
        })
    }
}

#[derive(Debug, Clone)]
struct ResolvedTaskGoal {
    goal_id: String,
    phase_id: String,
    phase_status: String,
}

fn resolve_goal_for_task_add(
    context: &AgentContext,
    goal_id: &str,
) -> ExoResult<Option<ResolvedTaskGoal>> {
    let mut matches = Vec::new();

    for epoch in &context.plan.epochs {
        for phase in &epoch.phases {
            if !phase_allows_task_add_goal_resolution(&phase.status) {
                continue;
            }
            for goal in &phase.goals {
                if !goal_allows_task_add_goal_resolution(&goal.status) {
                    continue;
                }
                if goal.id == goal_id || goal.aliases.iter().any(|alias| alias == goal_id) {
                    matches.push((
                        phase.id.as_str(),
                        phase.status.as_str(),
                        goal.id.as_str(),
                        goal.label.as_str(),
                    ));
                }
            }
        }
    }

    match matches.as_slice() {
        [] => Ok(None),
        [(phase_id, phase_status, canonical_goal_id, _)] => Ok(Some(ResolvedTaskGoal {
            goal_id: (*canonical_goal_id).to_string(),
            phase_id: (*phase_id).to_string(),
            phase_status: (*phase_status).to_string(),
        })),
        _ => {
            let available = matches
                .iter()
                .map(|(phase_id, _, canonical_goal_id, label)| {
                    format!("{canonical_goal_id}: {label} (phase {phase_id})")
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(anyhow::Error::new(ambiguous_goal_failure(
                goal_id, &available,
            )))
        }
    }
}

fn phase_allows_task_add_goal_resolution(status: &str) -> bool {
    matches!(status, "pending" | "in-progress" | "active")
}

fn goal_allows_task_add_goal_resolution(status: &str) -> bool {
    matches!(status, "pending" | "in-progress" | "active")
}

fn ensure_task_lifecycle_allowed(ctx: &MutableCommandContext, task_id: &str) -> ExoResult<()> {
    let Some(task_phase) = find_task_phase(ctx, task_id)? else {
        return Ok(());
    };
    if phase_allows_task_lifecycle(&task_phase.phase_status) {
        return phase_owner::ensure_phase_write_allowed(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &task_phase.phase_id,
        );
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::InvalidInput,
        format!(
            "Task '{task_id}' belongs to phase '{}' ({}) and cannot be changed by task lifecycle commands until that phase starts. Use `exo phase read-tasks {}` to review planned tasks.",
            task_phase.phase_id, task_phase.phase_status, task_phase.phase_id
        ),
        ExoFailure::orienting_steering(vec![
            SuggestedAction {
                label: "Read phase tasks".to_string(),
                command: format!("exo phase read-tasks {}", task_phase.phase_id),
                rationale: "Review planned tasks without mutating future-phase execution state."
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Start phase".to_string(),
                command: format!("exo phase start {}", task_phase.phase_id),
                rationale: "Start the phase before changing task lifecycle state.".to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(0.5),
            },
        ]),
    )))
}

fn ensure_task_complete_lifecycle_allowed(
    ctx: &MutableCommandContext,
    entity_id: &str,
) -> ExoResult<()> {
    if find_task_phase(ctx, entity_id)?.is_some() {
        return ensure_task_lifecycle_allowed(ctx, entity_id);
    }

    let Some(goal_phase) = find_goal_phase(ctx, entity_id)? else {
        return Ok(());
    };
    if phase_allows_task_lifecycle(&goal_phase.phase_status) {
        return phase_owner::ensure_phase_write_allowed(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &goal_phase.phase_id,
        );
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::InvalidInput,
        format!(
            "Goal '{entity_id}' belongs to phase '{}' ({}) and cannot be completed through `exo task complete` until that phase starts. Use `exo phase read-goals {}` to review planned goals.",
            goal_phase.phase_id, goal_phase.phase_status, goal_phase.phase_id
        ),
        ExoFailure::orienting_steering(vec![
            SuggestedAction {
                label: "Read phase goals".to_string(),
                command: format!("exo phase read-goals {}", goal_phase.phase_id),
                rationale: "Review planned goals without mutating future-phase execution state."
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Start phase".to_string(),
                command: format!("exo phase start {}", goal_phase.phase_id),
                rationale: "Start the phase before completing its goals.".to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(0.5),
            },
        ]),
    )))
}

fn phase_allows_task_lifecycle(status: &str) -> bool {
    matches!(status, "in-progress" | "active")
}

#[derive(Debug, Clone)]
struct TaskPhaseMatch {
    phase_id: String,
    phase_status: String,
}

fn find_task_phase(
    ctx: &MutableCommandContext,
    task_id: &str,
) -> ExoResult<Option<TaskPhaseMatch>> {
    let writer = SqliteWriter::open(ctx.db_path())?;
    Ok(writer
        .resolve_task_reference(task_id)?
        .map(|task| TaskPhaseMatch {
            phase_id: task.phase_id,
            phase_status: task.phase_status,
        }))
}

fn find_goal_phase(
    ctx: &MutableCommandContext,
    goal_id: &str,
) -> ExoResult<Option<TaskPhaseMatch>> {
    let loader = SqliteLoader::open(ctx.db_path())?;
    let conn = loader.database().connection();

    conn.query_row(
        "SELECT p.text_id, p.status
         FROM goals_data g
         JOIN phases_data p ON g.phase_id = p.id
         WHERE g.text_id = ?1",
        [goal_id],
        |row| {
            Ok(TaskPhaseMatch {
                phase_id: row.get(0)?,
                phase_status: row.get(1)?,
            })
        },
    )
    .optional()
    .context("Failed to query goal phase for task-complete fallback guard")
}

fn ambiguous_goal_failure(goal_id: &str, available: &str) -> ExoFailure {
    ExoFailure::new(
        ErrorCode::InvalidInput,
        format!(
            "Goal '{goal_id}' is ambiguous across active or pending phases: {available}. Move or rename goals before adding a task."
        ),
        ExoFailure::orienting_steering(vec![
            SuggestedAction {
                label: "Read plan".to_string(),
                command: "exo plan read".to_string(),
                rationale: "See phase and goal IDs before adding a task.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "Read phase goals".to_string(),
                command: "exo phase read-goals <phase-id>".to_string(),
                rationale: "Inspect a specific phase's goals before choosing a target.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]),
    )
    .with_details(serde_json::json!({
        "goal": goal_id,
        "matches": available,
    }))
}

fn format_goal_list(goals: &[Goal]) -> String {
    goals
        .iter()
        .map(|goal| format!("{}: {} ({})", goal.id, goal.label, goal.status))
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_active_goal(status: &str) -> bool {
    status == "in-progress"
}

// ============================================================================
// task add
// ============================================================================

/// Add a new task to the active phase.
#[derive(Debug, Clone)]
pub struct TaskAdd {
    pub id: String,
    pub label: String,
    pub goal: Option<String>,
}

impl TaskAdd {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            goal: None,
        }
    }

    pub fn with_goal(mut self, goal: impl Into<String>) -> Self {
        self.goal = Some(goal.into());
        self
    }
}

fn normalize_task_add_id_for_goal(
    id: &str,
    requested_goal: Option<&str>,
    resolved_goal_id: &str,
    prefix_goal_id: Option<&str>,
) -> ExoResult<String> {
    let Some(requested_goal) = requested_goal else {
        return Ok(id.to_string());
    };
    let Some((prefix, suffix)) = id.split_once("::") else {
        return Ok(id.to_string());
    };

    if prefix.is_empty() {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::InvalidInput,
            format!(
                "Task id '{id}' has an empty goal prefix. Use `--id {suffix}` with `--goal {requested_goal}`."
            ),
            ExoFailure::orienting_steering(default_task_steering()),
        )));
    }

    let Some(prefix_goal_id) = prefix_goal_id else {
        return Ok(id.to_string());
    };

    if suffix.is_empty() {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::InvalidInput,
            format!(
                "Task id '{id}' has an empty task component. Use `--id <task-id>` with `--goal {requested_goal}`."
            ),
            ExoFailure::orienting_steering(default_task_steering()),
        )));
    }

    if prefix_goal_id == resolved_goal_id {
        return Ok(suffix.to_string());
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::InvalidInput,
        format!(
            "Task id prefix '{prefix}' resolves to goal {prefix_goal_id}, but --goal {requested_goal} resolves to {resolved_goal_id}. Use `--goal {prefix}` or `--id {suffix}`."
        ),
        ExoFailure::orienting_steering(default_task_steering()),
    )))
}

#[derive(Debug, Serialize)]
struct TaskAddOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    goal_id: Option<String>,
    phase_id: Option<String>,
    phase_status: Option<String>,
    step_name: Option<String>,
}

impl Command for TaskAdd {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new task to the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        let resolved_goal = if let Some(goal) = &self.goal {
            match resolve_goal_for_task_add(&agent_ctx, goal)? {
                Some(resolved_goal) => resolved_goal,
                None => {
                    return Err(anyhow::Error::new(ExoFailure::new(
                        ErrorCode::InvalidInput,
                        format!(
                            "Goal '{goal}' not found. Use `exo phase read-goals <phase-id>` or `exo plan read` to see available goals."
                        ),
                        ExoFailure::orienting_steering(vec![
                            SuggestedAction {
                                label: "Read phase goals".to_string(),
                                command: "exo phase read-goals <phase-id>".to_string(),
                                rationale:
                                    "Inspect a specific phase's goals before choosing a target."
                                        .to_string(),
                                intent: WorkIntent::Orient,
                                confidence: Some(0.7),
                            },
                            SuggestedAction {
                                label: "Read plan".to_string(),
                                command: "exo plan read".to_string(),
                                rationale: "See available phases and goals before adding a task."
                                    .to_string(),
                                intent: WorkIntent::Orient,
                                confidence: Some(0.6),
                            },
                        ]),
                    )));
                }
            }
        } else {
            let Some(active_phase) = agent_ctx.find_workspace_active_phase()? else {
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::InvalidInput,
                    "No active phase found. Use `exo phase start <id>` to start one.".to_string(),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "Read plan".to_string(),
                        command: "exo plan read".to_string(),
                        rationale: "Find a phase to start before adding tasks.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.6),
                    }]),
                )));
            };

            let goals = &active_phase.phase.goals;
            if goals.is_empty() {
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::InvalidInput,
                    "No goals in active phase. Create one with `exo goal add \"Goal label\"`"
                        .to_string(),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "Add goal".to_string(),
                        command: "exo goal add \"Goal label\"".to_string(),
                        rationale: "Create a goal before adding a task.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.6),
                    }]),
                )));
            }

            let active_goals: Vec<&Goal> =
                goals.iter().filter(|g| is_active_goal(&g.status)).collect();

            if active_goals.len() == 1 {
                ResolvedTaskGoal {
                    goal_id: active_goals[0].id.clone(),
                    phase_id: active_phase.phase.id.clone(),
                    phase_status: active_phase.phase.status.clone(),
                }
            } else if goals.len() == 1 {
                ResolvedTaskGoal {
                    goal_id: goals[0].id.clone(),
                    phase_id: active_phase.phase.id.clone(),
                    phase_status: active_phase.phase.status.clone(),
                }
            } else if active_goals.is_empty() {
                let available = format_goal_list(goals);
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "No active goals in the active phase. Available goals: {available}. Use `--goal <id>` to choose one."
                    ),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "List goals".to_string(),
                        command: "exo goal list".to_string(),
                        rationale: "Pick a goal ID to pass to --goal.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.6),
                    }]),
                )));
            } else {
                let available = active_goals
                    .iter()
                    .map(|goal| format!("{}: {} ({})", goal.id, goal.label, goal.status))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::InvalidInput,
                    format!(
                        "Multiple active goals found: {available}. Use `--goal <id>` to choose one."
                    ),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "List goals".to_string(),
                        command: "exo goal list".to_string(),
                        rationale: "Pick a goal ID to pass to --goal.".to_string(),
                        intent: WorkIntent::Orient,
                        confidence: Some(0.6),
                    }]),
                )));
            }
        };

        let prefix_goal_id = if self.goal.is_some() {
            self.id
                .split_once("::")
                .map(|(prefix, _)| {
                    if prefix.is_empty() {
                        Ok(None)
                    } else {
                        resolve_goal_for_task_add(&agent_ctx, prefix)
                            .map(|resolved| resolved.map(|goal| goal.goal_id))
                    }
                })
                .transpose()?
                .flatten()
        } else {
            None
        };
        let task_id = normalize_task_add_id_for_goal(
            &self.id,
            self.goal.as_deref(),
            &resolved_goal.goal_id,
            prefix_goal_id.as_deref(),
        )?;
        phase_owner::ensure_phase_write_allowed(
            ctx.root,
            ctx.project,
            &ctx.db_path(),
            &resolved_goal.phase_id,
        )?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.add_task(&resolved_goal.goal_id, &task_id, &self.label, None)?;

        let step_name = None;

        let output = TaskAddOutput {
            kind: "task.add",
            ok: true,
            task_id: task_id.clone(),
            goal_id: Some(resolved_goal.goal_id),
            phase_id: Some(resolved_goal.phase_id),
            phase_status: Some(resolved_goal.phase_status),
            step_name,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if let Some(goal_id) = &output.goal_id {
                    let display_task_id = format!("{goal_id}::{task_id}");
                    let msg = if output
                        .phase_status
                        .as_deref()
                        .is_some_and(|status| matches!(status, "in-progress" | "active"))
                    {
                        format!(
                            "Added task under goal {goal_id}: {}\n→ Next: exo task start {}",
                            task_id, display_task_id
                        )
                    } else {
                        let phase_id = output.phase_id.as_deref().unwrap_or("<phase-id>");
                        format!(
                            "Added task under goal {goal_id}: {}\n→ Review future phase tasks: exo phase read-tasks {phase_id}",
                            task_id
                        )
                    };
                    Ok(CommandOutput::new(output, msg))
                } else {
                    Ok(CommandOutput::new(
                        output,
                        format!(
                            "Added task: {}\n→ Next: exo task start {}",
                            task_id, task_id
                        ),
                    ))
                }
            }
        }
    }
}

// ============================================================================
// task list
// ============================================================================

/// List tasks in the active phase.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskList;

impl TaskList {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Serialize)]
struct TaskListEntry {
    id: String,
    label: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct TaskListOutput {
    kind: &'static str,
    ok: bool,
    tasks: Vec<TaskListEntry>,
}

impl Command for TaskList {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List tasks in the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx =
            AgentContext::load_with_project(ctx.root.to_path_buf(), ctx.project.cloned())?;
        let tasks = task::list_tasks_for_context(&agent_ctx)?;

        let task_entries: Vec<TaskListEntry> = tasks
            .into_iter()
            .map(|(id, label, status)| TaskListEntry { id, label, status })
            .collect();

        let output = TaskListOutput {
            kind: "task.list",
            ok: true,
            tasks: task_entries,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let groups = task::list_task_groups_for_context(&agent_ctx)?;
                if groups.is_empty() {
                    Ok(CommandOutput::new(
                        output,
                        "No goals found in active phase.",
                    ))
                } else {
                    let mut msg = String::new();
                    for (index, group) in groups.iter().enumerate() {
                        if index > 0 {
                            msg.push('\n');
                        }

                        msg.push_str(&format!(
                            "Goal {} ({}): {}\n",
                            group.goal_id, group.goal_status, group.goal_label
                        ));

                        if group.tasks.is_empty() {
                            msg.push_str("  (no tasks)\n");
                            continue;
                        }

                        msg.push_str("  | ID | Label | Status |\n");
                        msg.push_str("  | :--- | :--- | :--- |\n");
                        for task in &group.tasks {
                            msg.push_str(&format!(
                                "  | {} | {} | {} |\n",
                                task.id, task.label, task.status
                            ));
                        }
                    }
                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

// ============================================================================
// task complete
// ============================================================================

/// Mark a task as completed with a log message.
#[derive(Debug, Clone)]
pub struct TaskComplete {
    pub id: String,
    pub log: String,
}

impl TaskComplete {
    pub fn new(id: impl Into<String>, log: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            log: log.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskCompleteOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    message: String,
    steering: crate::steering::SteeringBlock,
}

impl Command for TaskComplete {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "complete"
    }

    fn description(&self) -> &'static str {
        "Mark a task as completed with a log message"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskComplete should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskComplete {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_complete_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let resolved_task = writer.resolve_task_reference(&self.id)?;
        let canonical_id = resolved_task
            .as_ref()
            .map_or(self.id.as_str(), |task| task.task_id.as_str());

        // Completion guard: require a shared-perception claim before closing.
        // Try task first, then goal (mirrors the completion fallback below).
        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path)?;
        let completion_entity_type = if resolved_task.is_some() {
            "task"
        } else if find_goal_phase(ctx, &self.id)?.is_some() {
            "goal"
        } else {
            anyhow::bail!("Task not found: {}", self.id);
        };
        let goal_tasks = if completion_entity_type == "goal" {
            task::list_active_phase_tasks_only(ctx.root)?
                .into_iter()
                .filter(|(task_id, _, _)| {
                    task_id
                        .split("::")
                        .next()
                        .is_some_and(|goal_id| goal_id == canonical_id)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if completion_entity_type == "goal" {
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
                ctx,
                completion_entity_type,
                canonical_id,
                &self.log,
            )?;

        {
            use crate::command::completion_confirmation::{
                completion_confirmation_failure_with_workflow, goal_workflow_confirmation,
                task_workflow_confirmation,
            };
            use crate::context::sqlite_loader::CompletionClaimStatus;
            use crate::steering::completion_outcome_digest_summary_from_loader;
            let completion_digest = loader
                .load_completion_outcome_digest(completion_entity_type, canonical_id)
                .ok()
                .filter(|digest| !digest.claims.is_empty())
                .map(completion_outcome_digest_summary_from_loader);
            let workflow_confirmation = if completion_entity_type == "goal" {
                goal_workflow_confirmation(
                    canonical_id,
                    "",
                    &self.log,
                    goal_tasks.len(),
                    workflow_evidence_recorded,
                    completion_digest,
                )
            } else {
                task_workflow_confirmation(
                    canonical_id,
                    &self.log,
                    workflow_evidence_recorded,
                    completion_digest,
                )
            };

            let claim = match loader.has_completion_claim("task", canonical_id)? {
                CompletionClaimStatus::NoClaim if self.id != canonical_id => {
                    loader.has_completion_claim("task", &self.id)?
                }
                other => other,
            };
            let claim = match claim {
                CompletionClaimStatus::NoClaim => {
                    // No task claim — check if there's a goal claim (fallback path)
                    loader.has_completion_claim("goal", canonical_id)?
                }
                other => other,
            };

            if let Some(failure) = completion_confirmation_failure_with_workflow(
                completion_entity_type,
                canonical_id,
                claim,
                Some(workflow_confirmation),
            ) {
                return Err(anyhow::Error::new(failure));
            }
        }

        let message = if let Some(task) = &resolved_task {
            writer.complete_task_by_row_id(task.row_id, &self.log)?;
            format!("Task '{canonical_id}' marked as completed.")
        } else {
            // The TOML path supports `task complete <goal-id>` to complete
            // a goal as a "phase task". Mirror that behavior on SQLite.
            writer
                .update_goal_status(canonical_id, "completed")
                .map_err(|_| anyhow::anyhow!("Task not found: {}", self.id))?;
            if !self.log.is_empty() {
                let _ = writer.update_goal_completion_log(canonical_id, &self.log);
            }
            format!("Task '{canonical_id}' marked as completed.")
        };

        let steering = crate::steering::derive_entity_steering_from_db(
            &ctx.db_path(),
            "task",
            canonical_id,
            ctx.agent_id.as_deref(),
            None,
        );

        let output = TaskCompleteOutput {
            kind: "task.complete",
            ok: true,
            task_id: canonical_id.to_string(),
            title: resolved_task.as_ref().map(|task| task.title.clone()),
            message: self.log.clone(),
            steering,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("{message}\n→ Next: exo task list (check remaining tasks)"),
            )),
        }
    }
}

// ============================================================================
// task log
// ============================================================================

/// Append a progress log entry to a task.
#[derive(Debug, Clone)]
pub struct TaskLog {
    pub id: String,
    pub message: String,
}

impl TaskLog {
    pub fn new(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskLogOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    title: String,
    message: String,
    steering: crate::steering::SteeringBlock,
}

impl Command for TaskLog {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "log"
    }

    fn description(&self) -> &'static str {
        "Append a progress log entry to a task"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskLog should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskLog {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", self.id))?;
        writer.add_task_log_by_row_id(task.row_id, "progress", &self.message)?;
        let result_message = format!(
            "Logged progress on task '{}' ({}).",
            task.task_id, task.title
        );

        let steering = crate::steering::derive_entity_steering_from_db(
            &ctx.db_path(),
            "task",
            &task.task_id,
            ctx.agent_id.as_deref(),
            None,
        );

        let output = TaskLogOutput {
            kind: "task.log",
            ok: true,
            task_id: task.task_id,
            title: task.title,
            message: self.message.clone(),
            steering,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, result_message)),
        }
    }
}

// ============================================================================
// task start
// ============================================================================

/// Mark a task as in-progress (started).
#[derive(Debug, Clone)]
pub struct TaskStart {
    pub id: String,
}

impl TaskStart {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct TaskStartOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    title: String,
    steering: crate::steering::SteeringBlock,
}

impl Command for TaskStart {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "start"
    }

    fn description(&self) -> &'static str {
        "Mark a task as in-progress"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskStart should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", self.id))?;
        writer.update_task_status_by_row_id(task.row_id, "in-progress")?;
        let message = format!("Started task '{}' ({}).", task.task_id, task.title);

        let steering = crate::steering::derive_entity_steering_from_db(
            &ctx.db_path(),
            "task",
            &task.task_id,
            ctx.agent_id.as_deref(),
            None,
        );

        let output = TaskStartOutput {
            kind: "task.start",
            ok: true,
            task_id: task.task_id.clone(),
            title: task.title,
            steering,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "{message}\n→ Next: exo task log {} --message \"...\" or exo task complete {}",
                    task.task_id, task.task_id
                ),
            )),
        }
    }
}

// ============================================================================
// task remove
// ============================================================================

/// Remove a task from the active phase.
#[derive(Debug, Clone)]
pub struct TaskRemove {
    pub id: String,
}

impl TaskRemove {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct TaskRemoveOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    title: String,
}

impl Command for TaskRemove {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove a task from the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskRemove should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", self.id))?;
        writer.remove_task_by_row_id(task.row_id)?;
        let message = format!("Removed task '{}' ({}).", task.task_id, task.title);

        let output = TaskRemoveOutput {
            kind: "task.remove",
            ok: true,
            task_id: task.task_id,
            title: task.title,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, message)),
        }
    }
}

// ============================================================================
// task reorder
// ============================================================================

/// Reorder a task within the active phase.
#[derive(Debug, Clone)]
pub struct TaskReorder {
    pub id: String,
    pub position: String,
}

impl TaskReorder {
    pub fn new(id: impl Into<String>, position: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            position: position.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskReorderOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    title: String,
    position: String,
}

impl Command for TaskReorder {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "reorder"
    }

    fn description(&self) -> &'static str {
        "Reorder a task within the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskReorder should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskReorder {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", self.id))?;
        writer.reorder_task_by_row_id(task.row_id, &self.position)?;
        let message = format!(
            "Reordered task '{}' ({}) to position {}.",
            task.task_id, task.title, self.position
        );

        let output = TaskReorderOutput {
            kind: "task.reorder",
            ok: true,
            task_id: task.task_id,
            title: task.title,
            position: self.position.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, message)),
        }
    }
}

// ============================================================================
// task rename
// ============================================================================

/// Rename a task's canonical handle while preserving its old handle as an alias.
#[derive(Debug, Clone)]
pub struct TaskRename {
    pub id: String,
    pub to: String,
}

impl TaskRename {
    pub fn new(id: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            to: to.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskRenameOutput {
    kind: &'static str,
    ok: bool,
    old_task_id: String,
    task_id: String,
    goal_id: String,
    title: String,
}

fn invalid_task_rename(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ExoFailure::new(
        ErrorCode::InvalidInput,
        message.into(),
        ExoFailure::orienting_steering(default_task_steering()),
    ))
}

fn normalize_task_rename_id(
    writer: &SqliteWriter,
    requested: &str,
    current_goal_id: &str,
) -> ExoResult<String> {
    if requested.is_empty() {
        return Err(invalid_task_rename("The new task ID cannot be empty."));
    }
    if requested.split("::").any(str::is_empty) {
        return Err(invalid_task_rename(format!(
            "Task ID '{requested}' has an empty hierarchy component."
        )));
    }

    let Some((prefix, suffix)) = requested.split_once("::") else {
        return Ok(requested.to_string());
    };
    let Some(prefix_goal_id) = writer.resolve_goal_reference(prefix)? else {
        return Ok(requested.to_string());
    };

    if prefix_goal_id == current_goal_id {
        return Ok(suffix.to_string());
    }

    Err(invalid_task_rename(format!(
        "Task ID prefix '{prefix}' resolves to goal '{prefix_goal_id}', but the task belongs to goal '{current_goal_id}'. Use `--to {suffix}` or move the task separately."
    )))
}

impl Command for TaskRename {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "rename"
    }

    fn description(&self) -> &'static str {
        "Rename a task handle while preserving the old handle as an alias"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskRename should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskRename {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| invalid_task_rename(format!("Task not found: {}", self.id)))?;
        let new_task_id = normalize_task_rename_id(&writer, &self.to, &task.goal_id)?;

        if new_task_id == task.task_id {
            return Err(invalid_task_rename(format!(
                "Task '{}' already uses that canonical handle.",
                task.task_id
            )));
        }
        if writer.task_handle_conflicts_for_goal(&task.goal_id, &new_task_id, task.row_id)? {
            return Err(invalid_task_rename(format!(
                "Task handle '{new_task_id}' conflicts with another canonical, alias, or goal-qualified task reference."
            )));
        }

        writer.rename_task_handle(task.row_id, &task.task_id, &new_task_id)?;

        let output = TaskRenameOutput {
            kind: "task.rename",
            ok: true,
            old_task_id: task.task_id.clone(),
            task_id: new_task_id.clone(),
            goal_id: task.goal_id,
            title: task.title.clone(),
        };
        let message = format!(
            "Renamed task '{}' ({}) to '{}'. The old handle remains available as an alias.",
            task.task_id, task.title, new_task_id
        );

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, message)),
        }
    }
}

// ============================================================================
// task update
// ============================================================================

/// Update a task's title in the active phase.
#[derive(Debug, Clone)]
pub struct TaskUpdate {
    pub id: String,
    pub title: String,
}

impl TaskUpdate {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskUpdateOutput {
    kind: &'static str,
    ok: bool,
    task_id: String,
    title: String,
}

impl Command for TaskUpdate {
    fn namespace(&self) -> &'static str {
        "task"
    }

    fn operation(&self) -> &'static str {
        "update"
    }

    fn description(&self) -> &'static str {
        "Update a task's title in the active phase"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_task_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TaskUpdate should be dispatched via execute_mut")
    }
}

impl MutableCommand for TaskUpdate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        ensure_task_lifecycle_allowed(ctx, &self.id)?;

        let writer = SqliteWriter::open(ctx.db_path())?;
        let task = writer
            .resolve_task_reference(&self.id)?
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", self.id))?;
        writer.update_task_title_by_row_id(task.row_id, &self.title)?;
        let message = format!(
            "Task '{}' updated with new title: {}",
            task.task_id, self.title
        );

        let output = TaskUpdateOutput {
            kind: "task.update",
            ok: true,
            task_id: task.task_id,
            title: self.title.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_add_metadata() {
        let cmd = TaskAdd::new("test-id", "Test label");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "test-id");
        assert_eq!(cmd.label, "Test label");
    }

    #[test]
    fn test_task_list_metadata() {
        let cmd = TaskList::new();
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_task_complete_metadata() {
        let cmd = TaskComplete::new("test-id", "test log message");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "complete");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "test-id");
        assert_eq!(cmd.log, "test log message");
    }

    #[test]
    fn test_task_remove_metadata() {
        let cmd = TaskRemove::new("test-id");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "remove");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "test-id");
    }

    #[test]
    fn test_task_reorder_metadata() {
        let cmd = TaskReorder::new("test-id", "top");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "reorder");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "test-id");
        assert_eq!(cmd.position, "top");
    }

    #[test]
    fn test_task_rename_metadata() {
        let cmd = TaskRename::new("old-id", "new-id");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "rename");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "old-id");
        assert_eq!(cmd.to, "new-id");
    }

    #[test]
    fn test_task_update_metadata() {
        let cmd = TaskUpdate::new("test-id", "New Title");
        assert_eq!(cmd.namespace(), "task");
        assert_eq!(cmd.operation(), "update");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "test-id");
        assert_eq!(cmd.title, "New Title");
    }
}
