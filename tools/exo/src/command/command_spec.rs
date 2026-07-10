//! `CommandSpec`: A reflectable, machine-checkable description of the command tree.
//!
//! This module implements the data model from RFC 0132: CLI Patterns.
//! `CommandSpec` is derived from the `CommandRegistry` (RFC 0085) to ensure
//! the specification and implementation cannot diverge.
//!
//! # Design Principles
//!
//! - **Spec is Law**: The `CommandSpec` is the authoritative definition of
//!   available commands, their arguments, effects, and constraints.
//! - **Derived from Reality**: Generated from actual Command trait implementations,
//!   not written by hand.
//! - **Multiple Projections**: From `CommandSpec` we can derive:
//!   - CLI help text
//!   - LM tool JSON schemas
//!   - Capability tree (RFC 0125)
//!   - Documentation
//!
//! # Usage
//!
//! ```ignore
//! use crate::command::registry::default_registry;
//! use crate::command::command_spec::CommandSpec;
//!
//! let registry = default_registry();
//! let spec = CommandSpec::from_registry(&registry);
//!
//! // Generate LM tool schema
//! let schema = spec.to_lm_tool_schema();
//!
//! // Generate CLI help
//! let help = spec.namespace("phase").unwrap().to_help_text();
//! ```

use crate::api::protocol::{Effect, RecoveryClass};
use crate::command::lm_tool_metadata;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The root specification for all commands.
///
/// `CommandSpec` organizes commands hierarchically by namespace, then operation.
/// This mirrors the CLI structure: `exo <namespace> <operation> [args...]`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Version of the spec format (for forward compatibility).
    pub version: u32,
    /// Map from namespace name to namespace specification.
    pub namespaces: BTreeMap<String, NamespaceSpec>,
    /// Root operations without a namespace (e.g., `status`, `map`).
    pub root_operations: BTreeMap<String, OperationSpec>,
    /// Total count of operations across all namespaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_count: Option<usize>,
}

impl Default for CommandSpec {
    fn default() -> Self {
        Self {
            version: 1,
            namespaces: BTreeMap::new(),
            root_operations: BTreeMap::new(),
            operation_count: None,
        }
    }
}

/// Specification for a command namespace.
///
/// A namespace groups related operations. For example:
/// - `phase` contains `start`, `finish`, `status`
/// - `task` contains `add`, `complete`, `list`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceSpec {
    /// The namespace name (e.g., "phase", "task").
    pub name: String,
    /// Human-readable description of the namespace.
    pub description: String,
    /// Operations in this namespace.
    pub operations: BTreeMap<String, OperationSpec>,
}

/// Metadata for LM tool presentation and package.json contributions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LmToolMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(rename = "when", skip_serializing_if = "Option::is_none")]
    pub when_clause: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_reference_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_be_referenced_in_prompt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_sets: Option<Vec<String>>,
}

impl NamespaceSpec {
    /// Create a new namespace specification.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            operations: BTreeMap::new(),
        }
    }

    /// Add an operation to this namespace.
    pub fn with_operation(mut self, op: OperationSpec) -> Self {
        self.operations.insert(op.name.clone(), op);
        self
    }

    /// Get an operation by name.
    pub fn operation(&self, name: &str) -> Option<&OperationSpec> {
        self.operations.get(name)
    }

    /// Generate help text for this namespace.
    pub fn to_help_text(&self) -> String {
        let mut help = format!(
            "# {}\n\n{}\n\n## Operations\n\n",
            self.name, self.description
        );

        for (name, op) in &self.operations {
            let effect_badge = match op.effect {
                Effect::Pure => "🔍",
                Effect::Write => "✏️",
                Effect::Exec => "⚡",
            };
            help.push_str(&format!(
                "- `{}` {} - {}\n",
                name, effect_badge, op.description
            ));
        }

        help
    }
}

/// Specification for a single operation.
///
/// An operation is a leaf command that performs an action.
/// For example: `phase start`, `task complete`, `rfc create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSpec {
    /// The operation name (e.g., "start", "complete").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Effect classification (Pure, Write, Exec).
    pub effect: Effect,
    /// Recovery behavior after daemon replacement.
    pub recovery_class: RecoveryClass,
    /// Whether this operation requires an upgrade gate check.
    pub needs_upgrade_gate: bool,
    /// Arguments this operation accepts.
    pub args: Vec<ArgSpec>,
    /// Example usage (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// LM tool presentation metadata (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lm_tool: Option<LmToolMetadata>,
}

