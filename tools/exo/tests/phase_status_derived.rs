//! Integration tests for derived task status projection in `exo phase status`.

mod test_support;

use serde_json::Value as JsonValue;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage};

#[test_matrix(["sqlite"])]
fn phase_status_json_preserves_plan_status_without_derived_overlay(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    exo_init_with_storage(root, backend);

    // Add a goal and task via CLI (works on both backends)
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Seed RFCs", "--id", "seed-rfcs"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Seed task",
            "--id",
            "seed-task",
            "--goal",
            "seed-rfcs",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "status", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: JsonValue = serde_json::from_slice(&output).expect("valid json output");

    // Verify goals are visible in phase status
    let goals = json
        .get("result")
        .and_then(|v| v.get("goals"))
        .and_then(serde_json::Value::as_array)
        .expect("goals array");

    let seed = goals
        .iter()
        .find(|g| g.get("name").and_then(|v| v.as_str()) == Some("Seed RFCs"))
        .expect("seed-rfcs goal row");

    // Status should be the plan status (pending), not a derived overlay
    assert_eq!(seed.get("status").and_then(|v| v.as_str()), Some("pending"));
}
