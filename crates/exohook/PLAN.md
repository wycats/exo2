# Implementation Plan: Fix Terminal Viewport Semantics

## Status Summary

- ✅ Phase 1: Include Current Line in Snapshot — **Complete**
- ✅ `{{files}}` Substitution (input_mode = "paths") — **Complete**
- ✅ RFC Split (10060/10061) — **Complete**
- ⏳ Phase 3: Improved Test Coverage — **Planned**
- ⏸ Phase 2: Dynamic Line Count — **Deferred**

## Problem Statement

The `OutputBuffer::snapshot()` method currently returns only "committed" lines (terminated by `\n`), excluding the `current_line` where the cursor resides. This causes:

1. **Progress bars invisible**: Tools like `cargo`, `pnpm`, and many others use `\r` to update a line in place. Since `\r` never triggers a commit, the current line is never shown.

2. **Empty context lines**: Users see 3 empty lines even when output is actively flowing, because all the action is in `current_line`.

3. **Confusion about activity**: Long-running builds appear to have no output, triggering silence warnings incorrectly and causing users/agents to think something is stuck.

## Root Cause

In `output_buffer.rs`:

- `commit_current_line()` is only called when VTE's `execute(0x0A)` (line feed) fires
- `snapshot()` only returns from `lines` (committed buffer)
- `current_line` is excluded from the snapshot

This violates the mental model: we're supposed to emulate a terminal viewport, where the current line is always visible. The VTE parser correctly handles `\r` via `execute(0x0D)`, but we only use it to set a "pending overwrite" flag—we never expose the evolving `current_line` to the snapshot.

## Phase 1: Include Current Line in Snapshot (Complete)

### Changes to `output_buffer.rs`

**Modify `snapshot()` to include `current_line` (Complete):**

```rust
pub fn snapshot(&self) -> [String; 3] {
    let inner = self.inner.lock().unwrap();

    // The viewport shows: [line_n-2, line_n-1, current_line_or_last_committed]
    // If current_line is non-empty, it's the bottom of the viewport.
    // If current_line is empty, show the last 3 committed lines.

    let use_current = !inner.current_line.is_empty();
    let committed_needed = if use_current { 2 } else { 3 };

    let committed: Vec<String> = inner.lines
        .iter()
        .rev()
        .take(committed_needed)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if use_current {
        // [committed[-2], committed[-1], current_line]
        match committed.len() {
            0 => [String::new(), String::new(), inner.current_line.clone()],
            1 => [String::new(), committed[0].clone(), inner.current_line.clone()],
            _ => [committed[0].clone(), committed[1].clone(), inner.current_line.clone()],
        }
    } else {
        // [committed[-3], committed[-2], committed[-1]]
        match committed.len() {
            0 => [String::new(), String::new(), String::new()],
            1 => [String::new(), String::new(), committed[0].clone()],
            2 => [String::new(), committed[0].clone(), committed[1].clone()],
            _ => [committed[0].clone(), committed[1].clone(), committed[2].clone()],
        }
    }
}
```

### Update Tests

Add tests that verify the new behavior:

```rust
#[test]
fn test_carriage_return_visible_in_snapshot() {
    let buffer = OutputBuffer::new(3);

    // Simulate cargo-style progress bar
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
    buffer.feed(b"In progress...");  // No newline

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
    buffer.feed(b"Line 3\n");  // Cursor now on empty new line

    let snapshot = buffer.snapshot();
    // current_line is empty, so show last 3 committed
    assert_eq!(snapshot[0], "Line 1");
    assert_eq!(snapshot[1], "Line 2");
    assert_eq!(snapshot[2], "Line 3");
}
```

### Update Documentation

Update the doc comment on `snapshot()` to reflect the new semantics:

```rust
/// Get a snapshot of the current viewport state.
///
/// Returns the 3 most recent visible lines as they would appear in a terminal:
/// - If `current_line` is non-empty: [committed[-2], committed[-1], current_line]
/// - If `current_line` is empty: [committed[-3], committed[-2], committed[-1]]
///
/// This ensures that progress bars using `\r` are visible in the snapshot.
```

## `{{files}}` Substitution (Complete)

The `{{files}}` placeholder is now expanded using `input_mode = "paths"` semantics, enabling checks to receive explicit file lists derived from the lane scope.

## RFC Split (10060/10061) (Complete)

The streaming progress RFC was split into discrete RFCs:

- **RFC 0122**: Streaming progress implementation
- **RFC 0083**: Machine channel (separate scope)

## Phase 2: Dynamic Line Count (Deferred Optional Enhancement)

The current API returns `[String; 3]` which forces 3 lines even when empty. A future enhancement could:

1. Change return type to `Vec<String>` (0-3 elements)
2. Only return non-empty lines
3. Let `CheckProgressGroup` dynamically create/destroy progress bars

This is more invasive and may not be necessary if Phase 1 solves the visibility problem.

**Recommendation**: Defer Phase 2 unless Phase 1 doesn't fully address the UX issue.

## Phase 3: Improved Test Coverage (Planned)

### Test Scenarios to Add

The VTE-based approach means we should test against the actual control sequences that VTE dispatches:

1. **Cargo-style progress**: `Compiling...\n` followed by `Building [=>] X/Y\r` updates (tests `execute(0x0D)`)
2. **npm/pnpm style**: Package name with spinner characters (tests rapid `print()` calls)
3. **Long lines**: Lines that exceed terminal width (should truncate)
4. **Mixed CR/LF**: Windows-style `\r\n` line endings (tests `execute(0x0D)` then `execute(0x0A)`)
5. **Burst output**: Rapid output followed by silence
6. **No newlines ever**: Progress bar that never terminates (tests `current_line` visibility)
7. **CSI clear line**: `ESC[2K` sequences (tests `csi_dispatch`)
8. **Unicode line separators**: `\u{2028}` and `\u{2029}` (tests `print()` special cases)
9. **Color codes**: `ESC[32m` style ANSI colors (tests passthrough preservation)

### Property-Based Testing (Future)

Consider adding proptest for:

- Any sequence of bytes produces valid UTF-8 snapshot
- Snapshot length is always 3
- Snapshot elements are never longer than input

## Validation

After implementation:

1. Run `exohook validate gate` on this repository
2. Observe that `cargo clippy`, `cargo fmt`, etc. show live progress
3. Verify that `pnpm` checks show progress during package resolution
4. Test with narrow terminal (40 cols) to verify compact mode still works

## Estimated Effort

| Phase                  | Effort    | Risk                   |
| ---------------------- | --------- | ---------------------- |
| Phase 1: Core fix      | 1-2 hours | Low (localized change) |
| Phase 3: Tests         | 1 hour    | Low                    |
| Phase 2: Dynamic lines | 3-4 hours | Medium (API change)    |

**Recommendation**: Implement Phase 1 + Phase 3, defer Phase 2.
