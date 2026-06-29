//! JSON namespace commands.
//!
//! - `json read`: Read a JSON file (Pure)
//! - `json write`: Write to a JSON file (Write)
//! - `json schema`: Generate JSON schema (Pure)
//! - `json spec`: Generate `CommandSpec` from registry (Pure) - RFC 0132
//! - `json lm-tools`: Generate VS Code languageModelTools contributions (Pure)
//! - `json package-tools`: Generate package.json languageModelTools contributions (Pure)
//!
//! Note: `json server` is a special long-running protocol mode and is not
//! migrated to the command trait architecture.

use super::command_spec::CommandSpec;
use super::lm_tool_metadata;
use super::registry::default_registry;
use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::json_schema;
use crate::structured_io;
use anyhow::Result as ExoResult;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ============================================================================
// ExoSpec definition — single source of truth for the json namespace
// ============================================================================

/// Json namespace command specification.
///
/// This enum is the authoritative definition of the json namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `JsonCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(
    namespace = "json",
    description = "JSON read/write and schema generation commands"
)]
pub enum JsonCommands {
    #[exo(effect = "pure", description = "Read a value from a JSON file")]
    Read {
        #[exo(positional, description = "Path to the JSON file")]
        path: String,
        #[exo(
            long,
            optional,
            description = "JSON pointer to the value (e.g., /key/0)"
        )]
        pointer: Option<String>,
    },

    #[exo(effect = "write", description = "Write a value to a JSON file")]
    Write {
        #[exo(positional, description = "Path to the JSON file")]
        path: String,
        #[exo(positional, description = "JSON pointer to the value")]
        pointer: String,
        #[exo(
            positional,
            description = "Value to write (parsed as JSON if possible)"
        )]
        value: String,
    },

    #[exo(effect = "pure", description = "Generate JSON schema for exo commands")]
    Schema {
        #[exo(positional, description = "Output style: 'full' or 'lm-tool'")]
        style: String,
    },

    #[exo(
        effect = "pure",
        description = "Generate CommandSpec from registry (RFC 0132)"
    )]
    Spec,

    #[exo(
        effect = "write",
        description = "Generate CommandSpec artifact for VS Code extension"
    )]
    Artifact {
        #[exo(long, optional, description = "Output path (relative or absolute)")]
        output: Option<String>,
    },

    #[exo(
        effect = "pure",
        operation = "lm-tools",
        description = "Generate VS Code languageModelTools contribution format"
    )]
    LmTools {
        #[exo(
            long,
            optional,
            short = 'n',
            description = "Filter to a specific namespace (e.g., 'phase', 'epoch')"
        )]
        namespace: Option<String>,
    },

    #[exo(
        effect = "pure",
        operation = "package-tools",
        description = "Generate package.json languageModelTools contribution format"
    )]
    PackageTools,
}

impl JsonCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Read { path, pointer } => CommandBox::pure(JsonRead::new(path, pointer)),
            Self::Write {
                path,
                pointer,
                value,
            } => CommandBox::mutable(JsonWrite::new(path, pointer, value)),
            Self::Schema { style } => CommandBox::pure(JsonSchema::new(style)),
            Self::Spec => CommandBox::pure(JsonSpec),
            Self::Artifact { output } => {
                CommandBox::mutable(JsonArtifact::new(output.map(PathBuf::from)))
            }
            Self::LmTools { namespace } => CommandBox::pure(JsonLmTools::new(namespace)),
            Self::PackageTools => CommandBox::pure(JsonPackageTools),
        })
    }
}

// ============================================================================
// json read
// ============================================================================

/// Read a value from a JSON file.
#[derive(Debug, Clone)]
pub struct JsonRead {
    pub path: PathBuf,
    pub pointer: Option<String>,
}

impl JsonRead {
    pub fn new(path: impl Into<PathBuf>, pointer: Option<String>) -> Self {
        Self {
            path: path.into(),
            pointer,
        }
    }
}

impl Command for JsonRead {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read a value from a JSON file"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let full_path = ctx.root.join(&self.path);

