//! Docs namespace commands.
//!
//! - `docs links.check`: Validate documentation links (Pure)
//! - `docs links.fix`: Repair documentation links (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::{Effect, Steering};
use crate::docs_links;
use anyhow::Result as ExoResult;
use serde_json::{Value as JsonValue, json};

fn wrap_result_with_steering(result: JsonValue, steering: Option<Steering>) -> JsonValue {
    match steering {
        Some(steering) => json!({
            "_command_envelope": {
                "result": result,
                "steering": serde_json::to_value(steering).unwrap_or_else(|_| json!({})),
            }
        }),
        None => result,
    }
}

// ============================================================================
// ExoSpec definition — single source of truth for the docs namespace
// ============================================================================

/// Docs namespace command specification.
///
/// This enum is the authoritative definition of the docs namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `DocsCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "docs", description = "Documentation management commands")]
pub enum DocsCommands {
    #[exo(
        effect = "pure",
        operation = "links.check",
        description = "Validate documentation links"
    )]
    LinksCheck {
        #[exo(long, optional, json, description = "Docs link targets")]
        targets: Option<String>,
        #[exo(long, optional, json, description = "Docs link options")]
        options: Option<String>,
    },

    #[exo(
        effect = "write",
        operation = "links.fix",
        description = "Repair documentation links"
    )]
    LinksFix {
        #[exo(long, optional, json, description = "Docs link targets")]
        targets: Option<String>,
        #[exo(long, optional, json, description = "Docs link options")]
        options: Option<String>,
    },
}

impl DocsCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        fn parse_docs_links_input(
            targets: Option<String>,
            options: Option<String>,
        ) -> docs_links::DocsLinksInput {
            let mut input = json!({});

            if let Some(targets) = targets
                && let Ok(value) = serde_json::from_str::<JsonValue>(&targets)
            {
                input["targets"] = value;
            }

            if let Some(options) = options
                && let Ok(value) = serde_json::from_str::<JsonValue>(&options)
            {
                input["options"] = value;
            }

            docs_links::parse_input(&input).unwrap_or_default()
        }

        Ok(match self {
            Self::LinksCheck { targets, options } => {
                let input = parse_docs_links_input(targets, options);
                CommandBox::pure(DocsLinksCheckCommand::new(input))
            }
            Self::LinksFix { targets, options } => {
                let input = parse_docs_links_input(targets, options);
                CommandBox::mutable(DocsLinksFixCommand::new(input))
            }
        })
    }
}

// ============================================================================
// docs links.check
// ============================================================================

/// Validate documentation links.
#[derive(Debug, Clone, Default)]
pub struct DocsLinksCheckCommand {
    input: docs_links::DocsLinksInput,
}

impl DocsLinksCheckCommand {
    pub const fn new(input: docs_links::DocsLinksInput) -> Self {
        Self { input }
    }
}

impl Command for DocsLinksCheckCommand {
    fn namespace(&self) -> &'static str {
        "docs"
    }

    fn operation(&self) -> &'static str {
        "links.check"
    }

    fn description(&self) -> &'static str {
        "Validate documentation links"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let (result, steering) =
            docs_links::run_check_with_project(ctx.root, ctx.project, &self.input)?;
        let result_json = docs_links::result_to_json(result);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(wrap_result_with_steering(
                result_json,
                steering,
            ))),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&result_json)?;
                Ok(CommandOutput::new(result_json, pretty))
            }
        }
    }
}

// ============================================================================
// docs links.fix
// ============================================================================

/// Repair documentation links.
#[derive(Debug, Clone, Default)]
pub struct DocsLinksFixCommand {
    input: docs_links::DocsLinksInput,
}

impl DocsLinksFixCommand {
    pub const fn new(input: docs_links::DocsLinksInput) -> Self {
        Self { input }
    }
}

impl Command for DocsLinksFixCommand {
    fn namespace(&self) -> &'static str {
        "docs"
    }

    fn operation(&self) -> &'static str {
        "links.fix"
    }

    fn description(&self) -> &'static str {
        "Repair documentation links"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("DocsLinksFixCommand should be dispatched via execute_mut")
    }
}

impl MutableCommand for DocsLinksFixCommand {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let result = docs_links::run_fix(ctx.root, &self.input)?;
        let result_json = docs_links::result_to_json(result);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result_json)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&result_json)?;
                Ok(CommandOutput::new(result_json, pretty))
            }
        }
    }
}
