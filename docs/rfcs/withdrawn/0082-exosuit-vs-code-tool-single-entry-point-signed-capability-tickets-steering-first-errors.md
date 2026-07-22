<!-- exo:82 ulid:01kmzxbczda618nxtdgg5xnhs1 -->

# RFC 82: Exosuit VS Code Tool: Single Entry Point + Signed Capability Tickets + Steering-First Errors

- **Stage**: 0
- **Reason**:

> **Status**: Withdrawn (superseded by RFC 0083)
>
> This RFC proposed a single-tool architecture with port-based dispatch. After implementation experience and research into LLM tool design constraints, we adopted RFC 0083's 14-tool hybrid architecture instead.
>
> **What was preserved**: Steering-first error design, capability tickets, no shell strings principle
> **Why withdrawn**: Empirical research shows specialized tools outperform single overloaded tool

---

# RFC 0082: Exosuit VS Code Tool: Single Entry Point + Signed Capability Tickets + Steering-First Errors

## Summary

Expose one VS Code Language Model Tool, `exosuit`, that provides a single navigable entry point for:

- running project workflows (`run`)
- locating canonical artifacts (`locate`)
- editing Exosuit-managed context (`edit`)

The tool accepts structured inputs (no shell strings), uses signed capability tickets for safety + replay, and treats error responses as a steering surface by returning a deterministic `next_call` to re-rail general Copilot.

## Motivation

The current “general Copilot + terminal” approach hits a hard limit:

- quoting/escaping failures are unpredictable and expensive to recover from
- partial failures leave the agent in an ambiguous state (“did it run?”)
- the model’s tool selection can drift when there are too many similar commands

We also want to avoid the opposite extreme: a tool per `exo` subcommand (tool confusion + prompt bloat + context flooding).

This RFC proposes a middle shape:

- one tool
- few stable concepts
- navigation by gradient (errors include explicit recovery calls)

## Coherence / Prior Art

This RFC intentionally aligns with existing Exosuit vocabulary:

- **Steering**: treat “what to do next” as a first-class output concept. Tool failures must include concrete recovery instructions.
- **Single-entry discipline**: preserve the goal of a single obvious pathway for running workflows, but move execution behind a structured LM tool to eliminate escaping/quoting failures.
- **Tasks as config-driven workflows**: treat `exosuit.toml` as the source of truth for runnable tasks.

## Goals

- Eliminate shell escaping/quoting issues by never accepting a free-form shell command string.
- Work well in **general Copilot**, not only `@exosuit`.
- Avoid “tool-per-command” explosion.
- Ensure “navigation” converges quickly via structured guidance (`next_call`).
- Provide safe friction for risky actions without relying on bespoke UI.
- Make it easy to discover local validation entry points (e.g. `.config/exo/hooks.toml`, projected pre-commit/pre-push lanes) via `locate`.

## Non-Goals

- Full parity with every `exo` subcommand initially.
- Arbitrary command execution.
- A rich UI surface; LM Tool results are primarily for the model.

## Detailed Design

### Terminology

- **Tool**: VS Code Language Model Tool `exosuit`.
- **Ticket**: a signed capability token describing exactly what can be done (and constraints).
- **Port**: a top-level concept area: `run`, `locate`, `edit`.
- **Navigation**: the tool’s ability to guide general Copilot to the right capability in ≤2 calls.

### Primary UX requirements

1. **Single entry point**: general Copilot should always be able to start with `exosuit`.
2. **Progressive disclosure**: avoid dumping large enumerations into the chat context.
3. **Steering-first failures**: tool errors must include concrete recovery instructions.
4. **No shell strings**: the tool never accepts arbitrary `cmd` strings.

### Tool Name

`exosuit`

### Input Schema (conceptual)

The tool supports three operations:

- `op: "request"` — “I want to do X” (may execute immediately)
- `op: "use"` — execute/continue using a ticket
- `op: "list"` — enumerate tasks/recipes/ports/artifacts with paging