        match ctx.format {
            OutputFormat::Json => {
                let file_contents = std::fs::read_to_string(&full_path)?;
                let json: serde_json::Value = serde_json::from_str(&file_contents)?;
                let value = if let Some(ptr) = self.pointer.as_deref() {
                    json.pointer(ptr)
                        .ok_or_else(|| anyhow::anyhow!("Pointer not found"))?
                        .clone()
                } else {
                    json
                };
                Ok(CommandOutput::data(value))
            }
            OutputFormat::Human => {
                let value = structured_io::read_json(&full_path, self.pointer.as_deref())?;
                Ok(CommandOutput::message(value))
            }
        }
    }
}

// ============================================================================
// json write
// ============================================================================

/// Write a value to a JSON file.
#[derive(Debug, Clone)]
pub struct JsonWrite {
    pub path: PathBuf,
    pub pointer: String,
    pub value: String,
}

impl JsonWrite {
    pub fn new(
        path: impl Into<PathBuf>,
        pointer: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            pointer: pointer.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonWriteOutput {
    kind: &'static str,
    ok: bool,
}

impl Command for JsonWrite {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write a value to a JSON file"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("JsonWrite should be dispatched via execute_mut")
    }
}

impl MutableCommand for JsonWrite {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let full_path = ctx.root.join(&self.path);
        structured_io::write_json(&full_path, &self.pointer, &self.value)?;

        let output = JsonWriteOutput {
            kind: "json.write",
            ok: true,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, "Updated JSON file.")),
        }
    }
}

// ============================================================================
// json schema
// ============================================================================

/// Generate JSON schema for the exo tool surface.
#[derive(Debug, Clone)]
pub struct JsonSchema {
    pub style: String,
}

impl JsonSchema {
    pub fn new(style: impl Into<String>) -> Self {
        Self {
            style: style.into(),
        }
    }

    pub fn full() -> Self {
        Self::new("full")
    }

    pub fn lm_tool() -> Self {
        Self::new("lm-tool")
    }
}

impl Command for JsonSchema {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "schema"
    }

    fn description(&self) -> &'static str {
        "Generate JSON schema for exo commands"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let spec = CommandSpec::from_registry(&default_registry());
        let schema = if self.style == "full" {
            json_schema::generate_schema(&spec)
        } else {
            json_schema::generate_lm_tool_schema(&spec)
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(schema)),
            OutputFormat::Human => {
                let formatted = serde_json::to_string_pretty(&schema)?;
                Ok(CommandOutput::message(formatted))
            }
        }
    }
}

// ============================================================================
// json spec
// ============================================================================

/// Generate `CommandSpec` from the registry.
///
/// This command outputs the complete `CommandSpec` derived from the
/// `CommandRegistry`, providing introspection into all available commands,
/// their effects, and metadata.
///
/// # Output
///
/// JSON output includes:
/// - `version`: Spec format version
/// - `namespaces`: Map of namespace → operations
/// - `operation_count`: Total number of operations
///
/// Human output provides a summary with namespace/operation counts.
#[derive(Debug, Clone, Copy)]
pub struct JsonSpec;

impl Command for JsonSpec {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "spec"
    }

    fn description(&self) -> &'static str {
        "Generate CommandSpec from registry (RFC 0132)"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        match ctx.format {
            OutputFormat::Json => {
                #[derive(Serialize)]
                struct SpecOutput {
                    kind: &'static str,
                    ok: bool,
                    #[serde(flatten)]
                    spec: CommandSpec,
                }

                Ok(CommandOutput::data(SpecOutput {
                    kind: "json.spec",
                    ok: true,
                    spec,
                }))
            }
            OutputFormat::Human => {
                let mut output = String::new();
                output.push_str("# CommandSpec (RFC 0132)\n\n");
                output.push_str(&format!("**Version**: {}\n", spec.version));
                output.push_str(&format!("**Namespaces**: {}\n", spec.namespaces.len()));
                output.push_str(&format!(
                    "**Operations**: {}\n\n",
                    spec.operation_count.unwrap_or(0)
                ));

                output.push_str("## Namespaces\n\n");
                for (name, ns) in &spec.namespaces {
                    output.push_str(&format!(
                        "- **{}**: {} operations\n",
                        name,
                        ns.operations.len()
                    ));
                }

                Ok(CommandOutput::message(output))
            }
        }
    }
}

