<!-- exo:99 ulid:01kg5kp2fy0k186dy8fcadttn7 -->

# RFC 99: Exohook Adaptive Terminal Width


# RFC 0099: Exohook Adaptive Terminal Width

## Summary

Add adaptive terminal width detection to exohook's streaming progress UI, allowing it to gracefully degrade its display from full-featured wide output to compact narrow output, ensuring usability across terminal sizes from 30+ columns up to unlimited width. Support live resize via SIGWINCH.

## Motivation

The current exohook streaming UI has hard-coded width assumptions that cause visual breakage on narrow terminals:

- **PTY size** is fixed at 80 columns (`pty_runner.rs:64`)
- **Context line truncation** is hard-coded at 70 chars (`output_buffer.rs:355`, `main.rs:1665`, `main.rs:1839`)
- **Label padding** is fixed at 30 chars (`output_buffer.rs:314`, `output_buffer.rs:387`, `output_buffer.rs:413`, `main.rs:1655`, `main.rs:1830`)

This breaks usability in common scenarios:

1. **Split panes**: Developers often split their terminal vertically, resulting in 40-60 column windows
2. **Small monitors**: Laptop screens with limited real estate
3. **Embedded terminals**: VS Code's integrated terminal, IDE panels, floating terminal windows
4. **CI logs**: Some CI systems render logs in constrained widths
5. **Accessibility**: Users with vision preferences may use larger fonts, reducing effective columns

The minimum status line currently requires ~57 characters:

