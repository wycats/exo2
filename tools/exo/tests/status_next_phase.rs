//! Integration tests for the "next phase" hint in `exo phase status`.

#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exosuit_storage::rusqlite::Connection;
use predicates::prelude::*;
use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_phase_with_storage, exo_plan_update_status_with_storage,
};

fn phase_status(root: &std::path::Path, phase_id: &str) -> String {
    let conn =
        Connection::open(exo::context::db_path_resolving_project(root)).expect("open exo db");
    conn.query_row(
        "SELECT status FROM phases_data WHERE text_id = ?1",
        [phase_id],
        |row| row.get::<_, String>(0),
    )
    .expect("phase status")
}

#[test_matrix(["sqlite"])]
fn phase_status_suggests_next_phase_when_none_active(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    let epoch_id = exo_active_epoch_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");
    let phase2_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 2", None, None);

    exo_cmd_with_storage(root, backend)
        .arg("phase")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("No active phase found."))
        .stdout(predicate::str::contains("Next phase to start"))
        .stdout(predicate::str::contains("exo phase start"));

    let phase_status_json = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let phase_status_json: serde_json::Value =
        serde_json::from_slice(&phase_status_json).expect("valid phase status json");
    let current_owner = phase_status_json
        .get("result")
        .and_then(|result| result.get("current_owner"))
        .expect("current_owner in no-active phase status");
    assert!(
        current_owner
            .get("owner_kind")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert!(
        current_owner
            .get("owner_basis")
            .and_then(|value| value.as_str())
            .is_some()
    );

    let output = exo_cmd_with_storage(root, backend)
        .arg("status")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&phase2_id) || stdout.contains("Phase 2"),
        "Expected phase ID or title in output"
    );
}

#[test_matrix(["sqlite"])]
fn status_prefers_next_phase_after_last_executed_phase(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    let epoch_id = exo_active_epoch_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");
    let _phase69_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 69", None, None);
    let phase74_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 74", None, None);
    let _phase75_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 75", None, None);
    exo_plan_update_status_with_storage(root, backend, &phase74_id, "in-progress");
    exo_plan_update_status_with_storage(root, backend, &phase74_id, "completed");

    exo_cmd_with_storage(root, backend)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("exo phase start"))
        .stdout(predicate::str::contains("Phase 75"));
}

#[test_matrix(["sqlite"])]
fn phase_start_accepts_explicit_pending_phase_id(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    let epoch_id = exo_active_epoch_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");
    let first_pending =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "First pending", None, None);
    let target_pending =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Target pending", None, None);

    exo_cmd_with_storage(root, backend)
        .args(["phase", "start", &target_pending])
        .assert()
        .success()
        .stdout(predicate::str::contains("Target pending"));

    assert_eq!(phase_status(root, &phase_id), "completed");

    exo_cmd_with_storage(root, backend)
        .arg("phase")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Target pending"));

    exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "phase", "read-details"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&target_pending));

    exo_cmd_with_storage(root, backend)
        .args(["phase", "start", &target_pending])
        .assert()
        .success()
        .stdout(predicate::str::contains("Target pending"));
    assert_eq!(phase_status(root, &target_pending), "in-progress");

    exo_cmd_with_storage(root, backend)
        .args(["phase", "start", &first_pending])
        .assert()
        .success();
}
