use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::protocol::Steering;
use crate::command::Router as FlatRouter;
use crate::command::command_spec::{ArgKind, ArgSpec, CommandSpec, OperationSpec, ValueType};
use crate::command::router::{
    DiagnosticCode as RoutingDiagnosticCode, Invocation, RoutingDiagnostic, TypedValue,
};
use crate::diagnostics::{
    Diagnostic, DiagnosticCode, Span, Suggestion as LegacySuggestion,
    diagnostics_for_shell_operators, sort_diagnostics,
};
use crate::shell_ops::detect_shell_operators;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compilation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation: Option<Invocation>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steering: Option<Steering>,
}

pub fn compile(spec: &CommandSpec, argv: &[String]) -> Compilation {
    compile_argv_v2(spec, argv)
}

pub fn compile_argv_v2(spec: &CommandSpec, argv: &[String]) -> Compilation {
    let argv = strip_global_format_args(argv);
    let hits = detect_shell_operators(&argv);

    if !hits.is_empty() {
        let diagnostics = diagnostics_for_shell_operators(&hits);

        let steering = crate::steering::steering_for_shell_operators(&hits);

        return Compilation {
            invocation: None,
            diagnostics,
            steering,
        };
    }

    let routing_spec = optionalize_flat_spec(spec);

    let (route_tokens, route_token_indices, arg_start) = route_tokens_from_argv(&argv, spec);

    if route_tokens.len() == 1
        && let Some(operation_spec) = spec.root_operations.get(&route_tokens[0])
    {
        let mut invocation = Invocation::new("", route_tokens[0].clone());
        let mut diagnostics = Vec::<Diagnostic>::new();

        compile_args(
            operation_spec,
            &argv[arg_start..],
            arg_start,
            &mut invocation,
            &mut diagnostics,
        );

        sort_diagnostics(&mut diagnostics);

        return if diagnostics.is_empty() {
            Compilation {
                invocation: Some(invocation),
                diagnostics,
                steering: None,
            }
        } else {
            Compilation {
                invocation: None,
                diagnostics,
                steering: None,
            }
        };
    }

    let router = FlatRouter::new(&routing_spec);
    let result = router.route(&route_tokens.iter().map(String::as_str).collect::<Vec<_>>());

    let Some(route_invocation) = result.invocation else {
        let diagnostics = result
            .diagnostics
            .iter()
            .map(|diag| routing_diagnostic_to_legacy(diag, &route_token_indices))
            .collect();

        return Compilation {
            invocation: None,
            diagnostics,
            steering: None,
        };
    };

    let namespace = route_invocation.namespace().to_string();
    let operation = route_invocation.operation().to_string();
    let mut invocation = Invocation::new(namespace, operation);
    let mut diagnostics = Vec::<Diagnostic>::new();

    let Some(operation_spec) = spec.operation(invocation.namespace(), invocation.operation())
    else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::AmbiguousSubcommand,
            format!(
                "Unknown operation '{}' in namespace '{}'",
                invocation.operation(),
                invocation.namespace()
            ),
            None,
        ));

        return Compilation {
            invocation: None,
            diagnostics,
            steering: None,
        };
    };

    compile_args(
        operation_spec,
        &argv[arg_start..],
        arg_start,
        &mut invocation,
        &mut diagnostics,
    );

    sort_diagnostics(&mut diagnostics);

    if diagnostics.is_empty() {
        Compilation {
            invocation: Some(invocation),
            diagnostics,
            steering: None,
        }
    } else {
        Compilation {
            invocation: None,
            diagnostics,
            steering: None,
        }
    }
}

fn strip_global_format_args(argv: &[String]) -> Vec<String> {
    let mut stripped = Vec::with_capacity(argv.len());
    let mut i = 0;

    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--format" && argv.get(i + 1).is_some() {
            i += 2;
            continue;
        }

        if arg.strip_prefix("--format=").is_some() {
            i += 1;
            continue;
        }

        stripped.push(arg.clone());
        i += 1;
    }

    stripped
}

