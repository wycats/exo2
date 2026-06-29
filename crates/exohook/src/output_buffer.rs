//! Multi-line rolling buffer with VTE parsing for streaming terminal output.
//!
//! This module provides [`OutputBuffer`], a thread-safe buffer that processes
//! raw terminal output using VTE (Virtual Terminal Emulator) parsing. It maintains
//! a rolling window of the most recent complete lines, handling ANSI escape
//! sequences, carriage returns (for progress bars), and other terminal control
//! characters correctly.
//!
//! # Example
//!
//! ```
//! use exohook::OutputBuffer;
//!
//! let buffer = OutputBuffer::new(3);
//!
//! // Feed raw bytes from a child process
//! buffer.feed(b"Line 1\nLine 2\nLine 3\nLine 4\n");
//!
//! // Get the 3 most recent lines
//! let snapshot = buffer.snapshot();
//! assert_eq!(snapshot[0], "Line 2");
//! assert_eq!(snapshot[1], "Line 3");
//! assert_eq!(snapshot[2], "Line 4");
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use console::measure_text_width;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use vte::{Params, Parser, Perform};

use crate::terminal::format_result_line;

/// Sanitize a string by removing caret-notation OSC sequences that may have
/// slipped through fragmented chunk filtering.
///
/// This handles patterns like:
/// - `^[]10;rgb:...^[\` (OSC 10 response with ST terminator)
/// - `^[]11;rgb:...^[\` (OSC 11 response with ST terminator)
/// - `^[]10;rgb:...^G` (OSC 10 response with BEL terminator)
/// - `^[]11;rgb:...^G` (OSC 11 response with BEL terminator)
/// - `^[[1;1R` (CSI cursor position response)
fn sanitize_caret_osc(s: &str) -> String {
    // Fast path: if no caret, nothing to filter
    if !s.contains('^') {
        return s.to_string();
    }

    let mut result = s.to_string();

    // Remove CSI cursor position response: ^[[1;1R
    result = result.replace("^[[1;1R", "");

    // Remove caret OSC sequences with various terminators
    // Pattern: ^[]N;...^[\ or ^[]N;...^G where N is 10, 11, or 12
    loop {
        let before_len = result.len();

        // Find ^[]10; or ^[]11; or ^[]12;
        for prefix in &["^[]10;", "^[]11;", "^[]12;"] {
            if let Some(start) = result.find(prefix) {
                // Find the terminator (^[\ or ^G)
                let search_start = start + prefix.len();
                if let Some(st_pos) = result[search_start..].find("^[\\") {
                    // Remove the entire sequence including ST terminator
                    let end = search_start + st_pos + 3;
                    result = format!("{}{}", &result[..start], &result[end..]);
                } else if let Some(bel_pos) = result[search_start..].find("^G") {
                    // Remove the entire sequence including BEL terminator
                    let end = search_start + bel_pos + 2;
                    result = format!("{}{}", &result[..start], &result[end..]);
                }
            }
        }

        // No more changes, we're done
        if result.len() == before_len {
            break;
        }
    }

    result
}

/// A thread-safe rolling buffer for terminal output.
///
/// Uses VTE parsing to correctly handle ANSI escape sequences, carriage returns,
/// and other terminal control characters. Maintains the most recent N complete
/// lines where N is the configured capacity.
#[derive(Clone)]
pub struct OutputBuffer {
    inner: Arc<Mutex<OutputBufferInner>>,
}

/// Internal state for the output buffer.
///
/// This struct also implements `Perform` for VTE parsing.
struct OutputBufferInner {
    /// Rolling buffer of complete lines (up to capacity).
    lines: VecDeque<String>,

    /// Current line being built (not yet committed with newline).
    current_line: String,

    /// VTE parser instance (stateful).
    parser: Parser,

    /// Maximum number of lines to retain.
    capacity: usize,

    /// Pending carriage return - cleared on next print, ignored on newline.
    /// This allows CRLF to work correctly: \r sets pending, \n commits without clearing.
    /// For progress bars: \r sets pending, next print clears and overwrites.
    pending_cr: bool,

    /// When output was last received (for silence duration tracking).
    last_output: Instant,

    /// Total bytes fed (for debugging).
    #[cfg(debug_assertions)]
    total_bytes: usize,
}

