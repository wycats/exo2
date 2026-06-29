#![allow(missing_docs)]
#![allow(clippy::assertions_on_constants)]

#[macro_use]
mod test_support;

use exo::api::protocol::Effect;
use exo::command::CommandPath;
use exo::command::command_spec::{CommandSpec, NamespaceSpec, OperationSpec};
use exo::router::compile_argv;

#[test]
fn compile_argv_with_no_shell_operators_produces_invocation() {
    let mut spec = CommandSpec::new();

    let tasks_op = OperationSpec::new("tasks", "List tasks", Effect::Pure);
    let list_ns = NamespaceSpec::new("list", "List operations").with_operation(tasks_op);
    spec.namespaces.insert("list".to_string(), list_ns);

    let argv = vec!["list".to_string(), "tasks".to_string()];
    let out = compile_argv(&spec, &argv);

    assert!(out.diagnostics.is_empty());
    assert!(out.steering.is_none());

    let invocation = some_or_return!(out.invocation, "invocation should be present");
    assert_eq!(invocation.path, CommandPath::new("list", "tasks"));
}

#[test]
fn compile_argv_rejects_literal_shell_operator_tokens() {
    let spec = CommandSpec::new();

    let argv = vec!["|".to_string()];
    let out = compile_argv(&spec, &argv);

    assert!(out.invocation.is_none());
    assert!(!out.diagnostics.is_empty());
    assert!(out.steering.is_some());
}
