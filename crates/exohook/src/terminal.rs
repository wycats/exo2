//! Terminal width detection and adaptive configuration.
//!
//! This module provides terminal width detection and tier-based configuration
//! for the streaming progress UI. It allows the UI to gracefully adapt from
//! wide terminals (100+ cols) down to compact terminals (30+ cols).
//!
//! # Dependency Injection
//!
//! For testing, implement the [`TerminalDetector`] trait to provide mock
//! terminal dimensions. Use [`TerminalConfig::with_detector`] to inject
//! a custom detector.

use console::{Term, measure_text_width, truncate_str};
use std::sync::atomic::{AtomicU16, Ordering};

/// Thread-safe terminal width that can be updated on SIGWINCH.
static TERMINAL_WIDTH: AtomicU16 = AtomicU16::new(80);

/// Trait for terminal dimension detection.
///
/// Implement this trait to provide custom terminal detection logic for testing
/// or specialized environments.
pub trait TerminalDetector: Send + Sync {
    /// Return the current terminal width in columns.
    fn width(&self) -> u16;
}

/// Default terminal detector using the `console` crate.
///
/// This is the production implementation that queries the actual terminal.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleDetector;

impl TerminalDetector for ConsoleDetector {
    fn width(&self) -> u16 {
        let (_, cols) = Term::stdout().size();
        cols
    }
}

/// Fixed-width detector for testing.
///
/// This type is only available in test and documentation builds.
/// Use it in unit tests to mock terminal dimensions.
///
/// # Example (test context only)
///
/// ```ignore
/// use exohook::terminal::{FixedWidthDetector, TerminalConfig};
///
/// let config = TerminalConfig::with_detector(&FixedWidthDetector(50));
/// assert_eq!(config.width, 50);
/// ```
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct FixedWidthDetector(pub u16);

impl TerminalDetector for FixedWidthDetector {
    fn width(&self) -> u16 {
        self.0
    }
}

/// Configuration for terminal-adaptive rendering.
#[derive(Debug, Clone)]
pub struct TerminalConfig {
    /// Detected terminal width in columns
    pub width: u16,
    /// Width tier category
    pub tier: WidthTier,
    /// Label padding (fixed-width space for check names)
    pub label_padding: usize,
    /// Maximum width for truncated output lines
    pub truncation_limit: usize,
    /// Whether to show context lines (disabled in narrow/compact modes)
    pub show_context_lines: bool,
    /// Whether to use compact progress indicator instead of text
    pub use_compact_indicator: bool,
}

/// Terminal width tiers that determine which UI features are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthTier {
    /// Wide terminals (≥100 columns): Full features with generous spacing
    Wide,
    /// Normal terminals (60–99 columns): All features with reduced padding
    Normal,
    /// Narrow terminals (40–59 columns): Compact labels, no context lines
    Narrow,
    /// Compact terminals (30–39 columns): Minimal display with symbols
    Compact,
}

impl TerminalConfig {
    /// Detect terminal width and create appropriate configuration.
    ///
    /// Checks `EXOHOOK_COLUMNS` environment variable first for testing/override,
    /// then queries the actual terminal size using the default [`ConsoleDetector`].
    pub fn detect() -> Self {
        Self::with_detector(&ConsoleDetector)
    }

