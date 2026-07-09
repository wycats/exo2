#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use std::path::Path;
use std::process::Command;
use test_support::{exo_cmd, exo_init, exo_rfc_create};

use exo::context::SqliteLoader;
use exo::project::Project;

fn git_init(root: &Path) {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("run git init");

    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git command");

    assert!(
        output.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn age_file_for_verifier(path: &Path) {
    let output = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(path)
        .output()
        .expect("run touch");

    assert!(
        output.status.success(),
        "touch failed for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verifier_preserves_untracked_anchored_rfc_after_promote() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    git_init(root);
    exo_init(root);
    std::fs::create_dir_all(root.join("docs/rfcs/stage-0")).expect("create RFC dir");
    std::fs::write(root.join("docs/rfcs/stage-0/README.md"), "Stage 0 RFCs\n")
        .expect("write tracked RFC dir marker");
    run_git_ok(root, &["add", "."]);
    run_git_ok(
        root,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "init",
        ],
    );

    exo_rfc_create(
        root,
        "Identity Preserving Verifier",
        "10185",
        "0",
        "Verifier",
        Some("RFC body."),
    );
    exo_cmd(root)
        .args(["rfc", "promote", "10185", "--stage", "1"])
        .assert()
        .success();

    let project = Project::resolve(root).expect("resolve project");
    let loader = SqliteLoader::open(project.db_path()).expect("open RFC metadata");
    assert!(
        loader
            .load_rfc_by_number(10185)
            .expect("load RFC metadata")
            .is_none(),
        "promoting an unmerged RFC must not republish it into shared canonical metadata"
    );

    let promoted = root.join("docs/rfcs/stage-1/10185-identity-preserving-verifier.md");
    assert!(promoted.exists(), "promoted RFC must exist");
    let before = std::fs::read_to_string(&promoted).expect("read promoted RFC");
    assert!(before.starts_with("<!-- exo:10185 ulid:"));

    let reminders = exo::verifiers::run_global_verifiers(root);

    assert!(
        reminders
            .iter()
            .all(|reminder| reminder.kind != "rfc.manual_file_detected"
                && reminder.kind != "rfc.manual_file_repaired"),
        "anchored untracked RFC must not be reported or repaired as manual: {reminders:#?}"
    );
    assert!(promoted.exists(), "verifier must not remove promoted RFC");
    let after = std::fs::read_to_string(&promoted).expect("read promoted RFC after verifier");
    assert_eq!(after, before, "verifier must not rewrite anchored RFC");
    assert!(
        !root
            .join("docs/rfcs/stage-1/10186-identity-preserving-verifier.md")
            .exists(),
        "verifier must not recreate the RFC with a new number"
    );
    assert_eq!(
        after.matches("<!-- exo:").count(),
        1,
        "verifier must not nest RFC anchors"
    );
}

#[test]
fn verifier_ignores_empty_evidence_markdown() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    exo_init(root);
    let evidence = root.join("docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md");
    std::fs::create_dir_all(evidence.parent().expect("evidence parent"))
        .expect("create evidence dir");
    std::fs::write(&evidence, "\n").expect("write empty evidence note");

    let reminders = exo::verifiers::run_global_verifiers(root);

    assert!(
        reminders.iter().all(|reminder| {
            reminder.kind != "rfc.empty_file"
                && reminder.details.as_ref().is_none_or(|details| {
                    details["path"]
                        != "docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md"
                })
        }),
        "empty supporting evidence must not be reported as an empty RFC: {reminders:#?}"
    );
}

#[test]
fn verifier_preserves_available_manual_rfc_number_during_repair() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    git_init(root);
    exo_init(root);
    std::fs::create_dir_all(root.join("docs/rfcs/stage-0")).expect("create RFC dir");
    std::fs::write(root.join("docs/rfcs/stage-0/README.md"), "Stage 0 RFCs\n")
        .expect("write tracked RFC dir marker");
    run_git_ok(root, &["add", "."]);
    run_git_ok(
        root,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "init",
        ],
    );

    let manual_path = root.join("docs/rfcs/stage-0/00006-generated-sandboxd-client.md");
    std::fs::create_dir_all(manual_path.parent().expect("manual RFC parent"))
        .expect("create RFC dir");
    std::fs::write(
        &manual_path,
        "# Generated Sandboxd Client\n\nManual RFC body.\n",
    )
    .expect("write manual RFC");
    age_file_for_verifier(&manual_path);

    let reminders = exo::verifiers::run_global_verifiers(root);
    let repair = reminders
        .iter()
        .find(|reminder| reminder.kind == "rfc.manual_file_repaired")
        .expect("manual RFC should be repaired");

    assert!(
        repair.message.contains("RFC 00006"),
        "repair should explain preserved RFC number: {repair:#?}"
    );
    let details = repair.details.as_ref().expect("repair details");
    assert_eq!(details["old_id"], "00006");
    assert_eq!(details["new_id"], "00006");
    assert_eq!(details["reason"], "preserved_visible_filename_number");
    assert_eq!(
        details["old_path"],
        "docs/rfcs/stage-0/00006-generated-sandboxd-client.md"
    );
    assert_eq!(
        details["new_path"],
        "docs/rfcs/stage-0/00006-generated-sandboxd-client.md"
    );

    assert!(
        manual_path.exists(),
        "repaired RFC should keep the visible path"
    );
    let content = std::fs::read_to_string(&manual_path).expect("read repaired RFC");
    assert!(
        content.starts_with("<!-- exo:6 ulid:"),
        "repaired RFC should be Exo-anchored without changing number: {content}"
    );
    assert!(
        !root
            .join("docs/rfcs/stage-0/00007-generated-sandboxd-client.md")
            .exists(),
        "available manual number should not be renumbered"
    );
}

