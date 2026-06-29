//! AI namespace commands.
//!
//! - `ai context`: Dump context for AI handoff (Pure)
//! - `ai prompt`: Get a prompt template (Pure)
//! - `ai chat-history`: Read recent chat history from VS Code sessions (Pure)

use super::traits::{Command, CommandBox, CommandContext, CommandOutput, OutputFormat};
use crate::api::protocol::Effect;
use anyhow::{Context, Result as ExoResult};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

// ============================================================================
// ExoSpec definition — single source of truth for the ai namespace
// ============================================================================

/// AI namespace command specification.
///
/// This enum is the authoritative definition of the ai namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `AiCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "ai", description = "AI context and prompt commands")]
pub enum AiCommands {
    #[exo(effect = "pure", description = "Dump context for AI handoff")]
    Context {
        #[exo(long, optional, description = "Focus area for context dump")]
        focus: Option<String>,
        #[exo(flag, description = "Include deprecated files in output")]
        full: bool,
    },

    #[exo(effect = "pure", description = "Get a prompt template")]
    Prompt {
        #[exo(positional, description = "Name of the prompt template")]
        name: String,
    },

    #[exo(
        effect = "pure",
        operation = "chat-history",
        description = "Read recent chat history from VS Code sessions. Use this to recover context lost during conversation summarization—especially nuanced user feedback, decisions, and steering. Always provide match-text with a distinctive phrase from a recent user message to identify the correct session."
    )]
    ChatHistory {
        #[exo(
            long,
            optional,
            description = "Number of recent turns to retrieve (default: 10, max: 50)"
        )]
        turns: Option<i64>,
        #[exo(
            long,
            optional,
            description = "Exact workspace URI to match (e.g., file:///path/to/workspace)"
        )]
        workspace_uri: Option<String>,
        #[exo(
            long,
            optional,
            description = "Text snippet to match in user messages (identifies the correct session)"
        )]
        match_text: Option<String>,
        #[exo(flag, description = "Include extended thinking content")]
        include_thinking: bool,
        #[exo(flag, description = "Include tool invocations")]
        include_tools: bool,
        #[exo(
            flag,
            description = "Get turns before the last summarization (context that was just compacted)"
        )]
        before_summary: bool,
    },
}

impl AiCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Context { focus, full } => CommandBox::pure(AiContext::new(focus, full)),
            Self::Prompt { name } => CommandBox::pure(AiPrompt::new(name)),
            Self::ChatHistory {
                turns,
                workspace_uri,
                match_text,
                include_thinking,
                include_tools,
                before_summary,
            } => {
                let turns = turns
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(10);
                CommandBox::pure(AiChatHistory::new(
                    turns,
                    workspace_uri,
                    match_text,
                    include_thinking,
                    include_tools,
                    before_summary,
                ))
            }
        })
    }
}

// ============================================================================
// ai context
// ============================================================================

/// Dump context for AI handoff.
#[derive(Debug, Clone)]
pub struct AiContext {
    pub focus: Option<String>,
    pub full: bool,
}

impl AiContext {
    pub const fn new(focus: Option<String>, full: bool) -> Self {
        Self { focus, full }
    }
}

#[derive(Debug, Serialize)]
struct FileContent {
    path: String,
    content: String,
    deprecated: bool,
}

#[derive(Debug, Serialize)]
struct AiContextOutput {
    kind: &'static str,
    ok: bool,
    files: Vec<FileContent>,
    has_deprecated: bool,
}

