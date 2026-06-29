//! Run namespace commands.
//!
//! - `run tasks`: List available tasks from exosuit.toml (Pure)
//! - `run task`: Execute a task defined in exosuit.toml (Exec)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::run::load_config;
use anyhow::Result as ExoResult;
use serde_json::{Value as JsonValue, json};

pub const TASK_DIRECT_MODE_ENV: &str = "EXO_TASK_DIRECT_MODE";
pub const TASK_PARENT_DAEMON_PID_ENV: &str = "EXO_TASK_PARENT_DAEMON_PID";

const DEFAULT_LIMIT: usize = 20;

fn paginate_items<T, F>(
    items: &[T],
    cursor: Option<&str>,
    limit: Option<usize>,
    map_fn: F,
) -> JsonValue
where
    F: Fn(&T) -> JsonValue,
{
    let start = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let end = (start + limit).min(items.len());
    let has_more = end < items.len();
    let next_cursor = if has_more {
        Some(end.to_string())
    } else {
        None
    };

    let mapped: Vec<JsonValue> = (start..end).map(|i| map_fn(&items[i])).collect();

    json!({
        "items": mapped,
        "next_cursor": next_cursor,
        "has_more": has_more,
    })
}

// ============================================================================
// ExoSpec definition — single source of truth for the run namespace
// ============================================================================

/// Run namespace command specification.
///
/// This enum is the authoritative definition of the run namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `RunCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "run", description = "Task execution commands")]
pub enum RunCommands {
    #[exo(effect = "pure", description = "List available tasks")]
    Tasks {
        #[exo(long, optional, description = "Pagination cursor")]
        cursor: Option<String>,
        #[exo(long, optional, description = "Pagination limit")]
        limit: Option<i64>,
    },

    #[exo(effect = "exec", description = "Execute a task")]
    Task {
        #[exo(positional, description = "Task label")]
        id: String,
    },
}

impl RunCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Tasks { cursor, limit } => CommandBox::pure(RunTasksCommand::new(
                cursor,
                limit.and_then(|value| usize::try_from(value).ok()),
            )),
            Self::Task { id } => CommandBox::mutable(RunTaskCommand::new(id)),
        })
    }
}

// ============================================================================
// run tasks
// ============================================================================

/// List available tasks.
#[derive(Debug, Clone, Default)]
pub struct RunTasksCommand {
    cursor: Option<String>,
    limit: Option<usize>,
}

impl RunTasksCommand {
    pub const fn new(cursor: Option<String>, limit: Option<usize>) -> Self {
        Self { cursor, limit }
    }
}

impl Command for RunTasksCommand {
    fn namespace(&self) -> &'static str {
        "run"
    }

    fn operation(&self) -> &'static str {
        "tasks"
    }

    fn description(&self) -> &'static str {
        "List available tasks"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let config = load_config(ctx.root)?;
        let mut task_names: Vec<String> = config.tasks.keys().cloned().collect();
        task_names.sort();

        let result = paginate_items(&task_names, self.cursor.as_deref(), self.limit, |name| {
            json!(name)
        });

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&result)?;
                Ok(CommandOutput::new(result, pretty))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::command_spec::HasExoSpec;

    #[test]
    fn run_tasks_is_pure() {
        assert_eq!(RunTasksCommand::default().effect(), Effect::Pure);
    }

    #[test]
    fn run_task_is_exec() {
        assert_eq!(
            RunTaskCommand::new("build-ext".to_string()).effect(),
            Effect::Exec
        );
    }

    #[test]
    fn run_task_spec_is_exec() {
        let spec = RunCommands::spec();
        let task = spec
            .operations
            .get("task")
            .expect("run task operation should exist");
        assert_eq!(task.effect, Effect::Exec);
    }
}

// ============================================================================
// run task
// ============================================================================

/// Execute a task.
#[derive(Debug, Clone)]
pub struct RunTaskCommand {
    id: String,
}

impl RunTaskCommand {
    pub const fn new(id: String) -> Self {
        Self { id }
    }
}

impl Command for RunTaskCommand {
    fn namespace(&self) -> &'static str {
        "run"
    }

    fn operation(&self) -> &'static str {
        "task"
    }

    fn description(&self) -> &'static str {
        "Execute a task"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let mut ctx = MutableCommandContext {
            root: ctx.root,
            project: ctx.project,
            format: ctx.format,
            agent_id: ctx.agent_id.clone(),
            workflow_confirmation: ctx.workflow_confirmation.clone(),
        };
        self.execute_mut(&mut ctx)
    }
}

impl MutableCommand for RunTaskCommand {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let config = load_config(ctx.root)?;

        let task = config
            .tasks
            .get(&self.id)
            .ok_or_else(|| anyhow::anyhow!("Task '{}' not found in exosuit.toml", self.id))?;

        let cwd = if task.cwd == "root" {
            ctx.root.to_path_buf()
        } else {
            ctx.root.join(&task.cwd)
        };

        let mut command = task_shell_command(&task.cmd);
        let output = command
            .current_dir(cwd)
            .env(TASK_DIRECT_MODE_ENV, "1")
            .env(TASK_PARENT_DAEMON_PID_ENV, std::process::id().to_string())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let result = json!({
            "task_id": self.id,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        });

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(result)),
            OutputFormat::Human => {
                let pretty = serde_json::to_string_pretty(&result)?;
                Ok(CommandOutput::new(result, pretty))
            }
        }
    }
}

fn task_shell_command(command: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut shell = std::process::Command::new("cmd.exe");
        shell.arg("/C").arg(command);
        shell
    }

    #[cfg(not(windows))]
    {
        let mut shell = std::process::Command::new("sh");
        shell.arg("-c").arg(command);
        shell
    }
}
