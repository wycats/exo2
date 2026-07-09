<!-- exo:10190 ulid:01ktcmqy99segnkk98mj488dd5 -->

# RFC 10190: Durable MCP Proxy and Hot-Swappable Exo Worker

**Status**: Stage 1 Proposal
**Feature**: mcp

## Summary

Exo's MCP integration should use a durable client-facing proxy named `exo-mcp` that keeps the MCP connection stable while the Exo implementation behind it is replaced, restarted, or selected for a specific workspace.

The proxy owns MCP framing, startup metadata, static tool metadata, workspace-source selection for each tool call, lightweight Exo project resolution for worker selection, worker lifecycle, and diagnostics. The worker owns Exo command semantics, command-time project validation, sidecar policy, command execution, confirmations, persistence, and steering.

For Codex plugin launches, the active workspace comes from Codex tool-call metadata. `exo-mcp` advertises support for `codex/sandbox-state-meta` during MCP initialize. On `tools/call`, it reads `_meta["codex/sandbox-state-meta"].sandboxCwd`; when that value is absolute, Exo resolves the project from that cwd and starts or selects a worker for the resolved workspace. A Codex plugin call with a missing or relative `sandboxCwd` returns diagnostics instead of falling back to the proxy process cwd. Local/manual MCP launches continue to work when they are explicitly launched from an Exo-resolvable process cwd.

## Problem

MCP clients commonly treat their configured server process as the durable tool connection for a chat or workspace. If that process is also the full Exo implementation, every rebuild or dogfood update creates a runtime freshness problem:

- keeping the server process alive can preserve stale Exo semantics;
- killing the process forces the client to reconnect and may require a fresh chat or host reload;
- self-reexec helps only when the process can safely replace itself and replay the current request;
- mutations and exec effects need stable invocation identity and recorded-outcome recovery so transport repair does not execute them twice.

Codex plugin packaging adds a second workspace-binding problem. A packaged stdio MCP server can start with its process cwd set to the plugin package root. That directory is part of the plugin install, not the user's active Exo workspace. The active workspace is available on each tool call through Codex sandbox metadata. `exo-mcp` needs to bind workers to the resolved Exo project for the call, not to the package cwd that happened to launch the process.

The missing concept is a small stable proxy whose job is MCP connection management, host metadata interpretation, and worker supervision.

## Design

### Durable Entrypoint

Add and use a separate binary named `exo-mcp`.

The Codex plugin and other durable MCP integrations launch `exo-mcp`. The proxy owns the client stdio streams, parses MCP JSON-RPC frames, advertises server capabilities, serves static tool metadata, forwards tool calls to a worker, and writes MCP responses back to the client.

The proxy is semantically thin. It may call the normal Exo project resolver to turn a selected cwd into a worker routing identity, but it does not parse Exo CLI-shaped commands, open Exo SQLite databases, write `agent_events`, touch sidecar Git state, or perform Exo project mutations. Those responsibilities stay in the worker.

### MCP Initialize

On `initialize`, `exo-mcp` returns the normal MCP tools capability and advertises the Codex workspace metadata contract:

```json
{
  "capabilities": {
    "tools": {},
    "experimental": {
      "codex/sandbox-state-meta": {}
    }
  }
}
```

This advertisement tells Codex that `exo-mcp` can consume per-tool-call sandbox state. It does not make Codex metadata part of Exo command semantics; it gives the proxy a host-supported way to choose the Exo workspace for the call.

### Static Tools List

`tools/list` should be served by the proxy without resolving an Exo workspace and without starting a worker. The current implementation still forwards `tools/list` to the worker; the static proxy-owned listing is part of the remaining implementation work.

The baseline list includes the CLI-shaped `exo-run` tool defined by RFC 10200. The proxy may generate this static schema at build time from the same Rust source used by the worker, or it may keep a small checked-in schema owned by the proxy package. In both cases, `tools/list` must remain available when the process cwd is the plugin package root, when no Exo workspace can be resolved yet, and when no worker is running.

A worker-side tool listing method may exist for diagnostics or schema identity checks, but MCP client `tools/list` readiness does not depend on it.

### Workspace Binding On Tool Calls

`tools/call` is the first point where Exo workspace resolution is required.

The proxy evaluates workspace sources in this order:

1. Tool-call metadata: `_meta["codex/sandbox-state-meta"].sandboxCwd` when present and absolute.
2. Process cwd: used only when the proxy is in an explicit local/manual launch mode and the cwd resolves as an Exo project.
3. Compatibility metadata: session/global-state hints used only for diagnostics and transitional recovery.

The selected cwd is passed through Exo's normal project resolver. The success condition is that Exo resolves the cwd as the current project. The proxy does not use `exosuit.toml` existence as a separate host-specific shortcut.

For Codex plugin launches, a missing, relative, or otherwise unusable `sandboxCwd` is a workspace-binding error. The proxy reports the missing or invalid host metadata and the observed process cwd for diagnosis, but it does not route the call through process cwd. This prevents a packaged or source-dogfood plugin root from becoming the selected Exo workspace by accident.

