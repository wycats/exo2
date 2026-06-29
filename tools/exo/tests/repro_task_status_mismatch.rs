//! Repro test for implementation task status mismatch.
#[macro_use]
mod test_support;
use serde_json::Value as JsonValue;
use test_case::test_matrix;
use test_support::{
    exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_task_with_storage, write_implementation_plan,
};

#[test_matrix(["sqlite"])]
fn repro_task_status_mismatch(backend: &str) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    // 1. Setup plan with a goal and task
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    exo_plan_add_task_with_storage(root, backend, &phase_id, "goal-1", "Goal 1");

    // Add task under goal via CLI (works on both backends)
    exo_cmd_with_storage(root, backend)
        .args([
            "task", "add", "Task 1", "--id", "task-1", "--goal", "goal-1",
        ])
        .assert()
        .success();

    // On TOML, also write implementation-plan.toml with satisfies links
    if backend == "toml" {
        write_implementation_plan(
            root,
            &format!(
                r#"[phase]
id = "{phase_id}"
title = "Phase 1"

[[plan.goals]]
name = "goal-1"
type = "feat"
details = "Goal 1 details"
satisfies = ["goal-1"]
status = "pending"

[[plan.goals.tasks]]
id = "task-1"
title = "Task 1"
status = "pending"
"#,
            ),
        );
    }

    // 3. Mark task as completed

    // Let's try to manually update plan.toml to completed, as if the user did it.
    // Or use `exo task complete` if it works.

    // Completion guard requires a claim
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "goal-1::task-1",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "--log",
            "Completed task-1",
            "goal-1::task-1",
        ])
        .assert()
        .success();

    // 4. Check `exo task list` -> Should be completed
    let output = exo_cmd_with_storage(root, backend)
        .args(["task", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: JsonValue = serde_json::from_slice(&output).unwrap();
    println!("exo task list output: {}", json);
    let tasks = json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .unwrap();
    let task = tasks.iter().find(|t| t["id"] == "goal-1::task-1").unwrap();
    assert_eq!(
        task["status"], "completed",
        "exo task list should show completed from plan.toml"
    );

    // 5. Check `exo phase status` -> Should be completed
    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: JsonValue = serde_json::from_slice(&output).unwrap();
    let tasks = json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .unwrap();
    let task = tasks.iter().find(|t| t["id"] == "goal-1::task-1").unwrap();

    assert_eq!(
        task["status"], "completed",
        "exo phase status should show completed"
    );
}
