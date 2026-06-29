//! Integration tests for axiom bootstrapping during init.
//!
//! Axioms are stored in SQLite and dumped to `axioms.sql`.
//! These tests verify that `init_project` seeds the correct axioms
//! by querying the database via the CLI.

#[macro_use]
mod test_support;

use exo::command::init::init_project;
use test_case::test_matrix;
use test_support::exo_cmd;

#[test_matrix(["sqlite"])]
fn init_project_writes_scoped_axioms_strict_mode(_backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    init_project(root, "Test Mission", "strict", "First milestone", &[])
        .expect("init_project strict");

    // Verify axioms were seeded into SQLite via CLI
    let output = exo_cmd(root)
        .args(["--format", "json", "axiom", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let axioms = json
        .get("result")
        .and_then(|r| r.get("axioms"))
        .and_then(|v| v.as_array())
        .expect("axioms array");

    let ids: Vec<&str> = axioms
        .iter()
        .filter_map(|a| a.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&"strict-mode"),
        "expected strict-mode axiom, got: {ids:?}"
    );
    assert!(
        ids.contains(&"context-is-king"),
        "expected context-is-king axiom"
    );
    assert!(
        ids.contains(&"phased-execution"),
        "expected phased-execution axiom"
    );
    assert!(
        !ids.contains(&"loose-mode"),
        "should not have loose-mode axiom in strict"
    );
}

#[test_matrix(["sqlite"])]
fn init_project_writes_scoped_axioms_loose_mode(_backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    init_project(root, "Test Mission", "loose", "First milestone", &[])
        .expect("init_project loose");

    let output = exo_cmd(root)
        .args(["--format", "json", "axiom", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let axioms = json
        .get("result")
        .and_then(|r| r.get("axioms"))
        .and_then(|v| v.as_array())
        .expect("axioms array");

    let ids: Vec<&str> = axioms
        .iter()
        .filter_map(|a| a.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&"loose-mode"),
        "expected loose-mode axiom, got: {ids:?}"
    );
    assert!(
        !ids.contains(&"strict-mode"),
        "should not have strict-mode axiom in loose"
    );
}