```json
{
  "op": "request" | "use" | "list",

  "request": {
    "kind": "run" | "locate" | "edit",

    "query": "string (optional; messy NL intent)",

    "run": {
      "targetKind": "recipe" | "task" | null,
      "targetId": "string | null",
      "dryRun": "boolean (default false)"
    },

    "locate": {
      "what": "artifacts" | "context" | "rfc" | "docs" | null,
      "id": "string | null"
    },

    "edit": {
      "resource": "plan" | "tasks" | "walkthrough" | "decisions" | "ideas" | null,
      "action": "add" | "update" | "append" | null
    }
  },

  "ticket": "string (for op:use)",

  "input": "object (for op:use; ticket-scoped payload)",

  "confirm": "boolean (default false; for confirm-required tickets)",

  "list": {
    "kind": "tasks" | "recipes" | "ports" | "artifacts",
    "prefix": "string | null",
    "limit": "1..50 (default 20)"
  }
}
```

### Output Schema (conceptual)

All responses are structured and deterministic.

```json
{
  "status": "ok" | "needs_input" | "confirm_required" | "error",
  "message": "string",
  "data": "object | null",

  "ticket": "string | null",
  "next_call": {
    "op": "request" | "use" | "list",
    "...": "payload"
  } | null
}
```

### Capability Tickets

Tickets are intended to be:

- **scoped** (what can be done, to which resources)
- **signed** (cannot be forged by the model)
- **short-lived** (optional TTL) and/or **one-shot** (optional nonce)

Examples:

- a `run` ticket that allows running a named task with specific parameters
- an `edit` ticket that allows appending to `implementation-plan.toml` only
- a `locate` ticket that allows listing artifacts with paging

### Steering-first errors

If a request cannot be completed, the tool must return:

- a clear error message
- a deterministic `next_call` that either asks for missing input or guides discovery (typically `list` or a narrower `request`)

This is not just UX; it is the mechanism that keeps general Copilot from thrashing.

## Open Questions

- Ticket signing mechanism and key management (per-workspace vs per-install vs per-session).
- What “confirm_required” actions exist and what the default friction should be.
- How to map existing `exo` subcommands onto ports without losing important semantics.
- How much of “discoverability” should be in tool output vs documentation.

## Implementation Guidance

### Tool Surface Design (Superseded by RFC 0083)

The original proposal in this RFC described a single `exosuit` tool with three "ports" (run/locate/edit). This design has been superseded by [RFC 0083: Hybrid Tool Architecture](0083-hybrid-tool-architecture-for-lm-tools.md), which provides a more refined three-tier tool surface:

1. **Zero-arg orientation tools** (7 tools) - Pure, safe context queries
2. **Method-based dispatch tools** (5 tools) - Type-safe mutations
3. **Convenience zero-arg mutations** (2 tools) - High-frequency shortcuts

**What remains valid from this RFC**:

- Steering-first error design (every error includes `next_call`)
- Signed capability tickets for confirmation
- No shell strings (structured inputs only)
- Progressive disclosure via help ladder

**What changed**:

- Single tool → 14 specialized tools
- Port-based routing → direct tool calls
- Complex dispatch → method-based enums

**Migration path**: The VS Code extension should implement the RFC 0083 tool surface rather than the single-tool model originally described here. The steering principles and ticket-based confirmation flow remain unchanged.

Refer to RFC 0083 for:

- Complete tool enumeration
- Tool naming conventions
- Schema examples
- Implementation phases

### Relationship to Machine Channel Namespaces

The three "ports" (run/locate/edit) are **conceptual groupings** for user intent, now superseded by the RFC 0083 taxonomy:

- **Orientation tools** (zero-arg) → read operations (status, plan, phase, map, context)
- **Mutation tools** (method-based) → write operations (phase-ops, task-ops, plan-ops, rfc-ops, impl-ops)
- **Convenience tools** (zero-arg) → high-frequency writes (idea, add-task)

The capability tree (RFC 0125) provides flexible discovery; the RFC 0083 taxonomy provides the tool projection strategy.

### Tool Count Constraints (Empirical)

Research into LLM tool design reveals practical constraints:

1. **OpenAI recommends fewer than 20 functions** for optimal accuracy
2. **Tool schemas consume tokens**: 50+ tools can use 5,000-15,000 tokens
3. **Decision quality degrades** with too many tool choices

RFC 0083's 14-tool surface stays well under this limit while providing comprehensive coverage.

### Navigation Convergence

With the help ladder pattern, navigation converges in ≤2 calls:

1. `help(root)` → list of namespaces
2. `help(namespace)` → list of operations
3. `call(operation)` → execute

If the agent already knows the operation, it can call directly. Steering guides recovery for unknown paths.

**With RFC 0083**: Zero-arg orientation tools (`exo-map`, `exo-status`) provide immediate context without needing the help ladder at all.

