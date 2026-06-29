<!-- exo:223 ulid:01kmzxey1jxwnzk21h9ak25jnj -->

# RFC 223: CLI Namespace Consolidation


# RFC 00223: CLI Namespace Consolidation

## Summary

Deprecate the `exo impl` namespace by migrating its functionality into `exo goal` and `exo task`. Refactor `exo strike` to be a thin wrapper over `exo goal`. This aligns the CLI surface with the conceptual model established in RFC 00177 (Goals and Tasks) and RFC 00179 (Surgical Strikes as Goals).

## Motivation

### Current Problem

The CLI has overlapping namespaces with unclear boundaries:

| Command              | What it does                               | Conceptual confusion             |
| -------------------- | ------------------------------------------ | -------------------------------- |
| `impl add-step`      | Creates `[[plan.goals]]` in impl-plan.toml | "step" = goal with rich metadata |
| `impl add-task`      | Creates task under a step                  | Duplicates `task add --goal`     |
| `impl update-status` | Updates step status                        | "step" vs "goal" terminology     |
| `strike start`       | Writes directly to plan.toml               | Bypasses `goal add` entirely     |

This causes:

1. **Terminology drift**: "step" in CLI, "goal" in schema, same thing
2. **Duplicate code paths**: `impl add-task` vs `task add --goal`
3. **Inconsistent validation**: `task add` validates goal exists in plan.toml; `impl add-task` doesn't
4. **Strike bypass bug**: `strike start` didn't add goal to impl-plan.toml (fixed, but symptom of deeper issue)

### Root Cause

The `impl` namespace predates RFC 00177. It was designed for a "steps" model that has since been superseded by "goals and tasks." The namespace was never migrated.

### Evidence

From code analysis:

- `impl add-step` writes to `[[plan.goals]]` with fields: `name`, `type`, `details`, `files`, `tests`
- `goal add` writes to `epochs[].phases[].goals[]` with fields: `id`, `label`, `status`, `kind`
- These are the **same conceptual entity** split across files with different schemas

## Design

### Namespace Hierarchy (Target State)

```
exo plan      → Structural: epochs, phases (unchanged)
exo epoch     → Epoch lifecycle (unchanged)
exo phase     → Phase lifecycle (unchanged)
exo goal      → Intent + metadata (absorbs impl step features)
exo task      → Execution + logging (absorbs impl task features)
exo strike    → Thin wrapper over goal (refactored)
exo impl      → DEPRECATED (removed after migration)
```

### Migration Map

#### Goal Namespace (absorbs step-level impl commands)

| Old Command          | New Command                                 | Notes                            |
| -------------------- | ------------------------------------------- | -------------------------------- |
| `impl add-step`      | `goal add --type --details --files --tests` | Extended flags                   |
| `impl remove-step`   | `goal remove`                               | New command                      |
| `impl reorder-step`  | `goal reorder`                              | Already exists, verify parity    |
| `impl update-status` | `goal update --status`                      | Extend existing `goal update`    |
| `impl satisfy`       | `goal satisfy`                              | New command                      |
| `impl add-feedback`  | `goal feedback`                             | New command                      |
| `impl clear-steps`   | `goal clear`                                | New command (or remove via loop) |
| `impl list`          | `goal list --details`                       | Extended flag for rich output    |
| `impl show`          | `goal show`                                 | New command                      |
| `impl status`        | `phase status`                              | Already shows goal status        |

#### Task Namespace (absorbs task-level impl commands)

| Old Command                  | New Command            | Notes           |
| ---------------------------- | ---------------------- | --------------- |
| `impl add-task`              | `task add --goal`      | Already exists  |
| `impl update-task-status`    | `task update --status` | Extend existing |
| `impl add-task-log`          | `task log`             | New command     |
| `impl add-task-verification` | `task verify`          | New command     |

#### Strike Namespace (first-class commands)

Strike commands are **first-class**, not wrappers. They create goals with `kind = "strike"` but enforce strike-specific semantics (singleton constraint, immediate activation, upgrade gate bypass). See RFC 00179.

| Command                      | Behavior                                            |
| ---------------------------- | --------------------------------------------------- |
| `strike start --name --goal` | Creates goal with `kind = "strike"` in active phase |
| `strike finish`              | Sets strike goal to `status = "completed"`          |
| `strike abort`               | Sets strike goal to `status = "aborted"`            |

### Schema Unification

> **Note**: This section has been updated to reflect the SQLite-first architecture defined in RFC 10165 and RFC 10176.

With SQLite as the storage backend, the dual-file complexity disappears. Goals live in a single `goals` table with foreign keys to phases:

