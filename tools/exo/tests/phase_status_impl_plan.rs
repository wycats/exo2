//! Integration tests for `exo phase status` implementation-plan output.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use predicates::prelude::*;
use test_case::test_matrix;
use test_support::{
    exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage,
    write_implementation_plan,
};

#[test_matrix(["sqlite"])]
fn test_phase_status_shows_implementation_plan(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add a goal via CLI (works on both backends)
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "My Custom Step", "--id", "custom-step"])
        .assert()
        .success();

    // Also write implementation-plan.toml for TOML backend (adds verification metadata)
    if backend == "toml" {
        let impl_plan = r#"[phase]
id = "phase-1"
title = "Phase 1"

[[plan.goals]]
name = "custom-step"

[verification]
automated = ["Run scripts/verify-phase.sh"]
"#;
        write_implementation_plan(root, impl_plan);
    }

    // Core semantic: phase status shows the goal
    exo_cmd_with_storage(root, backend)
        .args(["phase", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("My Custom Step"));

    // TOML-specific: verification metadata from implementation-plan.toml
    if backend == "toml" {
        exo_cmd_with_storage(root, backend)
            .args(["phase", "status"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Run scripts/verify-phase.sh"));
    }
}

#[test_matrix(["sqlite"])]
fn test_phase_status_shows_done_display_for_pending_goal_with_completed_tasks(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal Awaiting Log", "--id", "goal-await-log"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Task complete",
            "--id",
            "task-1",
            "--goal",
            "goal-await-log",
        ])
        .assert()
        .success();

    // Completion guard requires a claim
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "goal-await-log::task-1",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "goal-await-log::task-1",
            "--log",
            "Implemented",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from phase status");

    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");

    let goal = goals
        .iter()
        .find(|g| g.get("name").and_then(|v| v.as_str()) == Some("Goal Awaiting Log"))
        .expect("goal row");

    assert_eq!(goal.get("status").and_then(|v| v.as_str()), Some("pending"));
    assert_eq!(
        goal.get("display_status").and_then(|v| v.as_str()),
        Some("done?")
    );
}
