<!-- exo:10167 ulid:01kmzxeffbfj16fabrp3y2e1e5 -->


# RFC 10167: Unified Derivation Layer

## Summary

All derived computations (epoch status, phase details, RFC status, steering, etc.) should be computed by Rust functions compiled into both the CLI binary and the WASM reactivity module. The TypeScript side should never reimplement derivation logic — it reads roots and calls into WASM for computation. The reactivity system is layered on top to provide invalidation signals.

## Motivation

Today, derived values are computed through multiple parallel mechanisms:

1. **File roots → TS-computed derived roots** — The reactive sidebar reads plan.toml as a file root, then TS reimplements derivation logic (finding active epoch, computing phase details, etc.)
2. **StatusService** — Calls `exo status --format json` (CLI), maintains its own 5s TTL cache, and is invalidated by reactivity — but feeds none of its results back into the root system.
3. **MachineChannelServer** — Tries TS-computed derived roots first, then falls through to a persistent `exo json server` subprocess for everything else.
4. **RfcStatusService** — Runs a standalone `rfc-status.wasm` via `@vscode/wasm-wasi`, completely outside the root system.
5. **Rust serializer** — `Epoch::serialize()` writes `derived_status()` back into plan.toml as the `status` field, creating a stale snapshot that TS consumers accidentally treat as a source of truth.

This creates several problems:

- **Duplicated logic**: Epoch status derivation exists in Rust (`derived_status()`) AND was being reimplemented in TS. The algorithms can drift.
- **Stale derived data on disk**: The Rust serializer writes derived values to plan.toml. Other readers (TS, editors, humans) see stale values between writes.
- **Parallel caching systems**: StatusService has its own cache with its own TTL, separate from the reactive root system.
- **Inconsistent computation paths**: The sidebar, LM tools, and CLI may compute the same value through different code paths and get different answers.

## Design

### Target Architecture

```
  Disk Files / In-Memory State
        │
        ▼
  ReactivityService (WASM)
  ├── Layer 1: Root Storage (exists today)
  │   Registers root values in-process.
  │
  ├── Layer 2: Invalidation (exists today)
  │   File change → which roots stale → fire events.
  │
  └── Layer 3: Derivation (NEW)
      Rust functions that compute from root data.
      Same source code compiled into CLI + WASM.

      derive_epoch_status(phases) → status
      derive_phase_details(plan, impl_plan) → details
      derive_rfc_index(rfcs_dir) → index
      derive_rfc_status(rfcs_dir) → statuses
      derive_status(plan, impl_plan, inbox) → full status
      etc.
        │
        ▼
  DerivedRootRegistry (TS)
  Pure plumbing: calls WASM, caches, exposes to consumers.
  Contains NO business logic.
        │
        ▼
  Consumers (all read derived roots the same way)
  - Sidebar UI
  - LM Tools (via MachineChannel)
  - Commands
```

### Current Mechanisms and Their Fate

**Mechanisms that stay (least refactoring):**

| #   | Mechanism                      | Why It's Already Right                                               |
| --- | ------------------------------ | -------------------------------------------------------------------- |
| 1   | File roots → ReactivityService | Plumbing is correct. Only the computation layer changes (TS → WASM). |
| 6   | State roots (in-memory)        | Already tracked by WASM engine. No change needed.                    |

**Mechanisms that get subsumed:**