impl OperationSpec {
    /// Create a new operation specification.
    pub fn new(name: impl Into<String>, description: impl Into<String>, effect: Effect) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            effect,
            recovery_class: if effect == Effect::Pure {
                RecoveryClass::ReplayableRead
            } else {
                RecoveryClass::ExternalAtMostOnce
            },
            needs_upgrade_gate: false,
            args: Vec::new(),
            example: None,
            lm_tool: None,
        }
    }

    /// Set the upgrade gate requirement.
    pub const fn with_upgrade_gate(mut self, needs: bool) -> Self {
        self.needs_upgrade_gate = needs;
        self
    }

    /// Add an argument specification.
    pub fn with_arg(mut self, arg: ArgSpec) -> Self {
        self.args.push(arg);
        self
    }

    /// Set an example.
    pub fn with_example(mut self, example: impl Into<String>) -> Self {
        self.example = Some(example.into());
        self
    }

    /// Attach LM tool metadata to this operation.
    pub fn with_lm_tool(mut self, metadata: LmToolMetadata) -> Self {
        self.lm_tool = Some(metadata);
        self
    }

    /// Check if this operation qualifies for zero-arg LM tool projection.
    ///
    /// Per RFC 0132, an operation qualifies for zero-arg orientation if:
    /// - Effect is Pure (no side effects)
    /// - All arguments are optional or have defaults
    pub fn is_zero_arg(&self) -> bool {
        self.effect == Effect::Pure && self.args.iter().all(|a| a.optional || a.default.is_some())
    }

    /// Get the fully qualified name (namespace.operation).
    pub fn full_name(&self, namespace: &str) -> String {
        format!("{}.{}", namespace, self.name)
    }

    /// Generate help text for this operation.
    ///
    /// Includes effect badge, argument descriptions, and example if available.
    pub fn to_help_text(&self, namespace: &str) -> String {
        let effect_badge = match self.effect {
            Effect::Pure => "🔍 Pure",
            Effect::Write => "✏️  Write",
            Effect::Exec => "⚡ Exec",
        };

        let mut help = format!(
            "# exo {} {}\n\n{}\n\nEffect: {}\n",
            namespace, self.name, self.description, effect_badge
        );

        if !self.args.is_empty() {
            help.push_str("\n## Arguments\n\n");
            for arg in &self.args {
                let req_marker = if arg.optional {
                    "(optional)"
                } else {
                    "(required)"
                };
                let kind_str = match arg.kind {
                    ArgKind::Flag => "flag",
                    ArgKind::Option => "option",
                    ArgKind::Positional => "positional",
                };

                help.push_str(&format!(
                    "- `{}` ({} {}) - {}\n",
                    arg.name, kind_str, req_marker, arg.description
                ));

                if let Some(default) = &arg.default {
                    help.push_str(&format!("  Default: `{default}`\n"));
                }
            }
        }

        if let Some(example) = &self.example {
            help.push_str(&format!("\n## Example\n\n```\n{example}\n```\n"));
        }

        help
    }
}

/// Specification for a command argument.
///
/// Arguments come in three forms per RFC 0132:
/// - Flag: boolean presence (`--verbose`)
/// - Option: key/value (`--limit 20`)
/// - Positional: ordered values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgSpec {
    /// Stable identifier for this argument.
    pub id: String,
    /// Display name (may differ from id for renames).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// The argument kind (flag, option, positional).
    pub kind: ArgKind,
    /// The value type.
    pub value_type: ValueType,
    /// Whether this argument is optional.
    pub optional: bool,
    /// Default value (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Short flag alias (e.g., "-v" for "--verbose").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    /// Whether this argument can be provided multiple times.
    #[serde(default)]
    pub repeatable: bool,
}

impl ArgSpec {
    /// Create a new flag argument.
    pub fn flag(id: impl Into<String>, description: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            description: description.into(),
            kind: ArgKind::Flag,
            value_type: ValueType::Bool,
            optional: true, // Flags are always optional
            default: Some("false".to_string()),
            short: None,
            repeatable: false,
        }
    }

    /// Create a new option argument.
    pub fn option(
        id: impl Into<String>,
        description: impl Into<String>,
        value_type: ValueType,
    ) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            description: description.into(),
            kind: ArgKind::Option,
            value_type,
            optional: false,
            default: None,
            short: None,
            repeatable: false,
        }
    }

    /// Create a new positional argument.
    pub fn positional(
        id: impl Into<String>,
        description: impl Into<String>,
        value_type: ValueType,
    ) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            description: description.into(),
            kind: ArgKind::Positional,
            value_type,
            optional: false,
            default: None,
            short: None,
            repeatable: false,
        }
    }

    /// Mark as optional.
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Set a default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self.optional = true; // Default implies optional
        self
    }

    /// Set a short alias.
    pub const fn with_short(mut self, short: char) -> Self {
        self.short = Some(short);
        self
    }

    /// Mark as repeatable (can be provided multiple times).
    pub const fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }
}

