//! PTY-based streaming runner for Unix platforms.
//!
//! This module provides PTY-based command execution that preserves native
//! terminal colors and escape sequences. It's used on Unix TTYs when human
//! output is desired.
//!
//! # Key Properties
//!
//! - Spawns commands attached to a pseudo-terminal (PTY)
//! - Preserves native colors without needing `--color=always` flags
//! - stdout and stderr are merged (inherent to PTY design)
//! - Terminal size is set to 24×(term_width)
//! - `TERM=xterm-256color` for 256-color support
//! - Responds to terminal capability queries (OSC 10/11, CSI 6n) to prevent timeouts

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use pty_process::Size;
use pty_process::blocking::{Command, Pty, open as pty_open};

use crate::output_buffer::OutputBuffer;

/// Patterns that indicate terminal query responses we wrote.
/// These get echoed back and should be filtered from output.
const OSC_10_RESPONSE: &[u8] = b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\";
const OSC_11_RESPONSE: &[u8] = b"\x1b]11;rgb:0000/0000/0000\x1b\\";
const CSI_CPR_RESPONSE: &[u8] = b"\x1b[1;1R";

/// Respond to terminal capability queries in the output stream.
///
/// Many tools (cargo, rustc, supports-color crate, etc.) send escape sequence
/// queries to detect terminal capabilities. If we don't respond, they wait
/// several seconds for a timeout before continuing, causing perceived delays.
///
/// Returns true if any response was written.
fn respond_to_terminal_queries(pty: &mut Pty, data: &[u8]) -> bool {
    let mut responded = false;

    // CSI 6n - Cursor Position Report
    if data.windows(4).any(|w| w == b"\x1b[6n") {
        let _ = pty.write_all(CSI_CPR_RESPONSE);
        let _ = pty.flush();
        responded = true;
    }

    // OSC 10;? - Query foreground color
    if data.windows(6).any(|w| w == b"\x1b]10;?") {
        let _ = pty.write_all(OSC_10_RESPONSE);
        let _ = pty.flush();
        responded = true;
    }

    // OSC 11;? - Query background color
    if data.windows(6).any(|w| w == b"\x1b]11;?") {
        let _ = pty.write_all(OSC_11_RESPONSE);
        let _ = pty.flush();
        responded = true;
    }

    responded
}

/// Filter out caret-notation query responses from the output.
///
/// PTY echo can convert escape sequences into caret notation (e.g. `^[`),
/// which turns control sequences into printable text. VTE only understands
/// real escape bytes, so we strip caret-notation responses before they hit
/// the rolling buffer or captured output.
///
/// This handles:
/// - Caret notation OSC 10/11/12 color query responses
/// - Caret notation CSI 6n cursor position response
fn filter_caret_query_responses(data: &[u8]) -> Vec<u8> {
    let mut result = data.to_vec();

    // Remove caret notation CSI cursor position response: ^[[1;1R
    const CARET_CSI_CPR_RESPONSE: &[u8] = b"^[[1;1R";
    while let Some(pos) = find_subsequence(&result, CARET_CSI_CPR_RESPONSE) {
        result.drain(pos..pos + CARET_CSI_CPR_RESPONSE.len());
    }

    // Remove caret notation OSC responses: ^[]N;...^[\ or ^[]N;...^G
    result = filter_caret_osc_sequences(&result, &[b"10", b"11", b"12"]);

    result
}

/// Filter OSC sequences in caret notation (^[]N;...^[\ or ^[]N;...^G).
///
/// Some terminals/PTYs echo escape sequences back in caret notation.
fn filter_caret_osc_sequences(data: &[u8], prefixes: &[&[u8]]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        // Check for caret notation OSC start: ^ [ ]
        if i + 2 < data.len() && data[i] == b'^' && data[i + 1] == b'[' && data[i + 2] == b']' {
            // Look for which prefix this matches
            let mut matched_prefix = false;
            for prefix in prefixes {
                let prefix_start = i + 3;
                let prefix_end = prefix_start + prefix.len();

                if prefix_end < data.len()
                    && &data[prefix_start..prefix_end] == *prefix
                    && data[prefix_end] == b';'
                {
                    // Found a matching caret OSC sequence, now find the terminator
                    if let Some(end) = find_caret_osc_terminator(&data[prefix_end + 1..]) {
                        // Skip the entire OSC sequence
                        i = prefix_end + 1 + end;
                        matched_prefix = true;
                        break;
                    }
                }
            }

            if matched_prefix {
                continue;
            }
        }

        // Not an OSC sequence we're filtering, keep the byte
        result.push(data[i]);
        i += 1;
    }

    result
}

