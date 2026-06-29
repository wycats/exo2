<!-- exo:141 ulid:01kg5kp2j0h0a8bwyvyjhg22q3 -->

# RFC 141: Exohook Streaming Progress Reporting


# RFC 0141: Exohook Streaming Progress Reporting

## Summary

Add real-time streaming output and progress indication to `exohook validate`, eliminating the "apparent hang" UX problem where long-running hooks (especially `gate` lane with tests + coverage) appear frozen. Uses a **hybrid approach**: PTY-based execution on Unix for native terminal experience, with piped streaming fallback for Windows and non-interactive environments.

## Motivation

### The Problem

When running `git push`, the pre-push hook triggers `exohook validate gate`. This lane runs:

- `pnpm -r run test:unit`
- `cargo llvm-cov --workspace --lcov --output-path lcov.info`

These checks can take 15-30+ seconds. During execution, the terminal shows **nothing**—the cursor just sits there. Users (and AI agents) reasonably interpret this as a hang, especially when:

1. They've never run the gate lane before
2. The checks need to compile first (adding 30+ seconds)
3. Network issues cause unexpected delays

The root cause is in `run_command_capture()` which uses `.output()`:

```rust
let output = Command::new(program)
    .args(args)
    .current_dir(repo_root)
    .output()  // <-- Blocks until complete, buffers all output
```

### Prior Art

| Tool          | Approach                                                        | Notes                                          |
| ------------- | --------------------------------------------------------------- | ---------------------------------------------- |
| **Lefthook**  | Configurable `execution_out` (per-check or all), no PTY         | Can show output live but no progress indicator |
| **Turborepo** | `--log-order=stream\|grouped\|auto`, optional TUI               | Auto-detects TTY, grouped for CI               |
| **pnpm**      | Spinner per package during install                              | Good parallel progress UX                      |
| **Cargo**     | Streams compiler output immediately, progress bar for downloads | Immediate feedback                             |

### Goals

1. **Immediate feedback**: User sees _something_ within 100ms of starting
2. **Progress visibility**: For parallel checks, show what's running
3. **Native colors**: Commands emit ANSI colors naturally via PTY (no `--color=always` needed)
4. **Input detection**: Detect commands waiting for stdin (invisible hang trap)
5. **Backward compatible**: Existing `--format=compact|grouped` still work
6. **Compatible**: Keep existing `--format=compact|grouped` outputs stable

## Design

### Hybrid Execution Strategy (Option D)

The core insight is that **PTY and piped streaming serve different use cases optimally**:

| Context                   | Execution Mode                 | Rationale                                   |
| ------------------------- | ------------------------------ | ------------------------------------------- |
| Unix + TTY + Human output | **PTY** via `pty-process`      | Native colors, realistic terminal emulation |
| Unix + no TTY (CI/pipe)   | **Pipes** with `FORCE_COLOR=1` | Can't use PTY without a terminal            |
| Windows (any mode)        | **Pipes** with `FORCE_COLOR=1` | `pty-process` doesn't support Windows       |

Decision logic:

```rust
fn use_pty(simple_output: bool) -> bool {
  cfg!(unix)
    && std::io::stdout().is_terminal()
    && !simple_output
}
```

### Why PTY Merges stdout and stderr

A key property of PTY execution is that **stdout and stderr are merged**. This is not a limitation of the `pty-process` crate—it's fundamental to how PTYs work:

1. A PTY emulates a **physical terminal** (think: VT100, xterm)
2. Physical terminals have **one screen**—there's no concept of separate output streams
3. When you open a PTY, you get a master/slave pair where all three fds (stdin, stdout, stderr) of the child process point to the **same pts device**
4. The child writes to "stdout" or "stderr", but both go to the same pts → same master fd

**For exohook, merged output is acceptable:**

- Error messages from checks still appear (just not in a separate stream)
- Color separation between stdout/stderr is rarely meaningful for build tools
- The alternative (pipes) loses native color support unless tools have `--color=always`

### Platform Support

