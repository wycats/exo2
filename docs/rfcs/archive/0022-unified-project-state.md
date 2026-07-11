<!-- exo:22 ulid:01kg5m2y5bxav6v3mbj69txrbs -->

# RFC 22: Unified Project State

- **Stage**: 4
- **Reason**: Superseded by RFC 10176 as the current SQLite-backed project-state model.

# RFC 0022: Unified Project State

- **Status**: Archived (superseded; formerly Stage 4 Stable)
- **Created**: 2025-05-20
- **Implemented**: `packages/exosuit-core`
- **Superseded by**: RFC 10176

## Summary

The Unified Project State architecture consolidates the various fragments of the Exosuit context (Plan, Axioms, Decisions, Tool Presentations) into a single, coherent service API. This service, `ContextService`, acts as the gateway for all read and write operations, ensuring consistency and type safety.

## Motivation

Previously, the agent and UI had to manually parse individual TOML or Markdown files to understand the project state. This led to:

1.  **Inconsistency**: Different parts of the system might parse files differently.
2.  **Coupling**: Code was tightly coupled to the file system structure.
3.  **Race Conditions**: Concurrent writes to the same file were difficult to manage.

## Design

### The Context Service

The `ContextService` class (in `packages/exosuit-core/src/ContextService.ts`) is the singleton entry point.

```typescript
class ContextService {
  constructor(rootDir: string);

  // Plan Access
  getPlan(): Plan;
  updatePlan(plan: Plan): void;

  // Axioms
  getAxioms(): Axiom[];
  addAxiom(axiom: Axiom): void;

  // Decisions
  getDecisions(): Decision[];
  addDecision(decision: Decision): void;
}
```

### Atomic Persistence

The service handles the details of serialization (via `smol-toml`) and file I/O. It ensures that:

- Files are read with strict schema validation (Zod).
- Writes are atomic (conceptually, though currently synchronous FS calls).
- The in-memory model always reflects the latest state on disk.

## Implementation

- **Models**: Defined in `packages/exosuit-core/src/models/`.
- **Validation**: Zod schemas for all TOML files.
- **Usage**: Used by `exosuit-vscode` to populate views and by `exo` CLI (future) for headless operations.
