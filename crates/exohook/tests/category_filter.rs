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
