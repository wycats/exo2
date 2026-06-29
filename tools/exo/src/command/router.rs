//! Spec-driven command router (RFC 0132).
//!
//! This module provides a routing layer that resolves tokens to commands
//! using the `CommandSpec`, producing an Invocation AST with structured
//! diagnostics.
//!
//! # Design Principles
//!
//! - **Spec is Law**: Routing uses `CommandSpec` as the authoritative definition.
//! - **Deterministic**: Ambiguity is an error, not silent resolution.
//! - **Agent-First Diagnostics**: Errors include structured suggestions.
//!
//! # Architecture
//!
//! ```text
//! Tokens → Router → Invocation AST + Diagnostics
//!                         ↓
//!              Dispatcher → CommandBox → Output
//! ```
//!
//! The router is frontend-agnostic: it works with token arrays from:
//! - CLI argv
//! - Tool JSON (presence-based CLI AST)
//! - DSL strings (after lexing)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// ============================================================================
// Invocation AST
// ============================================================================

/// The parsed representation of a command invocation.
///
/// An Invocation captures:
/// - The resolved command path (namespace + operation)
/// - Typed argument values
/// - Metadata for diagnostics
///
/// This is the output of the routing phase, before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invocation {
    /// The command path (e.g., `["phase", "start"]`).
    pub path: CommandPath,
    /// Resolved argument values.
    pub args: BTreeMap<String, TypedValue>,
    /// Occurrence counts for provided arguments.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub occurrences: BTreeMap<String, u32>,
    /// Source metadata for diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<InvocationSource>,
}

impl Invocation {
    /// Create a new invocation for a namespace operation.
    pub fn new(namespace: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            path: CommandPath {
                namespace: namespace.into(),
                operation: operation.into(),
            },
            args: BTreeMap::new(),
            occurrences: BTreeMap::new(),
            source: None,
        }
    }

    /// Add an argument value.
    pub fn with_arg(mut self, name: impl Into<String>, value: TypedValue) -> Self {
        let name = name.into();
        self.args.insert(name.clone(), value);
        *self.occurrences.entry(name).or_insert(0) += 1;
        self
    }

    /// Add source metadata.
    pub fn with_source(mut self, source: InvocationSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Get the namespace name.
    pub fn namespace(&self) -> &str {
        &self.path.namespace
    }

    /// Get the operation name.
    pub fn operation(&self) -> &str {
        &self.path.operation
    }

    /// Get an argument value by name.
    pub fn get_arg(&self, name: &str) -> Option<&TypedValue> {
        self.args
            .get(name)
            .or_else(|| self.args.get(&name.replace('_', "-")))
    }

    /// Get a string argument value.
    pub fn get_string(&self, name: &str) -> Option<&str> {
        self.get_arg(name).and_then(|v| v.as_str())
    }

    /// Get a boolean argument value.
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_arg(name).and_then(TypedValue::as_bool)
    }

    /// Get an integer argument value.
    pub fn get_int(&self, name: &str) -> Option<i64> {
        self.get_arg(name).and_then(TypedValue::as_int)
    }

    /// Get a JSON argument value (stored as string).
    pub fn get_json(&self, name: &str) -> Option<&str> {
        self.get_arg(name).and_then(TypedValue::as_json)
    }

    /// Convert invocation arguments to JSON input payload.
    pub fn to_json_input(&self) -> Value {
        let mut object = serde_json::Map::new();
        for (name, value) in &self.args {
            object.insert(name.clone(), value.to_json_value());
        }
        Value::Object(object)
    }

    /// Create a new invocation from JSON arguments and a command spec.
    pub fn from_json(
        input: &Value,
        namespace: &str,
        operation: &str,
        spec: &CommandSpec,
    ) -> Result<Self, RoutingDiagnostic> {
        let (operation_spec, path_context) = if namespace.is_empty() {
            let operation_spec = spec.root_operations.get(operation).ok_or_else(|| {
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownOperation,
                    format!("Unknown root operation '{operation}'"),
                )
                .with_context(DiagnosticContext {
                    path: None,
                    available: spec.root_operations.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: Some(operation.to_string()),
                })
            })?;

            (operation_spec, vec![operation.to_string()])
        } else {
            let namespace_spec = spec.namespaces.get(namespace).ok_or_else(|| {
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownNamespace,
                    format!("Unknown namespace: '{namespace}'"),
                )
                .with_context(DiagnosticContext {
                    path: None,
                    available: spec.namespaces.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: Some(namespace.to_string()),
                })
            })?;

            let operation_spec = namespace_spec.operations.get(operation).ok_or_else(|| {
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownOperation,
                    format!("Unknown operation '{operation}' in namespace '{namespace}'"),
                )
                .with_context(DiagnosticContext {
                    path: Some(vec![namespace.to_string()]),
                    available: namespace_spec.operations.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: Some(operation.to_string()),
                })
            })?;

            (
                operation_spec,
                vec![namespace.to_string(), operation.to_string()],
            )
        };

        let object = input.as_object().ok_or_else(|| {
            RoutingDiagnostic::new(
                DiagnosticCode::InvalidArgType,
                "Expected JSON object for invocation arguments",
            )
            .with_context(DiagnosticContext {
                path: Some(path_context.clone()),
                available: Vec::new(),
                expected_type: Some("object".to_string()),
                actual_value: Some(input.to_string()),
            })
        })?;

        let mut args = BTreeMap::new();
        let mut occurrences = BTreeMap::new();

        for (key, value) in object {
            // Look up by id first (for CLI path), then by name (for external JSON input).
            // Also try underscore↔hyphen normalization for backward compatibility
            // (ExoSpec uses hyphens, but callers may send underscores).
            // Finally, check short flag aliases (single-char keys like "n" for "-n").
            let normalized_key = key.replace('_', "-");
            let short_char = if key.len() == 1 {
                key.chars().next()
            } else {
                None
            };
            let arg_spec = operation_spec.args.iter().find(|arg| {
                arg.id == *key
                    || arg.name == *key
                    || arg.id == normalized_key
                    || arg.name == normalized_key
                    || (short_char.is_some() && arg.short == short_char)
            });

            let Some(arg_spec) = arg_spec else {
                return Err(RoutingDiagnostic::new(
                    DiagnosticCode::UnknownFlag,
                    format!("Unknown argument: '{key}'"),
                )
                .with_context(DiagnosticContext {
                    path: Some(path_context.clone()),
                    available: operation_spec
                        .args
                        .iter()
                        .map(|arg| arg.id.clone())
                        .collect(),
                    expected_type: None,
                    actual_value: Some(key.clone()),
                }));
            };

            let typed = TypedValue::from_json(value, arg_spec)?;
            // Always store by id for consistent lookup
            args.insert(arg_spec.id.clone(), typed);
            occurrences.insert(arg_spec.id.clone(), 1);
        }

        for arg_spec in &operation_spec.args {
            if args.contains_key(&arg_spec.id) {
                continue;
            }

            if arg_spec.default.is_some() {
                let typed = typed_value_from_default(arg_spec)?;
                args.insert(arg_spec.id.clone(), typed);
                continue;
            }

            if arg_spec.optional {
                continue;
            }

            return Err(RoutingDiagnostic::new(
                DiagnosticCode::MissingRequiredArg,
                format!("Missing required argument: '{}'", arg_spec.name),
            )
            .with_context(DiagnosticContext {
                path: Some(path_context),
                available: Vec::new(),
                expected_type: Some(format!("{:?}", arg_spec.value_type)),
                actual_value: None,
            }));
        }

        Ok(Self {
            path: CommandPath::new(namespace, operation),
            args,
            occurrences,
            source: Some(InvocationSource {
                frontend: Frontend::ToolJson,
                tokens: None,
                input: Some(input.to_string()),
            }),
        })
    }
}

