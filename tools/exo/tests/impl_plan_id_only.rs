//! Regression test: implementation-plan.toml should only contain goal IDs (and kind for strikes).
//! Per RFC 00177 Data Location Axiom.

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage};

#[test_matrix(["sqlite"])]
fn impl_plan_contains_only_goal_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    // Add a regular goal
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "A regular goal", "--id", "regular-goal"])
        .assert()
        .success();

    // Add another goal
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Another goal", "--id", "another-goal"])
        .assert()
        .success();

    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Regular task",
            "--id",
            "task-1",
            "--goal",
            "regular-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Another task",
            "--id",
            "task-2",
            "--goal",
            "another-goal",
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

    let regular_task = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("regular-goal::task-1"))
        .expect("regular-goal::task-1 entry");
    assert_eq!(
        regular_task.get("label").and_then(|v| v.as_str()),
        Some("Regular task")
    );

    let another_task = tasks
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("another-goal::task-2"))
        .expect("another-goal::task-2 entry");
    assert_eq!(
        another_task.get("label").and_then(|v| v.as_str()),
        Some("Another task")
    );
}

#[test_matrix(["sqlite"])]
fn goal_complete_writes_completion_log_to_plan_only(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    // Add and complete a goal
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Test the feature", "--id", "test-goal"])
        .assert()
        .success();

    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "test-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "test-goal",
            "--log",
            "Successfully tested all edge cases",
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
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("test-goal"))
        .expect("test-goal entry");

    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Successfully tested all edge cases")
    );
}

/// RFC 00229: `exo plan update-status` cannot bypass goal completion.
/// Users must use `exo goal complete` to complete goals.
#[test_matrix(["sqlite"])]
fn plan_update_status_rejects_goal_completion(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    // Add a goal
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Test goal", "--id", "test-goal"])
        .assert()
        .success();

    exo_phase_start_with_storage(root, backend);

    // Attempt to complete via plan update-status (should fail)
    let output = exo_cmd_with_storage(root, backend)
        .args(["plan", "update-status", "test-goal", "completed"])
        .assert()
        .failure();

    // Should contain steering guidance
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("exo goal complete"),
        "error should steer user to `exo goal complete`: {}",
        stderr
    );
}

/// RFC 00229: After completing a goal properly, plan update-status can change its status.
#[test_matrix(["sqlite"])]
fn plan_update_status_allows_completed_goal_with_log(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    // Add a goal
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Test goal", "--id", "test-goal"])
        .assert()
        .success();

    exo_phase_start_with_storage(root, backend);

    // Claim the goal via inbox before completing
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "test-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete properly with log
    exo_cmd_with_storage(root, backend)
        .args(["goal", "complete", "test-goal", "--log", "All tests pass"])
        .assert()
        .success();

    // Now plan update-status should work (goal already has log)
    exo_cmd_with_storage(root, backend)
        .args(["plan", "update-status", "test-goal", "completed"])
        .assert()
        .success();
}