// ============================================================================
// json artifact
// ============================================================================

/// Generate `CommandSpec` artifact for VS Code extension.
///
/// This command writes the complete `CommandSpec` to a JSON file in the
/// packages/exosuit-vscode/src directory for consumption by the LM tool
/// infrastructure.
///
/// The artifact includes a `_generated` field marking it as generated code.
#[derive(Debug, Clone)]
pub struct JsonArtifact {
    pub output_path: Option<PathBuf>,
}

impl JsonArtifact {
    pub const fn new(output_path: Option<PathBuf>) -> Self {
        Self { output_path }
    }

    fn default_output_path(root: &std::path::Path) -> PathBuf {
        root.join("packages/exosuit-vscode/src/command-spec.json")
    }
}

impl Command for JsonArtifact {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "artifact"
    }

    fn description(&self) -> &'static str {
        "Generate CommandSpec artifact for VS Code extension"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("JsonArtifact should be dispatched via execute_mut")
    }
}

impl MutableCommand for JsonArtifact {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        // Create output with generation marker
        #[derive(Serialize)]
        struct GeneratedSpec<'a> {
            _generated: &'static str,
            #[serde(flatten)]
            spec: &'a CommandSpec,
        }

        let output = GeneratedSpec {
            _generated: "This file is auto-generated by `exo json artifact`. DO NOT EDIT.",
            spec: &spec,
        };

        let output_path = self
            .output_path
            .clone()
            .unwrap_or_else(|| Self::default_output_path(ctx.root));

        let full_path = if output_path.is_absolute() {
            output_path
        } else {
            ctx.root.join(&output_path)
        };

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write pretty-printed JSON
        let json_str = serde_json::to_string_pretty(&output)?;
        std::fs::write(&full_path, json_str)?;

        let display_path = full_path
            .strip_prefix(ctx.root)
            .unwrap_or(&full_path)
            .display();

        match ctx.format {
            OutputFormat::Json => {
                #[derive(Serialize)]
                struct ArtifactOutput {
                    kind: &'static str,
                    ok: bool,
                    path: String,
                    namespace_count: usize,
                    operation_count: usize,
                }

                Ok(CommandOutput::data(ArtifactOutput {
                    kind: "json.artifact",
                    ok: true,
                    path: display_path.to_string(),
                    namespace_count: spec.namespaces.len(),
                    operation_count: spec.operation_count.unwrap_or(0),
                }))
            }
            OutputFormat::Human => {
                let message = format!(
                    "Generated command spec artifact:\n  Path: {}\n  Namespaces: {}\n  Operations: {}",
                    display_path,
                    spec.namespaces.len(),
                    spec.operation_count.unwrap_or(0)
                );
                Ok(CommandOutput::message(message))
            }
        }
    }
}

// ============================================================================
// json lm-tools
// ============================================================================

/// Generate VS Code `languageModelTools` contribution format.
///
/// This command outputs the complete array of LM tool contributions suitable
/// for insertion into package.json's `contributes.languageModelTools` array.
///
/// The output includes all fields required by VS Code:
/// - `name`: Tool identifier (e.g., "exo-status")
/// - `displayName`: Human-readable name
/// - `toolReferenceName`: Short reference name for prompts
/// - `canBeReferencedInPrompt`: Always true for exo tools
/// - `icon`: VS Code icon (default: $(hubot))
/// - `tags`: ["exosuit", namespace, ...]
/// - `userDescription`: Short description for users
/// - `modelDescription`: Full description with usage guidance for AI
/// - `inputSchema`: JSON Schema for tool input
#[derive(Debug, Clone)]
pub struct JsonLmTools {
    /// Filter to a specific namespace (e.g., "phase", "epoch")
    pub namespace: Option<String>,
}

impl JsonLmTools {
    pub const fn new(namespace: Option<String>) -> Self {
        Self { namespace }
    }
}

