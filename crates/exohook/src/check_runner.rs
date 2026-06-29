//! Hybrid command runner with automatic PTY/pipe dispatch.
//!
//! This module provides the main entry point for running checks, automatically
//! choosing between PTY-based execution (Unix TTY) and pipe-based execution
//! (Windows, CI, machine output).
//!
//! # Dispatch Logic
//!
//! | Context                   | Execution Mode  | Rationale                                   |
//! |---------------------------|-----------------|---------------------------------------------|
//! | Unix + TTY + Human output | PTY             | Native colors, realistic terminal emulation |
//! | Unix + no TTY (CI/pipe)   | Pipes           | Can't use PTY without a terminal            |
//! | Unix + machine format     | Pipes           | Machine output needs structured streams     |
//! | Windows (any mode)        | Pipes           | `pty-process` doesn't support Windows       |

use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::Result;

use crate::output_buffer::OutputBuffer;
use crate::pipe_runner::{self, PipeCheckResult};

#[cfg(unix)]
use crate::pty_runner::{self, PtyCheckResult};

/// Output mode for check execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable output (default).
    Human,
    /// Machine-readable output (JSON).
    Machine,
}

/// Unified result from running a check (regardless of runner).
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Process exit status.
    pub status: ExitStatus,
    /// Captured stdout bytes (empty for PTY mode - use `output` instead).
    pub stdout: Vec<u8>,
    /// Captured stderr bytes (empty for PTY mode - use `output` instead).
    pub stderr: Vec<u8>,
    /// Combined output (for PTY mode where stdout/stderr are merged).
    /// For pipe mode, this is stdout + stderr concatenated.
    pub output: Vec<u8>,
    /// How long the check took to run.
    pub duration: Duration,
    /// Whether PTY mode was used.
    pub used_pty: bool,
}

impl CheckResult {
    /// Get all output as a combined byte slice.
    ///
    /// For PTY mode, returns the merged output.
    /// For pipe mode, returns stdout followed by stderr.
    pub fn combined_output(&self) -> &[u8] {
        // For PTY mode or when output is already combined, return output
        if self.used_pty || !self.output.is_empty() {
            &self.output
        } else {
            // For pipe mode without combined, just return stdout
            &self.stdout
        }
    }
}

#[cfg(unix)]
impl From<PtyCheckResult> for CheckResult {
    fn from(pty: PtyCheckResult) -> Self {
        Self {
            status: pty.status,
            stdout: Vec::new(), // PTY merges streams
            stderr: Vec::new(), // PTY merges streams
            output: pty.output,
            duration: pty.duration,
            used_pty: true,
        }
    }
}

impl From<PipeCheckResult> for CheckResult {
    fn from(pipe: PipeCheckResult) -> Self {
        // Combine stdout + stderr for the output field
        let mut output = pipe.stdout.clone();
        output.extend_from_slice(&pipe.stderr);

        Self {
            status: pipe.status,
            stdout: pipe.stdout,
            stderr: pipe.stderr,
            output,
            duration: pipe.duration,
            used_pty: false,
        }
    }
}

/// Determine whether to use PTY mode.
///
/// Returns true if:
/// - Running on Unix
/// - stdout is a terminal (TTY)
/// - Output mode is Human (not Machine)
/// - force_pipes is false
fn should_use_pty(output_mode: OutputMode, force_pipes: bool) -> bool {
    !force_pipes
        && cfg!(unix)
        && std::io::stdout().is_terminal()
        && output_mode != OutputMode::Machine
}

/// Spawn a check with automatic PTY/pipe dispatch.
///
/// This is the main entry point for running checks. It automatically chooses
/// the best execution mode based on platform and context.
///
/// # Arguments
///
/// * `program` - The program to run (e.g., "cargo")
/// * `args` - Arguments to pass to the program
/// * `repo_root` - Working directory for the command
/// * `output_mode` - Whether output is for humans or machines
/// * `output_buffer` - Optional buffer for VTE-parsed output (for progress display)
/// * `stream_output` - If true, streams output to stdout/stderr in real-time
/// * `force_pipes` - If true, forces pipe-based execution even when PTY would normally be used
/// * `term_width` - Terminal width for PTY sizing
///
/// # Returns
///
/// A unified `CheckResult` that works regardless of which runner was used.
#[allow(clippy::too_many_arguments)]
pub fn spawn_check(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_mode: OutputMode,
    output_buffer: Option<OutputBuffer>,
    stream_output: bool,
    force_pipes: bool,
    term_width: u16,
) -> Result<CheckResult> {
    #[cfg(not(unix))]
    let _ = term_width;

    if should_use_pty(output_mode, force_pipes) {
        #[cfg(unix)]
        {
            let pty_result = pty_runner::spawn_streaming(
                program,
                args,
                repo_root,
                output_buffer,
                stream_output,
                term_width,
            )?;
            return Ok(pty_result.into());
        }
    }

    // Fallback to pipe runner (Windows, non-TTY, or machine output)
    let pipe_result =
        pipe_runner::spawn_streaming(program, args, repo_root, output_buffer, stream_output)?;
    Ok(pipe_result.into())
}

/// Check if PTY mode would be used for the given output mode.
///
/// This is useful for logging or debugging which mode will be used.
#[allow(dead_code)]
pub fn would_use_pty(output_mode: OutputMode, force_pipes: bool) -> bool {
    should_use_pty(output_mode, force_pipes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn echo_command(message: &str) -> (&'static str, Vec<String>) {
        #[cfg(windows)]
        {
            ("cmd.exe", vec!["/C".to_string(), format!("echo {message}")])
        }

        #[cfg(not(windows))]
        {
            ("echo", vec![message.to_string()])
        }
    }

    #[test]
    fn test_spawn_check_simple() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = echo_command("hello");
        let result = spawn_check(
            program,
            &args,
            &repo_root,
            OutputMode::Human,
            None,
            false,
            false,
            80, // default terminal width
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());

        let output_str = String::from_utf8_lossy(result.combined_output());
        assert!(output_str.contains("hello"));
    }

    #[test]
    fn test_spawn_check_machine_mode_uses_pipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = echo_command("hello");
        let result = spawn_check(
            program,
            &args,
            &repo_root,
            OutputMode::Machine,
            None,
            false,
            false,
            80,
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());
        // Machine mode should use pipes (not PTY)
        assert!(!result.used_pty);
    }

    #[test]
    fn test_spawn_check_with_buffer() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let buffer = OutputBuffer::new(3);
        let (program, args) = echo_command("test line");

        let result = spawn_check(
            program,
            &args,
            &repo_root,
            OutputMode::Human,
            Some(buffer.clone()),
            false,
            false,
            80,
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());

        // Buffer should have the output
        let snapshot = buffer.snapshot();
        let has_test = snapshot.iter().any(|l| l.contains("test"));
        assert!(has_test);
    }

    #[test]
    fn test_spawn_check_force_pipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = echo_command("hello");
        let result = spawn_check(
            program,
            &args,
            &repo_root,
            OutputMode::Human,
            None,
            false,
            true, // force_pipes = true
            80,
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());
        // force_pipes should force pipe mode (not PTY)
        assert!(!result.used_pty);
    }

    #[test]
    fn test_would_use_pty() {
        // Machine mode should never use PTY
        assert!(!would_use_pty(OutputMode::Machine, false));

        // force_pipes should never use PTY
        assert!(!would_use_pty(OutputMode::Human, true));

        // Human mode depends on platform and TTY, but the function should not panic
        let _ = would_use_pty(OutputMode::Human, false);
    }
}
