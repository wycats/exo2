use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
mod test_support;
use test_support::fs;
use toml_edit::{DocumentMut, Item};

#[test]
fn migrate_lefthook_writes_valid_hooks_toml() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    // Minimal lefthook.yml fixture
    fs::write(
        root.join("lefthook.yml"),
        r#"pre-commit:
  parallel: true
  commands:
    check:
      run: pnpm -r run check
    lint:
      run: pnpm -r run lint
    rust-fmt:
      run: cargo fmt --all
      stage_fixed: true
    rust-clippy:
      run: cargo clippy --workspace -- -D warnings
    verify-toml:
      run: pnpm run verify:toml

pre-push:
  parallel: true
  commands:
    test:
      run: pnpm -r run test:unit
    rust-coverage:
      run: cargo llvm-cov --workspace --lcov --output-path lcov.info
"#,
    )
    .unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["migrate", "lefthook"])
        .assert()
        .success();

    let out_path = root.join(".config/exo/hooks.toml");
    assert!(out_path.exists());

    let report_path = root.join(".config/exo/migrate-lefthook.report.txt");
    assert!(report_path.exists());

    // Validate the generated config.
    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["config", "validate"])
        .assert()
        .success();

    // Spot-check: coherence lane exists and rust-fmt is marked mutate.
    let content = fs::read_to_string(out_path).unwrap();
    let doc = content.parse::<DocumentMut>().unwrap();
    assert_eq!(doc["version"].as_integer(), Some(2));

    let lane_root = doc.get("lane").and_then(Item::as_table).unwrap();
    let coherence = lane_root.get("coherence").and_then(Item::as_table).unwrap();
    let gate = lane_root.get("gate").and_then(Item::as_table).unwrap();

    assert_eq!(
        coherence.get("parallel").and_then(Item::as_bool),
        Some(true)
    );
    assert_eq!(gate.get("parallel").and_then(Item::as_bool), Some(true));

    let check_root = doc.get("check").and_then(Item::as_table).unwrap();
    let rust_fmt = check_root.get("rust-fmt").and_then(Item::as_table).unwrap();
    assert_eq!(
        rust_fmt.get("category").and_then(Item::as_str),
        Some("mutate")
    );

    // Spot-check: pre_commit projection points at coherence.
    assert!(content.contains("pre_commit = \"coherence\""));

    // Ensure stage_fixed induced coherence override restage.
    let overrides = coherence.get("overrides").and_then(Item::as_table).unwrap();
    let rust_fmt_ov = overrides.get("rust-fmt").and_then(Item::as_table).unwrap();
    assert_eq!(
        rust_fmt_ov.get("restage").and_then(Item::as_str),
        Some("auto")
    );

    // Ensure run commands are preserved.
    assert_eq!(
        rust_fmt.get("run").and_then(Item::as_str),
        Some("cargo fmt --all")
    );

    let report = fs::read_to_string(report_path).unwrap();
    assert!(report.contains("exohook migrate lefthook report"));
    assert!(report.contains("Lane mapping:"));
    assert!(!report.contains("parallel:` is not represented"));
}

#[test]
fn migrate_lefthook_refuses_to_overwrite_without_force() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join(".config/exo")).unwrap();
    fs::write(root.join(".config/exo/hooks.toml"), "version = 1\n").unwrap();
    fs::write(root.join("lefthook.yml"), "pre-commit: {}\n").unwrap();

    cargo_bin_cmd!("exohook")
        .current_dir(root)
        .args(["migrate", "lefthook"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    let report_path = root.join(".config/exo/migrate-lefthook.report.txt");
    assert!(report_path.exists());
    let report = fs::read_to_string(report_path).unwrap();
    assert!(report.contains("output already exists"));
}