/// The kind of argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgKind {
    /// Boolean presence flag (`--verbose`).
    Flag,
    /// Key/value option (`--limit 20`).
    Option,
    /// Ordered positional value.
    Positional,
}

/// The type of an argument value.
///
/// Per RFC 0132, values are typed at parse time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// Boolean value.
    Bool,
    /// Integer value.
    Int,
    /// Floating point value.
    Float,
    /// String value.
    String,
    /// File path.
    Path,
    /// JSON value (parsed inline).
    Json,
    /// Enumeration of allowed values.
    Enum(Vec<String>),
}

impl ValueType {
    /// Get the JSON schema type for this value type.
    pub const fn json_schema_type(&self) -> &'static str {
        match self {
            Self::Bool => "boolean",
            Self::Int => "integer",
            Self::Float => "number",
            Self::String | Self::Path => "string",
            Self::Json => "object",
            Self::Enum(_) => "string",
        }
    }

    /// Generate a full JSON Schema object for this value type.
    ///
    /// This is used when generating LM tool parameter schemas.
    pub fn to_json_schema(&self) -> serde_json::Value {
        match self {
            Self::Bool => serde_json::json!({ "type": "boolean" }),
            Self::Int => serde_json::json!({ "type": "integer" }),
            Self::Float => serde_json::json!({ "type": "number" }),
            Self::String => serde_json::json!({ "type": "string" }),
            Self::Path => serde_json::json!({
                "type": "string",
                "description": "File or directory path"
            }),
            Self::Json => serde_json::json!({
                "type": "object",
                "additionalProperties": true
            }),
            Self::Enum(variants) => serde_json::json!({
                "type": "string",
                "enum": variants
            }),
        }
    }
}

impl CommandSpec {
    /// Create a new empty `CommandSpec`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate `CommandSpec` from a `CommandRegistry`.
    ///
    /// This is the primary way to create a `CommandSpec`. It introspects
    /// the registry's commands and builds the spec from their metadata.
    pub fn from_registry(registry: &super::registry::CommandRegistry) -> Self {
        let mut spec = Self::new();

        for cmd in registry.iter() {
            let namespace = cmd.namespace();
            let operation = cmd.operation();

            if namespace.is_empty() {
                let mut op = OperationSpec::new(operation, cmd.description(), cmd.effect());
                op.lm_tool = lm_tool_metadata::lookup(namespace, operation);
                spec.root_operations.insert(operation.to_string(), op);
                continue;
            }

            // Get or create namespace
            let ns = spec
                .namespaces
                .entry(namespace.to_string())
                .or_insert_with(|| {
                    NamespaceSpec::new(namespace, format!("{namespace} operations"))
                });

            let mut op = OperationSpec::new(operation, cmd.description(), cmd.effect());

            op.lm_tool = lm_tool_metadata::lookup(namespace, operation);

            ns.operations.insert(operation.to_string(), op);
        }

        // Override with ExoSpec-generated namespaces (authoritative source)
        spec.merge_exospec::<super::epoch::EpochCommands>();
        spec.merge_exospec::<super::strike::StrikeCommands>();
        spec.merge_exospec::<super::task::TaskCommands>();
        spec.merge_exospec::<super::phase_cmd::PhaseCommands>();
        spec.merge_exospec::<super::goal::GoalCommands>();
        spec.merge_exospec::<super::gc::GcCommands>();
        spec.merge_exospec::<super::verify::VerifyCommands>();
        spec.merge_exospec::<super::context::ContextCommands>();
        spec.merge_exospec::<super::axiom::AxiomCommands>();
        spec.merge_exospec::<super::inbox::InboxCommands>();
        spec.merge_exospec::<super::toml::TomlCommands>();
        spec.merge_exospec::<super::run::RunCommands>();
        spec.merge_exospec::<super::commit::CommitCommands>();
        spec.merge_exospec::<super::idea::IdeaCommands>();
        spec.merge_exospec::<super::plan::PlanCommands>();
        spec.merge_exospec::<super::project::ProjectCommands>();
        spec.merge_exospec::<super::sidecar::SidecarCommands>();
        spec.merge_exospec::<super::json::JsonCommands>();
        spec.merge_exospec::<super::docs::DocsCommands>();
        spec.merge_exospec::<super::ai::AiCommands>();
        spec.merge_exospec::<super::dogfood::DogfoodCommands>();
        spec.merge_exospec::<super::rfc::RfcCommands>();

        // Override root operations with ExoSpec-generated definitions
        spec.merge_exospec_root::<super::root::RootCommands>();

        // ExoSpec owns syntax while the registered command owns runtime
        // behavior. Reapply effect and recovery after ExoSpec replacement.
        for cmd in registry.iter() {
            let operation = if cmd.namespace().is_empty() {
                spec.root_operations.get_mut(cmd.operation())
            } else {
                spec.namespaces
                    .get_mut(cmd.namespace())
                    .and_then(|namespace| namespace.operations.get_mut(cmd.operation()))
            };
            if let Some(operation) = operation {
                operation.effect = cmd.effect();
                operation.recovery_class = cmd.recovery_class();
            }
        }

        // Calculate total operation count
        let namespace_ops: usize = spec.namespaces.values().map(|ns| ns.operations.len()).sum();
        spec.operation_count = Some(namespace_ops + spec.root_operations.len());

        spec
    }