```sql
CREATE TABLE goals (
    id TEXT PRIMARY KEY,
    phase_id TEXT NOT NULL REFERENCES phases(id),
    label TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending | in-progress | completed | abandoned
    -- Rich metadata (formerly in impl-plan.toml)
    rfc TEXT,           -- RFC link
    completion_log TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

> **Note**: See RFC 10176 for the canonical schema. The `kind` field was removed from goals (it applies to phases, not goals).

The `goal add` command writes a single row. No cross-file coordination, no join keys, no schema drift risk. The CLI namespace consolidation becomes a pure command-surface concern—the underlying storage is already unified.

### Deprecation Strategy

1. **Phase 1**: Add new commands to `goal`/`task` with feature parity
2. **Phase 2**: Add deprecation warnings to `impl` commands pointing to new equivalents
3. **Phase 3**: Remove `impl` namespace after one epoch

### Backward Compatibility

- `impl` commands continue to work during deprecation period
- Deprecation warnings include exact replacement command
- No schema changes required (both files already exist)

## Implementation

### Phase 1: Extend goal/task commands

1. Add flags to `goal add`: `--type`, `--details`, `--files`, `--tests`
2. Add `goal remove`, `goal show`, `goal satisfy`, `goal feedback`, `goal clear`
3. Add `task log`, `task verify`
4. Extend `goal update` and `task update` with `--status` flag

### Phase 2: Refactor strike

1. `strike start` → call `GoalAdd` with `kind = "strike"`
2. `strike finish` → call `GoalComplete` with strike-specific messaging
3. `strike abort` → call `GoalUpdate` with `status = "aborted"`

### Phase 3: Deprecate impl

1. Add deprecation warnings to all `impl` commands
2. Update documentation and LM tool descriptions
3. Remove after one epoch (or when no usage detected)

## Alternatives Considered

### Keep impl as "power user" namespace

Rejected: Creates permanent confusion. "Which command do I use?" should have one answer.

### Rename impl to something else

Rejected: Doesn't solve the duplication problem, just moves it.

### Merge everything into plan namespace

Rejected: `plan` is structural (epochs, phases). Goals and tasks are work items, conceptually different.

## Open Questions

1. Should `goal add` require `--type` or default to "feat"?
2. Should rich metadata (files, tests) be optional or encouraged?
3. How do we handle existing `impl` usage in LM tool prompts?

## Investigation Notes (2026-02-03)

A detailed codebase audit revealed significant hidden complexity that affects the implementation approach.

### Current State Audit

| Namespace | Status          | Key Findings                                                                                   |
| --------- | --------------- | ---------------------------------------------------------------------------------------------- |
| `impl`    | Fully featured  | Writes rich metadata to implementation-plan.toml via dedicated helpers                         |
| `goal`    | Thin            | Only basic CRUD (add, list, reorder, complete, update label). No metadata support              |
| `task`    | Partial         | Supports add, list, complete, remove, reorder, update (title only). No status/log/verification |
| `strike`  | Direct mutation | Edits plan.toml directly, separately adds strike goal to implementation-plan                   |

### Dependency Analysis

**Hard dependencies on `impl` commands:**

1. **LM Tools**: Tool factory and command-spec explicitly register `impl` operations
2. **Steering/Map**: State machine recommends `impl` commands in steering output
3. **VS Code Notebooks**: Comment feature calls `impl add-feedback`
4. **Tests**: Multiple test files assert `impl` semantics for task logs/verifications

### Hidden Complexity

> **Note**: Issues 1 and 3 are resolved by the SQLite migration (RFC 10165). Issue 2 remains relevant.

1. ~~**Schema Drift Risk**: `impl` writes to implementation-plan.toml; `goal` writes to plan.toml. Without atomic cross-file writes, state can diverge.~~ **Resolved**: SQLite provides a single source of truth with transactional writes.

2. **Identity Mismatch**: `goal add` uses title as identifier; `impl add-step` uses explicit id. This affects reorder, remove, and update operations. **Still relevant**: The CLI should standardize on explicit IDs (auto-generated from title if omitted).

3. ~~**Validation Asymmetry**: `task add --goal` validates goal exists in plan.toml; `impl add-task` does not. Migration may introduce new failures.~~ **Resolved**: SQLite foreign keys enforce referential integrity at the storage layer.

### Revised Implementation Approach

The original three-phase plan underestimates complexity. Recommended revision:

| Phase                    | Description                                                                                                                      | Complexity | Prerequisite |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ---------- | ------------ |
| **0: Alignment Design**  | Define canonical id mapping between plan goals and impl-plan entries. Decide flag semantics for `goal add`.                      | Small      | None         |
| **1: Parity Build-out**  | Extend `goal`/`task` to write metadata to impl-plan. Add missing commands (remove, show, satisfy, feedback, clear, log, verify). | **Large**  | Phase 0      |
| **2: Strike Wrapper**    | Refactor `strike` to call `goal add --kind strike`. Requires `goal add` to support `--kind` and `--description`.                 | Medium     | Phase 1      |
| **3: Tooling Migration** | Update LM tool registration, command-spec, notebook features, and steering to use new commands. Add deprecation warnings.        | Medium     | Phase 2      |
| **4: Removal**           | Delete `impl` namespace after one epoch. Update tests.                                                                           | Small      | Phase 3      |

### Recommendation

**Proceed with implementation** after the SQLite migration (RFC 10165) is complete. The data model unification is now handled at the storage layer (see RFC 10176: Project State Model), which eliminates the root cause of complexity identified in this RFC.

The remaining work is a pure CLI surface concern:

1. Add missing commands to `goal`/`task` namespaces
2. Standardize on explicit IDs (auto-generated from title if omitted)
3. Deprecate and remove `impl` namespace

## References

- RFC 10176: Project State Model (data model foundation)
- RFC 10165: SQLite Migration (storage unification)
- RFC 00177: Goals and Tasks: Unified Work Item Model
- RFC 00179: Surgical Strikes as Goals
- RFC 00200: CLI Argument Consistency (related: positional vs named args)
