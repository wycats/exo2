//! Generate help text and documentation from `CommandSpec`.
//!
//! This module produces structured help information that can be used
//! for CLI --help output, documentation generation, and machine channel help responses.

use crate::api::protocol::{
    Effect, HelpNamespace, HelpOperation, HelpResult, NextCall, NextCallKind,
};
use crate::command_spec::{ArgKind, ArgSpec, CommandNode, CommandSpec, ValueKind};
use serde_json::json;
use std::fmt::Write as _;

/// Generate a `HelpResult` for the root of a `CommandSpec`.
pub fn generate_root_help(spec: &CommandSpec) -> HelpResult {
    let mut namespaces = Vec::new();

    for child in &spec.root.children {
        if !child.children.is_empty() {
            // This is a namespace (has subcommands)
            namespaces.push(HelpNamespace {
                path: vec![child.name.clone()],
                summary: child.about.clone().unwrap_or_default(),
            });
        }
    }

    namespaces.sort_by(|a, b| a.path.cmp(&b.path));

    // Suggest exploring the first namespace if any exist
    let next_calls = namespaces
        .first()
        .map(|ns| vec![next_help_namespace(&ns.path)])
        .unwrap_or_default();

    HelpResult {
        title: spec.root.name.clone(),
        summary: spec
            .root
            .about
            .clone()
            .unwrap_or_else(|| format!("Root command: {}", spec.root.name)),
        namespaces,
        operations: vec![],
        next_calls,
    }
}

/// Generate a `HelpResult` for a namespace (non-leaf command).
pub fn generate_namespace_help(spec: &CommandSpec, path: &[String]) -> Option<HelpResult> {
    let node = find_node(&spec.root, path)?;

    let mut namespaces = Vec::new();
    let mut operations = Vec::new();

    for child in &node.children {
        if child.children.is_empty() {
            // This is a leaf (operation)
            operations.push(HelpOperation {
                path: child.name.clone(),
                effect: infer_effect(&child.name),
                summary: child.about.clone().unwrap_or_default(),
                args: vec![],
            });
        } else {
            // This is a nested namespace
            namespaces.push(HelpNamespace {
                path: child_path(path, &child.name),
                summary: child.about.clone().unwrap_or_default(),
            });
        }
    }

    namespaces.sort_by(|a, b| a.path.cmp(&b.path));
    operations.sort_by(|a, b| a.path.cmp(&b.path));

    let next_calls = if !operations.is_empty() {
        vec![next_help_operation(&child_path(path, &operations[0].path))]
    } else if !namespaces.is_empty() {
        vec![next_help_namespace(&namespaces[0].path)]
    } else {
        vec![]
    };

    Some(HelpResult {
        title: path.join("/"),
        summary: node
            .about
            .clone()
            .unwrap_or_else(|| format!("Namespace: {}", path.join("/"))),
        namespaces,
        operations,
        next_calls,
    })
}

/// Generate a `HelpResult` for an operation (leaf command).
pub fn generate_operation_help(spec: &CommandSpec, path: &[String]) -> Option<HelpResult> {
    let node = find_node(&spec.root, path)?;

    // For leaf commands, operations show the arguments
    let operations: Vec<HelpOperation> = node
        .args
        .iter()
        .map(|arg| HelpOperation {
            path: format_arg_name(arg),
            effect: Effect::Pure, // Args don't have effects
            summary: format_arg_help(arg),
            args: vec![],
        })
        .collect();

    Some(HelpResult {
        title: path.join(" "),
        summary: node
            .about
            .clone()
            .unwrap_or_else(|| format!("Operation: {}", path.join(" "))),
        namespaces: vec![],
        operations,
        next_calls: vec![],
    })
}

/// Generate help text suitable for CLI --help output.
pub fn generate_cli_help(spec: &CommandSpec) -> String {
    let mut out = String::new();

    let _ = writeln!(&mut out, "{}\n", spec.root.name);

    if let Some(about) = &spec.root.about {
        let _ = writeln!(&mut out, "{about}\n");
    }

    out.push_str("USAGE:\n");
    let _ = writeln!(&mut out, "    {} [OPTIONS] <COMMAND>\n", spec.root.name);

    if !spec.root.children.is_empty() {
        out.push_str("COMMANDS:\n");
        for child in &spec.root.children {
            let about = child.about.as_deref().unwrap_or("");
            let _ = writeln!(&mut out, "    {:16} {}", child.name, about);
        }
    }

    out
}

/// Generate help text for a specific command.
pub fn generate_command_help(node: &CommandNode, path: &[String]) -> String {
    let mut out = String::new();

    let full_path = path.join(" ");
    let _ = writeln!(&mut out, "{full_path}\n");

    if let Some(about) = &node.about {
        let _ = writeln!(&mut out, "{about}\n");
    }

    if !node.args.is_empty() {
        // Separate positionals and options
        let positionals: Vec<_> = node
            .args
            .iter()
            .filter(|a| matches!(a.kind, ArgKind::Positional { .. }))
            .collect();

        let options: Vec<_> = node
            .args
            .iter()
            .filter(|a| !matches!(a.kind, ArgKind::Positional { .. }))
            .collect();

        if !positionals.is_empty() {
            out.push_str("ARGS:\n");
            for arg in positionals {
                let _ = writeln!(
                    &mut out,
                    "    {:16} {}",
                    format_arg_name(arg),
                    format_arg_help(arg)
                );
            }
            out.push('\n');
        }

        if !options.is_empty() {
            out.push_str("OPTIONS:\n");
            for arg in options {
                let _ = writeln!(
                    &mut out,
                    "    {:16} {}",
                    format_arg_name(arg),
                    format_arg_help(arg)
                );
            }
        }
    }

    if !node.children.is_empty() {
        out.push_str("SUBCOMMANDS:\n");
        for child in &node.children {
            let about = child.about.as_deref().unwrap_or("");
            let _ = writeln!(&mut out, "    {:16} {}", child.name, about);
        }
    }

    out
}