    /// Merge a `HasExoSpec`-generated namespace into this spec.
    ///
    /// This replaces the namespace entry (if any) with the macro-generated
    /// `NamespaceSpec`, which includes the authoritative description and
    /// all operation metadata. LM tool metadata is preserved from any
    /// existing entry.
    pub fn merge_exospec<T: HasExoSpec>(&mut self) {
        let ns = T::spec();
        let name = ns.name.clone();

        // Preserve LM tool metadata from existing entries
        if let Some(existing) = self.namespaces.get(&name) {
            let mut merged = ns;
            for (op_name, merged_op) in &mut merged.operations {
                if let Some(existing_op) = existing.operations.get(op_name)
                    && merged_op.lm_tool.is_none()
                {
                    merged_op.lm_tool.clone_from(&existing_op.lm_tool);
                }
            }
            self.namespaces.insert(name, merged);
        } else {
            self.namespaces.insert(name, ns);
        }
    }

    /// Merge a `HasExoSpec`-generated namespace (with empty name) into root operations.
    ///
    /// This is the root-command counterpart of `merge_exospec`. It takes the
    /// operations from a `NamespaceSpec` with `name = ""` and merges them into
    /// `self.root_operations`, preserving LM tool metadata from existing entries.
    pub fn merge_exospec_root<T: HasExoSpec>(&mut self) {
        let ns = T::spec();
        debug_assert!(
            ns.name.is_empty(),
            "merge_exospec_root expects namespace = \"\", got {:?}",
            ns.name
        );

        for (op_name, mut op) in ns.operations {
            // Preserve LM tool metadata from existing entries
            if let Some(existing) = self.root_operations.get(&op_name)
                && op.lm_tool.is_none()
            {
                op.lm_tool.clone_from(&existing.lm_tool);
            }
            self.root_operations.insert(op_name, op);
        }
    }

    /// Get a namespace by name.
    pub fn namespace(&self, name: &str) -> Option<&NamespaceSpec> {
        self.namespaces.get(name)
    }

    /// Get all namespace names.
    pub fn namespace_names(&self) -> Vec<&str> {
        self.namespaces
            .keys()
            .map(std::string::String::as_str)
            .collect()
    }

    /// Get an operation by namespace and name.
    pub fn operation(&self, namespace: &str, operation: &str) -> Option<&OperationSpec> {
        if namespace.is_empty() {
            return self.root_operations.get(operation);
        }

        self.namespace(namespace)
            .and_then(|ns| ns.operation(operation))
    }

    /// Iterate over all operations, including root operations with empty namespace.
    pub fn iter_all_operations(&self) -> impl Iterator<Item = (&str, &str, &OperationSpec)> {
        let root_iter = self
            .root_operations // iter_all_operations
            .iter()
            .map(|(op_name, op)| ("", op_name.as_str(), op));

        let namespace_iter = self.namespaces.iter().flat_map(|(ns_name, ns)| {
            ns.operations
                .iter()
                .map(move |(op_name, op)| (ns_name.as_str(), op_name.as_str(), op))
        });

        root_iter.chain(namespace_iter)
    }

