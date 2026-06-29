//! Project namespace commands.
//!
//! - `project resolve`: Resolve project identity and state/runtime paths (Pure)
//! - `project list`: List locally known Exo projects (Pure)
//! - `project snapshot`: Read project-scoped cockpit roots by project id (Pure)
//! - `project repair`: Preview project policy repairs (Pure)
//! - `project repair-apply`: Apply project policy repairs (Exec)
//! - `project move-root`: Retarget a sidecar-backed project to a new checkout root (Exec)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, ErrorCode};
use crate::context::SqliteLoader;
use crate::daemon_transport::DaemonEndpoint;
use crate::failure::ExoFailure;
use crate::project::{
    Project, ProjectCatalog, ProjectCatalogDiagnostic, ProjectCatalogEntry,
    ProjectPolicyRepairApply, ProjectPolicyRepairPlan, ProjectResolver,
};
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::{Context, Result as ExoResult, anyhow};
use exosuit_storage::rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use toml_edit::{DocumentMut, Item, value};
use walkdir::WalkDir;

fn project_resolver_for_context(project: Option<&Project>) -> ProjectResolver {
    match project.and_then(|project| project.projects_config_path.as_deref()) {
        Some(path) => ProjectResolver::default().with_projects_config_path(path),
        None => ProjectResolver::default(),
    }
}

// ============================================================================
// ExoSpec definition — single source of truth for the project namespace
// ============================================================================

/// Project namespace command specification.
#[derive(Debug, Clone, exospec::ExoSpec)]
#[exo(
    namespace = "project",
    description = "Project identity and path commands"
)]
pub enum ProjectCommands {
    #[exo(
        effect = "pure",
        description = "Resolve project identity and state/runtime paths"
    )]
    Resolve,
    #[exo(
        effect = "pure",
        description = "List locally known Exo projects from project policy and sidecars"
    )]
    List,
    #[exo(
        effect = "pure",
        description = "Read project-scoped cockpit roots by project id"
    )]
    Snapshot {
        #[exo(positional, description = "Project id from `project list`")]
        id: String,
    },
    #[exo(effect = "pure", description = "Preview project policy repairs")]
    Repair {
        #[exo(flag, description = "Repair stale local-policy sidecar entries")]
        stale_sidecars: bool,
    },
    #[exo(
        operation = "repair-apply",
        effect = "exec",
        description = "Apply project policy repairs after reviewing the preview"
    )]
    RepairApply {
        #[exo(flag, description = "Repair stale local-policy sidecar entries")]
        stale_sidecars: bool,
    },
    #[exo(
        operation = "move-root",
        effect = "exec",
        description = "Retarget a sidecar-backed project to a new checkout root"
    )]
    MoveRoot {
        #[exo(long, description = "Sidecar project key")]
        key: String,
        #[exo(long, description = "New workspace root")]
        to: String,
        #[exo(flag, description = "Show every change without writing")]
        dry_run: bool,
    },
}

impl ProjectCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Resolve => CommandBox::pure(ProjectResolve::new()),
            Self::List => CommandBox::pure(ProjectList::new()),
            Self::Snapshot { id } => CommandBox::pure(ProjectSnapshot::new(id)),
            Self::Repair { stale_sidecars } => {
                CommandBox::pure(ProjectRepair::new(stale_sidecars, false))
            }
            Self::RepairApply { stale_sidecars } => {
                CommandBox::mutable(ProjectRepair::new(stale_sidecars, true))
            }
            Self::MoveRoot { key, to, dry_run } => {
                CommandBox::mutable(ProjectMoveRoot::new(key, PathBuf::from(to), dry_run))
            }
        })
    }
}

// ============================================================================
// project resolve
// ============================================================================

/// Resolve project identity and state/runtime paths.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectResolve;

impl ProjectResolve {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Serialize)]
struct ProjectResolveOutput {
    kind: &'static str,
    ok: bool,
    project: ProjectDto,
    paths: ProjectPathsDto,
}

#[derive(Debug, Serialize)]
struct ProjectDto {
    id: String,
    git_common_dir: PathBuf,
    workspace_root: Option<PathBuf>,
    policy: &'static str,
    sidecar_key: Option<String>,
    sidecar_root: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ProjectPathsDto {
    state_root: PathBuf,
    db_path: PathBuf,
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    endpoint: String,
    pid_path: PathBuf,
    sidecar_manifest_path: Option<PathBuf>,
    sidecar_projection_dir: Option<PathBuf>,
}

impl From<Project> for ProjectResolveOutput {
    fn from(project: Project) -> Self {
        let runtime_dir = project.runtime_dir();
        let socket_path = project.socket_path();
        #[cfg(windows)]
        let endpoint = DaemonEndpoint::from_runtime_dir(&runtime_dir).display();
        #[cfg(not(windows))]
        let endpoint = DaemonEndpoint::from_socket_path(&socket_path).display();
        let paths = ProjectPathsDto {
            state_root: project.state_root.clone(),
            db_path: project.db_path(),
            runtime_dir,
            socket_path,
            endpoint,
            pid_path: project.pid_path(),
            sidecar_manifest_path: project.sidecar_manifest_path(),
            sidecar_projection_dir: project.sidecar_projection_dir(),
        };
        let project = ProjectDto {
            id: project.id.as_str().to_string(),
            git_common_dir: project.git_common_dir,
            workspace_root: project.workspace_root,
            policy: project.policy.as_str(),
            sidecar_key: project.sidecar_key,
            sidecar_root: project.sidecar_root,
        };

        Self {
            kind: "project.resolve",
            ok: true,
            project,
            paths,
        }
    }
}

impl Command for ProjectResolve {
    fn namespace(&self) -> &'static str {
        "project"
    }

    fn operation(&self) -> &'static str {
        "resolve"
    }

    fn description(&self) -> &'static str {
        "Resolve project identity and state/runtime paths"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let output = ProjectResolveOutput::from(resolve_project(ctx.root)?);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = serde_json::to_string_pretty(&output)?;
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

// ============================================================================
// project list
// ============================================================================

/// List locally known Exo projects from project policy and sidecars.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectList;

impl ProjectList {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Serialize)]
struct ProjectListOutput {
    kind: &'static str,
    ok: bool,
    current_project_id: Option<String>,
    projects: Vec<crate::project::ProjectCatalogEntry>,
    diagnostics: Vec<crate::project::ProjectCatalogDiagnostic>,
}

impl From<ProjectCatalog> for ProjectListOutput {
    fn from(catalog: ProjectCatalog) -> Self {
        Self {
            kind: "project.list",
            ok: true,
            current_project_id: catalog.current_project_id,
            projects: catalog.projects,
            diagnostics: catalog.diagnostics,
        }
    }
}

impl Command for ProjectList {
    fn namespace(&self) -> &'static str {
        "project"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List locally known Exo projects from project policy and sidecars"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let output = ProjectListOutput::from(
            project_resolver_for_context(ctx.project).list_catalog(ctx.root)?,
        );

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = serde_json::to_string_pretty(&output)?;
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