impl OutputBuffer {
    /// Create a new output buffer with the specified line capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of complete lines to retain (typically 3)
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(OutputBufferInner {
                lines: VecDeque::with_capacity(capacity),
                current_line: String::new(),
                parser: Parser::new(),
                capacity,
                pending_cr: false,
                last_output: Instant::now(),
                #[cfg(debug_assertions)]
                total_bytes: 0,
            })),
        }
    }

    /// Feed raw bytes from a terminal stream.
    ///
    /// The bytes are parsed through VTE to correctly handle escape sequences,
    /// control characters, and build up lines. Complete lines (terminated by `\n`)
    /// are added to the rolling buffer.
    ///
    /// This method is thread-safe and can be called from reader threads.
    pub fn feed(&self, bytes: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        if !bytes.is_empty() {
            inner.last_output = Instant::now();
            #[cfg(debug_assertions)]
            {
                inner.total_bytes += bytes.len();
                if std::env::var("EXOHOOK_DEBUG_BUFFER").is_ok() {
                    use std::fs::OpenOptions;
                    use std::io::Write;
                    if let Ok(mut f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/exohook-buffer-debug.log")
                    {
                        let _ = writeln!(
                            f,
                            "[BUFFER] feed: arc={:x}, len={}, bytes={:?}, as_str={:?}",
                            Arc::as_ptr(&self.inner) as usize,
                            bytes.len(),
                            bytes,
                            String::from_utf8_lossy(bytes)
                        );
                    }
                }
            }
        }
        inner.feed_bytes(bytes);
    }

    /// Get the Arc pointer address (for debugging identity).
    #[cfg(debug_assertions)]
    pub fn arc_id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// Get debug stats about the buffer.
    #[cfg(debug_assertions)]
    pub fn debug_stats(&self) -> (usize, usize, usize) {
        let inner = self.inner.lock().unwrap();
        (
            inner.total_bytes,
            inner.lines.len(),
            inner.current_line.len(),
        )
    }

    /// Get the duration since output was last received.
    ///
    /// This is useful for detecting when a check has gone silent and may
    /// need a warning message to discourage AI agents from giving up.
    pub fn silence_duration(&self) -> Duration {
        let inner = self.inner.lock().unwrap();
        inner.last_output.elapsed()
    }

    /// Get a snapshot of the current viewport state.
    ///
    /// Returns the 3 most recent visible lines as they would appear in a terminal:
    /// - If `current_line` is non-empty: `[committed[-2], committed[-1], current_line]`
    /// - If `current_line` is empty: `[committed[-3], committed[-2], committed[-1]]`
    ///
    /// This ensures that progress bars using `\r` are visible in the snapshot,
    /// matching the terminal viewport mental model described in VISION.md.
    ///
    /// Lines are sanitized to remove any caret-notation OSC sequences that may
    /// have slipped through fragmented chunk filtering.
    pub fn snapshot(&self) -> [String; 3] {
        let inner = self.inner.lock().unwrap();

        // The viewport shows the last N lines as they would appear in a terminal.
        // If current_line is non-empty, it's the bottom of the viewport (cursor is there).
        // If current_line is empty (cursor at start of new line), show last 3 committed.
        let use_current = !inner.current_line.is_empty();
        let committed_needed = if use_current { 2 } else { 3 };

        // Collect the last N committed lines (in order: oldest to newest)
        let committed: Vec<String> = inner
            .lines
            .iter()
            .rev()
            .take(committed_needed)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Helper to sanitize a line (remove caret-notation OSC that slipped through)
        let sanitize = |s: String| -> String { sanitize_caret_osc(&s) };

        if use_current {
            // [committed[-2], committed[-1], current_line]
            match committed.len() {
                0 => [
                    String::new(),
                    String::new(),
                    sanitize(inner.current_line.clone()),
                ],
                1 => [
                    String::new(),
                    sanitize(committed[0].clone()),
                    sanitize(inner.current_line.clone()),
                ],
                _ => [
                    sanitize(committed[0].clone()),
                    sanitize(committed[1].clone()),
                    sanitize(inner.current_line.clone()),
                ],
            }
        } else {
            // [committed[-3], committed[-2], committed[-1]]
            match committed.len() {
                0 => [String::new(), String::new(), String::new()],
                1 => [String::new(), String::new(), sanitize(committed[0].clone())],
                2 => [
                    String::new(),
                    sanitize(committed[0].clone()),
                    sanitize(committed[1].clone()),
                ],
                _ => [
                    sanitize(committed[0].clone()),
                    sanitize(committed[1].clone()),
                    sanitize(committed[2].clone()),
                ],
            }
        }
    }

    /// Flush the current line to the buffer even if not terminated.
    ///
    /// This is useful when a child process exits and you want to capture
    /// any remaining partial output.
    pub fn flush(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.commit_current_line();
    }
}

