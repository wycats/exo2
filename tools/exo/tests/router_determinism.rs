#![allow(missing_docs)]

use exo::api::protocol::Effect;
use exo::argv_compiler::Compilation;
use exo::command::command_spec::{ArgSpec, CommandSpec, NamespaceSpec, OperationSpec, ValueType};
use exo::command::{CommandPath, TypedValue};
use exo::router::compile_argv;

fn strip_spans(out: &Compilation) -> Vec<exo::diagnostics::Diagnostic> {
    out.diagnostics
        .iter()
        .map(exo::diagnostics::Diagnostic::strip_spans)
        .collect()
}

fn build_spec() -> CommandSpec {
    let mut spec = CommandSpec::new();

    let tasks_op = OperationSpec::new("tasks", "List tasks", Effect::Pure);
    let list_ns = NamespaceSpec::new("list", "List operations").with_operation(tasks_op);

    spec.namespaces.insert("list".to_string(), list_ns);
    spec
}

#[test]
fn compile_is_deterministic_for_same_inputs() {
    let spec = build_spec();

    let argv = vec!["list".to_string(), "tasks".to_string()];
    let a = compile_argv(&spec, &argv);
    let b = compile_argv(&spec, &argv);

    assert_eq!(a.invocation, b.invocation);
    assert_eq!(strip_spans(&a), strip_spans(&b));
    assert_eq!(a.steering.is_some(), b.steering.is_some());
}

#[test]
fn compile_parses_flags_and_options_deterministically() {
    let mut spec = CommandSpec::new();

    let tasks_op = OperationSpec::new("tasks", "List tasks", Effect::Pure)
        .with_arg(ArgSpec::flag("verbose", "Verbose output").with_short('v'))
        .with_arg(ArgSpec::option("limit", "Limit results", ValueType::Int).optional());

    let list_ns = NamespaceSpec::new("list", "List operations").with_operation(tasks_op);

    spec.namespaces.insert("list".to_string(), list_ns);

    let argv = vec![
        "list".to_string(),
        "tasks".to_string(),
        "--limit".to_string(),
        "20".to_string(),
        "-v".to_string(),
    ];

    let out = compile_argv(&spec, &argv);
    assert!(out.diagnostics.is_empty());

    assert!(
        out.invocation.is_some(),
        "expected invocation to be present"
    );
    let Some(inv) = out.invocation else {
        return;
    };
    assert_eq!(inv.path, CommandPath::new("list", "tasks"));

    assert_eq!(inv.args.get("verbose"), Some(&TypedValue::Bool(true)));
    assert_eq!(inv.args.get("limit"), Some(&TypedValue::Int(20)));
}

#[test]
fn unknown_flag_emits_diagnostic() {
    let spec = build_spec();

    let argv = vec!["list".to_string(), "tasks".to_string(), "--wat".to_string()];
    let out = compile_argv(&spec, &argv);

    assert!(out.invocation.is_none());
    assert!(!out.diagnostics.is_empty());
}