#[test]
fn verifier_preserves_available_manual_rfc_number_from_underscore_filename() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    git_init(root);
    exo_init(root);
    std::fs::create_dir_all(root.join("docs/rfcs/stage-0")).expect("create RFC dir");
    std::fs::write(root.join("docs/rfcs/stage-0/README.md"), "Stage 0 RFCs\n")
        .expect("write tracked RFC dir marker");
    run_git_ok(root, &["add", "."]);
    run_git_ok(
        root,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "init",
        ],
    );

    let manual_path = root.join("docs/rfcs/stage-0/00006_generated-sandboxd-client.md");
    std::fs::write(
        &manual_path,
        "# Generated Sandboxd Client\n\nManual RFC body.\n",
    )
    .expect("write manual RFC");
    age_file_for_verifier(&manual_path);

    let reminders = exo::verifiers::run_global_verifiers(root);
    let repair = reminders
        .iter()
        .find(|reminder| reminder.kind == "rfc.manual_file_repaired")
        .expect("manual RFC should be repaired");

    assert!(
        repair.message.contains("RFC 00006"),
        "repair should explain preserved RFC number: {repair:#?}"
    );
    let details = repair.details.as_ref().expect("repair details");
    assert_eq!(details["old_id"], "00006");
    assert_eq!(details["new_id"], "00006");
    assert_eq!(details["reason"], "preserved_visible_filename_number");
    assert_eq!(
        details["old_path"],
        "docs/rfcs/stage-0/00006_generated-sandboxd-client.md"
    );
    assert_eq!(
        details["new_path"],
        "docs/rfcs/stage-0/00006-generated-sandboxd-client.md"
    );

    assert!(
        !manual_path.exists(),
        "underscore manual path should be replaced with Exo's canonical hyphen path"
    );
    let repaired_path = root.join("docs/rfcs/stage-0/00006-generated-sandboxd-client.md");
    assert!(
        repaired_path.exists(),
        "repaired RFC should keep number 00006"
    );
    let content = std::fs::read_to_string(&repaired_path).expect("read repaired RFC");
    assert!(
        content.starts_with("<!-- exo:6 ulid:"),
        "repaired RFC should be Exo-anchored without changing number: {content}"
    );
    assert!(
        !root
            .join("docs/rfcs/stage-0/00007-generated-sandboxd-client.md")
            .exists(),
        "available underscore manual number should not be renumbered"
    );
}

#[test]
fn verifier_explains_manual_rfc_renumbering_when_number_is_unavailable() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    git_init(root);
    exo_init(root);
    exo_rfc_create(
        root,
        "Existing Number",
        "00006",
        "0",
        "Verifier",
        Some("Existing RFC body."),
    );
    run_git_ok(root, &["add", "."]);
    run_git_ok(
        root,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "init",
        ],
    );

    let manual_path = root.join("docs/rfcs/stage-0/00006-generated-sandboxd-client.md");
    std::fs::write(
        &manual_path,
        "# Generated Sandboxd Client\n\nManual RFC body.\n",
    )
    .expect("write conflicting manual RFC");
    age_file_for_verifier(&manual_path);

    let reminders = exo::verifiers::run_global_verifiers(root);
    let repair = reminders
        .iter()
        .find(|reminder| reminder.kind == "rfc.manual_file_repaired")
        .expect("manual RFC should be repaired");

    assert!(
        repair.message.contains("from RFC 00006 to RFC 00007"),
        "repair should explain renumbering: {repair:#?}"
    );
    let details = repair.details.as_ref().expect("repair details");
    assert_eq!(details["old_id"], "00006");
    assert_eq!(details["new_id"], "00007");
    assert_eq!(details["reason"], "visible_filename_number_already_exists");
    assert_eq!(
        details["old_path"],
        "docs/rfcs/stage-0/00006-generated-sandboxd-client.md"
    );
    assert_eq!(
        details["new_path"],
        "docs/rfcs/stage-0/00007-generated-sandboxd-client.md"
    );

    assert!(
        !manual_path.exists(),
        "conflicting manual path should be removed after renumbered repair"
    );
    assert!(
        root.join("docs/rfcs/stage-0/00007-generated-sandboxd-client.md")
            .exists(),
        "renumbered repair should write the next available RFC"
    );
}
