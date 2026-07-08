<!-- exo:10195 ulid:01ktyev720322rf9bqxkeqmmmg -->

# RFC 10195: Daemon Lifecycle Authority and Shared Perception Surfaces

**Feature**: runtime-lifecycle-shared-perception
**Status**: Stage 1 (Proposal)
**Promotion Approval**: User approved Stage 1 promotion in Codex chat on 2026-06-19 before implementation began.

## Summary

This Stage 1 RFC proposes two connected but differently scoped directions:

1. The VS Code daemon should converge on the same ownership principle as the durable MCP proxy: one layer owns process lifecycle and identity, and clients do not duplicate restart policy.
2. Exosuit should eventually provide a host-neutral shared perception surface that can show cockpit/workbench state without requiring literal access to the user's VS Code sidebar.

Both items belong to the current epoch's product arc, but they are separate
implementation tracks. The daemon lifecycle work is the current implementation
scope for this RFC. The cockpit/workbench direction now follows the
[lane-centered workbench design package](../../design/lane-centered-workbench/README.md)
rather than the earlier read-only SvelteKit spike.

## Context

The durable MCP proxy needed a stable parent process because Codex owns the stdio MCP process lifecycle and does not reliably restart the server when the Exo binary changes. The proxy/worker split lets Exo own worker identity, replacement, restart diagnostics, and request retry boundaries.

The VS Code daemon is different. VS Code already calls Rust-owned lifecycle code through `exo --format json --direct daemon ensure --workspace <root>` and then connects to the reported Unix socket. Rust owns daemon runtime paths, PID files, identity files, stale daemon replacement, and PID locking. The extension also has client-side stale-binary checks, reconnect behavior, read-lane retry, and TraceCache invalidation.

That means the next improvement should not be another proxy process around the daemon. It should make Rust daemon lifecycle the single authority and reduce the extension to a thinner client that reconnects, invalidates cached roots, and reports diagnostics.

## Current-Epoch Shape

A bounded current-epoch slice should focus on daemon lifecycle authority and sidebar resync:

- Keep a machine-readable daemon status surface with daemon PID, instance ID,
  socket path, identity path, executable identity, and bounded health
  observations.
- Make `daemon ensure` the authoritative stale-binary and unresponsive-runtime
  replacement path for every client. Reuse requires both matching recorded
  identity and a successful bounded probe for the exact daemon instance.
- Keep VS Code as a lifecycle client: it reconnects when ensure reports a new
  instance, resets all socket lanes, and invalidates TraceCache without
  signaling daemon processes itself.
- Resolve project identity once at the tool boundary and carry that resolved
  project through status, steering, storage, and daemon request handling.
  Status must not repeatedly rediscover the same worktree or serialize
  independent repository observations.
- Validate the observed sidebar out-of-sync case: after daemon restart, state mutation, or reconnect, sidebar providers should not keep stale epoch/phase/task state.

## Language-Model Tool Reliability Contract

An agent calls the Exo tool available in its environment. Project resolution,
daemon discovery, stale or wedged runtime replacement, reconnection, and cache
invalidation happen behind that tool boundary. A successful repair returns the
requested command response; lifecycle commands are not normal recovery
instructions for the agent.

Every daemon process has a non-reusable runtime identity: instance ID, PID, and
process-start identity. The client probes that exact instance before reuse and
revalidates process identity before each termination signal. This prevents a
stale PID file or PID reuse from authorizing a signal to another process.

Pure requests may be replayed with the same request ID after automatic repair.
Write and exec requests require a durable request/outcome record before they
can be recovered transparently: reconnecting clients must query or replay the
same globally unique request ID and receive the recorded response without
executing a committed mutation twice. Until that ledger is implemented, the
non-replay safety boundary remains in force.

## Cockpit / Workbench Direction

The shared perception surface should use the same Exo state surfaces as VS Code
rather than copying VS Code UI internals. The current design authority is the
[lane-centered workbench design package](../../design/lane-centered-workbench/README.md).

Working direction:

- Treat a lane as the observable execution stream, not a prettier name for a
  branch, worktree, pull request, task list, phase, or chat thread.
- Keep the domain model independent from VS Code TreeView specifics. VS Code
  should be one adapter, not the source of truth for cockpit/workbench state.
- Reuse daemon status, trace-backed freshness, CommandSpec surfaces, and project
  state APIs as implementation substrate.
- Keep the dormant `packages/exosuit-cockpit` package buildable while the next
  implementation starts from the lane-centered workbench design.

The read-only SvelteKit cockpit spike is retained only as historical evidence
that a browser host can render Exo state. It is not the current implementation
target for this phase.

## Open Questions

- What transaction boundary should atomically associate a mutating command's
  durable state change with its replayable response?
- How long should completed daemon request outcomes be retained, and which
  bounded maintenance path removes expired entries?
- What is the minimum lane-centered workbench slice that proves a lane can be
  created, focused, and resumed from canonical project state?
- Which daemon/API surfaces should the first lane workbench implementation
  consume directly?

## Recommendation

Proceed first with daemon lifecycle/status hardening and sidebar resync
validation. Treat the lane-centered workbench package as the design source for
the next shared-perception implementation slice.
