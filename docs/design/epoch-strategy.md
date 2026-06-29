# Epoch Strategy

This document is a working strategy artifact for the current Exo epoch. It is
also an example of the kind of structure Exo should eventually encode directly.

The purpose is to help successive agents distinguish between:

- a good idea,
- work that moves the product's core loop forward,
- enabling work that makes the core loop trustworthy, and
- hygiene that preserves the system but should not be mistaken for product
  momentum.

## Terminology Note

The word "epoch" may eventually become "arc". This document uses "epoch" to
match the current Exo command surface, but the intended concept is an arc of
product movement: a coherent period of work organized around a product thesis,
not a short calendar window.

## Product Thesis

Exo should make human/agent software work feel like a shared, steerable system.
The user and agents should be able to see where the work is, understand what
should happen next, trust the recorded state, and carry work through review and
delivery without losing context.

The current epoch is about making that loop real across Codex, VS Code, MCP,
daemon state, and GitHub review/delivery.

## Triage Classes

Use these classes when adding ideas, RFCs, goals, or phases.

### Core-loop gap

Work that directly closes a missing part of Exo's product promise.

Core-loop work changes what Exo can do for a human/agent collaboration. It
usually deserves current-epoch attention if the missing loop is showing up in
daily dogfood.

Examples:

- Browser cockpit / shared sidebar perception for Codex.
- Pull requests as first-class workflow artifacts.
- Agent-facing steering based on attached PR checks, reviews, and merge state.

### Enabler

Work that does not itself complete the product loop, but unlocks or simplifies
core-loop work.

Enablers should be scheduled near the core-loop gap they unblock.

Examples:

- Durable `exo-mcp` proxy and hot-swappable worker.
- Daemon lifecycle authority and status diagnostics.
- Shared command text parsing when it prevents CLI/MCP divergence.

### Trust hygiene

Work that keeps Exo's state and operations trustworthy.

Trust hygiene is essential when trust is actively degraded, but it should not be
confused with user-visible product movement.

Examples:

- Sidecar write ownership and stale writer fencing.
- SQL dump/projection policy.
- RFC identity repair and generated-file ownership rules.

### Exploration

Work that may become important but has not yet been tied to a product loop,
dogfood pain, or delivery path.

Exploration should be captured, but not promoted into the current epoch without
a clear "why now".

## Strategic Fields

Until Exo has first-class fields for this, every significant phase/RFC/idea
should be expressible with these answers:

- **Strategic class**: core-loop gap, enabler, trust hygiene, or exploration.
- **Product promise**: shared perception, steering, persistence trust, execution
  continuity, or delivery lifecycle.
- **Why now**: the dogfood pain or opportunity that makes this timely.
- **Proof**: the observable behavior that would show the work moved the product.
- **Non-goal**: what tempting adjacent work is intentionally out of scope.

## Current Epoch Bets

### Durable MCP runtime

- **Strategic class**: enabler.
- **Product promise**: execution continuity and agent ergonomics.
- **Why now**: Codex uses MCP as the primary Exo interface, and stale or
  host-owned MCP processes make the workflow unreliable.
- **Proof**: the Codex plugin launches `exo-mcp`; the proxy survives worker
  replacement; MCP tool discovery and calls remain stable during dogfood.
- **Non-goal**: solving every daemon/sidebar lifecycle issue in the same slice.

### Browser cockpit / shared sidebar perception

- **Strategic class**: core-loop gap.
- **Product promise**: shared perception.
- **Why now**: Codex has no sidebar. The user currently keeps VS Code open with
  the full repo loaded just to see Exo state while working in Codex.
- **Proof**: a user and agent can inspect the same current phase, goals, tasks,
  RFCs, inbox, and active PR state from a browser surface without relying on the
  literal VS Code sidebar.
- **Non-goal**: a full write-capable replacement for the VS Code sidebar in the
  first slice.

### Pull requests as first-class workflow artifacts

- **Strategic class**: core-loop gap.
- **Product promise**: delivery lifecycle and steering.
- **Why now**: Exo can guide planning and implementation, but work often escapes
  into GitHub during commit, checks, review, and merge. The user then does
  manual steering to keep task completion, RFC stages, and PR reality aligned.
- **Proof**: Exo can create or attach a PR to a task, goal, phase, and/or RFC;
  `status`, `task complete`, `goal complete`, and `phase finish` steer based on
  check state, failing check logs, unresolved review threads, comment
  resolution state, draft state, mergeability, and merge completion.
- **First-class workflow**: Exo should cover PR creation and attachment, check
  inspection, failing-check triage, unresolved comment/thread listing, and
  review-thread resolution after fixes. These operations should remain
  interoperable with `git`, `gh`, GitHub MCP, and user-driven GitHub workflows.
- **Non-goal**: replacing `git`, `gh`, GitHub MCP, or user-owned GitHub
  workflows. Exo should integrate with them while describing the work in Exo
  terms.

### Daemon lifecycle authority and sidebar resync

- **Strategic class**: enabler.
- **Product promise**: shared perception and execution continuity.
- **Why now**: the VS Code sidebar has shown stale/out-of-sync state after
  reload and reconnect. Codex dogfood increases the cost of state drift because
  the sidebar is already not the agent's primary surface.
- **Proof**: daemon identity/status is Rust-owned; VS Code is a thinner client;
  sidebar providers invalidate and refresh after daemon restart, reconnect, and
  state mutation.
- **Non-goal**: adding a second proxy layer around the daemon.

### SQL dump and projection policy

- **Strategic class**: trust hygiene.
- **Product promise**: persistence trust.
- **Why now**: generated SQL/projection files and sidecar/repo policy confusion
  can make agents believe the wrong state source is canonical.
- **Proof**: agents and users can tell which files are source, which are
  generated projections, and which state lives in SQLite/sidecar without reading
  repository noise as truth.
- **Non-goal**: making every Exo state artifact human-managed Git source.

## How Agents Should Use This

When a new idea appears, do not only ask whether it is useful. Ask:

1. Which product promise does it serve?
2. Is it a core-loop gap, an enabler, trust hygiene, or exploration?
3. What current dogfood pain makes it timely?
4. What proof would demonstrate that the product is better after it lands?
5. What adjacent work should be explicitly excluded?

If the answers are weak, capture the idea without promoting it. If the answers
are strong, attach it to the current epoch and make the relationship visible in
phase, goal, RFC, and eventual PR state.

## Encoding Path

This document is a bridge. The desired end state is for Exo to encode the same
structure directly:

- phases and goals can carry strategic class and product promise,
- inbox items can preserve why-now and proof fields,
- RFCs can declare the loop they close,
- PRs can attach to goals/tasks/RFCs as delivery artifacts, including checks,
  review comments, resolution state, and merge state,
- browser and VS Code cockpit views can render the strategy alongside execution
  state.

Until then, this document should be treated as the current epoch strategy
reference.
