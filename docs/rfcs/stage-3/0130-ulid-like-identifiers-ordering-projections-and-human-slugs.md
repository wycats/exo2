<!-- exo:130 ulid:01kg5kp2hfvwwc67j1vyh5arfq -->

# RFC 130: ULID-like Identifiers, Ordering Projections, and Human Slugs


# RFC 0130: ULID-like Identifiers, Ordering Projections, and Human Slugs

## Summary

This RFC proposes a consistent model for **identity**, **ordering**, and **human-readable names** across Exosuit artifacts:

- **Stable identity** is an immutable, globally unique identifier (ULID-like).
- **Ordering** is a projection of storage shape (e.g. array order), not identity.
- **Human slugs** exist alongside stable IDs and have low-friction uniqueness rules.

The goal is to eliminate drift between “what we call something”, “where it appears”, and “what it _is_”, while keeping UX friendly.

## Motivation

Exosuit increasingly treats documents and derived views as projections. Today, several artifacts implicitly treat numeric order as identity (e.g. “Phase 44”), which creates problems:

- **Renumbering becomes a breaking change**: identity shifts when ordering changes.
- **UI/CLI ambiguity**: “44” might refer to order, filename stem, or internal identity.
- **Merges are fragile**: two people can independently create “the next number”.
- **Human intent is lost**: the thing humans remember is often a short name, not a number.

We want:

- Stable references that never change.
- A clear separation between identity and ordering.
- Human-friendly handles that are easy to create and resolve.

## Detailed Design

### Terminology

- **ID**: Stable identifier (ULID-like). Immutable. Unique.
- **Slug**: Human-friendly name (e.g. `polish-rfc-stage-indicator`). Mutable (within rules). Unique within a scope.
- **Order**: A projection determined by the container/document representation (e.g. array order). Not a stable identity.

### 1) Stable IDs (ULID-like)

Every artifact that participates in linking/referencing SHOULD have a stable ID.

Properties:

- **Uniqueness**: no collisions within a workspace.
- **Immutability**: IDs never change once assigned.
- **Opaque**: IDs are not user-facing by default.

Why ULID-like:

- Lexicographically sortable by time-of-creation is useful for debugging and “recent first”.
- Unlike incrementing integers, IDs do not require global coordination.

Non-goals:

- ULID ordering is not a substitute for explicit ordering. It is at best a default view.

### 2) Ordering is a projection

Ordering MUST be treated as a property of the _container_, not the _contained item_.

Examples:

- A task list’s order is the order of the array entries in `task-list.toml`.
- A list of phases shown in the UI is a projection (filter + sort) over a set of phase records.

Implications:

- Reordering does not change identity.
- “Phase N” can exist as a UI label, but it must be derived from the current projection.
- Any stable links MUST resolve via IDs (and optionally slugs).

### 3) Human slugs (low-friction uniqueness)

Humans need something better than ULIDs.

We introduce optional **slugs** for artifacts:

- Slugs SHOULD be lowercase kebab-case.
- Slugs SHOULD be unique within a well-defined scope (e.g. within an Epoch, within `docs/rfcs/`, within a task list).
- Slugs MAY change, but changes should be visible (e.g. via steering or a rename command).

Uniqueness rules (low-friction):

- On creation, if the desired slug is free: use it.
- If it’s taken: automatically suffix with `-2`, `-3`, … (or another deterministic suffix strategy).

Resolution rules:

- User inputs may refer to artifacts by **ID** or **slug**.
- If an input is ambiguous (same slug in multiple scopes), the system MUST require disambiguation and provide steering.

### 4) Mapping existing “numbered identity”

Some existing artifacts currently use numeric IDs (e.g. RFC number, Phase number).

This RFC does not require immediate migration, but it establishes direction:

- Numeric labels MAY remain as a **projection** (display order, sequence number).
- The underlying identity SHOULD move to a stable ID.
- Tools SHOULD provide steering toward stable references when ambiguity appears.

