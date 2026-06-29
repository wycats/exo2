//! Integration tests for `exo goal abandon --log`.

mod test_support;

use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage};

#[test_matrix(["sqlite"])]
fn goal_abandon_sets_status_and_log(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Test goal for abandonment",
            "--id",
            "goal-abandon-test",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "abandon",
            "goal-abandon-test",
            "--log",
            "No longer relevant",
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
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-abandon-test"))
        .expect("goal-abandon-test entry");

    assert_eq!(
        goal.get("status").and_then(|v| v.as_str()),
        Some("abandoned")
    );
    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("No longer relevant")
    );
}

#[test_matrix(["sqlite"])]
fn goal_abandon_works_with_pending_tasks(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal with pending tasks",
            "--id",
            "goal-with-pending",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Pending task",
            "--id",
            "task-1",
            "--goal",
            "goal-with-pending",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "abandon",
            "goal-with-pending",
            "--log",
            "Deprioritized",
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
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-with-pending"))
        .expect("goal-with-pending entry");

    assert_eq!(
        goal.get("status").and_then(|v| v.as_str()),
        Some("abandoned")
    );
    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Deprioritized")
    );
}

#[test_matrix(["sqlite"])]
fn goal_list_human_output_distinguishes_abandoned_goals(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal that remains pending",
            "--id",
            "pending-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal that gets abandoned",
            "--id",
            "abandoned-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args(["goal", "abandon", "abandoned-goal", "--log", "Not pursuing"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("stdout utf8");

    let pending_line = stdout
        .lines()
        .find(|line| line.contains("pending-goal"))
        .expect("pending-goal row");
    let abandoned_line = stdout
        .lines()
        .find(|line| line.contains("abandoned-goal"))
        .expect("abandoned-goal row");

    assert!(pending_line.contains("| pending |"));
    assert!(abandoned_line.contains("| abandoned |"));
}
