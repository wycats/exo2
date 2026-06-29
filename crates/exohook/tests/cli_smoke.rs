use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;

#[test]
fn help_works() {
    cargo_bin_cmd!("exohook")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validate"));
}

#[test]
fn validate_dry_run_prints_commands() {
    cargo_bin_cmd!("exohook")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cargo clippy"));
}

#[test]
fn validate_unknown_lane_fails() {
    cargo_bin_cmd!("exohook")
        .args(["validate", "nope", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown lane"));
}

#[test]
fn ci_emit_dry_run_works_with_v3_config() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let config_dir = root.join(".config/exo");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("hooks.toml"),
        r#"
version = 3

[workflow.ci]
label = "CI (HEAD)"
checks = ["test", "rust-coverage"]

[check.test]
label = "Test"
command = "pnpm -r run test:unit"

[check."rust-coverage"]
label = "Rust Coverage"
command = "cargo llvm-cov --workspace --lcov --output-path lcov.info"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["ci", "emit", "github-actions", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Source: .config/exo/hooks.toml [workflow.ci]",
        ))
        .stdout(predicate::str::contains("rust_coverage:"))
        .stdout(predicate::str::contains("permissions:\n  contents: read"))
        .stdout(predicate::str::contains("persist-credentials: false"));
}
