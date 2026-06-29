#![allow(missing_docs)]

use exo::command::command_spec::{ArgSpec as CommandArgSpec, CommandSpec, ValueType};
use exo::command::registry::default_registry;
use exo::command::router::{Invocation, TypedValue};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn token_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_./-]{1,16}".prop_map(|s| s)
}

fn json_value_strategy() -> BoxedStrategy<serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        (-1000i64..1000i64).prop_map(|i| serde_json::Value::Number(serde_json::Number::from(i))),
        token_strategy().prop_map(serde_json::Value::String),
    ];

    leaf.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::Array),
            prop::collection::btree_map(token_strategy(), inner, 0..4)
                .prop_map(|map| { serde_json::Value::Object(map.into_iter().collect()) }),
        ]
    })
    .boxed()
}

// NOTE: typed_value_strategy was removed - it was only used by the deleted frontend_case_strategy

fn typed_value_to_json(value: &TypedValue) -> serde_json::Value {
    match value {
        TypedValue::Bool(b) => serde_json::Value::Bool(*b),
        TypedValue::Int(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        TypedValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        TypedValue::String(s) | TypedValue::Path(s) | TypedValue::Enum(s) => {
            serde_json::Value::String(s.clone())
        }
        TypedValue::Json(raw) => {
            serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.clone()))
        }
        TypedValue::Array(values) => {
            serde_json::Value::Array(values.iter().map(typed_value_to_json).collect())
        }
    }
}

// NOTE: typed_value_to_argv_token, json_input_from_args, argv_from_args, and normalize_invocation
// were removed - they were only used by the deleted frontend_case_strategy tests

#[derive(Debug, Clone)]
struct OperationFixture {
    namespace: String,
    operation: String,
    args: Vec<CommandArgSpec>,
}

#[derive(Debug, Clone)]
struct ArgInput {
    spec: CommandArgSpec,
    include: bool,
    value: TypedValue,
}

#[derive(Debug, Clone)]
struct JsonInvocationCase {
    namespace: String,
    operation: String,
    input: serde_json::Value,
    expected_args: BTreeMap<String, TypedValue>,
}

#[derive(Debug, Clone)]
struct MissingRequiredCase {
    namespace: String,
    operation: String,
    input: serde_json::Value,
}

#[derive(Debug, Clone)]
struct InvalidTypeCase {
    namespace: String,
    operation: String,
    input: serde_json::Value,
}

fn operation_fixtures(spec: &CommandSpec) -> Vec<OperationFixture> {
    let mut fixtures = Vec::new();

    for (namespace, ns_spec) in &spec.namespaces {
        for (operation, op_spec) in &ns_spec.operations {
            fixtures.push(OperationFixture {
                namespace: namespace.clone(),
                operation: operation.clone(),
                args: op_spec.args.clone(),
            });
        }
    }

    fixtures
}

fn typed_value_strategy_for_value_type(value_type: &ValueType) -> BoxedStrategy<TypedValue> {
    match value_type {
        ValueType::Bool => any::<bool>().prop_map(TypedValue::Bool).boxed(),
        ValueType::Int => (-1000i64..1000i64).prop_map(TypedValue::Int).boxed(),
        ValueType::Float => (-1000.0f64..1000.0f64).prop_map(TypedValue::Float).boxed(),
        ValueType::String => token_strategy().prop_map(TypedValue::String).boxed(),
        ValueType::Path => token_strategy().prop_map(TypedValue::Path).boxed(),
        ValueType::Json => json_value_strategy()
            .prop_map(|value| TypedValue::Json(value.to_string()))
            .boxed(),
        ValueType::Enum(variants) => prop::sample::select(variants.clone())
            .prop_map(TypedValue::Enum)
            .boxed(),
    }
}

fn default_typed_value(arg: &CommandArgSpec) -> Option<TypedValue> {
    let default = arg.default.as_ref()?;

    match &arg.value_type {
        ValueType::Bool => default.parse::<bool>().ok().map(TypedValue::Bool),
        ValueType::Int => default.parse::<i64>().ok().map(TypedValue::Int),
        ValueType::Float => default.parse::<f64>().ok().map(TypedValue::Float),
        ValueType::String => Some(TypedValue::String(default.clone())),
        ValueType::Path => Some(TypedValue::Path(default.clone())),
        ValueType::Json => Some(TypedValue::Json(default.clone())),
        ValueType::Enum(allowed) => {
            if allowed.iter().any(|variant| variant == default) {
                Some(TypedValue::Enum(default.clone()))
            } else {
                None
            }
        }
    }
}

