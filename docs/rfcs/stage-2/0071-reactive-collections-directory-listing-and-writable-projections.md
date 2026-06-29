<!-- exo:71 ulid:01kmzxbcz097fkdpf7yycd5mw7 -->

# RFC 71: Reactive Collections: Directory Listing and Writable Projections


---

ulid: 01kg5kp2efkqget3f7zqzcc01m
title: Reactive Collections: Directory Listing and Writable Projections
feature: Reactivity
exo:
tool: exo rfc create
protocol: 1

---

# RFC 0071: Reactive Collections: Directory Listing and Writable Projections

## Summary

This RFC specifies how the **Resource Protocol** integrates with Exosuit’s existing reactivity system to make projections first-class and UI-friendly.

Key ideas:

- Introduce a reactive **Directory Listing** primitive (`DirListing`) as a foundational source.
- Treat many resources (notably RFCs) as **projection-backed resources**: stage and metadata can be derived from directory structure + file content.
- Define a standard way to expose **revisioned snapshots** and **dependency/source stamps** at the protocol boundary.
- Define how UIs perform **writes** when their read view is a projection: projections are read-only; operations mutate roots.
- Standardize writable list operations via `Position` and collection operations (insert/move/remove), rather than arbitrary mutation.

## Motivation

The Exosuit RFC corpus is inherently projection-shaped:

- RFC “stage” is controlled by directory placement (`docs/rfcs/stage-*`).
- RFC metadata and body are file-backed.
- A “list of RFCs” is naturally a derived, filterable, sortable sequence.

To support a generic VS Code client and reactive UI frameworks, the protocol must carry enough information for caching + invalidation without bespoke per-resource wiring.

## Goals

- Provide a reactive directory listing primitive suitable for RFC corpus projections.
- Ensure protocol responses can be consumed as reactive snapshots.
- Define writable interactions over projection-backed reads via standardized operations.
- Enable VS Code invalidation via file watching (not polling).

## Non-Goals

- Define the complete RFC schema.
- Design VS Code UI components.

## Directory Listing as a Reactive Primitive

We introduce a core reactive source:

- **`DirListing`**: a reactive sequence of directory entries.

### Existing foundation (current codebase)

`crates/exosuit-reactivity` already contains a substantial filesystem substrate (`rfs`):

- `DiskCell` with stable content hashing (`Revision::Disk`).
- `DirectoryCell` with deterministic Merkle hashing over entries.
- `Engine` supports source-scoped invalidation (`source_index`).
- `rfs::FileSystem::notify_changed` invalidates a changed path and its ancestor directories.

What is not yet present (and is required to realize `DirListing` end-to-end):

- A disk-backed loader that materializes `DirectoryCell` from the real filesystem (and stores/fetches child directory cells by revision).
- A canonical mapping from filesystem paths → `CellId` conventions for directory vs file cells.
- An integration layer that registers disk/directory cells into the runtime/engine as reactive sources.

In the VS Code extension, this integration is expected to be exposed as a **Root Materializer Registry**:

- a client-side registry keyed by root ID/address
- each root can be materialized by reading from disk and registering/refreshing the runtime’s disk/directory cells
- this is the only place where filesystem I/O is performed for these roots

Properties:

- Produces a `ReactiveSequence<DirEntry>` (or equivalent) suitable as an upstream for projections.
- Stable identity for each entry (path-based canonical ref initially).
- Revisions change when directory membership or relevant stat/content changes.

`DirEntry` should include at minimum:

- canonical path ref
- file kind (file/dir/symlink)
- minimal stampable metadata needed to detect changes

For alignment with the existing Merkle-based model in `exosuit-reactivity`, the minimal stampable metadata is:

- entry name
- entry kind
- entry revision (child content hash)

Where the entry revision is:

- file: hash(file bytes)
- dir: hash(child `DirectoryCell` entries)
- symlink: hash(link target bytes (e.g. `read_link`), not the dereferenced target’s contents)

## Snapshots, Revisions, and Dependency Reporting

Protocol-visible projection results should include:

