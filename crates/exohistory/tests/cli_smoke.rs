//! Smoke tests for exohistory CLI
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help_command() {
    let mut cmd = cargo_bin_cmd!("exohistory");
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "mining insights from VSCode Copilot",
        ));
}

#[test]
fn test_index_subcommand_help() {
    let mut cmd = cargo_bin_cmd!("exohistory");
    cmd.args(["index", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Index chat sessions"));
}

#[test]
fn test_tools_subcommand_help() {
    let mut cmd = cargo_bin_cmd!("exohistory");
    cmd.args(["tools", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tool usage statistics"));
}

#[test]
fn test_search_subcommand_help() {
    let mut cmd = cargo_bin_cmd!("exohistory");
    cmd.args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Search sessions by content"));
}
