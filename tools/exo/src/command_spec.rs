use serde::{Deserialize, Serialize};

/// Command effect classification for capability tree generation.
///
/// This is re-exported from `api::protocol::Effect` so that `command_spec`
/// consumers don't need to depend on the protocol module directly.
pub use crate::api::protocol::Effect;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommandId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArgId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub root: CommandNode,
}

impl CommandSpec {
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(root: CommandNode) -> Self {
        Self { root }
    }

    /// Validate the command spec for internal consistency.
    ///
    /// Returns a list of validation errors. An empty list means the spec is valid.
    #[must_use]
    pub fn validate(&self) -> Vec<SpecError> {
        let mut errors = Vec::new();
        Self::validate_node(&self.root, &mut Vec::new(), &mut errors);
        errors
    }

    fn validate_node(node: &CommandNode, path: &mut Vec<String>, errors: &mut Vec<SpecError>) {
        path.push(node.name.clone());

        // Check for duplicate arg IDs within this node
        let mut seen_arg_ids = std::collections::HashSet::new();
        for arg in &node.args {
            if !seen_arg_ids.insert(&arg.id) {
                errors.push(SpecError {
                    path: path.clone(),
                    kind: SpecErrorKind::DuplicateArgId(arg.id.clone()),
                });
            }

            // Validate enum specs have at least one variant
            match &arg.kind {
                ArgKind::Option {
                    value: ValueKind::Enum(spec),
                }
                | ArgKind::Positional {
                    value: ValueKind::Enum(spec),
                } if spec.variants.is_empty() => {
                    errors.push(SpecError {
                        path: path.clone(),
                        kind: SpecErrorKind::EmptyEnumVariants(arg.id.clone()),
                    });
                }
                _ => {}
            }
        }

        // Check for duplicate child command IDs
        let mut seen_child_ids = std::collections::HashSet::new();
        for child in &node.children {
            if !seen_child_ids.insert(&child.id) {
                errors.push(SpecError {
                    path: path.clone(),
                    kind: SpecErrorKind::DuplicateCommandId(child.id.clone()),
                });
            }
            Self::validate_node(child, path, errors);
        }

        path.pop();
    }
}

/// Error found during command spec validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecError {
    /// Command path where the error occurred.
    pub path: Vec<String>,
    /// The kind of validation error.
    pub kind: SpecErrorKind,
}

/// Kinds of spec validation errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecErrorKind {
    /// Duplicate argument ID within a command.
    DuplicateArgId(ArgId),
    /// Duplicate child command ID.
    DuplicateCommandId(CommandId),
    /// Enum value kind has no variants.
    EmptyEnumVariants(ArgId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandNode {
    pub id: CommandId,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// Effect classification for leaf commands (operations).
    /// - `None` for namespace nodes (non-leaf commands with children)
    /// - `Some(Effect::Pure)` for read-only operations
    /// - `Some(Effect::Write)` for operations that modify state
    /// - `Some(Effect::Exec)` for operations that execute external processes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    pub args: Vec<ArgSpec>,
    pub children: Vec<Self>,
}

impl CommandNode {
    /// Create a leaf command node (no children, no effect yet).
    ///
    /// For operations (true leaves), call `.with_effect()` to set the effect.
    /// For namespaces (will have children added), leave effect as None.
    /// Create a new command node.
    ///
    /// This is a general constructor used for both:
    /// - **Namespace nodes**: Will have children added via `.with_children()`
    /// - **Operation nodes**: Will have an effect set via `.with_effect()`
    ///
    /// For operations (true leaves), call `.with_effect()` to set the effect.
    /// Leaf nodes without an effect are considered incomplete and will trigger
    /// a debug assertion in capability tree generation.
    pub fn leaf(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: CommandId(id.into()),
            name: name.into(),
            about: None,
            effect: None,
            args: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Set the effect for this command node (builder pattern).
    #[must_use]
    pub const fn with_effect(mut self, effect: Effect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// Set the about/description for this command node (builder pattern).
    #[must_use]
    pub fn with_about(mut self, about: impl Into<String>) -> Self {
        self.about = Some(about.into());
        self
    }

    /// Returns true if this is a leaf node (operation) - has no children.
    #[must_use]
    pub const fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns true if this is a namespace node - has children.
    #[must_use]
    pub const fn is_namespace(&self) -> bool {
        !self.children.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    pub id: ArgId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    pub kind: ArgKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub repeatable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgKind {
    Flag,
    Option { value: ValueKind },
    Positional { value: ValueKind },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Bool,
    Int,
    Float,
    String,
    Path,
    Json,
    /// Enum with a fixed set of allowed variants.
    Enum(EnumSpec),
}

/// Specification for an enum argument with known variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumSpec {
    /// The allowed variant names.
    pub variants: Vec<String>,
}