impl OutputBufferInner {
    /// Feed bytes through the VTE parser.
    ///
    /// We use a two-phase approach to avoid borrow checker issues:
    /// 1. Collect actions from VTE parsing into a Vec
    /// 2. Apply the actions to our state
    fn feed_bytes(&mut self, bytes: &[u8]) {
        // Collect actions from VTE parsing
        let mut actions = Vec::new();

        for &byte in bytes {
            let mut collector = ActionCollector {
                actions: &mut actions,
            };
            self.parser.advance(&mut collector, byte);
        }

        // Debug: log action count (always, even if 0 actions)
        #[cfg(debug_assertions)]
        if std::env::var("EXOHOOK_DEBUG_VTE").is_ok() {
            use std::fs::OpenOptions;
            use std::io::Write;
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/exohook-vte-debug.log")
            {
                let _ = writeln!(
                    f,
                    "[VTE] {} bytes -> {} actions, first bytes: {:?}",
                    bytes.len(),
                    actions.len(),
                    &bytes[..bytes.len().min(20)]
                );
            }
        }

        // Apply collected actions to our state
        for action in actions {
            match action {
                VteAction::Print(c) => {
                    // If we have a pending CR, clear the line now (progress bar overwrite)
                    if self.pending_cr {
                        self.current_line.clear();
                        self.pending_cr = false;
                    }
                    self.current_line.push(c);
                }
                VteAction::LineFeed => {
                    // Pending CR is consumed without clearing (CRLF case)
                    self.pending_cr = false;
                    self.commit_current_line();
                }
                VteAction::CarriageReturn => {
                    // Don't clear immediately - set pending flag
                    // This allows CRLF to preserve content while progress bars overwrite
                    self.pending_cr = true;
                }
                VteAction::Tab => {
                    if self.pending_cr {
                        self.current_line.clear();
                        self.pending_cr = false;
                    }
                    let width = measure_text_width(&self.current_line);
                    let spaces_needed = 4 - (width % 4);
                    for _ in 0..spaces_needed {
                        self.current_line.push(' ');
                    }
                }
                VteAction::ClearLine => {
                    self.pending_cr = false;
                    self.current_line.clear();
                }
            }
        }
    }