fn compile_args(
    operation: &OperationSpec,
    argv: &[String],
    offset: usize,
    invocation: &mut Invocation,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Index flags/options by long/short for deterministic lookup.
    let mut by_long: BTreeMap<&str, &ArgSpec> = BTreeMap::new();
    let mut by_short: BTreeMap<char, &ArgSpec> = BTreeMap::new();
    let mut positional_specs: Vec<&ArgSpec> = Vec::new();

    for arg in &operation.args {
        if !arg.name.is_empty() {
            by_long.insert(arg.name.as_str(), arg);
        }
        if let Some(short) = arg.short {
            by_short.insert(short, arg);
        }
        if matches!(arg.kind, ArgKind::Positional) {
            positional_specs.push(arg);
        }
    }

    let mut positionals: Vec<(usize, String)> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];

        if token == "--" {
            // Treat remaining as raw positionals.
            for (j, rest) in argv.iter().enumerate().skip(i + 1) {
                positionals.push((offset + j, rest.clone()));
            }
            break;
        }

        if let Some(long) = token.strip_prefix("--") {
            let (name, inline_value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));

            let Some(spec) = by_long.get(name).copied() else {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnknownFlag,
                    message: format!("Unknown flag '--{name}'"),
                    span: Some(Span {
                        arg_index: offset + i,
                    }),
                    suggestions: Vec::new(),
                });
                i += 1;
                continue;
            };

            match &spec.kind {
                ArgKind::Flag => {
                    bump_occurrence(invocation, &spec.id);
                    invocation
                        .args
                        .insert(spec.id.clone(), TypedValue::Bool(true));
                    i += 1;
                }
                ArgKind::Option => {
                    let value_token = if let Some(v) = inline_value {
                        v.to_string()
                    } else {
                        let Some(next) = argv.get(i + 1) else {
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::MissingValue,
                                message: format!("Missing value for '--{name}'"),
                                span: Some(Span {
                                    arg_index: offset + i,
                                }),
                                suggestions: Vec::new(),
                            });
                            i += 1;
                            continue;
                        };
                        next.clone()
                    };

                    let value_index = if inline_value.is_some() { i } else { i + 1 };

                    if let Some(parsed) = parse_value(&spec.value_type, &value_token) {
                        bump_occurrence(invocation, &spec.id);
                        invocation.args.insert(spec.id.clone(), parsed);
                    } else {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::InvalidValue,
                            message: format!("Invalid value '{value_token}' for '--{name}'"),
                            span: Some(Span {
                                arg_index: offset + value_index,
                            }),
                            suggestions: Vec::new(),
                        });
                    }

                    i += if inline_value.is_some() { 1 } else { 2 };
                }
                ArgKind::Positional => {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::InvalidFlag,
                        message: format!("'--{name}' is not a flag"),
                        span: Some(Span {
                            arg_index: offset + i,
                        }),
                        suggestions: Vec::new(),
                    });
                    i += 1;
                }
            }

            continue;
        }

        if let Some(short_token) = token.strip_prefix('-')
            && short_token.len() == 1
        {
            let ch = short_token.chars().next().unwrap_or('?');
            let Some(spec) = by_short.get(&ch).copied() else {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnknownFlag,
                    message: format!("Unknown flag '-{ch}'"),
                    span: Some(Span {
                        arg_index: offset + i,
                    }),
                    suggestions: Vec::new(),
                });
                i += 1;
                continue;
            };

            match &spec.kind {
                ArgKind::Flag => {
                    bump_occurrence(invocation, &spec.id);
                    invocation
                        .args
                        .insert(spec.id.clone(), TypedValue::Bool(true));
                    i += 1;
                }
                ArgKind::Option => {
                    let Some(next) = argv.get(i + 1) else {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::MissingValue,
                            message: format!("Missing value for '-{ch}'"),
                            span: Some(Span {
                                arg_index: offset + i,
                            }),
                            suggestions: Vec::new(),
                        });
                        i += 1;
                        continue;
                    };

                    if let Some(parsed) = parse_value(&spec.value_type, next) {
                        bump_occurrence(invocation, &spec.id);
                        invocation.args.insert(spec.id.clone(), parsed);
                    } else {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::InvalidValue,
                            message: format!("Invalid value '{next}' for '-{ch}'"),
                            span: Some(Span {
                                arg_index: offset + i + 1,
                            }),
                            suggestions: Vec::new(),
                        });
                    }

                    i += 2;
                }
                ArgKind::Positional => {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::InvalidFlag,
                        message: format!("'-{ch}' is not a flag"),
                        span: Some(Span {
                            arg_index: offset + i,
                        }),
                        suggestions: Vec::new(),
                    });
                    i += 1;
                }
            }

            continue;
        }

        positionals.push((offset + i, token.clone()));
        i += 1;
    }

    // Assign positional tokens to positional specs.
    if positionals.len() > positional_specs.len() {
        // Deterministic: flag the first excess positional.
        let (arg_index, token) = &positionals[positional_specs.len()];
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::TooManyPositionals,
            message: format!("Unexpected positional argument '{token}'"),
            span: Some(Span {
                arg_index: *arg_index,
            }),
            suggestions: Vec::new(),
        });
    }

    for (slot, spec) in positional_specs.iter().enumerate() {
        let Some((arg_index, token)) = positionals.get(slot).cloned() else {
            if !spec.optional {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::MissingRequired,
                    message: "Missing required positional argument".to_string(),
                    span: None,
                    suggestions: Vec::new(),
                });
            }
            continue;
        };

        if let Some(parsed) = parse_value(&spec.value_type, &token) {
            bump_occurrence(invocation, &spec.id);
            invocation.args.insert(spec.id.clone(), parsed);
        } else {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::InvalidValue,
                message: format!("Invalid value '{token}' for positional"),
                span: Some(Span { arg_index }),
                suggestions: Vec::new(),
            });
        }
    }

    // Validate required/repeatable.
    for spec in &operation.args {
        let count = invocation.occurrences.get(&spec.id).copied().unwrap_or(0);

        if !spec.optional && count == 0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::MissingRequired,
                message: format!("Missing required argument '{}'", display_arg(spec)),
                span: None,
                suggestions: Vec::new(),
            });
        }

        if !spec.repeatable && count > 1 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::NonRepeatable,
                message: format!("Argument '{}' is not repeatable", display_arg(spec)),
                span: None,
                suggestions: Vec::new(),
            });
        }
    }

    // Apply defaults for optional args not provided.
    for spec in &operation.args {
        if invocation.args.contains_key(&spec.id) {
            continue;
        }

        if let Some(default_str) = &spec.default
            && let Some(parsed) = parse_value(&spec.value_type, default_str)
        {
            invocation.args.insert(spec.id.clone(), parsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;

    #[test]
    fn compile_args_applies_default_for_missing_optional_arg() {
        let mut operation = OperationSpec::new("tasks", "tasks", Effect::Pure);
        operation.args.push(ArgSpec {
            id: "limit".to_string(),
            name: "limit".to_string(),
            description: String::new(),
            kind: ArgKind::Option,
            value_type: ValueType::Int,
            optional: true,
            default: Some("10".to_string()),
            short: None,
            repeatable: false,
        });

        let mut invocation = Invocation::new("list", "tasks");
        let mut diagnostics = Vec::new();

        compile_args(&operation, &[], 0, &mut invocation, &mut diagnostics);

        assert!(diagnostics.is_empty(), "expected no diagnostics");
        assert_eq!(invocation.args.get("limit"), Some(&TypedValue::Int(10)));
    }

    #[test]
    fn compile_strips_global_format_flag_after_operation() {
        let mut spec = CommandSpec::new();
        let mut namespace = crate::command::command_spec::NamespaceSpec::new("dogfood", "dogfood");
        let mut operation = OperationSpec::new("verify", "verify", Effect::Pure);
        operation.args.push(ArgSpec {
            id: "skip_receipt".to_string(),
            name: "skip-receipt".to_string(),
            description: String::new(),
            kind: ArgKind::Flag,
            value_type: ValueType::Bool,
            optional: true,
            default: None,
            short: None,
            repeatable: false,
        });
        namespace = namespace.with_operation(operation);
        spec.namespaces.insert("dogfood".to_string(), namespace);

        let argv = vec![
            "dogfood".to_string(),
            "verify".to_string(),
            "--skip-receipt".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ];

        let compilation = compile_argv_v2(&spec, &argv);
        assert!(
            compilation.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            compilation.diagnostics
        );
        let invocation = compilation.invocation.expect("invocation");
        assert_eq!(invocation.namespace(), "dogfood");
        assert_eq!(invocation.operation(), "verify");
        assert_eq!(
            invocation.args.get("skip_receipt"),
            Some(&TypedValue::Bool(true))
        );
    }
}

fn bump_occurrence(invocation: &mut Invocation, arg_id: &str) {
    let entry = invocation
        .occurrences
        .entry(arg_id.to_string())
        .or_insert(0);
    *entry = entry.saturating_add(1);
}

fn parse_value(kind: &ValueType, token: &str) -> Option<TypedValue> {
    match kind {
        ValueType::Bool => match token {
            "true" => Some(TypedValue::Bool(true)),
            "false" => Some(TypedValue::Bool(false)),
            _ => None,
        },
        ValueType::Int => token.parse::<i64>().ok().map(TypedValue::Int),
        ValueType::Float => token.parse::<f64>().ok().map(TypedValue::Float),
        ValueType::String => Some(TypedValue::String(token.to_string())),
        ValueType::Path => Some(TypedValue::Path(token.to_string())),
        ValueType::Json => serde_json::from_str::<serde_json::Value>(token)
            .ok()
            .map(|_| TypedValue::Json(token.to_string())),
        ValueType::Enum(spec) => {
            if spec.iter().any(|v| v == token) {
                Some(TypedValue::Enum(token.to_string()))
            } else {
                None
            }
        }
    }
}

fn display_arg(spec: &ArgSpec) -> String {
    if matches!(spec.kind, ArgKind::Positional) {
        if spec.name.is_empty() {
            spec.id.clone()
        } else {
            spec.name.clone()
        }
    } else if !spec.name.is_empty() {
        format!("--{}", spec.name)
    } else if let Some(short) = spec.short {
        format!("-{short}")
    } else {
        spec.id.clone()
    }
}

fn optionalize_flat_spec(spec: &CommandSpec) -> CommandSpec {
    let mut spec = spec.clone();

    for namespace in spec.namespaces.values_mut() {
        for operation in namespace.operations.values_mut() {
            for arg in &mut operation.args {
                arg.optional = true;
            }
        }
    }

    spec
}

fn route_tokens_from_argv(
    argv: &[String],
    flat_spec: &CommandSpec,
) -> (Vec<String>, Vec<usize>, usize) {
    let mut path_limit = argv.len();
    for (idx, token) in argv.iter().enumerate() {
        if token.starts_with('-') {
            path_limit = idx;
            break;
        }
    }

    let mut tokens_with_index = argv
        .iter()
        .enumerate()
        .take(path_limit)
        .map(|(idx, token)| (token.clone(), idx))
        .collect::<Vec<_>>();

    if tokens_with_index.first().map(|(token, _)| token.as_str()) == Some("exo") {
        tokens_with_index.remove(0);
    }

    if tokens_with_index.is_empty() {
        return (Vec::new(), Vec::new(), 0);
    }

    let namespace = tokens_with_index[0].0.clone();
    let namespace_index = tokens_with_index[0].1;

    let mut operation = None;
    let mut operation_len = 0usize;

    if let Some(namespace_spec) = flat_spec.namespace(&namespace) {
        for idx in 1..tokens_with_index.len() {
            let candidate = tokens_with_index[1..=idx]
                .iter()
                .map(|(token, _)| token.as_str())
                .collect::<Vec<_>>()
                .join(".");
            if namespace_spec.operations.contains_key(&candidate) {
                operation = Some(candidate);
                operation_len = idx;
            }
        }
    }

    if tokens_with_index.len() == 1 {
        if let Some(namespace_spec) = flat_spec.namespace(&namespace)
            && namespace_spec.operations.len() == 1
            && let Some(operation) = namespace_spec.operations.keys().next().cloned()
        {
            return (
                vec![namespace, operation],
                vec![namespace_index, namespace_index],
                tokens_with_index[0].1 + 1,
            );
        }

        return (
            vec![namespace],
            vec![namespace_index],
            tokens_with_index[0].1 + 1,
        );
    }

    let operation = operation.unwrap_or_else(|| tokens_with_index[1].0.clone());
    let operation_index = tokens_with_index[1].1;
    let arg_start = if operation_len == 0 {
        tokens_with_index[1].1 + 1
    } else {
        tokens_with_index[operation_len].1 + 1
    };

    (
        vec![namespace, operation],
        vec![namespace_index, operation_index],
        arg_start,
    )
}

fn routing_diagnostic_to_legacy(diag: &RoutingDiagnostic, token_indices: &[usize]) -> Diagnostic {
    let span = diag
        .location
        .and_then(|location| location.token_index)
        .and_then(|token_index| token_indices.get(token_index).copied())
        .map(|arg_index| Span { arg_index });

    Diagnostic {
        code: routing_code_to_legacy(diag.code),
        message: diag.message.clone(),
        span,
        suggestions: diag
            .suggestions
            .iter()
            .map(|suggestion| LegacySuggestion {
                label: suggestion.label.clone(),
                replacement: suggestion.replacement.clone(),
            })
            .collect(),
    }
}

const fn routing_code_to_legacy(code: RoutingDiagnosticCode) -> DiagnosticCode {
    match code {
        RoutingDiagnosticCode::UnknownFlag => DiagnosticCode::UnknownFlag,
        RoutingDiagnosticCode::MissingRequiredArg => DiagnosticCode::MissingRequired,
        RoutingDiagnosticCode::InvalidArgType => DiagnosticCode::InvalidValue,
        RoutingDiagnosticCode::TooManyPositionals => DiagnosticCode::TooManyPositionals,
        RoutingDiagnosticCode::UnsupportedShellFeature => DiagnosticCode::ShellOperator,
        RoutingDiagnosticCode::UnknownNamespace
        | RoutingDiagnosticCode::UnknownOperation
        | RoutingDiagnosticCode::AmbiguousCommand => DiagnosticCode::AmbiguousSubcommand,
    }
}