    /// Create configuration using a custom terminal detector.
    ///
    /// This is the dependency injection entry point for testing. Pass any
    /// implementation of [`TerminalDetector`] to control the detected width.
    ///
    /// Also respects the `EXOHOOK_COLUMNS` environment variable, which takes
    /// precedence over the detector.
    ///
    /// # Example
    ///
    /// ```
    /// use exohook::terminal::{FixedWidthDetector, TerminalConfig, WidthTier};
    ///
    /// let config = TerminalConfig::with_detector(&FixedWidthDetector(50));
    /// assert_eq!(config.tier, WidthTier::Narrow);
    /// ```
    pub fn with_detector(detector: &dyn TerminalDetector) -> Self {
        let width = std::env::var("EXOHOOK_COLUMNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| detector.width());

        // Store in atomic for resize handler
        TERMINAL_WIDTH.store(width, Ordering::Relaxed);

        Self::from_width(width)
    }

    /// Create configuration for CI/non-TTY environments.
    ///
    /// Returns a sensible default (80 columns, Normal tier).
    pub fn for_ci() -> Self {
        Self::from_width(80)
    }

    /// Get current width from the atomic (may have been updated by resize handler).
    pub fn current_width() -> u16 {
        TERMINAL_WIDTH.load(Ordering::Relaxed)
    }

    /// Set the terminal width (called by resize handler or for testing).
    pub fn set_width(width: u16) {
        TERMINAL_WIDTH.store(width, Ordering::Relaxed);
    }

    /// Register SIGWINCH handler to update terminal width on resize (Unix only).
    ///
    /// Spawns a background thread that listens for SIGWINCH signals and updates
    /// the global TERMINAL_WIDTH atomic when the terminal is resized.
    #[cfg(unix)]
    pub fn register_resize_handler() -> Result<(), std::io::Error> {
        use signal_hook::consts::SIGWINCH;
        use signal_hook::iterator::Signals;
        use std::thread;

        let mut signals = Signals::new([SIGWINCH])?;

        thread::spawn(move || {
            for sig in signals.forever() {
                if sig == SIGWINCH {
                    // Re-detect terminal width
                    let (_, cols) = Term::stdout().size();
                    TERMINAL_WIDTH.store(cols, Ordering::Relaxed);
                }
            }
        });

        Ok(())
    }

    /// Register SIGWINCH handler (no-op on non-Unix platforms).
    #[cfg(not(unix))]
    pub fn register_resize_handler() -> Result<(), std::io::Error> {
        Ok(())
    }

    /// Create configuration from a specific width.
    ///
    /// Calculates tier, padding, and truncation limits based on the width.
    pub fn from_width(width: u16) -> Self {
        let tier = match width {
            w if w >= 100 => WidthTier::Wide,
            w if w >= 60 => WidthTier::Normal,
            w if w >= 40 => WidthTier::Narrow,
            _ => WidthTier::Compact,
        };

        // Label padding: clamp(width / 4, 10, 30)
        let label_padding = ((width / 4) as usize).clamp(10, 30);

        // Truncation limit: width - 10 (leaving margin for prefix/suffix)
        let truncation_limit = (width as usize).saturating_sub(10);

        // Show context lines only for Wide and Normal tiers
        let show_context_lines = matches!(tier, WidthTier::Wide | WidthTier::Normal);

        // Use compact indicator only for Compact tier
        let use_compact_indicator = matches!(tier, WidthTier::Compact);

        Self {
            width,
            tier,
            label_padding,
            truncation_limit,
            show_context_lines,
            use_compact_indicator,
        }
    }
}

/// Format a compact progress indicator for Compact mode terminals.
///
/// Returns an elapsed time indicator like `[12s]` or `[2m]` that fits
/// in very narrow terminals where truncated output would be useless.
///
/// # Arguments
///
/// * `elapsed_secs` - Elapsed time in seconds
///
/// # Returns
///
/// A compact string like `[5s]`, `[45s]`, `[2m]`, `[1h]`
pub fn compact_progress_indicator(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("[{}s]", elapsed_secs)
    } else if elapsed_secs < 3600 {
        format!("[{}m]", elapsed_secs / 60)
    } else {
        format!("[{}h]", elapsed_secs / 3600)
    }
}

/// Helper function for consistent truncation across the codebase.
///
/// Truncates a line to `max_width` characters, appending "..." if truncated.
///
/// # Arguments
///
/// * `line` - The line to truncate
/// * `max_width` - Maximum width in characters (not bytes)
///
/// # Returns
///
/// The truncated string with "..." suffix if it exceeded max_width,
/// or the original string if it fit within the limit.
pub fn truncate_for_display(line: &str, max_width: usize) -> String {
    if measure_text_width(line) > max_width {
        truncate_str(line, max_width, "...").to_string()
    } else {
        line.to_string()
    }
}

