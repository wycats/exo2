<!-- exo:10179 ulid:01kmzxbcy0d4qbhef6h56zxe0d -->


# RFC 10179: Binary Re-exec: Workspace-Local Development Builds

## Summary

When an exo-family binary (`exo`, `exohistory`, `exohook`) starts, it checks `exosuit.toml` for a `[dev] binary_dir` setting. If a different binary exists at that path, it re-execs to it. This ensures that a single system-installed binary always delegates to the workspace-local build, eliminating stale binary drift.

## Motivation

During development, the installed `exo` binary (in `~/.cargo/bin/`) becomes stale relative to the workspace's `target/debug/exo`. This causes subtle bugs:

- The daemon spawns using the PATH binary, which may have a different SQLite loader, different command set, or different protocol version than the code being developed
- Previous workarounds (copying binaries to `~/.proto/bin/`, symlinking, adding `cargo install` to build scripts) create maintenance burden and fail silently when forgotten
- The VS Code extension had `exosuit.exoBinaryDir` in `.vscode/settings.json`, but the CLI didn't know about it — the extension and CLI resolved different binaries

The re-exec pattern solves this at the root: one binary on PATH, workspace config determines which build actually runs.

## Design

### Configuration

In `exosuit.toml` (committed to git):

```toml
[dev]
binary_dir = "target/debug"
```

This is the expected, documented behavior for working in this workspace: you get the local build, not the global one.

### Re-exec Protocol

When any exo-family binary starts (before any argument parsing):

1. Check `EXO_NO_REEXEC` — if set, skip (loop prevention layer 1)
2. Find the workspace root (walk up from cwd looking for `exosuit.toml`)
3. Read `exosuit.toml`, check for `[dev] binary_dir`
4. If set, construct the candidate path: `{workspace_root}/{binary_dir}/{binary_name}`
5. Verify the candidate is not the currently executing binary (loop prevention layer 2)
6. If the candidate exists, is a file, and passes security checks:
   - Re-exec via `std::os::unix::process::CommandExt::exec`
   - Pass all original arguments unchanged
   - Set `EXO_NO_REEXEC=1` in the environment (loop prevention layer 3)

### Loop Prevention (Multi-Layer)

Three independent layers prevent infinite re-exec loops:

1. **Env var gate**: `EXO_NO_REEXEC=1` is set before re-exec and checked on entry
2. **Self-check**: Compare `std::env::current_exe()` (canonicalized) against the candidate path — if they're the same file, skip
3. **Env var on exec**: The re-exec'd process inherits `EXO_NO_REEXEC=1`, so even if the self-check fails (e.g., symlink resolution differences), the env var catches it

### Shared Crate: `exo-reexec`

The re-exec logic is extracted into a small shared crate (`crates/exo-reexec` or similar) so that `exo`, `exohistory`, and `exohook` all share the same implementation. Bug fixes apply to all binaries. Future binaries get re-exec support by adding one dependency and one function call at the top of `main()`.

### Daemon Freshness

Daemon freshness is enforced at the Rust lifecycle boundary rather than by
statting or re-executing from every request:

1. The daemon records its executable identity, instance ID, PID, and
   process-start identity when it starts.
2. Before a client reuses a daemon connection, `daemon ensure` compares the
   recorded executable/workspace identity with the current binary and performs
   a bounded probe for that exact instance.
3. A stale or unresponsive instance is identity-verified, terminated, and
   replaced. The client connects to the ensured socket and receives the new
   instance ID.
4. Long-lived clients discard every socket lane and invalidate cached state
   whenever ensure reports a different daemon instance.

This keeps binary freshness off the request hot path while preserving the
development contract: after a workspace-local build changes, the next ensured
tool request replaces the stale runtime and continues through the fresh binary.

### Extension Integration

The VS Code extension's `exosuit.exoBinaryDir` setting is **removed** (not deprecated — we are the only user). The extension calls `exo` on PATH; the re-exec protocol selects the workspace-local CLI, and Rust `daemon ensure` selects and repairs the daemon instance. This unifies binary resolution for CLI, daemon, and extension into one mechanism.

### Security

The `binary_dir` value comes from `exosuit.toml`, which is a workspace file. Same trust model as `.envrc` (direnv).

For the initial implementation, `binary_dir` is restricted to relative paths resolved within the workspace root. This is a simplicity constraint, not a security boundary — a future follow-up may support absolute paths (e.g., a global `exosuit.toml` pointing at a specific checkout's `target/debug/`) once the use case is validated.

## Implementation

### `find_workspace_root()`

Walk up from `cwd` looking for `exosuit.toml`:

```rust
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join("exosuit.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
```

**Stage 4 criteria**: Also check for `.config/exosuit.toml` if the config layout moves under the RFC 10184 project/workspace directory model.

### `[dev]` section in `ExosuitConfig`

```rust
#[derive(Debug, Deserialize, Default)]
pub struct DevConfig {
    pub binary_dir: Option<String>,
}
```

Add `pub dev: Option<DevConfig>` to `ExosuitConfig`.

### Re-exec Logic (in `exo-reexec` crate)

```rust
pub fn maybe_reexec() {
    if std::env::var("EXO_NO_REEXEC").is_ok() {
        return;
    }

    let cwd = std::env::current_dir().ok();
    let Some(root) = cwd.as_ref().and_then(|cwd| find_workspace_root(cwd)) else {
        return;
    };

    let config = match ExosuitConfig::load(&root) {
        Ok(config) => config,
        Err(_) => return,
    };

    let Some(binary_dir) = config.dev.as_ref().and_then(|d| d.binary_dir.as_ref()) else {
        return;
    };

    // Security: reject absolute paths
    if Path::new(binary_dir).is_absolute() {
        return;
    }

    let binary_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_os_string()));
    let Some(name) = binary_name else { return };

    let candidate = root.join(binary_dir).join(&name);

    if !candidate.exists() || !candidate.is_file() {
        return;
    }

    // Security: verify candidate is within workspace root
    let Ok(canonical_candidate) = candidate.canonicalize() else { return };
    if !canonical_candidate.starts_with(&root) {
        return;
    }

    // Loop prevention: don't re-exec to ourselves
    if let Ok(current) = std::env::current_exe().and_then(|p| p.canonicalize()) {
        if current == canonical_candidate {
            return;
        }
    }

    // Re-exec
    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let err = std::process::Command::new(&candidate)
        .args(&args)
        .env("EXO_NO_REEXEC", "1")
        .exec();
    eprintln!("exo: re-exec to {} failed: {err}", candidate.display());
}
```

### Cleanup

- Remove `~/.proto/bin/exo` (stale copy from previous agent session)
- Remove `cargo install` from `scripts/dev/dogfood-extension.sh`
- Remove the symlink `~/.proto/bin/exo → ~/.cargo/bin/exo`
- Add `[dev] binary_dir = "target/debug"` to `exosuit.toml`
- Remove `exosuit.exoBinaryDir` VS Code setting and all code that reads it

## Stage 4 Criteria

- Windows support: `CommandExt::exec` is Unix-only. Windows would need `Command::spawn` + `std::process::exit`.
- `.config/exosuit.toml` support in `find_workspace_root()` if adopted under RFC 10184.
