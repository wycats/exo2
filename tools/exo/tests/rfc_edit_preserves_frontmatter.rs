//! Integration test for `exo rfc edit` preserving anchor-only metadata.

#[macro_use]
mod test_support;

use exo::context::{SQLITE_DB_PATH, SqliteLoader};
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create, fs};

fn write_minimal_context(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

fn write_stage_0_rfc(root: &std::path::Path) -> std::path::PathBuf {
    exo_rfc_create(
        root,
        "Config Editing CLI",
        "0001",
        "0",
        "Validation",
        Some("Original body."),
    );
    root.join("docs/rfcs/stage-0/0001-config-editing-cli.md")
}

#[test_matrix(["sqlite"])]
fn rfc_edit_body_file_preserves_anchor_only_file_and_sqlite_feature(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let rfc_path = write_stage_0_rfc(root);

    let body_path = root.join("new-body.md");
    assert!(fs::write(&body_path, "Replacement body.\n").is_ok());

    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args(["rfc", "edit", "0001", "--body-file"])
        .arg(&body_path)
        .assert()
        .success();

    let updated = ok_or_return!(
        fs::read_to_string(&rfc_path),
        "failed to read RFC after edit"
    );
    assert!(
        updated.starts_with("<!-- exo:1 ulid:"),
        "expected RFC anchor to remain intact; got:\n{updated}"
    );
    assert!(
        !updated.starts_with("---\n"),
        "frontmatter should not be reintroduced; got:\n{updated}"
    );
    assert!(
        updated.contains("# RFC 0001: Config Editing CLI"),
        "expected H1 title to remain unchanged; got:\n{updated}"
    );
    assert!(
        updated.contains("Replacement body."),
        "expected replacement body to be written; got:\n{updated}"
    );

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.feature.as_deref(), Some("Validation"));
    assert_eq!(row.title, "Config Editing CLI");
}