/// The resolved command path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandPath {
    /// The namespace (e.g., "phase", "task").
    pub namespace: String,
    /// The operation (e.g., "start", "complete").
    pub operation: String,
}

/// Backward-compatible alias for command addressing.
pub type AddressPath = CommandPath;

impl CommandPath {
    /// Create a new command path.
    pub fn new(namespace: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            operation: operation.into(),
        }
    }

    /// Format as a dot-separated string.
    pub fn to_dotted(&self) -> String {
        format!("{}.{}", self.namespace, self.operation)
    }

    /// Format as CLI-style arguments.
    pub fn to_argv(&self) -> Vec<String> {
        vec![self.namespace.clone(), self.operation.clone()]
    }

    /// Format as a legacy path vector.
    pub fn to_vec(&self) -> Vec<String> {
        vec![self.namespace.clone(), self.operation.clone()]
    }
}

impl std::fmt::Display for CommandPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.namespace, self.operation)
    }
}

// ============================================================================
// Typed Values
// ============================================================================

/// A typed value from command arguments.
///
/// Values are typed at parse time (RFC 0132), keeping errors local.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum TypedValue {
    /// Boolean flag value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Float value.
    Float(f64),
    /// String value.
    String(String),
    /// File path value.
    Path(String),
    /// JSON value (stored as string for now).
    Json(String),
    /// Enum variant.
    Enum(String),
    /// Array of values.
    Array(Vec<Self>),
}

