//! Integration tests for `exo epoch add --after` ordering.

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_epoch_with_storage,
};

fn write_minimal_plan_with_storage(
    root: &std::path::Path,
    backend: &str,
) -> (String, String, String) {
    exo_init_with_storage(root, backend);
    let bootstrap_epoch_id = exo_active_epoch_id(root);
    let epoch16_id = exo_plan_add_epoch_with_storage(root, backend, "Epoch 16");
    let epoch17_id = exo_plan_add_epoch_with_storage(root, backend, "Epoch 17");
    (bootstrap_epoch_id, epoch16_id, epoch17_id)
}

#[test_matrix(["sqlite"])]
fn add_epoch_after_inserts_in_plan_order(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    let (bootstrap_epoch_id, epoch16_id, epoch17_id) =
        write_minimal_plan_with_storage(root, backend);

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Epoch 16.5: Polish",
            "--after",
            &epoch16_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let epoch165_id = json
        .get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string();

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let epochs = list_json
        .get("result")
        .and_then(|r| r.get("epochs"))
        .and_then(|v| v.as_array())
        .expect("epochs array");
    let ids: Vec<String> = epochs
        .iter()
        .filter_map(|e| {
            e.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.to_string())
        })
        .collect();

    // Bootstrap epoch is created by exo init --defaults
    assert_eq!(
        ids,
        vec![bootstrap_epoch_id, epoch16_id, epoch165_id, epoch17_id]
    );
}

#[test_matrix(["sqlite"])]
fn add_epoch_after_errors_when_epoch_not_found(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    let (_bootstrap_epoch_id, _epoch16_id, _epoch17_id) =
        write_minimal_plan_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .arg("epoch")
        .arg("add")
        .arg("--title")
        .arg("Epoch 16.5: Polish")
        .arg("--after")
        .arg("nope")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Epoch 'nope' not found"))
        .stderr(predicates::str::contains("[Next]"))
        .stderr(predicates::str::contains("exo map"));
}