    /// Find all operations that qualify for zero-arg LM tool projection.
    pub fn zero_arg_operations(&self) -> Vec<(&str, &OperationSpec)> {
        self.namespaces
            .iter()
            .flat_map(|(ns_name, ns)| {
                ns.operations
                    .values()
                    .filter(|op| op.is_zero_arg())
                    .map(move |op| (ns_name.as_str(), op))
            })
            .collect()
    }

    /// Find all namespaces that qualify for method-based LM tool dispatch.
    ///
    /// A namespace qualifies if it has multiple operations.
    pub fn method_dispatch_namespaces(&self) -> Vec<&NamespaceSpec> {
        self.namespaces
            .values()
            .filter(|ns| ns.operations.len() > 1)
            .collect()
    }

    /// Generate JSON schema for LM tools.
    ///
    /// This creates a schema suitable for VS Code language model tools
    /// per RFC 0132's tool projection strategy.
    ///
    /// # Projection Strategy
    ///
    /// 1. **Method-dispatch tools**: Namespaces with multiple operations get a single
    ///    tool with a `method` parameter to select the operation.
    /// 2. **Zero-arg tools**: Pure operations with no required arguments get their
    ///    own dedicated tools for easy discovery/orientation.
    pub fn to_lm_tool_schemas(&self) -> BTreeMap<String, serde_json::Value> {
        let mut schemas = BTreeMap::new();

        // Generate method-dispatch tools for namespaces
        for ns in self.method_dispatch_namespaces() {
            let tool_name = format!("exo-{}", ns.name);

            // Build method enum
            let methods: Vec<String> = ns.operations.keys().cloned().collect();

            // Collect all unique arguments across operations
            let mut all_properties = serde_json::Map::new();
            let mut all_descriptions = Vec::new();

            // Add method parameter
            all_properties.insert(
                "method".to_string(),
                serde_json::json!({
                    "type": "string",
                    "enum": methods,
                    "description": "The operation to perform"
                }),
            );

            // Collect parameters from all operations
            for op in ns.operations.values() {
                for arg in &op.args {
                    if !all_properties.contains_key(&arg.name) {
                        let mut schema = arg.value_type.to_json_schema();
                        if let Some(obj) = schema.as_object_mut() {
                            obj.insert(
                                "description".to_string(),
                                serde_json::Value::String(arg.description.clone()),
                            );
                        }
                        all_properties.insert(arg.name.clone(), schema);
                    }
                }
                all_descriptions.push(format!("- {}: {}", op.name, op.description));
            }

            // Build description with all operations listed
            let full_description = format!(
                "{}\n\nOperations:\n{}",
                ns.description,
                all_descriptions.join("\n")
            );

            let schema = serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": full_description,
                    "parameters": {
                        "type": "object",
                        "properties": all_properties,
                        "required": ["method"]
                    }
                }
            });

            schemas.insert(tool_name, schema);
        }

        // Generate zero-arg orientation tools
        for (ns_name, op) in self.zero_arg_operations() {
            let tool_name = format!("exo-{}-{}", ns_name, op.name);

            // Skip if namespace already has a method-dispatch tool
            // (zero-arg ops in multi-op namespaces are called via method param)
            if self
                .namespace(ns_name)
                .is_some_and(|ns| ns.operations.len() > 1)
            {
                continue;
            }

            let schema = serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": op.description,
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            });

            schemas.insert(tool_name, schema);
        }

        // Generate root operation tools (e.g., status, map)
        for (ns_name, op_name, op) in self.iter_all_operations() {
            if !ns_name.is_empty() {
                continue;
            }
            let tool_name = format!("exo-{op_name}");

            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for arg in &op.args {
                let mut schema = arg.value_type.to_json_schema();
                if let Some(obj) = schema.as_object_mut() {
                    obj.insert(
                        "description".to_string(),
                        serde_json::Value::String(arg.description.clone()),
                    );
                }
                properties.insert(arg.name.clone(), schema);

                if !arg.optional && arg.default.is_none() {
                    required.push(arg.name.clone());
                }
            }

            let schema = serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": op.description,
                    "parameters": {
                        "type": "object",
                        "properties": properties,
                        "required": required,
                    }
                }
            });

            schemas.insert(tool_name, schema);
        }

        schemas
    }

    /// Generate complete CLI help text for all commands.
    ///
    /// This creates a comprehensive reference document listing all namespaces
    /// and operations with their effects and arguments.
    pub fn to_full_help(&self) -> String {
        let mut help = String::from("# exo CLI Reference\n\n");
        help.push_str(&format!(
            "Version: {} | {} namespaces | {} operations\n\n",
            self.version,
            self.namespaces.len(),
            self.operation_count.unwrap_or(0)
        ));

        help.push_str("## Namespaces\n\n");
        for ns_name in self.namespace_names() {
            help.push_str(&format!("- `{ns_name}`\n"));
        }
        help.push_str("\n---\n\n");

        for (ns_name, ns) in &self.namespaces {
            help.push_str(&format!("## {}\n\n{}\n\n", ns_name, ns.description));

            for (op_name, op) in &ns.operations {
                let effect_badge = match op.effect {
                    Effect::Pure => "🔍",
                    Effect::Write => "✏️",
                    Effect::Exec => "⚡",
                };
                help.push_str(&format!(
                    "### `exo {} {}` {}\n\n{}\n\n",
                    ns_name, op_name, effect_badge, op.description
                ));

                if !op.args.is_empty() {
                    help.push_str("**Arguments:**\n");
                    for arg in &op.args {
                        let req = if arg.optional { "optional" } else { "required" };
                        help.push_str(&format!(
                            "- `{}` ({}) - {}\n",
                            arg.name, req, arg.description
                        ));
                    }
                    help.push('\n');
                }
            }
        }

        help
    }

    /// Generate help text for a specific namespace.
    pub fn namespace_help(&self, name: &str) -> Option<String> {
        self.namespace(name).map(NamespaceSpec::to_help_text)
    }

    /// Generate help text for a specific operation.
    pub fn operation_help(&self, namespace: &str, operation: &str) -> Option<String> {
        self.operation(namespace, operation)
            .map(|op| op.to_help_text(namespace))
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Serialize to JSON string.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

// ============================================================================
// HasExoSpec trait — the target for #[derive(ExoSpec)]
// ============================================================================

/// Trait implemented by `#[derive(ExoSpec)]` on namespace enums.
///
/// Each namespace enum (e.g., `TddCommands`) derives this trait, which provides
/// the full `NamespaceSpec` at compile time. This replaces the hand-written
/// argument metadata and the runtime `CommandSpec::from_registry()` path.
///
/// # Design
///
/// - The proc macro generates the `spec()` implementation from `#[exo(...)]` attributes.
/// - `from_invocation()` constructs the typed enum variant from an `Invocation`.
///
/// # Example (after migration)
///
/// ```ignore
/// #[derive(ExoSpec)]
/// #[exo(namespace = "tdd", description = "TDD workflow commands")]
/// enum TddCommands {
///     #[exo(effect = "exec")]
///     New {
///         #[exo(long, short = 'n')]
///         name: String,
///         #[exo(long, short = 't')]
///         test: String,
///     },
///     #[exo(effect = "exec")]
///     Red,
///     #[exo(effect = "exec")]
///     Green,
/// }
/// ```
pub trait HasExoSpec {
    /// Returns the full namespace specification for this command group.
    ///
    /// The returned `NamespaceSpec` contains all operations, their arguments,
    /// effects, and metadata — everything needed for CLI help, LM tool schemas,
    /// and the capability tree.
    fn spec() -> NamespaceSpec;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::registry::default_registry;

    #[test]
    fn test_command_spec_from_registry() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        // Should have multiple namespaces
        assert!(!spec.namespaces.is_empty());

        // Should have operation count
        assert!(spec.operation_count.is_some());
        let count = spec.operation_count.unwrap();
        assert!(count > 0);

        // Should have epoch namespace with list operation
        let epoch = spec
            .namespace("epoch")
            .expect("epoch namespace should exist");
        assert!(epoch.operation("list").is_some());
        assert!(epoch.operation("review").is_some());
    }

    #[test]
    fn command_spec_recovery_classes_match_registered_commands() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        for command in registry.metadata() {
            let operation = if command.namespace.is_empty() {
                spec.root_operations.get(command.operation)
            } else {
                spec.namespaces
                    .get(command.namespace)
                    .and_then(|namespace| namespace.operations.get(command.operation))
            }
            .expect("registered command should be present in the command spec");

            assert_eq!(
                operation.recovery_class, command.recovery_class,
                "recovery class drift for {} {}",
                command.namespace, command.operation
            );
            assert_eq!(
                operation.effect, command.effect,
                "effect drift for {} {}",
                command.namespace, command.operation
            );
        }

        assert_eq!(
            spec.namespace("context")
                .and_then(|namespace| namespace.operation("restore"))
                .map(|operation| operation.recovery_class),
            Some(RecoveryClass::ReplayableRead)
        );
        assert_eq!(
            spec.namespace("sidecar")
                .and_then(|namespace| namespace.operation("repo"))
                .map(|operation| operation.recovery_class),
            Some(RecoveryClass::ExternalAtMostOnce)
        );
    }

    #[test]
    fn test_command_spec_includes_root_operations() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        assert!(spec.root_operations.contains_key("status"));
        assert!(spec.root_operations.contains_key("map"));
    }

    #[test]
    fn test_iter_all_operations_includes_root_and_namespaced() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);
        let ops: Vec<_> = spec.iter_all_operations().collect();

        assert!(ops.iter().any(|(ns, op, _)| *ns == "" && *op == "status"));
        assert!(
            ops.iter()
                .any(|(ns, op, _)| *ns == "epoch" && *op == "list")
        );
        assert_eq!(ops.len(), spec.operation_count.unwrap());
    }

    #[test]
    fn test_operation_spec_zero_arg() {
        // Pure operation with no required args
        let op = OperationSpec::new("list", "List items", Effect::Pure);
        assert!(op.is_zero_arg());

        // Write operation is never zero-arg
        let op = OperationSpec::new("add", "Add item", Effect::Write);
        assert!(!op.is_zero_arg());

        // Pure with required arg
        let op = OperationSpec::new("show", "Show item", Effect::Pure)
            .with_arg(ArgSpec::positional("id", "Item ID", ValueType::String));
        assert!(!op.is_zero_arg());

        // Pure with optional arg
        let op = OperationSpec::new("show", "Show item", Effect::Pure)
            .with_arg(ArgSpec::positional("id", "Item ID", ValueType::String).optional());
        assert!(op.is_zero_arg());
    }

    #[test]
    fn test_arg_spec_builders() {
        let flag = ArgSpec::flag("verbose", "Enable verbose output").with_short('v');
        assert_eq!(flag.kind, ArgKind::Flag);
        assert!(flag.optional);
        assert_eq!(flag.short, Some('v'));

        let opt = ArgSpec::option("limit", "Max results", ValueType::Int).with_default("10");
        assert_eq!(opt.kind, ArgKind::Option);
        assert!(opt.optional); // default implies optional
        assert_eq!(opt.default, Some("10".to_string()));

        let pos = ArgSpec::positional("path", "File path", ValueType::Path);
        assert_eq!(pos.kind, ArgKind::Positional);
        assert!(!pos.optional);
    }

    #[test]
    fn test_arg_spec_repeatable() {
        let arg = ArgSpec::option("items", "Items to add", ValueType::String).repeatable();
        assert!(arg.repeatable);

        let arg2 = ArgSpec::option("limit", "Limit", ValueType::Int);
        assert!(!arg2.repeatable);
    }

    #[test]
    fn test_namespace_help_text() {
        let ns = NamespaceSpec::new("phase", "Phase lifecycle operations")
            .with_operation(OperationSpec::new("start", "Start a phase", Effect::Exec))
            .with_operation(OperationSpec::new("finish", "Finish a phase", Effect::Exec))
            .with_operation(OperationSpec::new(
                "status",
                "Show phase status",
                Effect::Pure,
            ));

        let help = ns.to_help_text();
        assert!(help.contains("# phase"));
        assert!(help.contains("Phase lifecycle operations"));
        assert!(help.contains("`start`"));
        assert!(help.contains("`finish`"));
        assert!(help.contains("`status`"));
    }

    #[test]
    fn test_lm_tool_schema_generation() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);
        let schemas = spec.to_lm_tool_schemas();

        // Should have generated some schemas
        assert!(!schemas.is_empty());

        // Root operation tools should be present
        assert!(schemas.contains_key("exo-status"));
        assert!(schemas.contains_key("exo-map"));

        // Check epoch tool schema
        if let Some(epoch_schema) = schemas.get("exo-epoch") {
            let methods = epoch_schema
                .pointer("/function/parameters/properties/method/enum")
                .expect("should have method enum");
            assert!(methods.is_array());

            // Check description includes operations list
            let description = epoch_schema
                .pointer("/function/description")
                .expect("should have description")
                .as_str()
                .expect("description should be string");
            assert!(description.contains("Operations:"));
        }
    }

    #[test]
    fn test_value_type_to_json_schema() {
        // Bool
        let schema = ValueType::Bool.to_json_schema();
        assert_eq!(schema["type"], "boolean");

        // Int
        let schema = ValueType::Int.to_json_schema();
        assert_eq!(schema["type"], "integer");

        // Float
        let schema = ValueType::Float.to_json_schema();
        assert_eq!(schema["type"], "number");

        // String
        let schema = ValueType::String.to_json_schema();
        assert_eq!(schema["type"], "string");

        // Path
        let schema = ValueType::Path.to_json_schema();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("description").is_some());

        // Json
        let schema = ValueType::Json.to_json_schema();
        assert_eq!(schema["type"], "object");

        // Enum
        let schema = ValueType::Enum(vec!["a".into(), "b".into()]).to_json_schema();
        assert_eq!(schema["type"], "string");
        assert!(schema["enum"].is_array());
    }

    #[test]
    fn test_lm_tool_schema_includes_parameters() {
        // Create a namespace with operations that have arguments
        let mut spec = CommandSpec::new();
        let ns = NamespaceSpec::new("test", "Test namespace")
            .with_operation(
                OperationSpec::new("create", "Create item", Effect::Write)
                    .with_arg(ArgSpec::positional("name", "Item name", ValueType::String))
                    .with_arg(ArgSpec::option("count", "Number of items", ValueType::Int)),
            )
            .with_operation(OperationSpec::new("list", "List items", Effect::Pure));

        spec.namespaces.insert("test".into(), ns);
        spec.operation_count = Some(2);

        let schemas = spec.to_lm_tool_schemas();
        let test_schema = schemas.get("exo-test").expect("should have test schema");

        // Should have method param
        let method = test_schema.pointer("/function/parameters/properties/method");
        assert!(method.is_some());

        // Should have name param from create operation
        let name = test_schema.pointer("/function/parameters/properties/name");
        assert!(name.is_some());
        assert_eq!(name.unwrap()["type"], "string");

        // Should have count param from create operation
        let count = test_schema.pointer("/function/parameters/properties/count");
        assert!(count.is_some());
        assert_eq!(count.unwrap()["type"], "integer");
    }

    #[test]
    fn test_method_dispatch_namespaces() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let dispatch_ns = spec.method_dispatch_namespaces();
        // Namespaces with multiple operations should be included
        for ns in dispatch_ns {
            assert!(ns.operations.len() > 1);
        }
    }

    #[test]
    fn test_command_spec_serialization() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        // Should serialize to JSON
        let json = spec.to_json_pretty();
        assert!(!json.is_empty());
        assert!(json.contains("namespaces"));

        // Should round-trip
        let parsed: CommandSpec = serde_json::from_str(&json).expect("should parse");
        assert_eq!(parsed.version, spec.version);
        assert_eq!(parsed.namespaces.len(), spec.namespaces.len());
    }

    #[test]
    fn test_operation_to_help_text() {
        let op = OperationSpec::new("start", "Start a new phase", Effect::Exec)
            .with_arg(ArgSpec::positional(
                "id",
                "Phase identifier",
                ValueType::String,
            ))
            .with_arg(ArgSpec::option("message", "Optional message", ValueType::String).optional())
            .with_example("exo phase start my-phase");

        let help = op.to_help_text("phase");

        assert!(help.contains("# exo phase start"));
        assert!(help.contains("Start a new phase"));
        assert!(help.contains("⚡ Exec"));
        assert!(help.contains("`id`"));
        assert!(help.contains("(required)"));
        assert!(help.contains("`message`"));
        assert!(help.contains("(optional)"));
        assert!(help.contains("## Example"));
        assert!(help.contains("exo phase start my-phase"));
    }

    #[test]
    fn test_command_spec_to_full_help() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let help = spec.to_full_help();

        assert!(help.contains("# exo CLI Reference"));
        assert!(help.contains("## Namespaces"));
        // Should have at least some namespaces listed
        assert!(help.contains("`phase`") || help.contains("`epoch`") || help.contains("`task`"));
    }

    #[test]
    fn test_namespace_help() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        // Should return help for existing namespace
        let help = spec.namespace_help("epoch");
        assert!(help.is_some());
        let help_text = help.unwrap();
        assert!(help_text.contains("# epoch"));

        // Should return None for non-existent namespace
        assert!(spec.namespace_help("nonexistent").is_none());
    }

    #[test]
    fn test_operation_help() {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        // Should return help for existing operation
        if let Some(help) = spec.operation_help("epoch", "list") {
            assert!(help.contains("exo epoch list"));
        }

        // Should return None for non-existent operation
        assert!(spec.operation_help("epoch", "nonexistent").is_none());
        assert!(spec.operation_help("nonexistent", "list").is_none());
    }
}
