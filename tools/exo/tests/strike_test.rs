//! Integration tests for `exo strike`.

#[macro_use]
mod test_support;

use predicates::prelude::*;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage};

fn setup_context(temp: &tempfile::TempDir, backend: &str) {
    let root = temp.path();
    exo_init_with_storage(root, backend);
    let impl_path = root.join("docs/agent-context/current/implementation-plan.toml");
    let _ = std::fs::remove_file(&impl_path);
}

#[test_matrix(["sqlite"])]
fn test_strike_workflow(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    setup_context(&temp, backend);

    // 1. Start Strike
    let output = exo_cmd_with_storage(temp.path(), backend)
        .args([
            "--format",
            "json",
            "strike",
            "start",
            "--name",
            "Test Strike",
            "--goal",
            "Fix the thing",
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
        Some("strike.start")
    );
    let strike_goal_id = json
        .get("result")
        .and_then(|r| r.get("strike_id"))
        .and_then(|v| v.as_str())
        .expect("strike id");

    // 2. Verify Status shows Strike
    exo_cmd_with_storage(temp.path(), backend)
        .args(["phase", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SURGICAL STRIKE: Test Strike"))
        .stdout(predicate::str::contains("Goal: Fix the thing"));

    // 3. Add Task to Strike (should work without manual workaround now)
    exo_cmd_with_storage(temp.path(), backend)
        .args([
            "task",
            "add",
            "Do work",
            "--id",
            "task-1",
            "--goal",
            strike_goal_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added task under goal"));

    // 4. Claim and complete Task
    exo_cmd_with_storage(temp.path(), backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "task-1",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(temp.path(), backend)
        .args(["task", "complete", "--log", "Completed the work", "task-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("marked as completed"));

    // 5. Finish Strike
    exo_cmd_with_storage(temp.path(), backend)
        .args(["strike", "finish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Finished surgical strike"));

    // 6. Verify Status reverts to Main Phase
    exo_cmd_with_storage(temp.path(), backend)
        .args(["phase", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase Status: Bootstrap"))
        .stdout(predicate::str::contains("SURGICAL STRIKE").not());
}

#[test_matrix(["sqlite"])]
fn test_strike_abort(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    setup_context(&temp, backend);

    // 1. Start Strike
    exo_cmd_with_storage(temp.path(), backend)
        .args([
            "strike",
            "start",
            "--name",
            "Abort Me",
            "--goal",
            "To be aborted",
        ])
        .assert()
        .success();

    // 2. Abort Strike
    exo_cmd_with_storage(temp.path(), backend)
        .args(["strike", "abort"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Aborted surgical strike"));

    // 3. Verify Status
    exo_cmd_with_storage(temp.path(), backend)
        .args(["phase", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SURGICAL STRIKE").not());
}