fn command_arg_input_strategy(arg: &CommandArgSpec) -> BoxedStrategy<ArgInput> {
    let value_strategy = typed_value_strategy_for_value_type(&arg.value_type);

    if arg.optional || arg.default.is_some() {
        (any::<bool>(), value_strategy)
            .prop_map({
                let arg = arg.clone();
                move |(include, value)| ArgInput {
                    spec: arg.clone(),
                    include,
                    value,
                }
            })
            .boxed()
    } else {
        value_strategy
            .prop_map({
                let arg = arg.clone();
                move |value| ArgInput {
                    spec: arg.clone(),
                    include: true,
                    value,
                }
            })
            .boxed()
    }
}

fn json_invocation_case_strategy() -> impl Strategy<Value = JsonInvocationCase> {
    let registry = default_registry();
    let spec = CommandSpec::from_registry(&registry);
    let fixtures = operation_fixtures(&spec);

    prop::sample::select(fixtures).prop_flat_map(|fixture| {
        let mut args_strategy: BoxedStrategy<Vec<ArgInput>> = Just(Vec::new()).boxed();

        for arg in fixture.args.clone() {
            let arg_strategy = command_arg_input_strategy(&arg);
            args_strategy = (args_strategy, arg_strategy)
                .prop_map(|(mut inputs, input)| {
                    inputs.push(input);
                    inputs
                })
                .boxed();
        }

        args_strategy.prop_map(move |inputs| {
            let mut object = serde_json::Map::new();
            let mut expected_args = BTreeMap::new();

            for input in inputs {
                if input.include {
                    // Input uses name (hyphenated) as key
                    object.insert(input.spec.name.clone(), typed_value_to_json(&input.value));
                    // But invocation stores by id (underscored) for consistency with CLI path
                    expected_args.insert(input.spec.id.clone(), input.value);
                } else if let Some(default_value) = default_typed_value(&input.spec) {
                    // Default values also stored by id
                    expected_args.insert(input.spec.id.clone(), default_value);
                }
            }

            JsonInvocationCase {
                namespace: fixture.namespace.clone(),
                operation: fixture.operation.clone(),
                input: serde_json::Value::Object(object),
                expected_args,
            }
        })
    })
}

fn missing_required_case_strategy() -> impl Strategy<Value = MissingRequiredCase> {
    let registry = default_registry();
    let spec = CommandSpec::from_registry(&registry);
    let fixtures: Vec<OperationFixture> = operation_fixtures(&spec)
        .into_iter()
        .filter(|fixture| fixture.args.iter().any(|arg| !arg.optional))
        .collect();

    prop::sample::select(fixtures).prop_flat_map(|fixture| {
        let required_args: Vec<CommandArgSpec> = fixture
            .args
            .iter()
            .filter(|arg| !arg.optional)
            .cloned()
            .collect();

        prop::sample::select(required_args).prop_flat_map(move |missing| {
            let provided_required: Vec<CommandArgSpec> = fixture
                .args
                .iter()
                .filter(|arg| !arg.optional && arg.name != missing.name)
                .cloned()
                .collect();
            let fixture = fixture.clone();

            let mut args_strategy: BoxedStrategy<Vec<ArgInput>> = Just(Vec::new()).boxed();
            for arg in provided_required {
                let arg_strategy = typed_value_strategy_for_value_type(&arg.value_type)
                    .prop_map({
                        let arg = arg.clone();
                        move |value| ArgInput {
                            spec: arg.clone(),
                            include: true,
                            value,
                        }
                    })
                    .boxed();
                args_strategy = (args_strategy, arg_strategy)
                    .prop_map(|(mut inputs, input)| {
                        inputs.push(input);
                        inputs
                    })
                    .boxed();
            }

            args_strategy.prop_map(move |inputs| {
                let mut object = serde_json::Map::new();

                for input in inputs {
                    object.insert(input.spec.name.clone(), typed_value_to_json(&input.value));
                }

                MissingRequiredCase {
                    namespace: fixture.namespace.clone(),
                    operation: fixture.operation.clone(),
                    input: serde_json::Value::Object(object),
                }
            })
        })
    })
}

fn invalid_value_strategy_for_value_type(
    value_type: &ValueType,
) -> BoxedStrategy<serde_json::Value> {
    match value_type {
        ValueType::Bool => prop_oneof![
            (-1000i64..1000i64)
                .prop_map(|i| serde_json::Value::Number(serde_json::Number::from(i))),
            // Exclude strings that from_json accepts as booleans: true/false/0/1/yes/no
            "[a-zA-Z]{2,8}"
                .prop_filter("must not be a boolean-like string", |s| {
                    !matches!(s.as_str(), "true" | "false" | "yes" | "no")
                })
                .prop_map(serde_json::Value::String),
            Just(serde_json::Value::Null),
        ]
        .boxed(),
        ValueType::Int => prop_oneof![
            any::<bool>().prop_map(serde_json::Value::Bool),
            // Exclude strings that parse as integers (from_json accepts numeric strings)
            "[a-zA-Z_./-]{1,16}".prop_map(serde_json::Value::String),
            (0.1f64..1000.0f64).prop_map(|f| {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }),
        ]
        .boxed(),
        ValueType::Float => prop_oneof![
            any::<bool>().prop_map(serde_json::Value::Bool),
            // Exclude strings that parse as floats (from_json accepts numeric strings)
            "[a-zA-Z_./-]{1,16}".prop_map(serde_json::Value::String),
            Just(serde_json::Value::Null),
        ]
        .boxed(),
        ValueType::String | ValueType::Path => prop_oneof![
            any::<bool>().prop_map(serde_json::Value::Bool),
            (-1000i64..1000i64)
                .prop_map(|i| serde_json::Value::Number(serde_json::Number::from(i))),
            Just(serde_json::Value::Null),
        ]
        .boxed(),
        ValueType::Enum(_) => prop_oneof![
            (-1000i64..1000i64)
                .prop_map(|i| serde_json::Value::Number(serde_json::Number::from(i))),
            Just(serde_json::Value::Null),
        ]
        .boxed(),
        ValueType::Json => Just(serde_json::Value::Null).boxed(),
    }
}

