use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;

mod test_support;
use test_support::git;

#[test]
fn extract_single_inline_check() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = [{ command = "cargo fmt --check", label = "Format" }]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "extract"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Extracted 1 inline check"));

    let content = fs::read_to_string(root.join(".config/exo/hooks.toml")).unwrap();
    assert!(
        content.contains("[check.format]")
            || content.contains("[check.inline-0]")
            || content.contains("[check.inline-format]")
    );
    assert!(content.contains("pre_commit = \"coherence\""));
    assert!(
        content.contains("checks = [\"format\"")
            || content.contains("checks = [\"inline-0\"")
            || content.contains("checks = [\"inline-format\"")
    );
}

#[test]
fn extract_dry_run_does_not_modify() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    let original = r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = [{ command = "cargo fmt" }]
"#;

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(root.join(".config/exo/hooks.toml"), original).unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "extract", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Extracted inline check"));

    let content = fs::read_to_string(root.join(".config/exo/hooks.toml")).unwrap();
    assert_eq!(content, original);
}

#[test]
fn extract_no_inlines_is_noop() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["fmt"]

[check.fmt]
command = "cargo fmt"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "extract"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No inline checks found"));
}