| Platform           | PTY Support | Notes                                     |
| ------------------ | ----------- | ----------------------------------------- |
| **Linux**          | ✓ Full      | `pty-process` uses `/dev/ptmx`            |
| **macOS**          | ✓ Full      | `pty-process` uses `openpty()`            |
| **WSL2**           | ✓ Full      | Real Linux kernel, works identically      |
| **Windows native** | ✗ Fallback  | Uses piped streaming with `FORCE_COLOR=1` |

### Execution Modes (Updated)

| Condition               | Execution     | Display Strategy                           |
| ----------------------- | ------------- | ------------------------------------------ |
| Unix + TTY + Sequential | PTY           | Stream output with native colors           |
| Unix + TTY + Parallel   | PTY per check | Spinner per check, grouped output          |
| No TTY (CI/pipe)        | Pipes         | Banner + grouped output (current behavior) |
| Windows                 | Pipes         | Same as Unix pipes, with `FORCE_COLOR=1`   |

### TTY-Aware Progress Display

When running parallel checks with TTY, show a **multi-line rolling buffer** below each spinner. This provides real-time context without overwhelming the display:

```
[exohook] lane 'gate': 2 checks

  ● test         running...
    │ Running 47 tests from packages/exosuit-core
    │ test command::registry::tests::test_default... ok
    │ test command::router::tests::test_route_valid... ok
  ● rust-coverage  running...
    │ Compiling exo v0.1.0
    │    Building [===================>     ] 244/300: exo
    │
```

After completion:

```
[exohook] lane 'gate': 2 checks

  ✓ test         ok    12.3s
  ✓ rust-coverage ok    18.5s

lane 'gate': ok (2/2) 18.51s
```

For sequential execution:

- PTY output streams directly (with native colors)
- Each check's output appears in real-time

### Configuration

New fields in `hooks.toml`:

```toml
[defaults]
# Timeout before warning about no output (seconds)
silence-warning-seconds = 30

# Force pipe mode even on Unix TTY (disables PTY for simpler output)
simple-output = false

# Stream output from parallel checks (may interleave)
show-parallel-output = false
```

## Implementation Status

- **PTY-based streaming** is fully implemented for Unix TTYs.
- **Pipe-based streaming** is implemented for non-TTY contexts and Windows.
- **Progress UI** is partially implemented (spinners + rolling buffer). The idle-timeout warning
  infrastructure exists, but the warning is not wired into the streaming loop yet.
- **Machine channel output was extracted to RFC 0083** and is not implemented here.

## Implementation Notes

This section documents the actual implementation as of the Stage 2 draft.

### File Structure

The streaming infrastructure is located in `crates/exohook/src/`:

```
crates/exohook/src/
├── main.rs           # CLI entry point, lane execution
├── check_runner.rs   # Hybrid dispatch (PTY vs pipes)
├── pty_runner.rs     # PTY-based execution (Unix only)
├── pipe_runner.rs    # Pipe-based execution (cross-platform)
└── output_buffer.rs  # VTE-parsed rolling buffer + CheckProgressGroup
```

### Module Organization

#### `check_runner.rs` — Hybrid Dispatch

The main entry point for running checks. Automatically chooses between PTY and pipe execution based on:

- Platform (`cfg!(unix)`)
- Terminal detection (`std::io::stdout().is_terminal()`)
- Output mode (interactive vs non-interactive)

Key types:

```rust
/// Output mode for check execution.
pub enum OutputMode {
    Human,   // Human-readable output (default)
    NonInteractive, // Non-interactive output (pipes)
}

/// Unified result from running a check (regardless of runner).
pub struct CheckResult {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,      // Empty for PTY mode
    pub stderr: Vec<u8>,      // Empty for PTY mode
    pub output: Vec<u8>,      // Combined output (PTY mode uses this)
    pub duration: Duration,
    pub used_pty: bool,
}
```

Key function:

```rust
pub fn spawn_check(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_mode: OutputMode,
    output_buffer: Option<OutputBuffer>,
    stream_output: bool,
) -> Result<CheckResult>
```

#### `pty_runner.rs` — Unix PTY Execution

Uses the `pty-process` crate for pseudo-terminal execution. Only compiled on Unix platforms (`#[cfg(unix)]`).

