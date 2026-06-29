<!-- exo:10161 ulid:01kmzxbd018d594kkkt3g27nwb -->


# RFC 10161: The Plan Object Model

- **Status**: Stage 4 (Stable)
- **Created**: 2025-05-20
- **Implemented**: `packages/exosuit-core`
- **Supersedes**: RFC 0011 (Dynamic Planning)

## Summary

The Plan Object Model is the canonical representation of an Exosuit project's state. It replaces the previous Markdown-based parsing strategy with a robust, TOML-backed object graph. This model is the "Brain" of the agent, tracking Epochs, Phases, and Tasks.

## Motivation

RFC 0011 originally proposed parsing `plan-outline.md` to derive the project state. However, Markdown proved too fragile for bidirectional editing (Agent <-> User).

1.  **Ambiguity**: Markdown structure is loose; parsing it reliably requires complex heuristics.
2.  **Round-tripping**: Preserving user formatting while the agent updates status is difficult in Markdown.
3.  **Type Safety**: We need a strongly-typed guarantee of the project state for the agent to make decisions.

## Design

### The TOML Source of Truth

The project plan is stored in `docs/agent-context/plan.toml`. TOML was chosen for its readability and unambiguous structure.

```toml
[project]
name = "Exosuit"
vision = "..."

[[epochs]]
id = 1
name = "Foundation"

[[epochs.phases]]
id = 1
name = "Bootstrap"
status = "completed"
```

### The Object Graph

The `packages/exosuit-core/src/models` directory defines the TypeScript classes that wrap this data:

- **`Plan`**: The root aggregate.
- **`Epoch`**: A high-level milestone.
- **`Phase`**: A concrete unit of work (the "Hands").
- **`PhaseTask`**: An atomic unit of execution.

### Dynamic Planning Engine

The `PlanModifier` class (in `packages/exosuit-core`) provides transactional methods to mutate the plan:

- `addPhase()`
- `completeTask()`
- `updateStatus()`

These mutations are immediately serialized back to `plan.toml`, ensuring the file on disk is always consistent with the in-memory model.

## Implementation

- **Persistence**: `smol-toml` is used for parsing and stringifying, preserving comments and structure where possible.
- **Validation**: `zod` schemas ensure that the TOML file matches the expected shape before it is loaded into the Object Model.