/// Represents a single VS Code languageModelTools contribution entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LmToolContribution {
    name: String,
    display_name: String,
    tool_reference_name: String,
    can_be_referenced_in_prompt: bool,
    icon: String,
    tags: Vec<String>,
    user_description: String,
    model_description: String,
    input_schema: serde_json::Value,
}

/// Represents a package.json languageModelTools contribution entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageToolContribution {
    name: String,
    display_name: String,
    tool_reference_name: String,
    can_be_referenced_in_prompt: bool,
    icon: String,
    tags: Vec<String>,
    user_description: String,
    model_description: String,
    #[serde(rename = "when", skip_serializing_if = "Option::is_none")]
    when_clause: Option<String>,
    input_schema: serde_json::Value,
}

/// Represents a package.json languageModelToolSets contribution entry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageToolSetContribution {
    name: String,
    display_name: String,
    description: String,
    tools: Vec<String>,
}

/// Combined output for package.json tool contributions.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageToolsOutput {
    tools: Vec<PackageToolContribution>,
    tool_sets: Vec<PackageToolSetContribution>,
}

impl Command for JsonLmTools {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "lm-tools"
    }

    fn description(&self) -> &'static str {
        "Generate VS Code languageModelTools contribution format"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let contributions = generate_lm_tool_contributions(&spec, self.namespace.as_deref());

        match ctx.format {
            OutputFormat::Json => {
                #[derive(Serialize)]
                struct LmToolsOutput {
                    kind: &'static str,
                    ok: bool,
                    count: usize,
                    tools: Vec<LmToolContribution>,
                }

                Ok(CommandOutput::data(LmToolsOutput {
                    kind: "json.lm-tools",
                    ok: true,
                    count: contributions.len(),
                    tools: contributions,
                }))
            }
            OutputFormat::Human => {
                // Human output shows the raw JSON array for easy copy-paste
                let json_array: Vec<serde_json::Value> = contributions
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?;
                let formatted = serde_json::to_string_pretty(&json_array)?;
                Ok(CommandOutput::message(formatted))
            }
        }
    }
}

// ============================================================================
// json package-tools
// ============================================================================

/// Generate package.json `languageModelTools` contribution format.
#[derive(Debug, Clone, Copy)]
pub struct JsonPackageTools;

impl Command for JsonPackageTools {
    fn namespace(&self) -> &'static str {
        "json"
    }

    fn operation(&self) -> &'static str {
        "package-tools"
    }

    fn description(&self) -> &'static str {
        "Generate package.json languageModelTools contribution format"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let registry = default_registry();
        let spec = CommandSpec::from_registry(&registry);

        let contributions = generate_package_tool_contributions(&spec);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(contributions)),
            OutputFormat::Human => {
                let formatted = serde_json::to_string_pretty(&contributions)?;
                Ok(CommandOutput::message(formatted))
            }
        }
    }
}