Key characteristics:

- Opens PTY with `pty_process::blocking::open()`
- Sets terminal size to 24×80
- Sets `TERM=xterm-256color` for color support
- Reads from PTY master in 4KB chunks
- Optionally streams to stdout and/or OutputBuffer

```rust
pub fn spawn_streaming(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_buffer: Option<OutputBuffer>,
    stream_to_stdout: bool,
) -> Result<PtyCheckResult>
```

#### `pipe_runner.rs` — Cross-Platform Fallback

Uses standard `std::process::Command` with piped stdout/stderr. Works on all platforms.

Key characteristics:

- Sets `stdin(Stdio::null())` to prevent stdin hangs
- Sets `FORCE_COLOR=1` and `CLICOLOR_FORCE=1` for color support
- Spawns separate reader threads for stdout and stderr
- Uses `mpsc::channel` for streaming output to main thread

```rust
pub fn spawn_streaming(
    program: &str,
    args: &[String],
    repo_root: &Path,
    output_buffer: Option<OutputBuffer>,
    stream_output: bool,
) -> Result<PipeCheckResult>
```

#### `output_buffer.rs` — VTE Parsing & Progress Display

Provides thread-safe rolling buffer with ANSI escape sequence parsing via the `vte` crate.

Key types:

```rust
/// Thread-safe rolling buffer for terminal output.
pub struct OutputBuffer {
    inner: Arc<Mutex<OutputBufferInner>>,
}

/// A group of progress bars for a single check: 1 spinner + 3 context lines.
pub struct CheckProgressGroup {
    pub spinner: ProgressBar,
    pub context_lines: [ProgressBar; 3],
    pub output_buffer: OutputBuffer,
}
```

The `OutputBuffer` handles:

- `\n` (line feed) — commits current line to buffer
- `\r` (carriage return) — marks pending overwrite (progress bar support)
- `\t` (tab) — expands to 4-space alignment
- CSI sequences (e.g., `ESC[2K` clear line)
- CRLF sequences (Windows line endings)

Implementation detail: OSC sequences can arrive fragmented across chunk boundaries. The
`sanitize_caret_notation_osc` helper normalizes these fragments before parsing so partial
escape sequences don't corrupt the rolling buffer.

The `CheckProgressGroup` manages indicatif progress bars:

- Main spinner showing check status
- 3 context lines showing recent output (dim, with `│` prefix)
- `update_context()` refreshes display from buffer snapshot
- `finish_success()` / `finish_failure()` for completion

### Integration Points in main.rs

The streaming infrastructure integrates via two main functions:

```rust
/// For capture mode (no streaming) — uses pipe_runner directly
fn run_command_capture(
    repo_root: &Path,
    program: &str,
    args: &[String],
) -> Result<CheckResult>

/// For streaming mode — uses hybrid dispatch via spawn_check
fn run_command_streaming(
    repo_root: &Path,
    program: &str,
    args: &[String],
    stream_output: bool,
    output_buffer: Option<OutputBuffer>,
) -> Result<CheckResult>
```

These are called from lane execution logic based on:

- Whether sequential or parallel execution is configured
- Whether `--verbose` flag is set
- Whether running interactively or in CI

### Dependencies

```toml
[dependencies]
indicatif = "0.17"     # Progress bars and spinners
console = "0.15"       # TTY detection, colors
vte = "0.13"           # ANSI/VTE parsing via Perform trait

[target.'cfg(unix)'.dependencies]
pty-process = "0.5"    # PTY support for Unix platforms
```

### Deviations from Original Spec

1. **Idle timeout warning not yet implemented** — The `silence-warning-seconds` configuration is defined but the warning logic is pending.

2. **Non-interactive hints not implemented** — The spec proposed a `[hints.non_interactive]` table for suggesting flags like `--yes`; this is deferred.

3. **Configuration options** — Renamed for user clarity:
   - `idle_timeout_seconds` → `silence-warning-seconds`
   - `force_pipes` → `simple-output`
   - `stream_parallel` → `show-parallel-output`

