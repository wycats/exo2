//! CLI dispatch coverage tests - safety net before CLITransport refactor.
//!
//! These tests verify that all namespaces have at least one working command
//! and that `--format json` produces valid JSON output.

mod test_support;

use std::path::PathBuf;
use std::process::Output;
use test_case::test_matrix;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

fn run_exo(backend: &str, args: &[&str]) -> Output {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    // Use --direct to bypass daemon (prevents test contamination from
    // daemon processes left by other test suites)
    cmd.current_dir(repo_root())
        .arg("--direct")
        .args(["--storage", backend])
        .args(args);
    cmd.output().expect("Failed to execute exo command")
}

fn run_exo_raw(args: &[&str]) -> Output {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(repo_root()).args(args);
    cmd.output().expect("Failed to execute exo command")
}

fn exo_cli(root: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    let home = test_support::test_home();
    cmd.current_dir(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"));
    cmd
}

fn assert_no_panic(output: &Output, name: &str) {
    let code = output.status.code();
    assert!(code.is_some(), "{name} terminated by signal");
    assert_ne!(code, Some(101), "{name} exited with panic");
}

fn assert_valid_json(name: &str, output: &str) {
    serde_json::from_str::<serde_json::Value>(output).unwrap_or_else(|err| {
        panic!("{name} output should be valid JSON: {err}\n{output}");
    });
}

#[test]
fn approved_flag_is_not_a_completion_option() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    test_support::exo_init_with_storage(root, "sqlite");
    test_support::exo_phase_start_with_storage(root, "sqlite");
    for direct in [false, true] {
        let mut command = exo_cli(root);
        if direct {
            command.arg("--direct");
        }
        command
            .args(["task", "complete", "missing", "--approved"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("Unknown flag '--approved'"));
    }
}

struct DispatchCase {
    name: &'static str,
    args: Vec<&'static str>,
    expect_json: bool,
    require_success: bool,
}

#[test_matrix(["sqlite"])]
fn namespace_dispatches_without_panic(backend: &str) {
    let cases = vec![
        DispatchCase {
            name: "plan",
            args: vec!["--format", "json", "plan", "health"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "phase",
            args: vec!["--format", "json", "phase", "status"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "task",
            args: vec!["--format", "json", "task", "list"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "epoch",
            args: vec!["--format", "json", "epoch", "list"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "idea",
            args: vec!["idea", "list"],
            expect_json: false,
            require_success: false,
        },
        DispatchCase {
            name: "inbox",
            args: vec!["--format", "json", "inbox", "list"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "rfc",
            args: vec!["--format", "json", "rfc", "status"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "axiom",
            args: vec!["axiom", "list", "--scope", "workflow"],
            expect_json: false,
            require_success: false,
        },
        DispatchCase {
            name: "strike",
            args: vec!["strike", "--help"],
            expect_json: false,
            require_success: true,
        },
        DispatchCase {
            name: "tdd",
            args: vec!["tdd", "--help"],
            expect_json: false,
            require_success: true,
        },
        DispatchCase {
            name: "commit",
            args: vec!["--format", "json", "commit", "status"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "ai",
            args: vec!["ai", "context"],
            expect_json: false,
            require_success: false,
        },
        DispatchCase {
            name: "context",
            args: vec!["--format", "json", "context", "paths"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "json",
            args: vec!["--format", "json", "json", "spec"],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "toml",
            args: vec![
                "--format",
                "json",
                "toml",
                "read",
                "docs/agent-context/plan.toml",
                "--key",
                "epochs",
            ],
            expect_json: true,
            require_success: false,
        },
        DispatchCase {
            name: "map",
            args: vec!["map", "--json"],
            expect_json: true,
            require_success: false,
        },
    ];

    for case in cases {
        let output = run_exo(backend, &case.args);
        assert_no_panic(&output, case.name);

        if case.require_success {
            assert!(
                output.status.success(),
                "{name} should succeed",
                name = case.name
            );
        }

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

            if case.expect_json {
                assert!(
                    !stdout.is_empty(),
                    "{name} returned empty stdout",
                    name = case.name
                );
                assert_valid_json(case.name, &stdout);
            } else {
                assert!(
                    !stdout.is_empty() || !stderr.is_empty(),
                    "{name} returned no output",
                    name = case.name
                );
            }
        }
    }
}

#[test_matrix(["sqlite"])]
fn help_command_works(backend: &str) {
    let output = run_exo(backend, &["--help"]);
    assert_no_panic(&output, "help");
    assert!(output.status.success(), "help command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exo"));
}

#[test]
fn command_text_help_forms_route_through_shared_frontend() {
    let help_task = run_exo_raw(&["help", "task"]);
    assert_no_panic(&help_task, "help task");
    assert!(help_task.status.success(), "help task should succeed");
    let stdout = String::from_utf8_lossy(&help_task.stdout);
    assert!(stdout.contains("task complete"), "{stdout}");

    let rfc_json = run_exo_raw(&["rfc", "promote", "--help", "--format", "json"]);
    assert_no_panic(&rfc_json, "rfc promote help json");
    assert!(rfc_json.status.success(), "rfc promote help should succeed");
    let stdout = String::from_utf8_lossy(&rfc_json.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("rfc promote help should be JSON");
    assert_eq!(json["operations"][0]["path"], "rfc promote");
    assert_eq!(json["operations"][0]["args"][0]["name"], "id");
    assert_eq!(json["operations"][0]["args"][1]["name"], "stage");
    assert_eq!(json["operations"][0]["args"][1]["value_type"], "int");
}

#[test]
fn special_command_help_works_without_loading_context() {
    let init = run_exo_raw(&["init", "--help"]);
    assert_no_panic(&init, "init help");
    assert!(init.status.success(), "init help should succeed");
    let stdout = String::from_utf8_lossy(&init.stdout);
    assert!(stdout.contains("init"), "{stdout}");
    assert!(stdout.contains("--defaults"), "{stdout}");

    let daemon = run_exo_raw(&[
        "daemon",
        "ensure",
        "--workspace",
        "/tmp/exo-help-target",
        "--help",
    ]);
    assert_no_panic(&daemon, "daemon ensure help");
    assert!(daemon.status.success(), "daemon ensure help should succeed");
    let stdout = String::from_utf8_lossy(&daemon.stdout);
    assert!(stdout.contains("daemon ensure"), "{stdout}");
    assert!(stdout.contains("--workspace"), "{stdout}");

    let merge_driver = run_exo_raw(&["merge-driver", "--help"]);
    assert_no_panic(&merge_driver, "merge-driver help");
    assert!(
        merge_driver.status.success(),
        "merge-driver help should succeed"
    );
    let stdout = String::from_utf8_lossy(&merge_driver.stdout);
    assert!(stdout.contains("merge-driver"), "{stdout}");
    assert!(stdout.contains("path"), "{stdout}");
    assert!(stdout.contains("optional"), "{stdout}");
}

#[test_matrix(["sqlite"])]
fn version_command_works(backend: &str) {
    let output = run_exo(backend, &["--version"]);
    assert_no_panic(&output, "version");
    assert!(output.status.success(), "version command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exo"));
}
