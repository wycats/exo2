<!-- exo:97 ulid:01kg5kp2ftkrshxfzveq3wqs5j -->

# RFC 0097: Machine Channel: Unified Server Architecture

## Summary

Evolve the Machine Channel from a VS Code-owned subprocess to a self-managing daemon that serves both the CLI and VS Code extension as equal clients. This eliminates duplicate load/save logic and enables a single source of truth for project state.

## Implementation Status

**Mostly implemented** (Phase: Daemon Hardening complete):

- ✅ Basic daemon infrastructure (`exo daemon run`)
- ✅ Project-scoped socket paths (`{state_root}/runtime/daemon.sock`)
- ✅ Idle timeout (5 minutes default, configurable)
- ✅ Connect-or-spawn helper (`ensure_daemon()`)
- ✅ Race condition prevention (flock-based PID locking)
- ✅ Graceful shutdown (SIGTERM/SIGINT handlers, cleanup)
- ✅ VS Code migration to socket (`DaemonChannelServer` wired as `MachineChannelServer`)
- ⏳ CLI migration to IPC (CLI still supports direct command execution)

**Project path note**: RFC 10184 supersedes the checkout-local runtime placement described in older versions of this RFC. The daemon is project-scoped. Linked worktrees share the same daemon socket because they share project identity and state root.

## Background: v1 (Implemented)

The first implementation established a persistent subprocess model:

- **Server**: `exo json server` reads NDJSON from stdin, writes responses to stdout
- **Client**: VS Code's `MachineChannelServer` spawns and owns the server process
- **Lifecycle**: Server dies when VS Code extension deactivates

This solved the spawn-per-request performance problem (~120ms → <20ms per call).

**Limitation**: The CLI (`exo task add`, etc.) bypasses this server entirely, loading and saving state directly. This creates:

1. **Duplicate code paths**: CLI and server both implement load/save logic
2. **Concurrency risk**: CLI and server can race on writes
3. **Migration burden**: Any storage change must be implemented twice

## Motivation

The daemon is the shared machine-channel surface for all clients. It gives VS Code, LM tools, and future clients one structured semantic interface for exo operations.

The `exo Everywhere` project/workspace split adds another requirement: the daemon boundary must be the project, not the current checkout. A primary worktree and linked worktree are two workspaces for the same project, so they connect to the same daemon.

## Design

### Architecture

```text
Clients
  CLI / VS Code / LM tools
        │
        ▼
Unix domain socket
{state_root}/runtime/daemon.sock
        │
        ▼
exo daemon
  • listens once per project state root
  • receives the caller workspace root with requests
  • owns machine-channel command handling
  • exits after idle timeout
        │
        ▼
Project SQLite database
{state_root}/cache/exo.db
```

### File locations

Runtime artifacts live under the resolved project state root from RFC 10184:

```text
{state_root}/runtime/
  daemon.sock
  daemon.pid
```

For default state in a normal repository this is:

```text
<primary-workspace>/.exo/runtime/daemon.sock
```

For shadow state this is:

```text
$HOME/.exo/projects/<project-id>/runtime/daemon.sock
```

This replaces the older checkout-local path:

```text
{workspace}/.runtime/daemon.sock
```

That older path is no longer normative.

### Project daemon, workspace request context

The daemon is project-scoped, but request handling remains workspace-aware.

- `LocalRuntimePaths` derives socket and PID paths from the resolved `Project`.
- The daemon receives a workspace root for the caller.
- Command handling resolves project state paths for the database and runtime.
- Commands that write checked-out files still write relative to the caller workspace.

This preserves workspace-specific diffs while avoiding one daemon per worktree.

### Lifecycle: self-managing daemon

The server is not tied to VS Code's lifecycle.

1. **First client spawns**: Any client can start the daemon.
2. **Idle timeout**: Daemon exits after N seconds with no connected clients.
3. **Transparent restart**: If daemon dies, the next client spawns a fresh one.

#### Connect-or-spawn protocol

```rust
pub async fn ensure_daemon(workspace: &Path) -> Result<UnixStream> {
    let paths = paths_for_workspace(workspace)?; // resolves Project first

    if let Ok(stream) = UnixStream::connect(paths.socket_path()).await {
        return Ok(stream);
    }

    if paths.pid_path().exists() {
        let pid = std::fs::read_to_string(paths.pid_path())?;
        if !process_exists(pid.parse()?) {
            let _ = std::fs::remove_file(paths.socket_path());
            let _ = std::fs::remove_file(paths.pid_path());
        }
    }

    spawn_daemon(workspace)?;
    wait_for_socket(workspace, timeout).await?;
    UnixStream::connect(paths.socket_path()).await
}
```

