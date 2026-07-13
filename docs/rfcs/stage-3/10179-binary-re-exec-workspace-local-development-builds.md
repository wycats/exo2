<!-- exo:10179 ulid:01kmzxbcy0d4qbhef6h56zxe0d -->

# RFC 10179: Binary Re-exec: Workspace-Local Development Builds

## Summary

An Exo development workspace may select its local toolchain build in
`exosuit.toml`:

```toml
[dev]
binary_dir = "target/debug"
```

This setting is the shared selection input for Exo's Rust entry points, daemon
lifecycle, and VS Code integration. An exo-family binary launched from outside
the workspace delegates to the selected build by replacing its process. The VS
Code extension can start a build selected from the same setting directly.
Before any client reuses a daemon, Rust lifecycle authority verifies that the
daemon belongs to the expected workspace and executable instance.

Together, these behaviors let a checked-out workspace run the code it just
built while retaining a stable command on `PATH` and a project-scoped daemon.

## Motivation

Exo development crosses several process boundaries. A shell starts `exo`; the
CLI may connect to a long-lived daemon; the VS Code extension keeps multiple
socket lanes open; MCP can add another persistent worker. A freshly compiled
workspace binary is useful only when every boundary converges on it.

Relying on installation discipline makes that convergence fragile. A developer
can build `target/debug/exo` while `~/.cargo/bin/exo` still contains an older
command registry, storage loader, or protocol implementation. Copying binaries,
maintaining symlinks, or teaching each client its own override moves the same
problem into several independent configuration surfaces.

The workspace already carries the relevant intent: this checkout is being
developed with this local build directory. RFC 10179 makes that intent the
shared selection input and assigns freshness enforcement to the process layer
best able to provide it.

## Guide-Level Explanation

When a developer runs an exo-family command from a configured workspace, the
launcher searches the current directory and its ancestors for `exosuit.toml`.
If `[dev].binary_dir` names a valid local build containing the same executable,
the running process delegates to that binary before parsing arguments. A normal
system installation can therefore remain on `PATH`; entering a configured
workspace is enough to select its development build.

The same setting applies when VS Code starts an Exo process. The extension
reads the workspace configuration and starts the selected binary directly when
it is available. Binary selection is a property of the workspace rather than
an editor preference, so terminals, MCP clients, and the extension share one
durable input without duplicating a user setting. The Rust selector currently
applies stricter path validation than the extension; the precise difference is
described below.

The daemon adds a second freshness boundary. It is long-lived, so selecting the
right client executable does not establish that an existing daemon is current.
Before reusing a daemon connection, `daemon ensure` compares the recorded
workspace and executable identity with the current client and probes the exact
daemon instance. A matching, responsive daemon is reused. A stale or
unresponsive instance is identified, replaced, and reconnected through its
project runtime endpoint.

Long-lived extension clients treat a changed daemon instance as a lifecycle
transition. They discard their socket lanes, reconnect through `daemon ensure`,
and invalidate cached state before serving later requests. Rebuilding Exo thus
converges on the new workspace binary at the next ensured interaction without
placing filesystem checks on the daemon request handler itself.

## Reference-Level Explanation

### Workspace configuration

The selection root is the nearest ancestor of the current working directory
that contains `exosuit.toml`. In a normal Exo checkout this is the workspace
root described by RFC 10184. The current configuration key is
`[dev].binary_dir`.

For Rust process replacement, `binary_dir` is a relative path. The selector
joins the configuration root, the configured directory, and the current
executable's file name. It accepts the candidate only when the path exists as a
file and its canonical path remains inside the canonical configuration root.
The Rust selector ignores absolute paths and candidates that escape the
workspace.

When no valid workspace candidate exists, the current Rust process continues.
The VS Code resolver follows the same workspace-first ordering, then honors
`EXO_BIN` for `exo` when present, and finally falls back to the executable name
on `PATH`. Its current workspace-candidate check joins the configured string to
the workspace root and tests only whether the resulting path exists. It does
not yet reject absolute values explicitly or canonicalize the candidate to
enforce workspace containment. A value containing parent traversal can
therefore select an extension-side candidate that the Rust selector rejects.
Matching the Rust validation contract is remaining stabilization work.

This repository keeps the development policy in its root `exosuit.toml`:

```toml
[dev]
binary_dir = "target/debug"
```

RFC 10184 separates workspace configuration from project identity and state
placement. Repo, shadow, and sidecar state policies therefore share this binary
selection behavior. A future configuration-layout RFC may add another
discovery location; `.config/exosuit.toml` is not part of the current contract.

### Process replacement

