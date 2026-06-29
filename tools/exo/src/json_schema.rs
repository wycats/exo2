//! Generate JSON Schema from `CommandSpec`.
//!
//! This module produces JSON Schema definitions for use with LM Tools
//! and other machine interfaces that need to understand the command structure.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::command::command_spec::{ArgKind, ArgSpec, CommandSpec, OperationSpec, ValueType};

/// JSON Schema output for a command spec.
#[derive(Debug, Clone, Serialize)]
pub struct JsonSchema {
    #[serde(rename = "$schema")]
    pub schema: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "oneOf", skip_serializing_if = "Vec::is_empty")]
    pub one_of: Vec<Value>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub properties: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

impl Default for JsonSchema {
    fn default() -> Self {
        Self {
            schema: "https://json-schema.org/draft/2020-12/schema".to_string(),
            ty: "object".to_string(),
            description: None,
            one_of: Vec::new(),
            properties: Map::new(),
            required: Vec::new(),
            additional_properties: Some(false),
        }
    }
}

/// Generate a JSON Schema from a `CommandSpec`.
///
/// The schema represents the command tree as a union of operation schemas,
/// where each leaf command becomes a distinct schema variant.
pub fn generate_schema(spec: &CommandSpec) -> Value {
    let variants = collect_command_schemas(spec);

    if variants.len() == 1 {
        // Single command, no oneOf needed
        variants
            .into_iter()
            .next()
            .unwrap_or_else(|| json!({"type": "object"}))
    } else {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "oneOf": variants
        })
    }
}

/// Generate a minimal input schema suitable for VS Code LM Tool inputSchema.
///
/// This produces a more compact schema focused on the operation discriminator pattern.
pub fn generate_lm_tool_schema(spec: &CommandSpec) -> Value {
    let variants = collect_operation_schemas(spec);

    json!({
        "type": "object",
        "oneOf": variants
    })
}

fn collect_command_schemas(spec: &CommandSpec) -> Vec<Value> {
    let mut variants = Vec::new();

    for (op_name, op) in &spec.root_operations {
        let path = root_operation_path(op_name);
        variants.push(generate_command_schema(op, &path));
    }

    for (ns_name, ns) in &spec.namespaces {
        for (op_name, op) in &ns.operations {
            let path = namespaced_operation_path(ns_name, op_name);
            variants.push(generate_command_schema(op, &path));
        }
    }

    variants
}

fn collect_operation_schemas(spec: &CommandSpec) -> Vec<Value> {
    let mut variants = Vec::new();

    for (op_name, op) in &spec.root_operations {
        let path = root_operation_path(op_name);
        variants.push(generate_operation_schema(op, &path));
    }

    for (ns_name, ns) in &spec.namespaces {
        for (op_name, op) in &ns.operations {
            let path = namespaced_operation_path(ns_name, op_name);
            variants.push(generate_operation_schema(op, &path));
        }
    }

    variants
}

fn root_operation_path(operation: &str) -> Vec<String> {
    vec!["exo".to_string(), operation.to_string()]
}

fn namespaced_operation_path(namespace: &str, operation: &str) -> Vec<String> {
    let mut path = vec!["exo".to_string(), namespace.to_string()];
    path.extend(operation.split('.').map(str::to_string));
    path
}

fn generate_command_schema(operation: &OperationSpec, path: &[String]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();

    // Add command path as a constant
    properties.insert(
        "command".to_string(),
        json!({
            "type": "array",
            "const": path
        }),
    );
    required.push("command".to_string());

    // Add argument properties
    for arg in &operation.args {
        let arg_schema = arg_to_json_schema(arg);
        properties.insert(arg.id.clone(), arg_schema);

        if !arg.optional {
            required.push(arg.id.clone());
        }
    }

    let mut schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required
    });

    if !operation.description.is_empty() {
        schema["description"] = json!(&operation.description);
    }

    schema
}

fn generate_operation_schema(operation: &OperationSpec, path: &[String]) -> Value {
    // For LM Tool style: wrap in operation key
    // e.g., { "run": { "task": { "id": "..." } } }

    let operation_key = if path.len() > 1 {
        path[1..].join("_")
    } else {
        operation.name.clone()
    };

    let mut inner_properties = Map::new();
    let mut inner_required = Vec::new();

    for arg in &operation.args {
        let arg_schema = arg_to_json_schema(arg);
        inner_properties.insert(arg.id.clone(), arg_schema);

        if !arg.optional {
            inner_required.push(arg.id.clone());
        }
    }

    let inner_schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": inner_properties,
        "required": inner_required
    });

    let mut outer_properties = Map::new();
    outer_properties.insert(operation_key.clone(), inner_schema);

    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [operation_key],
        "properties": outer_properties
    })
}

