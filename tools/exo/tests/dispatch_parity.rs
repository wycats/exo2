#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

#[allow(dead_code)]
mod support;

use exo::api::protocol::{
    Address, CallParams, ErrorBody, Op, PROTOCOL_VERSION, RequestEnvelope, Status,
};
use exo::command::command_spec::CommandSpec;
use exo::command::registry::default_registry;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use test_case::test_matrix;
use test_support::{
    exo_active_phase_id, exo_init_with_storage, exo_plan_update_status_with_storage,
};

static REPO_ROOT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
struct OperationCase {
    path: Vec<String>,
    is_root: bool,
}

#[derive(Debug, Clone)]
struct ParityEnvelope {
    status: Status,
    result: Option<serde_json::Value>,
    error: Option<ErrorBody>,
}

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

fn with_repo_root<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = REPO_ROOT_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("repo-root test lock");
    let root = repo_root();
    f(&root)
}

fn load_command_spec() -> CommandSpec {
    let registry = default_registry();
    CommandSpec::from_registry(&registry)
}

fn parse_cli_envelope(stdout: &str) -> ParityEnvelope {
    let value: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("failed to parse cli json output: {err}\nstdout: {stdout}"));

    let status: Status = serde_json::from_value(
        value
            .get("status")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .expect("expected status field");

    let result = value.get("result").cloned();
    let error = value
        .get("error")
        .cloned()
        .map(|val| serde_json::from_value::<ErrorBody>(val).expect("expected error body"));

    ParityEnvelope {
        status,
        result,
        error,
    }
}

fn run_cli_json(root: &Path, args: &[&str]) -> ParityEnvelope {
    let run = support::run_exo_interleaved(root, args);
    let stdout = run.stdout.trim();

    assert!(
        !stdout.is_empty(),
        "expected json stdout, got empty stdout (stderr={})",
        run.stderr.trim()
    );

    parse_cli_envelope(stdout)
}

fn run_cli_json_op(root: &Path, op: &OperationCase, extra_args: &[&str]) -> ParityEnvelope {
    let mut argv: Vec<String> = vec!["--format".to_string(), "json".to_string()];

    if op.is_root {
        argv.push(op.path[0].clone());
    } else {
        argv.push(op.path[0].clone());
        argv.push(op.path[1].clone());
    }

    argv.extend(extra_args.iter().map(|arg| (*arg).to_string()));

    let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run_cli_json(root, &argv_refs)
}

fn run_machine_channel(
    root: &Path,
    op: &OperationCase,
    input: serde_json::Value,
) -> ParityEnvelope {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: format!("dispatch-parity-{}", op.path.join(".")),
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
    };

    let response = test_support::run_machine_channel_in_process(root, &request);
    ParityEnvelope {
        status: response.status,
        result: response.result,
        error: response.error,
    }
}

/// Normalize a JSON value by round-tripping through string serialization.
/// This ensures floating-point values have consistent precision between
/// the CLI path (which goes through JSON string serialization) and the
/// machine channel path (which keeps raw f32/f64 values).
fn normalize_json(value: &serde_json::Value) -> serde_json::Value {
    let json_str = serde_json::to_string(value).expect("failed to serialize json");
    let mut value: serde_json::Value =
        serde_json::from_str(&json_str).expect("failed to deserialize json");
    normalize_volatile_fields(&mut value);
    value
}