// ============================================================================
// project snapshot
// ============================================================================

/// Read project-scoped cockpit roots by project id.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    id: String,
}

impl ProjectSnapshot {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct ProjectSnapshotOutput {
    kind: &'static str,
    ok: bool,
    project: ProjectCatalogEntry,
    workspace_key: Option<String>,
    capabilities: ProjectSnapshotCapabilities,
    roots: BTreeMap<&'static str, JsonValue>,
    diagnostics: Vec<ProjectCatalogDiagnostic>,
}

#[derive(Debug, Serialize)]
struct ProjectSnapshotCapabilities {
    state_readable: bool,
    workspace_available: bool,
    commands_available: bool,
    write_available: bool,
}

impl Command for ProjectSnapshot {
    fn namespace(&self) -> &'static str {
        "project"
    }

    fn operation(&self) -> &'static str {
        "snapshot"
    }

    fn description(&self) -> &'static str {
        "Read project-scoped cockpit roots by project id"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let output = project_snapshot_output(ctx.root, ctx.project, &self.id)?;

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = serde_json::to_string_pretty(&output)?;
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

fn project_snapshot_output(
    root: &std::path::Path,
    context_project: Option<&Project>,
    id: &str,
) -> ExoResult<ProjectSnapshotOutput> {
    let resolver = project_resolver_for_context(context_project);
    let catalog = resolver.list_catalog(root)?;
    let mut project = catalog
        .projects
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| anyhow!("Project {id} is not in the local Exo catalog"))?;
    let db_path = project
        .db_path
        .as_ref()
        .ok_or_else(|| anyhow!("Project {id} does not have a readable Exo state database"))?;
    if !db_path.exists() {
        let sql_dir = catalog_sql_projection_dir(root, &project)
            .filter(|sql_dir| sql_dir.join("epochs.sql").exists());
        if let Some(sql_dir) = sql_dir {
            crate::context::import_sql_dumps(&sql_dir, db_path)?;
        } else {
            anyhow::bail!(
                "Project {id} state database does not exist at {}",
                db_path.display()
            );
        }
    }

    let loader = SqliteLoader::open(db_path)?;
    let mut diagnostics = Vec::new();
    let workspace_roots = loader
        .list_workspace_active_phase_roots()
        .unwrap_or_default();
    let workspace_key = project
        .workspace_root
        .as_ref()
        .map(|root| root.to_string_lossy().into_owned())
        .or_else(|| (workspace_roots.len() == 1).then(|| workspace_roots[0].clone()));
    let workspace_root = project
        .workspace_root
        .clone()
        .or_else(|| canonical_workspace_root_from_state(&workspace_roots, &mut diagnostics));
    let workspace_available = workspace_root.as_deref().is_some_and(|workspace_root| {
        workspace_belongs_to_project(workspace_root, &project, &mut diagnostics)
    });
    if let Some(workspace_root) = workspace_root {
        if workspace_available {
            project.workspace_root = Some(workspace_root);
        } else {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "projects".to_string(),
                severity: "warning",
                message: format!(
                    "Recorded canonical checkout path {} is not available as a local git checkout; repo/git command panels are unavailable.",
                    workspace_root.display()
                ),
            });
        }
    }
    project.workspace_available = workspace_available;
    project.commands_available = workspace_available;
    project.write_available = workspace_available;

    if workspace_available
        && let Some(workspace_root) = project.workspace_root.as_deref()
        && let Ok(resolved_project) = resolver.resolve(workspace_root)
        && resolved_project.id.as_str() == project.id
    {
        crate::rfc::reconcile_rfcs_once_with_project(workspace_root, Some(&resolved_project))
            .with_context(|| "Failed to reconcile RFC metadata for project snapshot")?;
    }

    let phase_details = loader.load_active_phase_details_for_workspace(workspace_key.as_deref())?;
    let state = loader.load_state()?;
    let rfcs = loader.load_rfcs_for_display()?;
    let inbox = loader.load_inbox()?;
    let git_dirty = project
        .workspace_root
        .as_deref()
        .is_some_and(workspace_git_dirty);

    if !workspace_available && diagnostics.is_empty() {
        diagnostics.push(ProjectCatalogDiagnostic {
            source: "projects".to_string(),
            severity: "info",
            message: "No canonical local checkout binding is recorded for this project; state panels are available, but repo/git command panels are unavailable.".to_string(),
        });
    }
    if workspace_key.is_none() {
        diagnostics.push(ProjectCatalogDiagnostic {
            source: "projects".to_string(),
            severity: "warning",
            message: "No workspace active-phase key could be derived from project state."
                .to_string(),
        });
    }

    let mut roots = BTreeMap::new();
    roots.insert(
        "status",
        status_root_for_project(
            &project,
            workspace_key.as_deref(),
            phase_details.as_ref(),
            git_dirty,
        ),
    );
    roots.insert("phase-details", serde_json::to_value(&phase_details)?);
    roots.insert("plan", serde_json::to_value(&state)?);
    roots.insert(
        "rfc-pipeline",
        rfc_pipeline_root_for_phase(phase_details.as_ref(), rfcs)?,
    );
    roots.insert("inbox", json!({ "entries": inbox }));

    Ok(ProjectSnapshotOutput {
        kind: "project.snapshot",
        ok: true,
        capabilities: ProjectSnapshotCapabilities {
            state_readable: project.state_readable,
            workspace_available: project.workspace_available,
            commands_available: project.commands_available,
            write_available: project.write_available,
        },
        project,
        workspace_key,
        roots,
        diagnostics,
    })
}

fn status_root_for_project(
    project: &ProjectCatalogEntry,
    workspace_key: Option<&str>,
    phase_details: Option<&crate::context::sqlite_loader::PhaseDetailsData>,
    git_dirty: bool,
) -> JsonValue {
    let completed_goals = phase_details
        .map(|details| details.progress.goals_completed)
        .unwrap_or_default();
    let total_goals = phase_details
        .map(|details| details.progress.goals_total)
        .unwrap_or_default();

    json!({
        "kind": "project.status",
        "project_id": project.id,
        "phase_id": phase_details.map(|details| details.phase_id.as_str()),
        "phase_title": phase_details.map(|details| details.phase_title.as_str()).unwrap_or("No active phase"),
        "epoch_title": phase_details.map(|details| details.epoch_title.as_str()).unwrap_or("Unknown epoch"),
        "progress_mode": if phase_details.is_some() { "executing" } else { "unknown" },
        "git_dirty": git_dirty,
        "completed_goals": completed_goals,
        "pending_goals": total_goals.saturating_sub(completed_goals),
        "workspace_key": workspace_key,
        "workspace_root": project.workspace_root,
        "state_root": project.state_root,
        "state_readable": project.state_readable,
        "workspace_available": project.workspace_available,
        "commands_available": project.commands_available,
        "write_available": project.write_available,
    })
}

