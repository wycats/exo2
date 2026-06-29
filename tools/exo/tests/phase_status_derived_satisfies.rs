//! Integration tests for derived task status via implementation-plan `satisfies` links.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use serde_json::Value as JsonValue;
use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_phase_with_storage, exo_plan_add_task_with_storage,
    exo_plan_update_status_with_storage, write_implementation_plan,
};

#[test_matrix(["sqlite"])]
fn phase_status_uses_plan_status_with_satisfies_links_present(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    let bootstrap_phase_id = exo_active_phase_id(root);
    let epoch_id = exo_active_epoch_id(root);
    // Add a new phase and make it active
    let phase_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 67", None, None);
    exo_plan_add_task_with_storage(
        root,
        backend,
        &phase_id,
        "ui-update-studio",
        "Update Studio rendering",
    );
    exo_plan_update_status_with_storage(root, backend, &bootstrap_phase_id, "pending");
    exo_plan_update_status_with_storage(root, backend, &phase_id, "in-progress");

    // Add a task under the goal via CLI
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Render update",
            "--id",
            "render",
            "--goal",
            "ui-update-studio",
        ])
        .assert()
        .success();

    // On TOML, also write implementation-plan.toml with satisfies links
    if backend == "toml" {
        let impl_plan = format!(
            r#"[phase]
id = "{phase_id}"
title = "Phase 67"

[[plan.goals]]
name = "ui-update-studio"
type = "feat"
details = "..."
satisfies = ["ui-update-studio"]
status = "completed"

[[plan.goals.tasks]]
id = "render"
title = "Render update"
status = "pending"
"#,
            phase_id = phase_id
        );
        write_implementation_plan(root, &impl_plan);
    }

    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = ok_or_return!(
        serde_json::from_slice::<JsonValue>(&output),
        "expected valid json output"
    );

    // Verify goals are visible with correct status
    let goals = some_or_return!(
        json.get("result")
            .and_then(|v| v.get("goals"))
            .and_then(|v| v.as_array()),
        "expected goals array"
    );

    let goal = some_or_return!(
        goals
            .iter()
            .find(|g| g.get("name").and_then(|v| v.as_str()) == Some("Update Studio rendering")),
        "expected ui-update-studio goal row"
    );

    // Status should be the plan status (pending), not a derived overlay
    assert_eq!(goal.get("status").and_then(|v| v.as_str()), Some("pending"));
}