/// Duration thresholds for visual feedback
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationCategory {
    /// Fast: under 1 second - dim/normal
    Fast,
    /// Normal: 1-5 seconds - default
    Normal,
    /// Slow: 5-10 seconds - yellow/highlighted
    Slow,
    /// VerySlow: over 10 seconds - bold yellow
    VerySlow,
}

impl DurationCategory {
    /// Categorize a duration for visual feedback
    pub fn from_duration(d: std::time::Duration) -> Self {
        let secs = d.as_secs_f64();
        if secs < 1.0 {
            DurationCategory::Fast
        } else if secs < 5.0 {
            DurationCategory::Normal
        } else if secs < 10.0 {
            DurationCategory::Slow
        } else {
            DurationCategory::VerySlow
        }
    }
}

/// Format a duration for display with consistent alignment.
///
/// Returns a right-aligned duration string with appropriate unit.
/// The width is fixed at 7 characters (e.g., "  0.4s", " 24.6s", "  120s").
pub fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 10.0 {
        // "  0.4s" format for sub-10s
        format!("{:>5.1}s", secs)
    } else if secs < 100.0 {
        // " 24.6s" format for 10-99s
        format!("{:>5.1}s", secs)
    } else {
        // "  120s" format for 100s+
        format!("{:>5.0}s", secs)
    }
}

/// Format a complete result line with proper alignment and optional coloring.
///
/// Creates a consistently formatted line like:
/// ```text
///   ✓ Check           ok    4.4s
///   ✗ Rust Clippy     FAIL  7.5s
/// ```
///
/// # Arguments
///
/// * `success` - Whether the check passed
/// * `label` - The check label to display
/// * `duration` - How long the check took
/// * `label_padding` - Width for the label column
/// * `use_color` - Whether to apply ANSI colors
pub fn format_result_line(
    success: bool,
    label: &str,
    duration: std::time::Duration,
    label_padding: usize,
    use_color: bool,
) -> String {
    let duration_str = format_duration(duration);
    let category = DurationCategory::from_duration(duration);

    if use_color {
        let (icon, status) = if success {
            ("\x1b[32m✓\x1b[0m", "\x1b[32mok\x1b[0m") // green
        } else {
            ("\x1b[31m✗\x1b[0m", "\x1b[31;1mFAIL\x1b[0m") // red, bold
        };

        // Color the duration based on category
        let colored_duration = match category {
            DurationCategory::Fast => format!("\x1b[2m{}\x1b[0m", duration_str), // dim
            DurationCategory::Normal => duration_str,
            DurationCategory::Slow => format!("\x1b[33m{}\x1b[0m", duration_str), // yellow
            DurationCategory::VerySlow => format!("\x1b[33;1m{}\x1b[0m", duration_str), // bold yellow
        };

        format!(
            "{} {:<width$} {:>4}  {}",
            icon,
            label,
            status,
            colored_duration,
            width = label_padding
        )
    } else {
        let (icon, status) = if success {
            ("✓", "ok")
        } else {
            ("✗", "FAIL")
        };
        format!(
            "{} {:<width$} {:>4}  {}",
            icon,
            label,
            status,
            duration_str,
            width = label_padding
        )
    }
}