fn canonical_workspace_root_from_state(
    workspace_roots: &[String],
    diagnostics: &mut Vec<ProjectCatalogDiagnostic>,
) -> Option<PathBuf> {
    match workspace_roots {
        [root] => Some(PathBuf::from(root)),
        [] => None,
        roots => {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "projects".to_string(),
                severity: "warning",
                message: format!(
                    "Multiple workspace bindings are recorded for this project ({}). Choose one canonical checkout; additional checkouts should be represented as worktrees.",
                    roots.join(", ")
                ),
            });
            None
        }
    }
}

fn workspace_belongs_to_project(
    root: &Path,
    project: &ProjectCatalogEntry,
    diagnostics: &mut Vec<ProjectCatalogDiagnostic>,
) -> bool {
    if !root.exists() {
        return false;
    }

    let is_git_checkout = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
        .ok()
        .is_some_and(|output| output.status.success());
    if !is_git_checkout {
        return false;
    }

    match Project::resolve(root) {
        Ok(resolved) if resolved.id.as_str() == project.id => true,
        Ok(resolved) => {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "projects".to_string(),
                severity: "warning",
                message: format!(
                    "Recorded canonical checkout path {} belongs to project {}, not selected project {}; repo/git command panels are unavailable.",
                    root.display(),
                    resolved.id.as_str(),
                    project.id
                ),
            });
            false
        }
        Err(error) => {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "projects".to_string(),
                severity: "warning",
                message: format!(
                    "Recorded canonical checkout path {} could not be resolved as an Exo project ({error}); repo/git command panels are unavailable.",
                    root.display()
                ),
            });
            false
        }
    }
}

fn catalog_sql_projection_dir(root: &Path, project: &ProjectCatalogEntry) -> Option<PathBuf> {
    match project.state {
        "repo" => Some(
            project
                .workspace_root
                .as_deref()
                .unwrap_or(root)
                .join("docs/agent-context"),
        ),
        "sidecar" => Some(
            project
                .sidecar_root
                .as_ref()?
                .join("projects")
                .join(project.sidecar_key.as_ref()?)
                .join("agent-context"),
        ),
        "shadow" => None,
        _ => project
            .state_root
            .as_ref()
            .map(|state_root| state_root.join("agent-context")),
    }
}

fn workspace_git_dirty(root: &Path) -> bool {
    ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| !output.stdout.is_empty())
}

fn rfc_pipeline_root(rfcs: Vec<crate::context::sqlite_loader::RfcRecord>) -> ExoResult<JsonValue> {
    let mut by_stage: BTreeMap<String, Vec<JsonValue>> = BTreeMap::new();
    for rfc in rfcs {
        let stage = format!("stage-{}", rfc.stage);
        by_stage
            .entry(stage)
            .or_default()
            .push(serde_json::to_value(rfc)?);
    }

    let entries = by_stage
        .into_iter()
        .map(|(stage, rfcs)| json!({ "stage": stage, "rfcs": rfcs }))
        .collect::<Vec<_>>();
    Ok(json!({ "entries": entries }))
}

fn rfc_pipeline_root_for_phase(
    phase_details: Option<&crate::context::sqlite_loader::PhaseDetailsData>,
    rfcs: Vec<crate::context::sqlite_loader::RfcRecord>,
) -> ExoResult<JsonValue> {
    let Some(phase_details) = phase_details else {
        return rfc_pipeline_root(Vec::new());
    };

    let attached = phase_details
        .rfcs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let attached_numbers = phase_details
        .rfcs
        .iter()
        .filter_map(|id| id.parse::<i64>().ok())
        .collect::<BTreeSet<_>>();
    let scoped = rfcs
        .into_iter()
        .filter(|rfc| {
            attached.contains(rfc.text_id.as_str())
                || attached.contains(crate::rfc::format_rfc_number(rfc.rfc_number).as_str())
                || attached_numbers.contains(&rfc.rfc_number)
        })
        .collect::<Vec<_>>();
    rfc_pipeline_root(scoped)
}

// ============================================================================
// project move-root
// ============================================================================

/// Retarget a sidecar-backed project to a new checkout root.
#[derive(Debug, Clone)]
pub struct ProjectMoveRoot {
    key: String,
    to: PathBuf,
    dry_run: bool,
}

impl ProjectMoveRoot {
    pub fn new(key: impl Into<String>, to: PathBuf, dry_run: bool) -> Self {
        Self {
            key: key.into(),
            to,
            dry_run,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProjectMoveRootOutput {
    kind: &'static str,
    ok: bool,
    dry_run: bool,
    apply_ready: bool,
    key: String,
    sidecar_root: PathBuf,
    sidecar_project_dir: PathBuf,
    sidecar_project_id: String,
    config_path: PathBuf,
    old_policy_project_id: String,
    new_policy_project_id: String,
    #[serde(skip_serializing)]
    new_workspace_id: String,
    #[serde(skip_serializing)]
    absorb_completed_source_active_phase: bool,
    old_workspace_root: Option<String>,
    new_workspace_root: String,
    changes: Vec<ProjectMoveRootChange>,
    warnings: Vec<String>,
    blockers: Vec<String>,
    verification: ProjectMoveRootVerification,
}

#[derive(Debug, Serialize)]
struct ProjectMoveRootChange {
    target: &'static str,
    action: &'static str,
    from: Option<String>,
    to: Option<String>,
    rows: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ProjectMoveRootVerification {
    destination_exists: bool,
    destination_is_git_worktree: bool,
    rfc_00001_found: bool,
    rfc_00001_path: Option<PathBuf>,
    write_owner_marker: Option<ProjectMoveRootWriteOwnerMarker>,
}

#[derive(Debug, Serialize)]
struct ProjectMoveRootWriteOwnerMarker {
    path: PathBuf,
    workspace_root: Option<String>,
    pid: Option<u32>,
    status: &'static str,
}

#[derive(Debug, Clone)]
struct ProjectMoveRootPolicyBinding {
    project_id: String,
    sidecar_root: PathBuf,
}

#[derive(Debug, Clone)]
struct ProjectMoveRootDbRoots {
    workspace_active_phase_roots: Vec<String>,
    phase_owner_workspace_roots: Vec<String>,
}

impl MutableCommand for ProjectMoveRoot {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        if self.key.trim().is_empty() {
            anyhow::bail!("project move-root requires a non-empty --key");
        }

        let mut output = build_project_move_root_output(
            ctx.root,
            ctx.project,
            &self.key,
            &self.to,
            self.dry_run,
        )?;

        if !self.dry_run {
            if !output.blockers.is_empty() {
                anyhow::bail!(
                    "project move-root cannot apply until blockers are resolved: {}",
                    output.blockers.join("; ")
                );
            }
            apply_project_move_root(&mut output)?;
        }

        let message = project_move_root_message(&output);
        Ok(CommandOutput::new(output, message))
    }
}

impl Command for ProjectMoveRoot {
    fn namespace(&self) -> &'static str {
        "project"
    }

    fn operation(&self) -> &'static str {
        "move-root"
    }

    fn description(&self) -> &'static str {
        "Retarget a sidecar-backed project to a new checkout root"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("ProjectMoveRoot should be dispatched via execute_mut");
    }
}

