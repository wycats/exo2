#![allow(clippy::disallowed_methods)] // integration tests use real fs APIs

mod test_support;

use exo::context::AgentContext;
use exo::state_machine::{PrimaryState, detect_active_strike, resolve_primary_state};
use tempfile::TempDir;
use test_support::{
    exo_active_phase_id, exo_cmd, exo_init_with_storage, exo_plan_add_task, exo_plan_update_status,
    write_implementation_plan,
};

fn setup_test_workspace() -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    exo_init_with_storage(root, "sqlite");
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status(root, &phase_id, "pending");
    let impl_path = root.join("docs/agent-context/current/implementation-plan.toml");
    let _ = std::fs::remove_file(&impl_path);

    (temp_dir, phase_id)
}

fn init_impl_plan(temp_dir: &TempDir, phase_id: &str) {
    let root = temp_dir.path();
    exo_plan_update_status(root, phase_id, "in-progress");
    // Add a goal to the phase in SQLite — the state machine checks SQLite goals
    exo_plan_add_task(root, phase_id, "add-feature", "Add feature");
    let impl_plan = format!(
        r#"[phase]
id = "{phase_id}"
title = "Phase 1"

[plan]
"#,
    );
    write_implementation_plan(root, &impl_plan);
}

fn add_deprecated_projection(temp_dir: &TempDir) {
    let task_list_path = temp_dir
        .path()
        .join("docs/agent-context/current/task-list.toml");
    std::fs::create_dir_all(task_list_path.parent().expect("task list parent"))
        .expect("create deprecated projection dir");
    std::fs::write(&task_list_path, "# deprecated").unwrap();
}

fn set_phase_active(temp_dir: &TempDir, phase_id: &str) {
    exo_plan_update_status(temp_dir.path(), phase_id, "in-progress");
}

#[test]
fn test_resolve_no_active_phase() {
    let (temp_dir, _phase_id) = setup_test_workspace();
    // Phase is pending, not active
    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    let state = resolve_primary_state(&context).unwrap();
    assert_eq!(state, PrimaryState::PreparingNextPhase);
}

#[test]
fn test_resolve_active_phase_unprepared() {
    let (temp_dir, phase_id) = setup_test_workspace();
    set_phase_active(&temp_dir, &phase_id);
    // No implementation plan = unprepared
    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    let state = resolve_primary_state(&context).unwrap();
    assert_eq!(state, PrimaryState::ActivePhaseUnprepared);
}

#[test]
fn test_resolve_active_phase_executing() {
    let (temp_dir, phase_id) = setup_test_workspace();
    set_phase_active(&temp_dir, &phase_id);
    init_impl_plan(&temp_dir, &phase_id);
    let impl_plan = format!(
        r#"[phase]
id = "{phase_id}"
title = "Phase 1"

[[plan.goals]]
name = "Add feature"
type = "feat"
details = "Description"
status = "pending"
"#,
    );
    write_implementation_plan(temp_dir.path(), &impl_plan);
    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    let state = resolve_primary_state(&context).unwrap();
    assert_eq!(state, PrimaryState::ActivePhaseExecuting);
}

#[test]
fn test_detect_active_strike_from_goal() {
    let (temp_dir, phase_id) = setup_test_workspace();
    set_phase_active(&temp_dir, &phase_id);
    // Create a strike via the CLI (writes to SQLite)
    exo_cmd(temp_dir.path())
        .args([
            "strike",
            "start",
            "--name",
            "Urgent Fix",
            "--goal",
            "Fix the thing",
        ])
        .assert()
        .success();

    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    let strike_id = detect_active_strike(&context).unwrap();
    assert!(strike_id.is_some(), "Expected an active strike");
}

// ─────────────────────────────────────────────────────────────────────────────
// Upgrade Gate Tests
// ─────────────────────────────────────────────────────────────────────────────

use exo::state_machine::check_upgrade_gate;

#[test]
fn test_upgrade_gate_passes_when_no_deprecated_files() {
    let (temp_dir, phase_id) = setup_test_workspace();
    set_phase_active(&temp_dir, &phase_id);
    init_impl_plan(&temp_dir, &phase_id);
    let impl_plan = format!(
        r#"[phase]
id = "{phase_id}"
title = "Phase 1"

[[plan.goals]]
name = "Some work"
type = "feat"
details = "Description"
status = "pending"
"#,
    );
    write_implementation_plan(temp_dir.path(), &impl_plan);
    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    assert!(check_upgrade_gate(&context).is_ok());
}

#[test]
fn test_upgrade_gate_passes_during_active_strike() {
    let (temp_dir, phase_id) = setup_test_workspace();
    set_phase_active(&temp_dir, &phase_id);
    // Add an active strike before introducing deprecated projections
    exo_cmd(temp_dir.path())
        .args([
            "strike",
            "start",
            "--name",
            "Urgent Fix",
            "--goal",
            "Fix the thing",
        ])
        .assert()
        .success();
    add_deprecated_projection(&temp_dir);

    let context = AgentContext::load_with_backend(
        temp_dir.path().to_path_buf(),
        exo::context::StorageBackend::Sqlite,
    )
    .unwrap();
    // Strike is active, so upgrade gate should pass (strike bypasses it)
    assert!(check_upgrade_gate(&context).is_ok());
}
