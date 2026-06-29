//! Integration tests for `exo task add --goal` (nested task creation).

#[macro_use]
mod test_support;

use predicates::str::contains;
use test_case::test_matrix;
use test_support::{
    exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage,
    exo_plan_add_phase, exo_plan_update_status_with_storage,
};

fn task_list_json(root: &std::path::Path, backend: &str) -> serde_json::Value {
    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    serde_json::from_slice(&output).expect("valid task list json")
}

fn task_entries(json: &serde_json::Value) -> &Vec<serde_json::Value> {
    json.get("result")
        .and_then(|result| result.get("tasks"))
        .and_then(|tasks| tasks.as_array())
        .expect("tasks array")
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_creates_nested_task_under_goal(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Do the thing",
            "--id",
            "task-1",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();

    let phase_tasks_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "read-tasks"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let phase_tasks_json: serde_json::Value =
        serde_json::from_slice(&phase_tasks_output).expect("valid phase task json");
    let phase_tasks = phase_tasks_json["result"]
        .as_array()
        .expect("phase task array");
    let phase_task = phase_tasks
        .iter()
        .find(|task| task.get("id").and_then(|value| value.as_str()) == Some("goal-1::task-1"))
        .expect("task-1 in phase read-tasks");
    assert_eq!(
        phase_task.get("taskId").and_then(|value| value.as_str()),
        Some("task-1")
    );
    assert_eq!(
        phase_task.get("goalId").and_then(|value| value.as_str()),
        Some("goal-1")
    );
    assert_eq!(
        phase_task.get("goalTitle").and_then(|value| value.as_str()),
        Some("Goal 1")
    );
    assert_eq!(
        phase_task.get("status").and_then(|value| value.as_str()),
        Some("todo")
    );

    let phase_goals_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "read-goals"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let phase_goals_json: serde_json::Value =
        serde_json::from_slice(&phase_goals_output).expect("valid phase goal json");
    let phase_goals = phase_goals_json["result"]
        .as_array()
        .expect("phase goal array");
    let phase_goal = phase_goals
        .iter()
        .find(|goal| goal.get("id").and_then(|value| value.as_str()) == Some("goal-1"))
        .expect("goal-1 in phase read-goals");
    assert_eq!(
        phase_goal.get("status").and_then(|value| value.as_str()),
        Some("todo")
    );

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let tasks = json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    let task = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("goal-1::task-1"))
        .expect("goal-1::task-1 entry");

    assert_eq!(
        task.get("label").and_then(|v| v.as_str()),
        Some("Do the thing")
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_normalizes_redundant_composite_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    let add_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Do the thing",
            "--id",
            "goal-1::task-1",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let add_json: serde_json::Value =
        serde_json::from_slice(&add_output).expect("valid task add json");
    assert_eq!(add_json["status"], "ok", "{add_json}");
    assert_eq!(add_json["result"]["task_id"], "task-1", "{add_json}");
    assert_eq!(add_json["result"]["goal_id"], "goal-1", "{add_json}");

    let list_json = task_list_json(root, backend);
    let tasks = task_entries(&list_json);
    assert!(
        tasks
            .iter()
            .any(
                |task| task.get("id").and_then(|value| value.as_str()) == Some("goal-1::task-1")
                    && task.get("label").and_then(|value| value.as_str()) == Some("Do the thing")
            ),
        "expected normalized goal task display id in {list_json}"
    );
    assert!(
        !tasks
            .iter()
            .any(|task| task.get("id").and_then(|value| value.as_str())
                == Some("goal-1::goal-1::task-1")),
        "must not double-prefix task id: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_alias_normalizes_alias_prefixed_composite_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let phase_id = exo_active_phase_id(root);
    let writer = exo::context::SqliteWriter::open(&exo::context::db_path(root, None))
        .expect("open sqlite writer");
    writer
        .add_goal(
            &phase_id,
            "canonical-goal",
            "Canonical Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &["goal-alias".to_string()],
        )
        .expect("add aliased goal");
    drop(writer);

    let add_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Alias task",
            "--id",
            "goal-alias::task-1",
            "--goal",
            "goal-alias",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let add_json: serde_json::Value =
        serde_json::from_slice(&add_output).expect("valid task add json");
    assert_eq!(add_json["result"]["task_id"], "task-1", "{add_json}");
    assert_eq!(
        add_json["result"]["goal_id"], "canonical-goal",
        "{add_json}"
    );

    let list_json = task_list_json(root, backend);
    let tasks = task_entries(&list_json);
    assert!(
        tasks.iter().any(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("canonical-goal::task-1")
                && task.get("label").and_then(|value| value.as_str()) == Some("Alias task")
        }),
        "expected alias-prefixed id normalized under canonical goal: {list_json}"
    );
    assert!(
        !tasks.iter().any(|task| {
            task.get("id").and_then(|value| value.as_str())
                == Some("canonical-goal::goal-alias::task-1")
        }),
        "must not store alias-prefixed id as nested task id: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_canonical_goal_normalizes_alias_prefixed_composite_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let phase_id = exo_active_phase_id(root);
    let writer = exo::context::SqliteWriter::open(&exo::context::db_path(root, None))
        .expect("open sqlite writer");
    writer
        .add_goal(
            &phase_id,
            "canonical-goal",
            "Canonical Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &["goal-alias".to_string()],
        )
        .expect("add aliased goal");
    drop(writer);

    let add_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Canonical target alias task",
            "--id",
            "goal-alias::task-1",
            "--goal",
            "canonical-goal",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let add_json: serde_json::Value =
        serde_json::from_slice(&add_output).expect("valid task add json");
    assert_eq!(add_json["result"]["task_id"], "task-1", "{add_json}");
    assert_eq!(
        add_json["result"]["goal_id"], "canonical-goal",
        "{add_json}"
    );

    let list_json = task_list_json(root, backend);
    assert!(
        task_entries(&list_json).iter().any(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("canonical-goal::task-1")
                && task.get("label").and_then(|value| value.as_str())
                    == Some("Canonical target alias task")
        }),
        "expected alias-prefixed id accepted for canonical goal target: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_rejects_mismatched_composite_prefix(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 2", "--id", "goal-2"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Wrong goal",
            "--id",
            "goal-2::task-1",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid error json");
    let message = json["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Task id prefix 'goal-2' resolves to goal goal-2"),
        "{json}"
    );
    assert!(
        message.contains("--goal goal-1 resolves to goal-1"),
        "{json}"
    );
    assert!(
        message.contains("Use `--goal goal-2` or `--id task-1`"),
        "{json}"
    );

    let list_json = task_list_json(root, backend);
    let tasks = task_entries(&list_json);
    assert!(
        tasks.is_empty(),
        "mismatched prefix must not insert a task: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_allows_scoped_task_id_containing_double_colon(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    let add_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Scoped task",
            "--id",
            "parent::child",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let add_json: serde_json::Value =
        serde_json::from_slice(&add_output).expect("valid task add json");
    assert_eq!(add_json["result"]["task_id"], "parent::child", "{add_json}");
    assert_eq!(add_json["result"]["goal_id"], "goal-1", "{add_json}");

    let list_json = task_list_json(root, backend);
    assert!(
        task_entries(&list_json).iter().any(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("goal-1::parent::child")
                && task.get("label").and_then(|value| value.as_str()) == Some("Scoped task")
        }),
        "expected scoped task id preserved under selected goal: {list_json}"
    );

    exo_cmd_with_storage(root, backend)
        .args(["task", "start", "goal-1::parent::child"])
        .assert()
        .success();
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_rejects_empty_composite_prefix(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Empty prefix",
            "--id",
            "::task-1",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid error json");
    let message = json["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("has an empty goal prefix"), "{json}");
    assert!(
        message.contains("Use `--id task-1` with `--goal goal-1`"),
        "{json}"
    );

    let list_json = task_list_json(root, backend);
    assert!(
        task_entries(&list_json).is_empty(),
        "empty prefix must not insert a task: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_with_goal_rejects_empty_composite_suffix(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Empty suffix",
            "--id",
            "goal-1::",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid error json");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("has an empty task component"),
        "{json}"
    );

    let list_json = task_list_json(root, backend);
    assert!(
        task_entries(&list_json).is_empty(),
        "empty suffix must not insert a task: {list_json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_under_goal_suggests_composite_start_id_for_nested_task_ids(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Nested task",
            "--id",
            "goal-1::parent::child",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("exo task start goal-1::parent::child"),
        "human output should suggest the composite display id: {text}"
    );

    exo_cmd_with_storage(root, backend)
        .args(["task", "start", "goal-1::parent::child"])
        .assert()
        .success();
}

#[test_matrix(["sqlite"])]
fn task_start_accepts_composite_id_shown_by_task_list(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Do the thing",
            "--id",
            "task-1",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args(["task", "start", "goal-1::task-1"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let tasks = json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    let task = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("goal-1::task-1"))
        .expect("goal-1::task-1 entry");

    assert_eq!(
        task.get("status").and_then(|v| v.as_str()),
        Some("in-progress")
    );
}

#[test_matrix(["sqlite"])]
fn task_add_can_target_unique_pending_phase_goal(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let epoch_id = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let epoch_json: serde_json::Value = serde_json::from_slice(&epoch_id).expect("valid json");
    let epoch_id = epoch_json["result"]["epochs"][0]["id"]
        .as_str()
        .expect("epoch id");

    let pending_phase_id = exo_plan_add_phase(root, epoch_id, "Future Phase", None, None);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Future Goal",
            "--id",
            "future-goal",
            "--phase",
            &pending_phase_id,
        ])
        .assert()
        .success();

    let task_add_output = exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Future Task",
            "--id",
            "future-task",
            "--goal",
            "future-goal",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task_add_text = String::from_utf8_lossy(&task_add_output);
    assert!(
        !task_add_text.contains("exo task start future-task"),
        "future phase task add must not suggest starting the task before the phase starts: {task_add_text}"
    );
    assert!(
        task_add_text.contains(&format!("exo phase read-tasks {pending_phase_id}")),
        "future phase task add should point back to future phase planning: {task_add_text}"
    );

    for args in [
        vec!["task", "start", "future-task"],
        vec!["task", "log", "future-task", "--message", "not yet"],
        vec!["task", "complete", "future-task", "--log", "not yet"],
        vec!["task", "update", "future-task", "--title", "Not Yet"],
        vec!["task", "reorder", "future-task", "top"],
        vec!["task", "remove", "future-task"],
        vec!["task", "start", "future-goal::future-task"],
    ] {
        exo_cmd_with_storage(root, backend)
            .args(args)
            .assert()
            .failure()
            .stderr(contains("cannot be changed by task lifecycle commands"));
    }

    exo_cmd_with_storage(root, backend)
        .args(["task", "complete", "future-goal", "--log", "not yet"])
        .assert()
        .failure()
        .stderr(contains(
            "cannot be completed through `exo task complete` until that phase starts",
        ));

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "read-tasks", &pending_phase_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let tasks = json
        .get("result")
        .and_then(|result| result.as_array())
        .expect("phase task array");
    assert!(
        tasks.iter().any(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("future-goal::future-task")
                && task.get("taskId").and_then(|value| value.as_str()) == Some("future-task")
                && task.get("goalId").and_then(|value| value.as_str()) == Some("future-goal")
                && task.get("status").and_then(|value| value.as_str()) == Some("todo")
        }),
        "expected future phase task in {json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_goal_resolution_ignores_completed_phase_alias_conflicts(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let epoch_id = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let epoch_json: serde_json::Value = serde_json::from_slice(&epoch_id).expect("valid json");
    let epoch_id = epoch_json["result"]["epochs"][0]["id"]
        .as_str()
        .expect("epoch id");

    let completed_phase_id = exo_plan_add_phase(root, epoch_id, "Completed Phase", None, None);
    let writer = exo::context::SqliteWriter::open(&exo::context::db_path(root, None))
        .expect("open sqlite writer");
    writer
        .add_goal(
            &completed_phase_id,
            "old-goal",
            "Old Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &["shared-goal".to_string()],
        )
        .expect("add old aliased goal");
    drop(writer);
    exo_plan_update_status_with_storage(root, backend, &completed_phase_id, "completed");

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Shared Goal", "--id", "shared-goal"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Current Task",
            "--id",
            "current-task",
            "--goal",
            "shared-goal",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let tasks = json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    assert!(
        tasks.iter().any(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("shared-goal::current-task")
        }),
        "expected task under active goal despite completed-phase alias conflict: {json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_goal_resolution_rejects_closed_goals_in_open_phases(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Closed Goal", "--id", "closed-goal"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "abandon",
            "closed-goal",
            "--log",
            "Closed for regression coverage",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Should Not Attach",
            "--id",
            "should-not-attach",
            "--goal",
            "closed-goal",
        ])
        .assert()
        .failure()
        .stderr(contains("Goal 'closed-goal' not found"));
}

