use crate::ExoResult;
use crate::api::protocol::ErrorCode;
use crate::cli_quote::shell_quote_arg;
use crate::command_reference::ExoCommandReference;
use crate::failure::ExoFailure;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Context;
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const OUTPUT_TAIL_CHARS: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub runner: String,
}

struct VerifyRunner {
    name: &'static str,
    program: &'static str,
    args: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct VerifyFailureDetails {
    runner: String,
    command: String,
    program: String,
    args: Vec<String>,
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    setup_hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mutating_lane_warning: Option<String>,
}

impl VerifyRunner {
    fn command(&self, root: &Path) -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/d", "/c", self.program])
                .args(&self.args)
                .current_dir(root);
            cmd
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new(self.program);
            cmd.args(&self.args).current_dir(root);
            cmd
        }
    }

    fn command_line(&self) -> String {
        std::iter::once(self.program)
            .chain(self.args.iter().copied())
            .map(shell_quote_arg)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn args_json(&self) -> Vec<String> {
        self.args.iter().map(|arg| (*arg).to_string()).collect()
    }
}

fn verify_failure(
    runner: &VerifyRunner,
    exit_code: Option<i32>,
    output: Option<&Output>,
) -> anyhow::Error {
    let stdout_tail = output.and_then(|out| output_tail(&out.stdout));
    let stderr_tail = output.and_then(|out| output_tail(&out.stderr));
    let setup_hints = setup_hints(stdout_tail.as_deref(), stderr_tail.as_deref());
    let mutating_lane_warning = mutating_lane_warning(runner);
    let command = runner.command_line();
    let message = format!(
        "Verification failed via {} ({}).",
        runner.name,
        exit_code_label(exit_code)
    );

    let details = VerifyFailureDetails {
        runner: runner.name.to_string(),
        command: command.clone(),
        program: runner.program.to_string(),
        args: runner.args_json(),
        exit_code,
        stdout_tail,
        stderr_tail,
        setup_hints,
        mutating_lane_warning,
    };
    let details_json = serde_json::to_value(&details).unwrap_or_else(|_| {
        serde_json::json!({
            "runner": runner.name,
            "command": command,
            "exit_code": exit_code,
        })
    });

    anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::Internal,
            message,
            ExoFailure::orienting_steering(vec![
                SuggestedAction::exo(
                    "Re-run verification (human output)",
                    ExoCommandReference::new(&["verify", "run"]),
                    "Re-run the same verification path in a terminal if you need the full live output.",
                    WorkIntent::Execute,
                    Some(0.8),
                ),
                SuggestedAction::external_shell(
                    "Inspect working tree",
                    "git status --porcelain",
                    "Check for local changes that could impact verification results.",
                    WorkIntent::Orient,
                    Some(0.6),
                ),
            ]),
        )
        .with_details(details_json),
    )
}

pub fn run_verify(root: &Path, json_mode: bool) -> ExoResult<VerifyReport> {
    let runner = select_verify_runner(root);

    if json_mode {
        // Capture output so stdout stays JSON-clean.
        let mut cmd = runner.command(root);

        let output = cmd
            .output()
            .context(format!("Failed to execute {}", runner.name))?;

        if !output.status.success() {
            return Err(verify_failure(&runner, output.status.code(), Some(&output)));
        }

        return Ok(VerifyReport {
            runner: runner.name.to_string(),
        });
    }

    // Human mode: stream output directly.
    let mut cmd = runner.command(root);

    let status = cmd
        .status()
        .context(format!("Failed to execute {}", runner.name))?;

    if !status.success() {
        return Err(verify_failure(&runner, status.code(), None));
    }

    Ok(VerifyReport {
        runner: runner.name.to_string(),
    })
}

fn exit_code_label(exit_code: Option<i32>) -> String {
    exit_code.map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit {code}"),
    )
}

