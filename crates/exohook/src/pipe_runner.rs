//! Pipe-based streaming runner (fallback for non-Unix, non-TTY, or machine output).
//!
//! This module provides piped command execution that maintains separate
//! stdout/stderr streams. It's used when:
//!
//! - Running on Windows
//! - Running without a TTY (CI, piped output)
//! - Using machine-readable output format (--format=json)
//!
//! # Key Properties
//!
//! - Separate stdout and stderr streams (preserved)
//! - Respects environment settings for color (no forced color)
//! - stdin is null to prevent commands from blocking on input

use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(debug_assertions)]
use std::io::Write;

use anyhow::{Context, Result};

use crate::output_buffer::OutputBuffer;

/// Result of running a check via pipes.
#[derive(Debug, Clone)]
pub struct PipeCheckResult {
    /// Process exit status.
    pub status: ExitStatus,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// How long the check took to run.
    pub duration: Duration,
}

/// Spawn a command with pipe-based streaming.
///
/// This function spawns a child process with piped stdout/stderr,
/// maintaining separate streams while optionally streaming output
/// in real-time.
///
/// # Arguments
///
/// * `program` - The program to run (e.g., "cargo")
/// * `args` - Arguments to pass to the program
/// * `repo_root` - Working directory for the command
/// * `output_buffer` - Optional buffer for VTE-parsed output (for progress display)
/// * `stream_output` - If true, streams output to real stdout/stderr in real-time
///
/// # Returns
///
/// A `PipeCheckResult` containing the exit status, separated stdout/stderr, and duration.
pub fn spawn_streaming(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_buffer: Option<OutputBuffer>,
    stream_output: bool,
) -> Result<PipeCheckResult> {
    let start = Instant::now();

    let mut child = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::null()) // Prevent stdin hangs
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{program}'"))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let (tx, rx) = mpsc::channel::<(&'static str, Vec<u8>)>();
    let tx_stdout = tx.clone();
    let tx_stderr = tx;

    // Clone output buffer for each reader thread
    let output_buffer_stdout = output_buffer.clone();
    let output_buffer_stderr = output_buffer.clone();

    // Create debug file if needed (debug builds only)
    #[cfg(debug_assertions)]
    let debug_file = if std::env::var("EXOHOOK_DEBUG_PIPE").is_ok() {
        use std::fs::OpenOptions;
        Some(std::sync::Arc::new(std::sync::Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/exohook-pipe-debug.log")
                .ok(),
        )))
    } else {
        None
    };
    #[cfg(debug_assertions)]
    let debug_file_stdout = debug_file.clone();
    #[cfg(debug_assertions)]
    let debug_file_stderr = debug_file.clone();

    // Log initial state
    #[cfg(debug_assertions)]
    #[allow(clippy::collapsible_if)]
    if let Some(df) = &debug_file {
        if let Some(f) = df.lock().unwrap().as_mut() {
            let _ = writeln!(
                f,
                "[PIPE] spawn_streaming: program={}, has_buffer={}",
                program,
                output_buffer.is_some()
            );
        }
    }

    // Stdout reader thread
    let stdout_handle = thread::spawn(move || {
        let mut reader = stdout;
        let mut buffer = [0u8; 4096];
        let mut all_output = Vec::new();

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let chunk = buffer[..n].to_vec();
                    all_output.extend_from_slice(&chunk);

                    #[cfg(debug_assertions)]
                    #[allow(clippy::collapsible_if)]
                    if let Some(df) = &debug_file_stdout {
                        if let Some(f) = df.lock().unwrap().as_mut() {
                            let _ = writeln!(
                                f,
                                "[PIPE stdout] Read {} bytes, has_buffer={}",
                                n,
                                output_buffer_stdout.is_some()
                            );
                        }
                    }

                    // Feed to OutputBuffer if provided
                    if let Some(ref buf) = output_buffer_stdout {
                        buf.feed(&chunk);
                    }

                    // Send for streaming
                    let _ = tx_stdout.send(("out", chunk));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }

        all_output
    });

    // Stderr reader thread
    let stderr_handle = thread::spawn(move || {
        let mut reader = stderr;
        let mut buffer = [0u8; 4096];
        let mut all_output = Vec::new();

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let chunk = buffer[..n].to_vec();
                    all_output.extend_from_slice(&chunk);

                    #[cfg(debug_assertions)]
                    #[allow(clippy::collapsible_if)]
                    if let Some(df) = &debug_file_stderr {
                        if let Some(f) = df.lock().unwrap().as_mut() {
                            let _ = writeln!(
                                f,
                                "[PIPE stderr] Read {} bytes, has_buffer={}",
                                n,
                                output_buffer_stderr.is_some()
                            );
                        }
                    }

                    // Feed to OutputBuffer if provided
                    if let Some(ref buf) = output_buffer_stderr {
                        buf.feed(&chunk);
                    }

                    // Send for streaming
                    let _ = tx_stderr.send(("err", chunk));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }

        all_output
    });

    // Both senders are now moved into threads; rx will close when both threads finish

    if stream_output {
        for (stream, chunk) in rx {
            match stream {
                "out" => {
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(&chunk);
                    let _ = std::io::stdout().flush();
                }
                "err" => {
                    use std::io::Write;
                    let _ = std::io::stderr().write_all(&chunk);
                    let _ = std::io::stderr().flush();
                }
                _ => {}
            }
        }
    } else {
        // Drain without printing (for parallel mode with spinners)
        for _ in rx {}
    }

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for '{program}'"))?;

    Ok(PipeCheckResult {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        duration: start.elapsed(),
    })
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

    fn stderr_command() -> (&'static str, Vec<String>) {
        #[cfg(windows)]
        {
            (
                "cmd.exe",
                vec!["/C".to_string(), "echo stderr 1>&2".to_string()],
            )
        }

        #[cfg(not(windows))]
        {
            ("sh", vec!["-c".to_string(), "echo stderr >&2".to_string()])
        }
    }

    fn failing_command() -> (&'static str, Vec<String>) {
        #[cfg(windows)]
        {
            ("cmd.exe", vec!["/C".to_string(), "exit /B 1".to_string()])
        }

        #[cfg(not(windows))]
        {
            ("false", Vec::new())
        }
    }

    #[test]
    fn test_spawn_simple_command() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = echo_command("hello");
        let result = spawn_streaming(program, &args, &repo_root, None, false);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());
        let output_str = String::from_utf8_lossy(&result.stdout);
        assert!(output_str.contains("hello"));
    }

    #[test]
    fn test_spawn_with_output_buffer() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let buffer = OutputBuffer::new(3);
        let (program, args) = echo_command("line1");

        let result = spawn_streaming(program, &args, &repo_root, Some(buffer.clone()), false);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());

        // Buffer should have captured the output
        let snapshot = buffer.snapshot();
        let has_line1 = snapshot.iter().any(|l| l.contains("line1"));
        assert!(has_line1);
    }

    #[test]
    fn test_spawn_with_stderr() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = stderr_command();
        let result = spawn_streaming(program, &args, &repo_root, None, false);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());
        let stderr_str = String::from_utf8_lossy(&result.stderr);
        assert!(stderr_str.contains("stderr"));
    }

    #[test]
    fn test_spawn_failing_command() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (program, args) = failing_command();
        let result = spawn_streaming(program, &args, &repo_root, None, false);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(!result.status.success());
    }
}
