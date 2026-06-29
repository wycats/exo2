//! Root namespace `write` command.
//!
//! This is an escape valve for writing to `docs/agent-context/` from stdin.

use super::traits::{
    Command, CommandContext, CommandOutput, MutableCommand, MutableCommandContext, OutputFormat,
};
use crate::api::protocol::Effect;
use anyhow::{Result as ExoResult, anyhow};
use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

/// Write content to an agent-context file.
#[derive(Debug, Clone)]
pub struct Write {
    pub path: String,
    pub raw: bool,
}

impl Write {
    pub fn new(path: impl Into<String>, raw: bool) -> Self {
        Self {
            path: path.into(),
            raw,
        }
    }
}

#[derive(Debug, Serialize)]
struct WriteOutput {
    kind: &'static str,
    ok: bool,
    path: String,
    bytes: usize,
}

impl Command for Write {
    fn namespace(&self) -> &'static str {
        ""
    }

    fn operation(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write content to an agent-context file"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("Write should be dispatched via execute_mut")
    }
}

impl MutableCommand for Write {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content)?;

        if !self.raw {
            if self.path.ends_with(".toml") {
                if let Err(err) = content.parse::<toml::Table>() {
                    return Err(anyhow!("Invalid TOML syntax: {err}"));
                }
            } else if self.path.ends_with(".json")
                && let Err(err) = serde_json::from_str::<serde_json::Value>(&content)
            {
                return Err(anyhow!("Invalid JSON syntax: {err}"));
            }
        }

        if self.path.contains("..") {
            return Err(anyhow!(
                "Path contains parent directory traversal: {}",
                self.path
            ));
        }

        let agent_context_dir = ctx.root.join("docs/agent-context");
        let target_path: PathBuf = agent_context_dir.join(&self.path);

        let canonical_base = agent_context_dir
            .canonicalize()
            .unwrap_or_else(|_| agent_context_dir.clone());

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let canonical_target = target_path
            .canonicalize()
            .unwrap_or_else(|_| target_path.clone());

        if !canonical_target.starts_with(&canonical_base) {
            return Err(anyhow!(
                "Path escapes agent-context directory: {}",
                target_path.display()
            ));
        }

        std::fs::write(&target_path, &content)?;

        let output = WriteOutput {
            kind: "write",
            ok: true,
            path: target_path.display().to_string(),
            bytes: content.len(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "✓ Wrote {} bytes to {}",
                    content.len(),
                    target_path.display()
                ),
            )),
        }
    }
}