/// Generate LM tool contributions from `CommandSpec`.
///
/// This function creates VS Code `languageModelTools` entries from the
/// `CommandSpec`, following the projection strategy from RFC 0132:
///
/// 1. Zero-arg operations get dedicated tools for easy discovery
/// 2. Namespace operations get method-dispatch tools
fn generate_lm_tool_contributions(
    spec: &CommandSpec,
    namespace_filter: Option<&str>,
) -> Vec<LmToolContribution> {
    let mut contributions = Vec::new();

    // 1. Generate zero-arg orientation tools (pure operations with no required args)
    for (ns_name, op) in spec.zero_arg_operations() {
        if let Some(filter) = namespace_filter
            && ns_name != filter
        {
            continue;
        }

        let tool_name = format!("exo-{}", op.name);

        contributions.push(LmToolContribution {
            name: tool_name.clone(),
            display_name: humanize_name(&op.name),
            tool_reference_name: op.name.clone(),
            can_be_referenced_in_prompt: true,
            icon: select_icon(ns_name, &op.name),
            tags: build_tags(ns_name, &op.name),
            user_description: op.description.lines().next().unwrap_or("").to_string(),
            model_description: op.description.clone(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        });
    }

    // 2. Generate method-dispatch tools for namespaces with multiple operations
    for ns in spec.method_dispatch_namespaces() {
        if let Some(filter) = namespace_filter
            && ns.name != filter
        {
            continue;
        }

        let tool_name = format!("exo-{}", ns.name);

        // Build method enum and collect all arguments
        let methods: Vec<String> = ns.operations.keys().cloned().collect();
        let mut all_properties: BTreeMap<String, serde_json::Value> = BTreeMap::new();
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
            all_descriptions.push(format!(
                "- {}: {}",
                op.name,
                op.description.lines().next().unwrap_or("")
            ));
        }

        // Build description with all operations listed
        let full_description = format!(
            "{}\n\nOperations:\n{}",
            ns.description,
            all_descriptions.join("\n")
        );

        contributions.push(LmToolContribution {
            name: tool_name.clone(),
            display_name: humanize_name(&ns.name),
            tool_reference_name: ns.name.clone(),
            can_be_referenced_in_prompt: true,
            icon: select_icon(&ns.name, ""),
            tags: build_tags(&ns.name, ""),
            user_description: ns.description.lines().next().unwrap_or("").to_string(),
            model_description: full_description,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": all_properties,
                "required": ["method"]
            }),
        });
    }

    // 3. Generate root operation tools (status, map, etc.)
    for (ns_name, op_name, op) in spec.iter_all_operations() {
        if !ns_name.is_empty() {
            continue; // Skip namespace operations (handled above)
        }

        let tool_name = format!("exo-{op_name}");

        let mut properties: BTreeMap<String, serde_json::Value> = BTreeMap::new();
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

        contributions.push(LmToolContribution {
            name: tool_name.clone(),
            display_name: humanize_name(op_name),
            tool_reference_name: op_name.to_string(),
            can_be_referenced_in_prompt: true,
            icon: select_icon("", op_name),
            tags: build_tags("", op_name),
            user_description: op.description.lines().next().unwrap_or("").to_string(),
            model_description: op.description.clone(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required
            }),
        });
    }

    // Sort by name for stable output
    contributions
}

fn generate_package_tool_contributions(spec: &CommandSpec) -> PackageToolsOutput {
    let mut contributions = Vec::new();
    let mut tool_sets = build_tool_set_map();

    for (ns_name, op_name, op) in spec.iter_all_operations() {
        let Some(meta) = &op.lm_tool else {
            continue;
        };

        let Some(tool_reference_name) = meta.tool_reference_name.as_ref() else {
            continue;
        };

        let name = format!("exo-{tool_reference_name}");
        let display_name = meta
            .display_name
            .clone()
            .unwrap_or_else(|| humanize_name(tool_reference_name));
        let user_description = meta
            .user_description
            .clone()
            .unwrap_or_else(|| op.description.lines().next().unwrap_or("").to_string());
        let model_description = meta
            .model_description
            .clone()
            .unwrap_or_else(|| op.description.clone());
        let icon = meta
            .icon
            .clone()
            .unwrap_or_else(|| select_icon(ns_name, op_name));
        let tags = meta
            .tags
            .clone()
            .unwrap_or_else(|| build_tags(ns_name, op_name));
        let can_be_referenced_in_prompt = meta.can_be_referenced_in_prompt.unwrap_or(true);

        contributions.push(PackageToolContribution {
            name: name.clone(),
            display_name,
            tool_reference_name: tool_reference_name.clone(),
            can_be_referenced_in_prompt,
            icon,
            tags,
            user_description,
            model_description,
            when_clause: meta.when_clause.clone(),
            input_schema: build_operation_input_schema(op),
        });

        if let Some(tool_set_names) = meta.tool_sets.as_ref() {
            add_tool_to_sets(&mut tool_sets, &name, tool_set_names);
        }
    }

    for extra in lm_tool_metadata::extra_tools() {
        let name = extra.name.clone();
        contributions.push(PackageToolContribution {
            name: extra.name,
            display_name: extra.display_name,
            tool_reference_name: extra.tool_reference_name,
            can_be_referenced_in_prompt: extra.can_be_referenced_in_prompt,
            icon: extra.icon,
            tags: extra.tags,
            user_description: extra.user_description,
            model_description: extra.model_description,
            when_clause: None,
            input_schema: extra.input_schema,
        });

        add_tool_to_sets(&mut tool_sets, &name, &extra.tool_sets);
    }

    contributions.sort_by(|a, b| a.name.cmp(&b.name));

    let mut tool_sets: Vec<PackageToolSetContribution> = tool_sets
        .into_values()
        .filter(|set| !set.tools.is_empty())
        .collect();

    for set in &mut tool_sets {
        set.tools.sort();
    }

    tool_sets.sort_by(|a, b| a.name.cmp(&b.name));

    PackageToolsOutput {
        tools: contributions,
        tool_sets,
    }
}

