use crate::ExoResult;
use crate::api::protocol::Effect;
use crate::command::{
    Command, CommandBox, CommandContext, CommandOutput, OutputFormat, UpdateCommand, Write,
};
use crate::context::AgentContext;
use crate::steering::SuggestedAction;
use crate::{map, status};
use std::path::{Path, PathBuf};

// ============================================================================
// ExoSpec definition — single source of truth for root namespace commands
// ============================================================================

/// Root namespace command specification (namespace = "").
///
/// This enum is the authoritative definition of the root namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `RootCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, Clone, exospec::ExoSpec)]
#[exo(namespace = "", description = "Root commands")]
pub enum RootCommands {
    #[exo(effect = "pure", description = "Show project status")]
    Status,

    #[exo(effect = "pure", description = "Show the project map")]
    Map {
        #[exo(flag, description = "Show only the single best next action")]
        next: bool,
        #[exo(long, optional, description = "Explain why a command is suggested")]
        why: Option<String>,
    },

    #[exo(
        effect = "write",
        description = "Write content to an agent-context file"
    )]
    Write {
        #[exo(positional, description = "Relative path within agent-context")]
        path: String,
        #[exo(flag, description = "Skip validation (write raw content)")]
        raw: bool,
    },

    #[exo(effect = "write", description = "Apply all project upgrades")]
    Update,
}

impl RootCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Status => CommandBox::pure(StatusCommand::new()),
            Self::Map { next, why } => CommandBox::pure(MapCommand::new(next, why)),
            Self::Write { path, raw } => CommandBox::mutable(Write::new(path, raw)),
            Self::Update => CommandBox::mutable(UpdateCommand::new()),
        })
    }
}

fn resolve_workspace_root(start: &Path) -> PathBuf {
    let mut current = start;

    loop {
        if current.join("exosuit.toml").exists() {
            return current.to_path_buf();
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => return start.to_path_buf(),
        }
    }
}

/// Root command: `exo status`.
#[derive(Debug, Clone, Copy)]
pub struct StatusCommand;

impl StatusCommand {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for StatusCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for StatusCommand {
    fn namespace(&self) -> &'static str {
        ""
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "Show project status"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let root = resolve_workspace_root(ctx.root);
        let agent_ctx = AgentContext::load_with_project(root, ctx.project.cloned())?;

        match ctx.format {
            OutputFormat::Json => {
                let json = status::build_status_json(&agent_ctx, ctx.agent_id.as_deref())?;
                Ok(CommandOutput::data(json))
            }
            OutputFormat::Human => {
                status::show_status_human(&agent_ctx, ctx.agent_id.as_deref())?;
                Ok(CommandOutput::message(""))
            }
        }
    }
}

/// Root command: `exo map`.
#[derive(Debug, Clone)]
pub struct MapCommand {
    pub next: bool,
    pub why: Option<String>,
}

impl MapCommand {
    pub const fn new(next: bool, why: Option<String>) -> Self {
        Self { next, why }
    }
}

impl Default for MapCommand {
    fn default() -> Self {
        Self::new(false, None)
    }
}

impl Command for MapCommand {
    fn namespace(&self) -> &'static str {
        ""
    }

    fn operation(&self) -> &'static str {
        "map"
    }

    fn description(&self) -> &'static str {
        "Show the project map"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let root = resolve_workspace_root(ctx.root);
        let agent_ctx = AgentContext::load(root)?;

        match ctx.format {
            OutputFormat::Json => {
                let json = map::build_map_json(
                    &agent_ctx,
                    self.next,
                    self.why.as_deref(),
                    ctx.agent_id.as_deref(),
                )?;
                Ok(CommandOutput::data(json))
            }
            OutputFormat::Human => {
                map::show_map_human(
                    &agent_ctx,
                    self.next,
                    self.why.as_deref(),
                    ctx.agent_id.as_deref(),
                )?;
                Ok(CommandOutput::message(""))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::transport::{
        CommandError, ConfirmResult, SteeringOutput, TransportContext, TransportOutput,
    };

    /// A transport that provides a temp workspace with a valid exosuit.toml
    /// and empty SQLite database, avoiding dependence on the repo checkout.
    struct JsonTransport {
        _temp_dir: tempfile::TempDir,
        workspace_root: std::path::PathBuf,
    }

    impl JsonTransport {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("create temp dir");
            let root = temp_dir.path().to_path_buf();

            // Minimal exosuit.toml so resolve_workspace_root finds it
            std::fs::write(root.join("exosuit.toml"), "[project]\nname = \"test\"\n")
                .expect("write exosuit.toml");

            // Create .cache dir and empty database
            let cache_dir = root.join(".cache");
            std::fs::create_dir_all(&cache_dir).expect("create .cache");
            exosuit_storage::open_database(cache_dir.join("exo.db")).expect("create test database");

            Self {
                _temp_dir: temp_dir,
                workspace_root: root,
            }
        }
    }

    impl TransportContext for JsonTransport {
        fn workspace_root(&self) -> Option<&std::path::Path> {
            Some(&self.workspace_root)
        }

        fn confirm_exec(&self, _action: &str) -> ConfirmResult {
            ConfirmResult::Proceed
        }

        fn format_output(&self, output: CommandOutput) -> TransportOutput {
            TransportOutput::Json(output.data)
        }

        fn format_error(&self, error: CommandError) -> TransportOutput {
            TransportOutput::Json(serde_json::json!({
                "error": error.message(),
            }))
        }

        fn render_steering(&self, _suggestions: Vec<SuggestedAction>) -> SteeringOutput {
            SteeringOutput::Json(serde_json::json!({ "suggestions": [] }))
        }
    }

    #[test]
    fn status_command_invoke_json() {
        let cmd = StatusCommand::new();
        let transport = JsonTransport::new();
        let input = serde_json::json!({});

        let output = cmd
            .invoke_json(&input, &transport)
            .expect("expected status invoke_json success");

        assert!(output.is_object());
    }

    #[test]
    fn map_command_invoke_json() {
        let cmd = MapCommand::new(false, None);
        let transport = JsonTransport::new();
        let input = serde_json::json!({"next": true});

        let output = cmd
            .invoke_json(&input, &transport)
            .expect("expected map invoke_json success");

        assert!(output.is_object());
    }
}
