use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
mod test_support;
use test_support::fs;
use toml_edit::{DocumentMut, Item};

#[test]
fn config_init_creates_file_and_validate_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "init"])
        .assert()
        .success();

    let path = root.join(".config/exo/hooks.toml");
    assert!(path.exists());

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "validate"])
        .assert()
        .success();
}

#[test]
fn config_set_check_updates_value() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "init"])
        .assert()
        .success();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args([
            "config",
            "set-check",
            "--id",
            "rust-clippy",
            "argv",
            "--toml",
            "[\"cargo\",\"clippy\"]",
        ])
        .assert()
        .success();

    let path = root.join(".config/exo/hooks.toml");
    let content = fs::read_to_string(path).unwrap();
    let doc = content.parse::<DocumentMut>().unwrap();
    let check_root = doc.get("check").and_then(Item::as_table).unwrap();
    let rust_clippy = check_root
        .get("rust-clippy")
        .and_then(Item::as_table)
        .unwrap();
    let argv = rust_clippy.get("argv").and_then(Item::as_array).unwrap();
    let parts: Vec<&str> = argv.iter().map(|i| i.as_str().unwrap()).collect();
    assert_eq!(parts, vec!["cargo", "clippy"]);

    // Still validates.
    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "validate"])
        .assert()
        .success();
}

#[test]
fn validate_rejects_paths_check_without_files_placeholder() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "init"])
        .assert()
        .success();

    // Add a broken paths-based check.
    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args([
            "config",
            "set-check",
            "--id",
            "lint",
            "input_mode",
            "--toml",
            "\"paths\"",
        ])
        .assert()
        .success();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "validate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("lacks '{{files}}'"));
}
