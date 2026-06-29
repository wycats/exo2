use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
mod test_support;
use test_support::{fs, git, git_output};

#[test]
fn parallel_lane_runs_all_and_groups_output_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    let (run_a, run_b) = if cfg!(windows) {
        ("echo A & exit /B 1", "echo B & exit /B 1")
    } else {
        ("echo A; sleep 0.2; exit 1", "echo B; sleep 0.1; exit 1")
    };
    let hooks = format!(
        r#"version = 2

[lane.gate]
scope = {{ op = "base", base = "head" }}
parallel = true
checks = ["a", "b"]

[check.a]
label = "A"
input_mode = "none"
run = "{run_a}"

[check.b]
label = "B"
input_mode = "none"
run = "{run_b}"
"#
    );
    fs::write(root.join(".config/exo/hooks.toml"), &hooks).unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "gate", "--format", "grouped"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("==> a (A)"))
        .stdout(predicate::str::contains("A"))
        .stdout(predicate::str::contains("==> b (B)"))
        .stdout(predicate::str::contains("B"));
}

#[test]
fn autofix_restage_updates_index_and_enforces_containment() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    git(root, &["init"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);

    fs::write(root.join("a.txt"), "hello\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-m", "init"]);

    // Stage a change so coherence scope (staged) includes a.txt
    fs::write(root.join("a.txt"), "hello\nchange\n").unwrap();
    git(root, &["add", "a.txt"]);

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 2

[lane.coherence]
scope = { op = "base", base = "staged" }
parallel = true
checks = ["fix"]

[lane.coherence.overrides.fix]
restage = "auto"
restage_containment = "fail"

[check.fix]
label = "Fix"
input_mode = "none"
autofix = true
run = "echo fmt >> a.txt"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "coherence"])
        .assert()
        .success();

    // Worktree should be clean after restage.
    let unstaged = git_output(root, &["diff", "--name-only"]);
    assert!(unstaged.trim().is_empty());

    // Index version should include the appended line.
    let staged = git_output(root, &["show", ":a.txt"]);
    assert!(staged.contains("fmt"));

    // Prepare containment test:
    // - Commit current staged state so we can stage a fresh lane scope.
    // - Create a tracked, clean file outside lane scope.
    git(root, &["commit", "-m", "after restage"]);

    fs::write(root.join("b.txt"), "base\n").unwrap();
    git(root, &["add", "b.txt"]);
    git(root, &["commit", "-m", "add b"]);

    // Stage a new change so coherence scope includes only a.txt.
    fs::write(root.join("a.txt"), "hello\nchange\nfmt\nmore\n").unwrap();
    git(root, &["add", "a.txt"]);
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 2

[lane.coherence]
scope = { op = "base", base = "staged" }
checks = ["bad"]

[lane.coherence.overrides.bad]
restage = "auto"
restage_containment = "fail"

[check.bad]
label = "Bad"
input_mode = "none"
autofix = true
run = "echo outside >> b.txt"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "coherence"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("containment"));
}

#[test]
fn compact_format_hides_success_stdout_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 2

[lane.gate]
scope = { op = "base", base = "head" }
checks = ["ok"]

[check.ok]
label = "OK"
input_mode = "none"
run = "echo SHOULD_NOT_PRINT"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "gate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("gate: all 1 checks passed"))
        .stdout(predicate::str::contains("SHOULD_NOT_PRINT").not());
}