fn build_project_move_root_output(
    root: &Path,
    context_project: Option<&Project>,
    key: &str,
    to: &Path,
    dry_run: bool,
) -> ExoResult<ProjectMoveRootOutput> {
    let to = if to.is_absolute() {
        to.to_path_buf()
    } else {
        root.join(to)
    };
    let resolver = project_resolver_for_context(context_project);
    let config_path = resolver.local_projects_config_path()?;
    let doc = read_local_projects_doc(&config_path)?;

    let target_project = resolver.resolve(&to).map_err(|err| {
        let message = err.to_string();
        if message.contains("requires a git repository") {
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "Destination {} is not a git worktree. Run `git init` or clone the repository there before moving the Exo project root.",
                    to.display()
                ),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Initialize git".to_string(),
                    command: format!("git -C {} init", to.display()),
                    rationale: "project move-root requires the destination to have a git project identity."
                        .to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(1.0),
                }]),
            )
            .into()
        } else {
            err
        }
    })?;
    let new_workspace_root = target_project.workspace_root.clone().ok_or_else(|| {
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "Destination {} is a git repository without a worktree. Choose a non-bare checkout root for project move-root.",
                to.display()
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Use a checkout root".to_string(),
                command: "exo project move-root --key <key> --to <checkout-root>".to_string(),
                rationale: "workspace active-phase and ownership state require a real git worktree root.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(1.0),
            }]),
        )
    })?;
    let new_workspace_root_string = new_workspace_root.to_string_lossy().into_owned();
    let new_policy_project_id = target_project.id.as_str().to_string();
    let new_workspace_id =
        move_root_workspace_id(&new_policy_project_id, &new_workspace_root_string);
    let binding = find_sidecar_policy_binding(&doc, key, Some(&new_policy_project_id))?;

    let sidecar_project_dir = binding.sidecar_root.join("projects").join(key);
    let manifest_path = sidecar_project_dir.join("sidecar.toml");
    let sidecar_project_id = read_sidecar_manifest_project_id(&manifest_path, key)?;
    let db_path = sidecar_project_dir.join("cache").join("exo.db");

    let db_roots = read_project_move_root_db_roots(&db_path)?;
    let marker = read_move_root_write_owner_marker(&binding.sidecar_root, key)?;
    let marker_root = marker
        .as_ref()
        .and_then(|marker| marker.workspace_root.clone());

    let mut candidate_old_roots = BTreeSet::new();
    for root in db_roots
        .workspace_active_phase_roots
        .iter()
        .chain(db_roots.phase_owner_workspace_roots.iter())
    {
        if root != &new_workspace_root_string {
            candidate_old_roots.insert(root.clone());
        }
    }
    if let Some(root) = &marker_root
        && marker
            .as_ref()
            .is_none_or(|marker| marker.status != "owned")
        && root != &new_workspace_root_string
        && !candidate_old_roots
            .iter()
            .any(|candidate| move_root_paths_equivalent(candidate, root))
    {
        candidate_old_roots.insert(root.clone());
    }

    let old_workspace_root = match candidate_old_roots.len() {
        0 => None,
        1 => candidate_old_roots.iter().next().cloned(),
        _ => None,
    };

    let workspace_active_rows = old_workspace_root
        .as_deref()
        .map(|old| {
            count_project_move_root_rows(
                &db_path,
                "workspace_active_phase_data",
                "workspace_root",
                old,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let phase_owner_rows = old_workspace_root
        .as_deref()
        .map(|old| {
            count_project_move_root_rows(
                &db_path,
                "phase_ownership_data",
                "claimed_by_workspace_root",
                old,
            )
        })
        .transpose()?
        .unwrap_or_default();

    let mut changes = vec![ProjectMoveRootChange {
        target: "local_project_policy",
        action: if binding.project_id == new_policy_project_id {
            "unchanged"
        } else {
            "retarget"
        },
        from: Some(binding.project_id.clone()),
        to: Some(new_policy_project_id.clone()),
        rows: None,
    }];
    changes.push(ProjectMoveRootChange {
        target: "sidecar_manifest.project_id",
        action: if sidecar_project_id == new_policy_project_id {
            "unchanged"
        } else {
            "retarget"
        },
        from: Some(sidecar_project_id.clone()),
        to: Some(new_policy_project_id.clone()),
        rows: None,
    });

    let destination_active_root_recorded = db_roots
        .workspace_active_phase_roots
        .iter()
        .any(|root| root == &new_workspace_root_string);
    let destination_owner_root_recorded = db_roots
        .phase_owner_workspace_roots
        .iter()
        .any(|root| root == &new_workspace_root_string);
    let source_active_phase_statuses = old_workspace_root
        .as_deref()
        .map(|old| workspace_active_phase_statuses(&db_path, old))
        .transpose()?
        .unwrap_or_default();
    let source_active_phase_completed = !source_active_phase_statuses.is_empty()
        && source_active_phase_statuses
            .iter()
            .all(|status| status == "completed");
    let source_owned_phase_statuses = old_workspace_root
        .as_deref()
        .map(|old| phase_owner_statuses(&db_path, old))
        .transpose()?
        .unwrap_or_default();
    let source_owned_phases_retargetable = source_owned_phase_statuses
        .iter()
        .all(|status| status == "completed" || status == "pending");
    let absorb_completed_source_active_phase = old_workspace_root.is_some()
        && destination_active_root_recorded
        && source_active_phase_completed
        && source_owned_phases_retargetable;

    if let Some(old) = &old_workspace_root {
        changes.push(ProjectMoveRootChange {
            target: "workspace_active_phase.workspace_root",
            action: if absorb_completed_source_active_phase {
                "delete_completed_source"
            } else {
                "rewrite"
            },
            from: Some(old.clone()),
            to: if absorb_completed_source_active_phase {
                None
            } else {
                Some(new_workspace_root_string.clone())
            },
            rows: Some(workspace_active_rows),
        });
        changes.push(ProjectMoveRootChange {
            target: "phase_ownership.claimed_by_workspace_root",
            action: "rewrite",
            from: Some(old.clone()),
            to: Some(new_workspace_root_string.clone()),
            rows: Some(phase_owner_rows),
        });
    }

    if let Some(marker) = &marker
        && marker.workspace_root.as_deref() != Some(new_workspace_root_string.as_str())
    {
        changes.push(ProjectMoveRootChange {
            target: "sidecar_write_owner_marker.workspace_root",
            action: if matches!(marker.status, "owned" | "stale") {
                "rewrite"
            } else {
                "inspect"
            },
            from: marker.workspace_root.clone(),
            to: Some(new_workspace_root_string.clone()),
            rows: None,
        });
    }

    let mut warnings = Vec::new();
    let mut blockers = Vec::new();
    if let Err(error) =
        validate_policy_retarget_doc(&doc, &binding.project_id, &new_policy_project_id)
    {
        blockers.push(error.to_string());
    }
    if candidate_old_roots.len() > 1 {
        blockers.push(format!(
            "multiple existing workspace roots are recorded for key {key}: {}",
            candidate_old_roots
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if old_workspace_root.is_some()
        && destination_active_root_recorded
        && !absorb_completed_source_active_phase
    {
        if source_active_phase_completed && !source_owned_phases_retargetable {
            blockers.push(format!(
                "source workspace root still owns in-progress phase state; inspect {} before applying",
                db_path.display()
            ));
        } else {
            blockers.push(format!(
                "both old and new workspace roots have active project state; inspect {} before applying",
                db_path.display()
            ));
        }
    }
    if old_workspace_root.is_some()
        && destination_owner_root_recorded
        && !destination_active_root_recorded
    {
        blockers.push(format!(
            "destination workspace root already has phase ownership state; inspect {} before applying",
            db_path.display()
        ));
    }
    if let Some(marker) = &marker {
        match marker.status {
            "owned"
                if marker.workspace_root.as_deref() != Some(new_workspace_root_string.as_str()) =>
            {
                warnings.push(format!(
                    "current process owns sidecar write marker {}; it will be retargeted with the moved workspace",
                    marker.path.display()
                ));
            }
            "active" => {
                blockers.push(format!(
                    "sidecar write-owner marker {} belongs to a live process for {}",
                    marker.path.display(),
                    marker.workspace_root.as_deref().unwrap_or("<unknown>")
                ));
            }
            "stale"
                if marker.workspace_root.as_deref() != Some(new_workspace_root_string.as_str()) =>
            {
                warnings.push(format!(
                    "stale sidecar write-owner marker {} will be retargeted; remove the old checkout manually if the filesystem cannot delete it",
                    marker.path.display()
                ));
            }
            "unknown"
                if marker.workspace_root.as_deref() != Some(new_workspace_root_string.as_str()) =>
            {
                blockers.push(format!(
                    "sidecar write-owner marker {} has unknown liveness for {}",
                    marker.path.display(),
                    marker.workspace_root.as_deref().unwrap_or("<unknown>")
                ));
            }
            _ => {}
        }
    }

    let rfc_00001_path = discover_rfc_00001(&new_workspace_root)?;
    let verification = ProjectMoveRootVerification {
        destination_exists: new_workspace_root.exists(),
        destination_is_git_worktree: true,
        rfc_00001_found: rfc_00001_path.is_some(),
        rfc_00001_path,
        write_owner_marker: marker,
    };

    Ok(ProjectMoveRootOutput {
        kind: "project.move_root",
        ok: true,
        dry_run,
        apply_ready: blockers.is_empty(),
        key: key.to_string(),
        sidecar_root: binding.sidecar_root,
        sidecar_project_dir,
        sidecar_project_id,
        config_path,
        old_policy_project_id: binding.project_id,
        new_policy_project_id,
        new_workspace_id,
        absorb_completed_source_active_phase,
        old_workspace_root,
        new_workspace_root: new_workspace_root_string,
        changes,
        warnings,
        blockers,
        verification,
    })
}

fn apply_project_move_root(output: &mut ProjectMoveRootOutput) -> ExoResult<()> {
    let db_path = output.sidecar_project_dir.join("cache").join("exo.db");
    let mut doc = read_local_projects_doc(&output.config_path)?;
    validate_policy_retarget_doc(
        &doc,
        &output.old_policy_project_id,
        &output.new_policy_project_id,
    )?;
    read_sidecar_manifest_doc(
        &output.sidecar_project_dir.join("sidecar.toml"),
        &output.key,
    )?;

    if db_path.exists()
        && let Some(old_root) = output.old_workspace_root.as_deref()
    {
        let db = exosuit_storage::open_database(&db_path)
            .with_context(|| format!("Failed to open sidecar database {}", db_path.display()))?;
        let conn = db.connection();
        let result: ExoResult<(usize, usize)> = (|| {
            conn.execute_batch("BEGIN IMMEDIATE")
                .context("Failed to begin project move-root transaction")?;
            if output.absorb_completed_source_active_phase {
                validate_absorb_completed_source_active_phase_for_apply(conn, old_root)?;
            }
            let workspace_rows = if output.absorb_completed_source_active_phase {
                conn.execute(
                    "DELETE FROM workspace_active_phase
                     WHERE workspace_root = ?1",
                    [old_root],
                )
                .context("Failed to delete completed source workspace active-phase root")?
            } else {
                conn.execute(
                    "UPDATE workspace_active_phase
                     SET workspace_root = ?1
                     WHERE workspace_root = ?2",
                    (&output.new_workspace_root, old_root),
                )
                .context("Failed to rewrite workspace active-phase roots")?
            };
            let owner_rows = conn
                .execute(
                    "UPDATE phase_ownership
                     SET claimed_by_workspace_root = ?1,
                         claimed_by_workspace_id = ?2,
                         owner_id = CASE
                             WHEN owner_kind = 'workspace' THEN ?2
                             ELSE owner_id
                         END
                     WHERE claimed_by_workspace_root = ?3",
                    (
                        &output.new_workspace_root,
                        &output.new_workspace_id,
                        old_root,
                    ),
                )
                .context("Failed to rewrite phase ownership workspace roots")?;
            conn.execute_batch("COMMIT")
                .context("Failed to commit project move-root transaction")?;
            Ok((workspace_rows, owner_rows))
        })();
        let (workspace_rows, owner_rows) = match result {
            Ok(rows) => rows,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        };
        for change in &mut output.changes {
            match change.target {
                "workspace_active_phase.workspace_root" => change.rows = Some(workspace_rows),
                "phase_ownership.claimed_by_workspace_root" => change.rows = Some(owner_rows),
                _ => {}
            }
        }
    }

    retarget_sidecar_policy_doc(
        &mut doc,
        &output.old_policy_project_id,
        &output.new_policy_project_id,
    )?;
    write_sidecar_manifest_project_id(
        &output.sidecar_project_dir.join("sidecar.toml"),
        &output.key,
        &output.new_policy_project_id,
    )?;
    output.sidecar_project_id = output.new_policy_project_id.clone();
    write_local_projects_doc(&output.config_path, &doc)?;

    if let Some(marker) = &output.verification.write_owner_marker
        && matches!(marker.status, "owned" | "stale")
        && marker.workspace_root.as_deref() != Some(output.new_workspace_root.as_str())
    {
        rewrite_write_owner_marker_workspace_root(&marker.path, &output.new_workspace_root)?;
        if let Some(marker) = &mut output.verification.write_owner_marker {
            marker.workspace_root = Some(output.new_workspace_root.clone());
            marker.status = if marker.status == "owned" {
                "owned"
            } else {
                "retargeted"
            };
        }
    }

    output.dry_run = false;
    output.apply_ready = true;
    Ok(())
}

fn validate_absorb_completed_source_active_phase_for_apply(
    conn: &Connection,
    old_root: &str,
) -> ExoResult<()> {
    let active_non_completed = conn
        .query_row(
            "SELECT COUNT(*)
             FROM workspace_active_phase_data wap
             JOIN phases_data p ON p.id = wap.phase_id
             WHERE wap.workspace_root = ?1 AND p.status != 'completed'",
            [old_root],
            |row| row.get::<_, i64>(0),
        )
        .context("Failed to revalidate source active phase status")?;
    if active_non_completed > 0 {
        anyhow::bail!(
            "source workspace root active phase changed before move-root apply; retry after inspecting active project state"
        );
    }

    let owned_in_progress = conn
        .query_row(
            "SELECT COUNT(*)
             FROM phase_ownership_data po
             JOIN phases_data p ON p.id = po.phase_id
             WHERE po.claimed_by_workspace_root = ?1
               AND p.status NOT IN ('completed', 'pending')",
            [old_root],
            |row| row.get::<_, i64>(0),
        )
        .context("Failed to revalidate source phase ownership status")?;
    if owned_in_progress > 0 {
        anyhow::bail!(
            "source workspace root ownership changed before move-root apply; retry after inspecting active project state"
        );
    }

    Ok(())
}

fn project_move_root_message(output: &ProjectMoveRootOutput) -> String {
    let mode = if output.dry_run {
        "Project root move preview"
    } else {
        "Moved project root"
    };
    let mut message = format!(
        "{mode}: {} -> {}\nSidecar key: {}\nSidecar project id: {}",
        output
            .old_workspace_root
            .as_deref()
            .unwrap_or("<none recorded>"),
        output.new_workspace_root,
        output.key,
        output.sidecar_project_id
    );
    if !output.verification.rfc_00001_found {
        message.push_str("\nRFC 00001: not found at destination");
    } else if let Some(path) = &output.verification.rfc_00001_path {
        message.push_str(&format!("\nRFC 00001: {}", path.display()));
    }
    if !output.blockers.is_empty() {
        message.push_str("\nBlockers:");
        for blocker in &output.blockers {
            message.push_str(&format!("\n  - {blocker}"));
        }
    }
    if !output.warnings.is_empty() {
        message.push_str("\nWarnings:");
        for warning in &output.warnings {
            message.push_str(&format!("\n  - {warning}"));
        }
    }
    message
}

fn read_local_projects_doc(config_path: &Path) -> ExoResult<DocumentMut> {
    if !config_path.exists() {
        return Ok(DocumentMut::new());
    }
    let content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse {}", config_path.display()))
}

fn write_local_projects_doc(config_path: &Path, doc: &DocumentMut) -> ExoResult<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }
    std::fs::write(config_path, doc.to_string())
        .with_context(|| format!("Failed to write {}", config_path.display()))
}

fn find_sidecar_policy_binding(
    doc: &DocumentMut,
    key: &str,
    destination_project_id: Option<&str>,
) -> ExoResult<ProjectMoveRootPolicyBinding> {
    let Some(projects) = doc["projects"].as_table() else {
        anyhow::bail!("No local Exo project policy exists; link or bootstrap the sidecar first");
    };
    let mut matches = Vec::new();
    for (project_id, item) in projects.iter() {
        if item
            .get("state")
            .and_then(Item::as_str)
            .is_some_and(|state| state == "sidecar")
            && item
                .get("sidecar_key")
                .and_then(Item::as_str)
                .is_some_and(|sidecar_key| sidecar_key == key)
        {
            let sidecar_root = item
                .get("sidecar_root")
                .and_then(Item::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("Sidecar policy for key {key} is missing sidecar_root"))?;
            matches.push(ProjectMoveRootPolicyBinding {
                project_id: project_id.to_string(),
                sidecar_root,
            });
        }
    }
    match matches.len() {
        0 => anyhow::bail!("No local sidecar project policy entry found for key {key}"),
        1 => Ok(matches.remove(0)),
        _ => {
            if let Some(destination_project_id) = destination_project_id {
                let roots = matches
                    .iter()
                    .map(|binding| binding.sidecar_root.clone())
                    .collect::<BTreeSet<_>>();
                let source_matches = matches
                    .iter()
                    .filter(|binding| binding.project_id != destination_project_id)
                    .collect::<Vec<_>>();
                if roots.len() == 1 && source_matches.len() == 1 {
                    return Ok(source_matches[0].clone());
                }
            }
            anyhow::bail!(
                "Multiple local sidecar project policy entries found for key {key}; repair projects.toml before moving the root"
            )
        }
    }
}

fn retarget_sidecar_policy_doc(
    doc: &mut DocumentMut,
    old_project_id: &str,
    new_project_id: &str,
) -> ExoResult<()> {
    validate_policy_retarget_doc(doc, old_project_id, new_project_id)?;
    if old_project_id == new_project_id {
        return Ok(());
    }
    let Some(projects) = doc["projects"].as_table_mut() else {
        anyhow::bail!("No local Exo project policy exists");
    };
    let old_item = projects
        .remove(old_project_id)
        .ok_or_else(|| anyhow!("Project policy entry {old_project_id} disappeared"))?;
    if projects.get(new_project_id).is_some() {
        projects.remove(new_project_id);
    }
    projects.insert(new_project_id, old_item);
    Ok(())
}

fn validate_policy_retarget_doc(
    doc: &DocumentMut,
    old_project_id: &str,
    new_project_id: &str,
) -> ExoResult<()> {
    if old_project_id == new_project_id {
        return Ok(());
    }
    let Some(projects) = doc["projects"].as_table() else {
        anyhow::bail!("No local Exo project policy exists");
    };
    let old_item = projects
        .get(old_project_id)
        .ok_or_else(|| anyhow!("Project policy entry {old_project_id} disappeared"))?;
    if let Some(existing) = projects.get(new_project_id)
        && !same_sidecar_policy_entry(existing, old_item)
    {
        anyhow::bail!(
            "Destination project id {new_project_id} already has a different local project policy entry"
        );
    }
    Ok(())
}

fn same_sidecar_policy_entry(left: &Item, right: &Item) -> bool {
    for key in ["state", "sidecar_key", "sidecar_root"] {
        if left.get(key).and_then(Item::as_str) != right.get(key).and_then(Item::as_str) {
            return false;
        }
    }
    if sidecar_policy_auto_push(left) != sidecar_policy_auto_push(right) {
        return false;
    }
    if sidecar_policy_auto_commit(left) != sidecar_policy_auto_commit(right) {
        return false;
    }
    true
}

fn sidecar_policy_auto_push(item: &Item) -> Option<&str> {
    match item.get("auto_push") {
        Some(value) => value.as_str(),
        None => Some("if_remote"),
    }
}

fn sidecar_policy_auto_commit(item: &Item) -> Option<bool> {
    match item.get("auto_commit") {
        Some(value) => value.as_bool(),
        None => Some(true),
    }
}

fn move_root_workspace_id(project_id: &str, workspace_root: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(workspace_root.as_bytes());
    let hash = hasher.finalize().to_hex();
    format!("workspace:{project_id}:{}", &hash.as_str()[..16])
}

fn move_root_paths_equivalent(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let Ok(left) = Path::new(left).canonicalize() else {
        return false;
    };
    let Ok(right) = Path::new(right).canonicalize() else {
        return false;
    };
    left == right
}

fn read_sidecar_manifest_project_id(manifest_path: &Path, key: &str) -> ExoResult<String> {
    let value = read_sidecar_manifest_doc(manifest_path, key)?;
    value
        .get("sidecar")
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get("project_id"))
        .and_then(toml::Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            anyhow!(
                "Sidecar manifest {} is missing sidecar.project_id",
                manifest_path.display()
            )
        })
}

fn read_sidecar_manifest_doc(manifest_path: &Path, key: &str) -> ExoResult<toml::Value> {
    let content = std::fs::read_to_string(manifest_path).with_context(|| {
        format!(
            "Failed to read sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    let value: toml::Value = toml::from_str(&content).with_context(|| {
        format!(
            "Failed to parse sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    let section = value
        .get("sidecar")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            anyhow!(
                "Sidecar manifest {} is missing [sidecar]",
                manifest_path.display()
            )
        })?;
    let manifest_key = section
        .get("key")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            anyhow!(
                "Sidecar manifest {} is missing sidecar.key",
                manifest_path.display()
            )
        })?;
    if manifest_key != key {
        anyhow::bail!(
            "Sidecar manifest {} belongs to key {}, not {}",
            manifest_path.display(),
            manifest_key,
            key
        );
    }
    Ok(value)
}

fn write_sidecar_manifest_project_id(
    manifest_path: &Path,
    key: &str,
    project_id: &str,
) -> ExoResult<()> {
    read_sidecar_manifest_doc(manifest_path, key)?;
    let content = std::fs::read_to_string(manifest_path).with_context(|| {
        format!(
            "Failed to read sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    let mut doc = content.parse::<DocumentMut>().with_context(|| {
        format!(
            "Failed to parse sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    doc["sidecar"]["project_id"] = value(project_id);
    std::fs::write(manifest_path, doc.to_string()).with_context(|| {
        format!(
            "Failed to write sidecar manifest {}",
            manifest_path.display()
        )
    })
}

fn read_project_move_root_db_roots(db_path: &Path) -> ExoResult<ProjectMoveRootDbRoots> {
    if !db_path.exists() {
        return Ok(ProjectMoveRootDbRoots {
            workspace_active_phase_roots: Vec::new(),
            phase_owner_workspace_roots: Vec::new(),
        });
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open sidecar database {}", db_path.display()))?;
    Ok(ProjectMoveRootDbRoots {
        workspace_active_phase_roots: read_distinct_text_values(
            &conn,
            "workspace_active_phase_data",
            "workspace_root",
        )?,
        phase_owner_workspace_roots: read_distinct_text_values(
            &conn,
            "phase_ownership_data",
            "claimed_by_workspace_root",
        )?,
    })
}

fn read_distinct_text_values(
    conn: &Connection,
    table: &'static str,
    column: &'static str,
) -> ExoResult<Vec<String>> {
    if !sqlite_table_exists(conn, table)? {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT DISTINCT {column}
         FROM {table}
         WHERE {column} IS NOT NULL AND {column} != ''
         ORDER BY {column}"
    );
    let mut stmt = conn
        .prepare(&sql)
        .with_context(|| format!("Failed to prepare {table}.{column} query"))?;
    stmt.query_map([], |row| row.get::<_, String>(0))
        .with_context(|| format!("Failed to query {table}.{column}"))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to read {table}.{column}"))
}

fn count_project_move_root_rows(
    db_path: &Path,
    table: &'static str,
    column: &'static str,
    value: &str,
) -> ExoResult<usize> {
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open sidecar database {}", db_path.display()))?;
    if !sqlite_table_exists(&conn, table)? {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1");
    let count = conn
        .query_row(&sql, [value], |row| row.get::<_, i64>(0))
        .with_context(|| format!("Failed to count rows in {table}.{column}"))?;
    Ok(count as usize)
}

fn workspace_active_phase_statuses(db_path: &Path, workspace_root: &str) -> ExoResult<Vec<String>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open sidecar database {}", db_path.display()))?;
    if !sqlite_table_exists(&conn, "workspace_active_phase_data")?
        || !sqlite_table_exists(&conn, "phases_data")?
    {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT p.status
             FROM workspace_active_phase_data wap
             JOIN phases_data p ON p.id = wap.phase_id
             WHERE wap.workspace_root = ?1",
        )
        .context("Failed to prepare workspace active phase status query")?;
    stmt.query_map([workspace_root], |row| row.get::<_, String>(0))
        .context("Failed to query workspace active phase statuses")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read workspace active phase statuses")
}

fn phase_owner_statuses(db_path: &Path, workspace_root: &str) -> ExoResult<Vec<String>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open sidecar database {}", db_path.display()))?;
    if !sqlite_table_exists(&conn, "phase_ownership_data")?
        || !sqlite_table_exists(&conn, "phases_data")?
    {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT p.status
             FROM phase_ownership_data po
             JOIN phases_data p ON p.id = po.phase_id
             WHERE po.claimed_by_workspace_root = ?1",
        )
        .context("Failed to prepare phase ownership status query")?;
    stmt.query_map([workspace_root], |row| row.get::<_, String>(0))
        .context("Failed to query phase ownership statuses")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read phase ownership statuses")
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> ExoResult<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |_| Ok(()),
    )
    .optional()
    .map(|value| value.is_some())
    .context("Failed to inspect SQLite schema")
}

fn discover_rfc_00001(root: &Path) -> ExoResult<Option<PathBuf>> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        return Ok(None);
    }
    Ok(WalkDir::new(&rfc_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "md")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(crate::rfc::parse_rfc_number)
                    == Some(1)
        }))
}

