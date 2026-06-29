<!-- exo:234 ulid:01kmzxey1e303ajgs33phwdghp -->

# RFC 234: Command Output Boundary: Eliminating println! Pollution


# RFC 00234: Command Output Boundary: Eliminating println! Pollution

## Summary

Functions callable from the machine channel (JSON server) must return messages through `CommandOutput` rather than printing directly to stdout. Direct `println!` calls pollute the JSON stream and break the machine channel protocol.

## Motivation

The machine channel (`exo json server`) communicates via JSON on stdin/stdout. When old-style functions like `task::complete_task()` call `println!()`, that output goes to stdout and corrupts the JSON stream:

```
{"protocol_version":1,"id":"123",...}
Task 'foo' marked as completed.  <-- POLLUTION
{"protocol_version":1,"id":"124",...}
```

This causes JSON parse errors in the VS Code extension's `MachineChannelServer`.

### Current State

The command system has a proper output structure:

```rust
pub struct CommandOutput {
    pub data: serde_json::Value,
    pub human_message: Option<String>,
}
```

The dispatcher handles this correctly:

- JSON format: wraps in `ResponseEnvelope`, prints as JSON
- Human format: prints `human_message` or formatted `data`

However, legacy functions in `task.rs` bypass this by calling `println!()` directly.

## Detailed Design

### The Boundary Pattern

**Rule**: Functions that may be called from the command layer MUST NOT print to stdout. They should either:

1. Return a `Result<T, E>` where `T` contains any message data
2. Return a message string that the caller can include in `CommandOutput`
3. Use a callback/writer pattern for streaming output

### Refactoring Strategy

For `task.rs` functions (`complete_task`, `start_task`, `remove_task`, `reorder_task`, `update_task`):

**Before:**

```rust
pub fn complete_task(root: &Path, id: &str, log: &str) -> ExoResult<()> {
    // ... do work ...
    println!("Task '{id}' marked as completed.");
    Ok(())
}
```

**After:**

```rust
pub fn complete_task(root: &Path, id: &str, log: &str) -> ExoResult<String> {
    // ... do work ...
    Ok(format!("Task '{id}' marked as completed."))
}
```

The command layer then captures this:

```rust
impl MutableCommand for TaskComplete {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let message = task::complete_task(ctx.root, &self.id, &self.log)?;
        Ok(CommandOutput::new(TaskCompleteOutput { ... })
            .with_human_message(message))
    }
}
```

### Compile-Time Enforcement

Add `#![deny(clippy::print_stdout)]` to modules that should never print:

```rust
// tools/exo/src/task.rs
#![deny(clippy::print_stdout, clippy::print_stderr)]
```

This makes accidental `println!` a compile error.

### Exceptions

Some code legitimately needs to print:

- `command/dispatcher.rs` - the IO boundary
- `command/init.rs` - interactive prompts
- CLI-only code paths

These modules can use `#![allow(clippy::print_stdout)]` with a comment explaining why.

## Implementation Plan

### Phase 1: Refactor task.rs (This Phase)

1. Change return types from `ExoResult<()>` to `ExoResult<String>`
2. Replace `println!()` with `Ok(format!(...))`
3. Update command layer to capture returned messages
4. Add `#![deny(clippy::print_stdout)]` to task.rs

### Phase 2: Audit Other Modules (Future)

- `phase.rs` - has 11 `println!` calls
- `goal.rs` - appears clean
- Other modules as discovered

### Phase 3: Crate-Wide Lint (Future)

Consider adding to `clippy.toml` or `Cargo.toml`:

```toml
[lints.clippy]
print_stdout = "deny"
```

With explicit `#[allow]` for IO boundary code.

## Alternatives Considered

### Writer Pattern

Pass a `&mut dyn Write` to functions:

```rust
pub fn complete_task(root: &Path, id: &str, log: &str, out: &mut dyn Write) -> ExoResult<()>
```

Rejected: More invasive, and we already have `CommandOutput` for this purpose.

### Macro Wrapper

Create a macro that captures output:

```rust
let output = capture_stdout! { task::complete_task(...) };
```

Rejected: Doesn't prevent the pollution, just works around it.

## Prior Art

- Go's `io.Writer` pattern
- Rust's `std::io::Write` trait
- The existing `CommandOutput` structure in this codebase

## Unresolved Questions

1. Should we refactor `phase.rs` in this phase or defer?
2. Should the lint be crate-wide or module-specific?