#### Idle timeout

```rust
async fn run_with_idle_timeout(
    handler: impl Fn(Request) -> Response,
    idle_timeout: Duration,
) {
    let last_activity = Arc::new(AtomicU64::new(now_secs()));

    tokio::spawn({
        let last = last_activity.clone();
        async move {
            loop {
                tokio::time::sleep(idle_timeout / 2).await;
                let elapsed = now_secs() - last.load(Ordering::Relaxed);
                if elapsed > idle_timeout.as_secs() {
                    std::process::exit(0);
                }
            }
        }
    });
}
```

### CLI changes

The CLI can become a thin IPC client:

```rust
fn main() {
    let client = ensure_daemon()?;
    let response = client.send(&Request {
        operation: args_to_operation(args),
    })?;
    print_response(response);
}
```

`--direct` remains as a debugging and fallback path.

### Protocol

The protocol envelope is RFC 0125. This RFC only specifies daemon lifecycle and socket placement.

Transport differences:

- **stdio fallback**: `exo json channel`
- **socket primary**: `{state_root}/runtime/daemon.sock`

Both transports use the same request/response envelope.

## Implementation Plan

### Phase 1: Daemon Infrastructure ✅

1. ✅ Create `exo daemon run --workspace <path>` command
2. ✅ Implement idle timeout wrapper
3. ✅ Use project runtime paths from RFC 10184
4. ✅ Add flock-based PID file locking
5. ✅ Add SIGTERM/SIGINT handlers for graceful shutdown

### Phase 2: CLI as Client ✅ / partial

1. ✅ Create `ensure_daemon()` helper
2. ✅ Keep `--direct` flag for debugging
3. ⏳ Finish removing direct command execution as the default path where product needs require it

### Phase 3: VS Code Migration ✅

1. ✅ Create `DaemonChannelServer` to connect via socket
2. ✅ Wire as drop-in replacement for `MachineChannelServer`
3. ✅ Use same `ensure_daemon` pattern as CLI

### Phase 4: Cleanup

The remaining cleanup is command execution model simplification:

1. Commands receive `CommandContext` / `MutableCommandContext`.
2. Some handlers still load `AgentContext` themselves.
3. The target model is for command contexts to carry the already-resolved project/workspace state needed by commands.

This is not required for RFC 10184 correctness, but it reduces redundant loads and clarifies the daemon boundary.

## Effort Estimate

- **Size**: M
- **Risk**: Medium
- **Dependencies**: tokio, project resolution from RFC 10184

## Success Criteria

1. One daemon socket per project state root.
2. Primary and linked worktrees connect to the same daemon.
3. Shadow projects place daemon runtime under `$HOME/.exo/projects/<project-id>/runtime`.
4. `--direct` continues to work as fallback.
5. Idle cleanup removes project runtime socket/PID files.
6. VS Code and LM tools use the same machine-channel semantics as CLI fallback.

## Alternatives Considered

### Keep separate paths

Rejected. Separate CLI and daemon write paths duplicate storage behavior and create race conditions.

### VS Code owns daemon

Rejected. CLI and LM-tool clients need the same durable project daemon.

### Per-workspace daemon

Rejected by RFC 10184. Worktrees are workspace faces of one project; one daemon per worktree recreates split-brain state.

### Systemd/launchd integration

Rejected. It is platform-specific, requires user setup, and is unnecessary for a developer tool with connect-or-spawn.

### Muzan crate

Rejected. The crate hardcoded XDG global paths. exo needs runtime paths derived from project state policy.

## References

- Current daemon implementation: `tools/exo/src/daemon.rs`
- VS Code client: `packages/exosuit-vscode/src/machine-channel/DaemonChannelServer.ts`
- RFC 0125: Machine Channel Protocol (envelope format)
- RFC 10184: Project / Workspace / Worktree unbundling
- RFC 10180: Storage Disposition

## Appendix: v1 implementation

The stdio implementation remains functional as fallback:

- **Rust Server**: `exo json channel`
- **TypeScript Client**: machine-channel subprocess fallback
- **Lifecycle**: subprocess-owned by the caller

This RFC extends v1 rather than replacing it.
