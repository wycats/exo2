use crate::argv_compiler;
use crate::cli_quote::shell_quote_arg;
use crate::command::command_spec::CommandSpec;
use crate::command::registry::default_registry;
use crate::command::router::Invocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExoCommandReference {
    path: Vec<String>,
    args: Vec<CommandReferenceArg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandReferenceArg {
    Positional(CommandReferenceValue),
    Flag(String),
    Option {
        name: String,
        value: CommandReferenceValue,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandReferenceValue {
    Literal(String),
    Placeholder { label: String, sample: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedExoRun {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReferenceError {
    pub command: String,
    pub diagnostics: Vec<String>,
}

impl std::fmt::Display for CommandReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.diagnostics.is_empty() {
            write!(f, "Command reference did not compile: {}", self.command)
        } else {
            write!(
                f,
                "Command reference did not compile: {} ({})",
                self.command,
                self.diagnostics.join("; ")
            )
        }
    }
}

impl std::error::Error for CommandReferenceError {}

impl ExoCommandReference {
    pub fn new(path: &[&str]) -> Self {
        Self {
            path: path.iter().map(|part| (*part).to_string()).collect(),
            args: Vec::new(),
        }
    }

    pub fn positional(mut self, value: impl Into<String>) -> Self {
        self.args.push(CommandReferenceArg::Positional(
            CommandReferenceValue::Literal(value.into()),
        ));
        self
    }

    pub fn positional_placeholder(
        mut self,
        label: impl Into<String>,
        sample: impl Into<String>,
    ) -> Self {
        self.args.push(CommandReferenceArg::Positional(
            CommandReferenceValue::Placeholder {
                label: label.into(),
                sample: sample.into(),
            },
        ));
        self
    }

    pub fn flag(mut self, name: impl Into<String>) -> Self {
        self.args.push(CommandReferenceArg::Flag(name.into()));
        self
    }

    pub fn option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.args.push(CommandReferenceArg::Option {
            name: name.into(),
            value: CommandReferenceValue::Literal(value.into()),
        });
        self
    }

    pub fn option_placeholder(
        mut self,
        name: impl Into<String>,
        label: impl Into<String>,
        sample: impl Into<String>,
    ) -> Self {
        self.args.push(CommandReferenceArg::Option {
            name: name.into(),
            value: CommandReferenceValue::Placeholder {
                label: label.into(),
                sample: sample.into(),
            },
        });
        self
    }

    pub fn render_cli(&self) -> String {
        let mut tokens = vec!["exo".to_string()];
        tokens.extend(self.render_tokens(RenderMode::Cli));
        join_command_tokens(&tokens)
    }

    pub fn render_exo_run(&self) -> RenderedExoRun {
        let mut args = Vec::new();
        let tokens = self.render_exo_run_tokens(&mut args);
        RenderedExoRun {
            command: join_command_tokens(&tokens),
            args,
        }
    }

    pub fn validate(&self, spec: &CommandSpec) -> Result<Invocation, CommandReferenceError> {
        let tokens = self.validation_tokens();
        let compiled = argv_compiler::compile_argv_v2(spec, &tokens);
        if let Some(invocation) = compiled.invocation {
            return Ok(invocation);
        }

        Err(CommandReferenceError {
            command: self.render_cli(),
            diagnostics: compiled
                .diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect(),
        })
    }

    pub fn validate_against_default_spec(&self) -> Result<Invocation, CommandReferenceError> {
        let spec = CommandSpec::from_registry(&default_registry());
        self.validate(&spec)
    }

    fn validation_tokens(&self) -> Vec<String> {
        let mut tokens = self.path.clone();
        for arg in &self.args {
            match arg {
                CommandReferenceArg::Positional(value) => tokens.push(value.sample().to_string()),
                CommandReferenceArg::Flag(name) => tokens.push(flag_token(name)),
                CommandReferenceArg::Option { name, value } => {
                    tokens.push(flag_token(name));
                    tokens.push(value.sample().to_string());
                }
            }
        }
        tokens
    }

    fn render_tokens(&self, mode: RenderMode) -> Vec<String> {
        let mut tokens = self.path.clone();
        for arg in &self.args {
            match arg {
                CommandReferenceArg::Positional(value) => tokens.push(value.render(mode)),
                CommandReferenceArg::Flag(name) => tokens.push(flag_token(name)),
                CommandReferenceArg::Option { name, value } => {
                    tokens.push(flag_token(name));
                    tokens.push(value.render(mode));
                }
            }
        }
        tokens
    }

    fn render_exo_run_tokens(&self, args: &mut Vec<String>) -> Vec<String> {
        let mut tokens = self.path.clone();
        for arg in &self.args {
            match arg {
                CommandReferenceArg::Positional(value) => tokens.push(value.render_exo_run(args)),
                CommandReferenceArg::Flag(name) => tokens.push(flag_token(name)),
                CommandReferenceArg::Option { name, value } => {
                    tokens.push(flag_token(name));
                    tokens.push(value.render_exo_run(args));
                }
            }
        }
        tokens
    }
}

impl CommandReferenceValue {
    fn sample(&self) -> &str {
        match self {
            Self::Literal(value) => value,
            Self::Placeholder { sample, .. } => sample,
        }
    }

    fn render(&self, mode: RenderMode) -> String {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Placeholder { label, .. } => match mode {
                RenderMode::Cli => format!("<{label}>"),
            },
        }
    }

    fn render_exo_run(&self, args: &mut Vec<String>) -> String {
        match self {
            Self::Literal(value) => value.clone(),
            Self::Placeholder { sample, .. } => {
                args.push(sample.clone());
                format!("${}", args.len())
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RenderMode {
    Cli,
}

fn flag_token(name: &str) -> String {
    if name.starts_with('-') {
        name.to_string()
    } else {
        format!("--{name}")
    }
}

fn join_command_tokens(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| {
            if is_rendered_placeholder(token) {
                token.clone()
            } else {
                shell_quote_arg(token)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_rendered_placeholder(token: &str) -> bool {
    (token.starts_with('<') && token.ends_with('>'))
        || token
            .strip_prefix('$')
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_text::parse_command_text;

    #[test]
    fn validates_known_command_reference() {
        let reference = ExoCommandReference::new(&["task", "complete"])
            .positional_placeholder("id", "sample-task")
            .option_placeholder("log", "summary", "finished the task");

        let invocation = reference
            .validate_against_default_spec()
            .expect("reference should validate");

        assert_eq!(invocation.namespace(), "task");
        assert_eq!(invocation.operation(), "complete");
    }

    #[test]
    fn rejects_unknown_flags() {
        let reference = ExoCommandReference::new(&["task", "complete"])
            .positional_placeholder("id", "sample-task")
            .option_placeholder("message", "summary", "finished the task");

        let error = reference
            .validate_against_default_spec()
            .expect_err("unknown flag should be rejected");

        assert!(
            error
                .diagnostics
                .iter()
                .any(|message| message.contains("Unknown flag '--message'")),
            "expected unknown --message diagnostic, got {error:?}"
        );
    }

    #[test]
    fn renders_cli_and_exo_run_edges_from_same_reference() {
        let reference = ExoCommandReference::new(&["goal", "complete"])
            .positional_placeholder("id", "sample-goal")
            .option_placeholder("log", "summary", "goal is complete");

        assert_eq!(
            reference.render_cli(),
            "exo goal complete <id> --log <summary>"
        );

        let rendered = reference.render_exo_run();
        assert_eq!(rendered.command, "goal complete $1 --log $2");
        assert_eq!(rendered.args, vec!["sample-goal", "goal is complete"]);

        let parsed =
            parse_command_text(&rendered.command, &rendered.args).expect("exo-run text parses");
        assert_eq!(
            parsed.tokens,
            vec![
                "goal",
                "complete",
                "sample-goal",
                "--log",
                "goal is complete"
            ]
        );
    }
}
