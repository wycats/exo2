//! Integration tests for `exo rfc repair`.

#[macro_use]
mod test_support;

use exo::context::{SQLITE_DB_PATH, SqliteLoader, SqliteWriter};
use predicates::prelude::*;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_rfc_create};

fn write_minimal_context(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

fn write_four_digit_convention_rfc(root: &std::path::Path) {
    exo_rfc_create(root, "Convention Anchor", "0001", "0", "rfc", Some("Body."));
}

fn write_drifted_command_transport_rfc(root: &std::path::Path) {
    exo_rfc_create(
        root,
        "Command Transport",
        "00003",
        "0",
        "transport",
        Some("Body."),
    );
}

fn write_local_sandbox_malformed_anchor_fixture(root: &std::path::Path, backend: &str) {
    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Interface Compatibility",
        "0001",
        "0",
        "rfc",
        Some("Body."),
    );
    exo_rfc_create(
        root,
        "Self Contained Infrastructure",
        "0002",
        "0",
        "rfc",
        Some("Body."),
    );
    exo_rfc_create(root, "Local HTTPS", "0003", "0", "rfc", Some("Body."));
    let malformed = root.join("docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md");
    std::fs::write(
        malformed,
        "<!-- exo:1 -->\n\n# RFC 4: Local v0 Rehearsal Contract\n\nBody.\n",
    )
    .expect("write malformed RFC anchor fixture");
}

#[test_matrix(["sqlite"])]
fn rfc_status_reports_identity_repair_steering(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    write_four_digit_convention_rfc(root);
    write_drifted_command_transport_rfc(root);

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC Identity Repairs"))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/00003-command-transport.md",
        ))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/0003-command-transport.md",
        ))
        .stdout(predicate::str::contains("exo rfc repair 00003"));
}

#[test_matrix(["sqlite"])]
fn rfc_status_json_reports_stored_metadata_for_manual_metadata_drift(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Real Title", "0004", "0", "rfc", Some("Body."));

    let original_path = root.join("docs/rfcs/stage-0/0004-real-title.md");
    let drifted_path = root.join("docs/rfcs/stage-0/0004-untitled.md");
    std::fs::rename(&original_path, &drifted_path).expect("rename RFC path");

    let assert = exo_cmd_with_storage(root, backend)
        .args(["rfc", "status", "--format", "json"])
        .assert()
        .success();
    let stdout = assert.get_output().stdout.clone();
    let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "expected valid JSON from rfc status: {error}\n{}",
            String::from_utf8_lossy(&stdout)
        )
    });
    let repairs = value["result"]["repairs"]
        .as_array()
        .expect("repairs should be an array");
    let repair = repairs
        .iter()
        .find(|repair| repair["id"] == "00004")
        .expect("expected RFC 0004 repair");

    assert_eq!(
        repair["stored_metadata"]["path"],
        "docs/rfcs/stage-0/0004-real-title.md"
    );
    assert_eq!(repair["stored_metadata"]["stage"], 0);
    assert_eq!(repair["stored_metadata"]["status"], "active");
    assert_eq!(repair["stored_metadata"]["slug"], "real-title");
    assert_eq!(repair["stored_metadata"]["title"], "Real Title");
    assert!(
        repair["reasons"]
            .as_array()
            .expect("reasons array")
            .iter()
            .any(|reason| reason == "metadata_path_drift"),
        "expected metadata_path_drift reason: {repair:#?}"
    );
}

