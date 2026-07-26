//! Workbench lane commands.
//!
//! Lanes are portable execution streams associated with one phase. Their
//! current-workspace focus remains machine-local.

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::{ExoState, Phase, SqliteLoader, SqliteWriter, WorkbenchLaneData};
use crate::failure::ExoFailure;
use crate::phase_owner;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

// ============================================================================
// ExoSpec definition
// ============================================================================

/// Workbench lane command specification.
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "lane", description = "Workbench lane commands")]
pub enum LaneCommands {
    #[exo(effect = "write", description = "Create a prepared workbench lane")]
    Create {
        #[exo(positional, description = "Lane title")]
        title: String,
        #[exo(long, description = "The lane's durable execution intent")]
        intent: String,
        #[exo(long, description = "Execution phase ID")]
        phase: String,
    },

    #[exo(effect = "pure", description = "List workbench lanes")]
    List,

    #[exo(effect = "pure", description = "Show a workbench lane")]
    Show {
        #[exo(positional, description = "Lane ID")]
        id: String,
    },

    #[exo(
        effect = "pure",
        description = "Show the current workspace's focused lane"
    )]
    Current,

    #[exo(
        effect = "write",
        description = "Focus a lane and its phase in this workspace"
    )]
    Focus {
        #[exo(positional, description = "Lane ID")]
        id: String,
    },

    #[exo(
        effect = "write",
        description = "Start a prepared lane and focus it in this workspace"
    )]
    Start {
        #[exo(positional, description = "Lane ID")]
        id: String,
    },

    #[exo(effect = "write", description = "Remove a prepared workbench lane")]
    Remove {
        #[exo(positional, description = "Lane ID")]
        id: String,
    },
}

impl LaneCommands {
    /// Convert a parsed lane operation into a dispatchable command.
    pub fn to_command_box(self, _root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Create {
                title,
                intent,
                phase,
            } => CommandBox::mutable(LaneCreate::new(title, intent, phase)),
            Self::List => CommandBox::pure(LaneList),
            Self::Show { id } => CommandBox::pure(LaneShow::new(id)),
            Self::Current => CommandBox::pure(LaneCurrent),
            Self::Focus { id } => CommandBox::mutable(LaneFocus::new(id)),
            Self::Start { id } => CommandBox::mutable(LaneStart::new(id)),
            Self::Remove { id } => CommandBox::mutable(LaneRemove::new(id)),
        })
    }
}

