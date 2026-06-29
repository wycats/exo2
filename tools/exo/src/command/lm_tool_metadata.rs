use super::command_spec::LmToolMetadata;
use serde_json::json;

#[derive(Debug, Clone, Copy)]
pub struct ToolSetDefinition {
    pub name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
}

const TOOL_SET_DEFINITIONS: &[ToolSetDefinition] = &[
    ToolSetDefinition {
        name: "exo-project",
        display_name: "Project Lifecycle",
        description: "Phase and epoch management: start/finish phases, manage epochs",
    },
    ToolSetDefinition {
        name: "exo-governance",
        display_name: "Project Governance",
        description: "Axioms, council decisions, and operational modes",
    },
    ToolSetDefinition {
        name: "exo-tasks",
        display_name: "Task Management",
        description: "Manage tasks within the current phase",
    },
    ToolSetDefinition {
        name: "exo-context-ops",
        display_name: "Context Operations",
        description: "Read context artifacts like logs and diagnostics",
    },
    ToolSetDefinition {
        name: "exo-rfc",
        display_name: "RFC Lifecycle",
        description: "Manage RFCs: create, promote stages, and list status",
    },
    ToolSetDefinition {
        name: "exo-plan-ops",
        display_name: "Plan Modifications",
        description: "Modify the project plan structure (phases, tasks, metadata)",
    },
    ToolSetDefinition {
        name: "exo-discovery",
        display_name: "Discovery & Listing",
        description: "List and locate project artifacts (tasks, goals, epochs)",
    },
];

pub const fn tool_set_definitions() -> &'static [ToolSetDefinition] {
    TOOL_SET_DEFINITIONS
}

#[derive(Debug, Clone, Copy)]
struct LmToolOverride {
    namespace: &'static str,
    operation: &'static str,
    tool_reference_name: &'static str,
    display_name: &'static str,
    icon: &'static str,
    when_clause: Option<&'static str>,
    tags: &'static [&'static str],
    tool_sets: &'static [&'static str],
    user_description: &'static str,
    model_description: &'static str,
}

impl LmToolOverride {
    fn to_metadata(self) -> LmToolMetadata {
        LmToolMetadata {
            display_name: Some(self.display_name.to_string()),
            model_description: Some(self.model_description.to_string()),
            user_description: Some(self.user_description.to_string()),
            icon: Some(self.icon.to_string()),
            when_clause: self.when_clause.map(str::to_string),
            tool_reference_name: Some(self.tool_reference_name.to_string()),
            tags: Some(self.tags.iter().map(|tag| (*tag).to_string()).collect()),
            can_be_referenced_in_prompt: Some(true),
            tool_sets: if self.tool_sets.is_empty() {
                None
            } else {
                Some(
                    self.tool_sets
                        .iter()
                        .map(|set| (*set).to_string())
                        .collect(),
                )
            },
        }
    }
}

