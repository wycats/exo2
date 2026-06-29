<!-- exo:112 ulid:01kmzxefdypv6w9j9dymntg7se -->

# RFC 112: `pty-responder` - Query-Aware PTY Wrapper


# RFC 0112: `pty-responder` - Query-Aware PTY Wrapper

- **Stage**: 0 (Idea)
- **Created**: 2025-01-24
- **Author**: @wycats

## Summary

A Rust crate that wraps PTY I/O and automatically responds to terminal capability queries (OSC 10/11, DA1, CSI 6n, etc.), preventing child processes from blocking on query timeouts.

## Motivation

When spawning processes in a PTY, tools like cargo, rustc, and the `supports-color` crate send terminal capability queries to detect features. If the PTY host doesn't respond, these tools wait for a timeout (often 1-5 seconds) before continuing.

This creates a frustrating UX where:
- Progress output is delayed by several seconds
- The delay compounds when multiple tools each do their own detection
- Users perceive the tool as slow when it's actually waiting for timeouts

### Current Landscape

| Layer | Examples | Responds to Queries? |
|-------|----------|---------------------|
| Raw PTY | `pty-process`, `portable-pty` | ❌ No |
| Full Terminal Emulator | `alacritty_terminal`, Ghostty | ✅ Yes (but heavy) |
| **Missing Middle Ground** | `pty-responder` (proposed) | ✅ Yes (lightweight) |

### Real-World Impact

This affects any tool that:
- Uses a PTY to preserve colors (build tools, test runners)
- Captures output while maintaining TTY semantics
- Runs cargo/rustc/node in a pseudo-terminal

Examples: lazygit, zellij, exohook, any CI tool with PTY mode.

## Design

### Core Abstraction: `QueryResponder`

```rust
/// A lightweight terminal query responder.
/// 
/// Wraps PTY I/O and intercepts/responds to terminal capability queries
/// while passing all other data through unchanged.
pub struct QueryResponder<W: Write> {
    writer: W,
    config: ResponderConfig,
}

impl<W: Write> QueryResponder<W> {
    /// Create a new responder that writes responses to `writer`.
    pub fn new(writer: W) -> Self;
    
    /// Create with custom configuration.
    pub fn with_config(writer: W, config: ResponderConfig) -> Self;
    
    /// Process a chunk of PTY output.
    /// 
    /// - Detects queries in `data`
    /// - Writes responses to the underlying writer
    /// - Returns filtered data with echoed responses removed
    pub fn process(&mut self, data: &[u8]) -> ProcessResult;
}

pub struct ProcessResult {
    /// Data to pass through (queries and echoed responses filtered out)
    pub passthrough: Vec<u8>,
    /// Which queries were detected and responded to
    pub queries_handled: Vec<QueryType>,
}
```

### Query Types

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum QueryType {
    /// OSC 10 - Query foreground color
    ForegroundColor,
    /// OSC 11 - Query background color  
    BackgroundColor,
    /// OSC 12 - Query cursor color
    CursorColor,
    /// CSI 6n - Query cursor position (DSR)
    CursorPosition,
    /// CSI c (DA1) - Primary Device Attributes
    DeviceAttributes,
    /// CSI > c (DA2) - Secondary Device Attributes
    SecondaryDeviceAttributes,
    /// Custom/unknown query
    Custom(Vec<u8>),
}
```

### Configuration

```rust
pub struct ResponderConfig {
    /// Colors to report for OSC 10/11/12 queries
    pub colors: ColorConfig,
    /// Cursor position to report for CSI 6n
    pub cursor_position: (u16, u16),
    /// Device attributes string for DA1
    pub device_attributes: String,
    /// Whether to filter echoed responses from output
    pub filter_echoes: bool,
}

pub struct ColorConfig {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
}