// ============================================================================
// Shared read model
// ============================================================================

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LaneGoalSummary {
    id: String,
    title: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LaneSummary {
    id: String,
    title: String,
    intent: String,
    state: String,
    created_at: String,
    updated_at: String,
    phase_id: String,
    phase_title: String,
    phase_status: String,
    focused_here: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LaneDetails {
    #[serde(flatten)]
    summary: LaneSummary,
    goals: Vec<LaneGoalSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LaneDiagnostic {
    code: &'static str,
    message: String,
    lane_id: String,
    lane_phase_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    focused_phase_id: Option<String>,
}

struct LaneReadState {
    plan: ExoState,
    workspace_root: Option<String>,
    lanes: Vec<WorkbenchLaneData>,
    focused_lane_id: Option<String>,
    focused_phase_id: Option<String>,
}

impl LaneReadState {
    fn load(ctx: &CommandContext<'_>) -> ExoResult<Self> {
        Self::load_for(ctx.root, ctx.project)
    }

    fn load_mut(ctx: &MutableCommandContext<'_>) -> ExoResult<Self> {
        Self::load_for(ctx.root, ctx.project)
    }

    fn load_for(
        root: &std::path::Path,
        project: Option<&crate::project::Project>,
    ) -> ExoResult<Self> {
        let loader = SqliteLoader::open(crate::context::db_path(root, project))?;
        let plan = loader.load_state()?;
        let lanes = loader.load_workbench_lanes()?;
        let workspace_root = project
            .and_then(|project| project.workspace_root.as_ref())
            .map(|root| root.to_string_lossy().into_owned());
        let (focused_lane_id, focused_phase_id) =
            if let Some(workspace_root) = workspace_root.as_deref() {
                (
                    loader
                        .load_workspace_lane_focus(workspace_root)?
                        .map(|focus| focus.lane_id),
                    loader.load_workspace_active_phase(workspace_root)?,
                )
            } else {
                (None, None)
            };

        Ok(Self {
            plan,
            workspace_root,
            lanes,
            focused_lane_id,
            focused_phase_id,
        })
    }

    fn lane(&self, id: &str) -> ExoResult<&WorkbenchLaneData> {
        self.lanes
            .iter()
            .find(|lane| lane.text_id == id)
            .ok_or_else(|| anyhow::Error::new(lane_not_found_failure(id)))
    }

    fn phase(&self, phase_id: &str) -> ExoResult<&Phase> {
        find_phase(&self.plan, phase_id).ok_or_else(|| {
            anyhow::Error::new(ExoFailure::new(
                ErrorCode::NotFound,
                format!("Phase not found for workbench lane: {phase_id}"),
                lane_steering(),
            ))
        })
    }

    fn summary(&self, lane: &WorkbenchLaneData) -> ExoResult<LaneSummary> {
        let phase = self.phase(&lane.execution_phase_id)?;
        Ok(LaneSummary {
            id: lane.text_id.clone(),
            title: lane.title.clone(),
            intent: lane.intent.clone(),
            state: lane.state.clone(),
            created_at: lane.created_at.clone(),
            updated_at: lane.updated_at.clone(),
            phase_id: phase.id.clone(),
            phase_title: phase.title.clone(),
            phase_status: phase.status.clone(),
            focused_here: self.focused_lane_id.as_deref() == Some(lane.text_id.as_str()),
        })
    }

    fn details(&self, lane: &WorkbenchLaneData) -> ExoResult<LaneDetails> {
        let phase = self.phase(&lane.execution_phase_id)?;
        let goals = phase
            .goals
            .iter()
            .map(|goal| LaneGoalSummary {
                id: goal.id.clone(),
                title: goal.label.clone(),
                status: goal.status.clone(),
            })
            .collect();
        Ok(LaneDetails {
            summary: self.summary(lane)?,
            goals,
        })
    }

    fn diagnostics(&self) -> ExoResult<Vec<LaneDiagnostic>> {
        let Some(focused_lane_id) = self.focused_lane_id.as_deref() else {
            return Ok(vec![]);
        };
        let lane = self.lane(focused_lane_id)?;
        let phase = self.phase(&lane.execution_phase_id)?;
        if self.focused_phase_id.as_deref() == Some(lane.execution_phase_id.as_str())
            && phase.status == "in-progress"
        {
            return Ok(vec![]);
        }

        let message = if self.focused_phase_id.as_deref() == Some(lane.execution_phase_id.as_str())
        {
            format!(
                "Focused lane '{}' belongs to phase '{}', but that phase is {} rather than in-progress",
                lane.text_id, lane.execution_phase_id, phase.status
            )
        } else {
            format!(
                "Focused lane '{}' belongs to phase '{}', but this workspace's focused phase is {}",
                lane.text_id,
                lane.execution_phase_id,
                self.focused_phase_id.as_deref().unwrap_or("unset")
            )
        };

        Ok(vec![LaneDiagnostic {
            code: "lane.phase_focus_mismatch",
            message,
            lane_id: lane.text_id.clone(),
            lane_phase_id: lane.execution_phase_id.clone(),
            focused_phase_id: self.focused_phase_id.clone(),
        }])
    }
}

fn find_phase<'a>(plan: &'a ExoState, phase_id: &str) -> Option<&'a Phase> {
    plan.epochs
        .iter()
        .flat_map(|epoch| epoch.phases.iter())
        .find(|phase| phase.id == phase_id)
}

fn lane_steering() -> crate::steering::SteeringBlock {
    ExoFailure::orienting_steering(vec![SuggestedAction {
        label: "List workbench lanes".to_string(),
        command: "exo lane list".to_string(),
        rationale: "Inspect the current lane IDs, phases, and focus state.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.9),
    }])
}

fn lane_not_found_failure(id: &str) -> ExoFailure {
    ExoFailure::new(
        ErrorCode::NotFound,
        format!("Workbench lane not found: {id}"),
        lane_steering(),
    )
    .with_details(serde_json::json!({
        "kind": "lane.not_found",
        "lane_id": id,
    }))
}

fn phase_precondition_failure(
    lane_id: Option<&str>,
    phase_id: &str,
    phase_status: &str,
    operation: &str,
) -> ExoFailure {
    ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!(
            "Cannot {operation} workbench lane{} while phase '{phase_id}' is {phase_status}",
            lane_id.map_or_else(String::new, |id| format!(" '{id}'"))
        ),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Start the execution phase".to_string(),
            command: format!("exo phase start {phase_id}"),
            rationale: "Lane focus and execution require an in-progress phase.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.9),
        }]),
    )
    .with_details(serde_json::json!({
        "kind": "lane.phase_not_in_progress",
        "lane_id": lane_id,
        "phase_id": phase_id,
        "phase_status": phase_status,
    }))
}

