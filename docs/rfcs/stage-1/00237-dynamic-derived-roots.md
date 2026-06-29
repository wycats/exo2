<!-- exo:237 ulid:01kmzxey1asr756tm61vwvsvh4 -->

# RFC 237: Dynamic Derived Roots (Reactive Families)


# RFC 00237: Dynamic Derived Roots (Reactive Families)

## Summary

Extend the Derived Root system to support **Dynamic, Parameterized Roots** (e.g. `derived:task:123`) using a **Pull-Based, Validation-First** reactivity model. The lifecycle of these roots is organically managed via **Bridge Pinning**—binding the root's existence to the UI components that consume it.

## Motivation

### The "Kitchen Sink" Problem
Currently, derived roots are static singletons (e.g., `derived:phase.details`). To show details for a specific item (like *one* task or *one* RFC), developers must either:
1.  Compute a massive "kitchen sink" object containing *all* items (O(N) cost).
2.  Perform one-off non-reactive reads (losing live updates).

### The Dynamic need
We need to subscribe to `derived:task:123` and receive updates *only* when that specific task changes, without computing the state of the other 999 tasks.

### Identified Cleanup Targets (Migration Candidates)

The following areas were identified during the "Splash Damage" review as candidates for immediate cleanup once this RFC is active:

1.  **Monolithic Roots**: `src/services/derivedRoots.ts` (Specifically `derived:phase.details` and `derived:rfc.index` parsing everything O(N)).
2.  **Manual Push Services**: `src/services/StatusService.ts` (Manual watchers, manual invalidation).
3.  **Source-Coupled UI**: `EpochContextProvider` & `IdeasTreeProvider` (Listening to file paths instead of derived signals).
4.  **Ad-Hoc Sync**: `RichEditorProvider.ts` (Manual `postMessage` loops).

Implementing this RFC unlocks the ability to clean up these patterns.

## Terminology (The Starbeam Model)

We adopt the **Pull-Based** mental model from Starbeam/Glimmer, integrated with Exosuit's Digest-based validation.

-   **Cell**: A mutable source unit (e.g., a file on disk).
-   **Formula**: A pure function producing a value from Cells.
-   **Trace**: A record of `{ CellID, Digest }` pairs accessed during a Formula's last run.
-   **Validation**: The act of checking if the current Digest of every Cell in a Trace matches the recorded Digest. (Cheap metadata check).
-   **Resource**: A Formula with a lifecycle (Setup/Teardown).

## Design

### 1. Addressing Schema

We formally adopt a URI-like schema for Root IDs to support parameterization.

**Format**: `derived:<family>:<arg>`

-   **Family**: The namespace (e.g. `task`, `file`).
-   **Arg**: The parameter (e.g. `123`, `/src/main.rs`).

Example: `derived:file:/src/lib.rs` represents the derived status of a specific file.

### 2. Factory Registration API

Instead of registering static values, we register **Factories**.

```typescript
// packages/exosuit-vscode/src/services/DerivedRootRegistry.ts

type RootFactory<T> = (arg: string, scope: DerivedScope) => T;

interface DerivedRootRegistry {
  /**
   * Register a factory for a dynamic family.
   * Logic: "When someone asks for derived:family:X, call this factory with X."
   */
  registerFactory<T>(family: string, factory: RootFactory<T>): void;
}
```

**Usage:**

```typescript
registry.registerFactory("task", (taskId, scope) => {
  // 1. Read the implementation plan (Dependency)
  const plan = scope.read("derived:implementation-plan", tomlDecoder);
  
  // 2. Select just the data we need (Formula)
  return plan.tasks.find(t => t.id === taskId);
});
```

### 3. Lifecycle: The "Active Pin" Model

We solve the "Garbage Collection" problem by making lifecycle management **organic**.

**Concept**: A dynamic root exists *if and only if* a UI component (or Agent) is strictly looking at it.

