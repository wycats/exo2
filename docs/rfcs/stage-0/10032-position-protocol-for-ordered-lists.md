<!-- exo:10032 ulid:01kmzxefe80k4qpketh0cx5sqk -->


# RFC 10032: Position Protocol for Ordered Lists

## Summary

Introduce a single, reusable **Position protocol** for all ordered containers in Exosuit (phases-in-epoch, tasks-in-phase, RFC lists, etc.).

Companion RFC: RFC 10033 (Canonical ULIDs, Scoped Slugs, and RFC Corpus Control).

A Position is always interpreted as:

- **Container**: “which list are we editing?”
- **Placement**: “where in that list should the item go?”

Important: `prepend`/`append` do not require an item anchor, but they still require the **container**. In other words, the “parent” (the container) is the anchor that makes these operations meaningful.

This RFC defines:

- A concrete Position model (`before`, `after`, `prepend`, `append`).
- Resolution rules for anchors (stable IDs and/or scoped slugs).
- Deterministic error/steering behavior for ambiguity and collisions.

## Motivation

Manipulating ordered lists is one of the biggest sources of workflow friction and subtle bugs:

- “Move the rest of these tasks to a new phase” is conceptually simple but currently requires bespoke steps and hand-rolled logic.
- Numbered identities (e.g. `phase-74`) tempt tools/agents to treat order as identity.
- Operations are inconsistent across artifact types (tasks vs phases vs RFCs).

We need a **single mental model** and **single API** that:

- Works everywhere we have ordered lists.
- Is stable under reordering.
- Is friendly for humans and agents.

## Detailed Design

### Terminology

- **ContainerRef**: identifies a container that _has an order_ (e.g. “tasks of phase X”, “phases of epoch Y”).
- **ItemRef**: identifies an item that can be placed into a container (e.g. a task, a phase).
- **AnchorRef**: identifies an existing item in the same container used as a reference point.
- **Position**: a placement instruction evaluated relative to a container.

### The Position type

A Position MUST be one of:

- `prepend` (place as first item in container)
- `append` (place as last item in container)
- `before(anchor)`
- `after(anchor)`

Rules:

- Exactly one Position MUST be provided for insert/move operations.
- `prepend`/`append` are container-relative and require no **item** anchor, but they still require a container.
- `before/after` require that the anchor resolves **unambiguously** within the target container.

Additionally:

- A command MUST have a container, and it MUST be specified explicitly via `--in <container>`.

### CLI surface (normative)

Commands that insert or move list items SHOULD accept **one** of:

- `--prepend`
- `--append`
- `--before <anchor>`
- `--after <anchor>`

If multiple are provided, the command MUST fail with steering.

Commands that apply Position MUST accept a container selector: `--in <container>`.

#### Container selection grammar (normative)

When a command accepts `--in <container>`, the `<container>` argument MUST be a typed container reference.

Minimum required forms:

- `epoch:<ref>`
- `phase:<ref>`

Additionally, commands MUST accept:

- `active`

Where `<ref>` may be:

- A canonical ID reference (preferred for agents): `epoch@<ulid>` / `phase@<ulid>` (future)
- A scoped slug (human convenience)
- A legacy identifier during migration (e.g. `phase-74`)

Resolution rules:

- The container reference MUST resolve unambiguously.
- If it cannot be resolved, the command MUST fail with steering.

Design note:

- We intentionally avoid implicit defaults. Instead, commands should support `--in active`.
- Each command that supports `--in active` MUST define what “active” means for that command (e.g. “the active phase” vs “the active epoch”), and MUST surface that meaning in help text and/or steering.

Output requirement:

- If `--in active` is used, the command MUST include the resolved container (as a canonical reference) in machine output (`--format json`) and SHOULD echo it in human output.

Examples (illustrative):

- `exo plan add-phase --in active --prepend ...` ("active" meaning defined by command)
- `exo plan add-phase --in epoch:<ref> --after <phase-anchor> ...`
- `exo plan move-task <task> --in phase:<ref> --before <task-anchor>`
- `exo plan add-task --in phase:<ref> --append ...`
- `exo plan add-task --in phase:phase-75-coherence --append ...` (legacy container ID during migration)
- `exo plan add-task --in phase:reactivity-tests --append ...` (phase slug)

