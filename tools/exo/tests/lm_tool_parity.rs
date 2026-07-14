//! LM Tool Parity Tests
//!
//! This test suite verifies that the VS Code extension manifest exposes the curated
//! LM tool surface. `CommandSpec` still generates tool metadata for infrastructure,
//! but generated CommandSpec tools are not automatically contributed to VS Code.
//!
//! The active VS Code LM tool surface is intentionally limited to the curated tools.

#![allow(clippy::disallowed_methods)] // Tests use sync fs operations
#![allow(clippy::expect_used)] // Tests use expect for assertions

use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

const CURATED_TOOL_NAMES: &[&str] = &[
    "exo-ai-chat-history",
    "exo-diagnostics",
    "exo-logs",
    "exo-ping",
    "exo-run",
];

const CURATED_TOOL_REFERENCE_NAMES: &[&str] = &[
    "ai-chat-history",
    "diagnostics",
    "logs",
    "exo-ping",
    "exo-run",
];

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    contributes: Contributes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contributes {
    language_model_tools: Vec<LmTool>,
    #[serde(default)]
    language_model_tool_sets: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LmTool {
    name: String,
    tool_reference_name: String,
    input_schema: serde_json::Value,
}

/// Load the package.json languageModelTools.
fn load_package_json_tools() -> Vec<LmTool> {
    load_package_json().contributes.language_model_tools
}

fn load_package_json() -> PackageJson {
    let path = repo_root().join("packages/exosuit-vscode/package.json");
    let raw = std::fs::read_to_string(&path).expect("expected package.json to load");
    serde_json::from_str(&raw).expect("expected package.json to deserialize")
}

#[test]
fn package_json_tools_are_exact_curated_surface() {
    let package_tools = load_package_json_tools();

    let package_names: Vec<_> = package_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    let reference_names: Vec<_> = package_tools
        .iter()
        .map(|tool| tool.tool_reference_name.as_str())
        .collect();

    assert_eq!(package_names, CURATED_TOOL_NAMES);
    assert_eq!(reference_names, CURATED_TOOL_REFERENCE_NAMES);
}

#[test]
fn package_json_does_not_declare_language_model_tool_sets() {
    assert!(
        load_package_json()
            .contributes
            .language_model_tool_sets
            .is_none(),
        "package.json must not declare languageModelToolSets"
    );
}

#[test]
fn no_duplicate_tool_names() {
    let package_tools = load_package_json_tools();

    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();

    for tool in &package_tools {
        if !seen.insert(tool.name.clone()) {
            duplicates.push(tool.name.clone());
        }
    }

    assert!(
        duplicates.is_empty(),
        "Duplicate tool names in package.json:\n{}",
        duplicates.join("\n")
    );
}

#[test]
fn tool_schemas_are_valid() {
    let package_tools = load_package_json_tools();

    let mut invalid_schemas = Vec::new();

    for tool in &package_tools {
        // Basic schema validation: must be an object with "type": "object"
        let schema = &tool.input_schema;

        let is_object_type = schema
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == "object");

        if !is_object_type {
            invalid_schemas.push(format!(
                "{}: inputSchema must have type: 'object'",
                tool.name
            ));
            continue;
        }

        // Must have "properties" field
        if !schema
            .get("properties")
            .is_some_and(serde_json::Value::is_object)
        {
            invalid_schemas.push(format!(
                "{}: inputSchema must have 'properties' object",
                tool.name
            ));
        }
    }

    assert!(
        invalid_schemas.is_empty(),
        "Invalid tool schemas:\n{}",
        invalid_schemas.join("\n")
    );
}