fn invalid_type_case_strategy() -> impl Strategy<Value = InvalidTypeCase> {
    let registry = default_registry();
    let spec = CommandSpec::from_registry(&registry);
    let fixtures: Vec<OperationFixture> = operation_fixtures(&spec)
        .into_iter()
        .filter(|fixture| {
            fixture
                .args
                .iter()
                .any(|arg| arg.value_type != ValueType::Json)
        })
        .collect();

    prop::sample::select(fixtures).prop_flat_map(|fixture| {
        let eligible_args: Vec<CommandArgSpec> = fixture
            .args
            .iter()
            .filter(|arg| arg.value_type != ValueType::Json)
            .cloned()
            .collect();

        prop::sample::select(eligible_args).prop_flat_map(move |target| {
            let invalid_value_strategy = invalid_value_strategy_for_value_type(&target.value_type);
            let required_args: Vec<CommandArgSpec> = fixture
                .args
                .iter()
                .filter(|arg| !arg.optional && arg.name != target.name)
                .cloned()
                .collect();
            let fixture = fixture.clone();

            let mut args_strategy: BoxedStrategy<Vec<ArgInput>> = Just(Vec::new()).boxed();
            for arg in required_args {
                let arg_strategy = typed_value_strategy_for_value_type(&arg.value_type)
                    .prop_map({
                        let arg = arg.clone();
                        move |value| ArgInput {
                            spec: arg.clone(),
                            include: true,
                            value,
                        }
                    })
                    .boxed();
                args_strategy = (args_strategy, arg_strategy)
                    .prop_map(|(mut inputs, input)| {
                        inputs.push(input);
                        inputs
                    })
                    .boxed();
            }

            (args_strategy, invalid_value_strategy).prop_map(move |(inputs, invalid_value)| {
                let mut object = serde_json::Map::new();

                for input in inputs {
                    object.insert(input.spec.name.clone(), typed_value_to_json(&input.value));
                }

                object.insert(target.name.clone(), invalid_value);

                InvalidTypeCase {
                    namespace: fixture.namespace.clone(),
                    operation: fixture.operation.clone(),
                    input: serde_json::Value::Object(object),
                }
            })
        })
    })
}

// NOTE: frontend_case_strategy, argv_from_list, request_from_case, argv_from_case,
// tool_call_run_task_matches_argv_invocation, tool_list_run_tasks_normalizes_defaults,
// json_argv_produce_equivalent_invocations, and tool_call_context_paths_matches_argv_invocation
// were removed - they tested the old tool_surface module which was deleted as part of
// RFC 0135 transport unification.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn invocation_from_json_parses_valid_inputs(case in json_invocation_case_strategy()) {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let invocation = Invocation::from_json(
            &case.input,
            &case.namespace,
            &case.operation,
            &spec,
        )
        .expect("expected invocation");

        prop_assert_eq!(invocation.namespace(), case.namespace.as_str());
        prop_assert_eq!(invocation.operation(), case.operation.as_str());
        prop_assert_eq!(&invocation.args, &case.expected_args);

        let invocation_again = Invocation::from_json(
            &case.input,
            &case.namespace,
            &case.operation,
            &spec,
        )
        .expect("expected invocation");

        prop_assert_eq!(invocation, invocation_again);
    }

    #[test]
    fn invocation_from_json_errors_on_missing_required(case in missing_required_case_strategy()) {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let result = Invocation::from_json(
            &case.input,
            &case.namespace,
            &case.operation,
            &spec,
        );

        prop_assert!(result.is_err(), "expected missing required arg error");
    }

    #[test]
    fn invocation_from_json_errors_on_invalid_type(case in invalid_type_case_strategy()) {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let result = Invocation::from_json(
            &case.input,
            &case.namespace,
            &case.operation,
            &spec,
        );

        prop_assert!(result.is_err(), "expected invalid type error");
    }
}
