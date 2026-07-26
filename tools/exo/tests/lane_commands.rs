#![allow(clippy::disallowed_methods)]

use exo::command::lane::{
    LaneCreate, LaneCurrent, LaneFocus, LaneList, LaneRemove, LaneShow, LaneStart,
};
use exo::command::phase_cmd::{PhaseFocus, PhaseRemove, PhaseStart};
use exo::command::{Command, CommandContext, MutableCommand, MutableCommandContext, OutputFormat};
use exo::context::{SqliteWriter, db_path};
use exo::project::Project;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

fn git_init(root: &Path) {
    let status = ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

fn fixture() -> (tempfile::TempDir, Project, String, String) {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    let project = Project::resolve(root).expect("resolve fixture project");
    fs::create_dir_all(
        project
            .db_path()
            .parent()
            .expect("project database has a parent"),
    )
    .expect("create state root");
    let writer = SqliteWriter::open(project.db_path()).expect("open writer");
    let epoch_id = writer
        .add_epoch("Lane Test Epoch", None, &[])
        .expect("add epoch");
    let bootstrap_phase = writer
        .add_phase(&epoch_id, "Bootstrap", "regular", None, &[])
        .expect("add bootstrap phase");
    let execution_phase = writer
        .add_phase(&epoch_id, "Lane Execution", "regular", None, &[])
        .expect("add execution phase");
    writer
        .update_phase_status(&bootstrap_phase, "completed")
        .expect("complete bootstrap phase");

    (temp, project, bootstrap_phase, execution_phase)
}

fn read_context<'a>(root: &'a Path, project: &'a Project) -> CommandContext<'a> {
    CommandContext {
        root,
        project: Some(project),
        format: OutputFormat::Json,
        agent_id: None,
        workflow_confirmation: None,
    }
}

fn write_context<'a>(root: &'a Path, project: &'a Project) -> MutableCommandContext<'a> {
    MutableCommandContext {
        root,
        project: Some(project),
        format: OutputFormat::Json,
        agent_id: None,
        workflow_confirmation: None,
    }
}

fn execute<C: Command>(command: &C, root: &Path, project: &Project) -> Value {
    command
        .execute(&read_context(root, project))
        .expect("command succeeds")
        .data
}

fn execute_mut<C: MutableCommand>(command: &C, root: &Path, project: &Project) -> Value {
    command
        .execute_mut(&mut write_context(root, project))
        .expect("mutable command succeeds")
        .data
}

#[test]
fn lane_commands_cover_preparation_execution_focus_and_removal() {
    let (temp, project, _bootstrap_phase, execution_phase) = fixture();
    let root = temp.path();
    let writer = SqliteWriter::open(db_path(root, Some(&project))).expect("open writer");

    let empty_current = execute(&LaneCurrent, root, &project);
    assert!(empty_current["lane"].is_null());
    assert_eq!(empty_current["diagnostics"], serde_json::json!([]));

    let created = execute_mut(
        &LaneCreate::new(
            "First proof",
            "Deliver the command surface",
            &execution_phase,
        ),
        root,
        &project,
    );
    let lane_id = created["lane"]["id"].as_str().expect("lane id").to_string();
    assert_eq!(created["lane"]["state"], "prepared");
    assert_eq!(created["lane"]["phase_status"], "pending");

    let before_start = LaneFocus::new(&lane_id)
        .execute_mut(&mut write_context(root, &project))
        .expect_err("pending phase cannot be focused");
    let failure = before_start
        .downcast_ref::<exo::failure::ExoFailure>()
        .expect("structured failure");
    assert_eq!(
        failure.error.code,
        exo::api::protocol::ErrorCode::PreconditionFailed
    );
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "lane.phase_not_in_progress"
    );

    writer
        .update_phase_status(&execution_phase, "in-progress")
        .expect("start execution phase");
    writer
        .add_goal(
            &execution_phase,
            "command-proof",
            "Prove lane commands",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add goal");

    let started = execute_mut(&LaneStart::new(&lane_id), root, &project);
    assert_eq!(started["lane"]["state"], "executing");
    assert_eq!(started["lane"]["phase_status"], "in-progress");
    assert_eq!(started["lane"]["focused_here"], true);

    let repeated_start = LaneStart::new(&lane_id)
        .execute_mut(&mut write_context(root, &project))
        .expect_err("executing lane cannot start again");
    let failure = repeated_start
        .downcast_ref::<exo::failure::ExoFailure>()
        .expect("structured failure");
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "lane.not_prepared"
    );

    let listed = execute(&LaneList, root, &project);
    assert_eq!(listed["lanes"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["lanes"][0]["id"], lane_id);
    assert_eq!(listed["diagnostics"], serde_json::json!([]));

    let shown = execute(&LaneShow::new(&lane_id), root, &project);
    assert_eq!(shown["lane"]["goals"][0]["id"], "command-proof");
    assert_eq!(shown["lane"]["goals"][0]["title"], "Prove lane commands");

    let current = execute(&LaneCurrent, root, &project);
    assert_eq!(current["lane"]["id"], lane_id);
    assert_eq!(current["diagnostics"], serde_json::json!([]));
    assert!(
        !current.to_string().contains(&root.display().to_string()),
        "lane output must not expose workspace roots"
    );

    let executing_remove = LaneRemove::new(&lane_id)
        .execute_mut(&mut write_context(root, &project))
        .expect_err("executing lane cannot be removed");
    let failure = executing_remove
        .downcast_ref::<exo::failure::ExoFailure>()
        .expect("structured failure");
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "lane.not_prepared"
    );

    let prepared = execute_mut(
        &LaneCreate::new("Disposable", "Prove prepared removal", &execution_phase),
        root,
        &project,
    );
    let prepared_id = prepared["lane"]["id"]
        .as_str()
        .expect("prepared lane id")
        .to_string();
    execute_mut(&LaneFocus::new(&prepared_id), root, &project);
    let removed = execute_mut(&LaneRemove::new(&prepared_id), root, &project);
    assert_eq!(removed["id"], prepared_id);
    assert!(execute(&LaneCurrent, root, &project)["lane"].is_null());
}