- a **revision token** (monotonic in practice: hash/mtime/sequence)
- a set of **source stamps** identifying the underlying inputs the projection depends on

This is not re-implementing the reactive engine; it is exposing enough boundary metadata for clients.

## Projection-Backed Resources (RFCs)

Some resources are projections:

- An RFC resource’s `stage` is derived from its directory.
- The RFC read model is derived from file content + directory context.

Therefore, “resource identity” and “resource representation” may be derived:

- `RfcRef` should be stable even when the file moves between stage directories.
  - Canonical identity: `rfc:<number>` (or equivalent stable typed ref)
  - Location: a filesystem path (which determines stage)
- `RfcView` is a snapshot/projection derived from (location + file content).

## Writable Projections: Operations Mutate Roots

Rule:

- **Projections are read-only. Operations are writable.**

When a UI is rendering a projection (e.g. a list of RFCs), it must not “edit the projection”; it must call an operation that mutates canonical roots (files/directories) so the projection changes.

To keep mutation constrained and consistent, we standardize a small set of operation shapes.

### Collections as First-Class Resources

Introduce the concept of a **Collection** resource:

- A collection’s read model is often projection-backed (filter/sort is a derived view).
- A collection advertises standardized list operations.

Standard operations (normative vocabulary):

- `collection.insert(Position, NewItemSpec)`
- `collection.move(ItemRef, Position)`
- `collection.remove(ItemRef)`

These operations are implemented by mutating roots (create/move/delete files, edit canonical metadata, etc.).

### `Position` as the Standard List Operation Shape

Collections use the `Position` protocol:

- `prepend` / `append` (requires container)
- `before(anchor)` / `after(anchor)` (requires container + anchor)

When container references are resolved via aliases (e.g. “active”), machine output must echo canonical resolution.

## VS Code Invalidation Transport

VS Code should use its file watching APIs as the invalidation signal source.

### Long-lived reactive runtime (WASM in the extension)

This RFC assumes a **long-lived reactive runtime** exists and is embedded in the VS Code extension as a WASM module built from the existing Rust reactivity crates.

This matches the current codebase shape (`WasmEngine` in `crates/exosuit-reactivity/src/wasm.rs` and the extension-side `ReactivityService`).

### Watcher events are invalidation triggers (not digest reads)

Watcher notifications should be forwarded into the runtime as messages that mean:

- “file at path X changed”
- (optionally) “directory at path X changed”

On receipt, the runtime should:

- mark the relevant reactive cell(s) as invalid
- compute which registered roots are affected
- notify the client (e.g. with affected root IDs)

Critically, the runtime should **not** read the file, compute a digest, or otherwise perform I/O during this invalidation step.

Whether a digest is re-read (and whether any expensive computation is re-run) is determined by the next **top-down revalidation** pass.

### Watcher is a trigger; digests are authoritative

Watcher events are a low-latency trigger to schedule a top-down revalidation pass; they are not the source of truth.

Correctness must remain rebuildable from disk:

- if the watcher misses an event, revalidation (driven by demand) must still detect staleness via digest/Merkle validation
- if the watcher produces spurious events, revalidation may conclude values are still valid (harmless)

### Top-down revalidation must “consume” dependencies even on cache hits

When the client performs a revalidation pass (e.g. after receiving invalidations, or when a UI becomes visible), it may re-use cached values if the runtime determines they are still valid.

However, for correctness and cache lifecycle, it is imperative that revalidation causes the reactive cells that were used by the last computation to be treated as **consumed again**.

This ensures:

- roots survive any mark/sweep or liveness cycle
- nested observation works correctly (a parent observe scope should re-establish dependencies on a cached child computation)

Operationally: a “cache hit” should still re-link the cached computation’s dependency trace into the current observation scope.

This avoids the failure mode where cached computations remain valid but their upstream dependencies are no longer considered live/linked, causing incorrect invalidation behavior later.

### Minimal TS client API sketch

To make dependency tracking explicit and support nesting, the TS client should be shaped around explicit observation scopes.