If no source can resolve an Exo project, the MCP-visible error reports the observed sources: whether tool-call `sandboxCwd` was present, whether it was absolute, the process cwd, whether process cwd resolved, whether compatibility/session hints were present, and the resolver error for each attempted source.

### Result-Scoped Worker Selection

The proxy starts or selects workers by resolved Exo workspace identity.

A worker identity includes the resolved workspace root, project id, state root, database path, sidecar key and root when applicable, worker protocol version, tool schema identity, and command spec identity. A plugin-launched proxy can therefore serve a Codex conversation whose active workspace changes between calls, while each call routes through the worker for the workspace Exo resolved from that call's metadata.

The proxy may keep a small worker pool keyed by workspace root, or it may run one active worker and replace it when the resolved workspace changes. The externally visible rule is the same: a call for workspace A must not be routed through a worker whose identity reports workspace B.

### Replaceable Worker

The worker runs the normal Exo implementation, for example through an internal mode such as `exo mcp worker`.

The worker owns:

- command-time project validation and sidecar policy after the proxy has selected the cwd and routed to a worker;
- `exo-run` command parsing and help routing;
- machine-channel dispatch;
- confirmation and workflow-confirmation handling;
- response shaping;
- command-event logging and post-write policy;
- sidecar and daemon interactions.

The worker speaks a narrow proxy protocol over stdin/stdout. The first implementation may reuse newline-delimited JSON-RPC and the existing MCP request/response shape where possible. The important boundary is that the proxy can replace the worker without replacing the MCP client connection.

### Proxy-Worker Protocol

The first protocol version supports these methods:

- `worker/hello`: worker startup identity and supported protocol version.
- `worker/classify`: preflight classification for one MCP tool call.
- `worker/call`: execute one MCP tool call and return an MCP-shaped result.
- `worker/shutdown`: graceful worker drain before planned replacement.
- `worker/status`: diagnostics for the current worker identity and last error.

All request ids are proxy-local. The proxy preserves the MCP client's original request id at the client boundary and may use separate worker ids internally so a restarted worker cannot answer an old request by accident.

Worker protocol errors use JSON-RPC error objects. Exo command errors remain MCP tool results unless the worker cannot parse the proxy protocol itself.

### Effect Classification Boundary

The proxy needs request effect information to choose between read replay and durable outcome recovery. The worker supplies that information through `worker/classify` using the same command registry and `CommandSpec` effect metadata as execution.

The classification result includes:

- `effect`: `pure`, `write`, or `exec`;
- `retry_policy`: `auto_retry_read` for pure reads or `auto_recover_outcome` for writes and execs;
- `requires_confirmation`: whether Exo requires execution confirmation;
- `tool_schema_identity`;
- `command_spec_identity`.

The proxy may cache this result only for the in-flight request it is supervising.

### Worker Identity And Hot-Swap

On startup, the worker reports an identity envelope before serving tool calls. The envelope includes:

- executable path;
- executable identity: stable hash, mtime, and size;
- workspace root;
- project id;
- state root;
- database path;
- sidecar key and sidecar root when applicable;
- worker protocol version;
- tool schema identity;
- command spec identity.

The proxy compares this identity with the current on-disk worker binary and the workspace selected for the current request. If the worker identity is stale or reports the wrong workspace, the proxy restarts it before forwarding another request.

The proxy may replace the worker when:

- the worker binary identity changes on disk;
- dogfood restart asks for planned worker replacement;
- the worker exits or crashes;
- the worker reports an incompatible protocol or project identity.

Pure reads may be retried automatically across a worker restart. Writes and execs preserve one invocation identity across worker replacement and retry the transport with that identity. The daemon returns a completed response from its durable outcome ledger without executing the mutation twice.

### Outcome Recovery And Indeterminate Results

The daemon reserves a write or exec request identity before command execution and records the complete response before socket delivery. Reusing the same identity and payload returns the recorded response. Reusing an identity with a different payload is rejected.

If a daemon instance disappears after reserving an identity, recovery follows
the built command's RFC 10195 recovery class. For `atomic_project_state`, the
replacement daemon queries the canonical project outcome: it replays a
committed response and resumes idempotent finalization, or executes when no
outcome exists because the interrupted transaction did not commit. For
`external_at_most_once`, the replacement returns a structured
`daemon.request_outcome_indeterminate` result identifying the request and
effect rather than replaying an external effect.

If an `external_at_most_once` request has no recorded outcome after automatic
worker recovery, the proxy returns an MCP transport error with:

- `code`: `exo.retry_required`;
- `effect`: `write` or `exec`;
- `worker_restart_reason`;
- `request_summary`;
- whether the original worker request started.

The proxy keeps hidden confirmation tickets and workflow confirmation payloads out of human text. Automatic recovery reuses the original tool-call parameters and durable invocation identity rather than constructing a new command.

### Diagnostics

The proxy exposes enough diagnostics to make runtime drift and workspace binding visible. A diagnostic response should report:

- proxy executable path and identity;
- current worker pid;
- current worker executable path and identity;
- workspace source selected for the current call;
- tool-call `sandboxCwd` presence and value classification;
- process cwd;
- resolved worker workspace root and database path;
- worker protocol version;
- tool schema identity;
- command spec identity;
- restart count;
- last restart reason;
- last worker error.

