//! Integration tests for phase-scoped read commands.

#[macro_use]
mod test_support;

use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage};

#[test_matrix(["sqlite"])]
fn explicit_phase_scoped_reads_reject_unknown_phase_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);

    for operation in ["read-goals", "read-tasks"] {
        let output = exo_cmd_with_storage(root, backend)
            .args(["--format", "json", "phase", operation, "missing-phase"])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();

        let json: serde_json::Value = serde_json::from_slice(&output).expect("valid error json");
        assert_eq!(json["status"], "error", "{json}");
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("Phase 'missing-phase' not found"),
            "{json}"
        );
    }
}