> **Implementation Note (2026-02-02):** The codebase has two API layers:
>
> - **High-level**: `ObservationService` with `beginObserve()`/`endObserve()` — matches this sketch
> - **Low-level**: `ReactivityService` with `beginTrack()`/`endTrack()`/`scopeRead()`/`validateTrace()` — direct WASM wrapper
>
> Consumers can use either layer. `InboxStatusBarService` and `DashboardProvider` use the low-level API directly for simpler integration.

Sketch (not normative syntax):

- `const scope = client.beginObserve(parentHandle?)`
- inside the observation (render/update):
  - `scope.read(address, decode): Availability<T>`
- `const handle = client.endObserve(scope)`

The client is expected to have a **Root Materializer Registry**:

- `client.registerRootMaterializer(rootId, materializer)`

where `materializer` is responsible for the only permitted disk I/O:

- reading the authoritative bytes/entries from disk
- registering/updating the runtime’s disk/directory cells and their revisions

Where `handle` is both lifecycle-bound and sufficient for top-down revalidation:

- `handle.revision` (the observation revision)
- `handle.revalidate(prevRevision)` (revalidate against a prior revision; may be a no-op if still valid)
- `handle.dispose()` (drops the server-side subscription / observation node)

This handle structure enables a single top-down observation tree:

- UI containment maps to observation containment (pane → section → widget)
- revalidation is a top-down tree walk; if a parent is removed/disposed, children are naturally not reached and therefore do not demand values

Where `decode` converts protocol JSON into `T` (e.g. a zod schema or decoder function).

Notes:

- Dependency tracking is server/runtime-owned: `scope.read` establishes/consumes deps in the runtime regardless of cache hits.
- The returned handle is disposable and should be wired to the UI component lifecycle (Svelte component disposal, VS Code view disposal, etc.).

### Projection reads must be up to date (demand implies revalidation)

Any reactive primitive that returns cached projection values must ensure that an explicit read returns an up-to-date value.

Operationally:

- `scope.read(...)` is a **demand**.
- On demand, the runtime must validate the cached computation’s trace against current revisions.
  - If valid: return `Availability::Present(cached_value)` (and still consume/link dependencies for this observation).
  - If invalid: re-compute the projection (which produces a new revision/trace), then return `Availability::Present(new_value)`.
  - If the value cannot be produced (missing inputs, races, etc.): return `Availability::Absent(reason)`.

This fits the “Iron Rule” model: validation compares revision IDs (metadata). Refreshing the revision of an external source (e.g. computing a file’s new hash) may require I/O, but that happens only because the value was demanded by top-down revalidation.

When a demanded root is not yet materialized (no known digest/revision), the client should invoke the Root Materializer Registry for that root ID/address. This ensures:

- invalidation remains I/O-free
- disk reads happen only as part of demand/revalidation

This avoids polling and respects VS Code’s ignore/exclusion configuration.

## Customer Zero: RFC Corpus

Prove the model end-to-end using the RFC corpus:

- `DirListing(docs/rfcs/**)`
- Projection: `collection:rfcs` (list RFCs, stage derived from path)
- Operations:
  - create RFC (insert)
  - move RFC across stages (move)
  - delete RFC (remove)

---

## Technical Specification (Stage 2)

This section provides the detailed technical specification for implementing RFC 0071.

### 1. WASM Trace API

The WASM engine exposes scope-based dependency tracking via `WasmEngine` in `crates/exosuit-reactivity/src/wasm.rs`:

```rust
#[wasm_bindgen]
impl WasmEngine {
    /// Begin a new tracking scope. Returns a scope ID (ULID).
    /// If parent_scope_id is provided, dependencies will propagate to parent on end_track.
    pub fn begin_track(&mut self, parent_scope_id: Option<String>) -> String;

    /// End a tracking scope and return the serialized Trace as JSON.
    /// Dependencies are flattened into the parent scope (if any).
    pub fn end_track(&mut self, scope_id: &str) -> Result<String, JsValue>;

    /// Record a dependency in the specified scope.
    pub fn record_dependency(
        &mut self,
        scope_id: &str,
        cell_id_json: &str,
        revision_json: &str
    ) -> Result<(), JsValue>;

    /// Get the current revision hash for a scope (Merkle of all deps).
    pub fn get_scope_revision(&self, scope_id: &str) -> Result<String, JsValue>;

    /// Read a root within a scope: records dependency, returns value + digest.
    /// This is the primary read API for observation scopes.
    pub fn scope_read(&mut self, scope_id: &str, root_id: &str) -> Result<String, JsValue>;
    // Returns: JSON { value: any, digest: string }

    /// Validate a trace against current state. Returns true if all deps still match.
    /// Per the Iron Rule (docs/specs/algebras/reactivity.md §0), this is O(|T|) metadata comparisons.
    pub fn validate_trace(&self, trace_json: &str) -> Result<bool, JsValue>;
}
```

