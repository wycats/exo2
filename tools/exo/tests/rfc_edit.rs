//! Integration tests for `exo rfc edit`.

#[macro_use]
mod test_support;

use exo::context::{SQLITE_DB_PATH, SqliteLoader};
use predicates::prelude::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create, fs};

fn write_minimal_context(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

fn write_rfc(root: &std::path::Path) -> std::path::PathBuf {
    exo_rfc_create(root, "Old Title", "0001", "0", "General", Some("Old body."));
    root.join("docs/rfcs/stage-0/0001-old-title.md")
}

#[test_matrix(["sqlite"])]
fn rfc_edit_updates_title_and_keeps_readonly(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    write_minimal_context(root, backend);
    let rfc_path = write_rfc(root);

    // Make it read-only to match the project's invariants.
    let metadata = fs::metadata(&rfc_path);
    assert!(metadata.is_ok(), "expected RFC to exist");
    let Ok(metadata) = metadata else {
        return;
    };
    let mut perms = metadata.permissions();
    #[cfg(unix)]
    perms.set_mode(0o444);
    #[cfg(windows)]
    perms.set_readonly(true);
    assert!(
        fs::set_permissions(&rfc_path, perms).is_ok(),
        "failed to set RFC permissions"
    );

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args(["rfc", "edit", "0001", "--title", "New Title"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edited RFC:"));

    let updated_result = fs::read_to_string(&rfc_path);
    assert!(updated_result.is_ok(), "failed to read updated RFC");
    let Ok(updated) = updated_result else {
        return;
    };
    assert!(updated.starts_with("<!-- exo:1 ulid:"));
    assert!(!updated.starts_with("---\n"));
    assert!(updated.contains("# RFC 0001: New Title"));
    assert!(updated.contains("Old body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.title, "New Title");
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0001-old-title.md");

    let metadata = fs::metadata(&rfc_path);
    assert!(metadata.is_ok(), "expected RFC to exist after edit");
    let Ok(metadata) = metadata else {
        return;
    };
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o222, 0);
    }
    #[cfg(windows)]
    assert!(metadata.permissions().readonly());
}

#[test_matrix(["sqlite"])]
fn rfc_edit_can_replace_body_from_stdin(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    write_minimal_context(root, backend);
    let rfc_path = write_rfc(root);

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args(["rfc", "edit", "0001", "--body-file", "-"])
        .write_stdin("Replacement body from stdin\n")
        .assert()
        .success();

    let updated_result = fs::read_to_string(&rfc_path);
    assert!(updated_result.is_ok(), "failed to read updated RFC");
    let Ok(updated) = updated_result else {
        return;
    };
    assert!(updated.starts_with("<!-- exo:1 ulid:"));
    assert!(!updated.starts_with("---\n"));
    assert!(updated.contains("# RFC 0001: Old Title"));
    assert!(updated.contains("Replacement body from stdin"));
    assert!(!updated.contains("Old body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.title, "Old Title");

    let metadata = fs::metadata(&rfc_path);
    assert!(metadata.is_ok(), "expected RFC to exist after edit");
    let Ok(metadata) = metadata else {
        return;
    };
    // RFCs are intentionally kept writable (we enforce correctness at the
    // verification boundary, not via read-only filesystem permissions).
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode();
        assert_ne!(mode & 0o200, 0);
    }
    #[cfg(windows)]
    assert!(!metadata.permissions().readonly());
}
