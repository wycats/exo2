//! TOML namespace commands.
//!
//! - `toml read`: Read a TOML file (Pure)
//! - `toml write`: Write to a TOML file (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::structured_io;
use anyhow::Result as ExoResult;
use serde::Serialize;
use std::path::PathBuf;

// ============================================================================
// ExoSpec definition — single source of truth for the toml namespace
// ============================================================================

/// Toml namespace command specification.
///
/// This enum is the authoritative definition of the toml namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `TomlCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "toml", description = "TOML file operations")]
pub enum TomlCommands {
    #[exo(effect = "pure", description = "Read a value from a TOML file")]
    Read {
        #[exo(positional, description = "Path to the TOML file")]
        path: String,
        #[exo(
            long,
            optional,
            description = "Key to read (reads entire file if omitted)"
        )]
        key: Option<String>,
    },

    #[exo(effect = "write", description = "Write a value to a TOML file")]
    Write {
        #[exo(positional, description = "Path to the TOML file")]
        path: String,
        #[exo(positional, description = "Key to write")]
        key: String,
        #[exo(positional, description = "Value to write")]
        value: String,
    },
}

impl TomlCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Read { path, key } => CommandBox::pure(TomlRead::new(path, key)),
            Self::Write { path, key, value } => {
                CommandBox::mutable(TomlWrite::new(path, key, value))
            }
        })
    }
}

// ============================================================================
// toml read
// ============================================================================

/// Read a value from a TOML file.
#[derive(Debug, Clone)]
pub struct TomlRead {
    pub path: PathBuf,
    pub key: Option<String>,
}

impl TomlRead {
    pub fn new(path: impl Into<PathBuf>, key: Option<String>) -> Self {
        Self {
            path: path.into(),
            key,
        }
    }
}

impl Command for TomlRead {
    fn namespace(&self) -> &'static str {
        "toml"
    }

    fn operation(&self) -> &'static str {
        "read"
    }

    fn description(&self) -> &'static str {
        "Read a value from a TOML file"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let full_path = ctx.root.join(&self.path);

        match ctx.format {
            OutputFormat::Json => {
                let value = structured_io::read_toml_as_json(&full_path, self.key.as_deref())?;
                Ok(CommandOutput::data(value))
            }
            OutputFormat::Human => {
                let value = structured_io::read_toml(&full_path, self.key.as_deref())?;
                Ok(CommandOutput::message(value))
            }
        }
    }
}

// ============================================================================
// toml write
// ============================================================================

/// Write a value to a TOML file.
#[derive(Debug, Clone)]
pub struct TomlWrite {
    pub path: PathBuf,
    pub key: String,
    pub value: String,
}

impl TomlWrite {
    pub fn new(path: impl Into<PathBuf>, key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TomlWriteOutput {
    kind: &'static str,
    ok: bool,
}

impl Command for TomlWrite {
    fn namespace(&self) -> &'static str {
        "toml"
    }

    fn operation(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> &'static str {
        "Write a value to a TOML file"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("TomlWrite should be dispatched via execute_mut")
    }
}

impl MutableCommand for TomlWrite {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let full_path = ctx.root.join(&self.path);
        structured_io::write_toml(&full_path, &self.key, &self.value)?;

        let output = TomlWriteOutput {
            kind: "toml.write",
            ok: true,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(output, "Updated TOML file.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_read_metadata() {
        let cmd = TomlRead::new("test.toml", None);
        assert_eq!(cmd.namespace(), "toml");
        assert_eq!(cmd.operation(), "read");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_toml_write_metadata() {
        let cmd = TomlWrite::new("test.toml", "key", "value");
        assert_eq!(cmd.namespace(), "toml");
        assert_eq!(cmd.operation(), "write");
        assert_eq!(cmd.effect(), Effect::Write);
    }
}
