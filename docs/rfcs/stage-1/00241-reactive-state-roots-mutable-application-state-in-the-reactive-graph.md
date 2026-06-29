<!-- exo:241 ulid:01kmzxey0p5yreeq0mj5agfqn3 -->

# RFC 241: Reactive State Roots: Mutable Application State in the Reactive Graph


# RFC 00241: Reactive State Roots: Mutable Application State in the Reactive Graph

## Summary

Introduce **State Roots** — mutable in-memory values that participate in the reactive graph as first-class roots alongside file roots and derived roots. State roots are named Cells (per the reactive algebra) with a `set()` operation that bumps the root's revision, triggering re-validation of any derived roots whose traces include a read from that state root.

A `ReactiveStateRegistry` provides the registry for state roots, parallel to `DerivedRootRegistry` for derived/computed roots and `RootMaterializerRegistry` for file-backed roots.

## Motivation

The reactive system currently supports two root types:

1. **File roots** — backed by disk, invalidated by file watchers via WASM `notify_file_change()`
2. **Derived roots** — computed on demand, validated via traces recorded by `ObservationService`

A third category is needed: **mutable application state** that lives in memory and should participate in the reactive graph. The motivating use case is UI selection state (e.g., "which phase is currently selected in the sidebar"), but the pattern applies broadly to any ephemeral state that derived roots should be able to depend on.

Today, the closest pattern is `DiagnosticsService`, which calls `invalidateRoots()` to signal that `derived:diagnostics.summary` should recompute. But the diagnostics state itself lives in VS Code's API (`vscode.languages.getDiagnostics()`), not in the reactive system. There is no exosuit-owned mutable state root.

Without state roots, any UI state that feeds into derived computations requires ad-hoc wiring — each new piece of state reinvents the pattern of "hold a value, call `invalidateRoots` when it changes, hope consumers listen for the right IDs." State roots formalize this into a single, type-safe pattern.

## Design

### Algebraic Foundation

State roots are **named Cells** in the reactive algebra (RFC 0026). The algebra already defines:

- **Cells** — mutable state with revision tracking
- **Formulas** — derived computations that read Cells and produce cached results
- **Traces** — recorded dependencies from Formula computation, validated against Cell revisions

State roots add no new algebraic concepts. They are the application-level instantiation of Cells that the algebra already supports.

### The `state:` Namespace

State root keys use the `state:` prefix, distinguishing them from file roots (bare paths) and derived roots (`derived:` prefix):

```
state:ui.selectedPhase     # Currently selected phase ID (or null for active phase)
state:ui.expandedNodes     # (future) Expanded tree nodes
state:ui.viewMode          # (future) Current view mode preference
```

### ReactiveStateRegistry API

```typescript
interface StateRootDefinition<T> {
  /** Initial value when the state root hasn't been explicitly set */
  defaultValue: T;
}

class ReactiveStateRegistry {
  /**
   * Register a state root with its type and default value.
   * Must be called before set(). Enforces that state roots
   * are declared, not ad-hoc.
   */
  register<T>(key: string, definition: StateRootDefinition<T>): void;

  /**
   * Set a new value for a state root.
   * Bumps the root's revision and fires invalidateRoots([key]).
   */
  set<T>(key: string, value: T): void;
}
```

The registry exposes only `register()` and `set()`. There is no `get()` on the registry — **state roots are read via `scope.read()`**, the same API used to read file roots and derived roots inside compute functions. This ensures every read records a dependency in the caller's trace automatically.

Key registration is mandatory — calling `set()` on an unregistered key throws. This catches typos and ensures state roots are declared alongside the code that owns them, not scattered through consumers.

### Reading State Roots

State roots are consumed exclusively through `scope.read("state:ui.selectedPhase")` inside derived root compute functions. The observation scope dispatches `state:` keys to the state registry, returns the value, and records the dependency — all in one operation, consistent with the algebra's axiom that "dependency recording is a side-effect of value consumption."

Reading a state root is the simplest possible read path — no observation frame is created, no trace validation, no cache lookup. It's just:

1. Look up the value in the registry's `Map`
2. Record `(cellId, revision)` in the caller's already-open trace
3. Return the value

This contrasts with reading a derived root, which must open its own observation scope, validate its cached trace, potentially recompute, and cache the result. State roots have no computation to memoize — they're bare cells.

A bare cell is never a terminal output. State roots always feed into derived computations, which in turn feed into UI outputs. This means there is no need for top-level `get()` access to state root values.