\`\`\`
● Rust fmt running... (12.3s, please wait)
^^ ^^^^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
2 30 chars (label) ~25 chars (status text)
\`\`\`

Without adaptation, narrow terminals see wrapped lines, misaligned spinners, and garbled output.

## Detailed Design

### Terminology

- **Width Tier**: A category of terminal width that determines which UI features are enabled
- **Label Padding**: Fixed-width space allocated for check names (e.g., "Rust fmt")
- **Context Lines**: Optional lines showing command output snippets below the status line
- **Compact Mode**: Narrow terminal mode with symbols/color but simplified progress display

### User Experience (UX)

Users experience automatic adaptation with no configuration required:

- **Wide terminals (≥100 cols)**: Full current behavior with generous truncation and context lines
- **Normal terminals (60–99 cols)**: Reduced label padding, proportional truncation, all features work
- **Narrow terminals (40–59 cols)**: Compact mode with shorter labels, context lines hidden
- **Compact terminals (30–39 cols)**: Symbols and color retained; output replaced with progress dots/spinner instead of truncated text that would be meaninglessly short

The adaptation is transparent—users simply see output that fits their terminal. If the terminal resizes mid-run, the UI adapts on the next update tick.

### Architecture

The change affects these components:

1. **\`terminal.rs\` (new module)**: Contains \`TerminalConfig\`, \`WidthTier\`, width detection, and SIGWINCH handling
2. **\`CheckProgressGroup\`**: Receives width configuration, adjusts label padding
3. **\`OutputBuffer\`**: Uses dynamic truncation limits via shared helper
4. **\`pty_runner\`**: Sets PTY size to match real terminal (with reasonable minimum ~40 cols)
5. **Main progress rendering**: Conditionally hides context lines, switches to compact mode

### Implementation Details

#### New Module: \`terminal.rs\`

\`\`\`rust
use std::sync::atomic::{AtomicU16, Ordering};
use console::Term;

/// Thread-safe terminal width that can be updated on SIGWINCH.
static TERMINAL_WIDTH: AtomicU16 = AtomicU16::new(80);

pub struct TerminalConfig {
pub width: u16,
pub tier: WidthTier,
pub label_padding: usize,
pub truncation_limit: usize,
pub show_context_lines: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthTier {
Wide, // ≥100
Normal, // 60–99
Narrow, // 40–59
Compact, // 30–39
}

impl TerminalConfig {
/// Detect terminal width and create config.
pub fn detect() -> Self {
// Check env override first
let width = std::env::var("EXOHOOK*COLUMNS")
.ok()
.and_then(|s| s.parse().ok())
.unwrap_or_else(|| {
let (*, cols) = Term::stdout().size();
cols
});
Self::from_width(width)
}

    /// Create config for CI/non-TTY environments.
    pub fn for_ci() -> Self {
        Self::from_width(80) // Sensible default
    }

    /// Get current width (may have changed due to resize).
    pub fn current_width() -> u16 {
        TERMINAL_WIDTH.load(Ordering::Relaxed)
    }

    fn from_width(width: u16) -> Self { /* ... */ }

}

/// Helper function for consistent truncation across codebase.
pub fn truncate_for_display(line: &str, max_width: usize) -> String {
if line.chars().count() > max_width {
let t: String = line.chars().take(max_width.saturating_sub(3)).collect();
format!("{}...", t)
} else {
line.to_string()
}
}
\`\`\`

#### SIGWINCH Handling (Unix)

\`\`\`rust
use signal_hook::flag;
use signal_hook::consts::SIGWINCH;

/// Register SIGWINCH handler to update TERMINAL_WIDTH.
pub fn register_resize_handler() -> Result<(), std::io::Error> {
// Use signal-hook's iterator or flag pattern
// Update TERMINAL_WIDTH atomic on each signal
}
\`\`\`

#### TerminalDetector DI Pattern (Testing)

Width detection uses a `TerminalDetector` trait to support deterministic tests:

- `RealTerminalDetector` — production implementation that reads the real terminal width
- `MockTerminalDetector` — test implementation with a fixed width

This allows width-dependent rendering tests without mocking the terminal environment.

#### Dynamic Calculations

- **Label padding**: \`min(30, max(10, term_width / 4))\`
- **Truncation limit**: \`term_width.saturating_sub(10)\`
- **PTY width**: \`max(term_width, 40)\` (floor at 40 to prevent tool breakage)

#### Tier Behavior

| Tier    | Width | Label Pad | Context | Output Display                     |
| ------- | ----- | --------- | ------- | ---------------------------------- |
| Wide    | ≥100  | 30        | Yes     | Truncated lines (generous)         |
| Normal  | 60–99 | 20-24     | Yes     | Truncated lines (proportional)     |
| Narrow  | 40–59 | 12-15     | No      | Truncated lines (tight)            |
| Compact | 30–39 | 10        | No      | Progress indicator instead of text |

#### Compact Mode Output

In Compact mode (<40 cols), showing "Com..." as truncated output is useless. Instead:

\`\`\`
● fmt ●●●○○
● test ●●●●○
\`\`\`

Or a simple elapsed indicator:

\`\`\`
● fmt [12s]
● test [3s]
\`\`\`

## Implementation Plan

### Phase 1: Foundation + Truncation (Low Risk) ✅

- [x] Create \`terminal.rs\` module with \`TerminalConfig\`, \`WidthTier\`
- [x] Add \`truncate_for_display()\` helper function
- [x] Replace 3 duplicated truncation sites with helper call
- [x] Add \`EXOHOOK_COLUMNS\` env var support
- [x] Add \`TerminalConfig::for_ci()\` for non-TTY detection
- [x] Thread config through to rendering code

### Phase 2: Label Padding + PTY + Resize ✅

- [x] Add \`signal-hook\` dependency for SIGWINCH
- [x] Implement resize handler with \`AtomicU16\` width
- [x] Replace 30-char label padding with \`config.label_padding\`
- [x] Update \`CheckProgressGroup::new()\` to accept config
- [x] Set PTY size from terminal width (floored at 40)
- [x] Re-query width on each update tick

### Phase 3: Graceful Degradation ✅

- [x] Add \`show_context_lines\` flag, hide context in Narrow tier
- [x] Implement Compact mode progress indicator (\`[12s]\` elapsed style)
- [x] Add integration tests for each tier (6 tests in \`terminal_width.rs\`)
- [x] Add \`TerminalDetector\` trait for DI in tests
- [x] Add \`MockTerminalDetector\` for test fixtures

## Dependencies

**New:**

- \`signal-hook = "0.3"\` — SIGWINCH handling (Unix only, already has good cross-platform story)

**Existing (already in use):**

- \`console = "0.15"\` — \`Term::stdout().size()\` for width detection

## Context Updates (Stage 3)

- [ ] Update the relevant stabilized RFCs with terminal adaptation details
- [ ] Update the relevant exohook architecture RFC with terminal.rs details

## Drawbacks

1. **Complexity**: Adds conditional logic paths that must be tested across tiers
2. **Behavioral change**: Users with narrow terminals will see different (though better) output
3. **Testing burden**: Need to verify each tier renders correctly
4. **Signal handling**: SIGWINCH adds Unix-specific code path

## Alternatives

### 1. Explicit \`--compact\` Flag

Let users manually toggle compact mode. Rejected because:

- Requires user awareness of the problem
- Doesn't scale to multiple width breakpoints
- Poor UX for dynamic terminal resizing

### 2. Proportional-Only (No Tiers)

Scale everything proportionally without discrete tiers. Rejected because:

- Some features (context lines) don't scale well—they're either useful or not
- Proportional scaling can produce awkward intermediate states

### 3. Detect-and-Warn

Detect narrow terminal and print a warning. Rejected because:

- Doesn't solve the problem, just acknowledges it
- Adds noise to output

### 4. Require Minimum Width

Refuse to run or force scrolling if terminal is too narrow. Rejected because:

- Hostile to users with constrained environments
- No graceful degradation path

### 5. Startup Detection Only (No Resize)

Detect width once at startup, ignore SIGWINCH. Rejected because:

- VS Code terminal pane resizing is a primary use case
- \`signal-hook\` makes this straightforward

## Resolved Questions

1. **TerminalConfig location**: New \`terminal.rs\` module ✓
2. **PTY width**: Dynamic with 40-col floor, support resize ✓
3. **Compact mode**: Symbols/color retained, progress indicator instead of useless truncation ✓
4. **Deduplication**: Extract to \`truncate_for_display()\` helper in Phase 1 ✓
5. **CI default**: \`TerminalConfig::for_ci()\` with 80-col default ✓
6. **Resize handling**: Use \`signal-hook\` crate for SIGWINCH ✓
7. **User override**: \`EXOHOOK_COLUMNS\` env var ✓

## Future Possibilities

1. **Width profiles**: Named profiles (e.g., "compact", "verbose") users can select
2. **Per-check width**: Allow individual checks to request more/less space
3. **TUI mode**: Full terminal UI with panels for very wide terminals
4. **Windows resize**: ConPTY resize events if/when we add Windows PTY support