fn require_workspace_root(state: &LaneReadState) -> ExoResult<&str> {
    state.workspace_root.as_deref().ok_or_else(|| {
        anyhow::Error::new(
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                "Workbench lane focus requires a resolved workspace root",
                lane_steering(),
            )
            .with_details(serde_json::json!({
                "kind": "lane.workspace_unavailable",
            })),
        )
    })
}

fn require_nonempty(value: &str, name: &str) -> ExoResult<()> {
    if value.trim().is_empty() {
        return Err(anyhow::Error::new(
            ExoFailure::new(
                ErrorCode::InvalidInput,
                format!("Lane {name} cannot be empty"),
                lane_steering(),
            )
            .with_details(serde_json::json!({
                "kind": "lane.invalid_input",
                "field": name,
            })),
        ));
    }
    Ok(())
}

fn human_message_with_diagnostics(message: String, diagnostics: &[LaneDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return message;
    }

    let warnings = diagnostics
        .iter()
        .map(|diagnostic| format!("- {}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{message}\n\nWarnings:\n{warnings}")
}

fn mutable_command_unreachable(name: &str) -> ! {
    unreachable!("{name} should be dispatched via execute_mut")
}

// ============================================================================
// lane create
// ============================================================================

#[derive(Debug, Clone)]
pub struct LaneCreate {
    title: String,
    intent: String,
    phase_id: String,
}

impl LaneCreate {
    pub fn new(
        title: impl Into<String>,
        intent: impl Into<String>,
        phase_id: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            intent: intent.into(),
            phase_id: phase_id.into(),
        }
    }
}

impl Command for LaneCreate {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "create"
    }

    fn description(&self) -> &'static str {
        "Create a prepared workbench lane"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("LaneCreate")
    }
}

impl MutableCommand for LaneCreate {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        require_nonempty(&self.title, "title")?;
        require_nonempty(&self.intent, "intent")?;
        let state = LaneReadState::load_mut(ctx)?;
        let phase = state.phase(&self.phase_id)?;
        if !matches!(phase.status.as_str(), "pending" | "in-progress") {
            return Err(anyhow::Error::new(phase_precondition_failure(
                None,
                &phase.id,
                &phase.status,
                "create a",
            )));
        }
        phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), &phase.id)?;

        let lane_id = SqliteWriter::open(ctx.db_path())?.add_workbench_lane(
            self.title.trim(),
            self.intent.trim(),
            &phase.id,
        )?;
        let refreshed = LaneReadState::load_mut(ctx)?;
        let lane = refreshed.lane(&lane_id)?;
        let output = LaneMutationOutput {
            kind: "lane.create",
            ok: true,
            lane: refreshed.summary(lane)?,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "Created prepared lane \"{}\" ({lane_id})",
                    self.title.trim()
                ),
            )),
        }
    }
}

// ============================================================================
// lane list/show/current
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct LaneList;

