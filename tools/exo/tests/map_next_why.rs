#![allow(clippy::disallowed_methods)] // integration tests use real git processes

//! Integration tests for `exo map --next/--why`.

#[macro_use]
mod test_support;

use predicates::prelude::*;
use std::process::Command;
use test_case::test_matrix;
use test_support::{
    exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_task_with_storage,
};

fn setup_minimal_context(backend: &str) -> Option<tempfile::TempDir> {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return None;
    };
    let root = temp.path();

    // Initialize a git repo in the temp directory so that git status doesn't
    // walk up and find the parent exo2 repo (which may have uncommitted changes,
    // triggering world_needs_repair() and emitting a repair action instead of
    // the expected task-based action).
    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("failed to git init");
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(root)
        .output()
        .expect("failed to set git user.email");
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(root)
        .output()
        .expect("failed to set git user.name");

    exo_init_with_storage(root, backend);
    // init bootstraps a "Getting Started" epoch with active "Bootstrap" phase.
    let phase_id = exo_active_phase_id(root);
    exo_plan_add_task_with_storage(root, backend, &phase_id, "task-1", "Do the thing");

    // NOTE: We intentionally do NOT create task-list.toml or walkthrough.toml here
    // because those are deprecated files that would trigger the upgrade gate.

    // Commit all files so git status reports a clean state
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .output()
        .expect("failed to git add");
    Command::new("git")
        .args(["commit", "-m", "Initial test context"])
        .current_dir(root)
        .output()
        .expect("failed to git commit");

    Some(temp)
}

#[test_matrix(["sqlite"])]
fn map_next_emits_single_action_json(backend: &str) {
    let temp_opt = setup_minimal_context(backend);
    assert!(temp_opt.is_some(), "failed to set up minimal context");
    let Some(temp) = temp_opt else {
        return;
    };
    let root = temp.path();

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.arg("map")
        .arg("--next")
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\""))
        // The steering may return various next actions depending on state
        // (task complete, phase status, etc.) - just verify structure
        .stdout(predicate::str::contains("\"intent\""))
        .stdout(predicate::str::contains("\"rationale\""));
}

#[test_matrix(["sqlite"])]
fn map_why_emits_explanation_json(backend: &str) {
    let temp_opt = setup_minimal_context(backend);
    assert!(temp_opt.is_some(), "failed to set up minimal context");
    let Some(temp) = temp_opt else {
        return;
    };
    let root = temp.path();

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.arg("map")
        .arg("--why")
        .arg("exo phase finish")
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"preconditions\""))
        .stdout(predicate::str::contains("\"effects\""))
        .stdout(predicate::str::contains("\"suggested\""));
}
