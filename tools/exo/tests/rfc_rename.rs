//! Integration tests for `exo rfc rename`.
//!
//! NOTE: This test intentionally renames an RFC file by hand to create a
//! title/filename mismatch. Normal CLI usage keeps anchors and filenames in
//! sync, so we must manually construct the mismatch to test `rfc rename`.

#[macro_use]
mod test_support;

use exo::context::{SQLITE_DB_PATH, SqliteLoader};
use predicates::prelude::*;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create, fs};

fn write_minimal_context(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

fn write_rfc_with_untitled_filename(root: &std::path::Path) {
    // Intentionally create RFC with title "Surface Refinements" but filename "untitled"
    // to test that rfc rename can fix this mismatch.
    exo_rfc_create(
        root,
        "Surface Refinements",
        "0001",
        "0",
        "surface-refinements",
        Some("Body."),
    );

    let created = root.join("docs/rfcs/stage-0/0001-surface-refinements.md");
    let untitled = root.join("docs/rfcs/stage-0/0001-untitled.md");
    std::fs::rename(&created, &untitled).expect("rename RFC to untitled fixture");
}

#[test_matrix(["sqlite"])]
fn rfc_rename_updates_filename_slug(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    write_minimal_context(root, backend);
    write_rfc_with_untitled_filename(root);

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args(["rfc", "rename", "0001"])
        .assert()
        .success()
        // Updated to match new Command trait output format
        .stdout(predicate::str::contains("Renamed RFC 0001"));

    assert!(
        !root.join("docs/rfcs/stage-0/0001-untitled.md").exists(),
        "expected old untitled RFC filename to be removed"
    );

    let new_path = root.join("docs/rfcs/stage-0/0001-surface-refinements.md");
    assert!(new_path.exists(), "expected renamed RFC file to exist");

    let updated = fs::read_to_string(&new_path);
    assert!(updated.is_ok(), "failed to read renamed RFC");
    let Ok(updated) = updated else {
        return;
    };
    assert!(updated.starts_with("<!-- exo:1 ulid:"));
    assert!(!updated.starts_with("---"));
    assert!(updated.contains("# RFC 1: Surface Refinements"));
}

#[test_matrix(["sqlite"])]
fn rfc_rename_relinks_metadata_after_visible_id_width_changes(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Command Transport",
        "00003",
        "0",
        "transport",
        Some("Body."),
    );

    let original = root.join("docs/rfcs/stage-0/00003-command-transport.md");
    let normalized = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    std::fs::rename(&original, &normalized).expect("simulate manual RFC id-width normalization");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "rename", "00003"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Renamed RFC 00003"));

    assert!(
        normalized.exists(),
        "rename should preserve the visible numeric prefix already on disk"
    );
    assert!(
        !original.exists(),
        "rename should not recreate the stale id-width filename"
    );

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0003-command-transport.md");
}

#[test_matrix(["sqlite"])]
fn rfc_rename_does_not_match_template_by_numeric_prefix(backend: &str) {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "failed to create tempdir");
    let Ok(temp) = temp else {
        return;
    };
    let root = temp.path();

    write_minimal_context(root, backend);
    let template = root.join("docs/rfcs/stage-0/0000-template.md");
    std::fs::create_dir_all(template.parent().expect("template parent")).expect("create stage dir");
    std::fs::write(&template, "# RFC Template\n\nTemplate body.\n").expect("write template");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "rename", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("RFC 0 not found"));

    assert!(template.exists(), "template must not be renamed or mutated");
}
