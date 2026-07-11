<!-- exo:227 ulid:01kmzxbczwp02v0tdje3gy6ewt -->

# RFC 227: Computed Phase Details: Unified Derived Root

- **Status**: Withdrawn
- **Stage**: 2
- **Reason**:

# RFC 00227: Computed Phase Details: Unified Derived Root

- **Superseded by**: RFC 10173


## Summary

Introduce a `derived:phase.details` computed root that provides a unified, trace-validated phase details structure. This single computation serves all consumers: the Phase Details sidebar, CLI commands (`exo phase status`), and LM tools (`exo-phase`—via the MachineChannel Hybrid Server).

## Motivation

### Current State

Phase details are computed in multiple places with different logic:

1. **Phase Details sidebar** (`TreeDataService.ts`): Reads `plan.toml` and `implementation-plan.toml` directly, parses them, joins goals with tasks, handles surgical strikes, etc.

2. **CLI** (`phase_cmd.rs`): Reads the same files, computes similar structure, renders to human/JSON output.

3. **LM tools** (`exo-phase`): Calls CLI, gets output.

This causes:

- **Duplicated logic** that can drift between Rust and TypeScript
- **No caching** — each consumer recomputes from scratch
- **Inconsistency risk** — different rendering logic, different edge case handling

### The Derived Root Model

Per RFC 00188, derived roots are "computed values watched like physical files":

- They compose data from source roots (disk-backed files)
- They're trace-validated (recompute only when dependencies change)
- One-off reads get the current value without managing invalidation
- Watchers get notified when the computed value changes

Existing derived roots (`inbox.summary`, `ideas.summary`) demonstrate the pattern but are limited to lightweight summaries.

## Design

### The `derived:phase.details` Root

A new derived root that computes the full phase details structure:

```typescript
interface PhaseDetails {
  // Phase metadata
  phase: {
    id: string;
    title: string;
    epochId: string;
    epochTitle: string;
    rfcs: string[];
  } | null;

  // Progress summary
  progress: {
    mode: ProgressMode; // "planning" | "executing" | "between-phases" | etc.
    goalsCompleted: number;
    goalsTotal: number;
    tasksCompleted: number;
    tasksTotal: number;
  };

  // Hierarchical goal → task structure
  goals: Array<{
    id: string;
    title: string;
    status: "pending" | "in-progress" | "completed" | "skipped";
    kind?: "strike";
    startedAt?: string;
    completionLog?: string[];
    tasks: Array<{
      id: string;
      title: string;
      status: "pending" | "in-progress" | "completed" | "skipped";
      completionLog?: string[];
      derivedStatus?: {
        status: string;
        reason: string;
      };
    }>;
  }>;

  // Active surgical strike (if any)
  activeStrike?: {
    goalId: string;
    title: string;
    startedAt: string;
  };

  // Context health
  contextCheck: {
    implementationPlan: "found" | "missing";
    gitDirty: boolean;
  };

  // Verification requirements
  verification: {
    automated: string[];
    manual: string[];
  };
}
```

### Hybrid MachineChannel Access

To allow the CLI and LM tools to access this extension-hosted value without duplicating logic, we utilize the **Hybrid MachineChannel** architecture:

1.  **Interception**: The `MachineChannelServer` in the extension listens for requests.
2.  **Routing**: It checks if a request targets the `derived:*` namespace (e.g., `derived:phase.details`).
3.  **Service Access**: If the root is registered in the `DerivedRootRegistry`, the server bypasses the CLI subprocess and resolves the request directly from the extension's in-memory cache.

This treats `derived:phase.details` as a **Shared Application Service**. The CLI "accesses" the service remotely, but the computation happens once, centrally, within the extension host.

### Dependencies

The derived root depends on:

- `root:plan.toml` — goal definitions, phase status
- `root:implementation-plan.toml` — tasks nested under goals
- Git status (optional, may be separate)

When either source file changes, the derived root revalidates via trace comparison and recomputes if needed.

### Consumers

**Phase Details sidebar:**