fn arg_to_json_schema(arg: &ArgSpec) -> Value {
    match &arg.kind {
        ArgKind::Flag => {
            json!({
                "type": "boolean",
                "default": false
            })
        }
        ArgKind::Option | ArgKind::Positional => {
            value_type_to_json_schema(&arg.value_type, !arg.optional)
        }
    }
}

fn value_type_to_json_schema(kind: &ValueType, required: bool) -> Value {
    let base = match kind {
        ValueType::Bool => json!({"type": "boolean"}),
        ValueType::Int => json!({"type": "integer"}),
        ValueType::Float => json!({"type": "number"}),
        ValueType::String => json!({"type": "string"}),
        ValueType::Path => json!({
            "type": "string",
            "description": "File system path"
        }),
        ValueType::Json => json!({
            "type": ["object", "array", "string", "number", "boolean", "null"],
            "description": "JSON value"
        }),
        ValueType::Enum(spec) => {
            json!({
                "type": "string",
                "enum": spec
            })
        }
    };

    // If not required, allow null
    if required {
        return base;
    }

    match base {
        Value::Object(mut obj) => {
            match obj.get("type").cloned() {
                Some(Value::String(ty)) => {
                    // Single type string -> convert to array with null
                    obj.insert("type".to_string(), json!([ty, "null"]));
                    obj.insert("default".to_string(), Value::Null);
                }
                Some(Value::Array(mut types)) => {
                    // Already an array -> add null if not present
                    let null_val = Value::String("null".to_string());
                    if !types.contains(&null_val) {
                        types.push(null_val);
                        obj.insert("type".to_string(), Value::Array(types));
                    }
                    obj.insert("default".to_string(), Value::Null);
                }
                _ => {}
            }
            Value::Object(obj)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;
    use crate::command::command_spec::{CommandSpec, OperationSpec, ValueType};

    #[test]
    fn generate_schema_for_simple_command() {
        let mut spec = CommandSpec::default();
        let mut run = OperationSpec::new("run", "run", Effect::Pure);
        run.args.push(ArgSpec::option("id", "", ValueType::String));
        spec.root_operations.insert("run".to_string(), run);

        let schema = generate_schema(&spec);
        assert!(schema.get("oneOf").is_some() || schema.get("properties").is_some());
    }

    #[test]
    fn generate_lm_tool_schema_produces_operation_discriminator() {
        let mut spec = CommandSpec::default();
        let mut list = OperationSpec::new("list", "list", Effect::Pure);
        list.args.push(ArgSpec::positional(
            "kind",
            "",
            ValueType::Enum(vec!["tasks".to_string(), "recipes".to_string()]),
        ));
        spec.root_operations.insert("list".to_string(), list);

        let schema = generate_lm_tool_schema(&spec);
        let one_of = schema.get("oneOf").expect("should have oneOf");
        assert!(one_of.is_array());
    }

    #[test]
    fn optional_json_value_includes_null_in_type_array() {
        // ValueType::Json already produces a type array; when not required,
        // null should be added if not already present
        let schema = value_type_to_json_schema(&ValueType::Json, false);
        let obj = schema.as_object().expect("should be object");
        let types = obj.get("type").expect("should have type");
        let type_array = types.as_array().expect("type should be array");

        // Should contain null
        let has_null = type_array.iter().any(|v| v.as_str() == Some("null"));
        assert!(
            has_null,
            "optional Json value should include null in type array"
        );

        // Should have default: null
        let default = obj.get("default").expect("should have default");
        assert!(default.is_null(), "default should be null");
    }

    #[test]
    fn required_json_value_already_has_null() {
        // ValueType::Json includes null in its type array by default
        let schema = value_type_to_json_schema(&ValueType::Json, true);
        let obj = schema.as_object().expect("should be object");
        let types = obj.get("type").expect("should have type");
        let type_array = types.as_array().expect("type should be array");

        // Should already contain null (Json accepts any JSON value)
        let has_null = type_array.iter().any(|v| v.as_str() == Some("null"));
        assert!(has_null, "Json type array should include null");

        // Required values should NOT have default: null
        assert!(
            obj.get("default").is_none(),
            "required values should not have default"
        );
    }
}
