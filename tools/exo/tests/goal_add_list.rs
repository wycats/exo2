//! Integration tests for `exo goal add` and `exo goal list`.

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage};

#[test_matrix(["sqlite"])]
fn goal_add_creates_plan_task_in_active_phase(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    // init bootstraps an active "Bootstrap" phase — no separate start needed.

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "add",
            "Ship goal alias",
            "--id",
            "goal-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("kind"))
            .and_then(|v| v.as_str()),
        Some("goal.add")
    );

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let goals = list_json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-1"))
        .expect("goal-1 entry");
    assert_eq!(
        goal.get("label").and_then(|v| v.as_str()),
        Some("Ship goal alias")
    );
}

#[test_matrix(["sqlite"])]
fn goal_list_includes_new_goal(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    // init bootstraps an active phase — no separate start needed.

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Listable goal", "--id", "goal-2"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("kind"))
            .and_then(|v| v.as_str()),
        Some("goal.list")
    );

    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");

    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-2"))
        .expect("goal-2 entry");
    assert_eq!(
        goal.get("label").and_then(|v| v.as_str()),
        Some("Listable goal")
    );
    assert_eq!(goal.get("task_count").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(goal.get("source").and_then(|v| v.as_str()), Some("sqlite"));
}

#[test_matrix(["sqlite"])]
fn goal_list_reports_task_count_from_impl_plan(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal with tasks", "--id", "goal-4"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task", "add", "Do work", "--id", "task-1", "--goal", "goal-4",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");

    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-4"))
        .expect("goal-4 entry");
    assert_eq!(goal.get("task_count").and_then(|v| v.as_u64()), Some(1));
}

#[test_matrix(["sqlite"])]
fn goal_add_can_link_rfc_to_active_phase(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "RFC linked goal",
            "--id",
            "goal-3",
            "--rfc",
            "00177",
        ])
        .assert()
        .success();

    // Verify via goal list --format json (backend-agnostic)
    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let goals = json
        .get("result")
        .and_then(|v| v.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-3"))
        .expect("goal-3 entry");
    assert_eq!(goal.get("rfc").and_then(|v| v.as_str()), Some("00177"));
}

#[test_matrix(["sqlite"])]
fn goal_list_shows_done_when_all_tasks_are_complete(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Done Candidate", "--id", "goal-done"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Only task",
            "--id",
            "task-1",
            "--goal",
            "goal-done",
        ])
        .assert()
        .success();

    // Completion guard requires a claim before completing
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "goal-done::task-1",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "goal-done::task-1",
            "--log",
            "Implemented",
        ])
        .assert()
        .success();

    let human_output = exo_cmd_with_storage(root, backend)
        .args(["goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human_text = String::from_utf8(human_output).expect("utf8 output");
    assert!(
        human_text.contains("| goal-done | Done Candidate") && human_text.contains("| done? |"),
        "expected done? display in goal list output: {human_text}"
    );

    let json_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json_output).expect("valid json");
    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");

    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-done"))
        .expect("goal-done entry");

    assert_eq!(goal.get("status").and_then(|v| v.as_str()), Some("pending"));
    assert_eq!(
        goal.get("display_status").and_then(|v| v.as_str()),
        Some("done?")
    );
}
