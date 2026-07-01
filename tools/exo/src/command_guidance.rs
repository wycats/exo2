use crate::command::command_spec::CommandSpec;
use crate::command::registry::default_registry;
use crate::command_text::{CommandTextIntent, parse_command_text};
use crate::router::compile_argv;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGuidanceKind {
    /// A terminal command rendered with the leading `exo` executable name.
    ExoCli,
    /// The command text accepted by the `exo-run` MCP tool.
    ExoRun,
    /// Human action text stored in a command-shaped field.
    HumanAction,
    /// External shell command guidance that is intentionally outside CommandSpec.
    ExternalShell,
    /// Exo-authored guidance for a command surface not yet represented in CommandSpec.
    LegacyExoSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandGuidance {
    pub id: &'static str,
    pub surface: &'static str,
    pub command: &'static str,
    pub kind: CommandGuidanceKind,
}

impl CommandGuidance {
    pub const fn exo_cli(id: &'static str, surface: &'static str, command: &'static str) -> Self {
        Self {
            id,
            surface,
            command,
            kind: CommandGuidanceKind::ExoCli,
        }
    }

    pub const fn exo_run(id: &'static str, surface: &'static str, command: &'static str) -> Self {
        Self {
            id,
            surface,
            command,
            kind: CommandGuidanceKind::ExoRun,
        }
    }

    pub const fn human_action(
        id: &'static str,
        surface: &'static str,
        command: &'static str,
    ) -> Self {
        Self {
            id,
            surface,
            command,
            kind: CommandGuidanceKind::HumanAction,
        }
    }

    pub const fn external_shell(
        id: &'static str,
        surface: &'static str,
        command: &'static str,
    ) -> Self {
        Self {
            id,
            surface,
            command,
            kind: CommandGuidanceKind::ExternalShell,
        }
    }

    pub const fn legacy_exo_surface(
        id: &'static str,
        surface: &'static str,
        command: &'static str,
    ) -> Self {
        Self {
            id,
            surface,
            command,
            kind: CommandGuidanceKind::LegacyExoSurface,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandGuidanceStatus {
    Valid {
        command_text: String,
        tokens: Vec<String>,
        path: String,
    },
    Skipped {
        reason: String,
    },
    Invalid {
        command_text: String,
        message: String,
    },
}

impl CommandGuidanceStatus {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }

    #[must_use]
    pub const fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }
}

#[must_use]
pub fn representative_command_guidance_inventory() -> Vec<CommandGuidance> {
    vec![
        CommandGuidance::exo_cli("steering.status", "runtime steering", "exo status"),
        CommandGuidance::exo_cli(
            "steering.phase-status",
            "runtime steering",
            "exo phase status",
        ),
        CommandGuidance::exo_cli(
            "steering.plan-review",
            "runtime steering",
            "exo plan review",
        ),
        CommandGuidance::exo_cli(
            "steering.phase-start",
            "runtime steering",
            "exo phase start <phase-id>",
        ),
        CommandGuidance::exo_cli(
            "steering.task-add",
            "runtime steering",
            "exo task add <title> --id <id>",
        ),
        CommandGuidance::exo_cli(
            "steering.task-complete",
            "runtime steering",
            "exo task complete <id>",
        ),
        CommandGuidance::exo_cli(
            "steering.goal-complete-log",
            "runtime steering",
            "exo goal complete <id> --log <summary>",
        ),
        CommandGuidance::exo_cli(
            "steering.phase-finish",
            "runtime steering",
            "exo phase finish",
        ),
        CommandGuidance::exo_cli(
            "steering.phase-finish-message",
            "runtime steering",
            "exo phase finish -m \"...\"",
        ),
        CommandGuidance::exo_cli(
            "steering.rfc-show",
            "runtime steering",
            "exo rfc show 10194",
        ),
        CommandGuidance::exo_cli(
            "steering.sidecar-repo-status",
            "runtime steering",
            "exo sidecar repo status",
        ),
        CommandGuidance::exo_cli(
            "steering.sidecar-bootstrap",
            "runtime steering",
            "exo sidecar bootstrap --discover",
        ),
        CommandGuidance::exo_cli("steering.verify-run", "runtime steering", "exo verify run"),
        CommandGuidance::exo_run("plugin.status", "packaged exo skill", "status"),
        CommandGuidance::exo_run("plugin.task-list", "packaged exo skill", "task list"),
        CommandGuidance::exo_run(
            "plugin.task-complete-log",
            "packaged exo skill",
            "task complete <id> --log $1",
        ),
        CommandGuidance::human_action(
            "confirmation.ask-human",
            "completion confirmation",
            "Ask the human to confirm that the work is complete.",
        ),
        CommandGuidance::human_action(
            "confirmation.describe-outcome",
            "completion confirmation",
            "Describe what is complete and ask for confirmation.",
        ),
        CommandGuidance::external_shell("phase.git-status", "phase finish recovery", "git status"),
    ]
}

#[must_use]
pub fn validate_command_guidance(guidance: &CommandGuidance) -> CommandGuidanceStatus {
    match guidance.kind {
        CommandGuidanceKind::HumanAction => {
            return CommandGuidanceStatus::Skipped {
                reason: "human action text, not Exo command guidance".to_string(),
            };
        }
        CommandGuidanceKind::ExternalShell => {
            return CommandGuidanceStatus::Skipped {
                reason: "external shell command outside CommandSpec".to_string(),
            };
        }
        CommandGuidanceKind::LegacyExoSurface => {
            return CommandGuidanceStatus::Skipped {
                reason: "known Exo-authored surface not yet represented in CommandSpec".to_string(),
            };
        }
        CommandGuidanceKind::ExoCli | CommandGuidanceKind::ExoRun => {}
    }

    let Some(command_text) = command_text_for_validation(guidance) else {
        return CommandGuidanceStatus::Invalid {
            command_text: guidance.command.to_string(),
            message: "Exo CLI guidance must start with `exo `".to_string(),
        };
    };

    if guidance.kind == CommandGuidanceKind::ExoCli
        && contains_numbered_shell_placeholder(&command_text)
    {
        return CommandGuidanceStatus::Invalid {
            command_text,
            message: "Exo CLI guidance must be copyable terminal text; use `<placeholder>` instead of `$n` arguments".to_string(),
        };
    }

    let command_text = substitute_guidance_placeholders(&command_text);
    let args = placeholder_args_for(&command_text);
    let parsed = match parse_command_text(&command_text, &args) {
        Ok(parsed) => parsed,
        Err(err) => {
            return CommandGuidanceStatus::Invalid {
                command_text,
                message: err,
            };
        }
    };

    let tokens = match parsed.intent {
        CommandTextIntent::Call => parsed.tokens,
        CommandTextIntent::Help { target } => target,
    };

    let spec = CommandSpec::from_registry(&default_registry());
    let compiled = compile_argv(&spec, &tokens);
    let Some(invocation) = compiled.invocation else {
        let message = if compiled.diagnostics.is_empty() {
            "Command did not compile and produced no diagnostics".to_string()
        } else {
            compiled
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>()
                .join("; ")
        };
        return CommandGuidanceStatus::Invalid {
            command_text,
            message,
        };
    };

    CommandGuidanceStatus::Valid {
        command_text,
        tokens,
        path: if invocation.namespace().is_empty() {
            invocation.operation().to_string()
        } else {
            format!("{} {}", invocation.namespace(), invocation.operation())
        },
    }
}

fn command_text_for_validation(guidance: &CommandGuidance) -> Option<String> {
    match guidance.kind {
        CommandGuidanceKind::ExoCli => guidance
            .command
            .trim()
            .strip_prefix("exo ")
            .map(str::to_string),
        CommandGuidanceKind::ExoRun => Some(guidance.command.trim().to_string()),
        CommandGuidanceKind::HumanAction
        | CommandGuidanceKind::ExternalShell
        | CommandGuidanceKind::LegacyExoSurface => None,
    }
}

fn placeholder_args_for(command_text: &str) -> Vec<String> {
    let max_placeholder = command_text
        .split_whitespace()
        .filter_map(|token| token.strip_prefix('$'))
        .filter_map(|rest| rest.parse::<usize>().ok())
        .max()
        .unwrap_or(0);

    (1..=max_placeholder)
        .map(|index| format!("sample value {index}"))
        .collect()
}

fn contains_numbered_shell_placeholder(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek().is_some_and(char::is_ascii_digit) {
            return true;
        }
    }
    false
}

fn substitute_guidance_placeholders(command: &str) -> String {
    let mut output = String::new();
    let mut rest = command;

    while let Some(start) = rest.find('<') {
        let (before, after_start) = rest.split_at(start);
        output.push_str(before);
        let Some(end) = after_start.find('>') else {
            output.push_str(after_start);
            return output;
        };

        let name = &after_start[1..end];
        output.push_str(sample_value_for_placeholder(name));
        rest = &after_start[end + 1..];
    }

    output.push_str(rest);
    output
}

fn sample_value_for_placeholder(name: &str) -> &'static str {
    match name {
        "branch" => "sample-branch",
        "github-url" | "url" => "https://example.invalid/exo.git",
        "id" => "sample-id",
        "message" | "msg" => "sample-message",
        "name" => "sample-name",
        "path" | "registry-file" | "root" | "test-file" => "/tmp/exo-sample",
        "phase-id" => "sample-phase",
        "remote" => "origin",
        "rfc-id" => "10194",
        "sidecar-root" => "/tmp/exo-sidecar",
        "summary" => "sample-summary",
        "task-id" => "sample-task",
        "title" => "sample-title",
        _ => "sample",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_known_placeholders_before_parsing() {
        assert_eq!(
            substitute_guidance_placeholders("task add <title> --id <id>"),
            "task add sample-title --id sample-id"
        );
    }
}
