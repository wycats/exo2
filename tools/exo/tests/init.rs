//! Integration tests for `exo init`.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use std::fs;
use tempfile::TempDir;
use test_case::test_matrix;

#[test_matrix(["sqlite"])]
fn test_init_non_empty_dir(backend: &str) {
    let temp = ok_or_return!(TempDir::new(), "failed to create tempdir");
    let file_path = temp.path().join("some_file.txt");
    assert!(fs::write(file_path, "content").is_ok());

    let mut cmd = test_support::exo_cmd_with_storage(temp.path(), backend);
    cmd.arg("init")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Directory contains files beyond recognized context files",
        ));
}

// Note: Testing the interactive success path is difficult with assert_cmd/dialoguer
// without a more complex setup (e.g. rexpect).
// For now, we verify the failure case and rely on manual verification for the success case.