The shared `exo-reexec` crate implements startup delegation for `exo`,
`exo-mcp`, `exohook`, and `exohistory`. Each entry point calls
`maybe_reexec()` before argument parsing or runtime initialization.

On Unix, delegation uses process replacement. The selected binary receives the
original arguments and inherits the process environment with
`EXO_NO_REEXEC=1`. Two checks prevent recursive delegation:

1. A process carrying `EXO_NO_REEXEC` continues without another selection.
2. A candidate whose canonical path equals the current executable continues in
   place.

The environment marker is process-local loop prevention. It does not choose a
binary or persist workspace policy.

### Daemon executable authority

There is one daemon runtime endpoint per project state root. Its runtime
identity records:

- the workspace root that established the daemon;
- the executable path and filesystem identity;
- the daemon instance ID;
- the PID and process-start identity.

`daemon ensure` connects to the project endpoint and performs a bounded probe
for the recorded instance. It reuses the connection only when the workspace
and executable identity are current and the probe succeeds. Otherwise, the
lifecycle code verifies the observed process identity before terminating stale
runtime state and spawning the selected executable.

This rule also applies across linked worktrees. RFC 10184 gives linked
worktrees one project state root and runtime endpoint, while each worktree is a
distinct workspace. An ensure from a different worktree or development build
may replace the daemon attached to that shared endpoint. The replacement keeps
project state shared while making the active runtime's workspace and executable
authority explicit.

### VS Code integration

The active extension machine channel is `DaemonChannelServer`. It resolves the
workspace-local `exo` candidate from `exosuit.toml` when starting lifecycle
commands. Before reusing a live primary or read socket, it invokes Rust daemon
ensure again.

When ensure reports a replacement or a different instance ID, the extension
closes every connection lane, advances its connection generation, reconnects
to the ensured endpoint, and invalidates `TraceCache`. Requests issued after
the transition observe the replacement daemon rather than a connection or
cached view retained from the previous executable.

The extension does not expose a separate binary-directory setting. The
workspace file is the durable selection surface; Rust daemon lifecycle remains
the authority for deciding whether an existing process can be reused.

## Implementation Status

The Stage 3 contract is implemented in the following layers:

- `crates/exo-reexec` provides workspace discovery, candidate validation, loop
  prevention, and Unix process replacement.
- The `exo`, `exo-mcp`, `exohook`, and `exohistory` entry points invoke the
  shared selector before normal startup.
- Rust daemon lifecycle records executable and instance identity, performs the
  bounded probe, and replaces stale or wedged instances.
- The VS Code `DaemonChannelServer` resolves the same workspace-local binary,
  re-enters daemon ensure before socket reuse, and resets connections and cache
  state when the daemon instance changes.

The repository's broader daemon and extension suites exercise lifecycle
replacement and reconnection. Focused process-replacement coverage inside
`exo-reexec` remains useful hardening work.

## Drawbacks

Workspace-local selection deliberately trusts executable configuration checked
into the workspace. This resembles other development-environment mechanisms:
entering an unfamiliar checkout can select code from that checkout. Candidate
containment limits accidental path escape, while repository trust remains the
meaningful security boundary.

Selection also favors continuity when configuration is absent or invalid. The
current binary keeps running, which preserves command availability but can
make a missing local build less obvious. Diagnostics that explain why a
candidate was skipped would improve this experience.

Linked worktrees can select different development builds while sharing one
project daemon endpoint. Identity-aware replacement keeps execution correct,
but alternating between those worktrees can restart the daemon more often.

## Alternatives

### Editor-specific binary configuration

A VS Code setting can point the extension at a build, but terminals, MCP, and
other editors would still select independently. The workspace file gives every
client the same durable input and keeps editor configuration focused on editor
behavior.

### Reinstall after every build

Installing or copying each successful build makes the global binary current,
but it turns a local compile into a machine-wide mutation and relies on every
build path remembering the installation step. Workspace selection keeps local
development local.

### Filesystem watching in each client

Clients can watch a binary and restart their own subprocesses when it changes.
That approach does not establish whether the shared daemon is the expected
instance. Central daemon identity and bounded probing provide one lifecycle
decision that all clients can observe.

## Stage 4 Criteria

RFC 10179 can advance to Stable when the supported-platform contract and its
verification are complete. The remaining criteria are:

- define and implement Windows behavior equivalent to Unix process
  replacement;
- apply the Rust selector's relative-path and canonical-containment rules in
  the VS Code resolver, with focused parity tests;
- add focused tests for workspace discovery, candidate containment, loop
  prevention, and process delegation in `exo-reexec`;
- add user-facing diagnostics for skipped or invalid configured candidates;
- incorporate any future configuration location only after the project and
  workspace configuration model adopts it.