const LM_TOOL_OVERRIDES: &[LmToolOverride] = &[
    LmToolOverride {
        namespace: "",
        operation: "status",
        tool_reference_name: "status",
        display_name: "Project Status",
        icon: "$(info)",
        when_clause: None,
        tags: &["exosuit", "status"],
        tool_sets: &[],
        user_description: "Get current project phase, active goals, and next steps.",
        model_description: r"Returns current phase, active goals, and singular next step.

**Use this when**: User asks 'where am I?', requests a quick status snapshot, or needs orientation.

**Do NOT use when**: User wants the big picture (use exo-plan), a phase deep dive (use exo-phase), or navigation options (use exo-steering).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "plan",
        operation: "review",
        tool_reference_name: "plan",
        display_name: "Project Plan",
        icon: "$(list-tree)",
        when_clause: None,
        tags: &["exosuit", "plan"],
        tool_sets: &[],
        user_description: "Get high-level project roadmap and plan health.",
        model_description: r"Returns high-level project plan, epoch structure, and health metrics.

**Use this when**: User asks for the roadmap, wants the big picture, or needs strategic context.

**Do NOT use when**: User needs current phase details (use exo-phase) or a quick status snapshot (use exo-status).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "phase",
        operation: "status",
        tool_reference_name: "phase",
        display_name: "Current Phase",
        icon: "$(milestone)",
        when_clause: None,
        tags: &["exosuit", "phase"],
        tool_sets: &[],
        user_description: "Get detailed current phase information.",
        model_description: r"Returns current phase details including goals, walkthroughs, and artifacts.

**Use this when**: User asks what's in this phase or wants a detailed phase breakdown.

**Do NOT use when**: User needs the big picture (use exo-plan) or a quick status snapshot (use exo-status).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "",
        operation: "map",
        tool_reference_name: "steering",
        display_name: "Steering",
        icon: "$(compass)",
        when_clause: None,
        tags: &["exosuit", "navigation", "steering"],
        tool_sets: &[],
        user_description: "Get AI-scored navigation with next action suggestions.",
        model_description: r"Returns multiple next action options with confidence scores, repair paths, and blockers.

**Use this when**: User asks what to do next, is stuck, or needs navigation choices.

**Do NOT use when**: User just needs status (use exo-status) or phase details (use exo-phase).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "ai",
        operation: "context",
        tool_reference_name: "context",
        display_name: "Full Context",
        icon: "$(book)",
        when_clause: None,
        tags: &["exosuit", "context", "handoff"],
        tool_sets: &[],
        user_description: "Get full project context for session handoff.",
        model_description: r"Returns the canonical full context dump for session handoff or recovery.

**Use this when**: Starting a fresh session, recovering from context loss, or needing historical context.

**Do NOT use when**: User only needs status (use exo-status) or phase details (use exo-phase).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "idea",
        operation: "add",
        tool_reference_name: "idea",
        display_name: "Add Idea",
        icon: "$(lightbulb)",
        when_clause: Some("exosuit.projectInitialized"),
        tags: &["exosuit", "mutation", "idea"],
        tool_sets: &[],
        user_description: "Quickly capture an idea to the project backlog.",
        model_description: r"Adds an idea to the project backlog.

**Use this when**: Capturing a future improvement or thought that does not belong in the current phase.

**Do NOT use when**: The item is a current-phase task (use exo-add-task) or a formal RFC action is needed.",
    },
    LmToolOverride {
        namespace: "task",
        operation: "add",
        tool_reference_name: "add-task",
        display_name: "Add Task",
        icon: "$(add)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "mutation", "task"],
        tool_sets: &["exo-tasks"],
        user_description: "Add a new task to the current active phase.",
        model_description: r"Adds a new task to the current active phase.

**Use this when**: User explicitly asks to add a task to the active phase or work reveals a new task.

**Do NOT use when**: There is no active phase (check exo-status), the item is for the backlog (use exo-idea), or the task belongs to a future phase.",
    },
    LmToolOverride {
        namespace: "inbox",
        operation: "list",
        tool_reference_name: "inbox",
        display_name: "Inbox",
        icon: "$(inbox)",
        when_clause: Some("exosuit.projectInitialized"),
        tags: &["exosuit", "inbox"],
        tool_sets: &[],
        user_description: "Get pending user intents from inbox.",
        model_description: r"Returns pending user intents that need agent attention or acknowledgment.

**Use this when**: Starting a session, user mentions feedback/corrections, or you suspect there are pending intents.

**Do NOT use when**: You only need status (use exo-status) or phase details (use exo-phase).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "phase",
        operation: "start",
        tool_reference_name: "phase-start",
        display_name: "Start Phase",
        icon: "$(play)",
        when_clause: Some("exosuit.projectInitialized"),
        tags: &["exosuit", "phase", "lifecycle"],
        tool_sets: &["exo-project"],
        user_description: "Start a phase by ID.",
        model_description: r"Starts a phase, making it the active phase for the project.

Phases have a `kind`: **regular** (default) or **chore**. Chore phases have lighter ceremony — no TDD nudges, no RFC linkage requirement. Use chore for maintenance, cleanup, or housekeeping work.

**Use this when**: User says 'start the next phase' or 'begin phase X'.

**Do NOT use when**: A phase is already active (check exo-status).",
    },
    LmToolOverride {
        namespace: "phase",
        operation: "finish",
        tool_reference_name: "phase-finish",
        display_name: "Finish Phase",
        icon: "$(check)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "phase", "lifecycle"],
        tool_sets: &["exo-project"],
        user_description: "Finish the current active phase.",
        model_description: r"Finishes the current active phase, marking it as complete.

**Use this when**: User confirms finishing a phase and goals are complete.

**Do NOT use when**: There are incomplete goals (check exo-phase) or no phase is active.",
    },
    LmToolOverride {
        namespace: "task",
        operation: "start",
        tool_reference_name: "task-start",
        display_name: "Start Task",
        icon: "$(play)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Mark a task as in-progress.",
        model_description: r"Marks a task as in-progress (started).

**Use this when**: Starting work on a specific task and want to signal it's underway.

**Do NOT use when**: The task is already in-progress or completed (check exo-phase).",
    },
    LmToolOverride {
        namespace: "task",
        operation: "complete",
        tool_reference_name: "task-complete",
        display_name: "Complete Task",
        icon: "$(check)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Mark a task as complete.",
        model_description: r"Marks a task in the current phase as complete.

**Use this when**: A specific task is finished and should be marked done.

**Do NOT use when**: The task is not finished or the task ID is unclear (check exo-phase).",
    },
    LmToolOverride {
        namespace: "task",
        operation: "log",
        tool_reference_name: "task-log",
        display_name: "Log Task Progress",
        icon: "$(note)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Log a progress note on a task.",
        model_description: r"Appends a progress log entry to a task in the current phase.

**Use this when**: Recording intermediate progress, decisions, or notes during task execution.

**Do NOT use when**: The task is finished (use exo-task-complete instead).",
    },
    LmToolOverride {
        namespace: "tdd",
        operation: "new",
        tool_reference_name: "tdd-start",
        display_name: "Start TDD Cycle",
        icon: "$(beaker)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "tdd", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Start a new TDD cycle for a task.",
        model_description: "Starts a new TDD cycle for a task. Run BEFORE writing code. Marks the task as TDD RED (tdd_status=red) and records the test file. Instructs you to write a failing test first.",
    },
    LmToolOverride {
        namespace: "tdd",
        operation: "red",
        tool_reference_name: "tdd-red",
        display_name: "Confirm TDD Red",
        icon: "$(circle-slash)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "tdd", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Confirm the failing test (red phase).",
        model_description: "Confirms the test is failing (red phase). Run after writing a failing test. Moves the active task to TDD GREEN (tdd_status=green).",
    },
    LmToolOverride {
        namespace: "tdd",
        operation: "green",
        tool_reference_name: "tdd-green",
        display_name: "Confirm TDD Green",
        icon: "$(check)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "tdd", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Confirm the tests pass (green phase).",
        model_description: "Confirms the test is passing (green phase). Run after implementation passes tests. Leaves tdd_status=green as evidence and clears the active TDD pointer; complete the task separately with exo-task-complete.",
    },
    LmToolOverride {
        namespace: "task",
        operation: "remove",
        tool_reference_name: "task-remove",
        display_name: "Remove Task",
        icon: "$(trash)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Remove a task from the current phase.",
        model_description: r"Removes a task from the current phase.

**Use this when**: A task is no longer needed.

**Do NOT use when**: The task should just be marked complete (use exo-task-complete).",
    },
    LmToolOverride {
        namespace: "task",
        operation: "reorder",
        tool_reference_name: "task-reorder",
        display_name: "Reorder Task",
        icon: "$(move)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Reorder a task within the current phase.",
        model_description: r"Reorders a task within the current phase.

**Use this when**: User wants to move a task to a different position (top, bottom, or index).

**Do NOT use when**: The task title needs updating (use exo-task-update) or the task should be completed (use exo-task-complete).",
    },
    LmToolOverride {
        namespace: "task",
        operation: "update",
        tool_reference_name: "task-update",
        display_name: "Update Task",
        icon: "$(edit)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "task", "mutation"],
        tool_sets: &["exo-tasks"],
        user_description: "Update a task's title in the current phase.",
        model_description: r"Updates a task's title in the current phase.

**Use this when**: User wants to rename a task or clarify its label.

**Do NOT use when**: The task should be completed (use exo-task-complete) or removed (use exo-task-remove).",
    },
    LmToolOverride {
        namespace: "task",
        operation: "list",
        tool_reference_name: "list-tasks",
        display_name: "List Tasks",
        icon: "$(list-ordered)",
        when_clause: None,
        tags: &["exosuit", "task", "list"],
        tool_sets: &["exo-discovery"],
        user_description: "List tasks in the active phase.",
        model_description: r"Lists tasks in the active phase.

**Use this when**: User asks to see current tasks or wants an overview of the active phase tasks.

**Do NOT use when**: User needs the full phase details (use exo-phase) or a quick status snapshot (use exo-status).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "goal",
        operation: "add",
        tool_reference_name: "add-goal",
        display_name: "Add Goal",
        icon: "$(add)",
        when_clause: Some("exosuit.hasActivePhase"),
        tags: &["exosuit", "mutation", "goal"],
        tool_sets: &["exo-tasks"],
        user_description: "Add a new goal to the current active phase.",
        model_description: r"Adds a new goal to the current active phase.

**Use this when**: User explicitly asks to add a goal to the active phase or work reveals a new goal.

**Do NOT use when**: There is no active phase (check exo-status), the item is for the backlog (use exo-idea), or the goal belongs to a future phase.",
    },
    LmToolOverride {
        namespace: "goal",
        operation: "list",
        tool_reference_name: "goal-list",
        display_name: "List Goals",
        icon: "$(target)",
        when_clause: None,
        tags: &["exosuit", "goal", "list"],
        tool_sets: &["exo-discovery"],
        user_description: "List goals (planning tasks) in the active phase.",
        model_description: r"Lists goals in the active phase with their status and RFC linkage.

**Use this when**: User asks about goals, planning tasks, or wants to see what needs to be done in the current phase.

**Do NOT use when**: User needs execution-level task details (use exo-list-tasks) or phase completion status (use exo-status).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "rfc",
        operation: "create",
        tool_reference_name: "rfc-create",
        display_name: "Create RFC",
        icon: "$(add)",
        when_clause: None,
        tags: &["exosuit", "rfc", "mutation"],
        tool_sets: &["exo-rfc"],
        user_description: "Create a new RFC.",
        model_description: r"Creates a new RFC.

**Use this when**: User explicitly requests an RFC to be created.

**Do NOT use when**: The user only wants to capture a rough idea (use exo-idea) or edit an existing RFC (use exo-rfc-edit).",
    },
    LmToolOverride {
        namespace: "rfc",
        operation: "promote",
        tool_reference_name: "rfc-promote",
        display_name: "Promote RFC",
        icon: "$(arrow-up)",
        when_clause: None,
        tags: &["exosuit", "rfc", "mutation"],
        tool_sets: &["exo-rfc"],
        user_description: "Promote an RFC to the next stage.",
        model_description: r"Promotes an RFC to the next stage.

**Use this when**: User explicitly asks to advance an RFC's stage.

**Do NOT use when**: User is only asking about RFC status or wants to edit content (use exo-rfc-list or exo-rfc-edit).",
    },
    LmToolOverride {
        namespace: "rfc",
        operation: "list",
        tool_reference_name: "rfc-list",
        display_name: "List RFCs",
        icon: "$(list-tree)",
        when_clause: None,
        tags: &["exosuit", "rfc"],
        tool_sets: &["exo-rfc"],
        user_description: "List RFCs, optionally filtered by stage.",
        model_description: r"Lists RFCs, optionally filtered by stage.

**Use this when**: User asks to list RFCs or wants to see a specific stage.

**Do NOT use when**: User needs to create or promote an RFC (use exo-rfc-create or exo-rfc-promote).",
    },
    LmToolOverride {
        namespace: "epoch",
        operation: "start",
        tool_reference_name: "epoch-start",
        display_name: "Start Epoch",
        icon: "$(play)",
        when_clause: None,
        tags: &["exosuit", "epoch", "lifecycle"],
        tool_sets: &["exo-project"],
        user_description: "Start an epoch by ID.",
        model_description: r"Starts an epoch, making it the active epoch.

**Use this when**: User says 'start epoch X' or wants to begin a specific epoch.

**Do NOT use when**: An epoch is already active and the user hasn't asked to switch (check exo-status).",
    },
    LmToolOverride {
        namespace: "epoch",
        operation: "finish",
        tool_reference_name: "epoch-finish",
        display_name: "Finish Epoch",
        icon: "$(check)",
        when_clause: None,
        tags: &["exosuit", "epoch", "lifecycle"],
        tool_sets: &["exo-project"],
        user_description: "Finish the current active epoch.",
        model_description: r"Finishes the current active epoch.

**Use this when**: User confirms the epoch is complete and ready to close.

**Do NOT use when**: There are incomplete phases or the user has not asked to finish the epoch.",
    },
    LmToolOverride {
        namespace: "epoch",
        operation: "list",
        tool_reference_name: "epoch-list",
        display_name: "List Epochs",
        icon: "$(list-tree)",
        when_clause: None,
        tags: &["exosuit", "epoch"],
        tool_sets: &["exo-project"],
        user_description: "List all epochs with status.",
        model_description: r"Lists all epochs with status.

**Use this when**: User asks to see epochs or wants an overview of epoch status.

**Do NOT use when**: User wants to start or finish an epoch (use exo-epoch-start or exo-epoch-finish).

**Zero arguments required.**",
    },
    LmToolOverride {
        namespace: "ai",
        operation: "chat-history",
        tool_reference_name: "ai-chat-history",
        display_name: "Chat History",
        icon: "$(history)",
        when_clause: None,
        tags: &["exosuit", "context"],
        tool_sets: &["exo-context-ops"],
        user_description: "Read recent conversation history from this chat session.",
        model_description: r"Reads recent conversation history from the current VS Code chat session.

Use this to recover context that may have been lost during conversation summarization—especially nuanced user feedback, decisions, and steering.

**Use this when**: The conversation summary mentions a previous session or you need to recover lost context.

**Do NOT use when**: You have full context and don't need to recover past messages.

**Always provide `match-text`** with a distinctive phrase from a recent user message to identify the correct session.",
    },
];