impl Command for AiContext {
    fn namespace(&self) -> &'static str {
        "ai"
    }

    fn operation(&self) -> &'static str {
        "context"
    }

    fn description(&self) -> &'static str {
        "Dump context for AI handoff"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let canonical_files = vec!["AGENTS.md"];

        let mut files = Vec::new();
        let mut human_output = String::new();

        // Read canonical files from disk
        for file in &canonical_files {
            let path = ctx.root.join(file);
            if path.exists() {
                let content =
                    fs::read_to_string(&path).with_context(|| format!("Failed to read {file}"))?;
                files.push(FileContent {
                    path: file.to_string(),
                    content: content.trim().to_string(),
                    deprecated: false,
                });

                human_output.push_str(&format!("--- {file} ---\n"));
                human_output.push_str(content.trim());
                human_output.push_str("\n\n");
            }
        }

        // Read axioms from SQLite
        let db_path = ctx.db_path();
        if db_path.exists() {
            let loader = crate::context::SqliteLoader::open(&db_path)
                .with_context(|| "Failed to open SQLite database for axiom context")?;

            for scope in &["workflow", "system", "design"] {
                let axioms = loader.list_axioms(Some(scope)).unwrap_or_default();
                if !axioms.is_empty() {
                    let label = format!("axioms (scope: {scope})");
                    let mut content = String::new();
                    for axiom in &axioms {
                        content.push_str(&format!("- {}: {}\n", axiom.id, axiom.principle));
                        if let Some(ref rationale) = axiom.rationale {
                            content.push_str(&format!("  rationale: {rationale}\n"));
                        }
                        for imp in &axiom.implications {
                            content.push_str(&format!("  implication: {imp}\n"));
                        }
                    }
                    files.push(FileContent {
                        path: label.clone(),
                        content: content.trim().to_string(),
                        deprecated: false,
                    });

                    human_output.push_str(&format!("--- {label} ---\n"));
                    human_output.push_str(content.trim());
                    human_output.push_str("\n\n");
                }
            }
        }

        let output = AiContextOutput {
            kind: "ai.context",
            ok: true,
            files,
            has_deprecated: false,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, human_output)),
        }
    }
}

// ============================================================================
// ai prompt
// ============================================================================

/// Get a prompt template.
#[derive(Debug, Clone)]
pub struct AiPrompt {
    pub name: String,
}

impl AiPrompt {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(Debug, Serialize)]
struct AiPromptOutput {
    kind: &'static str,
    ok: bool,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    found: bool,
}

impl Command for AiPrompt {
    fn namespace(&self) -> &'static str {
        "ai"
    }

    fn operation(&self) -> &'static str {
        "prompt"
    }

    fn description(&self) -> &'static str {
        "Get a prompt template"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let prompt_path = ctx
            .root
            .join(".github/prompts")
            .join(format!("{}.prompt.md", self.name));

        if prompt_path.exists() {
            let content = fs::read_to_string(&prompt_path).context("Failed to read prompt")?;
            let output = AiPromptOutput {
                kind: "ai.prompt",
                ok: true,
                name: self.name.clone(),
                content: Some(content.clone()),
                found: true,
            };

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(output, content)),
            }
        } else {
            let output = AiPromptOutput {
                kind: "ai.prompt",
                ok: true,
                name: self.name.clone(),
                content: None,
                found: false,
            };
            let msg = format!("Prompt '{}' not found.", self.name);

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(output, msg)),
            }
        }
    }
}

// ============================================================================
// ai chat-history
// ============================================================================

/// Read recent chat history from VS Code sessions.
#[derive(Debug, Clone)]
pub struct AiChatHistory {
    pub turns: usize,
    pub workspace_uri: Option<String>,
    pub match_text: Option<String>,
    pub include_thinking: bool,
    pub include_tools: bool,
    pub before_summary: bool,
}

impl AiChatHistory {
    pub const fn new(
        turns: usize,
        workspace_uri: Option<String>,
        match_text: Option<String>,
        include_thinking: bool,
        include_tools: bool,
        before_summary: bool,
    ) -> Self {
        Self {
            turns,
            workspace_uri,
            match_text,
            include_thinking,
            include_tools,
            before_summary,
        }
    }
}

#[derive(Debug, Serialize)]
struct AiChatHistoryOutput {
    kind: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

impl Command for AiChatHistory {
    fn namespace(&self) -> &'static str {
        "ai"
    }

