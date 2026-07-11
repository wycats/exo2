#![allow(clippy::disallowed_methods)] // integration tests use real fs/process APIs

//! Machine channel coverage test for RFC 0135 Step 14.
//!
//! This test ensures every operation defined in the command spec can be invoked
//! through the in-process machine channel without returning an `unknown_address` error.
//!
//! Expected statuses per operation:
//! - "ok" - Operation succeeded
//! - "confirm_required" - Operation needs confirmation (Exec effect)
//! - "needs_input" - Operation needs more input
//! - "error" with code "invalid_input" - Operation found but args invalid (acceptable)
//!
//! The ONLY failure case is "error" with code "unknown_address" which indicates
//! the transport abstraction is broken and the operation wasn't routed correctly.

#[macro_use]
mod test_support;

use exo::api::protocol::{
    Address, CallParams, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope, Status,
};
use exo::command::command_spec::CommandSpec;
use exo::project::ProjectResolver;
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use std::process::Command;
use test_case::test_matrix;

use test_support::exo_init_with_storage;
use test_support::run_machine_channel_in_process_with_project_as_writer;

#[derive(Debug, Clone)]
struct OperationCase {
    path: Vec<String>,
    is_root: bool,
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

fn load_command_spec() -> JsonValue {
    let path = repo_root().join("packages/exosuit-vscode/src/command-spec.json");
    let raw = ok_or_return!(std::fs::read_to_string(&path), "expected command spec to load";
        JsonValue::Null
    );
    ok_or_return!(serde_json::from_str(&raw), "expected valid command spec json";
        JsonValue::Null
    )
}

fn setup_minimal_fixture(backend: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();

    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("failed to git init");
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(root)
        .output()
        .expect("failed to set git user.email");
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(root)
        .output()
        .expect("failed to set git user.name");

    exo_init_with_storage(root, backend);

    std::fs::create_dir_all(root.join("scripts")).expect("create scripts");

    std::fs::write(
        root.join("scripts/verify-phase.sh"),
        "#!/usr/bin/env bash\nexit 0\n",
    )
    .expect("write scripts/verify-phase.sh");

    temp
}

fn collect_operations() -> (Vec<OperationCase>, usize, usize, Option<u64>) {
    let mut operations = Vec::new();
    let spec_json = load_command_spec();
    let spec: CommandSpec = ok_or_return!(
        serde_json::from_value(spec_json),
        "expected command spec json to deserialize";
        (operations, 0, 0, None)
    );

    let mut root_count = 0;
    let mut namespaced_count = 0;
    for (ns_name, op_name, _op) in spec.iter_all_operations() {
        let (path, is_root) = if ns_name.is_empty() {
            (vec![op_name.to_string()], true)
        } else {
            (vec![ns_name.to_string(), op_name.to_string()], false)
        };

        if is_root {
            root_count += 1;
        } else {
            namespaced_count += 1;
        }

        operations.push(OperationCase { path, is_root });
    }

    operations.sort_by(|a, b| a.path.join(".").cmp(&b.path.join(".")));

    let expected_total = spec.operation_count.map(|count| count as u64);

    (operations, namespaced_count, root_count, expected_total)
}

fn build_request(op: &OperationCase, fixture_root: &std::path::Path) -> RequestEnvelope {
    let id = if op.is_root {
        format!("coverage-test-{}", op.path.join("."))
    } else {
        format!("coverage-test-{}", op.path.join("."))
    };

    // Sidecar operations default their root to the real `$HOME/exo/sidecars`
    // when invoked with empty input. This test runs the handler in-process
    // (no env isolation), so pin sidecar state inside the fixture to keep
    // coverage runs from leaking into the developer's sidecar repo.
    let input = if op.path.first().is_some_and(|ns| ns == "sidecar") {
        json!({
            "key": "coverage-sidecar",
            "root": fixture_root.join("coverage-sidecars").to_string_lossy(),
        })
    } else {
        json!({})
    };

    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: op.path.clone(),
            },
            input,
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

fn real_projects_config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config_home| config_home.join("exo/projects.toml"))
}

fn read_optional(path: &std::path::Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

#[test_matrix(["sqlite"])]
fn machine_channel_coverage_all_operations(backend: &str) {
    let real_policy_path = real_projects_config_path();
    let real_policy_before = real_policy_path.as_deref().and_then(read_optional);
    let fixture = setup_minimal_fixture(backend);
    let root = fixture.path();
    let fixture_home = root.join("home");
    let fixture_config_home = root.join("config");
    std::fs::create_dir_all(&fixture_home).expect("create fixture home");
    std::fs::create_dir_all(&fixture_config_home).expect("create fixture config home");
    let fixture_project = ProjectResolver::default()
        .with_home_dir(&fixture_home)
        .with_config_home(&fixture_config_home)
        .resolve(root)
        .expect("resolve fixture project");
    let (operations, namespaced_count, root_count, expected_total) = collect_operations();

    // Keep this in sync with the generated command spec.
    assert_eq!(namespaced_count, 121, "expected 121 namespaced operations");
    assert_eq!(root_count, 4, "expected 4 root operations");

    if let Some(total) = expected_total {
        assert_eq!(operations.len() as u64, total, "operation count mismatch");
        assert_eq!(
            total, 125,
            "expected command spec operation_count to be 125"
        );
    } else {
        assert_eq!(operations.len(), 125, "expected 125 total operations");
    }

    let mut failures = Vec::new();

    for op in operations {
        let request = build_request(&op, root);
        let resp = run_machine_channel_in_process_with_project_as_writer(
            root,
            Some(&fixture_project),
            &request,
        );

        let status = resp.status;
        let error_code = resp.error.as_ref().map(|e| e.code);

        // The ONLY failure case is "unknown_address" - that means the transport
        // abstraction is broken and the operation wasn't routed correctly.
        // All other responses (ok, confirm_required, needs_input, invalid_input)
        // indicate the operation was found and dispatched successfully.
        if status == Status::Error && error_code == Some(ErrorCode::UnknownAddress) {
            failures.push(format!(
                "{} -> status=error code=unknown_address (TRANSPORT BUG)",
                op.path.join(".")
            ));
        }
        // Log but don't fail on other statuses for debugging
        let error_code_text = error_code.map(|c| format!("{:?}", c)).unwrap_or_default();
        eprintln!(
            "{} -> status={} {}",
            op.path.join("."),
            format!("{:?}", status).to_lowercase(),
            if error_code_text.is_empty() {
                String::new()
            } else {
                format!("code={}", error_code_text.to_lowercase())
            }
        );
    }

    assert!(
        failures.is_empty(),
        "machine channel coverage failures:\n{}",
        failures.join("\n")
    );
    let fixture_policy = std::fs::read_to_string(fixture_config_home.join("exo/projects.toml"))
        .expect("fixture policy should receive coverage sidecar writes");
    assert!(
        fixture_policy.contains("coverage-sidecar"),
        "coverage sidecar policy should be written under fixture config"
    );
    if let Some(path) = real_policy_path {
        assert_eq!(
            read_optional(&path),
            real_policy_before,
            "machine channel coverage must not modify real project policy at {}",
            path.display()
        );
    }
}