#### The Bridge Pinning Protocol
We utilize the **MachineChannel** (or Reactive Bridge) to track subscribers.

1.  **Mount**: When a webview component mounts `useDerived("derived:task:123")`, it sends a `PIN` signal.
2.  **Ref Counting**: The Host increments the RefCount for that ID.
    -   **0 -> 1**: The Root is **Instantiated**. The Factory runs, the initial Trace is recorded. It enters the **Active Set**.
3.  **Updates**: When files change, the System notifies all roots in the **Active Set**: *"Something changed. Validate yourselves."*
4.  **Unmount**: When the component unmounts (or tab closes), it sends `UNPIN`.
5.  **Teardown**: The Host decrements RefCount.
    -   **1 -> 0**: The Root is **Disposed**. It is removed from the Active Set. It stops checking for updates.

### 4. Hierarchical Caching (Internal Computeds)

To avoid computing massive monolithic roots, we introduce **Internal Caching**. This optimizes the computation graph by enabling "Cut-Offs" where intermediate nodes absorb changes.

**API: `scope.memo<T>(formula: () => T): T`**

This allows a Factory to create anonymous internal nodes.

1.  **Usage**:
    ```typescript
    registry.registerFactory("phase.goals", (phaseId, scope) => {
      // 1. Compute/Cache the raw plan structure (Expensive I/O or parsing)
      const rawPlan = scope.memo(() => {
         return scope.read("agent.plan.toml", parseToml);
      });

      // 2. Derive specific goals (Cheap)
      // If 'agent.plan.toml' changes, 'rawPlan' re-validates.
      // If 'rawPlan' result is unchanged (same structure), this downstream code DOES NOT run.
      return rawPlan.phases[phaseId].goals;
    });
    ```

2.  **Mechanism (Graph Dependencies)**:
    *   **Node**: The `memo` creates a transient Root ID (e.g. `internal:uid:123`).
    *   **Dependency**: The Parent consumes a dependency on this Node ID (Graph Edge), not the Node's dependencies (Flattening).
    *   **Validation**:
        *   Parent asks: "Is `internal:uid:123` valid?"
        *   System checks `internal:uid:123`'s trace recursively.
        *   If valid -> Return Cached Value.
        *   If invalid -> Re-run `formula`.
    *   **The Cut-Off**: If re-running `formula` produces a value with the same **Digest/Revision**, the `internal:uid:123` revision is *not* bumped. The Parent sees the dependency as valid and skips its own re-computation.

### 5. The Reactivity Loop (Pull-Based)


This is the Starbeam Lite model adapted for Exosuit.

1.  **Mutation**: File `plan.toml` changes. System bumps its **Global Revision** / **Digest**.
2.  **Notification**: System sends a coalesced "Dirty" signal (e.g. via `requestIdleCallback`) to the **Active Set** (Pinned Roots).
3.  **Validation (The Pull)**: 
    -   Each Pinned Root checks its Trace.
    -   *"Is `plan.toml` digest == recorded digest?"*
    -   **Match**: No-op. (Cheap).
    -   **Mismatch**: Re-run the Factory.
4.  **Propagation**: 
    -   Run Factory. Produce new Value. 
    -   If `New Value != Old Value`: Push update to Webview via Bridge.

## Implementation Plan

1.  **Registry Upgrade**: 
    -   Update `DerivedRootRegistry` to support `registerFactory`.
    -   Implement "Address Parsing" logic (`derived:family:arg`).
2.  **Lifecycle Manager**:
    -   Create `PinnedRootsService` to handle RefCounting.
    -   Connect it to `MachineChannel` / Bridge `PIN`/`UNPIN` messages.
3.  **Bridge Hook**:
    -   Create `useDerived(id)` hook for Svelte/Webview that manages the PIN protocol automatically.

## Cross-References
-   [Resource Algebra](../../specs/algebras/resource.md)
-   RFC 00188 (Derived Roots)
-   Starbeam (Incremental Reactivity)

