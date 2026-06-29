<!-- exo:10164 ulid:01kmzxefekh6wrnvcfj08wef4y -->


# RFC 10164: Principled Invalidation: Pure Trace Matching and Resource Pattern

## Problem

The reactive invalidation system has architectural issues discovered during the Phase Details staleness bug (commit 9f11acc):

### 1. handleInvalidation uses mutable WASM state

`DerivedRootRegistry.#handleInvalidation` called `validateTrace()` which checks the trace against `Engine.current_state` — global mutable WASM state. This creates a race: if another subscriber recomputes a derived root between `bumpStateRoot()` and the handler running, WASM state matches the fresh trace and `validateTrace()` returns "valid" even though the cache entry is logically stale.

**Current fix**: String-scan on serialized trace JSON to check `source_id` overlap with invalidated roots. Works but unprincipled.

**Correct fix**: `handleInvalidation` should deserialize the trace and do a pure set-intersection of `trace.dependencies[].cell_id.source_id` against `invalidatedRoots`. No WASM round-trip needed. `validateTrace()` should only run inside `get()` as the authoritative freshness check at read time.

### 2. Dual notification system

Tree providers get data updates through TWO parallel mechanisms:

- **Reactive path**: `derivedRootRegistry.get()` inside `getChildren()` with traced reads
- **File watcher path**: `extension.ts` has `FileSystemWatcher` instances that call `planService.invalidate()` + `treeProvider.refresh()`

For `phase-details`, the reactive path covers the data (since `computePhaseDetails` reads `agent.plan.toml` via `scope.read()`). But for `project-plan` and `epoch-details` branches, data flows through `PlanService.getPlan()` which is an untracked async call — updates only work because the file watcher catches them.

### 3. The Resource pattern gap

The root cause is the lack of an async-to-reactive bridge. Currently:

- All `scope.read()` calls are synchronous (file reads, state reads)
- `PlanService.getPlan()` is async but lives outside tracking
- `getChildren()` has `await` calls before `derivedRootRegistry.get()` — these aren't tracked

The Resource pattern (sketched in `docs/specs/algebras/resource.md`) would solve this: reify async loads as reactive cells with `Availability<T>` values (`Present | Absent`). Then ALL consumers read through tracked scopes, and the file watcher becomes purely a mutation trigger (Phase A), not a parallel notification system.

## Key Principle

Auto-tracking MUST NOT cross async boundaries. Within a synchronous execution frame, a global "current scope" variable is safe because JavaScript is run-to-completion. But any `await` breaks the tracking contract. The pattern is:

1. Async operations produce Resources (Phase C: Commit)
2. Resources expose synchronous reactive cells
3. Computations read cells synchronously (Phase B: Render)

See `docs/specs/algebras/resource.md` for the full formalization.

## Related RFCs

**Parents** (this RFC refines or extends):

- **RFC 00188** (Stage 3) — _Derived Roots & Reactive Caches_: 10164 fixes how derived roots handle invalidation, a gap in the original design.
- **RFC 10143** (Stage 3) — _Validation-Based Reactive Architecture_: The `validateTrace()` flaw contradicts the pure-validation premise this architecture depends on.

**Siblings** (parallel concerns):

- **RFC 00241** (Stage 1) — _Reactive State Roots_: Directly related — state roots participate in the invalidation path that the string-scan hack patches.
- **RFC 00237** (Stage 1) — _Dynamic Derived Roots (Reactive Families)_: Extends the derived root model that 10164 patches.
- **RFC 10147** (Stage 3) — _Reactive File System_: The dual notification problem (file watchers + reactive doing one job) is a gap in the reactive file system vision.

**Epoch alignment**: The "Immediate cleanup" layer could fit into the current **Validation Flywheel** phase of the **Goal Loop** epoch. Medium-term and longer-term layers are future-epoch work.

## Concrete Work Items

### Immediate (cleanup of current fix)

- Replace string-scan hack in `handleInvalidation` with proper trace deserialization + set intersection
- Remove `validateTrace()` call from `handleInvalidation` entirely (keep only in `get()`)

### Medium-term (eliminate dual notification)

- Migrate `project-plan` and `epoch-details` tree branches to use derived roots
- Remove redundant file watchers from `extension.ts`
- Unify all tree data flow through reactive reads

### Longer-term (Resource pattern)

- Design the Resource primitive for async-to-reactive bridging
- Reify `PlanService.getPlan()` as a Resource cell
- Ensure no auto-tracking crosses async boundaries anywhere in the extension
