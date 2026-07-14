use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
mod test_support;
use test_support::fs;

#[test]
fn category_filter_runs_only_matching_v3_checks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[workflow.dev]
checks = ["inspect", "rewrite"]
scope = "all"

[check.inspect]
command = "rustc --version"
category = "observe"

[check.rewrite]
command = "rustc --definitely-invalid-flag"
category = "mutate"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "dev", "--category", "observe"])
        .assert()
        .success()
        .stdout(predicate::str::contains("all 1 checks passed"));

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "dev"])
        .assert()
        .failure();
}

#[test]
fn category_filter_requires_a_lane() {
    cargo_bin_cmd!("exohook")
        .args(["validate", "--category", "observe"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--category requires a validation lane",
        ));
}

#[test]
fn category_filter_points_v1_and_v2_projects_to_the_migration_command() {
    for version in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join(".config/exo")).unwrap();
        let config = format!(
            r#"version = {version}

[lane.gate]
scope = {{ op = "base", base = "head" }}
checks = ["inspect"]

[check.inspect]
input_mode = "none"
run = "rustc --version"
"#
        );
        fs::write(root.join(".config/exo/hooks.toml"), &config).unwrap();

        cargo_bin_cmd!("exohook")
            .current_dir(root)
            .args(["validate", "gate", "--category", "observe"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("exohook migrate v3 --in-place"));
    }
}

#[test]
fn category_filter_points_bootstrap_projects_to_the_migration_command() {
    let temp = tempfile::tempdir().unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(temp.path())
        .args(["validate", "gate", "--category", "observe"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("exohook migrate v3 --in-place"));
}