#[test_matrix(["sqlite"])]
fn malformed_rfc_anchor_is_repair_debt_not_context_load_blocker(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_local_sandbox_malformed_anchor_fixture(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["status"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args(["task", "list"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC Identity Repairs"))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md",
        ))
        .stdout(predicate::str::contains("exo rfc repair 0004"));

    let repairs = exo::rfc::detect_rfc_repair_candidates(root).expect("detect repairs");
    let repair = repairs
        .iter()
        .find(|repair| repair.id == "0004")
        .expect("expected malformed RFC 4 repair debt");
    assert!(
        repair
            .reasons
            .iter()
            .any(|reason| reason == "missing_anchor_ulid"),
        "expected missing anchor ULID reason: {repair:#?}"
    );
    assert!(
        repair
            .reasons
            .iter()
            .any(|reason| reason == "anchor_rfc_number_drift"),
        "expected anchor number drift reason: {repair:#?}"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_stamps_malformed_anchor_and_syncs_missing_metadata(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_local_sandbox_malformed_anchor_fixture(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Repaired RFC 0004"))
        .stdout(predicate::str::contains("missing_anchor_ulid"))
        .stdout(predicate::str::contains("anchor_rfc_number_drift"));

    let repaired =
        std::fs::read_to_string(root.join("docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md"))
            .expect("read repaired RFC");
    assert!(
        repaired.starts_with("<!-- exo:4 ulid:"),
        "repair should stamp RFC 4 anchor: {repaired}"
    );

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(4)
        .expect("load repaired RFC row")
        .expect("expected RFC 4 row");
    assert_eq!(
        row.file_path,
        "docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md"
    );
    assert!(
        exo::rfc::detect_rfc_repair_candidates(root)
            .expect("detect repairs after repair")
            .iter()
            .all(|repair| repair.id != "0004"),
        "RFC 4 repair debt should be cleared"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_existing_metadata_row_for_malformed_anchor(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Local v0 Rehearsal Contract",
        "0004",
        "0",
        "rfc",
        Some("Body."),
    );

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let before = loader
        .load_rfc_by_number(4)
        .expect("load RFC 4 before repair")
        .expect("expected RFC 4 row before repair");
    let path = root.join("docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md");
    let original = std::fs::read_to_string(&path).expect("read RFC 4");
    let (_, body) = original.split_once('\n').expect("anchor line");
    std::fs::write(&path, format!("<!-- exo:1 -->\n{body}"))
        .expect("write malformed anchor over existing row");

    let repairs = exo::rfc::detect_rfc_repair_candidates(root).expect("detect repair debt");
    let repair = repairs
        .iter()
        .find(|repair| repair.id == "0004")
        .expect("expected RFC 0004 repair debt");
    assert_eq!(
        repair
            .stored_metadata
            .as_ref()
            .expect("stored metadata")
            .path,
        "docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md"
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success();

    let repaired = std::fs::read_to_string(&path).expect("read repaired RFC");
    assert!(
        repaired.starts_with(&format!("<!-- exo:4 ulid:{} -->", before.text_id)),
        "repair should preserve existing metadata identity: {repaired}"
    );
    let after = loader
        .load_rfc_by_number(4)
        .expect("load RFC 4 after repair")
        .expect("expected RFC 4 row after repair");
    assert_eq!(after.text_id, before.text_id);
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_metadata_for_path_drifted_malformed_anchor(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Real Title", "0004", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let before = loader
        .load_rfc_by_number(4)
        .expect("load RFC 4 before repair")
        .expect("expected RFC 4 row before repair");
    let original_path = root.join("docs/rfcs/stage-0/0004-real-title.md");
    let drifted_path = root.join("docs/rfcs/stage-0/0004-untitled.md");
    let original = std::fs::read_to_string(&original_path).expect("read RFC 4");
    let (_, body) = original.split_once('\n').expect("anchor line");
    std::fs::rename(&original_path, &drifted_path).expect("rename RFC path");
    std::fs::write(&drifted_path, format!("<!-- exo:4 -->\n{body}"))
        .expect("write malformed anchor over drifted RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_anchor_ulid"))
        .stdout(predicate::str::contains("metadata_path_drift"));

    assert!(
        !drifted_path.exists(),
        "repair should move the drifted RFC back to the metadata path"
    );
    let repaired = std::fs::read_to_string(&original_path).expect("read repaired RFC");
    assert!(
        repaired.starts_with(&format!("<!-- exo:4 ulid:{} -->", before.text_id)),
        "repair should preserve existing metadata identity: {repaired}"
    );
    let rows = loader.load_rfcs().expect("load RFC rows after repair");
    assert_eq!(
        rows.iter().filter(|row| row.rfc_number == 4).count(),
        1,
        "repair must not create duplicate RFC 4 metadata rows"
    );
    let after = rows
        .iter()
        .find(|row| row.rfc_number == 4)
        .expect("expected RFC 4 row after repair");
    assert_eq!(after.text_id, before.text_id);
    assert_eq!(after.feature, before.feature);
    assert_eq!(after.file_path, "docs/rfcs/stage-0/0004-real-title.md");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_uses_partial_anchor_number_for_malformed_visible_drift(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Wrong Visible", "00003", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let before = loader
        .load_rfc_by_number(3)
        .expect("load RFC 3 before repair")
        .expect("expected RFC 3 row before repair");
    let original_path = root.join("docs/rfcs/stage-0/00003-wrong-visible.md");
    let drifted_path = root.join("docs/rfcs/stage-0/00004-wrong-visible.md");
    let original = std::fs::read_to_string(&original_path).expect("read RFC 3");
    let (_, body) = original.split_once('\n').expect("anchor line");
    std::fs::rename(&original_path, &drifted_path).expect("rename RFC path");
    std::fs::write(&drifted_path, format!("<!-- exo:3 -->\n{body}"))
        .expect("write malformed anchor over drifted RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exo rfc repair 00003"));

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "00003"])
        .assert()
        .success()
        .stdout(predicate::str::contains("filename_rfc_number_drift"))
        .stdout(predicate::str::contains("missing_anchor_ulid"))
        .stdout(predicate::str::contains("unexpected_rfc_number_width").not());

    assert!(
        !drifted_path.exists(),
        "repair should move the malformed visible drift back to RFC 3"
    );
    let repaired = std::fs::read_to_string(&original_path).expect("read repaired RFC");
    assert!(
        repaired.starts_with(&format!("<!-- exo:3 ulid:{} -->", before.text_id)),
        "repair should preserve RFC 3 metadata identity: {repaired}"
    );
    let rows = loader.load_rfcs().expect("load RFC rows after repair");
    assert_eq!(
        rows.iter().filter(|row| row.rfc_number == 3).count(),
        1,
        "repair must not create duplicate RFC 3 metadata rows"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_metadata_when_anchor_ulid_drifts(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Wrong Ulid", "0004", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let before = loader
        .load_rfc_by_number(4)
        .expect("load RFC 4 before repair")
        .expect("expected RFC 4 row before repair");
    let path = root.join("docs/rfcs/stage-0/0004-wrong-ulid.md");
    let original = std::fs::read_to_string(&path).expect("read RFC 4");
    let (_, body) = original.split_once('\n').expect("anchor line");
    std::fs::write(
        &path,
        format!("<!-- exo:4 ulid:01ARZ3NDEKTSV4RRFFQ69G5FAV -->\n{body}"),
    )
    .expect("write wrong but valid ULID anchor over existing row");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success()
        .stdout(predicate::str::contains("anchor_ulid_drift"));

    let repaired = std::fs::read_to_string(&path).expect("read repaired RFC");
    assert!(
        repaired.starts_with(&format!("<!-- exo:4 ulid:{} -->", before.text_id)),
        "repair should restore existing metadata ULID: {repaired}"
    );
    let rows = loader.load_rfcs().expect("load RFC rows after repair");
    assert_eq!(
        rows.iter().filter(|row| row.rfc_number == 4).count(),
        1,
        "repair must not create duplicate RFC 4 metadata rows"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_restores_own_identity_when_anchor_ulid_copied_from_other_rfc(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Victim Rfc", "0004", "0", "rfc", Some("Body."));
    exo_rfc_create(root, "Donor Rfc", "0005", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let victim_before = loader
        .load_rfc_by_number(4)
        .expect("load RFC 4 before repair")
        .expect("expected RFC 4 row before repair");
    let donor_before = loader
        .load_rfc_by_number(5)
        .expect("load RFC 5 before repair")
        .expect("expected RFC 5 row before repair");

    // Copy RFC 5's anchor (number + ULID) onto RFC 4's file.
    let victim_path = root.join("docs/rfcs/stage-0/0004-victim-rfc.md");
    let original = std::fs::read_to_string(&victim_path).expect("read RFC 4");
    let (_, body) = original.split_once('\n').expect("anchor line");
    std::fs::write(
        &victim_path,
        format!("<!-- exo:5 ulid:{} -->\n{body}", donor_before.text_id),
    )
    .expect("write copied anchor over RFC 4 file");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success();

    // RFC 4's file must be repaired back to its own identity...
    let repaired = std::fs::read_to_string(&victim_path).expect("read repaired RFC 4");
    assert!(
        repaired.starts_with(&format!("<!-- exo:4 ulid:{} -->", victim_before.text_id)),
        "repair should restore RFC 4's own identity, not adopt the copied ULID: {repaired}"
    );

    // ...and RFC 5's metadata must be untouched.
    let donor_after = loader
        .load_rfc_by_number(5)
        .expect("load RFC 5 after repair")
        .expect("expected RFC 5 row after repair");
    assert_eq!(donor_after.file_path, donor_before.file_path);
    assert_eq!(donor_after.title, donor_before.title);
    assert!(
        root.join("docs/rfcs/stage-0/0005-donor-rfc.md").exists(),
        "RFC 5's file must remain in place"
    );
}

#[test_matrix(["sqlite"])]
fn non_repair_writes_refuse_identity_drift(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Wrong Number", "00003", "2", "rfc", Some("Body."));

    let original_path = root.join("docs/rfcs/stage-2/00003-wrong-number.md");
    let drifted_path = root.join("docs/rfcs/stage-2/00004-wrong-number.md");
    std::fs::rename(&original_path, &drifted_path).expect("rename RFC path");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "withdraw", "00004", "--reason", "obsolete"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("identity repair debt"))
        .stderr(predicate::str::contains("exo rfc repair 00003"));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load RFC 3 after rejected withdraw")
        .expect("expected RFC 3 row after rejected withdraw");
    assert_eq!(row.status, "active");
    assert!(
        !root
            .join("docs/rfcs/withdrawn/00004-wrong-number.md")
            .exists()
    );
}

#[test_matrix(["sqlite"])]
fn rfc_status_preserves_mixed_historical_low_number_widths(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Early RFC", "0001", "0", "rfc", Some("Body."));
    exo_rfc_create(root, "Neighbor RFC", "00227", "0", "rfc", Some("Body."));
    exo_rfc_create(
        root,
        "Wide Historical RFC",
        "00228",
        "0",
        "rfc",
        Some("Body."),
    );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exo rfc repair 00228").not())
        .stdout(predicate::str::contains("docs/rfcs/stage-0/0228-wide-historical-rfc.md").not());
}

#[test_matrix(["sqlite"])]
fn rfc_repair_normalizes_filename_to_repo_width(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    write_four_digit_convention_rfc(root);
    write_drifted_command_transport_rfc(root);

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "00003"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Repaired RFC 00003"))
        .stdout(predicate::str::contains("unexpected_rfc_number_width"));

    let original = root.join("docs/rfcs/stage-0/00003-command-transport.md");
    let normalized = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    assert!(!original.exists(), "stale RFC filename should be gone");
    assert!(normalized.exists(), "normalized RFC filename should exist");

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0003-command-transport.md");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_meaningful_existing_slug(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "New Title", "0001", "0", "rfc", Some("Body."));

    let current = root.join("docs/rfcs/stage-0/0001-new-title.md");
    let stale = root.join("docs/rfcs/stage-0/0001-old-title.md");
    std::fs::rename(&current, &stale).expect("simulate stale title slug");
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .upsert_rfc(
            &row.text_id,
            row.rfc_number,
            &row.title,
            row.stage,
            &row.status,
            row.feature.as_deref(),
            "old-title",
            "docs/rfcs/stage-0/0001-old-title.md",
            row.superseded_by.as_deref(),
            row.supersedes.as_deref(),
            row.withdrawal_reason.as_deref(),
            row.archived_reason.as_deref(),
            row.consolidated_into.as_deref(),
        )
        .expect("sync sqlite to stale path fixture");

    let outcome = exo::rfc::repair(root, "0001").expect("repair stale title slug");
    assert!(
        !outcome.repaired,
        "meaningful existing slug should not be repair debt: {outcome:#?}"
    );
    assert_eq!(outcome.old_path, "docs/rfcs/stage-0/0001-old-title.md");
    assert_eq!(outcome.new_path, "docs/rfcs/stage-0/0001-old-title.md");
    assert!(
        stale.exists(),
        "repair should preserve meaningful historical slug"
    );
    assert!(
        !current.exists(),
        "repair should not rewrite meaningful slug to the title"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_reports_repaired_when_unexpected_slug_policy_moves_file(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "New Title", "0001", "0", "rfc", Some("Body."));

    let current = root.join("docs/rfcs/stage-0/0001-new-title.md");
    let stale = root.join("docs/rfcs/stage-0/0001-untitled.md");
    std::fs::rename(&current, &stale).expect("simulate placeholder slug drift");
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .upsert_rfc(
            &row.text_id,
            row.rfc_number,
            &row.title,
            row.stage,
            &row.status,
            row.feature.as_deref(),
            "untitled",
            "docs/rfcs/stage-0/0001-untitled.md",
            row.superseded_by.as_deref(),
            row.supersedes.as_deref(),
            row.withdrawal_reason.as_deref(),
            row.archived_reason.as_deref(),
            row.consolidated_into.as_deref(),
        )
        .expect("sync sqlite to placeholder path fixture");

    let outcome = exo::rfc::repair(root, "0001").expect("repair placeholder title slug");
    assert!(outcome.repaired, "repair should report file move");
    assert_eq!(outcome.old_path, "docs/rfcs/stage-0/0001-untitled.md");
    assert_eq!(outcome.new_path, "docs/rfcs/stage-0/0001-new-title.md");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|reason| reason == "filename_slug_drift"),
        "expected filename slug drift reason: {outcome:#?}"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_reports_repaired_when_metadata_row_is_missing(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Metadata Missing", "0001", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer.delete_rfc(&row.text_id).expect("delete rfc row");

    let outcome = exo::rfc::repair(root, "0001").expect("repair missing metadata row");
    assert!(outcome.repaired, "repair should report metadata write");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|reason| reason == "metadata_relink"),
        "expected metadata relink reason: {outcome:#?}"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_status_reports_missing_metadata_row_as_repair_debt(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Metadata Missing", "0001", "0", "rfc", Some("Body."));
    exo_rfc_create(root, "Stable Metadata", "0002", "0", "rfc", Some("Body."));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer.delete_rfc(&row.text_id).expect("delete rfc row");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC Identity Repairs"))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/0001-metadata-missing.md",
        ))
        .stdout(predicate::str::contains("exo rfc repair 00001"));
}

#[test_matrix(["sqlite"])]
fn rfc_status_reports_missing_metadata_when_all_rows_are_missing(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Only RFC", "0001", "0", "rfc", Some("Body."));
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .delete_rfc(&row.text_id)
        .expect("delete only rfc row");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC Identity Repairs"))
        .stdout(predicate::str::contains("metadata_relink"))
        .stdout(predicate::str::contains("exo rfc repair 00001"));
    assert!(
        loader
            .load_rfc_by_number(1)
            .expect("load after status")
            .is_none(),
        "status must not silently recreate missing RFC metadata rows"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_uses_anchor_identity_when_metadata_missing_and_filename_drifted(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Missing Drift", "0003", "0", "rfc", Some("Body."));
    exo_rfc_create(root, "Stable Metadata", "0004", "0", "rfc", Some("Body."));

    let canonical = root.join("docs/rfcs/stage-0/0003-missing-drift.md");
    let drifted = root.join("docs/rfcs/stage-0/00005-missing-drift.md");
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let repaired_text_id = row.text_id.clone();
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer.delete_rfc(&row.text_id).expect("delete rfc row");
    writer
        .upsert_rfc(
            "01wrongduplicate",
            3,
            "Wrong Duplicate",
            0,
            "active",
            Some("wrong-feature"),
            "wrong-duplicate",
            "docs/rfcs/stage-0/0003-wrong-duplicate.md",
            None,
            Some("0002"),
            Some("do not inherit"),
            None,
            None,
        )
        .expect("insert duplicate numeric row");
    std::fs::rename(&canonical, &drifted).expect("simulate missing-row numeric drift");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC 00003"))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/00005-missing-drift.md",
        ))
        .stdout(predicate::str::contains("exo rfc repair 00003"))
        .stdout(predicate::str::contains("exo rfc repair 00005").not());

    let outcome = exo::rfc::repair(root, "00003").expect("repair by anchor identity");
    assert!(outcome.repaired, "repair should relink missing metadata");
    assert_eq!(outcome.id, "00003");
    assert_eq!(outcome.new_path, "docs/rfcs/stage-0/0003-missing-drift.md");

    let rows = loader.load_rfcs().expect("load rfc rows after repair");
    let repaired = rows
        .iter()
        .find(|row| row.text_id == repaired_text_id)
        .expect("expected repaired RFC 3 anchor row");
    assert_eq!(repaired.rfc_number, 3);
    assert_eq!(
        repaired.file_path,
        "docs/rfcs/stage-0/0003-missing-drift.md"
    );
    assert!(
        repaired.feature.is_none(),
        "missing-row repair must not inherit feature metadata from numeric duplicate"
    );
    assert!(
        repaired.supersedes.is_none(),
        "missing-row repair must not inherit relationship metadata from numeric duplicate"
    );
    assert!(
        !rows.iter().any(|row| row.rfc_number == 5),
        "repair must not create RFC 5"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_status_reports_invalid_anchor_ulid_as_repair_debt(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let path = root.join("docs/rfcs/stage-0/0004-invalid-anchor.md");
    std::fs::create_dir_all(path.parent().expect("rfc parent")).expect("create rfc dir");
    std::fs::write(
        &path,
        "<!-- exo:4 ulid:not_valid! -->\n\n# RFC 4: Invalid Anchor\n\nBody.\n",
    )
    .expect("write invalid anchor RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC Identity Repairs"))
        .stdout(predicate::str::contains("invalid_anchor_ulid"))
        .stdout(predicate::str::contains("exo rfc repair 0004"));
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_valid_ulid_when_anchor_number_is_malformed(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let path = root.join("docs/rfcs/stage-0/0004-invalid-anchor-number.md");
    std::fs::create_dir_all(path.parent().expect("rfc parent")).expect("create rfc dir");
    let text_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    std::fs::write(
        &path,
        format!("<!-- exo:x ulid:{text_id} -->\n\n# RFC 4: Invalid Anchor Number\n\nBody.\n"),
    )
    .expect("write invalid anchor number RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0004"])
        .assert()
        .success()
        .stdout(predicate::str::contains("invalid_anchor_number"))
        .stdout(predicate::str::contains("missing_anchor_ulid").not());

    let repaired = std::fs::read_to_string(&path).expect("read repaired RFC");
    assert!(
        repaired.starts_with(&format!("<!-- exo:4 ulid:{text_id} -->")),
        "repair should preserve the valid ULID when only the anchor number is malformed: {repaired}"
    );
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(4)
        .expect("load repaired RFC")
        .expect("expected repaired RFC row");
    assert_eq!(row.text_id, text_id);
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_selected_text_id_when_duplicate_numbers_exist(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Duplicate Number",
        "0001",
        "0",
        "right-feature",
        Some("Body."),
    );

    let canonical = root.join("docs/rfcs/stage-0/0001-duplicate-number.md");
    let drifted = root.join("docs/rfcs/stage-0/0001-duplicate-number-drift.md");
    std::fs::rename(&canonical, &drifted).expect("simulate metadata path drift");

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let right_row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .upsert_rfc(
            "01wrongfeature",
            1,
            "Wrong Duplicate",
            0,
            "active",
            Some("wrong-feature"),
            "wrong-duplicate",
            "docs/rfcs/stage-0/0001-wrong-duplicate.md",
            None,
            None,
            None,
            None,
            None,
        )
        .expect("insert duplicate numeric row");

    let outcome = exo::rfc::repair(root, "0001").expect("repair selected anchored row");
    assert!(outcome.repaired, "repair should update metadata path");

    let rows = loader.load_rfcs().expect("load rfc rows");
    let repaired = rows
        .iter()
        .find(|row| row.text_id == right_row.text_id)
        .expect("selected anchor row remains");
    assert_eq!(repaired.feature.as_deref(), Some("right-feature"));
    assert_eq!(
        repaired.file_path,
        "docs/rfcs/stage-0/0001-duplicate-number-drift.md"
    );
    let duplicate = rows
        .iter()
        .find(|row| row.text_id == "01wrongfeature")
        .expect("duplicate numeric row remains");
    assert_eq!(duplicate.feature.as_deref(), Some("wrong-feature"));
    assert_eq!(
        duplicate.file_path,
        "docs/rfcs/stage-0/0001-wrong-duplicate.md"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_rejects_malformed_filename_without_separator(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let malformed = root.join("docs/rfcs/stage-0/0001notes.md");
    std::fs::create_dir_all(malformed.parent().expect("malformed parent")).expect("create stage");
    std::fs::write(
        &malformed,
        "<!-- exo:1 ulid:01malformed -->\n\n# RFC 1: Malformed\n\nBody.\n",
    )
    .expect("write malformed RFC-like file");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0001"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("RFC 0001 not found"));

    assert!(
        malformed.exists(),
        "malformed RFC-like filename must not be renamed or mutated"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_reconcile_preserves_metadata_for_parse_failed_present_file(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Do Not Drop", "0001", "0", "rfc", Some("Body."));

    let original = root.join("docs/rfcs/stage-0/0001-do-not-drop.md");
    let malformed = root.join("docs/rfcs/stage-0/0001notes.md");
    std::fs::rename(&original, &malformed).expect("simulate malformed manual rename");

    exo::rfc::reconcile_rfcs_with_project(root, None)
        .expect("reconcile should skip malformed name");

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(1)
        .expect("load rfc row")
        .expect("metadata must not be deleted for present parse-failed RFC file");
    assert_eq!(row.title, "Do Not Drop");
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0001-do-not-drop.md");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_preserves_anchored_metadata_identity_for_numeric_drift(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    write_four_digit_convention_rfc(root);
    exo_rfc_create(
        root,
        "Command Transport",
        "0003",
        "0",
        "transport",
        Some("Body."),
    );

    let canonical = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    let drifted = root.join("docs/rfcs/stage-0/00004-command-transport.md");
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .upsert_rfc(
            &row.text_id,
            row.rfc_number,
            &row.title,
            row.stage,
            &row.status,
            row.feature.as_deref(),
            "command-transport",
            "docs/rfcs/stage-0/0003-command-transport.md",
            row.superseded_by.as_deref(),
            row.supersedes.as_deref(),
            row.withdrawal_reason.as_deref(),
            row.archived_reason.as_deref(),
            row.consolidated_into.as_deref(),
        )
        .expect("sync fixture metadata to four-digit canonical path");
    let row = loader
        .load_rfc_by_number(3)
        .expect("reload rfc row")
        .expect("expected sqlite row after fixture sync");
    assert_eq!(
        row.file_path, "docs/rfcs/stage-0/0003-command-transport.md",
        "fixture metadata should preserve the anchored RFC 3 path"
    );
    std::fs::rename(&canonical, &drifted).expect("simulate wrong visible numeric prefix");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RFC 00003"))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/00004-command-transport.md",
        ))
        .stdout(predicate::str::contains(
            "docs/rfcs/stage-0/0003-command-transport.md",
        ))
        .stdout(predicate::str::contains("exo rfc repair 00003"));

    let outcome = exo::rfc::repair(root, "00003").expect("repair visible numeric drift");
    assert!(outcome.repaired, "numeric drift should be repaired");
    assert_eq!(outcome.id, "00003");
    assert_eq!(
        outcome.old_path,
        "docs/rfcs/stage-0/00004-command-transport.md"
    );
    assert_eq!(
        outcome.new_path,
        "docs/rfcs/stage-0/0003-command-transport.md"
    );
    assert!(
        outcome
            .reasons
            .iter()
            .any(|reason| reason == "filename_rfc_number_drift"),
        "expected filename number drift reason: {outcome:#?}"
    );
    assert!(canonical.exists(), "repair should restore RFC 3 filename");
    assert!(!drifted.exists(), "drifted RFC 4 filename should be gone");

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected repaired RFC 3 row");
    assert_eq!(row.rfc_number, 3);
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0003-command-transport.md");
    assert!(
        loader
            .load_rfc_by_number(4)
            .expect("load wrong rfc row")
            .is_none(),
        "repair must not create RFC 4"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_restores_anchor_number_when_anchor_drifts_from_metadata(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Anchor Drift",
        "0003",
        "0",
        "transport",
        Some("Body."),
    );

    let path = root.join("docs/rfcs/stage-0/0003-anchor-drift.md");
    let content = std::fs::read_to_string(&path).expect("read RFC");
    std::fs::write(&path, content.replacen("<!-- exo:3 ", "<!-- exo:4 ", 1))
        .expect("simulate anchor-number drift");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exo rfc repair 00003"))
        .stdout(predicate::str::contains("exo rfc repair 00004").not())
        .stderr(predicate::str::contains("anchor_rfc_number_drift"));

    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let before = loader
        .load_rfc_by_number(3)
        .expect("load rfc row before repair")
        .expect("expected RFC 3 row before repair");
    assert_eq!(
        before.rfc_number, 3,
        "status must not rewrite SQLite identity"
    );

    let outcome = exo::rfc::repair(root, "00003").expect("repair anchor-number drift");
    assert!(outcome.repaired, "anchor repair should report work");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|reason| reason == "anchor_rfc_number_drift"),
        "expected anchor drift reason: {outcome:#?}"
    );

    let repaired = std::fs::read_to_string(&path).expect("read repaired RFC");
    assert!(
        repaired.starts_with("<!-- exo:3 "),
        "repair should restore the anchor number to the selected RFC identity"
    );
    let after = loader
        .load_rfc_by_number(3)
        .expect("load rfc row after repair")
        .expect("expected RFC 3 row after repair");
    assert_eq!(after.rfc_number, 3);
    assert_eq!(after.file_path, "docs/rfcs/stage-0/0003-anchor-drift.md");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_relinks_metadata_when_file_already_canonical(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    write_four_digit_convention_rfc(root);
    write_drifted_command_transport_rfc(root);

    let original = root.join("docs/rfcs/stage-0/00003-command-transport.md");
    let normalized = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    std::fs::rename(&original, &normalized).expect("simulate manual filename normalization");

    let outcome = exo::rfc::repair(root, "00003").expect("repair stale metadata");
    assert!(outcome.repaired, "repair should report metadata work");
    assert!(
        outcome
            .reasons
            .iter()
            .any(|reason| reason == "metadata_path_drift"),
        "expected metadata path drift reason: {outcome:#?}"
    );

    assert!(normalized.exists(), "canonical RFC file should remain");
    let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).expect("open sqlite loader");
    let row = loader
        .load_rfc_by_number(3)
        .expect("load rfc row")
        .expect("expected sqlite row");
    assert_eq!(row.file_path, "docs/rfcs/stage-0/0003-command-transport.md");
}

#[test_matrix(["sqlite"])]
fn archived_rfc_read_surfaces_do_not_render_as_stage_zero_ideas(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    std::fs::create_dir_all(root.join("docs/rfcs/archive")).expect("create archive dir");
    std::fs::write(
        root.join("docs/rfcs/archive/0022-unified-project-state.md"),
        "<!-- exo:22 ulid:01archivedstate -->\n\n# RFC 0022: Unified Project State\n\n- **Status**: Archived (superseded; formerly Stage 4 Stable)\n- **Superseded by**: RFC 10176\n\nBody.\n",
    )
    .expect("write archived RFC");

    let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).expect("open sqlite writer");
    writer
        .upsert_rfc(
            "01archivedstate",
            22,
            "Unified Project State",
            0,
            "archived",
            None,
            "unified-project-state",
            "docs/rfcs/archive/0022-unified-project-state.md",
            Some("10176"),
            None,
            None,
            Some("Superseded by RFC 10176 as the current SQLite-backed project-state model."),
            None,
        )
        .expect("seed archived RFC");

    let show = exo_cmd_with_storage(root, backend)
        .args(["rfc", "show", "0022"])
        .assert()
        .success();
    let show_stdout = String::from_utf8(show.get_output().stdout.clone())
        .expect("rfc show stdout should be utf8");
    assert!(
        show_stdout.contains("**Status**: Archived"),
        "{show_stdout}"
    );
    assert!(
        !show_stdout.contains("Stage**: ○○○○ 0 (Idea)"),
        "{show_stdout}"
    );

    let status = exo_cmd_with_storage(root, backend)
        .args(["rfc", "status", "--format", "json"])
        .assert()
        .success();
    let status_stdout = status.get_output().stdout.clone();
    let value: serde_json::Value = serde_json::from_slice(&status_stdout).unwrap_or_else(|error| {
        panic!(
            "expected valid JSON from rfc status: {error}\n{}",
            String::from_utf8_lossy(&status_stdout)
        )
    });
    let stage_zero = value["result"]["stages"]
        .as_array()
        .expect("stages should be an array")
        .iter()
        .find(|stage| stage["stage"] == 0)
        .expect("stage 0 group should exist");
    assert!(
        !stage_zero["rfcs"]
            .as_array()
            .expect("stage rfcs should be an array")
            .iter()
            .any(|rfc| rfc["id"] == "00022"),
        "archived RFC should not appear as a Stage 0 idea: {stage_zero}"
    );

    let archived_group = value["result"]["lifecycle"]
        .as_array()
        .expect("lifecycle should be an array")
        .iter()
        .find(|group| group["status"] == "archived")
        .expect("archived lifecycle group should exist");
    assert!(
        archived_group["rfcs"]
            .as_array()
            .expect("archived rfcs should be an array")
            .iter()
            .any(|rfc| rfc["id"] == "00022" && rfc["status"] == "archived"),
        "archived RFC should appear in lifecycle group: {archived_group}"
    );
}

#[test_matrix(["sqlite"])]
fn superseded_active_rfc_read_surfaces_do_not_render_as_active_stage_work(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(
        root,
        "Old Proposal",
        "0001",
        "1",
        "rfc",
        Some("- **Superseded by**: RFC 0002\n\nBody."),
    );
    exo_rfc_create(root, "New Proposal", "0002", "1", "rfc", Some("Body."));

    let show = exo_cmd_with_storage(root, backend)
        .args(["rfc", "show", "0001"])
        .assert()
        .success();
    let show_stdout = String::from_utf8(show.get_output().stdout.clone())
        .expect("rfc show stdout should be utf8");
    assert!(
        show_stdout.contains("**Status**: Superseded"),
        "{show_stdout}"
    );
    assert!(
        show_stdout.contains("**Superseded by**: RFC 0002"),
        "{show_stdout}"
    );
    assert!(
        !show_stdout.contains("**Stage**: ○●○○ 1 (Proposal)"),
        "{show_stdout}"
    );

    let show_json = exo_cmd_with_storage(root, backend)
        .args(["rfc", "show", "0001", "--format", "json"])
        .assert()
        .success();
    let show_json_stdout = show_json.get_output().stdout.clone();
    let show_value: serde_json::Value =
        serde_json::from_slice(&show_json_stdout).unwrap_or_else(|error| {
            panic!(
                "expected valid JSON from rfc show: {error}\n{}",
                String::from_utf8_lossy(&show_json_stdout)
            )
        });
    assert_eq!(show_value["result"]["status"], "superseded");
    assert_eq!(show_value["result"]["superseded_by"], "0002");

    let status = exo_cmd_with_storage(root, backend)
        .args(["rfc", "status", "--format", "json"])
        .assert()
        .success();
    let status_stdout = status.get_output().stdout.clone();
    let value: serde_json::Value = serde_json::from_slice(&status_stdout).unwrap_or_else(|error| {
        panic!(
            "expected valid JSON from rfc status: {error}\n{}",
            String::from_utf8_lossy(&status_stdout)
        )
    });
    let stage_one = value["result"]["stages"]
        .as_array()
        .expect("stages should be an array")
        .iter()
        .find(|stage| stage["stage"] == 1)
        .expect("stage 1 group should exist");
    assert!(
        !stage_one["rfcs"]
            .as_array()
            .expect("stage rfcs should be an array")
            .iter()
            .any(|rfc| rfc["id"] == "00001"),
        "superseded RFC should not appear as active Stage 1 work: {stage_one}"
    );
    assert!(
        stage_one["rfcs"]
            .as_array()
            .expect("stage rfcs should be an array")
            .iter()
            .any(|rfc| rfc["id"] == "00002"),
        "non-superseded RFC should remain in the Stage 1 group: {stage_one}"
    );

    let superseded_group = value["result"]["lifecycle"]
        .as_array()
        .expect("lifecycle should be an array")
        .iter()
        .find(|group| group["status"] == "superseded")
        .expect("superseded lifecycle group should exist");
    assert!(
        superseded_group["rfcs"]
            .as_array()
            .expect("superseded rfcs should be an array")
            .iter()
            .any(|rfc| rfc["id"] == "00001" && rfc["status"] == "superseded"),
        "superseded RFC should appear in lifecycle group: {superseded_group}"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_rejects_malformed_id_without_leading_digit_lookup(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    exo_rfc_create(root, "Do Not Touch", "0001", "0", "rfc", Some("Body."));

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "1abc"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid RFC ID: 1abc"));

    assert!(
        root.join("docs/rfcs/stage-0/0001-do-not-touch.md").exists(),
        "malformed id must not mutate RFC 1"
    );
}

#[test_matrix(["sqlite"])]
fn rfc_repair_rejects_ambiguous_equivalent_numeric_prefixes(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    write_drifted_command_transport_rfc(root);

    let duplicate = root.join("docs/rfcs/stage-0/0003-command-transport.md");
    std::fs::write(
        &duplicate,
        "<!-- exo:3 ulid:01duplicate000000000000000 -->\n\n# RFC 3: Command Transport Duplicate\n\nBody.\n",
    )
    .expect("write duplicate numeric RFC");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0003"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("RFC 0003 is ambiguous"));

    assert!(
        root.join("docs/rfcs/stage-0/00003-command-transport.md")
            .exists(),
        "original RFC must not move when lookup is ambiguous"
    );
    assert!(duplicate.exists(), "duplicate RFC must remain in place");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_does_not_match_template_by_numeric_prefix(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let template = root.join("docs/rfcs/stage-0/0000-template.md");
    std::fs::create_dir_all(template.parent().expect("template parent")).expect("create stage dir");
    std::fs::write(&template, "# RFC Template\n\nTemplate body.\n").expect("write template");

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("RFC 0 not found"));

    assert!(template.exists(), "template must not be renamed or mutated");
}

#[test_matrix(["sqlite"])]
fn rfc_repair_ignores_evidence_markdown_with_date_prefix(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    write_minimal_context(root, backend);
    let evidence = root.join("docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md");
    std::fs::create_dir_all(evidence.parent().expect("evidence parent"))
        .expect("create evidence dir");
    let original = "# Live probes\n\nEvidence notes for RFC 8.\n";
    std::fs::write(&evidence, original).expect("write evidence note");

    let repairs = exo::rfc::detect_rfc_repair_candidates(root).expect("detect repairs");
    assert!(
        repairs.iter().all(|repair| repair.id != "2026"),
        "evidence notes must not be surfaced as RFC repair debt: {repairs:#?}"
    );

    exo_cmd_with_storage(root, backend)
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exo rfc repair 2026").not())
        .stdout(
            predicate::str::contains(
                "docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md",
            )
            .not(),
        );

    exo_cmd_with_storage(root, backend)
        .args(["rfc", "repair", "2026"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("RFC 2026 not found"));

    let after = std::fs::read_to_string(&evidence).expect("read evidence note");
    assert_eq!(
        after, original,
        "repair must not stamp RFC anchors into evidence notes"
    );
}
