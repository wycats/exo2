use exo::api::protocol::{ErrorCode, NextCallKind, Priority};
use exo::preload_guidance::classify_context_load_error;
use std::fs;
use std::path::Path;
use std::process::Command;

#[macro_use]
mod test_support;

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

fn exo_cmd_with_home(root: &Path, home: &Path, config_home: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home);
    cmd
}

fn write_legacy_rfc(root: &Path) {
    let rfc_path = root.join("docs/rfcs/stage-1/0001-legacy-rfc.md");
    assert!(fs::create_dir_all(rfc_path.parent().expect("rfc parent")).is_ok());
    assert!(
        fs::write(
            &rfc_path,
            "---\ntitle: Legacy RFC\nulid: 01H00000000000000000000000\n---\n\n# RFC 0001: Legacy RFC\n\nA legacy RFC without an anchor.\n",
        )
        .is_ok()
    );
}

fn missing_anchor_error() -> anyhow::Error {
    anyhow::anyhow!("RFC file missing anchor comment: docs/rfcs/stage-1/0001-legacy-rfc.md")
        .context("Failed to reconcile RFC metadata from disk into SQLite")
}

#[test]
fn classifies_missing_rfc_anchor_as_migration_blocker() {
    let err = missing_anchor_error();
    let guidance = classify_context_load_error(&err, "exo status")
        .expect("missing RFC anchor should produce preload guidance");

    assert_eq!(
        guidance.classification,
        "migration_blocked:rfc_metadata_anchor"
    );
    assert_eq!(guidance.error_code, ErrorCode::PreconditionFailed);
    assert_eq!(guidance.next_command, "exo update");
    assert_eq!(guidance.retry_command.as_deref(), Some("exo status"));
    assert_eq!(
        guidance.diagnostic_command.as_deref(),
        Some("exo rfc status")
    );
    assert_eq!(
        guidance.affected_path.as_deref(),
        Some("docs/rfcs/stage-1/0001-legacy-rfc.md")
    );
    assert!(guidance.message().contains("legacy RFC metadata migration"));
}

#[test]
fn builds_blocking_machine_steering_for_migration_blocker() {
    let err = missing_anchor_error();
    let guidance = classify_context_load_error(&err, "exo status")
        .expect("missing RFC anchor should produce preload guidance");
    let steering = guidance.to_steering();

    assert_eq!(steering.next_call.kind, NextCallKind::Call);
    assert_eq!(steering.priority, Some(Priority::Blocking));
    assert_eq!(steering.confidence, Some(1.0));
    assert_eq!(
        steering.next_call.params["address"]["path"],
        serde_json::json!(["update"])
    );
    assert!(
        steering
            .context_note
            .as_deref()
            .unwrap_or_default()
            .contains("rerun exo status")
    );
}

#[test]
fn includes_structured_error_details() {
    let err = missing_anchor_error();
    let guidance = classify_context_load_error(&err, "exo status")
        .expect("missing RFC anchor should produce preload guidance");
    let details = guidance.details();

    assert_eq!(
        details["classification"],
        "migration_blocked:rfc_metadata_anchor"
    );
    assert_eq!(details["subsystem"], "rfc metadata reconciliation");
    assert_eq!(details["cause"], "legacy RFC file missing anchor comment");
    assert_eq!(
        details["affected_path"],
        "docs/rfcs/stage-1/0001-legacy-rfc.md"
    );
    assert_eq!(details["next_command"], "exo update");
    assert_eq!(details["retry_command"], "exo status");
    assert_eq!(details["diagnostic_command"], "exo rfc status");
}

#[test]
fn leaves_unknown_rfc_reconciliation_failures_unclassified() {
    let err = anyhow::anyhow!("unexpected RFC parse failure: docs/rfcs/stage-1/bad.md")
        .context("Failed to reconcile RFC metadata from disk into SQLite");

    assert!(classify_context_load_error(&err, "exo status").is_none());
}

#[test]
fn json_status_with_legacy_rfc_anchor_succeeds() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );
    write_legacy_rfc(root);

    let assert = test_support::exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success();
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );
    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected json status"
    );

    assert_eq!(value["status"], "ok");
}

#[test]
fn human_rfc_status_surfaces_legacy_rfc_repair_debt() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(
        fs::write(
            root.join("exosuit.toml"),
            "[storage]\nbackend = \"sqlite\"\n"
        )
        .is_ok()
    );
    write_legacy_rfc(root);

    let assert = test_support::exo_cmd(root)
        .args(["rfc", "status"])
        .assert()
        .success();
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );

    assert!(stdout.contains("RFC Identity Repairs"), "{stdout}");
    assert!(stdout.contains("docs/rfcs/stage-1/0001-legacy-rfc.md"));
    assert!(stdout.contains("exo rfc repair 0001"));
}

#[test]
fn sidecar_legacy_rfc_fixture_surfaces_repair_debt() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let repo = temp.path().join("locald-like-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    assert!(fs::create_dir_all(&repo).is_ok());
    git_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "locald",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    write_legacy_rfc(&repo);
    assert!(!repo.join("exosuit.toml").exists());

    let sidecar_status_output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sidecar_status: serde_json::Value = ok_or_return!(
        serde_json::from_slice(&sidecar_status_output),
        "expected sidecar status json"
    );
    assert_eq!(sidecar_status["status"], "ok");

    let status_output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = ok_or_return!(
        serde_json::from_slice(&status_output),
        "expected status json"
    );

    assert_eq!(status["status"], "ok");

    let rfc_status = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "rfc", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rfc_status = ok_or_return!(String::from_utf8(rfc_status), "expected rfc status stdout");
    assert!(rfc_status.contains("RFC Identity Repairs"), "{rfc_status}");
    assert!(rfc_status.contains("docs/rfcs/stage-1/0001-legacy-rfc.md"));
    assert!(rfc_status.contains("exo rfc repair 0001"));
}

#[test]
fn sidecar_legacy_rfc_repair_flow_then_status_succeeds() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let repo = temp.path().join("locald-like-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    assert!(fs::create_dir_all(&repo).is_ok());
    git_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "locald",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    write_legacy_rfc(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "status"])
        .assert()
        .success();

    let repair_output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "rfc", "repair", "0001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let repair: serde_json::Value = ok_or_return!(
        serde_json::from_slice(&repair_output),
        "expected repair json"
    );
    assert_eq!(repair["status"], "ok");

    let migrated = ok_or_return!(
        fs::read_to_string(repo.join("docs/rfcs/stage-1/0001-legacy-rfc.md")),
        "expected migrated RFC"
    );
    assert!(migrated.starts_with("<!-- exo:1 ulid:"), "{migrated}");

    let status_output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = ok_or_return!(
        serde_json::from_slice(&status_output),
        "expected status json after update"
    );
    assert_eq!(status["status"], "ok");
    assert!(!repo.join("exosuit.toml").exists());
    assert!(
        sidecar_root
            .join("projects/locald/agent-context/rfcs.sql")
            .exists(),
        "expected sidecar RFC SQL projection after recovery"
    );
}
