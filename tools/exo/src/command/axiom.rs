//! Axiom namespace commands.
//!
//! - `axiom add`: Add a new axiom (Write)
//! - `axiom list`: List axioms (Pure)
//! - `axiom remove`: Remove an axiom (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::axiom::{self, AxiomScope};
use crate::context::AgentContext;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for axiom commands.
fn default_axiom_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "List axioms".to_string(),
        command: "exo axiom list".to_string(),
        rationale: "View all axioms to understand system constraints.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the axiom namespace
// ============================================================================

/// Axiom namespace command specification.
///
/// This enum is the authoritative definition of the axiom namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `AxiomCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "axiom", description = "Axiom management commands")]
pub enum AxiomCommands {
    #[exo(effect = "write", description = "Add a new axiom")]
    Add {
        #[exo(
            long,
            optional,
            description = "The axiom scope (system, workflow, design)"
        )]
        scope: Option<String>,
        #[exo(long, description = "The axiom ID")]
        id: String,
        #[exo(long, description = "The principle of the axiom")]
        principle: String,
        #[exo(
            long,
            optional,
            description = "Why this axiom exists (stored as rationale)"
        )]
        why: Option<String>,
        #[exo(long, optional, description = "The implication of this axiom")]
        implication: Option<String>,
    },

    #[exo(effect = "pure", description = "List axioms")]
    List {
        #[exo(
            long,
            optional,
            description = "Filter by scope (system, workflow, legacy)"
        )]
        scope: Option<String>,
    },

    #[exo(effect = "write", description = "Remove an axiom")]
    Remove {
        #[exo(
            long,
            optional,
            description = "The axiom scope (system, workflow, design)"
        )]
        scope: Option<String>,
        #[exo(long, description = "The axiom ID to remove")]
        id: String,
    },
}

impl AxiomCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        fn parse_scope(scope: Option<String>) -> anyhow::Result<AxiomScope> {
            let Some(scope) = scope else {
                return Ok(AxiomScope::Workflow);
            };

            match scope.to_lowercase().as_str() {
                "workflow" => Ok(AxiomScope::Workflow),
                "system" => Ok(AxiomScope::System),
                "design" => Ok(AxiomScope::Design),
                other => Err(anyhow::anyhow!(
                    "Invalid value for argument 'scope': {other}"
                )),
            }
        }

        const fn scope_str(scope: AxiomScope) -> &'static str {
            match scope {
                AxiomScope::Workflow => "workflow",
                AxiomScope::System => "system",
                AxiomScope::Design => "design",
            }
        }

        Ok(match self {
            Self::Add {
                scope,
                id,
                principle,
                why,
                implication,
            } => {
                let text = format!(
                    "{id}::{principle}::{}::{}",
                    why.unwrap_or_default(),
                    implication.unwrap_or_default()
                );
                CommandBox::mutable(AxiomAdd::new(parse_scope(scope)?, text))
            }
            Self::List { scope } => CommandBox::pure(AxiomList::new(parse_scope(scope)?)),
            Self::Remove { scope, id } => {
                let parsed_scope = parse_scope(scope.clone())?;
                let _ = scope_str(parsed_scope);
                CommandBox::mutable(AxiomRemove::new(parse_scope(scope)?, id))
            }
        })
    }
}

// ============================================================================
// axiom add
// ============================================================================

/// Add a new axiom.
#[derive(Debug, Clone)]
pub struct AxiomAdd {
    pub scope: AxiomScope,
    pub text: String,
}

impl AxiomAdd {
    pub fn new(scope: AxiomScope, text: impl Into<String>) -> Self {
        Self {
            scope,
            text: text.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AxiomAddOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for AxiomAdd {
    fn namespace(&self) -> &'static str {
        "axiom"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new axiom"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_axiom_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("AxiomAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for AxiomAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let (id, principle, rationale, implication) = parse_axiom_text(&self.text);
        let axiom_data = axiom::Axiom {
            id: id.clone(),
            principle,
            rationale,
            implications: implication.into_iter().collect(),
            notes: None,
            tags: Vec::new(),
        };

        axiom::add_axiom(ctx.root, scope_name(self.scope), axiom_data)?;

        let output = AxiomAddOutput {
            kind: "axiom.add",
            ok: true,
            id: id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, format!("Added axiom: {id}"))),
        }
    }
}