    fn operation(&self) -> &'static str {
        "chat-history"
    }

    fn description(&self) -> &'static str {
        "Read recent chat history from VS Code sessions. Use this to recover context lost during conversation summarization—especially nuanced user feedback, decisions, and steering. Always provide match-text with a distinctive phrase from a recent user message to identify the correct session."
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        // Build exohistory command using resolved binary path
        let mut cmd = resolve_exohistory_command(ctx.root)?;
        cmd.arg("recent");
        cmd.arg("--turns").arg(self.turns.to_string());
        cmd.arg("--format").arg("json");

        if let Some(ref uri) = self.workspace_uri {
            cmd.arg("--workspace-uri").arg(uri);
        }
        if let Some(ref text) = self.match_text {
            cmd.arg("--match-text").arg(text);
        }
        if self.include_thinking {
            cmd.arg("--include-thinking");
        }
        if self.include_tools {
            cmd.arg("--include-tools");
        }
        if self.before_summary {
            cmd.arg("--before-summary");
        }

        // Execute exohistory
        let output = cmd
            .output()
            .context("Failed to execute exohistory. Is it installed? (cargo install --path crates/exohistory)")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse as JSON to validate and re-emit
            let data: serde_json::Value =
                serde_json::from_str(&stdout).context("Invalid JSON from exohistory")?;

            let result = AiChatHistoryOutput {
                kind: "ai.chat-history",
                ok: true,
                error: None,
                data: Some(data.clone()),
            };

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(result)),
                OutputFormat::Human => {
                    // Pretty-print for human output
                    let pretty =
                        serde_json::to_string_pretty(&data).unwrap_or_else(|_| stdout.to_string());
                    Ok(CommandOutput::new(result, pretty))
                }
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let result = AiChatHistoryOutput {
                kind: "ai.chat-history",
                ok: false,
                error: Some(stderr.trim().to_string()),
                data: None,
            };

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(result)),
                OutputFormat::Human => Ok(CommandOutput::new(result, stderr.to_string())),
            }
        }
    }
}

// ============================================================================
// Helper: resolve exohistory binary
// ============================================================================

/// Resolve the exohistory binary, checking local build paths first.
///
/// Resolution order:
/// 1. `{root}/target/release/exohistory`
/// 2. `{root}/target/debug/exohistory`
/// 3. `exohistory` in PATH
/// 4. `cargo run -q -p exohistory --`
fn resolve_exohistory_command(root: &Path) -> ExoResult<ProcessCommand> {
    let release = root.join("target/release/exohistory");
    if is_executable(&release) {
        return Ok(ProcessCommand::new(release));
    }

    let debug = root.join("target/debug/exohistory");
    if is_executable(&debug) {
        return Ok(ProcessCommand::new(debug));
    }

    if find_in_path("exohistory").is_some() {
        return Ok(ProcessCommand::new("exohistory"));
    }

    if find_in_path("cargo").is_some() {
        let mut cmd = ProcessCommand::new("cargo");
        cmd.arg("run")
            .arg("-q")
            .arg("-p")
            .arg("exohistory")
            .arg("--");
        return Ok(cmd);
    }

    anyhow::bail!(
        "could not find exohistory binary. Build with `cargo build -p exohistory` or install with `cargo install --path crates/exohistory`"
    )
}

fn find_in_path(name: &str) -> Option<std::path::PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for path in std::env::split_paths(&paths) {
        let candidate = path.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_context_metadata() {
        let cmd = AiContext::new(None, false);
        assert_eq!(cmd.namespace(), "ai");
        assert_eq!(cmd.operation(), "context");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_ai_prompt_metadata() {
        let cmd = AiPrompt::new("test");
        assert_eq!(cmd.namespace(), "ai");
        assert_eq!(cmd.operation(), "prompt");
        assert_eq!(cmd.effect(), Effect::Pure);
    }
}