fn output_tail(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_end();
    if trimmed.chars().count() <= OUTPUT_TAIL_CHARS {
        return Some(trimmed.to_string());
    }

    let tail: String = trimmed
        .chars()
        .rev()
        .take(OUTPUT_TAIL_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(format!(
        "[truncated to last {OUTPUT_TAIL_CHARS} chars]\n{tail}"
    ))
}

fn setup_hints(stdout_tail: Option<&str>, stderr_tail: Option<&str>) -> Vec<String> {
    let combined = format!(
        "{}\n{}",
        stdout_tail.unwrap_or(""),
        stderr_tail.unwrap_or("")
    );
    let lower = combined.to_ascii_lowercase();
    let mut hints = Vec::new();

    if lower.contains("svelte-check")
        || lower.contains("tsc: command not found")
        || lower.contains("cannot find module")
        || lower.contains("node_modules")
    {
        hints.push(
            "Install JavaScript dependencies with `pnpm install --frozen-lockfile` before rerunning verification."
                .to_string(),
        );
    }

    if lower.contains("exosuit_ulid")
        || lower.contains("@exosuit/rtd")
        || (lower.contains("wasm") && lower.contains("not found"))
    {
        hints.push(
            "Regenerate Exosuit runtime artifacts before rerunning verification.".to_string(),
        );
    }

    hints
}

fn mutating_lane_warning(runner: &VerifyRunner) -> Option<String> {
    (runner.name == "exo validate dev").then(|| {
        "Verify uses the Exohook dev lane; inspect the working tree after checks that may apply fixes."
            .to_string()
    })
}

fn select_verify_runner(root: &Path) -> VerifyRunner {
    if root.join(".config/exo/hooks.toml").exists() {
        return VerifyRunner {
            name: "exo validate dev",
            program: "exo",
            args: vec!["validate", "dev", "--color", "never"],
        };
    }

    let check_script = root.join("scripts/check");

    if check_script.exists() && !cfg!(windows) {
        VerifyRunner {
            name: "scripts/check",
            program: "./scripts/check",
            args: vec![],
        }
    } else if command_succeeds(root, "pnpm", &["--version"]) {
        VerifyRunner {
            name: "pnpm test",
            program: "pnpm",
            args: vec!["test"],
        }
    } else {
        VerifyRunner {
            name: "npm test",
            program: "npm",
            args: vec!["test"],
        }
    }
}

fn command_succeeds(root: &Path, program: &str, args: &[&str]) -> bool {
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/d", "/c", program]);
        cmd
    };

    #[cfg(not(windows))]
    let mut cmd = Command::new(program);

    cmd.args(args)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_hints_identify_missing_javascript_dependencies() {
        let hints = setup_hints(None, Some("sh: svelte-check: command not found"));
        assert!(
            hints
                .iter()
                .any(|hint| hint.contains("pnpm install --frozen-lockfile")),
            "{hints:?}"
        );
    }

    #[test]
    fn output_tail_truncates_large_output() {
        let large = "a".repeat(OUTPUT_TAIL_CHARS + 10);
        let tail = output_tail(large.as_bytes()).expect("tail");
        assert!(tail.starts_with("[truncated to last "));
        let payload = tail.split_once('\n').expect("truncation marker").1;
        assert_eq!(
            payload.chars().filter(|c| *c == 'a').count(),
            OUTPUT_TAIL_CHARS
        );
    }

    #[test]
    fn verify_runner_command_line_quotes_only_when_needed() {
        let runner = VerifyRunner {
            name: "custom",
            program: "tool",
            args: vec!["safe", "needs space"],
        };

        assert_eq!(runner.command_line(), "tool safe 'needs space'");
    }

    #[cfg(windows)]
    #[test]
    fn select_verify_runner_skips_scripts_check_on_windows() {
        let root = std::env::temp_dir().join(format!(
            "exo-verify-runner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let scripts_dir = root.join("scripts");
        std::fs::create_dir_all(&scripts_dir).expect("create scripts dir");
        std::fs::write(scripts_dir.join("check"), "echo check").expect("write check script");

        let runner = select_verify_runner(&root);

        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(runner.name, "scripts/check");
    }
}