fn read_move_root_write_owner_marker(
    sidecar_root: &Path,
    key: &str,
) -> ExoResult<Option<ProjectMoveRootWriteOwnerMarker>> {
    let path = sidecar_write_owner_marker_path(sidecar_root, key);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "Failed to read sidecar write-owner marker {}",
            path.display()
        )
    })?;
    let value: JsonValue = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse sidecar write-owner marker {}",
            path.display()
        )
    })?;
    let workspace_root = value
        .get("workspace_root")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let pid = value
        .get("pid")
        .and_then(JsonValue::as_u64)
        .and_then(|pid| u32::try_from(pid).ok());
    let status = match pid {
        Some(pid) if pid == std::process::id() => "owned",
        Some(pid) => match process_liveness(pid) {
            MoveRootProcessLiveness::Alive => "active",
            MoveRootProcessLiveness::Dead => "stale",
            MoveRootProcessLiveness::Unknown => "unknown",
        },
        None => "unknown",
    };
    Ok(Some(ProjectMoveRootWriteOwnerMarker {
        path,
        workspace_root,
        pid,
        status,
    }))
}

fn rewrite_write_owner_marker_workspace_root(
    marker_path: &Path,
    workspace_root: &str,
) -> ExoResult<()> {
    let content = std::fs::read_to_string(marker_path).with_context(|| {
        format!(
            "Failed to read sidecar write-owner marker {}",
            marker_path.display()
        )
    })?;
    let mut value: JsonValue = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse sidecar write-owner marker {}",
            marker_path.display()
        )
    })?;
    value["workspace_root"] = JsonValue::String(workspace_root.to_string());
    std::fs::write(marker_path, serde_json::to_string_pretty(&value)?).with_context(|| {
        format!(
            "Failed to write sidecar write-owner marker {}",
            marker_path.display()
        )
    })
}

