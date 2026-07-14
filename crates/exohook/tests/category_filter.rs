use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
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

#[test]
fn inline_check_ids_match_discovery_for_workflow_and_hook_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(
        root.join(".config/exo/hooks.toml"),
        r#"version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = [{ command = "rustc --version", category = "observe" }]
"#,
    )
    .unwrap();

    let discovery = cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["discover", "--format", "jsonl", "--lane", "coherence"])
        .output()
        .unwrap();
    assert!(discovery.status.success());
    let discovered_id = jsonl_id(&discovery.stdout, "check", "id");

    let workflow = cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "coherence", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(workflow.status.success());
    let workflow_id = jsonl_id(&workflow.stdout, "check_enqueued", "check_id");

    let hook = cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["validate", "pre_commit", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(hook.status.success());
    let hook_id = jsonl_id(&hook.stdout, "check_enqueued", "check_id");

    assert_eq!(discovered_id, "coherence-inline-0");
    assert_eq!(workflow_id, discovered_id);
    assert_eq!(hook_id, discovered_id);
}

fn jsonl_id(output: &[u8], event_type: &str, id_field: &str) -> String {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["type"] == event_type)
        .and_then(|event| event[id_field].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("missing {id_field} for {event_type} in JSONL output"))
}