#[test_matrix(["sqlite"])]
fn task_add_goal_resolution_reports_live_phase_ambiguity_as_input_error(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let epoch_id = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let epoch_json: serde_json::Value = serde_json::from_slice(&epoch_id).expect("valid json");
    let epoch_id = epoch_json["result"]["epochs"][0]["id"]
        .as_str()
        .expect("epoch id");

    let pending_phase_id = exo_plan_add_phase(root, epoch_id, "Pending Phase", None, None);
    let writer = exo::context::SqliteWriter::open(&exo::context::db_path(root, None))
        .expect("open sqlite writer");
    writer
        .add_goal(
            &pending_phase_id,
            "future-shared-goal",
            "Future Shared Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &["shared-goal".to_string()],
        )
        .expect("add future aliased goal");
    drop(writer);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Shared Goal", "--id", "shared-goal"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Ambiguous Task",
            "--id",
            "ambiguous-task",
            "--goal",
            "shared-goal",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(json["status"], "error", "{json}");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ambiguous across active or pending phases"),
        "{json}"
    );
    assert_eq!(json["error"]["details"]["goal"], "shared-goal", "{json}");
}

#[test_matrix(["sqlite"])]
fn task_add_with_missing_goal_fails_with_help(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Do work",
            "--id",
            "task-2",
            "--goal",
            "missing-goal",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid error json");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Goal 'missing-goal' not found"),
        "{json}"
    );
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("exo phase read-goals <phase-id>"),
        "missing-goal error should mention phase read-goals: {json}"
    );
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("exo plan read"),
        "missing-goal error should mention plan read: {json}"
    );
}

#[test_matrix(["sqlite"])]
fn task_add_without_goal_fails_with_help(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["task", "add", "Do work", "--id", "task-3"])
        .assert()
        .failure()
        .stderr(contains("No goals in active phase"))
        .stderr(contains("exo goal add"));
}
