//! Integration test for `exo rfc promote`.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::context::{SQLITE_DB_PATH, SqliteLoader};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value as JsonValue;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create};

fn write_minimal_context(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

fn write_stage_2_rfc(root: &std::path::Path) -> std::path::PathBuf {
    exo_rfc_create(
        root,
        "Config Editing CLI",
        "0001",
        "2",
        "Config",
        Some("Draft body."),
    );
    root.join("docs/rfcs/stage-2/0001-config-editing-cli.md")
}

#[test_matrix(["sqlite"])]
fn rfc_promote_moves_within_docs_rfcs_and_does_not_create_repo_root_stage_dir(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let stage2_path = write_stage_2_rfc(root);

    // Promote from stage-2 -> stage-3.
    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args(["rfc", "promote", "0001", "--stage", "3"])
        .assert()
        .success();

    let stage3_dir = root.join("docs/rfcs/stage-3");
    assert!(
        stage3_dir.exists(),
        "expected {} to exist",
        stage3_dir.display()
    );

    // The promoted file should now exist under docs/rfcs/stage-3 with the same filename.
    let promoted_path = stage3_dir.join("0001-config-editing-cli.md");
    assert!(
        promoted_path.exists(),
        "expected promoted RFC at {}",
        promoted_path.display()
    );

    // The source should be gone.
    assert!(
        !stage2_path.exists(),
        "expected original stage-2 RFC to be moved"
    );

    // Regression: do not create repo-root stage-3.
    let repo_root_stage3 = root.join("stage-3");
    assert!(
        !repo_root_stage3.exists(),
        "did not expect repo-root stage dir at {}",
        repo_root_stage3.display()
    );
}

#[test_matrix(["sqlite"])]
fn rfc_promote_refuses_missing_or_stale_target_stage_without_mutating(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let stage2_path = write_stage_2_rfc(root);

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001"])
        .assert()
        .failure();
    assert!(
        stage2_path.exists(),
        "missing --stage must not move the RFC"
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "2"])
        .assert()
        .failure()
        .stderr(contains("target stage 2 does not match next stage 3"));
    assert!(
        stage2_path.exists(),
        "stale target stage must not move the RFC"
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "3"])
        .assert()
        .success();
    let stage3_path = root.join("docs/rfcs/stage-3/0001-config-editing-cli.md");
    assert!(
        stage3_path.exists(),
        "correct target stage promotes the RFC"
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "3"])
        .assert()
        .failure()
        .stderr(contains("target stage 3 does not match next stage 4"));
    assert!(
        stage3_path.exists(),
        "reusing a stale target stage must leave the RFC in place"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_promote_rejects_stable_rfc_without_mutating(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Stable RFC",
        "0001",
        "4",
        "Config",
        Some("Stable body."),
    );
    let stable_path = root.join("docs/rfcs/stage-4/0001-stable-rfc.md");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "5"])
        .assert()
        .failure()
        .stderr(contains("already at Stage 4"));

    assert!(stable_path.exists(), "stable RFC should remain in stage-4");
}

#[test_matrix(["sqlite"])]
fn rfc_promote_rejects_superseded_rfc_without_mutating(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Old Proposal",
        "0001",
        "1",
        "Config",
        Some("- **Superseded by**: RFC 0002\n\nBody."),
    );
    exo_rfc_create(root, "New Proposal", "0002", "1", "Config", Some("Body."));
    let old_path = root.join("docs/rfcs/stage-1/0001-old-proposal.md");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "2"])
        .assert()
        .failure()
        .stderr(contains("RFC is Superseded and is not active stage work"));

    assert!(old_path.exists(), "superseded RFC should remain in stage-1");
    assert!(
        !root.join("docs/rfcs/stage-2/0001-old-proposal.md").exists(),
        "superseded RFC must not move to stage-2"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_promote_allows_manual_filename_normalization_after_reconcile(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Command Transport",
        "00003",
        "0",
        "Transport",
        Some("Body."),
    );

    let original = root.join("docs/rfcs/stage-0/00003-command-transport.md");
    let normalized = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    std::fs::rename(&original, &normalized).expect("simulate manual RFC id-width normalization");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "show", "00003"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "00003", "--stage", "1"])
        .assert()
        .success();

    let promoted = root.join("docs/rfcs/stage-1/0003-command-transport.md");
    assert!(
        promoted.exists(),
        "promote should move the visible normalized filename"
    );
    assert!(
        !normalized.exists(),
        "stage-0 normalized RFC should be moved"
    );

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.stage, 1);
    assert_eq!(row.file_path, "docs/rfcs/stage-1/0003-command-transport.md");
}