impl TypedValue {
    /// Get as string if this is a String or Path value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Path(s) => Some(s),
            _ => None,
        }
    }

    /// Get as bool if this is a Bool value.
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as int if this is an Int value.
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Get as float if this is a Float value.
    pub const fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Get as JSON string if this is a Json value.
    pub fn as_json(&self) -> Option<&str> {
        match self {
            Self::Json(s) => Some(s),
            _ => None,
        }
    }

    /// Parse a JSON value into a typed value based on the argument spec.
    ///
    /// Applies lenient coercion for machine-channel inputs:
    /// - String "true"/"false" → Bool
    /// - String of digits → Int
    /// - String of digits with decimal → Float
    /// - JSON array → comma-separated String (for CSV-list args)
    pub fn from_json(value: &Value, arg_spec: &ArgSpec) -> Result<Self, RoutingDiagnostic> {
        match &arg_spec.value_type {
            ValueType::Bool => value
                .as_bool()
                .or_else(|| match value.as_str() {
                    Some("true" | "1" | "yes") => Some(true),
                    Some("false" | "0" | "no") => Some(false),
                    _ => None,
                })
                .map(Self::Bool)
                .ok_or_else(|| invalid_arg_type(arg_spec, "boolean", value)),
            ValueType::Int => value
                .as_i64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
                .map(Self::Int)
                .ok_or_else(|| invalid_arg_type(arg_spec, "integer", value)),
            ValueType::Float => value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
                .map(Self::Float)
                .ok_or_else(|| invalid_arg_type(arg_spec, "number", value)),
            ValueType::String => {
                // Accept strings directly, or coerce arrays to comma-separated strings.
                // All array elements are stringified (non-string JSON values use their
                // display representation). Empty arrays are rejected.
                if let Some(s) = value.as_str() {
                    Ok(Self::String(s.to_string()))
                } else if let Some(arr) = value.as_array() {
                    if arr.is_empty() {
                        return Err(invalid_arg_type(
                            arg_spec,
                            "non-empty string or array",
                            value,
                        ));
                    }
                    let items: Vec<String> = arr
                        .iter()
                        .map(|v| v.as_str().map_or_else(|| v.to_string(), String::from))
                        .collect();
                    Ok(Self::String(items.join(",")))
                } else {
                    Err(invalid_arg_type(arg_spec, "string", value))
                }
            }
            ValueType::Path => value
                .as_str()
                .map(|s| Self::Path(s.to_string()))
                .ok_or_else(|| invalid_arg_type(arg_spec, "string", value)),
            ValueType::Json => Ok(Self::Json(value.to_string())),
            ValueType::Enum(allowed) => {
                let Some(variant) = value.as_str() else {
                    return Err(invalid_arg_type(arg_spec, "string", value));
                };

                if allowed.iter().any(|v| v == variant) {
                    Ok(Self::Enum(variant.to_string()))
                } else {
                    Err(RoutingDiagnostic::new(
                        DiagnosticCode::InvalidArgType,
                        format!(
                            "Invalid enum value '{variant}' for argument '{}'",
                            arg_spec.name
                        ),
                    )
                    .with_context(DiagnosticContext {
                        path: None,
                        available: allowed.clone(),
                        expected_type: Some(format!("enum({})", allowed.join(", "))),
                        actual_value: Some(variant.to_string()),
                    }))
                }
            }
        }
    }

    pub fn to_json_value(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::Number(serde_json::Number::from(*i)),
            Self::Float(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
            Self::String(s) | Self::Path(s) | Self::Enum(s) => Value::String(s.clone()),
            Self::Json(raw) => {
                serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
            }
            Self::Array(values) => Value::Array(values.iter().map(Self::to_json_value).collect()),
        }
    }
}

fn invalid_arg_type(arg_spec: &ArgSpec, expected: &str, actual: &Value) -> RoutingDiagnostic {
    RoutingDiagnostic::new(
        DiagnosticCode::InvalidArgType,
        format!("Expected {expected} for argument '{}'", arg_spec.name),
    )
    .with_context(DiagnosticContext {
        path: None,
        available: Vec::new(),
        expected_type: Some(expected.to_string()),
        actual_value: Some(actual.to_string()),
    })
}

fn typed_value_from_default(arg_spec: &ArgSpec) -> Result<TypedValue, RoutingDiagnostic> {
    let default = arg_spec.default.as_ref().ok_or_else(|| {
        RoutingDiagnostic::new(
            DiagnosticCode::InvalidArgType,
            format!("Missing default value for argument '{}'", arg_spec.name),
        )
    })?;

    match &arg_spec.value_type {
        ValueType::Bool => default
            .parse::<bool>()
            .map(TypedValue::Bool)
            .map_err(|_| invalid_default_value(arg_spec, "boolean", default)),
        ValueType::Int => default
            .parse::<i64>()
            .map(TypedValue::Int)
            .map_err(|_| invalid_default_value(arg_spec, "integer", default)),
        ValueType::Float => default
            .parse::<f64>()
            .map(TypedValue::Float)
            .map_err(|_| invalid_default_value(arg_spec, "number", default)),
        ValueType::String => Ok(TypedValue::String(default.clone())),
        ValueType::Path => Ok(TypedValue::Path(default.clone())),
        ValueType::Json => Ok(TypedValue::Json(default.clone())),
        ValueType::Enum(allowed) => {
            if allowed.iter().any(|v| v == default) {
                Ok(TypedValue::Enum(default.clone()))
            } else {
                Err(RoutingDiagnostic::new(
                    DiagnosticCode::InvalidArgType,
                    format!(
                        "Invalid default enum value '{default}' for argument '{}'",
                        arg_spec.name
                    ),
                )
                .with_context(DiagnosticContext {
                    path: None,
                    available: allowed.clone(),
                    expected_type: Some(format!("enum({})", allowed.join(", "))),
                    actual_value: Some(default.clone()),
                }))
            }
        }
    }
}

fn invalid_default_value(arg_spec: &ArgSpec, expected: &str, actual: &str) -> RoutingDiagnostic {
    RoutingDiagnostic::new(
        DiagnosticCode::InvalidArgType,
        format!("Invalid default value for argument '{}'", arg_spec.name),
    )
    .with_context(DiagnosticContext {
        path: None,
        available: Vec::new(),
        expected_type: Some(expected.to_string()),
        actual_value: Some(actual.to_string()),
    })
}

impl From<bool> for TypedValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for TypedValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<String> for TypedValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for TypedValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

// ============================================================================
// Source Metadata
// ============================================================================

/// Source information for diagnostics.
///
/// Preserves enough context to generate rich error messages
/// pointing to the original input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvocationSource {
    /// The frontend that produced this invocation.
    pub frontend: Frontend,
    /// Original tokens (for token-based frontends).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Vec<String>>,
    /// Original input string (for string-based frontends).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

/// The frontend that produced the invocation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Frontend {
    /// Token array (argv).
    Argv,
    /// Tool JSON (presence-based CLI AST).
    ToolJson,
    /// DSL string.
    Dsl,
    /// Clap bridge (compatibility).
    ClapBridge,
}

// ============================================================================
// Routing Diagnostics
// ============================================================================

