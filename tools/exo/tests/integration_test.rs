//! Broad integration tests for the `exo` CLI.

#[macro_use]
mod test_support;

use predicates::prelude::*;
use test_case::test_matrix;

/// Bare command with --direct and --storage but no current_dir set.
fn bare_exo_cmd(backend: &str) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.arg("--direct").args(["--storage", backend]);
    cmd
}

#[test_matrix(["sqlite"])]
fn test_help(backend: &str) {
    let mut cmd = bare_exo_cmd(backend);
    cmd.env("NO_COLOR", "1")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("exo"));
}

#[test_matrix(["sqlite"])]
fn test_phase_status_no_context(backend: &str) {
    // Run in a temp dir so it doesn't find the real context
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let mut cmd = bare_exo_cmd(backend);
    cmd.current_dir(temp.path())
        .arg("phase")
        .arg("status")
        .assert()
        .failure(); // Should fail or print error because no context found
}

#[test_matrix(["sqlite"])]
fn test_status_alias_no_context(backend: &str) {
    // `exo status` should behave like `exo phase status` (and fail without context).
    // Run in a temp dir so it doesn't find the real context
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };

    let mut cmd = bare_exo_cmd(backend);
    cmd.current_dir(temp.path())
        .arg("status")
        .assert()
        .failure();
}