/// Find the end of a caret notation OSC sequence.
/// Returns the index just past the terminator.
///
/// Caret notation terminators:
/// - ^G (BEL in caret notation)
/// - ^[\ (ST in caret notation)
fn find_caret_osc_terminator(data: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < data.len() {
        // ^G (caret notation for BEL)
        if data[i] == b'^' && i + 1 < data.len() && data[i + 1] == b'G' {
            return Some(i + 2);
        }
        // ^[\ (caret notation for ST)
        if data[i] == b'^' && i + 2 < data.len() && data[i + 1] == b'[' && data[i + 2] == b'\\' {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

/// Find a subsequence in a slice
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Result of running a check via PTY.
#[derive(Debug, Clone)]
pub struct PtyCheckResult {
    /// Process exit status.
    pub status: ExitStatus,
    /// Combined stdout+stderr output (merged by PTY).
    pub output: Vec<u8>,
    /// How long the check took to run.
    pub duration: Duration,
}

/// Spawn a command with PTY-based streaming.
///
/// This function spawns a child process attached to a pseudo-terminal,
/// providing native terminal colors and escape sequence handling.
///
/// # Arguments
///
/// * `program` - The program to run (e.g., "cargo")
/// * `args` - Arguments to pass to the program
/// * `repo_root` - Working directory for the command
/// * `output_buffer` - Optional buffer for VTE-parsed output (for progress display)
/// * `stream_to_stdout` - If true, streams output to real stdout in real-time
/// * `term_width` - Terminal width for PTY sizing (floored at 40)
///
/// # Returns
///
/// A `PtyCheckResult` containing the exit status, combined output, and duration.
pub fn spawn_streaming(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_buffer: Option<OutputBuffer>,
    stream_to_stdout: bool,
    term_width: u16,
) -> Result<PtyCheckResult> {
    let start = Instant::now();

    // Open PTY master/slave pair
    let (mut pty, pts) = pty_open().context("failed to create PTY")?;

    // Use dynamic width with floor at 40 columns to prevent tool breakage
    let pty_width = term_width.max(40);
    pty.resize(Size::new(24, pty_width))
        .context("failed to resize PTY")?;

    // Spawn child attached to PTY slave
    let cmd = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .env("TERM", "xterm-256color") // Enable 256-color support
        // Tell cargo we support colors so it doesn't send OSC queries and wait for a response.
        // Without this, cargo sends OSC 10/11 queries to detect terminal colors, and since
        // our PTY doesn't respond, cargo waits several seconds before continuing.
        .env("CARGO_TERM_COLOR", "always");

    let mut child = cmd
        .spawn(pts)
        .with_context(|| format!("failed to spawn '{program}' with PTY"))?;

    // Stream output from PTY master
    let mut output = Vec::new();
    let mut buf = [0u8; 4096];

    // File-based debug logging
    #[cfg(debug_assertions)]
    let debug_file = if std::env::var("EXOHOOK_DEBUG_PTY").is_ok() {
        use std::fs::OpenOptions;
        Some(std::sync::Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/exohook-pty-debug.log")
                .ok(),
        ))
    } else {
        None
    };

    #[cfg(debug_assertions)]
    #[allow(clippy::collapsible_if)]
    if let Some(ref df) = debug_file {
        if let Some(f) = df.lock().unwrap().as_mut() {
            use std::io::Write;
            let arc_id = output_buffer.as_ref().map(|b| b.arc_id());
            let _ = writeln!(
                f,
                "[PTY] spawn_streaming: program={}, has_buffer={}, arc={:?}",
                program,
                output_buffer.is_some(),
                arc_id
            );
        }
    }

    loop {
        match pty.read(&mut buf) {
            Ok(0) => break, // EOF - child closed PTY
            Ok(n) => {
                let chunk = &buf[..n];

                // Respond to terminal queries immediately to prevent child from blocking
                respond_to_terminal_queries(&mut pty, chunk);

                // Filter out caret-notation query responses for buffer/capture
                // Only run the filter if there's a '^' in the chunk
                let has_caret = chunk.contains(&b'^');
                let caret_filtered = if has_caret {
                    filter_caret_query_responses(chunk)
                } else {
                    Vec::new()
                };

                // Determine which bytes to use for buffer/capture:
                // - If no '^' in chunk: use original chunk as-is
                // - If filter removed everything: skip this chunk entirely
                // - If filter removed some bytes: use filtered result
                // - If filter removed nothing: use original chunk (avoid allocation)
                let caret_chunk: &[u8] = if !has_caret {
                    chunk // No caret in chunk, use original
                } else if caret_filtered.is_empty() {
                    // Filter removed everything, skip
                    continue;
                } else if caret_filtered.len() == chunk.len() {
                    chunk // Filter didn't remove anything
                } else {
                    &caret_filtered // Use filtered result
                };

                #[cfg(debug_assertions)]
                #[allow(clippy::collapsible_if)]
                if let Some(ref df) = debug_file {
                    if let Some(f) = df.lock().unwrap().as_mut() {
                        use std::io::Write;
                        let _ = writeln!(
                            f,
                            "[PTY] Read {} bytes (filtered to {}), has_buffer={}",
                            n,
                            caret_chunk.len(),
                            output_buffer.is_some()
                        );
                    }
                }

                // Write to real stdout for immediate display if requested
                if stream_to_stdout {
                    let _ = std::io::stdout().write_all(chunk);
                    let _ = std::io::stdout().flush();
                }

                // Feed to OutputBuffer if provided (for progress display)
                if let Some(ref buffer) = output_buffer {
                    buffer.feed(caret_chunk);
                }

                // Capture for later use
                output.extend_from_slice(caret_chunk);
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => break, // PTY closed
        }
    }

    // Flush any partial line remaining in the buffer
    if let Some(ref buffer) = output_buffer {
        buffer.flush();
    }

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for '{program}'"))?;

    Ok(PtyCheckResult {
        status,
        output,
        duration: start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_spawn_simple_command() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let result = spawn_streaming("echo", &["hello".to_string()], &repo_root, None, false, 80);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());
        let output_str = String::from_utf8_lossy(&result.output);
        assert!(output_str.contains("hello"));
    }

    #[test]
    fn test_spawn_with_output_buffer() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let buffer = OutputBuffer::new(3);

        let result = spawn_streaming(
            "echo",
            &["line1".to_string()],
            &repo_root,
            Some(buffer.clone()),
            false,
            80,
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.status.success());

        // Buffer should have captured the output
        let snapshot = buffer.snapshot();
        // At least one line should have "line1"
        let has_line1 = snapshot.iter().any(|l| l.contains("line1"));
        assert!(has_line1);
    }

    #[test]
    fn test_spawn_failing_command() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let result = spawn_streaming("false", &[], &repo_root, None, false, 80);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(!result.status.success());
    }

    // ========================================================================
    // Tests for caret notation query response filtering
    // ========================================================================

    // ========================================================================
    // Tests for caret notation OSC filtering
    // ========================================================================

    #[test]
    fn test_filter_caret_osc_11_with_st_terminator() {
        // Caret notation: ^[]11;rgb:0000/0000/0000^[\
        let input = b"^[]11;rgb:0000/0000/0000^[\\info: cargo";
        let filtered = filter_caret_query_responses(input);
        let result = String::from_utf8_lossy(&filtered);

        assert_eq!(
            result, "info: cargo",
            "Caret notation OSC 11 with ST should be filtered"
        );
    }

    #[test]
    fn test_filter_caret_osc_11_with_bel_terminator() {
        // Caret notation: ^[]11;rgb:0000/0000/0000^G
        let input = b"^[]11;rgb:0000/0000/0000^Ginfo: cargo";
        let filtered = filter_caret_query_responses(input);
        let result = String::from_utf8_lossy(&filtered);

        assert_eq!(
            result, "info: cargo",
            "Caret notation OSC 11 with BEL should be filtered"
        );
    }

    #[test]
    fn test_filter_caret_osc_10_with_st_terminator() {
        // Caret notation: ^[]10;rgb:ffff/ffff/ffff^[\
        let input = b"^[]10;rgb:ffff/ffff/ffff^[\\text";
        let filtered = filter_caret_query_responses(input);
        let result = String::from_utf8_lossy(&filtered);

        assert_eq!(
            result, "text",
            "Caret notation OSC 10 with ST should be filtered"
        );
    }

    #[test]
    fn test_filter_mixed_real_and_caret_osc() {
        // Mix of real ESC sequences and caret notation
        let input = b"\x1b]11;rgb:0000/0000/0000\x07^[]10;rgb:ffff/ffff/ffff^[\\output";
        let filtered = filter_caret_query_responses(input);

        let mut expected = b"\x1b]11;rgb:0000/0000/0000\x07".to_vec();
        expected.extend_from_slice(b"output");

        assert_eq!(
            filtered, expected,
            "Should preserve real OSC sequences while filtering caret notation"
        );
    }

    #[test]
    fn test_filter_caret_csi_cpr_response() {
        let input = b"^[[1;1Rinfo: cargo";
        let filtered = filter_caret_query_responses(input);
        let result = String::from_utf8_lossy(&filtered);

        assert_eq!(
            result, "info: cargo",
            "Caret notation CSI CPR response should be filtered"
        );
    }

    #[test]
    fn test_filter_preserves_real_osc() {
        let input = b"\x1b]11;rgb:0000/0000/0000\x07info";
        let filtered = filter_caret_query_responses(input);

        assert_eq!(
            filtered, input,
            "Real OSC sequences should pass through caret-only filtering"
        );
    }

    #[test]
    fn test_filter_caret_osc_split_across_chunks() {
        // Document the limitation: caret filter only works when entire sequence is in one chunk.
        // When split across chunks, the sanitize_caret_osc() in OutputBuffer::snapshot() handles it.

        // First chunk ends mid-OSC (no terminator found)
        let chunk1 = b"^[]11;rgb:0000/0000";
        let chunk2 = b"/0000^[\\info: cargo";

        // Each chunk is filtered independently - neither matches the full pattern
        let filtered1 = filter_caret_query_responses(chunk1);
        let filtered2 = filter_caret_query_responses(chunk2);

        // Chunk1: has ^[] but no terminator, so passes through
        assert_eq!(filtered1.as_slice(), chunk1);

        // Chunk2: starts mid-sequence (no ^[]), so passes through
        assert_eq!(filtered2.as_slice(), chunk2);

        // NOTE: This is why OutputBuffer::snapshot() has sanitize_caret_osc() as a backup.
        // The caret filter is the first line of defense, but fragmented sequences
        // slip through and get caught by the snapshot sanitizer.
    }
}