fn parse_axiom_text(text: &str) -> (String, String, Option<String>, Option<String>) {
    let mut parts = text.split("::").map(str::trim);
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    let third = parts.next();
    let fourth = parts.next();

    if let Some(principle) = second {
        let id = if first.is_empty() {
            axiom_id_from_text(principle)
        } else {
            first.to_string()
        };
        return (
            id,
            principle.to_string(),
            third.filter(|value| !value.is_empty()).map(str::to_string),
            fourth.filter(|value| !value.is_empty()).map(str::to_string),
        );
    }

    let id = axiom_id_from_text(first);
    (id, first.to_string(), None, None)
}

fn axiom_id_from_text(text: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in text.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "axiom".to_string()
    } else {
        trimmed.to_string()
    }
}

// ============================================================================
// axiom list
// ============================================================================

/// List axioms in a scope.
#[derive(Debug, Clone, Copy)]
pub struct AxiomList {
    pub scope: AxiomScope,
}

impl AxiomList {
    pub const fn new(scope: AxiomScope) -> Self {
        Self { scope }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AxiomListEntry {
    id: String,
    principle: String,
    rationale: Option<String>,
    implications: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AxiomListOutput {
    kind: &'static str,
    ok: bool,
    axioms: Vec<AxiomListEntry>,
}

impl Command for AxiomList {
    fn namespace(&self) -> &'static str {
        "axiom"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List axioms"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_axiom_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
        let result = axiom::list_axioms(&agent_ctx.root, scope_name(self.scope))?;

        let axioms: Vec<AxiomListEntry> = result
            .into_iter()
            .map(|a| AxiomListEntry {
                id: a.id,
                principle: a.principle,
                rationale: a.rationale,
                implications: a.implications,
            })
            .collect();

        let output = AxiomListOutput {
            kind: "axiom.list",
            ok: true,
            axioms: axioms.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if axioms.is_empty() {
                    Ok(CommandOutput::new(output, "No axioms found.".to_string()))
                } else {
                    let mut msg = String::new();
                    msg.push_str("| ID | Principle | Rationale | Implications |\n");
                    msg.push_str("| :--- | :--- | :--- | :--- |\n");

                    for axiom in &axioms {
                        let rationale = axiom.rationale.clone().unwrap_or_default();
                        let implications = if axiom.implications.is_empty() {
                            String::new()
                        } else {
                            axiom.implications.join("; ")
                        };
                        msg.push_str(&format!(
                            "| {} | {} | {} | {} |\n",
                            axiom.id, axiom.principle, rationale, implications
                        ));
                    }

                    Ok(CommandOutput::new(output, msg))
                }
            }
        }
    }
}

// ============================================================================
// axiom remove
// ============================================================================

/// Remove an axiom.
#[derive(Debug, Clone)]
pub struct AxiomRemove {
    pub scope: AxiomScope,
    pub id: String,
}

impl AxiomRemove {
    pub fn new(scope: AxiomScope, id: impl Into<String>) -> Self {
        Self {
            scope,
            id: id.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AxiomRemoveOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for AxiomRemove {
    fn namespace(&self) -> &'static str {
        "axiom"
    }

    fn operation(&self) -> &'static str {
        "remove"
    }

    fn description(&self) -> &'static str {
        "Remove an axiom"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_axiom_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("AxiomRemove should be dispatched via execute_mut")
    }
}

impl MutableCommand for AxiomRemove {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let _ = self.scope;
        axiom::remove_axiom(ctx.root, &self.id)?;

        let output = AxiomRemoveOutput {
            kind: "axiom.remove",
            ok: true,
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Removed axiom: {}", self.id),
            )),
        }
    }
}

const fn scope_name(scope: AxiomScope) -> &'static str {
    match scope {
        AxiomScope::Workflow => "workflow",
        AxiomScope::System => "system",
        AxiomScope::Design => "design",
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axiom_add_metadata() {
        let cmd = AxiomAdd::new(AxiomScope::Workflow, "Test principle");
        assert_eq!(cmd.namespace(), "axiom");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_axiom_list_metadata() {
        let cmd = AxiomList::new(AxiomScope::Workflow);
        assert_eq!(cmd.namespace(), "axiom");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_axiom_remove_metadata() {
        let cmd = AxiomRemove::new(AxiomScope::Workflow, "test-axiom");
        assert_eq!(cmd.namespace(), "axiom");
        assert_eq!(cmd.operation(), "remove");
        assert_eq!(cmd.effect(), Effect::Write);
    }
}