/// A routing diagnostic with suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDiagnostic {
    /// Stable diagnostic code.
    pub code: DiagnosticCode,
    /// Human/agent-facing message.
    pub message: String,
    /// Where the error occurred (token index or span).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
    /// Structured context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<DiagnosticContext>,
    /// Suggested fixes.
    pub suggestions: Vec<Suggestion>,
}

impl RoutingDiagnostic {
    /// Create a new diagnostic.
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            location: None,
            context: None,
            suggestions: Vec::new(),
        }
    }

    /// Add a location.
    pub const fn with_location(mut self, location: DiagnosticLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// Add context.
    pub fn with_context(mut self, context: DiagnosticContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Add a suggestion.
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }
}

/// Stable diagnostic codes for routing errors.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCode {
    /// Unknown namespace.
    UnknownNamespace,
    /// Unknown operation within a namespace.
    UnknownOperation,
    /// Unknown flag.
    UnknownFlag,
    /// Missing required argument.
    MissingRequiredArg,
    /// Invalid argument value type.
    InvalidArgType,
    /// Ambiguous command.
    AmbiguousCommand,
    /// Unsupported shell feature.
    UnsupportedShellFeature,
    /// Too many positional arguments.
    TooManyPositionals,
}

/// Location of an error in the input.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DiagnosticLocation {
    /// Token index (for argv/token frontends).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_index: Option<usize>,
    /// Byte span in the original string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<(usize, usize)>,
}

/// Structured context for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticContext {
    /// The command path so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// Available options.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub available: Vec<String>,
    /// Expected type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_type: Option<String>,
    /// Actual value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<String>,
}

/// A suggested fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// What the suggestion fixes.
    pub label: String,
    /// The suggested command or fix.
    pub replacement: String,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
}

impl Suggestion {
    /// Create a new suggestion.
    pub fn new(label: impl Into<String>, replacement: impl Into<String>, confidence: f32) -> Self {
        Self {
            label: label.into(),
            replacement: replacement.into(),
            confidence,
        }
    }
}

// ============================================================================
// Routing Result
// ============================================================================

/// The result of routing a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingResult {
    /// The parsed invocation (if successful).
    pub invocation: Option<Invocation>,
    /// Diagnostics (errors and warnings).
    pub diagnostics: Vec<RoutingDiagnostic>,
    /// Whether routing succeeded.
    pub ok: bool,
}

impl RoutingResult {
    /// Create a successful result.
    pub const fn success(invocation: Invocation) -> Self {
        Self {
            invocation: Some(invocation),
            diagnostics: Vec::new(),
            ok: true,
        }
    }

    /// Create a failed result with a diagnostic.
    pub fn failure(diagnostic: RoutingDiagnostic) -> Self {
        Self {
            invocation: None,
            diagnostics: vec![diagnostic],
            ok: false,
        }
    }

    /// Add a warning to a successful result.
    pub fn with_warning(mut self, warning: RoutingDiagnostic) -> Self {
        self.diagnostics.push(warning);
        self
    }
}

// ============================================================================
// Router Trait
// ============================================================================

use super::command_spec::{ArgSpec, CommandSpec, ValueType};

/// A router that resolves tokens to invocations using a `CommandSpec`.
#[derive(Debug)]
pub struct Router<'a> {
    spec: &'a CommandSpec,
}

impl<'a> Router<'a> {
    /// Create a new router with the given spec.
    pub const fn new(spec: &'a CommandSpec) -> Self {
        Self { spec }
    }

    /// Route a token array to an invocation.
    ///
    /// Tokens should be the command-specific portion of argv,
    /// excluding the program name and any global flags.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tokens = vec!["phase", "start", "map-phase-7"];
    /// let result = router.route(&tokens);
    /// ```
    pub fn route(&self, tokens: &[&str]) -> RoutingResult {
        if tokens.is_empty() {
            return RoutingResult::failure(
                RoutingDiagnostic::new(DiagnosticCode::UnknownNamespace, "No command specified")
                    .with_context(DiagnosticContext {
                        path: None,
                        available: self.spec.namespaces.keys().cloned().collect(),
                        expected_type: None,
                        actual_value: None,
                    }),
            );
        }

        let namespace_name = tokens[0];

        // Find the namespace
        let namespace = if let Some(ns) = self.spec.namespaces.get(namespace_name) {
            ns
        } else {
            // Try to find similar namespaces for suggestions
            let suggestions = self.suggest_namespaces(namespace_name);
            return RoutingResult::failure(
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownNamespace,
                    format!("Unknown namespace: '{namespace_name}'"),
                )
                .with_location(DiagnosticLocation {
                    token_index: Some(0),
                    span: None,
                })
                .with_context(DiagnosticContext {
                    path: None,
                    available: self.spec.namespaces.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: Some(namespace_name.to_string()),
                })
                .with_suggestion(suggestions.first().cloned().unwrap_or_else(|| {
                    Suggestion::new("List available commands", "exo --help", 0.5)
                })),
            );
        };