fn build_tool_set_map() -> BTreeMap<String, PackageToolSetContribution> {
    let mut map = BTreeMap::new();

    for def in lm_tool_metadata::tool_set_definitions() {
        map.insert(
            def.name.to_string(),
            PackageToolSetContribution {
                name: def.name.to_string(),
                display_name: def.display_name.to_string(),
                description: def.description.to_string(),
                tools: Vec::new(),
            },
        );
    }

    map
}

fn add_tool_to_sets(
    tool_sets: &mut BTreeMap<String, PackageToolSetContribution>,
    tool_name: &str,
    set_names: &[String],
) {
    for set_name in set_names {
        if let Some(entry) = tool_sets.get_mut(set_name) {
            entry.tools.push(tool_name.to_string());
        }
    }
}

fn build_operation_input_schema(
    op: &crate::command::command_spec::OperationSpec,
) -> serde_json::Value {
    let mut properties: BTreeMap<String, serde_json::Value> = BTreeMap::new();
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

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

/// Convert `snake_case` or kebab-case to Title Case.
fn humanize_name(name: &str) -> String {
    name.split(['_', '-'])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Select an appropriate icon based on namespace/operation.
fn select_icon(namespace: &str, operation: &str) -> String {
    match (namespace, operation) {
        ("", "status") => "$(info)".to_string(),
        ("", "map") => "$(list-tree)".to_string(),
        ("phase", _) => "$(milestone)".to_string(),
        ("epoch", _) => "$(calendar)".to_string(),
        ("task", _) => "$(tasklist)".to_string(),
        ("idea", _) => "$(lightbulb)".to_string(),
        ("rfc", _) => "$(file-text)".to_string(),
        ("tdd", _) => "$(beaker)".to_string(),
        ("impl", _) => "$(tools)".to_string(),
        ("ai", _) => "$(robot)".to_string(),
        ("axiom", _) => "$(law)".to_string(),
        ("inbox", _) => "$(inbox)".to_string(),
        _ => "$(hubot)".to_string(),
    }
}

/// Build tags for a tool.
fn build_tags(namespace: &str, operation: &str) -> Vec<String> {
    let mut tags = vec!["exosuit".to_string()];
    if !namespace.is_empty() {
        tags.push(namespace.to_string());
    }
    if !operation.is_empty() && operation != namespace {
        tags.push(operation.to_string());
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_read_metadata() {
        let cmd = JsonRead::new("test.json", None);
        assert_eq!(cmd.namespace(), "json");
        assert_eq!(cmd.operation(), "read");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_json_write_metadata() {
        let cmd = JsonWrite::new("test.json", "/key", "value");
        assert_eq!(cmd.namespace(), "json");
        assert_eq!(cmd.operation(), "write");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_json_schema_metadata() {
        let cmd = JsonSchema::full();
        assert_eq!(cmd.namespace(), "json");
        assert_eq!(cmd.operation(), "schema");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_json_artifact_metadata() {
        let cmd = JsonArtifact::new(None);
        assert_eq!(cmd.namespace(), "json");
        assert_eq!(cmd.operation(), "artifact");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_json_lm_tools_metadata() {
        let cmd = JsonLmTools::new(None);
        assert_eq!(cmd.namespace(), "json");
        assert_eq!(cmd.operation(), "lm-tools");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_humanize_name() {
        assert_eq!(humanize_name("status"), "Status");
        assert_eq!(humanize_name("add-task"), "Add Task");
        assert_eq!(humanize_name("lm_tools"), "Lm Tools");
    }
}