    /// Commit the current line to the buffer.
    fn commit_current_line(&mut self) {
        let line = std::mem::take(&mut self.current_line);
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

/// Actions that can be collected from VTE parsing.
enum VteAction {
    Print(char),
    LineFeed,
    CarriageReturn,
    Tab,
    ClearLine,
}

/// A VTE Perform implementation that collects actions instead of directly mutating state.
struct ActionCollector<'a> {
    actions: &'a mut Vec<VteAction>,
}

impl Perform for ActionCollector<'_> {
    fn print(&mut self, c: char) {
        match c {
            '\u{2028}' | '\u{2029}' => self.actions.push(VteAction::LineFeed),
            _ => self.actions.push(VteAction::Print(c)),
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x0A => self.actions.push(VteAction::LineFeed),
            0x0D => self.actions.push(VteAction::CarriageReturn),
            0x09 => self.actions.push(VteAction::Tab),
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        if action == 'K' {
            let mode = params
                .iter()
                .next()
                .and_then(|p| p.first().copied())
                .unwrap_or(0);
            if mode == 2 {
                self.actions.push(VteAction::ClearLine);
            }
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

/// A group of progress bars for a single check: 1 spinner + optional context line.
///
/// This provides a multi-line progress display where the main spinner shows
/// the check status, and optionally a single context bar below shows recent output
/// (dim styled with │ prefix). The context bar can display 0-3 lines dynamically
/// using embedded newlines, showing only lines that have content.
///
/// Context lines are hidden in Narrow/Compact terminal modes to save vertical space.
#[derive(Clone)]
pub struct CheckProgressGroup {
    /// Main spinner showing check status (e.g., "  ● test         running...")
    pub spinner: ProgressBar,

    /// Single context bar showing recent output (dim, with │ prefix).
    ///
    /// This is `None` in Narrow/Compact terminal modes where context is hidden.
    /// When present, it can display 0-3 lines dynamically using embedded newlines.
    pub context_bar: Option<ProgressBar>,

    /// Output buffer managing the rolling window of recent lines
    pub output_buffer: OutputBuffer,

    /// Whether this check has started execution.
    ///
    /// Used to distinguish "waiting..." (queued but not started) from
    /// "running..." (actively executing). This matters for smart scheduling
    /// where sequential checks wait for parallel checks to complete.
    started: Arc<AtomicBool>,

    /// When this check started execution (set by mark_started).
    ///
    /// Used to display accurate elapsed time for each check rather than
    /// the total job time.
    start_time: Arc<Mutex<Option<Instant>>>,

    /// High water mark for context lines displayed.
    ///
    /// Once we've shown N lines, we keep showing N lines (with empty placeholders
    /// if needed) to prevent visual jumping. This provides stable viewport height.
    max_context_lines_shown: Arc<AtomicUsize>,
}

impl CheckProgressGroup {
    /// Create a new progress group attached to a MultiProgress.
    ///
    /// # Arguments
    ///
    /// * `mp` - The MultiProgress instance to attach progress bars to
    /// * `label` - The label for the check (e.g., "test", "clippy")
    /// * `label_padding` - Width for label padding (from terminal config)
    ///
    /// # Note
    ///
    /// This always creates context lines. Use [`CheckProgressGroup::new_with_config`]
    /// to conditionally hide context lines in narrow terminals.
    pub fn new(mp: &MultiProgress, label: &str, label_padding: usize) -> Self {
        Self::new_with_config(mp, label, label_padding, true)
    }

    /// Create a new progress group with explicit context line control.
    ///
    /// # Arguments
    ///
    /// * `mp` - The MultiProgress instance to attach progress bars to
    /// * `label` - The label for the check (e.g., "test", "clippy")
    /// * `label_padding` - Width for label padding (from terminal config)
    /// * `show_context_lines` - Whether to create and show context lines
    pub fn new_with_config(
        mp: &MultiProgress,
        label: &str,
        label_padding: usize,
        show_context_lines: bool,
    ) -> Self {
        // Create spinner (same style as existing create_check_spinner)
        let spinner = mp.add(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.cyan} {msg}")
                .expect("valid spinner template"),
        );
        spinner.set_message(format!(
            "{:<width$} waiting...",
            label,
            width = label_padding
        ));
        spinner.enable_steady_tick(Duration::from_millis(100));

        let context_bar = if show_context_lines {
            // Create a single context bar that can show 0-3 lines with embedded newlines.
            // The template uses {msg} which can contain newlines for multi-line display.
            let context_style = ProgressStyle::default_bar()
                .template("{msg}")
                .expect("valid template");

            let bar = mp.insert_after(&spinner, ProgressBar::new(0));
            bar.set_style(context_style);
            // Start with empty message - bar is visible but takes zero height.
            // IMPORTANT: Do NOT use hidden draw target as toggling visibility causes
            // layout jumps in indicatif's MultiProgress.
            bar.set_message("");

            Some(bar)
        } else {
            None
        };

        Self {
            spinner,
            context_bar,
            output_buffer: OutputBuffer::new(3),
            started: Arc::new(AtomicBool::new(false)),
            start_time: Arc::new(Mutex::new(None)),
            max_context_lines_shown: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Mark this check as started.
    ///
    /// Call this when the check begins execution. The updater thread will
    /// then show "running..." instead of "waiting...". Also records the
    /// start time for accurate elapsed time display.
    pub fn mark_started(&self) {
        *self.start_time.lock().unwrap() = Some(Instant::now());
        self.started.store(true, Ordering::Release);
    }

    /// Check if this check has started execution.
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Get the elapsed time since this check started.
    ///
    /// Returns `None` if the check hasn't started yet.
    pub fn elapsed(&self) -> Option<Duration> {
        self.start_time.lock().unwrap().map(|t| t.elapsed())
    }

    /// Update context display with current buffer state.
    ///
    /// Call this periodically from an updater thread to refresh the display
    /// with the latest output from the running command.
    ///
    /// # Arguments
    ///
    /// * `use_color` - Whether to apply dim ANSI styling to the context lines
    /// * `truncation_limit` - Maximum width for truncated output lines
    pub fn update_context(&self, use_color: bool, truncation_limit: usize) {
        // Don't update if spinner is finished (race condition guard)
        if self.spinner.is_finished() {
            return;
        }

        let Some(bar) = &self.context_bar else {
            return;
        };

        // Also check if context bar itself is finished
        if bar.is_finished() {
            return;
        }

        let snapshot = self.output_buffer.snapshot();

        // Collect non-empty lines with proper formatting
        let mut formatted_lines: Vec<String> = snapshot
            .iter()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let truncated = crate::terminal::truncate_for_display(line, truncation_limit);
                let content = if use_color {
                    format!("\x1b[2m{}\x1b[0m", truncated) // dim
                } else {
                    truncated
                };
                format!("    │ {}", content)
            })
            .collect();

        // High water mark: once we've shown N lines, keep showing N lines
        // to prevent visual jumping. Pad with empty │ lines if needed.
        let current_line_count = formatted_lines.len();
        let prev_max = self.max_context_lines_shown.load(Ordering::Relaxed);
        let new_max = current_line_count.max(prev_max);

        if current_line_count > prev_max {
            self.max_context_lines_shown
                .store(new_max, Ordering::Relaxed);
        }

        // Pad with empty placeholder lines to maintain stable height
        while formatted_lines.len() < new_max {
            let placeholder = if use_color {
                "    │ \x1b[2m\x1b[0m".to_string()
            } else {
                "    │".to_string()
            };
            formatted_lines.push(placeholder);
        }

        if formatted_lines.is_empty() {
            // No content and no high water mark - keep message empty (zero height)
            bar.set_message("");
        } else {
            // Join lines with newlines for multi-line display.
            // No visibility toggle needed - bar is always visible, just with varying content.
            bar.set_message(formatted_lines.join("\n"));
        }
    }

    /// Finish the progress group with success status.
    ///
    /// Updates the spinner to show a checkmark and clears the context bar.
    ///
    /// # Arguments
    ///
    /// * `label` - The check label to display
    /// * `duration` - How long the check took to run
    /// * `label_padding` - Width for label padding
    /// * `use_color` - Whether to apply ANSI color styling
    pub fn finish_success(
        &self,
        label: &str,
        duration: Duration,
        label_padding: usize,
        use_color: bool,
    ) {
        self.spinner.set_style(
            ProgressStyle::default_spinner()
                .template("  {msg}")
                .expect("valid template"),
        );
        self.spinner.finish_with_message(format_result_line(
            true,
            label,
            duration,
            label_padding,
            use_color,
        ));

        // Clear context bar - first set to empty message to ensure proper line clearing
        if let Some(bar) = &self.context_bar {
            bar.set_message("");
            bar.finish_and_clear();
        }
    }

    /// Finish the progress group with failure status.
    ///
    /// Updates the spinner to show an X and clears the context bar.
    ///
    /// # Arguments
    ///
    /// * `label` - The check label to display
    /// * `duration` - How long the check took to run
    /// * `label_padding` - Width for label padding
    /// * `use_color` - Whether to apply ANSI color styling
    pub fn finish_failure(
        &self,
        label: &str,
        duration: Duration,
        label_padding: usize,
        use_color: bool,
    ) {
        self.spinner.set_style(
            ProgressStyle::default_spinner()
                .template("  {msg}")
                .expect("valid template"),
        );
        self.spinner.finish_with_message(format_result_line(
            false,
            label,
            duration,
            label_padding,
            use_color,
        ));

        // Clear context bar - first set to empty message to ensure proper line clearing
        if let Some(bar) = &self.context_bar {
            bar.set_message("");
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_lines() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"First line\n");
        buffer.feed(b"Second line\n");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "First line");
        assert_eq!(snapshot[2], "Second line");
    }

    #[test]
    fn test_carriage_return() {
        let buffer = OutputBuffer::new(3);

        // Simulate a progress bar that uses \r to overwrite
        buffer.feed(b"Progress: 50%\rProgress: 100%\n");

        let snapshot = buffer.snapshot();
        // Only the final version after \r should be captured
        assert_eq!(snapshot[2], "Progress: 100%");
    }

    #[test]
    fn test_csi_clear_line() {
        let buffer = OutputBuffer::new(3);

        // ESC [ 2 K is the sequence for "clear entire line"
        buffer.feed(b"Old content\x1b[2KNew content\n");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[2], "New content");
    }

    #[test]
    fn test_rolling_window() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Line 1\nLine 2\nLine 3\nLine 4\nLine 5\n");

        let snapshot = buffer.snapshot();
        // Should only have the 3 most recent lines
        assert_eq!(snapshot[0], "Line 3");
        assert_eq!(snapshot[1], "Line 4");
        assert_eq!(snapshot[2], "Line 5");
    }