        // Need at least one more token for the operation
        if tokens.len() < 2 {
            return RoutingResult::failure(
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownOperation,
                    format!("No operation specified for '{namespace_name}'"),
                )
                .with_context(DiagnosticContext {
                    path: Some(vec![namespace_name.to_string()]),
                    available: namespace.operations.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: None,
                }),
            );
        }

        let operation_name = tokens[1];

        // Find the operation
        let operation = if let Some(op) = namespace.operations.get(operation_name) {
            op
        } else {
            let suggestions = self.suggest_operations(namespace, operation_name);
            return RoutingResult::failure(
                RoutingDiagnostic::new(
                    DiagnosticCode::UnknownOperation,
                    format!("Unknown operation '{operation_name}' in namespace '{namespace_name}'"),
                )
                .with_location(DiagnosticLocation {
                    token_index: Some(1),
                    span: None,
                })
                .with_context(DiagnosticContext {
                    path: Some(vec![namespace_name.to_string()]),
                    available: namespace.operations.keys().cloned().collect(),
                    expected_type: None,
                    actual_value: Some(operation_name.to_string()),
                })
                .with_suggestion(suggestions.first().cloned().unwrap_or_else(|| {
                    Suggestion::new(
                        "Show namespace help",
                        format!("exo {namespace_name} --help"),
                        0.5,
                    )
                })),
            );
        };

        // Create the invocation
        let mut invocation = Invocation::new(namespace_name, operation_name);

        // Parse remaining tokens as arguments
        // For now, we collect positionals - flag parsing will be added later
        let arg_tokens = &tokens[2..];
        let mut positional_index = 0;
        let mut i = 0;

        while i < arg_tokens.len() {
            let token = arg_tokens[i];

            if let Some(flag_name) = token.strip_prefix("--") {
                // Flag or option
                // Check if it's a known flag/option
                let arg_spec = operation.args.iter().find(|a| a.name == flag_name);

                if let Some(spec) = arg_spec {
                    use super::command_spec::ArgKind;
                    match spec.kind {
                        ArgKind::Flag => {
                            invocation
                                .args
                                .insert(flag_name.to_string(), TypedValue::Bool(true));
                            *invocation
                                .occurrences
                                .entry(flag_name.to_string())
                                .or_insert(0) += 1;
                        }
                        ArgKind::Option | ArgKind::Positional => {
                            // Need a value
                            if i + 1 >= arg_tokens.len() {
                                return RoutingResult::failure(
                                    RoutingDiagnostic::new(
                                        DiagnosticCode::MissingRequiredArg,
                                        format!("Option '--{flag_name}' requires a value"),
                                    )
                                    .with_location(
                                        DiagnosticLocation {
                                            token_index: Some(2 + i),
                                            span: None,
                                        },
                                    ),
                                );
                            }
                            i += 1;
                            let value = arg_tokens[i];
                            invocation.args.insert(
                                flag_name.to_string(),
                                TypedValue::String(value.to_string()),
                            );
                            *invocation
                                .occurrences
                                .entry(flag_name.to_string())
                                .or_insert(0) += 1;
                        }
                    }
                } else {
                    // Unknown flag - generate suggestion
                    let available_flags: Vec<String> = operation
                        .args
                        .iter()
                        .filter(|a| {
                            matches!(
                                a.kind,
                                super::command_spec::ArgKind::Flag
                                    | super::command_spec::ArgKind::Option
                            )
                        })
                        .map(|a| format!("--{}", a.name))
                        .collect();

                    return RoutingResult::failure(
                        RoutingDiagnostic::new(
                            DiagnosticCode::UnknownFlag,
                            format!("Unknown flag '--{flag_name}'"),
                        )
                        .with_location(DiagnosticLocation {
                            token_index: Some(2 + i),
                            span: None,
                        })
                        .with_context(DiagnosticContext {
                            path: Some(vec![
                                namespace_name.to_string(),
                                operation_name.to_string(),
                            ]),
                            available: available_flags,
                            expected_type: None,
                            actual_value: Some(token.to_string()),
                        }),
                    );
                }
            } else if token.starts_with('-') && token.len() == 2 {
                // Short flag
                let Some(short_char) = token.chars().nth(1) else {
                    return RoutingResult::failure(
                        RoutingDiagnostic::new(
                            DiagnosticCode::UnknownFlag,
                            format!("Unknown short flag '{token}'"),
                        )
                        .with_location(DiagnosticLocation {
                            token_index: Some(2 + i),
                            span: None,
                        }),
                    );
                };
                let arg_spec = operation.args.iter().find(|a| a.short == Some(short_char));

                match arg_spec {
                    Some(spec) => {
                        invocation
                            .args
                            .insert(spec.name.clone(), TypedValue::Bool(true));
                        *invocation.occurrences.entry(spec.name.clone()).or_insert(0) += 1;
                    }
                    None => {
                        return RoutingResult::failure(
                            RoutingDiagnostic::new(
                                DiagnosticCode::UnknownFlag,
                                format!("Unknown short flag '-{short_char}'"),
                            )
                            .with_location(DiagnosticLocation {
                                token_index: Some(2 + i),
                                span: None,
                            }),
                        );
                    }
                }
            } else {
                // Positional argument
                let positional_specs: Vec<_> = operation
                    .args
                    .iter()
                    .filter(|a| matches!(a.kind, super::command_spec::ArgKind::Positional))
                    .collect();

                if positional_index >= positional_specs.len() {
                    if positional_specs.is_empty() {
                        return RoutingResult::failure(
                            RoutingDiagnostic::new(
                                DiagnosticCode::TooManyPositionals,
                                format!(
                                    "Command '{namespace_name} {operation_name}' does not accept positional arguments"
                                ),
                            )
                            .with_location(DiagnosticLocation {
                                token_index: Some(2 + i),
                                span: None,
                            })
                            .with_context(DiagnosticContext {
                                path: Some(vec![
                                    namespace_name.to_string(),
                                    operation_name.to_string(),
                                ]),
                                available: vec![],
                                expected_type: None,
                                actual_value: Some(token.to_string()),
                            }),
                        );
                    }
                    return RoutingResult::failure(
                        RoutingDiagnostic::new(
                            DiagnosticCode::TooManyPositionals,
                            format!(
                                "Too many positional arguments (expected {}, got {}+)",
                                positional_specs.len(),
                                positional_index + 1
                            ),
                        )
                        .with_location(DiagnosticLocation {
                            token_index: Some(2 + i),
                            span: None,
                        }),
                    );
                }

                let spec = positional_specs[positional_index];
                invocation
                    .args
                    .insert(spec.name.clone(), TypedValue::String(token.to_string()));
                *invocation.occurrences.entry(spec.name.clone()).or_insert(0) += 1;
                positional_index += 1;
            }

            i += 1;
        }

        // Check for missing required arguments
        for arg in &operation.args {
            if !arg.optional && arg.default.is_none() && !invocation.args.contains_key(&arg.name) {
                return RoutingResult::failure(
                    RoutingDiagnostic::new(
                        DiagnosticCode::MissingRequiredArg,
                        format!("Missing required argument: '{}'", arg.name),
                    )
                    .with_context(DiagnosticContext {
                        path: Some(vec![namespace_name.to_string(), operation_name.to_string()]),
                        available: vec![],
                        expected_type: Some(format!("{:?}", arg.value_type)),
                        actual_value: None,
                    }),
                );
            }
        }

        // Add source metadata
        invocation.source = Some(InvocationSource {
            frontend: Frontend::Argv,
            tokens: Some(
                tokens
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
            ),
            input: None,
        });

        RoutingResult::success(invocation)
    }

    /// Suggest similar namespaces using edit distance.
    fn suggest_namespaces(&self, input: &str) -> Vec<Suggestion> {
        let mut suggestions: Vec<_> = self
            .spec
            .namespaces
            .keys()
            .filter_map(|ns| {
                let distance = levenshtein(input, ns);
                if distance <= 2 {
                    Some(Suggestion::new(
                        format!("Did you mean '{ns}'?"),
                        format!("exo {ns}"),
                        1.0 - (distance as f32 / 3.0),
                    ))
                } else {
                    None
                }
            })
            .collect();

        suggestions.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        suggestions.truncate(3);
        suggestions
    }

    /// Suggest similar operations using edit distance.
    fn suggest_operations(
        &self,
        namespace: &super::command_spec::NamespaceSpec,
        input: &str,
    ) -> Vec<Suggestion> {
        let mut suggestions: Vec<_> = namespace
            .operations
            .keys()
            .filter_map(|op| {
                let distance = levenshtein(input, op);
                if distance <= 2 {
                    Some(Suggestion::new(
                        format!("Did you mean '{op}'?"),
                        format!("exo {} {}", namespace.name, op),
                        1.0 - (distance as f32 / 3.0),
                    ))
                } else {
                    None
                }
            })
            .collect();

        suggestions.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        suggestions.truncate(3);
        suggestions
    }
}

