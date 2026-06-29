//! Integration tests for world-state probing.

#[macro_use]
mod test_support;

use exo::context::{AgentContext, StorageBackend};
use exo::world_state::WorldState;
use tempfile::TempDir;
use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_epoch_with_storage, exo_plan_add_phase_with_storage,
    exo_plan_add_task_with_storage, exo_plan_update_status_with_storage, write_implementation_plan,
};

#[test_matrix(["sqlite"])]
fn world_state_probes_active_phase_tasks_and_steps(backend: &str) {
    let tmp = TempDir::new();
    assert!(tmp.is_ok(), "failed to create temp dir");
    let Ok(tmp) = tmp else {
        return;
    };
    let root = tmp.path();

    exo_init_with_storage(root, backend);
    let bootstrap_epoch_id = exo_active_epoch_id(root);
    exo_cmd_with_storage(root, backend)
        .args(["epoch", "remove", &bootstrap_epoch_id])
        .assert()
        .success();
    let epoch_id = exo_plan_add_epoch_with_storage(root, backend, "Epoch 1");
    let phase_id = exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 1", None, None);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "in-progress");
    exo_plan_add_task_with_storage(root, backend, &phase_id, "goal-1", "Do the thing");

    // Add a task under the goal via CLI (works on both backends)
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

    // On TOML, also write implementation-plan.toml
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
details = "Desc"
status = "pending"

[[plan.goals.tasks]]
id = "task-1"
title = "Do the thing"
status = "pending"
"#
            ),
        );
    }

    let storage = if backend == "sqlite" {
        StorageBackend::Sqlite
    } else {
        StorageBackend::Sqlite
    };
    let context_result = AgentContext::load_with_backend(root.to_path_buf(), storage);
    assert!(context_result.is_ok(), "failed to load AgentContext");
    let Ok(context) = context_result else {
        return;
    };
    let world_result = WorldState::probe(&context);
    assert!(world_result.is_ok(), "failed to probe WorldState");
    let Ok(world) = world_result else {
        return;
    };

    assert!(world.active_phase.is_some(), "expected active phase");
    let Some(active) = world.active_phase else {
        return;
    };
    assert_eq!(active.id, phase_id);
    assert_eq!(active.epoch_title, "Epoch 1");

    assert_eq!(world.tasks.len(), 1);
    assert_eq!(world.tasks[0].0, "goal-1::task-1");

    assert_eq!(world.goals.len(), 1);
    assert_eq!(world.goals[0].id, "goal-1");
    assert_eq!(world.goals[0].label, "Do the thing");

    // TempDir is not necessarily a git repo; probe should not fail.
    assert!(!world.git_dirty);
}
