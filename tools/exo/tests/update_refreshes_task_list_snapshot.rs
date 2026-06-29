//! Integration test: `exo update` does not create deprecated projection snapshots.

mod test_support;

use exo::command::update::run_update;
use exo::context::{AgentContext, ExoState};
use std::path::Path;
use test_case::test_matrix;
use test_support::fs;

fn dummy_context(root: &Path) -> AgentContext {
    let cache_dir = root.join(".cache");
    assert!(fs::create_dir_all(&cache_dir).is_ok());
    assert!(exosuit_storage::open_database(cache_dir.join("exo.db")).is_ok());

    AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState {
            meta: None,
            epochs: Vec::new(),
        },
    }
}

#[test_matrix(["sqlite"])]
fn update_does_not_create_task_list_snapshot_when_missing(_backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    assert!(
        fs::create_dir_all(root.join("docs/agent-context/current")).is_ok(),
        "failed to create docs/agent-context/current"
    );

    let write_result = fs::write(
        root.join("docs/agent-context/current/implementation-plan.toml"),
        r#"# READ-ONLY: Use 'exo' CLI to modify this file.
# Implementation Plan

[phase]
id = "phase-test"
title = "Phase Test"

[plan]

[[plan.goals]]
name = "Change A"
type = "feat"
details = "Demo change"
files = []
tests = []

[[plan.goals.tasks]]
id = "t1"
title = "Task 1"
status = "completed"
"#,
    );
    assert!(
        write_result.is_ok(),
        "failed to write implementation-plan.toml"
    );

    let mut ctx = dummy_context(root);
    let update_result = run_update(&mut ctx);
    assert!(update_result.is_ok(), "run_update failed");

    let task_list_result =
        fs::read_to_string(root.join("docs/agent-context/current/task-list.toml"));
    assert!(
        task_list_result.is_err(),
        "task-list.toml is a deprecated projection and should not be created"
    );
}