// --- Helper functions ---

fn find_node<'a>(root: &'a CommandNode, path: &[String]) -> Option<&'a CommandNode> {
    if path.is_empty() {
        return Some(root);
    }

    let mut node = root;
    for segment in path {
        node = node.children.iter().find(|c| &c.name == segment)?;
    }
    Some(node)
}

fn child_path(parent: &[String], child: &str) -> Vec<String> {
    let mut path = parent.to_vec();
    path.push(child.to_string());
    path
}

fn infer_effect(name: &str) -> Effect {
    // Heuristic: common patterns for read vs write
    let pure_patterns = ["list", "show", "get", "check", "status", "paths"];
    let write_patterns = ["create", "add", "update", "fix", "delete", "remove"];

    let name_lower = name.to_lowercase();

    for pattern in pure_patterns {
        if name_lower.contains(pattern) {
            return Effect::Pure;
        }
    }

    for pattern in write_patterns {
        if name_lower.contains(pattern) {
            return Effect::Write;
        }
    }

    Effect::Pure // Default to pure (safe)
}

fn format_arg_name(arg: &ArgSpec) -> String {
    match &arg.kind {
        ArgKind::Flag => match (arg.short, arg.long.as_deref()) {
            (Some(short), Some(long)) => format!("-{short}, --{long}"),
            (Some(short), None) => format!("-{short}"),
            (None, Some(long)) => format!("--{long}"),
            (None, None) => arg.id.0.clone(),
        },
        ArgKind::Option { value } => {
            let value_hint = value_kind_hint(value);
            arg.long.as_deref().map_or_else(
                || format!("<{}>", arg.id.0),
                |long| format!("--{long} <{value_hint}>"),
            )
        }
        ArgKind::Positional { value } => {
            let value_hint = value_kind_hint(value);
            if arg.required {
                format!("<{value_hint}>")
            } else {
                format!("[{value_hint}]")
            }
        }
    }
}

fn format_arg_help(arg: &ArgSpec) -> String {
    let mut parts = Vec::new();

    // Type info
    let type_str = match &arg.kind {
        ArgKind::Flag => "flag".to_string(),
        ArgKind::Option { value } | ArgKind::Positional { value } => value_kind_name(value),
    };

    if arg.required {
        parts.push("required".to_string());
    }

    if arg.repeatable {
        parts.push("repeatable".to_string());
    }

    parts.push(type_str);

    format!("[{}]", parts.join(", "))
}

fn value_kind_hint(kind: &ValueKind) -> String {
    match kind {
        ValueKind::Bool => "BOOL".to_string(),
        ValueKind::Int => "INT".to_string(),
        ValueKind::Float => "FLOAT".to_string(),
        ValueKind::String => "STRING".to_string(),
        ValueKind::Path => "PATH".to_string(),
        ValueKind::Json => "JSON".to_string(),
        ValueKind::Enum(spec) => {
            if spec.variants.len() <= 3 {
                spec.variants.join("|")
            } else {
                format!("{}|...", spec.variants[..2].join("|"))
            }
        }
    }
}

fn value_kind_name(kind: &ValueKind) -> String {
    match kind {
        ValueKind::Bool => "boolean".to_string(),
        ValueKind::Int => "integer".to_string(),
        ValueKind::Float => "float".to_string(),
        ValueKind::String => "string".to_string(),
        ValueKind::Path => "path".to_string(),
        ValueKind::Json => "json".to_string(),
        ValueKind::Enum(_) => "enum".to_string(),
    }
}

fn next_help_namespace(path: &[String]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "namespace", "path": path }
        }),
    }
}

fn next_help_operation(path: &[String]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "operation", "path": path }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_spec::ArgId;

    fn sample_spec() -> CommandSpec {
        let mut root = CommandNode::leaf("root", "exo");
        root.about = Some("Exosuit CLI".to_string());

        let mut run = CommandNode::leaf("run", "run");
        run.about = Some("Run project workflows".to_string());

        let mut run_task = CommandNode::leaf("run_task", "task");
        run_task.about = Some("Execute a task".to_string());
        run_task.args.push(ArgSpec {
            id: ArgId("id".to_string()),
            long: Some("id".to_string()),
            short: None,
            kind: ArgKind::Option {
                value: ValueKind::String,
            },
            required: true,
            repeatable: false,
        });

        run.children.push(run_task);
        root.children.push(run);

        CommandSpec::new(root)
    }

    #[test]
    fn generate_root_help_includes_namespaces() {
        let spec = sample_spec();
        let help = generate_root_help(&spec);

        assert_eq!(help.title, "exo");
        assert!(!help.namespaces.is_empty());
    }

    #[test]
    fn generate_cli_help_produces_output() {
        let spec = sample_spec();
        let help = generate_cli_help(&spec);

        assert!(help.contains("exo"));
        assert!(help.contains("COMMANDS:"));
        assert!(help.contains("run"));
    }

    #[test]
    fn generate_namespace_help_shows_operations() {
        let spec = sample_spec();
        let help = generate_namespace_help(&spec, &["run".to_string()]).unwrap();

        assert!(!help.operations.is_empty());
        assert!(help.operations.iter().any(|op| op.path == "task"));
    }
}
