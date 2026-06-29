use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

mod test_support;
use test_support::{fs, git};

#[test]
fn validate_v3_hook_dry_run_uses_staged_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::write(root.join("a.txt"), "hello\n").unwrap();
    git(root, &["add", "a.txt"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["fmt"]

[check.fmt]
command = "echo {{files}}"
filters = ["**/*.txt"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "pre_commit", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("echo a.txt"));
}

#[test]
fn validate_v3_tool_dry_run_shows_address() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    // Create a staged .md file so the filter matches
    fs::write(root.join("README.md"), "# Hello\n").unwrap();
    git(root, &["add", "README.md"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["verify-links"]

[check.verify-links]
tool = "exo.docs.links.check"
filters = ["**/*.md"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "pre_commit", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "tool: verify-links -> docs.links.check",
        ));
}

#[test]
fn validate_v3_skip_if_empty_default_skips_check() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::write(root.join("a.txt"), "hello\n").unwrap();
    git(root, &["add", "a.txt"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["fmt"]

[check.fmt]
command = "echo {{files}}"
filters = ["**/*.rs"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "pre_commit", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped (no matches)"))
        .stdout(predicate::str::contains("echo").not());
}

#[test]
fn validate_v3_skip_if_empty_false_runs_check() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::write(root.join("a.txt"), "hello\n").unwrap();
    git(root, &["add", "a.txt"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["fmt"]

[check.fmt]
command = "echo {{files}}"
filters = ["**/*.rs"]
skip_if_empty = false
"#,
    )
    .unwrap();

    let shell_prefix = if cfg!(windows) {
        "cmd.exe /C echo"
    } else {
        "bash -lc echo"
    };

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "pre_commit", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(shell_prefix))
        .stdout(predicate::str::contains("skipped (no matches)").not());
}

#[test]
fn run_workflow_basic() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[workflow.test]
label = "Test workflow"
checks = ["echo"]

[check.echo]
command = "echo hello"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["run", "test", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("echo hello"));
}

#[test]
fn run_workflow_not_found() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3
    "#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["run", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("workflow 'nonexistent' not found"));
}