### Test Coverage

Each module has unit tests:

- `pty_runner::tests` — simple command, output buffer integration
- `pipe_runner::tests` — stdout/stderr separation, failing commands
- `check_runner::tests` — hybrid dispatch, non-interactive mode uses pipes
- `output_buffer::tests` — CR/LF handling, VTE parsing, thread safety, rolling window

## Placement in MAP

This RFC should be implemented as part of **MAP Phase 5: Exohook Integration**, which already includes:

- `exohook-verify`: Wire exohook to exo verify
- `exohook-ai-output`: Machine-readable output (now specified in RFC 0083)
- `exohook-lanes`: Implement validation lanes (fast/slow)

The machine-channel output task is specified separately in RFC 0083.

### Task Status

The following tasks from the original RFC are now complete:

- ✓ `exohook-pty-runner` — Implement PTY-based streaming for Unix
- ✓ `exohook-pipe-runner` — Implement piped streaming fallback
- ✓ `exohook-hybrid-dispatch` — Add hybrid dispatch logic (PTY vs pipes)
- ○ `exohook-progress-ui` — Add TTY progress indicators (spinners) for parallel checks (partially complete: `CheckProgressGroup` implemented)
- ○ `exohook-idle-timeout` — Add idle timeout warning (designed but not implemented)

## Decision Matrix

| Approach            | Native Colors   | Separate stderr    | Windows      | Complexity |
| ------------------- | --------------- | ------------------ | ------------ | ---------- |
| Pipes only          | ✗ (needs flags) | ✓                  | ✓            | Low        |
| PTY only            | ✓               | ✗ (merged)         | ✗            | Medium     |
| **Hybrid (chosen)** | ✓ (Unix)        | ✓ (pipes fallback) | ✓ (fallback) | Medium     |

The hybrid approach gives native colors on Unix (the common case) while maintaining full compatibility elsewhere.

## Alternatives Considered

### Pipes-Only Approach

Using only `Stdio::piped()` with `FORCE_COLOR=1`:

- ✓ Simple implementation
- ✓ Separate stdout/stderr
- ✗ Many tools don't respect `FORCE_COLOR`
- ✗ Colors often look wrong (tools detect pipe, use different palette)

**Rejected** because color support is unreliable—too many tools auto-detect TTY regardless of env vars.

### Turbo-style TUI

A full terminal UI (like Turborepo's `--ui=tui`) would provide the richest experience but:

- Requires additional dependencies
- May not work well in all terminal emulators
- Overkill for typical hook runs

**Rejected** in favor of simpler PTY streaming + spinners.

## Estimated Effort

| Task                          | Effort | Status  |
| ----------------------------- | ------ | ------- |
| PTY runner implementation     | 2h     | ✓ Done  |
| Pipe runner implementation    | 1.5h   | ✓ Done  |
| Hybrid dispatch logic         | 0.5h   | ✓ Done  |
| Progress spinners (indicatif) | 2h     | Partial |
| Idle timeout warning          | 0.5h   | Pending |
| Tests (PTY, pipes, hybrid)    | 1.5h   | ✓ Done  |
| **Total**                     | ~8h    | ~70%    |

## Open Questions

1. ~~Should `force_pipes = true` in config override TTY detection entirely?~~ **Resolved**: Yes, via `simple-output` option.
2. Should we support custom `TERM` values in config for PTY mode?
3. ~~Should progress events be added to the machine channel protocol now or later?~~ **Resolved**: Extracted to RFC 0083 for a focused, later-stage protocol specification.

## References

- [RFC 0081: Exohook File Expansion Worked Examples](../stage-1/0081-exohook-file-expansion-worked-examples.md)
- [RFC 0083: Exohook Machine Channel Protocol](../stage-1/0083-exohook-machine-channel.md)
- [pty-process crate](https://docs.rs/pty-process)
- [Lefthook configuration docs](https://github.com/evilmartians/lefthook/blob/master/docs/configuration.md)
- [Turborepo log ordering](https://turbo.build/repo/docs/reference/run#--log-order)
- [indicatif crate](https://docs.rs/indicatif)