| #   | Mechanism                                | Becomes                                           |
| --- | ---------------------------------------- | ------------------------------------------------- |
| 2   | StatusService (CLI + 5s cache)           | A derived root backed by WASM computation         |
| 3   | MachineChannel CLI fallthrough           | Reads derived roots (they're always computed now) |
| 4   | RfcStatusService (standalone WASM)       | A derived root backed by shared WASM              |
| 5   | Rust serializer writing derived `status` | Removed — derived means never stored              |

### Key Principles

1. **If it can live in Rust, it lives in Rust.** TS exists to interact with VS Code APIs — that's its job, and it's important. But parsing, schema validation, derivation, and all domain logic belong in Rust. "Who should parse TOML" is not a hard call.

2. **Derived means computed, never stored.** The `status` field should not be written to plan.toml for epochs. Derivation happens at read time.

3. **One implementation, two compilation targets.** Rust derivation functions are compiled into the CLI binary (for command-line use) and into the WASM module (for in-process extension use). Same source, same results.

4. **Reactivity is a layer, not a requirement.** The derivation functions are pure: they take data in, return computed values. The reactive system calls them when inputs change. CLI commands call them directly. No coupling.

5. **TS derived roots become thin wrappers.** `DerivedRootRegistry` entries stop containing business logic. They call WASM exports and return the result.

6. **WASM derivation functions read roots directly.** Derivation functions use `scope_read()` / `get_root_value()` to access root data from the engine's internal storage. This keeps dependency tracking implicit and prevents TS from needing to know what a derivation's inputs are.

### Migration Path

The migration is incremental. Each step is independently shippable:

1. **Expose derivation functions as WASM exports** — Add `derive_epoch_status()`, etc. to `exosuit_reactivity` crate alongside existing tracking functions. Shared Rust source with CLI.

2. **Convert derived roots one by one** — Each derived root goes from "TS computes from raw file content" to "TS calls WASM, WASM computes from root data." Start with epoch status (simplest), then phase details, then RFC index.

3. **Subsume StatusService** — Its consumers read derived roots instead of calling the CLI. The 5s TTL cache is replaced by reactive invalidation.

4. **Subsume RfcStatusService** — RFC status computation moves from standalone `rfc-status.wasm` into the shared WASM module.

5. **Stop writing derived values to plan.toml** — `Epoch::serialize()` drops the `status` field. `EpochInput` already ignores it on read, so backward compat is free.

6. **Clean up** — Remove dead code paths, parallel caching, CLI-specific JSON formatters that exist only because the extension needed them.

## Drawbacks

- **WASM module grows** — Adding derivation logic to `exosuit_reactivity` increases its size. Currently it's pure tracking; this adds domain logic.
- **Shared code coupling** — Derivation functions must work in both CLI and WASM contexts. Anything that depends on platform-specific features (filesystem, etc.) needs abstraction.
- **Migration risk** — Incrementally replacing TS computation with WASM calls requires careful testing to ensure behavioral parity.

## Alternatives

- **Keep TS computation, just centralize it.** This is what was attempted in the previous commit (`findActiveEpoch()` in TS). Avoids WASM complexity but leaves logic duplicated across languages.
- **CLI bridge only.** Have derived roots call `exo` CLI for computation (like StatusService does). Simpler than WASM but adds ~100ms per derivation and requires process management.
- **Separate WASM modules per domain.** Like RfcStatusService today. Avoids coupling but multiplies the number of WASM modules to build and load.

## Resolved Questions

- **TOML parsing boundary**: WASM receives raw file contents. Rust parses. This follows directly from principle #1 — if it can live in Rust, it does.
- **WASM computation boundary**: Derivation functions read roots directly from the engine's internal storage via `scope_read()` / `get_root_value()`. TS materializes file contents into the engine and requests computed results — it never pre-parses or pre-selects inputs for WASM. This keeps dependency traces inside the WASM engine automatically.

## Unresolved Questions

- How should the derivation layer handle errors? Today TS derived roots return fallback values on parse errors. Should WASM do the same, or signal errors differently?
- The MachineChannel currently falls through to `exo json server` for non-derived operations. Should that subprocess eventually be eliminated too, or does it serve a distinct purpose?

## Codebase Audit

### TS Derived Roots (derivedRoots.ts)

8 derived roots, all currently computed in TypeScript:

| Root ID                       | Compute Function              | Complexity | Lines       | Can Move to WASM?                      |
| ----------------------------- | ----------------------------- | ---------- | ----------- | -------------------------------------- |
| `derived:inbox.summary`       | `computeInboxSummary`         | moderate   | L677–L704   | Yes                                    |
| `derived:ideas.summary`       | `computeIdeasSummary`         | moderate   | L706–L730   | Yes                                    |
| `derived:diagnostics.summary` | `computeDiagnosticSummary`    | moderate   | L732–L781   | **No** — reads VS Code diagnostics API |
| `derived:phase.active`        | `computePhaseActiveSummary`   | moderate   | L1074–L1131 | Yes                                    |
| `derived:rfc.index`           | `computeRfcIndex`             | moderate   | L398–L460   | Yes                                    |
| `derived:rfc.supersession`    | `computeRfcSupersessionIndex` | moderate   | L465–L558   | Yes                                    |
| `derived:rfc.pipeline`        | `computeRfcPipeline`          | moderate   | L570–L675   | Yes                                    |
| `derived:phase.details`       | `computePhaseDetails`         | complex    | L824–L1028  | Yes                                    |

**7 of 8** can move to WASM. `derived:diagnostics.summary` stays in TS because it reads the VS Code diagnostics API (a VS Code-only data source).

### StatusService

- **File**: `packages/exosuit-vscode/src/services/StatusService.ts` (L19–L185)
- **What**: Calls `exo status --format json` via CLI, caches result for 5s TTL, invalidates on reactive root changes
- **Consumers**: `ExosuitTreeProvider` (L150–L246) — fetches status, pending intents, progress mode
- **Tests**: Mocked in `ExosuitTreeProvider.test.ts` (L163–L214)
- **Verdict**: Actively used. To be subsumed by derived roots backed by WASM computation.

### RfcStatusService

- **File**: `packages/exosuit-vscode/src/services/RfcStatusService.ts` (L1–L73)
- **Consumers**: **Zero** — no imports found anywhere in the codebase
- **Verdict**: Dead code. Delete immediately.

### MachineChannelServer Interception

- **File**: `packages/exosuit-vscode/src/agent/lmtool/MachineChannelServer.ts`
- **Pattern**: `tryHandleLocally()` (L103–L163) checks if `op.kind === "call"` with a `namespace` address whose first path segment is `"derived"`. Reconstructs `rootId` as `derived:${path.slice(1).join(".")}` (L115–L118). If `derivedRootRegistry.has(rootId)` is true, returns the cached value directly. Otherwise falls through to the `exo json server` subprocess.
- **Impact**: As derived roots become WASM-backed, more requests will be handled locally. Eventually the CLI fallthrough may become unnecessary for most operations.

### Epoch::serialize Stale Writes

- **Stale write**: `Epoch::serialize` writes `derived_status()` to disk at `context.rs` L780: `state.serialize_field("status", self.derived_status())`
- **Disk write**: `AgentContext::save()` writes plan.toml at `context.rs` L951
- **Ignored on read**: `EpochInput` at `context.rs` L719–L724 does not include a `status` field — it's silently dropped on deserialization
- **Derivation**: `Epoch::derived_status()` at `context.rs` L830 computes status from phase states
- **Impact**: The written `status` field is stale between CLI invocations. TS consumers that read plan.toml see stale values. Removing this write is safe because `EpochInput` already ignores it.

### WASM Reactivity Module (exosuit-reactivity)

**File**: `crates/exosuit-reactivity/src/wasm.rs`

Current exports (all `#[wasm_bindgen]` on `WasmEngine`):

| Export                   | Line | Purpose                           |
| ------------------------ | ---- | --------------------------------- |
| `new`                    | L42  | Constructor                       |
| `notify_file_change`     | L52  | File change → invalidation        |
| `validate_root`          | L85  | Check if root is still valid      |
| `get_root_digest`        | L90  | Get root content hash             |
| `remove_root`            | L95  | Unregister a root                 |
| `set_disk_revision`      | L99  | Update disk revision counter      |
| `register_state_root`    | L109 | Register in-memory state root     |
| `bump_state_root`        | L127 | Increment state root revision     |
| `get_state_revision`     | L137 | Read state root revision          |
| `compute_directory_hash` | L152 | Hash directory contents           |
| `register_root`          | L180 | Register file-backed root         |
| `fetch_root`             | L244 | Fetch root (triggers TS callback) |
| `get_root_value`         | L253 | Read root value from storage      |
| `begin_transaction`      | L262 | Start batch update                |
| `end_transaction`        | L266 | End batch update                  |
| `begin_track`            | L271 | Start dependency tracking scope   |
| `record_dependency`      | L282 | Record a dependency               |
| `scope_read`             | L303 | Read root within tracking scope   |
| `end_track`              | L334 | End tracking scope                |
| `get_scope_revision`     | L356 | Get scope revision                |
| `validate_trace`         | L365 | Validate dependency trace         |

**Key for Layer 3**: `scope_read()` (L303) and `get_root_value()` (L253) already exist. Derivation functions can read root data directly from the engine's internal storage, keeping dependency traces implicit.

**What needs to be added**: New WASM exports like `derive_epoch_status()`, `derive_phase_details()`, etc. that use `scope_read()` internally to access root data and return computed JSON.

### Rust Derivation Functions

Functions that compute derived values (candidates for sharing between CLI and WASM):

**context.rs** (plan/epoch/phase):
| Function | Line | Purpose |
|----------|------|---------|
| `find_active_phase()` | L128 | Find the active phase with its epoch context |
| `find_active_phase_id()` | L147 | Get just the active phase ID |
| `find_active_epoch()` | L156 | Find the epoch with `derived_status() == "active"` |
| `find_next_pending_phase()` | L164 | Find next pending phase after an anchor |
| `find_unreviewed_epochs()` | L247 | Find epochs needing review |
| `Epoch::derived_status()` | L830 | Derive epoch status from phase states |
| `Epoch::needs_review()` | L885 | Check if epoch needs review |

**status.rs** (full status computation):
| Function | Line | Purpose |
|----------|------|---------|
| `compute_between_phases_context()` | L109 | Compute context when between phases |
| `build_status_json()` | L167 | Build full status JSON (the `exo status` output) |
| `show_status_human()` | L243 | Human-readable status display |

**world_state.rs** (world state probing):
| Function | Line | Purpose |
|----------|------|---------|
| `WorldState::probe()` | L128 | Probe full world state |
| `compute_epoch_state()` | L206 | Compute epoch boundary state |
| `find_next_phase()` | L247 | Find next phase for transition |
| `build_rfc_pipeline()` | L290 | Build RFC pipeline for a phase |

### Consumer Inventory

**Sidebar/Tree Providers** (read derived roots):

- `ExosuitTreeProvider` — reads `derived:phase.active`, `derived:inbox.summary`, StatusService
- `TreeDataService` — reads derived roots for tree item rendering
- `RfcPipelineProvider` — reads `derived:rfc.pipeline`, `derived:rfc.index`
- `IdeasTreeProvider` — reads `derived:ideas.summary`
- `EpochContextProvider` — reads derived roots for epoch context display

**Status Bars** (read derived roots):

- `InboxStatusBarService` — reads `derived:inbox.summary`
- `PhaseStatusBarService` — reads `derived:phase.active`
- `DiagnosticsStatusBarService` — reads `derived:diagnostics.summary`

**Rich Editor** (reads derived roots):

- `RichEditorProvider` — reads derived roots for editor rendering

**LM Tools / Machine Channel** (read derived roots + CLI fallthrough):

- `MachineChannelServer.tryHandleLocally()` — intercepts derived root requests
- `exo-run.ts` — routes through MachineChannel
- `tool-factory.ts` — zero-arg tools route through MachineChannel
- `locate.ts`, `list.ts` — specific LM tools using MachineChannel

**Mappers**:

- `ImplementationPlanMapper` — reads derived roots for plan mapping
- `ImplementationPlanExecution` — reads derived roots

**Webview**:

- `ConsistencyService.svelte.ts` — reads roots via webview bridge
- `machineChannelBridge.ts` — webview → MachineChannel requests

## Implementation Plan

### Phase 0: Dead Code Removal (parallel with Phase 1)

**Goal**: Remove dead code that would otherwise need to be migrated.

| Task                                               | Files                                   | Verification                         |
| -------------------------------------------------- | --------------------------------------- | ------------------------------------ |
| Delete `RfcStatusService.ts`                       | `services/RfcStatusService.ts`          | No imports break, extension compiles |
| Remove any standalone `rfc-status.wasm` references | Search for `rfc-status` across codebase | Clean build                          |

### Phase 1: WASM Infrastructure (parallel with Phase 0)

**Goal**: Add Layer 3 scaffolding to the WASM module so derivation functions can be added incrementally.

| Task                                                         | Files                                      | Verification                         |
| ------------------------------------------------------------ | ------------------------------------------ | ------------------------------------ |
| Create shared derivation crate (`crates/exosuit-derive/`)    | New crate with plan/epoch/phase types      | `cargo test` passes                  |
| Extract `Epoch::derived_status()` logic into shared crate    | `context.rs` → `exosuit-derive/src/lib.rs` | CLI still works, same behavior       |
| Add first WASM export: `derive_epoch_status()`               | `crates/exosuit-reactivity/src/wasm.rs`    | WASM builds, export callable from TS |
| Wire TS to call WASM for `derived:phase.active` epoch status | `derivedRoots.ts`                          | Sidebar shows correct epoch status   |

### Phase 2: Trivial Derivations

**Goal**: Move the simplest derived roots to WASM.

| Root                    | Complexity | Dependencies                                                   |
| ----------------------- | ---------- | -------------------------------------------------------------- |
| `derived:inbox.summary` | moderate   | Reads `agent.inbox` root (TOML)                                |
| `derived:ideas.summary` | moderate   | Reads `agent.ideas` root (TOML)                                |
| `derived:phase.active`  | moderate   | Reads `agent.plan` + `agent.current.implementation-plan` roots |

Each conversion follows the same pattern:

1. Write Rust derivation function in `exosuit-derive`
2. Add WASM export in `exosuit-reactivity`
3. Replace TS computation in `derivedRoots.ts` with WASM call
4. Verify consumers see identical data

### Phase 3: Moderate Derivations

**Goal**: Move RFC-related derived roots to WASM. These require filesystem access (reading RFC directory contents).

| Root                       | Complexity | Dependencies                               |
| -------------------------- | ---------- | ------------------------------------------ |
| `derived:rfc.index`        | moderate   | Reads RFC files across `docs/rfcs/` stages |
| `derived:rfc.supersession` | moderate   | Reads RFC frontmatter for `superseded_by`  |
| `derived:rfc.pipeline`     | moderate   | Reads `derived:rfc.index` + plan data      |

**Prerequisite**: Resolve how WASM accesses RFC file contents. Options:

- (a) Register per-RFC-file roots (TS materializes, WASM reads via `scope_read()`)
- (b) Register `agent.rfcs.dir` as a directory root with file contents

### Phase 4: Complex Derivations

**Goal**: Move `derived:phase.details` — the most complex derived root — to WASM.

| Root                    | Complexity           | Dependencies                                                              |
| ----------------------- | -------------------- | ------------------------------------------------------------------------- |
| `derived:phase.details` | complex (200+ lines) | Reads plan, implementation plan, parses goals/tasks/strikes/epoch context |

This is the largest single migration. The Rust function needs to replicate the full phase hierarchy construction. Test thoroughly against the existing TS output.

### Phase 5: Subsume StatusService

**Goal**: Replace the CLI-based StatusService with derived roots.

| Task                                              | Files                               | Verification                                     |
| ------------------------------------------------- | ----------------------------------- | ------------------------------------------------ |
| Create `derived:status.full` root backed by WASM  | `derivedRoots.ts`, `exosuit-derive` | Returns same shape as `exo status --format json` |
| Update `ExosuitTreeProvider` to read derived root | `ExosuitTreeProvider.ts`            | Sidebar renders identically                      |
| Remove `StatusService.ts`                         | `services/StatusService.ts`         | No imports break                                 |
| Remove CLI `exo status --format json` dependency  | Verify no remaining callers         | Clean build                                      |

### Phase 6: Stale Write Cleanup

**Goal**: Stop writing derived values to disk.

| Task                                                        | Files                        | Verification                                     |
| ----------------------------------------------------------- | ---------------------------- | ------------------------------------------------ |
| Remove `derived_status()` call from `Epoch::serialize`      | `context.rs` L780            | plan.toml no longer has `status` field on epochs |
| Verify `EpochInput` still works (already ignores `status`)  | `context.rs` L719            | All tests pass                                   |
| Update any TS code that reads `epoch.status` from plan.toml | `derivedRoots.ts`, `Plan.ts` | Consumers use WASM-derived value                 |

**Gate**: All consumers must be migrated to WASM-derived values before this phase. Otherwise removing the stale write breaks consumers that still read it.

### Phase Dependencies

```
Phase 0 ─┐
          ├─→ Phase 2 → Phase 3 → Phase 4 → Phase 5 → Phase 6
Phase 1 ─┘
```

Phases 0 and 1 are parallel (no dependencies). Phases 2–6 are sequential — each builds on the infrastructure and patterns established by the previous phase.

### What Stays in TS

- `derived:diagnostics.summary` — reads VS Code diagnostics API, cannot move to WASM
- `DerivedRootRegistry` — pure plumbing (register, cache, invalidate), stays as-is
- `ReactivityService` — WASM engine host, stays as-is
- All UI consumers — they read derived roots, no logic changes needed

## Future Possibilities

- **Full reactive graph in WASM** — Instead of TS calling WASM for individual derivations, the WASM engine could own the entire derived root graph, including dependency edges between derived values. TS would only observe outputs.
- **Hot-reload derivation logic** — Since derivation is in WASM, rebuilding the WASM module could update computation logic without restarting VS Code.
- **Shared derivation for other consumers** — Web UIs, CI tools, or other editors could load the same WASM module for consistent derived values.