Practical rule:

- If a thing is referenced across files, across time, or via commands, it should have a stable ID.

### 5) Interaction with RFC numbering

RFC numbers (e.g. `0057`) are already stable enough for many workflows, but:

- They are globally coordinated and can be expensive to manage.
- They encourage “number-as-identity” thinking.

This RFC does not propose removing RFC numbers, but it does propose using them _as a handle_, not as “the model”.

## Drawbacks

- Adds conceptual overhead: ID vs slug vs order.
- Requires careful steering to avoid confusing UX.
- Implies migrations for legacy numeric-only artifacts.

## Alternatives

- **Use integers everywhere**: simple, but coordination and renumbering problems persist.
- **Use UUIDs only**: stable but unfriendly; encourages copy/paste of opaque values.
- **Use slugs only**: friendly, but renames and collisions make long-lived references fragile.

## Unresolved Questions

1. ~~Which artifacts must have stable IDs first (tasks, phases, walkthrough entries, RFCs, ideas)?~~ **Resolved: See Implementation Addendum - Slice 1 covers epochs, phases, and tasks.**
2. ~~What are the exact scopes for slug uniqueness (workspace-wide vs per-file vs per-artifact-type)?~~ **Resolved: See Implementation Addendum A.2.**
3. ~~What is the canonical UX for disambiguation (prompting vs steering suggestions)?~~ **Resolved: Fail with steering, agent/LLM retries with different input.**

## Future Possibilities

- A unified resolver: `exo resolve <id-or-slug>`.
- A rename command that preserves back-references.
- A consistent "display label" system: e.g. `Phase 44: polish-stage-indicator` where `44` is projection and the slug is human memory.

---

## Implementation Addendum (Stage 1 Amendment)

_Added: 2026-01-01 for Phase 3 implementation readiness. Incorporates decisions from RFC 0069._

### A.1 ULID Library Requirements

**Rust:**

```toml
# Cargo.toml
[dependencies]
ulid = "1.1"
```

**TypeScript:**

```json
{
  "dependencies": {
    "ulid": "^2.3.0"
  }
}
```

**Format Specification:**

- 26-character Crockford Base32 encoding
- Lexicographically sortable by creation time
- Example: `01HZVY8XMQK6YXGZJ4V3PNRB9W`

**Validation Regex:**

```
^[0-9A-HJKMNP-TV-Z]{26}$
```

### A.2 Slug Uniqueness Scopes (Normative)

| Artifact Type      | Slug Scope          | Example                                                   |
| ------------------ | ------------------- | --------------------------------------------------------- |
| Epoch              | Global (all epochs) | `map-epoch-2-steering-excellence` unique across workspace |
| Phase              | Within parent epoch | `phase-state-machine` unique within its epoch             |
| Task               | Within parent phase | `implement-ulids` unique within its phase                 |
| Idea               | Global (all ideas)  | `deferred-zero-arg-channel` unique across ideas           |
| Feedback           | ULID only, no slug  | -                                                         |
| Walkthrough/Strike | ULID only, no slug  | -                                                         |

### A.3 Slug Collision Handling (Normative)

**Decision:** Adopt RFC 0069's "fail with steering" approach over auto-suffix.

**Rationale:**

- LLM workflows can retry quickly with clear steering
- Humans prefer intentionally distinct names over auto-generated suffixes
- Avoids confusing names like `polish-rfc-2`, `polish-rfc-3`

**Error Format:**

```
Error: Slug collision in scope "epoch:map-epoch-2"
  Slug "phase-state-machine" already exists.

Suggestion: Choose a different slug. Options:
  - phase-state-machine-v2
  - state-machine-ulids
  - phase-3-state-machine
```

**Resolution Rules:**

1. Resolve within command's natural scope first (e.g., task operations → active phase)
2. If exactly one match: proceed
3. If zero matches: fail with "not found" steering
4. If multiple matches: fail with disambiguation steering listing candidates