Dogfood activation treats the proxy and worker as separate identities. A healthy dogfood run proves that the MCP client is connected to the expected proxy and that the proxy is routing through the expected worker for the active workspace.

## Packaging

`exo-mcp` is installed anywhere the Exo Codex plugin is expected to work. The supported install and dogfood paths install both `exo` and `exo-mcp`.

The Codex plugin launches:

```json
{
  "command": "exo-mcp",
  "args": []
}
```

`exo mcp serve` may remain as a direct manual entrypoint. Dogfood health uses the proxy path because it exercises the durable runtime boundary this RFC defines.

RFC 10193 owns Codex-facing packaging, plugin README guidance, reload ergonomics, and cockpit strategy. This RFC owns the proxy and worker behavior.

## Relationship To Sidecar Durability

RFC 10189 says pure reads may record durable local SQLite activity but must not wait for sidecar Git portability work. This RFC addresses the runtime side of the same dogfood issue: after read-path policy is fixed, an old MCP worker must stop serving future calls.

The durable proxy makes sidecar dogfood meaningful because the client-facing MCP connection no longer pins an old Exo implementation. When the worker binary changes, the proxy can route future reads through the new worker while preserving the MCP connection.

RFC 10191 owns sidecar write ownership and stale writer fencing. This RFC keeps the MCP runtime fresh enough that RFC 10191's ownership decisions are made by the expected worker.

## Incremental Delivery

### Phase 1: Proxy Skeleton And Codex Workspace Binding

Add `exo-mcp`. The proxy serves `initialize`, static `tools/list`, and `tools/call`. It advertises `codex/sandbox-state-meta`, reads tool-call `sandboxCwd`, resolves Exo from that cwd when absolute, and starts a worker for the resolved workspace. Local/manual launches use process cwd when it resolves as an Exo project.

### Phase 2: Worker Identity And Hot Replacement

Add the worker startup identity envelope. Before each request, the proxy checks whether the worker binary identity still matches disk and whether the worker reports the workspace selected for the call. Diagnostics show proxy identity, workspace source, worker identity, restart count, and last restart reason.

### Phase 3: Request Effect Handling

Teach the proxy to distinguish read retry from write/exec drain or retry-required behavior. The proxy gets effect from `worker/classify` and keeps Exo command semantics in the worker.

### Phase 4: Dogfood And Plugin Integration

Keep plugin packaging, install scripts, dogfood verification, and restart commands on `exo-mcp`. Dogfood proves that replacing the worker preserves the MCP connection, routes through the active Codex workspace, and stops stale read-path behavior from continuing.

## Acceptance Criteria

- `initialize` advertises tools plus `experimental["codex/sandbox-state-meta"]`.
- `tools/list` returns the static `exo-run` definition without resolving a workspace or starting a worker.
- A Codex tool call with absolute `sandboxCwd` resolves Exo from that cwd and starts/selects a worker for the resolved workspace.
- A local/manual MCP launch from an Exo-resolvable process cwd still serves `exo-run` successfully.
- A plugin launch from the plugin package root serves `tools/list` and then routes `tools/call` through the active Codex workspace.
- Diagnostic errors report tool-call `sandboxCwd`, process cwd, compatibility/session metadata presence, and resolver results.
- Replacing the `exo` worker binary between two read calls restarts the worker without disconnecting the MCP client.
- A crashed worker is restarted and a later pure read succeeds on the same MCP connection.
- Worker identity includes executable identity, workspace root, project/state paths, protocol version, tool schema identity, and command spec identity.
- Pure reads may be retried across a restart; writes and execs are drained or return a retry-required error and are never silently replayed.
- Confirmation and workflow-confirmation data remains hidden and is never replayed by the proxy without a user-approved retry.
- The plugin launches `exo-mcp`.

## Stage Readiness

This RFC is a Stage 1 proposal. Readiness for Stage 2 depends on aligning the implementation with the acceptance criteria above, especially Codex metadata binding, static proxy-owned `tools/list`, and workspace-scoped worker selection.

Stage 1 approval was recorded during the open PR backlog salvage sweep before the clean public root cutover.

## Non-Goals

- Replacing the Exo daemon.
- Exposing arbitrary shell or process supervision to agents.
- Making the proxy a second Exo API surface.
- Moving command execution, SQLite access, sidecar policy, or command parsing into the proxy.
- Silently replaying interrupted mutations after a worker crash.
- Guaranteeing that one proxy binary can handle incompatible future MCP tool schemas indefinitely.

## Related RFCs

- RFC 10193: Codex Integration and Cockpit Adapter
- RFC 10200: CLI-Shaped exo-run MCP Transport
- RFC 10189: Sidecar Sync Contract and Machine Portability Policy
- RFC 10191: Sidecar Write Ownership and Stale Writer Fencing
- RFC 10180: Storage Disposition: Canonical State, Configuration, and Documents
- RFC 0125: Exosuit Capability Tree Machine Channel v1
