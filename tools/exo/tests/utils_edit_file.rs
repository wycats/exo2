//! Integration tests for file-edit permission helpers.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::utils::edit_file_with_permissions;
use std::fs;
use tempfile::TempDir;
use test_case::test_matrix;

#[test_matrix(["sqlite"])]
fn edit_file_with_permissions_creates_missing_file_and_dirs(_backend: &str) {
    let tmp = ok_or_return!(TempDir::new(), "failed to create tempdir");
    let root = tmp.path();

    let path = root.join("docs/agent-context/current/task-list.toml");
    assert!(!path.exists());

    let result = edit_file_with_permissions(&path, |content| {
        assert!(content.is_empty());
        Ok("hello\n".to_string())
    });
    assert!(result.is_ok());

    assert!(path.exists());
    let content = ok_or_return!(fs::read_to_string(&path), "failed to read file contents");
    assert_eq!(content, "hello\n");
}

#[test_matrix(["sqlite"])]
fn edit_file_with_permissions_updates_existing_file(_backend: &str) {
    let tmp = ok_or_return!(TempDir::new(), "failed to create tempdir");
    let path = tmp.path().join("settings.toml");
    ok_or_return!(fs::write(&path, "before = true\n"), "failed to write file");

    let result = edit_file_with_permissions(&path, |content| {
        assert_eq!(content, "before = true\n");
        Ok("after = true\n".to_string())
    });

    assert!(result.is_ok());
    let content = ok_or_return!(fs::read_to_string(&path), "failed to read file contents");
    assert_eq!(content, "after = true\n");
}