#[test_matrix(["sqlite"])]
fn rfc_promote_allows_slug_only_repair_debt(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Config Editing CLI",
        "0001",
        "2",
        "Config",
        Some("Draft body."),
    );

    let original = root.join("docs/rfcs/stage-2/0001-config-editing-cli.md");
    let untitled = root.join("docs/rfcs/stage-2/0001-untitled.md");
    std::fs::rename(&original, &untitled).expect("simulate slug-only filename drift");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "3"])
        .assert()
        .success();

    assert!(
        root.join("docs/rfcs/stage-3/0001-untitled.md").exists(),
        "promote should not be blocked by cosmetic slug-only repair debt"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_withdraw_format_json_emits_single_valid_envelope(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Withdraw Me",
        "0001",
        "0",
        "Config",
        Some("Draft body."),
    );

    let assert = exo_cmd_with_storage(root, backend)
        .args([
            "--format", "json", "rfc", "withdraw", "0001", "--reason", "obsolete",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: JsonValue = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("rfc withdraw --format json should emit one JSON envelope: {error}\n{stdout}")
    });
    assert_eq!(value["status"], "ok");
    assert_eq!(value["result"]["kind"], "rfc.withdraw");
    assert_eq!(value["result"]["id"], "0001");
    assert!(
        root.join("docs/rfcs/withdrawn/0001-withdraw-me.md")
            .exists(),
        "withdraw should move the RFC"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_supersede_format_json_reports_missing_symmetric_update(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Old RFC", "0001", "0", "Config", Some("Old body."));

    let assert = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "rfc",
            "supersede",
            "0001",
            "--by",
            "99999",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: JsonValue = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("rfc supersede --format json should emit one JSON envelope: {error}\n{stdout}")
    });
    assert_eq!(value["status"], "ok");
    assert_eq!(value["result"]["kind"], "rfc.supersede");
    assert_eq!(value["result"]["id"], "0001");
    assert_eq!(value["result"]["superseded_by"], "99999");
    assert_eq!(value["result"]["symmetric_update_applied"], false);
}

#[test_matrix(["sqlite"])]
fn rfc_supersede_rejects_invalid_by_without_mutating(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Old RFC", "0001", "0", "Config", Some("Old body."));

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "supersede", "0001", "--by", "not-a-number"])
        .assert()
        .failure()
        .stderr(contains("Invalid RFC ID 'not-a-number'"));

    let json_assert = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "rfc",
            "supersede",
            "0001",
            "--by",
            "not-a-number",
        ])
        .assert()
        .failure();
    let stdout = String::from_utf8(json_assert.get_output().stdout.clone()).expect("utf8 stdout");
    let value: JsonValue = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("rfc supersede invalid --by should emit a JSON envelope: {error}\n{stdout}")
    });
    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["details"]["operation"], "rfc.supersede");
    assert_eq!(value["error"]["details"]["superseded_by"], "not-a-number");

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.superseded_by, None);
}

#[test_matrix(["sqlite"])]
fn rfc_supersede_rejects_ambiguous_by_without_mutating(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Old RFC", "0001", "0", "Config", Some("Old body."));
    exo_rfc_create(root, "New RFC", "0002", "0", "Config", Some("New body."));
    std::fs::write(
        root.join("docs/rfcs/stage-0/00002-duplicate-new-rfc.md"),
        "<!-- exo:2 ulid:01duplicate000000000000000000 -->\n\n# RFC 2: Duplicate New RFC\n\nBody.\n",
    )
    .expect("write duplicate RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "supersede", "0001", "--by", "0002"])
        .assert()
        .failure()
        .stderr(contains("ambiguous"));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.superseded_by, None);
}

#[test_matrix(["sqlite"])]
fn rfc_promote_reconciles_manual_stage_move_before_target_check(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Command Transport",
        "0001",
        "0",
        "Transport",
        Some("Body."),
    );

    let original = root.join("docs/rfcs/stage-0/0001-command-transport.md");
    let moved = root.join("docs/rfcs/stage-1/0001-command-transport.md");
    std::fs::create_dir_all(moved.parent().expect("stage-1 parent")).expect("create stage-1");
    std::fs::rename(&original, &moved).expect("simulate manual stage directory move");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "1"])
        .assert()
        .failure()
        .stderr(contains("target stage 1 does not match next stage 2"));

    assert!(moved.exists(), "stale target stage must not move file");
    assert!(
        !root
            .join("docs/rfcs/stage-2/0001-command-transport.md")
            .exists(),
        "promote must not advance on-disk stage when the target is stale"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_promote_reconciles_stale_stage_metadata_before_stable_status(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Stable Metadata",
        "0001",
        "4",
        "Transport",
        Some("Stable body."),
    );

    let original = root.join("docs/rfcs/stage-4/0001-stable-metadata.md");
    let moved = root.join("docs/rfcs/stage-3/0001-stable-metadata.md");
    std::fs::create_dir_all(moved.parent().expect("stage-3 parent")).expect("create stage-3");
    std::fs::rename(&original, &moved).expect("simulate stale stage-4 metadata path drift");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0001", "--stage", "5"])
        .assert()
        .failure()
        .stderr(contains("target stage 5 does not match next stage 4"))
        .stderr(contains("already at Stage 4").not());
}

#[test_matrix(["sqlite"])]
fn rfc_promote_rejects_ambiguous_equivalent_numeric_prefixes(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Command Transport",
        "00003",
        "0",
        "Transport",
        Some("Body."),
    );

    let duplicate = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    std::fs::write(
        &duplicate,
        "<!-- exo:3 ulid:01duplicate000000000000000 -->\n\n# RFC 3: Command Transport Duplicate\n\nBody.\n",
    )
    .expect("write duplicate numeric RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "promote", "0003", "--stage", "1"])
        .assert()
        .failure()
        .stderr(contains("RFC 0003 is ambiguous"));

    assert!(
        root.join("docs/rfcs/stage-0/00003-command-transport.md")
            .exists(),
        "canonical-width RFC should not move when lookup is ambiguous"
    );
    assert!(
        duplicate.exists(),
        "normalized-width duplicate should not move when lookup is ambiguous"
    );
    assert!(
        !root
            .join("docs/rfcs/stage-1/00003-command-transport.md")
            .exists(),
        "ambiguous promote must not create a stage-1 file"
    );
}
