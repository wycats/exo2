<!-- exo:73 ulid:01kg5kp2ejaym82bt2zsw9b84f -->

# RFC 73: Semantic Merge Driver for Structured Context Files

- **Superseded by**: RFC 72

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0073: Semantic Merge Driver for Structured Context Files

## Motivation

We frequently hit merge conflicts in “structured” workspace files that are:

- mechanically edited by `exo` (or agents),
- often updated in parallel across branches,
- semantically mergeable (e.g. union-by-`id`),
- but *textually* conflict-prone.

Examples:

- `docs/agent-context/ideas.toml`: independent additions should merge cleanly.
- `docs/agent-context/plan.toml`: reordering and parallel edits should not produce noisy conflicts.
- `docs/agent-context/current/*`: these are projections/snapshots; merging them is usually counterproductive.

## Goal

Define and implement a repo-local, installable merge driver that:

1. Provides *semantic* 3-way merges for the Exosuit structured files (initially TOML).
2. Falls back to a normal textual merge (with conflict markers) when semantics are ambiguous.
3. Encourages deterministic conflict resolution without introducing new workflow complexity.

## Non-Goals

- A universal TOML merge for arbitrary schemas.
- Solving all Git conflicts (only the high-value structured files).
- Hiding real conflicts; if two branches disagree on the same scalar field, we should surface it.

## Proposed Design

### 1) A Git merge driver backed by `exo`

Add a CLI entrypoint suitable for use as a Git merge driver:

```bash
exo merge-driver toml <base> <current> <other> [--path <path>]
```

Where:

- `<current>` is the file that Git expects to be updated in-place.
- Exit code `0` means “merged cleanly”.
- Exit code `1` means “merge contains conflicts” (but driver may still write a conflict-marked result).

### 2) Merge semantics (TOML)

We define a conservative 3-way merge on TOML values:

- If `current == other`: take it.
- If `current == base`: take `other`.
- If `other == base`: take `current`.
- Tables: recursively merge keys.
- Arrays-of-tables with `id`: merge by `id`.
    - For matching `id`, recursively merge table fields.
    - Pure additions of distinct `id`s should not conflict.
- Anything else: treat as a conflict and fall back to textual merge.

### 3) Special-case: `docs/agent-context/current/*`

For snapshot/projection files under `docs/agent-context/current/`, default behavior should be “ours wins” (keep `<current>` as-is).

This matches our existing posture:

- These files are derived artifacts.
- They can be regenerated and should not block merges.

## Repository Integration

### `.gitattributes`

We will mark specific files to use the driver:

- `docs/agent-context/ideas.toml`
- `docs/agent-context/plan.toml`
- `docs/agent-context/current/*.toml`

### Installation

Git requires a one-time local config entry to map a driver name to a command.
We will provide a helper script (repo-local) that runs:

```bash
git config merge.exo-toml.name "Exosuit TOML semantic merge"
git config merge.exo-toml.driver "exo merge-driver toml %O %A %B %P"
```

## Rollout Plan

1. Land the RFC + minimal implementation.
2. Update `.gitattributes` with a narrow initial file set.
3. Iterate on merge semantics in response to real conflicts.

## Open Questions

- Should we also support a “union” mode for some arrays without stable `id` keys?
- Should the driver emit structured diagnostics (for agents) when conflicts remain?