/// Format a lane summary line with proper styling.
///
/// Creates a summary like:
/// ```text
/// ✓ dev: all 8 checks passed in 24.6s
/// ✗ dev: 2 of 8 checks failed in 45.2s
/// ```
pub fn format_lane_summary(
    lane: &str,
    passed: usize,
    total: usize,
    duration: std::time::Duration,
    use_color: bool,
) -> String {
    let all_passed = passed == total;
    let duration_str = format_duration(duration);

    if use_color {
        if all_passed {
            format!(
                "\x1b[32m✓\x1b[0m \x1b[1m{}\x1b[0m: all {} checks passed in \x1b[2m{}\x1b[0m",
                lane, total, duration_str
            )
        } else {
            let failed = total - passed;
            format!(
                "\x1b[31m✗\x1b[0m \x1b[1m{}\x1b[0m: \x1b[31;1m{}\x1b[0m of {} checks failed in {}",
                lane, failed, total, duration_str
            )
        }
    } else if all_passed {
        format!(
            "✓ {}: all {} checks passed in {}",
            lane, total, duration_str
        )
    } else {
        let failed = total - passed;
        format!(
            "✗ {}: {} of {} checks failed in {}",
            lane, failed, total, duration_str
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_width_tiers() {
        assert_eq!(TerminalConfig::from_width(120).tier, WidthTier::Wide);
        assert_eq!(TerminalConfig::from_width(100).tier, WidthTier::Wide);
        assert_eq!(TerminalConfig::from_width(99).tier, WidthTier::Normal);
        assert_eq!(TerminalConfig::from_width(60).tier, WidthTier::Normal);
        assert_eq!(TerminalConfig::from_width(59).tier, WidthTier::Narrow);
        assert_eq!(TerminalConfig::from_width(40).tier, WidthTier::Narrow);
        assert_eq!(TerminalConfig::from_width(39).tier, WidthTier::Compact);
        assert_eq!(TerminalConfig::from_width(30).tier, WidthTier::Compact);
    }

    #[test]
    fn test_label_padding() {
        // Wide: width/4 = 120/4 = 30, min(30, max(10, 30)) = 30
        assert_eq!(TerminalConfig::from_width(120).label_padding, 30);

        // Normal: width/4 = 80/4 = 20, min(30, max(10, 20)) = 20
        assert_eq!(TerminalConfig::from_width(80).label_padding, 20);

        // Narrow: width/4 = 50/4 = 12, min(30, max(10, 12)) = 12
        assert_eq!(TerminalConfig::from_width(50).label_padding, 12);

        // Compact: width/4 = 35/4 = 8, min(30, max(10, 8)) = 10
        assert_eq!(TerminalConfig::from_width(35).label_padding, 10);
    }

    #[test]
    fn test_truncation_limit() {
        assert_eq!(TerminalConfig::from_width(80).truncation_limit, 70);
        assert_eq!(TerminalConfig::from_width(50).truncation_limit, 40);
        assert_eq!(TerminalConfig::from_width(30).truncation_limit, 20);
    }

    #[test]
    fn test_show_context_lines() {
        assert!(TerminalConfig::from_width(100).show_context_lines); // Wide
        assert!(TerminalConfig::from_width(80).show_context_lines); // Normal
        assert!(!TerminalConfig::from_width(50).show_context_lines); // Narrow
        assert!(!TerminalConfig::from_width(35).show_context_lines); // Compact
    }

    #[test]
    fn test_truncate_for_display() {
        assert_eq!(truncate_for_display("short", 10), "short");
        assert_eq!(truncate_for_display("exactly ten", 11), "exactly ten");
        assert_eq!(truncate_for_display("this is too long", 10), "this is...");
        assert_eq!(truncate_for_display("", 10), "");
    }

    #[test]
    fn test_truncate_unicode() {
        // Unicode characters should be measured by display width (2 columns for CJK)
        assert_eq!(truncate_for_display("こんにちは", 10), "こんにちは");
        // Width 14 > 10. Truncates.
        assert_eq!(truncate_for_display("こんにちは世界", 10), "こんに...");
    }

    #[test]
    fn test_for_ci() {
        let config = TerminalConfig::for_ci();
        assert_eq!(config.width, 80);
        assert_eq!(config.tier, WidthTier::Normal);
    }

    #[test]
    fn test_current_width_and_set_width() {
        // Store original to restore after test
        let original = TerminalConfig::current_width();
        let original_env = std::env::var("EXOHOOK_COLUMNS").ok();
        unsafe {
            std::env::remove_var("EXOHOOK_COLUMNS");
        }

        let result = std::panic::catch_unwind(|| {
            TerminalConfig::set_width(120);
            assert_eq!(TerminalConfig::current_width(), 120);

            TerminalConfig::set_width(50);
            assert_eq!(TerminalConfig::current_width(), 50);
        });

        // Restore original
        TerminalConfig::set_width(original);
        unsafe {
            if let Some(value) = original_env {
                std::env::set_var("EXOHOOK_COLUMNS", value);
            }
        }
        if let Err(err) = result {
            std::panic::resume_unwind(err);
        }
    }

    #[test]
    fn test_with_detector_fixed_width() {
        // Test the DI approach with FixedWidthDetector
        let detector = FixedWidthDetector(50);
        let config = TerminalConfig::with_detector(&detector);
        assert_eq!(config.width, 50);
        assert_eq!(config.tier, WidthTier::Narrow);
        assert!(!config.show_context_lines);
    }

    #[test]
    fn test_with_detector_all_tiers() {
        // Wide
        let config = TerminalConfig::with_detector(&FixedWidthDetector(120));
        assert_eq!(config.tier, WidthTier::Wide);
        assert!(config.show_context_lines);

        // Normal
        let config = TerminalConfig::with_detector(&FixedWidthDetector(80));
        assert_eq!(config.tier, WidthTier::Normal);
        assert!(config.show_context_lines);

        // Narrow
        let config = TerminalConfig::with_detector(&FixedWidthDetector(50));
        assert_eq!(config.tier, WidthTier::Narrow);
        assert!(!config.show_context_lines);

        // Compact
        let config = TerminalConfig::with_detector(&FixedWidthDetector(35));
        assert_eq!(config.tier, WidthTier::Compact);
        assert!(!config.show_context_lines);
    }

    #[test]
    fn test_custom_detector_implementation() {
        // Test that custom implementations work
        use std::sync::atomic::{AtomicU16, Ordering};

        struct MockDetector {
            width: AtomicU16,
        }

        impl TerminalDetector for MockDetector {
            fn width(&self) -> u16 {
                self.width.load(Ordering::Relaxed)
            }
        }

        let detector = MockDetector {
            width: AtomicU16::new(100),
        };

        assert_eq!(
            TerminalConfig::with_detector(&detector).tier,
            WidthTier::Wide
        );

        detector.width.store(60, Ordering::Relaxed);
        assert_eq!(
            TerminalConfig::with_detector(&detector).tier,
            WidthTier::Normal
        );

        detector.width.store(40, Ordering::Relaxed);
        assert_eq!(
            TerminalConfig::with_detector(&detector).tier,
            WidthTier::Narrow
        );
    }

    #[test]
    fn test_tier_boundary_values() {
        // Test exact boundary values
        assert_eq!(TerminalConfig::from_width(100).tier, WidthTier::Wide);
        assert_eq!(TerminalConfig::from_width(99).tier, WidthTier::Normal);
        assert_eq!(TerminalConfig::from_width(60).tier, WidthTier::Normal);
        assert_eq!(TerminalConfig::from_width(59).tier, WidthTier::Narrow);
        assert_eq!(TerminalConfig::from_width(40).tier, WidthTier::Narrow);
        assert_eq!(TerminalConfig::from_width(39).tier, WidthTier::Compact);
        assert_eq!(TerminalConfig::from_width(30).tier, WidthTier::Compact);
        assert_eq!(TerminalConfig::from_width(1).tier, WidthTier::Compact);
    }

    #[test]
    fn test_config_values_for_each_tier() {
        // Wide (120 columns)
        let wide = TerminalConfig::from_width(120);
        assert_eq!(wide.label_padding, 30); // 120/4 = 30
        assert_eq!(wide.truncation_limit, 110); // 120 - 10
        assert!(wide.show_context_lines);

        // Normal (80 columns)
        let normal = TerminalConfig::from_width(80);
        assert_eq!(normal.label_padding, 20); // 80/4 = 20
        assert_eq!(normal.truncation_limit, 70); // 80 - 10
        assert!(normal.show_context_lines);

        // Narrow (50 columns)
        let narrow = TerminalConfig::from_width(50);
        assert_eq!(narrow.label_padding, 12); // 50/4 = 12
        assert_eq!(narrow.truncation_limit, 40); // 50 - 10
        assert!(!narrow.show_context_lines);
        assert!(!narrow.use_compact_indicator);

        // Compact (35 columns)
        let compact = TerminalConfig::from_width(35);
        assert_eq!(compact.label_padding, 10); // 35/4 = 8, clamped to 10
        assert_eq!(compact.truncation_limit, 25); // 35 - 10
        assert!(!compact.show_context_lines);
        assert!(compact.use_compact_indicator);
    }

    #[test]
    fn test_use_compact_indicator() {
        assert!(!TerminalConfig::from_width(100).use_compact_indicator); // Wide
        assert!(!TerminalConfig::from_width(80).use_compact_indicator); // Normal
        assert!(!TerminalConfig::from_width(50).use_compact_indicator); // Narrow
        assert!(TerminalConfig::from_width(35).use_compact_indicator); // Compact
        assert!(TerminalConfig::from_width(30).use_compact_indicator); // Compact
    }

    #[test]
    fn test_compact_progress_indicator() {
        // Seconds
        assert_eq!(compact_progress_indicator(0), "[0s]");
        assert_eq!(compact_progress_indicator(5), "[5s]");
        assert_eq!(compact_progress_indicator(45), "[45s]");
        assert_eq!(compact_progress_indicator(59), "[59s]");

        // Minutes
        assert_eq!(compact_progress_indicator(60), "[1m]");
        assert_eq!(compact_progress_indicator(120), "[2m]");
        assert_eq!(compact_progress_indicator(3599), "[59m]");

        // Hours
        assert_eq!(compact_progress_indicator(3600), "[1h]");
        assert_eq!(compact_progress_indicator(7200), "[2h]");
    }

    #[test]
    fn test_duration_category() {
        use std::time::Duration;

        // Fast: under 1 second
        assert_eq!(
            DurationCategory::from_duration(Duration::from_millis(500)),
            DurationCategory::Fast
        );
        assert_eq!(
            DurationCategory::from_duration(Duration::from_millis(999)),
            DurationCategory::Fast
        );

        // Normal: 1-5 seconds
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(1)),
            DurationCategory::Normal
        );
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(4)),
            DurationCategory::Normal
        );

        // Slow: 5-10 seconds
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(5)),
            DurationCategory::Slow
        );
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(9)),
            DurationCategory::Slow
        );

        // VerySlow: over 10 seconds
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(10)),
            DurationCategory::VerySlow
        );
        assert_eq!(
            DurationCategory::from_duration(Duration::from_secs(60)),
            DurationCategory::VerySlow
        );
    }

    #[test]
    fn test_format_duration() {
        use std::time::Duration;

        // Sub-10 seconds: right-aligned with one decimal
        assert_eq!(format_duration(Duration::from_millis(400)), "  0.4s");
        assert_eq!(format_duration(Duration::from_secs(4)), "  4.0s");
        assert_eq!(format_duration(Duration::from_millis(9900)), "  9.9s");

        // 10-99 seconds
        assert_eq!(format_duration(Duration::from_secs(24)), " 24.0s");
        assert_eq!(format_duration(Duration::from_millis(24560)), " 24.6s");

        // 100+ seconds
        assert_eq!(format_duration(Duration::from_secs(120)), "  120s");
    }

    #[test]
    fn test_format_result_line_no_color() {
        use std::time::Duration;

        let result = format_result_line(true, "Check", Duration::from_secs(4), 20, false);
        // Should contain the icon, label, status, and duration
        assert!(result.contains("✓"));
        assert!(result.contains("Check"));
        assert!(result.contains("ok"));
        assert!(result.contains("4.0s"));

        let result = format_result_line(false, "Lint", Duration::from_secs(2), 20, false);
        assert!(result.contains("✗"));
        assert!(result.contains("Lint"));
        assert!(result.contains("FAIL"));
    }

    #[test]
    fn test_format_lane_summary_no_color() {
        use std::time::Duration;

        // All passed
        let result = format_lane_summary("dev", 8, 8, Duration::from_secs(25), false);
        assert!(result.contains("✓"));
        assert!(result.contains("dev"));
        assert!(result.contains("all 8 checks passed"));
        assert!(result.contains("25.0s"));

        // Some failed
        let result = format_lane_summary("dev", 6, 8, Duration::from_secs(45), false);
        assert!(result.contains("✗"));
        assert!(result.contains("dev"));
        assert!(result.contains("2 of 8 checks failed"));
    }
}