pub fn lookup(namespace: &str, operation: &str) -> Option<LmToolMetadata> {
    LM_TOOL_OVERRIDES
        .iter()
        .find(|entry| entry.namespace == namespace && entry.operation == operation)
        .map(|entry| entry.to_metadata())
}

#[derive(Debug, Clone)]
pub struct ExtraLmTool {
    pub name: String,
    pub display_name: String,
    pub tool_reference_name: String,
    pub can_be_referenced_in_prompt: bool,
    pub icon: String,
    pub tags: Vec<String>,
    pub tool_sets: Vec<String>,
    pub user_description: String,
    pub model_description: String,
    pub input_schema: serde_json::Value,
}

pub fn extra_tools() -> Vec<ExtraLmTool> {
    vec![ExtraLmTool {
        name: "exo-diagnostics".to_string(),
        display_name: "Workspace Diagnostics".to_string(),
        tool_reference_name: "diagnostics".to_string(),
        can_be_referenced_in_prompt: true,
        icon: "$(warning)".to_string(),
        tags: vec!["exosuit".to_string(), "diagnostics".to_string(), "debug".to_string()],
        tool_sets: vec!["exo-context-ops".to_string()],
        user_description: "Get detailed workspace error and warning diagnostics.".to_string(),
        model_description: r"Get detailed workspace diagnostics (errors, warnings) from VS Code.

**Use this when**: Investigating build errors, checking for warnings after code changes, or diagnosing issues flagged by exo-status.

**Do NOT use when**: You just need a quick error count (check exo-status which includes a diagnostics summary)."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "Filter diagnostics to files matching this substring (e.g., 'extension.ts' or 'src/services')"
                },
                "severity": {
                    "type": "string",
                    "enum": ["error", "warning", "info", "hint"],
                    "description": "Filter by severity level"
                },
                "source": {
                    "type": "string",
                    "description": "Filter by diagnostic source (e.g., 'ts', 'eslint')"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of diagnostics to return (default: 50, max: 200)"
                }
            },
            "required": []
        }),
    }, ExtraLmTool {
        name: "exo-logs".to_string(),
        display_name: "Extension Logs".to_string(),
        tool_reference_name: "logs".to_string(),
        can_be_referenced_in_prompt: true,
        icon: "$(output)".to_string(),
        tags: vec!["exosuit".to_string(), "debug".to_string()],
        tool_sets: vec!["exo-context-ops".to_string()],
        user_description: "Read recent Exosuit extension logs for debugging.".to_string(),
        model_description: r"Returns recent log entries from the Exosuit output channel.

**Use this when**: Diagnosing errors, understanding extension behavior, or when a previous operation failed unexpectedly.

**Do NOT use when**: Everything is working normally."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "lines": {
                    "type": "number",
                    "description": "Number of recent log lines to return (default: 50, max: 500)"
                },
                "level": {
                    "type": "string",
                    "enum": ["error", "warn", "info", "debug"],
                    "description": "Minimum log level to include (default: all levels)"
                },
                "component": {
                    "type": "string",
                    "description": "Filter by component (e.g., 'lmtool', 'extension', 'webview')"
                }
            },
            "required": []
        }),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goal_add_lookup() {
        let result = lookup("goal", "add");
        assert!(result.is_some(), "goal.add should have an override");
        let meta = result.unwrap();
        assert_eq!(meta.display_name, Some("Add Goal".to_string()));
    }

    #[test]
    fn test_goal_list_lookup() {
        let result = lookup("goal", "list");
        assert!(result.is_some(), "goal.list should have an override");
        let meta = result.unwrap();
        assert_eq!(meta.display_name, Some("List Goals".to_string()));
    }
}
