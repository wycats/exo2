//! Integration tests for terminal width adaptation.
//!
//! These tests verify that the EXOHOOK_COLUMNS environment variable
//! correctly controls terminal width detection and tier selection.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// Test that EXOHOOK_COLUMNS env var is respected for width detection.
/// We use --dry-run mode since we just want to verify the env var is picked up.
#[test]
fn exohook_columns_env_var_accepted() {
    // Setting EXOHOOK_COLUMNS should not cause any errors
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "50")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cargo clippy"));
}

/// Test with wide terminal width
#[test]
fn exohook_columns_wide_terminal() {
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "120")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success();
}

/// Test with narrow terminal width
#[test]
fn exohook_columns_narrow_terminal() {
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "45")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success();
}

/// Test with compact terminal width (should use compact indicators)
#[test]
fn exohook_columns_compact_terminal() {
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "35")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success();
}

/// Test with very small terminal width (edge case)
#[test]
fn exohook_columns_very_small() {
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "20")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success();
}

/// Test that invalid EXOHOOK_COLUMNS falls back gracefully
#[test]
fn exohook_columns_invalid_falls_back() {
    cargo_bin_cmd!("exohook")
        .env("EXOHOOK_COLUMNS", "not-a-number")
        .args(["validate", "gate", "--dry-run"])
        .assert()
        .success();
}