```typescript
// Before: Parse files, compute structure
const parsed = parseImplementationPlanForPhaseDetails(implPlan, goals);

// After: Read derived root
const details = await observationService.read("derived:phase.details");
```

**CLI (`exo phase status`):**

```rust
// Rust CLI can either:
// 1. Call the same computation logic (shared with derived root)
// 2. Read from the derived root via IPC (if extension is running)
// 3. Compute independently (current behavior, acceptable for CLI)
```

**LM tools:**
Already call CLI, so they get the benefit indirectly. Could also read derived root directly if available.

### Not Materialized

The derived root is **not** written to disk as a file. It's a computed value that:

- Lives in memory (cached)
- Is invalidated when sources change
- Is recomputed on-demand

Materializing computed values to disk is an anti-pattern because:

- Creates another file to keep in sync
- Obscures the derivation relationship
- Adds write I/O for something that can be recomputed cheaply

### Relationship to CLI

The CLI continues to work without the extension:

- It computes phase details from source files directly
- No dependency on derived roots (which require the extension's reactivity system)

When the extension is running:

- The derived root caches the computation
- Multiple consumers share the cached result
- File watchers trigger revalidation automatically

## Migration

1. **Phase 1**: Implement `derived:phase.details` in the extension
2. **Phase 2**: Migrate Phase Details sidebar to consume the derived root
3. **Phase 3**: Ensure CLI computation logic matches derived root logic (shared types, shared test fixtures)
4. **Phase 4**: Consider exposing derived root to LM tools via machine channel

## Success Criteria

- [x] `derived:phase.details` root exists and is trace-validated
- [x] Phase Details sidebar reads from derived root (not raw files)
- [x] Changing `plan.toml` or `implementation-plan.toml` triggers sidebar update
- [x] CLI and derived root produce equivalent structures (tested)

## Related RFCs

- **RFC 00188**: Derived Roots & Reactive Caches
- **RFC 0026**: Validation-based Reactivity
- **RFC 0118**: Reactivity vNext/Collections
- **RFC 00177**: Goals and Tasks: Unified Work Item Model

## Resolved Questions

### 1. Should the derived root include steering suggestions?

**Decision: No.** Steering is a separate derived root.

**Rationale:**

- Steering depends on a much wider surface than phase details: git status, inbox intents, snapshots, world state
- RFC 00188 already lists "Steering Cache" as a separate derived root use case
- The Phase Details sidebar doesn't display steering—it only needs goals/tasks/progress
- Including steering would cause frequent invalidations due to the wide dependency surface

**Implementation:** If steering caching is needed, add a separate `derived:steering` or `derived:world.status` root per RFC 00188's steering cache pattern.

### 2. How does this interact with "between phases" navigation state?

**Decision:** Include `progress.mode` in `derived:phase.details` with `phase: null` when between phases.

**Rationale:**

- The schema already handles this (RFC shows `phase: {...} | null`)
- The sidebar can then use _one_ derived root for both active and between-state rendering
- `progress.mode` values include `"planning"`, `"executing"`, `"between-phases"`, etc.
- This minimizes CLI churn and aligns with the "unified derived root" goal

**Implementation:** The `progress` field is mandatory and includes `mode`. When no active phase exists, `phase` is `null` but `progress.mode` still conveys the navigation state (e.g., `"between-phases"`, `"no-active-epoch"`).

### 3. Should there be `phase.summary` vs `phase.details`?

**Decision:** Use existing `derived:status.summary` for lightweight consumers; add `derived:phase.details` for full hierarchical data.

**Rationale:**

- Summary roots already exist: `derived:status.summary` includes active phase + goal/task counts
- Creating a new `derived:phase.summary` would duplicate `derived:status.summary`
- Full details are only needed by specific consumers: Phase Details sidebar, CLI, LM tools
- This avoids duplication and leverages existing infrastructure

**Implementation:** No new summary root needed. Consumers needing lightweight data use `derived:status.summary`; consumers needing full goal→task hierarchy use `derived:phase.details`.