**Design rationale** (per `docs/specs/algebras/reactivity.md`):

1. **The Iron Rule**: Validation is metadata-only. `validate_trace` compares revision IDs without touching values.
2. **Trace Flattening** (§8): Child scope deps propagate to parent on `end_track`. No intermediate nodes in the graph.
3. **All-or-nothing validation**: If trace is valid, use cached value. If not, re-run computation. No partial revalidation.

**Implementation notes:**

- Scope IDs are ULIDs generated by the WASM engine
- Parent-child relationship enables trace flattening (child deps propagate to parent)
- Scopes are stored in a `HashMap<String, ScopeState>` inside `WasmEngine`
- The digest returned by `scope_read` serves as the revision for roots (they're equivalent)

### 2. Enhanced ObservationService

Update `packages/exosuit-vscode/src/services/ObservationService.ts`:

```typescript
export interface ObserveScope {
  readonly id: string;
  readonly parentId: string | undefined;

  /**
   * Read a root, recording it as a dependency in the WASM trace.
   * If the root doesn't exist and a materializer is registered, materialize it.
   */
  read<T>(rootId: string, decode: Decoder<T>): Availability<T>;

  /**
   * Create a nested observation scope.
   * Dependencies from the child are propagated to this scope via WASM.
   */
  beginChild(id?: string): ObserveScope;

  /**
   * End this observation scope and return a handle.
   */
  endObserve(): ObserveHandle;
}

export interface ObserveHandle {
  readonly id: string;
  readonly revision: string;
  readonly parentId: string | undefined;

  /**
   * Revalidate against a previous revision.
   * Returns { ok: true, revision } if still valid.
   * Returns { ok: false, revision: newRevision } if stale.
   */
  revalidate(prevRevision: string): { ok: boolean; revision: string };

  /**
   * Dispose this handle and release dependencies.
   * Note: Child handles are NOT automatically disposed.
   */
  dispose(): void;
}
```

**Key changes from current implementation:**

- `beginChild()` method for nested scopes
- `parentId` field on both `ObserveScope` and `ObserveHandle`
- Dependencies recorded via WASM `record_dependency()` instead of local Map

### 3. DirListing Primitive

**Type definition** (new file or in `types/reactivity.ts`):

```typescript
export interface DirEntry {
  /** Entry name (not full path) */
  name: string;
  /** Entry kind */
  kind: "file" | "dir" | "symlink";
  /** Child content hash (SHA-256 hex) */
  childHash: string;
}

export interface DirListing {
  /** Absolute path to directory */
  path: string;
  /** Merkle hash of the listing */
  hash: string;
  /** Sorted entries (alphabetical by name) */
  entries: DirEntry[];
}
```

**Root ID convention:**

```
dir:<absolute-path>

Examples:
- dir:/workspace/docs/rfcs
- dir:/workspace/docs/rfcs/stage-1
```

**RootMaterializerRegistry addition:**

```typescript
// In RootMaterializerRegistry.materialize()
if (rootId.startsWith("dir:")) {
  const dirPath = rootId.slice(4); // Remove 'dir:' prefix
  if (!fs.existsSync(dirPath)) {
    return false;
  }
  const listing = reactivityService.ingestShallowDirListing(dirPath);
  const value: DirListing = {
    path: dirPath,
    hash: listing.dirHash,
    entries: listing.entries.map((e) => ({
      name: e.name,
      kind: e.kind,
      childHash:
        listing.ingested.find((i) => i.path.endsWith(e.name))?.hash ?? "",
    })),
  };
  reactivityService.registerRoot(
    rootId,
    traceForDiskPath(dirPath, listing.dirHash),
    value,
    true,
  );
  return true;
}
```

### 4. CellId Conventions

Standardize cell identity across the system:

| Resource Type | CellId Format                             | Example                                              |
| ------------- | ----------------------------------------- | ---------------------------------------------------- |
| File          | `{ source_id: "<path>", pointer: "" }`    | `{ source_id: "/workspace/plan.toml", pointer: "" }` |
| Directory     | `{ source_id: "<path>", pointer: "" }`    | `{ source_id: "/workspace/docs/rfcs", pointer: "" }` |
| Root          | `{ source_id: "root:<id>", pointer: "" }` | `{ source_id: "root:agent.plan.toml", pointer: "" }` |

**Note:** Files and directories share the same format. The engine distinguishes them by which materializer registered them.

### 5. Error Handling

```typescript
export type ObservationError =
  | { kind: "MaterializationFailed"; rootId: string; cause: Error }
  | { kind: "RootNotFound"; rootId: string }
  | { kind: "ScopeDisposed"; scopeId: string }
  | { kind: "ScopeNotFound"; scopeId: string }
  | { kind: "InvalidTrace"; message: string };

// Availability type (already exists, formalized here)
export type Availability<T> =
  | { kind: "Present"; value: T }
  | {
      kind: "Absent";
      reason: "Loading" | "Error" | "Corrupted";
      message?: string;
    };
```

### 6. Migration Path

**Phase 1: WASM APIs** ✅ COMPLETE

- ✅ Add `begin_track`, `end_track`, `record_dependency`, `get_scope_revision` to WASM
- ✅ Add corresponding methods to `ReactivityService`
- ✅ Existing `ObservationService` continues to work unchanged

**Phase 2: Complete WASM API + Simplify TS** (current)

- Add `scope_read` to WASM (read root, record dep, return value+digest)
- Add `validate_trace` to WASM (validate trace, return bool)
- Simplify `ObservationService`: remove `#deps` Map, use WASM for all tracking
- Add property tests for nested scopes

**Phase 3: First consumer migration** (`StatusBarService`)

- Replace manual `onDidInvalidateRoots` subscription with observation scope
- Use `handle.revalidate()` pattern
- Validate memory lifecycle with `dispose()`

**Phase 4: DirListing integration** (`ContextService`)

- Add `dir:` prefix support to `RootMaterializerRegistry`
- Migrate RFC listing from manual watcher to `DirListing` root
- Use nested scopes for RFC corpus traversal

### 7. Test Strategy

**Property tests** (Rust, `crates/exosuit-reactivity/src/wasm.rs` or dedicated test module):

- Nested scope invariants: child deps always appear in parent after `end_track`
- Trace validation: `validate_trace(trace)` returns true iff all deps unchanged
- Invalidation propagation: changing a cell invalidates all scopes depending on it
- Scope isolation: sibling scopes don't affect each other

**Unit tests** (`ObservationService.test.ts`):

- Nested scope creation and disposal
- Dependency propagation from child to parent
- Revision computation with nested deps
- Error cases (disposed scope, missing root)

**WASM integration tests** (`reactivity.test.ts`):

- `begin_track` / `end_track` round-trip
- `scope_read` records dependency and returns value+digest
- `validate_trace` returns correct boolean
- Parent-child scope trace flattening
- Scope cleanup on `end_track`

**E2E tests**:

- DirListing invalidation when file added/removed
- Nested observation with RFC corpus
- Memory leak detection for long-running scopes

---

## Implementation Status

> **Note:** This section documents the current implementation state as of 2026-02-02.

### Already Implemented

| Component                  | Location                                                           | Notes                                                                                          |
| -------------------------- | ------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| `ObserveScope` class       | `packages/exosuit-vscode/src/services/ObservationService.ts`       | Core observation scope with `read()` method                                                    |
| `ObserveHandle` interface  | `packages/exosuit-vscode/src/services/ObservationService.ts`       | `revision`, `revalidate()`, `dispose()`                                                        |
| `Availability<T>` type     | `packages/exosuit-vscode/src/services/ObservationService.ts`       | `Present` / `Absent` with reason                                                               |
| `RootMaterializerRegistry` | `packages/exosuit-vscode/src/services/RootMaterializerRegistry.ts` | Materializes `agent.plan.toml`, `agent.implementation-plan.toml`, `agent.rfcs.dir`             |
| WASM Engine core           | `crates/exosuit-reactivity/src/wasm.rs`                            | `register_root`, `validate_root`, `notify_file_change`                                         |
| **WASM scope tracking**    | `crates/exosuit-reactivity/src/wasm.rs`                            | `begin_track`, `end_track`, `record_dependency`, `get_scope_revision`                          |
| **`scope_read`**           | `crates/exosuit-reactivity/src/wasm.rs`                            | Read root within scope, records dep, returns value+digest (per Iron Rule: no staleness check)  |
| **`validate_trace`**       | `crates/exosuit-reactivity/src/wasm.rs`                            | Validate trace against current state (Iron Rule: metadata only)                                |
| **TS scope wrappers**      | `packages/exosuit-vscode/src/services/ReactivityService.ts`        | `beginTrack`, `endTrack`, `recordDependency`, `getScopeRevision`, `scopeRead`, `validateTrace` |
| Rust `ACTIVE_TRACE` stack  | `crates/exosuit-reactivity/src/runtime.rs`                         | Thread-local trace stack for dependency capture                                                |
| Directory hashing          | `ReactivityService.ingestShallowDirListing()`                      | Merkle-style hashing matching Rust `DirectoryCell`                                             |
| Trace flattening           | `end_track` in WASM                                                | Child deps propagate to parent scope                                                           |

### Not Yet Implemented

| Component                   | Description                                                       | Size |
| --------------------------- | ----------------------------------------------------------------- | ---- |
| Property tests              | Nested scopes, revalidation, invalidation propagation             | L    |
| `DirListing` primitive      | Not a first-class reactive type (Phase 4)                         | L    |
| Collection operations       | `insert`, `move`, `remove` with `Position` protocol (future RFC)  | —    |
| Projection-backed resources | `RfcRef` with stable identity separate from location (future RFC) | —    |

### Recently Completed

| Component                   | Description                                                     | Date       |
| --------------------------- | --------------------------------------------------------------- | ---------- |
| Simplify ObservationService | `#deps` Map only populated when WASM tracking is NOT active     | 2026-02-02 |
| Iron Rule compliance        | `scopeRead` no longer validates staleness (Data Operation only) | 2026-02-02 |
| DashboardProvider migration | Uses ReactivityService directly instead of ObservationService   | 2026-02-02 |

## Open Questions

### Resolved

- **What is the initial `NewItemSpec` for RFC creation?**
  Answer: `{ title: string, feature?: string, stage: 0 | 1 | 2 | 3 | 4 }`

- **What is the minimal `sources[]` schema for invalidation?**
  Answer: Paths only. Hashes/mtimes are internal to the engine.

- **Should parent handle nesting be required for the MVP?**
  Answer: No. Flat observation scopes work for MVP. Parent handle nesting is additive and can be adopted incrementally by consumers that need UI containment semantics.

- **What is the migration path for existing consumers?**
  Answer: See "Migration Path" in Technical Specification. Four phases: WASM APIs → ObservationService enhancements → First consumer → DirListing integration. Existing consumers continue to work unchanged until they opt-in.

### Open

- How does ordering interact with filesystem ordering (explicit ordering projection vs directory listing order)?
  - Current answer: Directory listings are sorted alphabetically by name. Explicit ordering projections are deferred to a future RFC.

- Should collection operations (`insert`, `move`, `remove`) be part of this RFC or split into a separate RFC?
  - Current answer: Deferred. This RFC focuses on observation scopes and DirListing. Collection operations can be a follow-up RFC once the reactive foundation is solid.

