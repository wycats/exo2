//! Commit namespace commands.
//!
//! Provides git commit operations:
//! - `commit status`: Check working directory status
//! - `commit`: Stage all changes and commit

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;
use std::process::Command as ProcessCommand;

// ============================================================================
// ExoSpec definition — single source of truth for the commit namespace
// ============================================================================

/// Commit namespace command specification.
///
/// This enum is the authoritative definition of the commit namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `CommitCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "commit", description = "Git commit operations")]
pub enum CommitCommands {
    #[exo(effect = "pure", description = "Check git working directory status")]
    Status,

    #[exo(
        effect = "write",
        operation = "create",
        description = "Stage all changes and create a git commit"
    )]
    Create {
        #[exo(long, short = 'm', description = "The commit message")]
        message: String,
    },
}

impl CommitCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Status => CommandBox::pure(CommitStatus::new()),
            Self::Create { message } => CommandBox::mutable(Commit::new(message)),
        })
    }
}

// ============================================================================
// commit status
// ============================================================================

/// Check git working directory status.
#[derive(Debug, Clone, Copy)]
pub struct CommitStatus;

impl CommitStatus {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for CommitStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
struct FileStatus {
    path: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct CommitStatusOutput {
    kind: &'static str,
    ok: bool,
    clean: bool,
    files: Vec<FileStatus>,
}

impl Command for CommitStatus {
    fn namespace(&self) -> &'static str {
        "commit"
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn description(&self) -> &'static str {
        "Check git working directory status"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Commit changes".to_string(),
                command: "exo commit \"<message>\"".to_string(),
                rationale: "If there are changes, commit them.".to_string(),
                intent: WorkIntent::Ship,
                confidence: Some(0.7),
            },
            SuggestedAction {
                label: "View diff".to_string(),
                command: "git diff".to_string(),
                rationale: "Review changes before committing.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let output = ProcessCommand::new("git")
            .args(["status", "--porcelain"])
            .current_dir(ctx.root)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<FileStatus> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let status_code = &line[..2];
                let path = line[3..].to_string();
                let status = match status_code.trim() {
                    "M" | " M" | "MM" => "modified",
                    "A" | " A" => "added",
                    "D" | " D" => "deleted",
                    "R" => "renamed",
                    "??" => "untracked",
                    _ => "unknown",
                }
                .to_string();
                FileStatus { path, status }
            })
            .collect();

        let clean = files.is_empty();

        let result = CommitStatusOutput {
            kind: "commit.status",
            ok: true,
            clean,
            files,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => {
                if clean {
                    Ok(CommandOutput::new(
                        result,
                        "Working directory is clean.".to_string(),
                    ))
                } else {
                    let mut msg = String::from("Changes:\n");
                    for file in &result.files {
                        msg.push_str(&format!("  {} {}\n", file.status, file.path));
                    }
                    Ok(CommandOutput::new(result, msg.trim_end().to_string()))
                }
            }
        }
    }
}

// ============================================================================
// commit (create a commit)
// ============================================================================

/// Stage all changes and create a git commit.
#[derive(Debug, Clone)]
pub struct Commit {
    pub message: String,
}

impl Commit {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct CommitOutput {
    kind: &'static str,
    ok: bool,
    hash: Option<String>,
    files_changed: usize,
}

impl Command for Commit {
    fn namespace(&self) -> &'static str {
        "commit"
    }

    fn operation(&self) -> &'static str {
        "create"
    }

    fn description(&self) -> &'static str {
        "Stage all changes and create a git commit"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Check status".to_string(),
                command: "exo commit status".to_string(),
                rationale: "Verify what was committed.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "View log".to_string(),
                command: "git log -1".to_string(),
                rationale: "See the commit that was created.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ]
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("Commit should be dispatched via execute_mut")
    }
}

impl MutableCommand for Commit {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        // First check if there are changes to commit
        let status_output = ProcessCommand::new("git")
            .args(["status", "--porcelain"])
            .current_dir(ctx.root)
            .output()?;

        let status_stdout = String::from_utf8_lossy(&status_output.stdout);
        let files_changed = status_stdout.lines().filter(|l| !l.is_empty()).count();

        if files_changed == 0 {
            let result = CommitOutput {
                kind: "commit.create",
                ok: true,
                hash: None,
                files_changed: 0,
            };
            return match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(result)),
                OutputFormat::Human => {
                    Ok(CommandOutput::new(result, "Nothing to commit.".to_string()))
                }
            };
        }

        // Stage all changes
        let add_output = ProcessCommand::new("git")
            .args(["add", "-A"])
            .current_dir(ctx.root)
            .output()?;

        if !add_output.status.success() {
            anyhow::bail!(
                "Failed to stage changes: {}",
                String::from_utf8_lossy(&add_output.stderr)
            );
        }

        // Create the commit
        let commit_output = ProcessCommand::new("git")
            .args(["commit", "-m", &self.message])
            .current_dir(ctx.root)
            .output()?;

        if !commit_output.status.success() {
            anyhow::bail!(
                "Failed to create commit: {}",
                String::from_utf8_lossy(&commit_output.stderr)
            );
        }

        // Get the commit hash
        let hash_output = ProcessCommand::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(ctx.root)
            .output()?;

        let hash = String::from_utf8_lossy(&hash_output.stdout)
            .trim()
            .to_string();

        let result = CommitOutput {
            kind: "commit.create",
            ok: true,
            hash: Some(hash.clone()),
            files_changed,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => Ok(CommandOutput::new(
                result,
                format!("Created commit {hash} ({files_changed} file(s) changed)"),
            )),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_status_metadata() {
        let cmd = CommitStatus::new();
        assert_eq!(cmd.namespace(), "commit");
        assert_eq!(cmd.operation(), "status");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_commit_metadata() {
        let cmd = Commit::new("test message");
        assert_eq!(cmd.namespace(), "commit");
        assert_eq!(cmd.operation(), "create");
        assert_eq!(cmd.effect(), Effect::Write);
    }
}
