//! Integration tests for `exo phase add` ordering (--after, --before, --first).

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{
    exo_cmd_with_storage, exo_init_with_storage, exo_plan_add_epoch_with_storage,
    exo_plan_add_phase_with_storage,
};

fn write_minimal_plan_with_storage(
    root: &std::path::Path,
    backend: &str,
) -> (String, String, String, String) {
    exo_init_with_storage(root, backend);
    let epoch_id = exo_plan_add_epoch_with_storage(root, backend, "Epoch 16");
    let phase64_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 64", None, None);
    let phase65_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 65", None, None);
    let phase66_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 66", None, None);
    (epoch_id, phase64_id, phase65_id, phase66_id)
}

fn epoch_phase_ids(root: &std::path::Path, backend: &str, epoch_id: &str) -> Vec<String> {
    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "status", epoch_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let phases = json
        .get("result")
        .and_then(|r| r.get("phases"))
        .and_then(|v| v.as_array())
        .expect("phases array");
    phases
        .iter()
        .filter_map(|p| {
            p.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.to_string())
        })
        .collect()
}

#[test_matrix(["sqlite"])]
fn add_phase_after_inserts_in_epoch_order(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();
    let (epoch_id, phase64_id, phase65_id, phase66_id) =
        write_minimal_plan_with_storage(root, backend);

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "phase",
            "add",
            "--epoch",
            &epoch_id,
            "--title",
            "Inserted",
            "--after",
            &phase65_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let phase_x_id = json
        .get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string();

    let ids = epoch_phase_ids(root, backend, &epoch_id);
    assert_eq!(ids, vec![phase64_id, phase65_id, phase_x_id, phase66_id]);
}

#[test_matrix(["sqlite"])]
fn add_phase_after_errors_when_phase_not_in_epoch(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();
    let (epoch_id, _phase64_id, _phase65_id, _phase66_id) =
        write_minimal_plan_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .arg("phase")
        .arg("add")
        .arg("--epoch")
        .arg(&epoch_id)
        .arg("--title")
        .arg("Inserted")
        .arg("--after")
        .arg("nope")
        .assert()
        .failure();
}

#[test_matrix(["sqlite"])]
fn add_phase_before_inserts_in_epoch_order(backend: &str) {
    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let root = temp.path();
    let (epoch_id, phase64_id, phase65_id, phase66_id) =
        write_minimal_plan_with_storage(root, backend);

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "phase",
            "add",
            "--epoch",
            &epoch_id,
            "--title",
            "Before 65",
            "--before",
            &phase65_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let phase_x_id = json
        .get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string();

    let ids = epoch_phase_ids(root, backend, &epoch_id);
    assert_eq!(ids, vec![phase64_id, phase_x_id, phase65_id, phase66_id]);
}

#[test_matrix(["sqlite"])]
fn add_phase_first_inserts_at_beginning(backend: &str) {
    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let root = temp.path();
    let (epoch_id, phase64_id, phase65_id, phase66_id) =
        write_minimal_plan_with_storage(root, backend);

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "phase",
            "add",
            "--epoch",
            &epoch_id,
            "--title",
            "First Phase",
            "--first",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let phase_x_id = json
        .get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string();

    let ids = epoch_phase_ids(root, backend, &epoch_id);
    assert_eq!(ids, vec![phase_x_id, phase64_id, phase65_id, phase66_id]);
}

#[test_matrix(["sqlite"])]
fn add_phase_before_errors_when_phase_not_in_epoch(backend: &str) {
    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let root = temp.path();
    let (epoch_id, _phase64_id, _phase65_id, _phase66_id) =
        write_minimal_plan_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .arg("phase")
        .arg("add")
        .arg("--epoch")
        .arg(&epoch_id)
        .arg("--title")
        .arg("Inserted")
        .arg("--before")
        .arg("nope")
        .assert()
        .failure();
}