    #[test]
    fn test_partial_line() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Complete line\n");
        buffer.feed(b"Partial line without newline");

        let snapshot = buffer.snapshot();
        // Partial line (current_line) IS now included in snapshot (viewport semantics)
        assert_eq!(snapshot[1], "Complete line");
        assert_eq!(snapshot[2], "Partial line without newline");

        // After flush, behavior should be the same (line is committed)
        buffer.flush();
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[1], "Complete line");
        assert_eq!(snapshot[2], "Partial line without newline");
    }

    #[test]
    fn test_tab_expansion() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Col1\tCol2\n");

        let snapshot = buffer.snapshot();
        // Tab should be expanded to spaces
        assert_eq!(snapshot[2], "Col1    Col2");
    }

    #[test]
    fn test_empty_lines() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"\n\n\n");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "");
        assert_eq!(snapshot[2], "");
    }

    #[test]
    fn test_ansi_colors_preserved() {
        let buffer = OutputBuffer::new(3);

        // ANSI color codes should be passed through to the line content
        // ESC[32m = green, ESC[0m = reset
        buffer.feed(b"\x1b[32mGreen text\x1b[0m\n");

        let snapshot = buffer.snapshot();
        // The text should be present (colors are processed but text preserved)
        assert!(snapshot[2].contains("Green text"));
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let buffer = OutputBuffer::new(3);
        let buffer_clone = buffer.clone();

        let handle = thread::spawn(move || {
            for i in 0..100 {
                buffer_clone.feed(format!("Line {i}\n").as_bytes());
            }
        });

        // Main thread also reads
        for _ in 0..50 {
            let _ = buffer.snapshot();
        }

        handle.join().unwrap();

        // Final snapshot should have valid content
        let snapshot = buffer.snapshot();
        assert!(!snapshot[2].is_empty() || buffer.inner.lock().unwrap().lines.is_empty());
    }

    #[test]
    fn test_mixed_cr_lf() {
        let buffer = OutputBuffer::new(3);

        // Windows-style CRLF should work: \r followed by \n preserves content
        buffer.feed(b"Line 1\r\nLine 2\r\n");

        let snapshot = buffer.snapshot();
        // CRLF should preserve content (deferred CR consumed by LF without clearing)
        assert_eq!(snapshot[1], "Line 1");
        assert_eq!(snapshot[2], "Line 2");
    }

    #[test]
    fn test_capacity_one() {
        let buffer = OutputBuffer::new(1);

        buffer.feed(b"Line 1\nLine 2\nLine 3\n");

        let snapshot = buffer.snapshot();
        // With capacity 1, only the most recent line is kept
        // snapshot() returns [3] array, so earlier slots are empty
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "");
        assert_eq!(snapshot[2], "Line 3");
    }

    #[test]
    fn test_silence_duration_tracking() {
        use std::thread;
        use std::time::Duration;

        let buffer = OutputBuffer::new(3);

        // Just created, silence duration should be near zero
        let initial = buffer.silence_duration();
        assert!(initial < Duration::from_millis(50));

        // Wait a bit
        thread::sleep(Duration::from_millis(100));

        // Should now be at least 100ms
        let after_wait = buffer.silence_duration();
        assert!(after_wait >= Duration::from_millis(100));

        // Feed some data
        buffer.feed(b"New output\n");

        // Silence duration should reset to near zero
        let after_feed = buffer.silence_duration();
        assert!(after_feed < Duration::from_millis(50));
    }

    #[test]
    fn test_silence_duration_empty_feed_no_reset() {
        use std::thread;
        use std::time::Duration;

        let buffer = OutputBuffer::new(3);

        // Wait a bit
        thread::sleep(Duration::from_millis(100));

        // Feed empty bytes (should NOT reset the timer)
        buffer.feed(b"");

        // Silence duration should still be >= 100ms
        let after_empty_feed = buffer.silence_duration();
        assert!(after_empty_feed >= Duration::from_millis(100));
    }

    #[test]
    fn test_unicode_line_separator() {
        let buffer = OutputBuffer::new(3);
        // \u{2028} is Line Separator, \u{2029} is Paragraph Separator
        // Both should trigger a newline commit
        buffer.feed("Line 1\u{2028}Line 2\u{2029}Line 3\u{2028}".as_bytes());

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "Line 1");
        assert_eq!(snapshot[1], "Line 2");
        assert_eq!(snapshot[2], "Line 3");
    }

    // ========================================================================
    // Tests for viewport semantics (current_line visibility)
    // See VISION.md for the mental model these tests verify.
    // ========================================================================

    #[test]
    fn test_carriage_return_visible_in_snapshot() {
        let buffer = OutputBuffer::new(3);

        // Simulate cargo-style progress bar that uses \r to update in place
        buffer.feed(b"Building [=>  ] 1/10\r");
        buffer.feed(b"Building [===>] 5/10\r");
        buffer.feed(b"Building [====] 10/10");

        let snapshot = buffer.snapshot();
        // The current line should be visible (bottom of viewport)
        assert!(snapshot[2].contains("Building"));
        assert!(snapshot[2].contains("10/10"));
    }

    #[test]
    fn test_viewport_shows_current_line() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Line 1\n");
        buffer.feed(b"Line 2\n");
        buffer.feed(b"In progress..."); // No newline - this is current_line

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "Line 1");
        assert_eq!(snapshot[1], "Line 2");
        assert_eq!(snapshot[2], "In progress...");
    }

    #[test]
    fn test_viewport_after_newline() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Line 1\n");
        buffer.feed(b"Line 2\n");
        buffer.feed(b"Line 3\n"); // Cursor now on empty new line

        let snapshot = buffer.snapshot();
        // current_line is empty, so show last 3 committed
        assert_eq!(snapshot[0], "Line 1");
        assert_eq!(snapshot[1], "Line 2");
        assert_eq!(snapshot[2], "Line 3");
    }

    #[test]
    fn test_viewport_only_current_line() {
        let buffer = OutputBuffer::new(3);

        // Only current_line, no committed lines
        buffer.feed(b"Just typing...");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "");
        assert_eq!(snapshot[2], "Just typing...");
    }

    #[test]
    fn test_viewport_one_committed_plus_current() {
        let buffer = OutputBuffer::new(3);

        buffer.feed(b"Committed line\n");
        buffer.feed(b"Current line");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "Committed line");
        assert_eq!(snapshot[2], "Current line");
    }

    #[test]
    fn test_viewport_progress_bar_never_newlines() {
        let buffer = OutputBuffer::new(3);

        // Simulate a progress bar that never emits \n
        buffer.feed(b"[          ] 0%\r");
        buffer.feed(b"[==        ] 20%\r");
        buffer.feed(b"[====      ] 40%\r");
        buffer.feed(b"[======    ] 60%\r");
        buffer.feed(b"[========  ] 80%\r");
        buffer.feed(b"[==========] 100%");

        let snapshot = buffer.snapshot();
        // Should show the final state even though no \n was ever emitted
        assert!(snapshot[2].contains("100%"));
        // Earlier slots should be empty (no committed lines)
        assert_eq!(snapshot[0], "");
        assert_eq!(snapshot[1], "");
    }

    #[test]
    fn test_viewport_cargo_style_mixed() {
        let buffer = OutputBuffer::new(3);

        // Cargo emits "Compiling..." lines with \n, then a progress bar with \r
        buffer.feed(b"   Compiling foo v0.1.0\n");
        buffer.feed(b"   Compiling bar v0.1.0\n");
        buffer.feed(b"    Building [======>    ] 150/300: baz\r");
        buffer.feed(b"    Building [========>  ] 200/300: baz\r");
        buffer.feed(b"    Building [==========>] 300/300: baz");

        let snapshot = buffer.snapshot();
        // Should show 2 committed lines + current line
        assert!(snapshot[0].contains("Compiling foo"));
        assert!(snapshot[1].contains("Compiling bar"));
        assert!(snapshot[2].contains("300/300"));
    }

    // ========================================================================
    // Tests for OSC sequence handling
    // ========================================================================

    #[test]
    fn test_osc_11_response_bel_terminated() {
        let buffer = OutputBuffer::new(3);

        // OSC 11 response with BEL terminator (the bug we're seeing)
        buffer.feed(b"\x1b]11;rgb:0000/0000/0000\x07info: cargo-llvm-cov currently...\n");

        let snapshot = buffer.snapshot();
        // The OSC sequence should NOT appear in the visible output
        assert!(
            !snapshot[2].contains("rgb:"),
            "OSC response should be filtered, got: {:?}",
            snapshot[2]
        );
        assert!(
            snapshot[2].contains("info:"),
            "Legitimate text should be preserved, got: {:?}",
            snapshot[2]
        );
    }

    #[test]
    fn test_osc_11_response_st_terminated() {
        let buffer = OutputBuffer::new(3);

        // OSC 11 response with ST terminator (ESC \)
        buffer.feed(b"\x1b]11;rgb:0000/0000/0000\x1b\\info: cargo-llvm-cov currently...\n");

        let snapshot = buffer.snapshot();
        assert!(
            !snapshot[2].contains("rgb:"),
            "OSC response should be filtered, got: {:?}",
            snapshot[2]
        );
        assert!(
            snapshot[2].contains("info:"),
            "Legitimate text should be preserved, got: {:?}",
            snapshot[2]
        );
    }

    #[test]
    fn test_osc_11_response_split_across_feeds() {
        let buffer = OutputBuffer::new(3);

        // Simulate OSC sequence split across multiple reads (likely the real bug)
        buffer.feed(b"\x1b]11;rgb:0000/0000");
        buffer.feed(b"/0000\x07info: cargo-llvm-cov currently...\n");

        let snapshot = buffer.snapshot();
        assert!(
            !snapshot[2].contains("rgb:"),
            "Split OSC response should be filtered, got: {:?}",
            snapshot[2]
        );
        assert!(
            snapshot[2].contains("info:"),
            "Legitimate text should be preserved after split OSC, got: {:?}",
            snapshot[2]
        );
    }

    #[test]
    fn test_osc_sequence_very_fragmented() {
        let buffer = OutputBuffer::new(3);

        // Extreme fragmentation - byte by byte
        for &byte in b"\x1b]11;rgb:0000/0000/0000\x07" {
            buffer.feed(&[byte]);
        }
        buffer.feed(b"info: cargo\n");

        let snapshot = buffer.snapshot();
        assert!(
            !snapshot[2].contains("rgb:"),
            "Byte-by-byte OSC should be filtered, got: {:?}",
            snapshot[2]
        );
    }

    #[test]
    fn test_vte_osc_with_exact_reported_sequence() {
        let buffer = OutputBuffer::new(3);

        // The exact sequence from the bug report (ST terminator)
        // ^[]11;rgb:0000/0000/0000^[\info:
        // which is: ESC ] 11 ; rgb:0000/0000/0000 ESC \ info:
        let bytes = b"\x1b]11;rgb:0000/0000/0000\x1b\\info: cargo-llvm-cov currently...\n";
        buffer.feed(bytes);

        let snapshot = buffer.snapshot();
        eprintln!("Snapshot: {:?}", snapshot);
        assert!(
            !snapshot[2].contains("11;rgb"),
            "OSC sequence should be consumed by VTE, got: {:?}",
            snapshot[2]
        );
    }

    #[test]
    fn test_vte_osc_partial_midway_snapshot() {
        let buffer = OutputBuffer::new(3);

        // Feed the first part of an OSC sequence (no terminator yet)
        buffer.feed(b"\x1b]11;rgb:0000/0000");

        // Take a snapshot while OSC is incomplete
        let snapshot_midway = buffer.snapshot();
        eprintln!("Midway snapshot: {:?}", snapshot_midway);

        // The incomplete OSC should NOT appear in the snapshot
        // because VTE is buffering it internally
        let any_has_rgb = snapshot_midway.iter().any(|s| s.contains("rgb:"));
        assert!(
            !any_has_rgb,
            "Incomplete OSC should not appear in snapshot, got: {:?}",
            snapshot_midway
        );

        // Complete the sequence
        buffer.feed(b"/0000\x07info: cargo\n");
        let snapshot_final = buffer.snapshot();
        eprintln!("Final snapshot: {:?}", snapshot_final);

        assert!(
            snapshot_final[2].contains("info:"),
            "Final text should appear after OSC completes"
        );
    }

    #[test]
    fn test_vte_parser_directly() {
        use vte::{Parser, Perform};

        struct TestPerformer {
            printed: Vec<char>,
            osc_count: usize,
        }

        impl Perform for TestPerformer {
            fn print(&mut self, c: char) {
                eprintln!("VTE print: {:?}", c);
                self.printed.push(c);
            }
            fn execute(&mut self, _byte: u8) {}
            fn hook(
                &mut self,
                _params: &vte::Params,
                _intermediates: &[u8],
                _ignore: bool,
                _action: char,
            ) {
            }
            fn put(&mut self, _byte: u8) {}
            fn unhook(&mut self) {}
            fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
                eprintln!("VTE osc_dispatch: {:?}", params);
                self.osc_count += 1;
            }
            fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
            fn csi_dispatch(
                &mut self,
                _params: &vte::Params,
                _intermediates: &[u8],
                _ignore: bool,
                _action: char,
            ) {
            }
        }

        let mut parser = Parser::new();
        let mut performer = TestPerformer {
            printed: Vec::new(),
            osc_count: 0,
        };

        // Feed the exact sequence from the bug report
        let bytes = b"\x1b]11;rgb:0000/0000/0000\x1b\\info: cargo\n";
        eprintln!("Feeding {} bytes: {:?}", bytes.len(), bytes);

        for &byte in bytes {
            parser.advance(&mut performer, byte);
        }

        eprintln!("Printed chars: {:?}", performer.printed);
        eprintln!("OSC count: {}", performer.osc_count);

        assert_eq!(
            performer.osc_count, 1,
            "Should have received 1 OSC dispatch"
        );
        let printed_str: String = performer.printed.iter().collect();
        eprintln!("Printed string: {:?}", printed_str);
        assert!(
            !printed_str.contains("rgb:"),
            "OSC content should not be printed, got: {:?}",
            printed_str
        );
    }

    #[test]
    fn test_sanitize_caret_osc_11_st_terminator() {
        let input = "^[]11;rgb:0000/0000/0000^[\\info: cargo-llvm-cov";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "info: cargo-llvm-cov");
    }

    #[test]
    fn test_sanitize_caret_osc_11_bel_terminator() {
        let input = "^[]11;rgb:0000/0000/0000^Ginfo: cargo-llvm-cov";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "info: cargo-llvm-cov");
    }

    #[test]
    fn test_sanitize_caret_osc_10_st_terminator() {
        let input = "^[]10;rgb:ffff/ffff/ffff^[\\hello world";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_sanitize_csi_cpr_response() {
        let input = "^[[1;1Rhello world";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_sanitize_no_caret_fast_path() {
        let input = "hello world";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_sanitize_multiple_osc_sequences() {
        let input = "^[]10;rgb:ffff/ffff/ffff^[\\^[]11;rgb:0000/0000/0000^[\\real text";
        let result = super::sanitize_caret_osc(input);
        assert_eq!(result, "real text");
    }

    #[test]
    fn test_snapshot_sanitizes_caret_osc() {
        let buffer = OutputBuffer::new(3);

        // Simulate fragmented caret OSC that slipped through filtering
        // This might happen when the sequence is split across chunk boundaries
        buffer.feed(b"^[]11;rgb:0000/0000/0000^[\\info: cargo-llvm-cov currently...\n");

        let snapshot = buffer.snapshot();
        eprintln!("Snapshot: {:?}", snapshot);

        // The sanitization in snapshot() should have removed the caret OSC
        assert!(
            !snapshot[2].contains("^[]"),
            "Caret OSC should be sanitized from snapshot, got: {:?}",
            snapshot[2]
        );
        assert!(
            snapshot[2].contains("info: cargo-llvm-cov"),
            "Real text should remain after sanitization"
        );
    }
}
