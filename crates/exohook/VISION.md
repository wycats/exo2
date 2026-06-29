# Exohook Vision

## Purpose

Exohook provides validation lane execution for git hooks, with **real-time progress feedback** that keeps humans and AI agents informed about what's happening. The key insight is that long-running checks (builds, tests, linting) **appear to hang** when they produce no visible output, leading to premature abandonment or confusion.

## Core Axiom: The Terminal Viewport

The central abstraction is a **terminal viewport emulator**. When displaying progress for a running check, we show the **last N lines as they would appear in a real terminal**.

This means:

- We parse output through VTE (Virtual Terminal Emulator) to handle escape sequences correctly
- We maintain a rolling window of visible lines (typically 3)
- **The current line the cursor is on is always part of the viewport**
- Carriage return (`\r`) overwrites the current line (progress bars work correctly)
- Line feed (`\n`) scrolls the viewport up and starts a new current line

### Why VTE?

We use the `vte` crate because it provides **structured handlers for the exhaustive set of terminal control sequences**. Rather than ad-hoc parsing of `\r`, `\n`, and escape codes, VTE implements the actual state machine that terminals use:

- The `Perform` trait gives us callbacks for `print(char)`, `execute(byte)`, `csi_dispatch(...)`, etc.
- This ensures we handle edge cases correctly (e.g., `\r\n` vs `\n\r`, escape sequences that span multiple bytes)
- As tools evolve to use new control sequences, VTE's comprehensive parsing keeps us correct

The VTE approach means our viewport emulation is grounded in the terminal specification, not in guesswork about what control characters "probably" mean.

### Why This Matters

Consider `cargo build` output:

```
   Compiling foo v0.1.0
   Compiling bar v0.1.0
    Building [===========>        ] 145/300: baz
```

The bottom line uses `\r` to update in place. In a real terminal, you see it updating. In exohook's progress display, we must also show it updating—otherwise the user sees nothing for the entire build.

**The viewport is not "complete lines only"—it's "what would you see right now".**

## Output Buffer Semantics

The `OutputBuffer` maintains:

1. **Committed lines**: Lines that have been terminated by `\n` and scrolled up
2. **Current line**: The line currently being built (cursor is here)

A `snapshot()` of the viewport returns both:

```
[committed_line_n-2, committed_line_n-1, current_line]
```

If `current_line` is empty (cursor at start of fresh line), we show the most recent 3 committed lines instead. This handles the common case where a tool prints complete lines.

### Control Character Handling via VTE

The `vte` crate's `Perform` trait provides structured callbacks for all terminal control sequences. Our `ActionCollector` translates these into viewport operations:

| VTE Callback                | Trigger                    | Viewport Effect                              |
| --------------------------- | -------------------------- | -------------------------------------------- |
| `print(char)`               | Printable character        | Append to current line (clear if pending CR) |
| `execute(0x0A)`             | Line Feed (`\n`)           | Commit current line, scroll viewport         |
| `execute(0x0D)`             | Carriage Return (`\r`)     | Mark pending overwrite                       |
| `execute(0x09)`             | Tab                        | Expand to 4-space alignment                  |
| `csi_dispatch('K', [2])`    | `ESC[2K` (clear line)      | Clear current line                           |
| `print('\u{2028}')`         | Unicode Line Separator     | Commit line (like `\n`)                      |
| `print('\u{2029}')`         | Unicode Paragraph Sep      | Commit line (like `\n`)                      |
| Other CSI/OSC/ESC sequences | Colors, cursor moves, etc. | Passed through (preserved in line content)   |

This exhaustive handling means we correctly process any control sequence a tool might emit, not just the common cases.

## Progress Display Architecture

### Parallel Checks

Each check gets a `CheckProgressGroup`:

- 1 spinner showing check name + elapsed time
- 0-3 context lines showing the viewport

The context lines update on a 50ms tick, pulling from each check's `OutputBuffer`.

### Sequential Checks

Same display, but only one group is active at a time.

### Adaptive Width

Terminal width affects display:

- **Wide (≥100)**: 3 context lines, generous label padding
- **Normal (60-99)**: 3 context lines, reduced padding
- **Narrow (40-59)**: No context lines, compact labels
- **Compact (<40)**: Elapsed time indicator only

## Design Principles

1. **Show something immediately**: Within 100ms of starting, the user sees progress
2. **Reflect reality**: The viewport shows what a terminal would show
3. **Fail visibly**: Silence warnings appear if a check goes quiet for 30s
4. **Preserve colors**: PTY execution on Unix for native color support
5. **Graceful degradation**: Works on narrow terminals, CI, Windows

## Testing Strategy

Tests should verify terminal viewport semantics:

- Progress bar updates via `\r` are visible in snapshot
- `\n` scrolls the viewport correctly
- CRLF sequences work (Windows line endings)
- Long-running output with no `\n` still shows in viewport
- Empty lines are handled correctly

## Non-Goals

- Full terminal emulation (cursor positioning, alternate screens)
- Preserving exact ANSI sequence positions (we preserve colors, not cursor moves)
- Supporting interactive input (stdin is null)