/// Simple Levenshtein distance for typo suggestions.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

// ============================================================================
// FromInvocation Trait (Factory Pattern)
// ============================================================================

use super::traits::CommandBox;
use anyhow::Result as ExoResult;

/// Trait for constructing commands from an Invocation.
///
/// Commands implement this trait to enable spec-driven dispatch.
/// This provides a gradual migration path: commands can be constructed
/// either via Clap enum + From or via Invocation + `FromInvocation`.
///
/// # Example
///
/// ```ignore
/// impl FromInvocation for PhaseStart {
///     fn from_invocation(inv: &Invocation) -> ExoResult<Self> {
///         let id = inv.get_string("id")
///             .ok_or_else(|| anyhow::anyhow!("Missing required argument: id"))?;
///         Ok(PhaseStart::new(id.to_string()))
///     }
/// }
/// ```
pub trait FromInvocation: Sized {
    /// Construct this command from an invocation.
    fn from_invocation(inv: &Invocation) -> ExoResult<Self>;
}

/// Type-erased factory function for creating `CommandBox` from Invocation.
pub type CommandFactory = fn(&Invocation) -> ExoResult<CommandBox>;

/// Registry of factory functions for spec-driven dispatch.
///
/// Maps (namespace, operation) pairs to factory functions that construct
/// `CommandBox` values from Invocations.
#[derive(Default)]
pub struct FactoryRegistry {
    factories: std::collections::HashMap<(String, String), CommandFactory>,
}

impl FactoryRegistry {
    /// Create an empty factory registry.
    pub fn new() -> Self {
        Self {
            factories: std::collections::HashMap::new(),
        }
    }

    /// Register a factory function for a command.
    pub fn register(
        &mut self,
        namespace: impl Into<String>,
        operation: impl Into<String>,
        factory: CommandFactory,
    ) {
        self.factories
            .insert((namespace.into(), operation.into()), factory);
    }

    /// Look up a factory function for a command.
    pub fn get(&self, namespace: &str, operation: &str) -> Option<CommandFactory> {
        self.factories
            .get(&(namespace.to_string(), operation.to_string()))
            .copied()
    }

    /// Construct a `CommandBox` from an Invocation.
    ///
    /// Returns `None` if no factory is registered for the command.
    pub fn construct(&self, inv: &Invocation) -> Option<ExoResult<CommandBox>> {
        self.get(inv.namespace(), inv.operation())
            .map(|factory| factory(inv))
    }

    /// Number of registered factories.
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}