#[test]
fn lane_current_reports_phase_focus_mismatch_without_repairing_it() {
    let (temp, project, bootstrap_phase, execution_phase) = fixture();
    let root = temp.path();
    let writer = SqliteWriter::open(project.db_path()).expect("open writer");
    writer
        .update_phase_status(&execution_phase, "in-progress")
        .expect("start execution phase");

    let created = execute_mut(
        &LaneCreate::new("Focused lane", "Keep reads non-mutating", &execution_phase),
        root,
        &project,
    );
    let lane_id = created["lane"]["id"].as_str().expect("lane id").to_string();
    execute_mut(&LaneFocus::new(&lane_id), root, &project);

    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    writer
        .set_workspace_active_phase(&workspace_root, &bootstrap_phase)
        .expect("create mismatch");

    let mismatched = execute(&LaneCurrent, root, &project);
    assert_eq!(mismatched["lane"]["id"], lane_id);
    assert_eq!(
        mismatched["diagnostics"][0]["code"],
        "lane.phase_focus_mismatch"
    );
    assert_eq!(
        mismatched["diagnostics"][0]["focused_phase_id"],
        bootstrap_phase
    );

    let still_mismatched = execute(&LaneCurrent, root, &project);
    assert_eq!(
        still_mismatched["diagnostics"][0]["code"], "lane.phase_focus_mismatch",
        "pure reads must not silently repair phase focus"
    );
}

#[test]
fn phase_commands_preserve_or_clear_lane_focus_by_execution_phase() {
    let (temp, project, _bootstrap_phase, execution_phase) = fixture();
    let root = temp.path();
    let writer = SqliteWriter::open(project.db_path()).expect("open writer");
    let epoch_id = writer
        .database()
        .connection()
        .query_row(
            "SELECT text_id FROM epochs_data WHERE title = 'Lane Test Epoch'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("load epoch id");
    let later_phase = writer
        .add_phase(&epoch_id, "Later Lane Execution", "regular", None, &[])
        .expect("add later phase");
    let execution_lane = writer
        .add_workbench_lane(
            "Execution lane",
            "Preserve matching focus",
            &execution_phase,
        )
        .expect("add execution lane");
    let later_lane = writer
        .add_workbench_lane("Later lane", "Preserve start focus", &later_phase)
        .expect("add later lane");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();

    writer
        .update_phase_status(&execution_phase, "in-progress")
        .expect("start execution phase");
    writer
        .focus_workbench_lane(&workspace_root, &execution_lane, &execution_phase)
        .expect("focus execution lane");

    execute_mut(&PhaseFocus::new(&execution_phase), root, &project);
    assert_eq!(
        exo::context::SqliteLoader::open(project.db_path())
            .expect("open loader")
            .load_workspace_lane_focus(&workspace_root)
            .expect("load focus")
            .map(|focus| focus.lane_id),
        Some(execution_lane),
        "focusing the matching in-progress phase preserves lane focus"
    );

    execute_mut(&PhaseFocus::new(&later_phase), root, &project);
    assert_eq!(
        exo::context::SqliteLoader::open(project.db_path())
            .expect("open loader")
            .load_workspace_lane_focus(&workspace_root)
            .expect("load focus"),
        None,
        "focusing a pending phase clears lane focus"
    );

    writer
        .set_workspace_lane_focus(&workspace_root, &later_lane)
        .expect("seed matching prepared lane focus");
    execute_mut(
        &PhaseStart::new(Some(later_phase.clone()), false),
        root,
        &project,
    );
    let loader = exo::context::SqliteLoader::open(project.db_path()).expect("open loader");
    assert_eq!(
        loader
            .load_workspace_lane_focus(&workspace_root)
            .expect("load focus")
            .map(|focus| focus.lane_id),
        Some(later_lane),
        "starting a lane's phase preserves its existing focus"
    );
    assert_eq!(
        loader
            .load_workspace_active_phase(&workspace_root)
            .expect("load phase focus"),
        Some(later_phase)
    );
}

#[test]
fn phase_remove_reports_the_lanes_that_restrict_deletion() {
    let (temp, project, _bootstrap_phase, execution_phase) = fixture();
    let root = temp.path();
    let writer = SqliteWriter::open(project.db_path()).expect("open writer");
    let lane_id = writer
        .add_workbench_lane("Durable lane", "Keep the phase reachable", &execution_phase)
        .expect("add lane");

    let error = PhaseRemove::new(&execution_phase)
        .execute_mut(&mut write_context(root, &project))
        .expect_err("phase with a lane cannot be removed");
    let failure = error
        .downcast_ref::<exo::failure::ExoFailure>()
        .expect("structured failure");
    assert_eq!(
        failure.error.code,
        exo::api::protocol::ErrorCode::PreconditionFailed
    );
    let details = failure.error.details.as_ref().expect("details");
    assert_eq!(details["kind"], "phase.has_workbench_lanes");
    assert_eq!(details["phase_id"], execution_phase);
    assert_eq!(details["lane_ids"], serde_json::json!([lane_id]));
}