### Anchor resolution

Anchors MAY be specified as:

- Canonical ID form (preferred for agents): `type@<ulid>` (future) or equivalent stable reference.
- A scoped slug (human convenience): `migrate-tests`.
- A legacy identifier during migration (e.g. `phase-74`).

Resolution MUST follow these rules:

1. **Container-scoped first**: resolution is performed within the target container.
2. If exactly one match, succeed.
3. If zero matches, fail with steering.
4. If multiple matches, fail with steering and list candidates (canonical refs).

### Determinism and steering

When the tool must refuse an operation, it MUST provide deterministic steering:

- Explain why (missing anchor, ambiguous anchor, anchor not in container).
- Provide candidate canonical refs and/or suggest disambiguation.

This is a core agent ergonomics feature: it turns “LLM guessing” into a reliable feedback loop.

### Collisions: slugs within scope

This RFC assumes some containers/items may have optional human slugs.

When creating an item with a requested slug that collides within the **relevant scope**:

- The tool SHOULD fail with a collision error and steering.
- The steering MUST suggest: “pick a different slug” (and ideally present nearby/related existing slugs).

We intentionally prefer this over auto-suffixing because LLM-driven workflows can trivially retry and a human often wants a meaningfully distinct name.

### Compatibility and incremental adoption

We can adopt Position without immediately migrating identities:

- In the short term, anchors can resolve against current IDs (e.g. task IDs, `phase-74`), but operations must be container-scoped.
- As ULIDs become canonical (see companion RFC), anchors become stable and portable.

## Library support (normative)

Insert/move operations SHOULD be implemented via a shared library surface so commands stay consistent.

This RFC does not mandate a specific crate/package layout, but it does require that the shared layer encodes the rules above:

- Container-scoped anchor resolution
- Deterministic ambiguity errors + steering payload
- Collision handling policy (fail + retry guidance)
- A single authoritative implementation of `prepend/append/before/after`

Commands should delegate to this shared layer rather than re-implementing ad-hoc list manipulation.

## Non-goals

- Defining a universal cross-container “global order”.
- Replacing all bespoke commands immediately.
- Introducing new UI affordances beyond what is necessary to express Position.

## Drawbacks

- Adds a new abstraction (“Position”), which must be explained well.
- Requires care to keep resolution deterministic and non-surprising.

## Alternatives

- Keep bespoke per-command flags: leads to ongoing inconsistency.
- Index-based positions (`--index 3`): brittle under concurrent edits and reorders.

## Unresolved Questions

- What is the minimal canonical "type@id" representation we want to standardize first?

## Implementation Status (2026-02-24)

> Partial implementation exists in `tools/exo/src/context/sqlite_writer.rs`:
>
> | Feature                       | Status        | Notes                                           |
> | ----------------------------- | ------------- | ----------------------------------------------- |
> | `top` / `bottom`              | ✅ Complete   | Maps to `new_before(first)` / `new_after(last)` |
> | `before:<id>` / `after:<id>`  | ✅ Complete   | Relative positioning via named anchors          |
> | Numeric index (`0`, `1`, ...) | ⚠️ Deprecated | Emits warning, still functional                 |
> | `fractional_index` storage    | ✅ Complete   | `sort_key TEXT` column, V007 migration          |
> | Goal reorder                  | ✅ Complete   | `SqliteWriter::reorder_goal()` + CLI dispatch   |
> | Task reorder                  | ✅ Complete   | `SqliteWriter::reorder_task()` + CLI dispatch   |
>
> **Storage**: `sort_key TEXT` column on `phases_data`, `goals_data`, `tasks_data`.
> Keys are hex-encoded `FractionalIndex` values (lexicographically sortable).
>
> **Query**: `ORDER BY sort_key NULLS LAST, id`
>
> **Resolved questions**:
>
> - Numeric index positions are deprecated with a warning. Use `top`, `bottom`, `before:<id>`, or `after:<id>`.
> - Goal and task reorder migrated first. Phase reorder deferred (epochs don't have sort_key).

**Epoch:** SQLite as Source of Truth