impl Default for ResponderConfig {
    fn default() -> Self {
        Self {
            colors: ColorConfig {
                foreground: Color::from_rgb(0xFF, 0xFF, 0xFF), // White
                background: Color::from_rgb(0x00, 0x00, 0x00), // Black
                cursor: Color::from_rgb(0xFF, 0xFF, 0xFF),
            },
            cursor_position: (1, 1),
            device_attributes: "?62;c".to_string(), // VT220
            filter_echoes: true,
        }
    }
}
```

### Feature: Inherit from Parent Terminal

```rust
// With `inherit` feature enabled:
impl ResponderConfig {
    /// Query the actual terminal for its colors and use those.
    /// Falls back to defaults if queries fail or not in a terminal.
    pub fn from_terminal() -> Self {
        // Uses terminal-colorsaurus under the hood
        let bg = terminal_colorsaurus::background_color()
            .unwrap_or(Color::BLACK);
        let fg = terminal_colorsaurus::foreground_color()
            .unwrap_or(Color::WHITE);
        
        Self {
            colors: ColorConfig {
                foreground: fg,
                background: bg,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
```

### Integration Example

```rust
use pty_responder::QueryResponder;
use pty_process::blocking::Pty;

let (mut pty, pts) = pty_process::blocking::open()?;

// Spawn child on pts...
let mut child = Command::new("cargo").arg("build").spawn(pts)?;

// Create responder that writes back to PTY
let mut responder = QueryResponder::new(&mut pty);

let mut buf = [0u8; 4096];
loop {
    let n = pty.read(&mut buf)?;
    if n == 0 { break; }
    
    // Process queries and get filtered output
    let result = responder.process(&buf[..n]);
    
    // Use filtered output (no query/response garbage)
    stdout().write_all(&result.passthrough)?;
    captured_output.extend(&result.passthrough);
}
```

## Supported Queries

### Phase 1 (MVP)
| Query | Sequence | Response |
|-------|----------|----------|
| Foreground Color | `OSC 10 ; ? ST` | `OSC 10 ; rgb:RRRR/GGGG/BBBB ST` |
| Background Color | `OSC 11 ; ? ST` | `OSC 11 ; rgb:RRRR/GGGG/BBBB ST` |
| Cursor Position | `CSI 6 n` | `CSI Pl ; Pc R` |
| Device Attributes | `CSI c` | `CSI ? 62 ; ... c` |

### Phase 2
| Query | Sequence | Purpose |
|-------|----------|---------|
| Cursor Color | `OSC 12 ; ? ST` | Cursor color |
| Selection Colors | `OSC 17/19 ; ? ST` | Selection fg/bg |
| Window Title | `OSC 21 ; ? ST` | Get window title |
| DA2 | `CSI > c` | Secondary device attributes |
| XTVERSION | `CSI > 0 q` | Terminal version |

## Open Questions

1. **Async support?** Should there be `async-std`/`tokio` features?

2. **VTE integration?** Should this build on the `vte` crate for parsing, or use simpler pattern matching? VTE is more correct but adds a dependency.

3. **Stateful vs stateless?** Should the responder track state (e.g., remember which queries it's seen)? Useful for deduplication but adds complexity.

4. **Echo timing?** PTY echo is asynchronous - responses may arrive split across reads. How much buffering is needed to reliably filter them?

5. **Light/dark mode?** Should `from_terminal()` be called once at startup, or should there be a way to refresh if the user changes themes?

## Prior Art

- **terminal-colorsaurus**: Query-side library for detecting terminal colors
- **vte**: Low-level escape sequence parser (could be used internally)
- **Alacritty/Ghostty**: Full terminal emulators that respond to all queries
- **Zellij**: Terminal multiplexer (has bugs in query responses)

## Implementation Notes

The pattern matching for queries is straightforward:
- OSC queries: `\x1b]N;?\x1b\\` or `\x1b]N;?\x07` (where N is 10, 11, 12, etc.)
- CSI 6n: `\x1b[6n`
- DA1: `\x1b[c` or `\x1b[0c`

Response filtering requires matching the exact bytes we wrote, accounting for:
- PTY echo delay (response may come in next read)
- Partial responses split across reads
- Interleaved with real output

## Alternatives Considered

1. **Set TERM=dumb**: Disables queries but also disables colors entirely
2. **Use pipes instead of PTY**: Loses TTY semantics, breaks progress bars
3. **Full terminal emulator**: Works but massive overkill for this use case
4. **Environment variables (COLORTERM, NO_COLOR)**: Only affects some tools

## References

- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [ANSI Escape Codes](https://en.wikipedia.org/wiki/ANSI_escape_code)
- [terminal-colorsaurus crate](https://crates.io/crates/terminal-colorsaurus)
- [lazygit issue #3419](https://github.com/jesseduffield/lazygit/issues/3419) - Same problem in Go ecosystem
- [zellij issue #3590](https://github.com/zellij-org/zellij/issues/3590) - Bug in query responses