fn sidecar_write_owner_marker_path(sidecar_root: &Path, key: &str) -> PathBuf {
    let mut sanitized = String::new();
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }
    let sanitized = sanitized.trim_matches('-');
    let prefix = if sanitized.is_empty() {
        "sidecar"
    } else {
        sanitized
    };
    let digest = blake3::hash(key.as_bytes()).to_hex().to_string();
    sidecar_root
        .join(".git/exo-write-owners")
        .join(format!("{prefix}-{}.json", &digest[..12]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveRootProcessLiveness {
    Alive,
    Dead,
    Unknown,
}

#[cfg(target_os = "linux")]
fn process_is_defunct(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(close_paren) = stat.rfind(')') else {
        return false;
    };
    stat[close_paren + 1..]
        .split_whitespace()
        .next()
        .is_some_and(|state| matches!(state, "Z" | "X" | "x"))
}

#[cfg(target_os = "macos")]
fn process_is_defunct(pid: u32) -> bool {
    let Ok(output) = ProcessCommand::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .is_some_and(|state| state.contains('Z'))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_is_defunct(_pid: u32) -> bool {
    false
}

fn process_liveness(pid: u32) -> MoveRootProcessLiveness {
    #[cfg(unix)]
    {
        if pid == 0 {
            return MoveRootProcessLiveness::Unknown;
        }
        if process_is_defunct(pid) {
            return MoveRootProcessLiveness::Dead;
        }
        let pid = match i32::try_from(pid) {
            Ok(pid) => pid,
            Err(_) => return MoveRootProcessLiveness::Unknown,
        };
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) => MoveRootProcessLiveness::Alive,
            Err(nix::errno::Errno::ESRCH) => MoveRootProcessLiveness::Dead,
            Err(nix::errno::Errno::EPERM) => MoveRootProcessLiveness::Alive,
            Err(_) => MoveRootProcessLiveness::Unknown,
        }
    }
    #[cfg(windows)]
    {
        if pid == 0 {
            return MoveRootProcessLiveness::Unknown;
        }
        if pid == std::process::id() {
            return MoveRootProcessLiveness::Alive;
        }
        let filter = format!("PID eq {pid}");
        let output = match ProcessCommand::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
        {
            Ok(output) => output,
            Err(_) => return MoveRootProcessLiveness::Unknown,
        };
        if !output.status.success() {
            return MoveRootProcessLiveness::Unknown;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim_start().starts_with("INFO:") {
            return MoveRootProcessLiveness::Dead;
        }
        let pid_text = pid.to_string();
        if stdout.lines().any(|line| {
            line.split(',')
                .nth(1)
                .map(|field| field.trim().trim_matches('"') == pid_text)
                .unwrap_or(false)
        }) {
            MoveRootProcessLiveness::Alive
        } else {
            MoveRootProcessLiveness::Dead
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        if pid == std::process::id() {
            MoveRootProcessLiveness::Alive
        } else {
            MoveRootProcessLiveness::Unknown
        }
    }
}

// ============================================================================
// project repair
// ============================================================================

/// Preview or apply project policy repairs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectRepair {
    stale_sidecars: bool,
    apply: bool,
}

impl ProjectRepair {
    pub const fn new(stale_sidecars: bool, apply: bool) -> Self {
        Self {
            stale_sidecars,
            apply,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProjectRepairPreviewOutput {
    kind: &'static str,
    ok: bool,
    preview: bool,
    config_path: PathBuf,
    stale_sidecars: Vec<crate::project::StaleSidecarPolicyEntry>,
    note: &'static str,
}

#[derive(Debug, Serialize)]
struct ProjectRepairApplyOutput {
    kind: &'static str,
    ok: bool,
    preview: bool,
    config_path: PathBuf,
    removed: Vec<crate::project::StaleSidecarPolicyEntry>,
}

impl From<ProjectPolicyRepairPlan> for ProjectRepairPreviewOutput {
    fn from(plan: ProjectPolicyRepairPlan) -> Self {
        let note = if plan.stale_sidecars.is_empty() {
            "No stale local-policy sidecar entries were found."
        } else {
            "Run `exo project repair-apply --stale-sidecars` after reviewing this preview."
        };
        Self {
            kind: "project.repair.preview",
            ok: true,
            preview: true,
            config_path: plan.config_path,
            stale_sidecars: plan.stale_sidecars,
            note,
        }
    }
}

impl From<ProjectPolicyRepairApply> for ProjectRepairApplyOutput {
    fn from(applied: ProjectPolicyRepairApply) -> Self {
        Self {
            kind: "project.repair.apply",
            ok: true,
            preview: false,
            config_path: applied.config_path,
            removed: applied.removed,
        }
    }
}

impl Command for ProjectRepair {
    fn namespace(&self) -> &'static str {
        "project"
    }

    fn operation(&self) -> &'static str {
        if self.apply { "repair-apply" } else { "repair" }
    }

    fn description(&self) -> &'static str {
        if self.apply {
            "Apply project policy repairs after reviewing the preview"
        } else {
            "Preview project policy repairs"
        }
    }

    fn effect(&self) -> Effect {
        if self.apply {
            Effect::Exec
        } else {
            Effect::Pure
        }
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        if self.apply {
            unreachable!("ProjectRepair --apply should be dispatched via execute_mut");
        }
        if !self.stale_sidecars {
            return Err(anyhow!(
                "project repair currently requires --stale-sidecars"
            ));
        }

        let output = ProjectRepairPreviewOutput::from(
            project_resolver_for_context(ctx.project).preview_stale_sidecar_policy_repair()?,
        );

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = if output.stale_sidecars.is_empty() {
                    "No stale local-policy sidecar entries were found.".to_string()
                } else {
                    format!(
                        "{} stale local-policy sidecar entries would be removed from {}.\nRun `exo project repair-apply --stale-sidecars` after reviewing this preview.",
                        output.stale_sidecars.len(),
                        output.config_path.display()
                    )
                };
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

impl MutableCommand for ProjectRepair {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        if !self.apply {
            unreachable!("ProjectRepair preview should be dispatched via execute");
        }
        if !self.stale_sidecars {
            return Err(anyhow!(
                "project repair currently requires --stale-sidecars"
            ));
        }

        let output = ProjectRepairApplyOutput::from(
            project_resolver_for_context(ctx.project).apply_stale_sidecar_policy_repair()?,
        );

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Applied stale project sidecar policy repair.",
            )),
        }
    }
}

fn resolve_project(root: &std::path::Path) -> ExoResult<Project> {
    Project::resolve(root).map_err(|err| {
        let message = err.to_string();
        if message.contains("requires a git repository") {
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                message,
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Initialize git".to_string(),
                    command: "git init".to_string(),
                    rationale: "exo projects require git; initialize this directory first."
                        .to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(1.0),
                }]),
            )
            .into()
        } else {
            err
        }
    })
}