impl std::fmt::Debug for FactoryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryRegistry")
            .field("count", &self.factories.len())
            .finish()
    }
}

/// A dispatcher that combines Router and `FactoryRegistry` for spec-driven execution.
#[derive(Debug)]
pub struct SpecDispatcher<'a> {
    router: Router<'a>,
    factories: &'a FactoryRegistry,
}

impl<'a> SpecDispatcher<'a> {
    /// Create a new spec-driven dispatcher.
    pub const fn new(router: Router<'a>, factories: &'a FactoryRegistry) -> Self {
        Self { router, factories }
    }

    /// Route tokens and construct the command.
    ///
    /// Returns:
    /// - `Ok(Some(CommandBox))` if routing and construction succeeded
    /// - `Ok(None)` if routing succeeded but no factory is registered
    /// - `Err(RoutingResult)` if routing failed
    pub fn route_and_construct(
        &self,
        tokens: &[&str],
    ) -> Result<Option<CommandBox>, RoutingResult> {
        let result = self.router.route(tokens);

        if !result.ok {
            return Err(result);
        }

        let Some(inv) = result.invocation.as_ref() else {
            return Err(RoutingResult {
                invocation: None,
                diagnostics: vec![RoutingDiagnostic::new(
                    DiagnosticCode::InvalidArgType,
                    "Router succeeded without producing an invocation".to_string(),
                )],
                ok: false,
            });
        };

        match self.factories.construct(inv) {
            Some(Ok(cmd)) => Ok(Some(cmd)),
            Some(Err(e)) => {
                // Factory error - convert to routing diagnostic
                Err(RoutingResult {
                    invocation: result.invocation,
                    diagnostics: vec![RoutingDiagnostic::new(
                        DiagnosticCode::InvalidArgType,
                        format!("Failed to construct command: {e}"),
                    )],
                    ok: false,
                })
            }
            None => Ok(None), // No factory registered, fallback to Clap
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;
    use crate::command::command_spec::{
        ArgSpec, CommandSpec, NamespaceSpec, OperationSpec, ValueType,
    };

    fn test_spec() -> CommandSpec {
        let mut spec = CommandSpec::default();

        // Add 'phase' namespace
        let mut phase_ns = NamespaceSpec::new("phase", "Phase lifecycle operations");
        phase_ns.operations.insert(
            "start".to_string(),
            OperationSpec::new("start", "Start a new phase", Effect::Exec).with_arg(
                ArgSpec::positional("id", "Phase ID to start", ValueType::String),
            ),
        );
        phase_ns.operations.insert(
            "status".to_string(),
            OperationSpec::new("status", "Show phase status", Effect::Pure),
        );
        phase_ns.operations.insert(
            "finish".to_string(),
            OperationSpec::new("finish", "Finish current phase", Effect::Exec).with_arg(
                ArgSpec::flag("force", "Force finish even with incomplete tasks").with_short('f'),
            ),
        );
        spec.namespaces.insert("phase".to_string(), phase_ns);

        // Add 'task' namespace
        let mut task_ns = NamespaceSpec::new("task", "Task operations");
        task_ns.operations.insert(
            "complete".to_string(),
            OperationSpec::new("complete", "Complete a task", Effect::Write).with_arg(
                ArgSpec::positional("id", "Task ID to complete", ValueType::String),
            ),
        );
        spec.namespaces.insert("task".to_string(), task_ns);

        spec
    }

    #[test]
    fn test_route_simple_command() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "status"]);
        assert!(result.ok);
        let inv = result.invocation.unwrap();
        assert_eq!(inv.namespace(), "phase");
        assert_eq!(inv.operation(), "status");
    }

    #[test]
    fn test_route_with_positional() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "start", "map-phase-7"]);
        assert!(result.ok);
        let inv = result.invocation.unwrap();
        assert_eq!(inv.namespace(), "phase");
        assert_eq!(inv.operation(), "start");
        assert_eq!(inv.get_string("id"), Some("map-phase-7"));
    }

    #[test]
    fn test_route_with_flag() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "finish", "--force"]);
        assert!(result.ok);
        let inv = result.invocation.unwrap();
        assert_eq!(inv.get_bool("force"), Some(true));
    }

    #[test]
    fn test_route_with_short_flag() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "finish", "-f"]);
        assert!(result.ok);
        let inv = result.invocation.unwrap();
        assert_eq!(inv.get_bool("force"), Some(true));
    }

    #[test]
    fn test_unknown_namespace() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phaes", "start"]); // typo
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnknownNamespace);
        // Should suggest "phase"
        assert!(!result.diagnostics[0].suggestions.is_empty());
    }

    #[test]
    fn test_unknown_operation() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "strat"]); // typo
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnknownOperation);
    }

    #[test]
    fn test_too_many_positionals() {
        let spec = test_spec();
        let router = Router::new(&spec);

        let result = router.route(&["phase", "status", "extra"]);
        assert!(!result.ok);
        assert_eq!(
            result.diagnostics[0].code,
            DiagnosticCode::TooManyPositionals
        );
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("phase", "phase"), 0);
        assert_eq!(levenshtein("phase", "phaes"), 2);
        assert_eq!(levenshtein("phase", "phases"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }
}

#[cfg(test)]
mod invocation_from_json_tests {
    use super::*;
    use crate::api::protocol::Effect;
    use crate::command::command_spec::{
        ArgSpec, CommandSpec, NamespaceSpec, OperationSpec, ValueType,
    };
    use serde_json::json;