fn normalize_volatile_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::Object(session_boundary)) =
                obj.get_mut("session_boundary")
            {
                session_boundary.remove("rationale");
            }

            for child in obj.values_mut() {
                normalize_volatile_fields(child);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                normalize_volatile_fields(child);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn assert_parity(label: &str, cli: &ParityEnvelope, machine: &ParityEnvelope) {
    assert_eq!(
        cli.status, machine.status,
        "{label}: status mismatch (cli={:?}, machine={:?})",
        cli.status, machine.status
    );

    // Normalize both results through JSON string round-trip to handle
    // floating-point precision differences between CLI and machine channel paths.
    let cli_result_normalized = cli.result.as_ref().map(normalize_json);
    let machine_result_normalized = machine.result.as_ref().map(normalize_json);

    assert_eq!(
        cli_result_normalized, machine_result_normalized,
        "{label}: result mismatch\ncli={:?}\nmachine={:?}",
        cli.result, machine.result
    );

    // Compare error code and message.
    // Note: We intentionally skip comparing error.details because the machine channel
    // merges steering into details via `merge_error_details()` in handler.rs, while
    // the CLI path keeps steering as a separate top-level field. This is an intentional
    // transport-level difference, not a semantic difference.
    let cli_error = cli.error.as_ref().map(|e| (&e.code, &e.message));
    let machine_error = machine.error.as_ref().map(|e| (&e.code, &e.message));
    assert_eq!(
        cli_error, machine_error,
        "{label}: error mismatch\ncli={:?}\nmachine={:?}",
        cli.error, machine.error
    );
}

fn assert_parity_for_op(
    root: &Path,
    op: &OperationCase,
    input: serde_json::Value,
    extra_args: &[&str],
) {
    let cli = run_cli_json_op(root, op, extra_args);
    let machine = run_machine_channel(root, op, input);
    assert_parity(&op.path.join("."), &cli, &machine);
}

fn write_plan_no_active_phase(root: &Path, backend: &str) {
    exo_init_with_storage(root, backend);
    // init bootstraps a "Getting Started" epoch with active "Bootstrap" phase.
    // Complete that phase so there's no active phase.
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");
}

fn write_plan_with_active_phase(root: &Path, backend: &str) {
    exo_init_with_storage(root, backend);
    // init already bootstraps an active phase — nothing else needed.
}

#[test_matrix(["sqlite"])]
fn dispatch_parity_status(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    write_plan_with_active_phase(root, backend);

    let op = OperationCase {
        path: vec!["status".to_string()],
        is_root: true,
    };

    assert_parity_for_op(root, &op, json!({}), &[]);
}

#[test]
fn dispatch_parity_map() {
    with_repo_root(|root| {
        let op = OperationCase {
            path: vec!["map".to_string()],
            is_root: true,
        };

        assert_parity_for_op(root, &op, json!({}), &[]);
    });
}

#[test]
fn dispatch_parity_ai_context() {
    with_repo_root(|root| {
        let op = OperationCase {
            path: vec!["ai".to_string(), "context".to_string()],
            is_root: false,
        };

        assert_parity_for_op(root, &op, json!({}), &[]);
    });
}

#[test]
fn dispatch_parity_axiom_list() {
    with_repo_root(|root| {
        let op = OperationCase {
            path: vec!["axiom".to_string(), "list".to_string()],
            is_root: false,
        };

        assert_parity_for_op(root, &op, json!({}), &[]);
    });
}

#[test]
fn dispatch_parity_rfc_not_found() {
    with_repo_root(|root| {
        let op = OperationCase {
            path: vec!["rfc".to_string(), "show".to_string()],
            is_root: false,
        };

        assert_parity_for_op(root, &op, json!({ "id": "99999" }), &["99999"]);
    });
}

#[test_matrix(["sqlite"])]
fn dispatch_parity_no_active_phase(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    write_plan_no_active_phase(root, backend);

    let op = OperationCase {
        path: vec!["task".to_string(), "list".to_string()],
        is_root: false,
    };

    assert_parity_for_op(root, &op, json!({}), &[]);
}

#[test_matrix(["sqlite"])]
fn dispatch_parity_task_not_found(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    write_plan_with_active_phase(root, backend);

    let op = OperationCase {
        path: vec!["task".to_string(), "complete".to_string()],
        is_root: false,
    };

    assert_parity_for_op(
        root,
        &op,
        json!({ "id": "missing-task", "log": "test completion" }),
        &["missing-task", "--log", "test completion"],
    );
}

#[test]
fn dispatch_parity_pure_zero_arg_operations() {
    with_repo_root(|root| {
        let spec = load_command_spec();
        // Skip 'status' because it's tested individually above with more context,
        // skip active-phase reads because they need a controlled workspace,
        // skip 'verify.run' because it runs external scripts, and skip
        // dogfood.verify because it reports transport/runtime activation health.
        // ai.chat-history is transport-sensitive (CLI vs machine channel outputs differ).
        let skip_ops = [
            "verify.run",
            "dogfood.verify",
            "ai.chat-history",
            "phase.read-goals",
            "phase.read-tasks",
        ];

        let mut operations: Vec<OperationCase> = Vec::new();

        for (name, op) in &spec.root_operations {
            if op.effect == exo::api::protocol::Effect::Pure
                && op
                    .args
                    .iter()
                    .all(|arg| arg.optional || arg.default.is_some())
            {
                let path = vec![name.clone()];
                if skip_ops.contains(&path.join(".").as_str()) {
                    continue;
                }
                operations.push(OperationCase {
                    path,
                    is_root: true,
                });
            }
        }

        for (namespace, ns_spec) in &spec.namespaces {
            for (name, op) in &ns_spec.operations {
                if op.effect == exo::api::protocol::Effect::Pure
                    && op
                        .args
                        .iter()
                        .all(|arg| arg.optional || arg.default.is_some())
                {
                    let path = vec![namespace.clone(), name.clone()];
                    if skip_ops.contains(&path.join(".").as_str()) {
                        continue;
                    }
                    operations.push(OperationCase {
                        path,
                        is_root: false,
                    });
                }
            }
        }

        operations.sort_by(|a, b| a.path.join(".").cmp(&b.path.join(".")));

        let mut failures = Vec::new();

        for op in operations {
            let cli = run_cli_json_op(&root, &op, &[]);
            let machine = run_machine_channel(&root, &op, json!({}));

            // Normalize results through JSON round-trip to handle floating-point precision differences
            let cli_result_normalized = cli.result.as_ref().map(normalize_json);
            let machine_result_normalized = machine.result.as_ref().map(normalize_json);

            if cli.status != machine.status || cli_result_normalized != machine_result_normalized {
                failures.push(format!("{} -> status/result mismatch", op.path.join(".")));
                continue;
            }

            // Compare error code and message (see assert_parity for rationale on skipping details)
            let cli_error = cli.error.as_ref().map(|e| (&e.code, &e.message));
            let machine_error = machine.error.as_ref().map(|e| (&e.code, &e.message));
            if cli_error != machine_error {
                failures.push(format!(
                    "{} -> error mismatch (cli={:?}, machine={:?})",
                    op.path.join("."),
                    cli.error,
                    machine.error
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "dispatch parity failures:\n{}",
            failures.join("\n")
        );
    });
}

#[test_matrix(["sqlite"])]
fn dispatch_parity_phase_reads_active_phase(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    write_plan_with_active_phase(root, backend);

    for operation in ["read-goals", "read-tasks"] {
        let op = OperationCase {
            path: vec!["phase".to_string(), operation.to_string()],
            is_root: false,
        };

        assert_parity_for_op(root, &op, json!({}), &[]);
    }
}