### Integration with the Reactive Graph

State roots participate in the reactive graph through the existing trace-based validation mechanism:

1. **During derived root computation**: `scope.read("state:ui.selectedPhase")` returns the value and records the state root as a dependency in the compute function's trace.
2. **On mutation**: `stateRegistry.set("state:ui.selectedPhase", newId)` bumps the state root's revision and fires `invalidateRoots(["state:ui.selectedPhase"])`.
3. **UI consumer response**: A UI consumer subscribed to `onDidInvalidateRoots` hears the state root changed, re-validates its derived root (via the memoization layer), and reconciles the output.

### UI Consumer Pattern

UI consumers (tree view providers, status bar services) sit at the terminal end of the reactive graph. They need three things:

1. **Memoized computation** — the derived root cache validates traces and recomputes only when dependencies are stale (`validateTrace()` is cheap: revision comparisons, no I/O).
2. **Invalidation subscription** — `onDidInvalidateRoots` tells the consumer "a leaf root you care about changed, re-pull." Only leaf roots (file roots, state roots) appear in invalidation events.
3. **Output reconciliation** — reflecting the new derived value into the UI. Currently this means rebuilding tree items and calling `refresh()`. Future evolution: Glimmer-style atomic updates where cache invalidation manages its own output updates.

Derived roots do not need their own entries in the invalidation system. When a consumer re-validates a derived root, `validateTrace()` recursively validates the entire dependency chain — including nested derived roots — via cheap revision comparison. If nothing actually changed, validation is a no-op.

This means derived root A can depend on derived root B which depends on state root S, and the system works correctly without B having its own invalidation entry. When S changes, a consumer of A hears S changed, re-validates A, which re-validates B, which detects S is stale, recomputes B, then A sees B changed and recomputes. All driven by a single leaf-root invalidation event.

### Scoping

State roots are **window-scoped** — the `ReactiveStateRegistry` is a singleton in the extension host process. Since each VS Code window runs its own extension host, this is automatically one registry per window.

Multi-root workspaces with multiple exosuit projects are not supported. The working assumption is one exosuit project per VS Code window. If this changes, _all_ reactive infrastructure would need scoping, not just state roots.

### Lifecycle

State roots are ephemeral — they do not survive extension host restarts. On activation, all state roots start at their registered `defaultValue`.

## Drawbacks

- Adds a third registry alongside `DerivedRootRegistry` and `RootMaterializerRegistry`. However, each serves a distinct purpose (mutable state, computed state, file-backed state) and the implementation is small (~50 lines).
- Key registration ceremony adds boilerplate compared to a bare `Map`. This is intentional — state roots are infrastructure, not ad-hoc, and the ceremony catches bugs.

## Alternatives

- **No registry; services hold state and call `invalidateRoots` manually.** This is the current pattern for diagnostics. It works but doesn't compose — each new piece of state reinvents the wiring, and there's no way for derived roots to declare state dependencies via traces.
- **Store state in files (e.g., a `.exo/ui-state.json`).** This would make state roots file roots, leveraging existing infrastructure. But it's semantically wrong — ephemeral UI state shouldn't be persisted to disk — and adds unnecessary I/O.
- **Extend `DerivedRootRegistry` to support mutable roots.** Conflates two different concepts: derived roots are computed and have no setter; state roots are explicitly set and have no compute function.

## Future Work

### Memento Persistence

Selected state roots could opt into persistence via VS Code's `workspaceState` memento, surviving window restarts:

```typescript
interface StateRootDefinition<T> {
  defaultValue: T;
  persist?: "workspace" | "global";
}
```

When `persist` is set, `set()` writes to memento, and `register()` reads the initial value from memento (falling back to `defaultValue`). This should be implemented based on usage experience — when users lose important UI state on restart frequently enough to create friction.

## References

- **RFC 0026** (Stage 4) — Validation-Based Reactive Architecture: "All mutable state is modeled as a Cell"
- **RFC 00188** (Stage 3) — Derived Roots & Reactive Caches: Core derived root architecture
- **RFC 0119** (Stage 4) — Reactive File System: File root invalidation patterns
- **RFC 00225** (Stage 1) — Problems Pane Integration: Introduced `invalidateRoots()` for non-file state
- **RFC 00238** (Stage 1) — Pipeline-Aware Self-Model: Shared perception channels via reactive roots
- **RFC 0016** (Stage 1) — Sidebar Navigation: Describes phase selection as UI state
