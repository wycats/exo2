//! Context namespace commands.
//!
//! - `context restore`: Restore context from a previous session (Pure)
//! - `context paths`: Show context file paths (Pure)
//! - `context snapshot`: Return full workspace state snapshot (Pure)
//! - `context validate-trace`: Validate a cached reactive trace (Pure)

use super::traits::{Command, CommandBox, CommandContext, CommandOutput, OutputFormat};
use crate::api::protocol::Effect;
use crate::context::{AgentContext, SqliteLoader};
use crate::project::{Project, StatePolicy};
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::{Context as _, Result as ExoResult};
use exosuit_storage::SqliteStateProvider;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Default steering for context commands.
fn default_context_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "Show status".to_string(),
        command: "exo status".to_string(),
        rationale: "Get current project status and orientation.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the context namespace
// ============================================================================

/// Context namespace command specification.
///
/// This enum is the authoritative definition of the context namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `ContextCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "context", description = "Context management commands")]
pub enum ContextCommands {
    #[exo(
        effect = "write",
        description = "Restore context from a previous session"
    )]
    Restore,

    #[exo(effect = "pure", description = "Show context file paths")]
    Paths,

    #[exo(
        effect = "pure",
        description = "Return full workspace state snapshot (plan, inbox, ideas) as JSON"
    )]
    Snapshot,

    #[exo(
        effect = "pure",
        operation = "validate-trace",
        description = "Validate a cached reactive trace against current revisions"
    )]
    ValidateTrace {
        #[exo(positional, description = "JSON-serialized trace to validate")]
        trace_json: String,
    },
}

impl ContextCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Restore => CommandBox::pure(ContextRestore::new()),
            Self::Paths => CommandBox::pure(ContextPaths::new()),
            Self::Snapshot => CommandBox::pure(ContextSnapshot),
            Self::ValidateTrace { trace_json } => {
                CommandBox::pure(ContextValidateTrace::new(trace_json))
            }
        })
    }
}

// ============================================================================
// context restore
// ============================================================================

/// Restore context from a previous session.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextRestore;

impl ContextRestore {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Serialize)]
struct ContextRestoreOutput {
    kind: &'static str,
    ok: bool,
    git_diff_stat: Option<String>,
    git_recent_commits: Option<String>,
}

impl Command for ContextRestore {
    fn namespace(&self) -> &'static str {
        "context"
    }

    fn operation(&self) -> &'static str {
        "restore"
    }

    fn description(&self) -> &'static str {
        "Restore context from a previous session"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_context_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        // Helper to run a git command and capture stdout as trimmed string.
        let run_git = |args: &[&str]| -> Option<String> {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&agent_ctx.root)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };

        let output = ContextRestoreOutput {
            kind: "context.restore",
            ok: true,
            git_diff_stat: run_git(&["diff", "--stat"]),
            git_recent_commits: run_git(&["log", "-n", "5", "--oneline"]),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = String::new();
                msg.push_str("Restoring context...\n");

                msg.push_str("\n--- Git Diff Summary ---\n");
                if let Some(stat) = &output.git_diff_stat {
                    msg.push_str(stat);
                    msg.push('\n');
                }

                msg.push_str("\n--- Recent Commits ---\n");
                if let Some(commits) = &output.git_recent_commits {
                    msg.push_str(commits);
                    msg.push('\n');
                }

                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// context paths
// ============================================================================

/// Show context paths.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextPaths;

impl ContextPaths {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Serialize)]
struct ContextPathsOutput {
    kind: &'static str,
    ok: bool,
    policy: &'static str,
    projection: ContextProjection,
    paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ContextPathInfo {
    pub(crate) policy: &'static str,
    pub(crate) projection: ContextProjection,
    pub(crate) paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ContextProjection {
    pub(crate) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) root: Option<String>,
}

pub(crate) fn context_path_info(root: &Path, project: Option<&Project>) -> ContextPathInfo {
    let policy = project.map_or("repo", |project| project.policy.as_str());
    let projection_dir = context_projection_dir(root, project);

    let projection = ContextProjection {
        kind: match (&projection_dir, project.map(|project| project.policy)) {
            (None, _) => "none",
            (Some(_), Some(StatePolicy::Sidecar)) => "sidecar_sql_projection",
            (Some(_), _) => "repo_sql_projection",
        },
        root: projection_dir
            .as_ref()
            .map(|dir| context_path_string(root, dir)),
    };

    let paths = projection_dir.map_or_else(BTreeMap::new, |dir| {
        BTreeMap::from([
            (
                "plan".to_string(),
                context_path_string(root, &dir.join("epochs.sql")),
            ),
            (
                "tasks".to_string(),
                context_path_string(root, &dir.join("tasks.sql")),
            ),
            (
                "ideas".to_string(),
                context_path_string(root, &dir.join("ideas.sql")),
            ),
            (
                "axioms".to_string(),
                context_path_string(root, &dir.join("axioms.sql")),
            ),
        ])
    });

    ContextPathInfo {
        policy,
        projection,
        paths,
    }
}

fn context_projection_dir(root: &Path, project: Option<&Project>) -> Option<PathBuf> {
    match project.map(|project| (project.policy, project.sidecar_projection_dir())) {
        Some((StatePolicy::Shadow, _)) => None,
        Some((StatePolicy::Sidecar, sidecar_dir)) => sidecar_dir,
        Some((StatePolicy::Repo, _)) | None => Some(root.join("docs/agent-context")),
    }
}

fn context_path_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

impl Command for ContextPaths {
    fn namespace(&self) -> &'static str {
        "context"
    }

