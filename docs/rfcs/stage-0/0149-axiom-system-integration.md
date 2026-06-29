<!-- exo:149 ulid:01kg5kp2jdnga9q77kyrfkc9ck -->

# RFC 149: Axiom System Integration


# RFC 0149: Axiom System Integration

## Problem Statement

Axioms exist as static files (`docs/agent-context/axioms.workflow.toml`, `axioms.system.toml`, `docs/design/axioms.design.toml`) but are **not integrated** into the system's decision-making or discovery processes:

- **Not consulted during steering**: Steering computes `ProgressMode` and suggests actions without reference to axioms
- **Not linked to phases/epochs**: Unlike RFCs (`rfcs = ["10108"]`), phases have no `axioms = [...]` field
- **No discovery mechanism**: Nothing prompts axiom creation during workflow friction
- **No validation**: `exo verify` doesn't check axiom coherence
- **Sparse population**: Only 18 workflow axioms exist because nothing drives their creation

## Current State

### Axiom Files

| File                   | Location              | Content                           |
| ---------------------- | --------------------- | --------------------------------- |
| `axioms.workflow.toml` | `docs/agent-context/` | 18 axioms (core principles)       |
| `axioms.system.toml`   | `docs/agent-context/` | Empty                             |
| `axioms.design.toml`   | `docs/design/`        | 2 axioms (duplicates of workflow) |
| `axioms.legacy.toml`   | `docs/design/`        | Deprecated format                 |

### Usage

- `exo axiom add/list/remove` CLI exists
- Axioms included in AI context dumps (`exo ai context`)
- Referenced in prompts but not dynamically loaded

## Proposed Integration Points

### 1. Axiom-Phase Links

```toml
[[epochs.phases]]
id = "orphan-cleanup"
axioms = ["context-is-king", "phased-execution"]
```

Phases declare which axioms they embody. Steering surfaces relevant axioms when starting a phase.

### 2. Axiom Discovery via Friction

When agents encounter repeated friction patterns, prompt:

> "This sounds like it could become an axiom. Capture it?"

Similar to idea capture from conversations.

### 3. Axiom Validation in Verification

`exo verify` checks:

- Axioms with no linked phases (orphaned principles)
- Phases violating stated axioms (coherence check)

### 4. Axiom-Aware Steering

```json
{
  "next_actions": [...],
  "relevant_axioms": [
    {"id": "context-is-king", "guidance": "Read context before starting phase"}
  ]
}
```

### 5. Axiom Lifecycle (like RFC stages)

- **Draft**: Proposed principle, not yet validated
- **Active**: Validated through use, linked to phases
- **Deprecated**: Superseded by newer understanding

## Consolidation vs. Projection

### Current Split

Three scopes exist: `workflow`, `system`, `design`. The split was intended to separate concerns but:

- `system` is empty
- `design` duplicates `workflow`
- The distinction adds complexity without clear value

### Recommendation

**Consolidate to single file** (`axioms.toml`) with **projection for views**:

- Store all axioms in one canonical file
- Use tags/categories for logical grouping
- Generate scoped views via projection (like how phases project from plan.toml)
- Agents never read axiom files directly anyway—they go through tools

```toml
[[axioms]]
id = "context-is-king"
principle = "Context is King"
category = "workflow"  # or "system", "design"
tags = ["core", "philosophy"]
linked_phases = ["phase-1", "orphan-cleanup"]
```

## Implementation Research (2026-01-31)

### Current Axiom Structure

The `Axiom` struct in `tools/exo/src/axiom.rs`:

```rust
pub struct Axiom {
    pub id: String,
    pub principle: String,
    pub rationale: Option<String>,
    pub implications: Vec<String>,
    pub notes: Option<String>,
    pub tags: Vec<String>,
}
```

**Workflow Axioms** (7 defined):

1. Context is King
2. Phased Execution
3. Living Documentation (Laws vs. Code)
4. User in the Loop
5. Inverted Source of Truth (Tooling Independence)
6. Evolutionary Context
7. Testing Philosophy

### Integration Points Identified

| Area              | Current State                                           | What's Missing            |
| ----------------- | ------------------------------------------------------- | ------------------------- |
| **Idea System**   | `Idea` struct has: id, title, description, status, tags | No axiom alignment fields |
| **RFC Promotion** | `rfc::promote()` only checks stage ≤ 4                  | No axiom validation       |
| **Steering**      | `derive_world_steering()` → `SteeringBlock`             | No axiom references       |
| **LM Tools**      | `exo-context` dumps axiom files                         | No alignment summaries    |

### Key Files to Modify

| File                        | Changes Needed                                        |
| --------------------------- | ----------------------------------------------------- |
| `tools/exo/src/axiom.rs`    | Add `evaluate_alignment(text) -> Vec<AxiomAlignment>` |
| `tools/exo/src/idea.rs`     | Add axiom alignment to list output                    |
| `tools/exo/src/rfc.rs`      | Add axiom check in `promote()` for Stage 1→2          |
| `tools/exo/src/steering.rs` | Include axiom refs in `SuggestedAction.rationale`     |

### Proposed New Types

```rust
pub struct AxiomAlignment {
    pub axiom_id: String,
    pub principle: String,
    pub relevance: AxiomRelevance,
    pub matched_tags: Vec<String>,
}

pub enum AxiomRelevance {
    Supports,    // Aligns with axiom
    Conflicts,   // May violate axiom
    Neutral,     // No clear relationship
}
```

### Estimated Scope

- **~300 lines of Rust** across 5 files
- **Medium complexity** - mostly wiring, no new architecture
- **Not urgent** - axioms work today, just not integrated

## Migration Path

1. **Consolidate files**: Merge `axioms.*.toml` → `axioms.toml`
2. **Update CLI**: `exo axiom --scope` becomes `exo axiom --category`
3. **Add phase links**: Extend plan.toml schema for `axioms = [...]`
4. **Wire steering**: Include relevant axioms in steering output
5. **Add discovery**: Friction → axiom capture workflow

## Open Questions

1. Should axiom lifecycle mirror RFC stages (0-4)?
2. How should axiom violations be surfaced? Warning vs. blocker?
3. Should axioms be versioned (like schema versions)?
4. **Tag-based matching vs. semantic matching?** Simple tag intersection is fast but limited; semantic matching requires more infrastructure.

## Related

- RFC 0106: Staged RFC Process (calls for deleting `docs/design/`)
- RFC 0107: Coherent Workflow Model (steering integration)
- Modes/Personas: Similar integration problem (see RFC 0150)

## Status

**Deferred**: This RFC captures important work but is not blocking current priorities. The Wiring Epoch (RFC 0107) has been withdrawn; this work should be scheduled when axiom integration becomes a friction point.