impl Command for LaneList {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List workbench lanes"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        #[derive(Serialize)]
        struct Output {
            kind: &'static str,
            ok: bool,
            lanes: Vec<LaneSummary>,
            diagnostics: Vec<LaneDiagnostic>,
        }

        let state = LaneReadState::load(ctx)?;
        let lanes = state
            .lanes
            .iter()
            .map(|lane| state.summary(lane))
            .collect::<ExoResult<Vec<_>>>()?;
        let diagnostics = state.diagnostics()?;
        let output = Output {
            kind: "lane.list",
            ok: true,
            lanes,
            diagnostics,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = if output.lanes.is_empty() {
                    "No workbench lanes".to_string()
                } else {
                    output
                        .lanes
                        .iter()
                        .map(|lane| {
                            let focus = if lane.focused_here { " (focused)" } else { "" };
                            format!(
                                "{}  {} [{}] — {}{}",
                                lane.id, lane.title, lane.state, lane.phase_title, focus
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                let message = human_message_with_diagnostics(message, &output.diagnostics);
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaneShow {
    id: String,
}

impl LaneShow {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Command for LaneShow {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "show"
    }

    fn description(&self) -> &'static str {
        "Show a workbench lane"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        #[derive(Serialize)]
        struct Output {
            kind: &'static str,
            ok: bool,
            lane: LaneDetails,
            diagnostics: Vec<LaneDiagnostic>,
        }

        let state = LaneReadState::load(ctx)?;
        let lane = state.lane(&self.id)?;
        let details = state.details(lane)?;
        let diagnostics = state.diagnostics()?;
        let output = Output {
            kind: "lane.show",
            ok: true,
            lane: details,
            diagnostics,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format!(
                    "# {}\n\nID: {}\nState: {}\nPhase: {} ({})\n\n{}",
                    lane.title,
                    lane.text_id,
                    lane.state,
                    lane.execution_phase_id,
                    state.phase(&lane.execution_phase_id)?.status,
                    lane.intent
                );
                let message = human_message_with_diagnostics(message, &output.diagnostics);
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LaneCurrent;

impl Command for LaneCurrent {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "current"
    }

    fn description(&self) -> &'static str {
        "Show the current workspace's focused lane"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        #[derive(Serialize)]
        struct Output {
            kind: &'static str,
            ok: bool,
            lane: Option<LaneDetails>,
            diagnostics: Vec<LaneDiagnostic>,
        }

        let state = LaneReadState::load(ctx)?;
        let lane = state
            .focused_lane_id
            .as_deref()
            .map(|id| state.lane(id).and_then(|lane| state.details(lane)))
            .transpose()?;
        let diagnostics = state.diagnostics()?;
        let output = Output {
            kind: "lane.current",
            ok: true,
            lane,
            diagnostics,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = output.lane.as_ref().map_or_else(
                    || "No workbench lane is focused in this workspace".to_string(),
                    |lane| {
                        format!(
                            "Current lane: {} ({}) — {}",
                            lane.summary.title, lane.summary.id, lane.summary.phase_title
                        )
                    },
                );
                let message = human_message_with_diagnostics(message, &output.diagnostics);
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

// ============================================================================
// lane focus/start/remove
// ============================================================================

#[derive(Debug, Serialize)]
struct LaneMutationOutput {
    kind: &'static str,
    ok: bool,
    lane: LaneSummary,
}

#[derive(Debug, Clone)]
pub struct LaneFocus {
    id: String,
}

impl LaneFocus {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Command for LaneFocus {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "focus"
    }

    fn description(&self) -> &'static str {
        "Focus a lane and its phase in this workspace"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("LaneFocus")
    }
}

impl MutableCommand for LaneFocus {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let state = LaneReadState::load_mut(ctx)?;
        let lane = state.lane(&self.id)?;
        let phase = state.phase(&lane.execution_phase_id)?;
        if phase.status != "in-progress" {
            return Err(anyhow::Error::new(phase_precondition_failure(
                Some(&lane.text_id),
                &phase.id,
                &phase.status,
                "focus",
            )));
        }
        let workspace_root = require_workspace_root(&state)?;
        SqliteWriter::open(ctx.db_path())?.focus_workbench_lane(
            &workspace_root,
            &lane.text_id,
            &phase.id,
        )?;

        lane_mutation_output(ctx, "lane.focus", &lane.text_id, "Focused")
    }
}

#[derive(Debug, Clone)]
pub struct LaneStart {
    id: String,
}

impl LaneStart {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Command for LaneStart {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "start"
    }

    fn description(&self) -> &'static str {
        "Start a prepared lane and focus it in this workspace"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("LaneStart")
    }
}

impl MutableCommand for LaneStart {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let state = LaneReadState::load_mut(ctx)?;
        let lane = state.lane(&self.id)?;
        let phase = state.phase(&lane.execution_phase_id)?;
        if phase.status != "in-progress" {
            return Err(anyhow::Error::new(phase_precondition_failure(
                Some(&lane.text_id),
                &phase.id,
                &phase.status,
                "start",
            )));
        }
        if lane.state != "prepared" {
            return Err(anyhow::Error::new(
                ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    format!(
                        "Cannot start workbench lane '{}' from state {}",
                        lane.text_id, lane.state
                    ),
                    lane_steering(),
                )
                .with_details(serde_json::json!({
                    "kind": "lane.not_prepared",
                    "lane_id": lane.text_id,
                    "lane_state": lane.state,
                })),
            ));
        }
        phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), &phase.id)?;
        let workspace_root = require_workspace_root(&state)?;
        SqliteWriter::open(ctx.db_path())?.start_and_focus_workbench_lane(
            &workspace_root,
            &lane.text_id,
            &phase.id,
        )?;

        lane_mutation_output(ctx, "lane.start", &lane.text_id, "Started")
    }
}

#[derive(Debug, Clone)]
pub struct LaneRemove {
    id: String,
}

impl LaneRemove {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

impl Command for LaneRemove {
    fn namespace(&self) -> &'static str {
        "lane"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove a prepared workbench lane"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        mutable_command_unreachable("LaneRemove")
    }
}

impl MutableCommand for LaneRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        #[derive(Serialize)]
        struct Output {
            kind: &'static str,
            ok: bool,
            id: String,
            title: String,
        }

        let state = LaneReadState::load_mut(ctx)?;
        let lane = state.lane(&self.id)?;
        let phase = state.phase(&lane.execution_phase_id)?;
        if lane.state != "prepared" {
            return Err(anyhow::Error::new(
                ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    format!(
                        "Cannot remove workbench lane '{}' from state {}",
                        lane.text_id, lane.state
                    ),
                    lane_steering(),
                )
                .with_details(serde_json::json!({
                    "kind": "lane.not_prepared",
                    "lane_id": lane.text_id,
                    "lane_state": lane.state,
                })),
            ));
        }
        phase_owner::ensure_phase_write_allowed(ctx.root, ctx.project, &ctx.db_path(), &phase.id)?;

        let title = lane.title.clone();
        SqliteWriter::open(ctx.db_path())?.remove_prepared_workbench_lane(&lane.text_id)?;
        let output = Output {
            kind: "lane.remove",
            ok: true,
            id: lane.text_id.clone(),
            title: title.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Removed prepared lane \"{title}\""),
            )),
        }
    }
}

fn lane_mutation_output(
    ctx: &MutableCommandContext<'_>,
    kind: &'static str,
    lane_id: &str,
    verb: &str,
) -> ExoResult<CommandOutput> {
    let refreshed = LaneReadState::load_mut(ctx)?;
    let lane = refreshed.lane(lane_id)?;
    let output = LaneMutationOutput {
        kind,
        ok: true,
        lane: refreshed.summary(lane)?,
    };
    match ctx.format {
        OutputFormat::Json => Ok(CommandOutput::data(output)),
        OutputFormat::Human => Ok(CommandOutput::new(
            output,
            format!("{verb} lane \"{}\" ({})", lane.title, lane.text_id),
        )),
    }
}