    fn operation(&self) -> &'static str {
        "paths"
    }

    fn description(&self) -> &'static str {
        "Show context file paths"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_context_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let info = context_path_info(ctx.root, ctx.project);

        let output = ContextPathsOutput {
            kind: "context.paths",
            ok: true,
            policy: info.policy,
            projection: info.projection,
            paths: info.paths,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let formatted = serde_json::to_string_pretty(&output)?;
                Ok(CommandOutput::new(output, formatted))
            }
        }
    }
}

// ============================================================================
// context snapshot (Pure)
// ============================================================================

/// Return full workspace state snapshot (plan, inbox, ideas) as JSON.
///
/// This is the primary data source for the VS Code extension's sidebar.
/// It returns the full plan tree, all inbox items, and all ideas in the
/// shapes that the extension's Zod schemas expect.
///
/// The extension calls this once on activation and after mutations to
/// refresh its reactive roots.
#[derive(Debug, Clone, Copy)]
pub struct ContextSnapshot;

impl Command for ContextSnapshot {
    fn namespace(&self) -> &'static str {
        "context"
    }

    fn operation(&self) -> &'static str {
        "snapshot"
    }

    fn description(&self) -> &'static str {
        "Return full workspace state snapshot (plan, inbox, ideas) as JSON"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_context_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;

        // Serialize the full plan tree
        let plan =
            serde_json::to_value(&agent_ctx.plan).context("Failed to serialize plan state")?;

        // Load inbox and ideas from SQLite
        let (inbox, ideas) = {
            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path).context("Failed to open SQLite database")?;

            let inbox_items = loader
                .load_inbox()
                .context("Failed to load inbox from SQLite")?;
            let inbox_file = crate::inbox::InboxFile { items: inbox_items };
            let inbox_json =
                serde_json::to_value(&inbox_file).context("Failed to serialize inbox")?;

            let idea_items = loader
                .load_ideas()
                .context("Failed to load ideas from SQLite")?;
            let ideas_file = crate::idea::IdeasFile {
                meta: None,
                ideas: idea_items,
            };
            let ideas_json =
                serde_json::to_value(&ideas_file).context("Failed to serialize ideas")?;

            (inbox_json, ideas_json)
        };

        let snapshot = serde_json::json!({
            "plan": plan,
            "inbox": inbox,
            "ideas": ideas,
        });

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(snapshot)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&snapshot)?;
                Ok(CommandOutput::new(snapshot, pretty))
            }
        }
    }
}

// ============================================================================
// context validate-trace
// ============================================================================

/// Validate a cached reactive trace against current revisions.
///
/// Takes a JSON-serialized `Trace` (the opaque token returned alongside
/// snapshot data) and checks each `(cell, revision)` entry against the
/// current `RevisionStore`. Returns `{ valid: true }` if all revisions
/// still match, `{ valid: false }` otherwise.
#[derive(Debug, Clone)]
pub struct ContextValidateTrace {
    trace_json: String,
}

impl ContextValidateTrace {
    pub const fn new(trace_json: String) -> Self {
        Self { trace_json }
    }
}

#[derive(Debug, Serialize)]
struct ValidateTraceOutput {
    kind: &'static str,
    valid: bool,
}

impl Command for ContextValidateTrace {
    fn namespace(&self) -> &'static str {
        "context"
    }

    fn operation(&self) -> &'static str {
        "validate-trace"
    }

    fn description(&self) -> &'static str {
        "Validate a cached reactive trace against current revisions"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let trace: exosuit_storage::Trace =
            serde_json::from_str(&self.trace_json).context("Failed to deserialize trace JSON")?;

        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path).context("Failed to open database")?;
        let db = loader.database();
        let mut provider = SqliteStateProvider::new(db.connection(), db.revision_store());
        let valid = trace.validate(&mut provider);

        let output = ValidateTraceOutput {
            kind: "context.validate-trace",
            valid,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = if valid {
                    "Trace is valid"
                } else {
                    "Trace is invalid"
                };
                Ok(CommandOutput::new(output, msg))
            }
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
    fn test_context_restore_metadata() {
        let cmd = ContextRestore::new();
        assert_eq!(cmd.namespace(), "context");
        assert_eq!(cmd.operation(), "restore");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_context_paths_metadata() {
        let cmd = ContextPaths::new();
        assert_eq!(cmd.namespace(), "context");
        assert_eq!(cmd.operation(), "paths");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_context_snapshot_metadata() {
        let cmd = ContextSnapshot;
        assert_eq!(cmd.namespace(), "context");
        assert_eq!(cmd.operation(), "snapshot");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_context_validate_trace_metadata() {
        let cmd = ContextValidateTrace::new("{}".to_string());
        assert_eq!(cmd.namespace(), "context");
        assert_eq!(cmd.operation(), "validate-trace");
        assert_eq!(cmd.effect(), Effect::Pure);
    }
}