    fn json_spec() -> CommandSpec {
        let mut spec = CommandSpec::default();

        let mut task_ns = NamespaceSpec::new("task", "Task operations");
        let op = OperationSpec::new("run", "Run a task", Effect::Exec)
            .with_arg(ArgSpec::option("name", "Task name", ValueType::String))
            .with_arg(ArgSpec::option("active", "Active flag", ValueType::Bool))
            .with_arg(ArgSpec::option(
                "mode",
                "Execution mode",
                ValueType::Enum(vec!["fast".into(), "slow".into()]),
            ))
            .with_arg(ArgSpec::option("note", "Optional note", ValueType::String).optional())
            .with_arg(ArgSpec::option("limit", "Limit", ValueType::Int).with_default("3"))
            .with_arg(ArgSpec::option("ratio", "Ratio", ValueType::Float).optional());

        task_ns.operations.insert("run".to_string(), op);
        spec.namespaces.insert("task".to_string(), task_ns);

        spec
    }

    #[test]
    fn valid_string_argument() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_string("name"), Some("build"));
    }

    #[test]
    fn valid_bool_argument() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": false, "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_bool("active"), Some(false));
    }

    #[test]
    fn valid_enum_argument() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(
            inv.args.get("mode"),
            Some(&TypedValue::Enum("fast".to_string()))
        );
    }

    #[test]
    fn invalid_enum_argument() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "turbo"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn missing_required_argument() {
        let spec = json_spec();
        let input = json!({"active": true, "mode": "fast"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::MissingRequiredArg);
    }

    #[test]
    fn extra_unknown_argument() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast", "extra": "nope"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::UnknownFlag);
    }

    #[test]
    fn type_mismatch_argument() {
        let spec = json_spec();
        // "maybe" is not a valid bool coercion (only true/false/yes/no/1/0)
        let input = json!({"name": "build", "active": "maybe", "mode": "fast"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn string_to_bool_coercion() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": "yes", "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_bool("active"), Some(true));
    }

    #[test]
    fn string_to_int_coercion() {
        let spec = json_spec();
        // limit is an optional Int arg
        let input = json!({"name": "build", "active": true, "mode": "fast", "limit": "42"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_int("limit"), Some(42));
    }

    #[test]
    fn string_to_float_coercion() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast", "ratio": "3.14"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.args.get("ratio"), Some(&TypedValue::Float(3.14)));
    }

    #[test]
    fn invalid_float_string_rejected() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast", "ratio": "abc"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn bool_coercion_zero_one_no() {
        let spec = json_spec();
        // "0" → false
        let input = json!({"name": "build", "active": "0", "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_bool("active"), Some(false));
        // "1" → true
        let input = json!({"name": "build", "active": "1", "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_bool("active"), Some(true));
        // "no" → false
        let input = json!({"name": "build", "active": "no", "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_bool("active"), Some(false));
    }

    #[test]
    fn bool_coercion_empty_string_rejected() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": "", "mode": "fast"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn int_overflow_rejected() {
        let spec = json_spec();
        // i64::MAX + 1 as a string
        let input = json!({"name": "build", "active": true, "mode": "fast", "limit": "9223372036854775808"});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn array_to_csv_string_elements() {
        let spec = json_spec();
        let input =
            json!({"name": "build", "active": true, "mode": "fast", "note": ["a", "b", "c"]});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_string("note"), Some("a,b,c"));
    }

    #[test]
    fn array_to_csv_mixed_elements() {
        let spec = json_spec();
        // Non-string elements are stringified via their JSON representation
        let input =
            json!({"name": "build", "active": true, "mode": "fast", "note": ["hello", 42, true]});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_string("note"), Some("hello,42,true"));
    }

    #[test]
    fn empty_array_rejected() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast", "note": []});
        let err = Invocation::from_json(&input, "task", "run", &spec).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::InvalidArgType);
    }

    #[test]
    fn optional_argument_present() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast", "note": "hello"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_string("note"), Some("hello"));
    }

    #[test]
    fn optional_argument_absent_uses_default() {
        let spec = json_spec();
        let input = json!({"name": "build", "active": true, "mode": "fast"});
        let inv = Invocation::from_json(&input, "task", "run", &spec).unwrap();
        assert_eq!(inv.get_int("limit"), Some(3));
    }

    /// Test that short flag aliases work in JSON input.
    /// When exo-run parses `-n "value"`, it sends `{"n": "value"}`.
    /// The router should resolve "n" to the arg with `short = Some('n')`.
    #[test]
    fn short_flag_alias_lookup() {
        let mut spec = CommandSpec::default();
        let mut strike_ns = NamespaceSpec::new("strike", "Strike operations");
        let op = OperationSpec::new("start", "Start a strike", Effect::Exec)
            .with_arg(ArgSpec::option("name", "Strike name", ValueType::String).with_short('n'))
            .with_arg(ArgSpec::option("goal", "Strike goal", ValueType::String).with_short('g'));
        strike_ns.operations.insert("start".to_string(), op);
        spec.namespaces.insert("strike".to_string(), strike_ns);

        // Input uses short flag keys (as sent by exo-run TypeScript)
        let input = json!({"n": "Test Strike", "g": "Fix the bug"});
        let inv = Invocation::from_json(&input, "strike", "start", &spec).unwrap();

        // Values should be stored by the canonical id (long name)
        assert_eq!(inv.get_string("name"), Some("Test Strike"));
        assert_eq!(inv.get_string("goal"), Some("Fix the bug"));
    }
}
