//! Integration test: `exo update` repairs legacy RFC file permissions.

#![allow(clippy::disallowed_methods)]
#![cfg(unix)]

#[macro_use]
mod test_support;

use exo::command::update::run_update;
use exo::context::{AgentContext, ExoState};
use std::os::unix::fs::PermissionsExt;
use test_case::test_matrix;
use test_support::fs;

#[test_matrix(["sqlite"])]
fn update_makes_rfc_markdown_files_writable(_backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    let rfc_path = root.join("docs/rfcs/stage-0/0001-test.md");
    assert!(fs::create_dir_all(rfc_path.parent().unwrap()).is_ok());
    assert!(fs::write(&rfc_path, "# Test RFC\n").is_ok());

    // Simulate legacy behavior where RFCs were made read-only.
    let mut perms = ok_or_return!(
        fs::metadata(&rfc_path).map(|m| m.permissions()),
        "failed to read metadata"
    );
    perms.set_mode(0o444);
    assert!(fs::set_permissions(&rfc_path, perms).is_ok());

    // exosuit.toml is required by upgrade plugins that resolve the workspace root
    assert!(fs::write(root.join("exosuit.toml"), "[project]\nname = \"test\"\n").is_ok());

    let cache_dir = root.join(".cache");
    assert!(fs::create_dir_all(&cache_dir).is_ok());
    assert!(exosuit_storage::open_database(cache_dir.join("exo.db")).is_ok());

    // Create agent-context dir and write SQL dump files — run_update verifies they exist
    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());
    exo::context::write_sql_dump(root);

    let mut ctx = AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState::default(),
    };
    run_update(&mut ctx).expect("run_update should succeed");

    let mode = ok_or_return!(
        fs::metadata(&rfc_path).map(|m| m.permissions().mode()),
        "failed to read metadata after update"
    );
    assert_ne!(mode & 0o200, 0, "expected RFC to be writable after update");
}