### A.4 Canonical Reference Syntax

**Format:** `type@<ulid>`

**Supported Types:**

- `epoch@<ulid>`
- `phase@<ulid>`
- `task@<ulid>`
- `idea@<ulid>`
- `strike@<ulid>`

**Display Format (Human Output):**

```
Phase "Phase State Machine" (phase@01HZVY8XMQ...)
Task "implement-ulids" (task@01HZW0K3NP...)
```

**JSON Output:**

```json
{
  "id": "01HZVY8XMQK6YXGZJ4V3PNRB9W",
  "ref": "phase@01HZVY8XMQK6YXGZJ4V3PNRB9W",
  "slug": "phase-state-machine",
  "title": "Phase State Machine + ULIDs"
}
```

### A.5 Migration Command Specification

**Command:**

```bash
exo plan migrate-ids [--dry-run]
```

**Algorithm:**

1. **Generate ULIDs:** For each epoch, phase, and task without an `id` field:
   - Generate new ULID
   - Set `id = "<ulid>"`
   - Preserve current string identifier as `slug`

2. **Update Cross-References:** Scan all files for references to old identifiers:
   - SQLite-backed epoch, phase, goal, task, idea, and inbox records
   - RFC files and durable docs that contain human-authored references
   - SQL dumps generated from Exo state
3. **Validate:** Ensure no broken references after migration

4. **Report:**
   - List all generated ULIDs with their slugs
   - List all updated cross-references
   - Flag any ambiguities requiring manual resolution

**Dry-Run Output:**

```
Migration Preview (--dry-run):

Epochs (2):
  + id: 01HZVY... slug: map-epoch-1-foundation (was: map-epoch-1-foundation)
  + id: 01HZVZ... slug: map-epoch-2-steering-excellence (was: map-epoch-2-steering-excellence)

Phases (44):
  + id: 01HZW0... slug: phase-2.5-tool-surface (was: map-phase-2.5-tool-surface)
  + id: 01HZW1... slug: phase-3-phase-state-machine (was: map-phase-3-phase-state-machine)
  ...

Tasks (52):
  + id: 01HZW2... slug: implement-ulids (was: phase-ulids)
  ...

Cross-references to update: 12
No ambiguities detected.

Run without --dry-run to apply migration.
```

### A.6 Legacy ID Retention

**Policy:** Legacy identifiers are preserved as `slug` values permanently.

**Aliases:** During transition, old IDs may be stored in an `aliases` array:

```toml
[[epochs]]
id = "01HZVY8XMQK6YXGZJ4V3PNRB9W"
slug = "map-epoch-2-steering-excellence"
aliases = ["map-epoch-2"]  # Optional, for backward compatibility
title = "MAP Epoch 2: Steering Excellence"
```

**Resolution Order:**

1. Exact ULID match
2. Exact slug match (within scope)
3. Alias match (within scope)
4. Fail with steering if no match

### A.7 Implementation Slice 1: Plan Artifacts (Phase 3 Scope)

This slice covers:

1. **Epochs** in `plan.toml`:
   - Add `id` (ULID)
   - Preserve current identifier as `slug`

2. **Phases** in `plan.toml`:
   - Add `id` (ULID)
   - Preserve current identifier as `slug`

3. **Tasks** in `implementation-plan.toml`:
   - Add `id` (ULID)
   - Preserve current identifier as `slug`

4. **CLI Commands:**
   - Update `exo plan`, `exo phase`, `exo task` to accept ULID or slug
   - Always echo canonical reference in output
   - Implement resolution logic per A.3

**Deferred to Slice 2:**

- Ideas ULID migration (already have UUID-like IDs)
- Feedback ULID-only policy
- Walkthrough/strike entries

**Deferred Indefinitely:**

- RFC ULID identity (RFCs already have stable governance handles)
