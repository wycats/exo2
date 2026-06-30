<!-- exo:10165 ulid:01kmzxbcy3gy1qm1e1tz97xq6n -->


# RFC 10165: Reactive SQLite: Virtual Table Integration with Revision Algebra

## Implementation Status

> **As of 2026-06-23:**
>
> | Component                         | Status            | Location                                                                                                       |
> | --------------------------------- | ----------------- | -------------------------------------------------------------------------------------------------------------- |
> | SQLite schema (V001–V010)         | ✅ Complete       | `crates/exosuit-storage/migrations/`                                                                           |
> | Shadow table infrastructure       | ✅ Complete       | `migrations/V002__shadow.sql`                                                                                  |
> | Revision tables                   | ✅ Complete       | `migrations/V003__revisions.sql`, `migrations/V020__reactive_revision_coverage.sql`                            |
> | TraceScope thread-local           | ✅ Complete       | `src/trace.rs`                                                                                                 |
> | ReactiveVTab (xFilter, xColumn)   | ✅ Complete       | `src/vtab/reactive.rs`                                                                                         |
> | ReactiveVTab (xUpdate)            | ✅ Complete       | Writable vtab writes mutate `*_data`, preserve the vtab write contract, persist `*_rev`, and bump `rowset_revisions` |
> | RevisionStore                     | ✅ Complete       | `src/revisions.rs`                                                                                             |
> | content_hash() function           | ✅ Complete       | `src/functions.rs`                                                                                             |
> | Defensive mode + shadow boundary  | ✅ Complete       | `crates/exosuit-storage/src/schema.rs`, `crates/exosuit-storage/src/vtab/shadow.rs`; `xShadowName` and `SQLITE_DBCONFIG_DEFENSIVE` protect `*_data`/`*_rev` from ordinary direct writes |
> | SqliteLoader (read path)          | ✅ Complete       | `tools/exo/src/context/sqlite_loader.rs`                                                                       |
> | TOML → SQLite migration           | ✅ Complete       | `tools/exo/src/command/migrate.rs`                                                                             |
> | StorageBackend::Toml removal      | ✅ Complete       | Variant deleted, all TOML match arms removed                                                                   |
> | TOML cache bridge                 | ✅ **Removed**    | `write_toml_cache()` deleted, SQL dumps replaced it                                                            |
> | Git-friendly serialization        | ✅ Complete       | RFC 10178 fully implemented: `dump_tables()`, `import_tables()`, `exo verify dump`, auto-import on fresh clone |
> | SQL dump parallel write + cutover | ✅ Complete       | `write_sql_dump()` in daemon handler + direct mode                                                             |
> | Stale TOML file deletion          | ✅ Complete       | `plan.toml`, `ideas.toml`, `inbox.toml` removed from git                                                       |
> | Multi-repo migration              | ✅ Complete       | 8 workspaces migrated, all pass `exo verify dump`                                                              |
> | Extension daemon integration      | ✅ Complete       | `PlanService` → `plan.snapshot`, `context.snapshot`                                                            |
> | Trace-backed sidebar freshness    | ✅ Complete       | `context.snapshot`, `context validate-trace`, VS Code `TraceCache`                                             |
> | SqliteWriter reactive writes      | ✅ Complete       | Ordinary Exo state mutations write through reactive table names                                                |
> | xShadowName shim                  | ✅ Complete       | The reactive module patches rusqlite's module struct to advertise `data`/`rev` shadow suffixes                 |
> | **`exosuit-reactivity-core`**     | ✅ Complete       | Shared types crate extracted. Both `exosuit-storage` and `exosuit-reactivity` depend on it.                    |
> | **Persistent rowset revisions**   | ✅ Complete       | `Revision::Counter(u64)` variant, V011 migration drops epoch column, counters persist across restarts          |
> | **Reactive extension roots**      | ✅ Complete       | Sidebar roots are served through daemon-trace-backed `TraceCache` validation and refetch                       |
>
> Focused storage and sidebar freshness coverage now includes file-backed cross-connection trace invalidation, VS Code `TraceCache` write-notification refetch coverage, and defensive-mode coverage for the ordinary shadow-table boundary.
>
> **Related RFCs:**
>
> - RFC 10176 (Project State Model): Defines the data model and schemas
> - RFC 10178 (Git-Friendly Serialization): Sorted SQL text dumps — **fully implemented**
> - RFC 10174 (Hierarchical Intent Queue): Defines inbox behavioral semantics
>
> **Epoch:** SQLite as Source of Truth (Phase: Legacy TOML Code Removal — complete)

## Summary

Replace bespoke TOML flat files (`plan.toml`, `ideas.toml`, `inbox.toml`, etc.) with SQLite as the query/storage engine, using **virtual tables** backed by **shadow tables** to intercept reads and ordinary Exo state writes for organic trace recording per the Revision Algebra (see `docs/specs/algebras/reactivity.md`).

The virtual table layer provides two read interceptors — `xFilter` for **Membership** observations and `xColumn` for **Content** observations — plus a single write interceptor (`xUpdate`). Shadow tables store the actual data, and `xShadowName` plus defensive mode blocks ordinary direct writes to those backing tables.

Git-friendly diffs are preserved via sorted SQL text dumps.

## Motivation

The current system uses ~8600 lines of hand-managed TOML with:

- Manual `find_*` methods for every lookup pattern
- String-based foreign keys with no validation
- Cross-file joins (plan.toml ↔ implementation-plan.toml ↔ RFC markdown files)
- Error-prone derived status computation

## Key Design Decisions

### 1. SQLite for Query Power

Full SQL eliminates hand-coded lookups:

```sql
-- Replace find_active_phase() + manual goal iteration
SELECT g.id, g.label, g.status
FROM goals g
JOIN phases p ON g.phase_id = p.id
WHERE p.status = 'active';

-- Replace DeriveContext cross-file joins
SELECT g.id, g.label, r.stage as rfc_stage
FROM goals g
LEFT JOIN rfcs r ON g.rfc_id = r.number
WHERE g.target_stage IS NOT NULL AND r.stage < g.target_stage;
```

Real foreign keys catch broken references at write time.[^sqlite-foreign-keys]

### 2. Dual-Mediator Read Interception

SQLite virtual tables provide two fundamentally different read interceptors, corresponding to the two observation kinds defined in the core algebra (§1 Desiderata):

| Callback  | Observation Kind | What is Observed             | Trace Entry                                   |
| --------- | ---------------- | ---------------------------- | --------------------------------------------- |
| `xFilter` | **Membership**   | Which rows exist in the scan | `(TableRowSet(T), Membership, rowset_rev(T))` |
| `xColumn` | **Content**      | What a column value is       | `(⟨T, R⟩, Content, row_rev(R))`               |

Not all queries observe both kinds. A `COUNT(*)` only observes Membership (xFilter fires, xColumn does not). A `SELECT col WHERE rowid = ?` observes both.[^sqlite-count]

Both mediators record dependencies automatically inside the virtual table implementation.

### 3. Shadow Table Architecture

Each virtual table stores its data in real SQLite tables (shadow tables):

```
CREATE VIRTUAL TABLE goals USING reactive(...)
  → goals_data       -- real table: stores actual row data
  → goals_revisions  -- real table: stores per-row content digests
```

The virtual table is the **reactive mediator**; shadow tables are the **backing store**. Ordinary Exo state mutations go through the virtual table write path. `xShadowName` declares the relationship so `SQLITE_DBCONFIG_DEFENSIVE` makes shadow tables read-only to ordinary SQL:

- Direct `INSERT INTO goals_data(...)` → blocked by SQLite defensive mode
- Ordinary Exo state writes go through `xUpdate` on the virtual table
- All reads **must** go through `xFilter`/`xColumn` on the virtual table
- Trusted import, projection, and migration paths may write shadow tables only while explicitly managing defensive mode.

### 4. Observation Kinds in Traces

Traces are sets of `(cell_id, revision)` pairs. The observation kind (Membership vs Content) is implicit in the `CellId` and `Revision`:

```rust
struct TraceEntry {
    cell_id: CellId,   // source_id = table, pointer = rowid or ""
    revision: Revision, // Disk { hash } for both content digests and counters
}
```

- **Membership observation**: `CellId::root(table)` (pointer is `""`) with a counter-based `Revision::Disk`
- **Content observation**: `CellId::new(table, rowid)` (pointer is the rowid) with a digest-based `Revision::Disk`

The two observation kinds have different invalidation semantics (Existential Dependency, core algebra §3):

- **Membership change → Content invalidated**: If a row is added/removed, Content observations on that collection's members must revalidate.
- **Content change → Membership unchanged**: If a row's value changes at stable identity, Membership observations are still valid.

### 5. Conservative Write Bumping

Ordinary Exo state mutations flow through `xUpdate`, which applies these revision bumps:

| Operation | Row Revision  | Row-Set Revision |
| --------- | ------------- | ---------------- |
| `INSERT`  | Created (new) | Bumped           |
| `UPDATE`  | Bumped        | **Bumped**       |
| `DELETE`  | Removed       | Bumped           |

UPDATE bumps the Row-Set revision conservatively. This is over-tracking (wasted revalidation, always correct), not under-tracking (staleness). The alternative — leaving Row-Set unchanged on UPDATE — creates a gap: if an UPDATE changes a predicate column, a row may silently enter or leave a query's result set.

Per the core algebra's One-Sided Error property: over-tracking is a cost issue; under-tracking is a correctness bug.

### 5.1 Virtual Table Write Semantics

Virtual-table SQL is Exo's internal writer substrate. Shadow-table SQL is reserved for trusted import, projection, and migration code.

`INSERT` follows the backing table's default semantics:

- omitted columns with backing-table defaults are omitted from the shadow-table `INSERT`, so SQLite applies the default;
- bound `NULL` values are explicit and reach the backing table, so `NOT NULL` constraints apply;
- literal `NULL` in an `INSERT` for a defaulted column is not recoverable as distinct writer intent once SQLite calls `xUpdate`, because the virtual table receives a full row-shaped argument list with no reliable omitted-column marker.

`UPDATE` uses SQLite's no-change signal:

- unchanged columns are omitted from the shadow-table `UPDATE`;
- explicit `NULL` updates are preserved and reach the backing table;
- backing constraints remain the authority for rejecting invalid values.

If Exo needs literal-`NULL` INSERT intent for defaulted columns, that intent must be represented before the write is lowered to SQLite virtual-table SQL.

### 6. Revision Scheme

All revisions in the storage layer must survive process restarts. The `Revision::Memory` variant is not used here; it exists for the WASM reactivity engine's in-memory state.

**Row revisions** are **content digests** (BLAKE3 hashes of row bytes), mapped to `Revision::Disk { hash }`.

- **Survives process restarts**: The hash is derived from the data, so it is reconstructable without any stored counter.
- **Detects no-op UPDATEs**: `UPDATE goals SET status = 'active' WHERE status = 'active'` doesn't change the digest, so dependents don't revalidate.
- **Identity-Equivalence separation** (core algebra §1): Identity is the rowid. Equivalence is the digest.
- **Trade-off**: O(row_size) per write to compute the digest, vs O(1) for a counter. Acceptable for our dataset sizes (hundreds of rows, not millions).

**Row-Set revisions** are **persistent monotonic counters**, mapped to `Revision::Disk { hash }` (encoding the counter as a string).

- **Survives process restarts**: The counter is stored in the `rowset_revisions` table and loaded on startup — not reset.
- **Bumped on every membership-affecting mutation**: INSERT, UPDATE, DELETE all increment the counter (see §5 Conservative Write Bumping).
- **No epoch**: There is no process-scoped UUID. The counter is the sole revision. Two processes that read the same counter value agree on the revision.
- **No-op detection is not needed**: Reactivity is observation-based and discrete. If no one observed the value between two mutations, there is no output to invalidate. A monotonic counter that bumps on every mutation is correct.

The `rowset_revisions` table schema:

```sql
CREATE TABLE rowset_revisions (
    table_name TEXT PRIMARY KEY,
    counter INTEGER NOT NULL DEFAULT 0
);
```

### 7. Git-Friendly Serialization

> **Implemented in RFC 10178.** Sorted SQL text dumps are the git-committed representation of workspace state. See [RFC 10178](../stage-1/10178-git-friendly-serialization-sorted-sql-text-dumps.md) for the format specification.
>
> Key properties: lossless round-trip (verified on 2,485 rows), deterministic output, single-line-per-entity (git diff friendly). Auto-import on fresh clone (`git clone → exo status` just works).

### 8. Shared Types: `exosuit-reactivity-core` (Implemented)

> **Status**: Fully implemented. The `exosuit-reactivity-core` crate is extracted and both `exosuit-storage` and `exosuit-reactivity` depend on it.

The reactive loop crosses three compilation targets: the daemon (native), the WASM reactivity engine (`wasm32-unknown-unknown`), and the JSON machine channel between them. All three need the same trace vocabulary.

The `exosuit-reactivity-core` crate contains the shared types and validation logic. Dependencies: `serde`, `serde_json`, `uuid`.

**What lives in `exosuit-reactivity-core`:**

- `CellId { source_id: String, pointer: String }` — cell identity
- `Epoch(Uuid)` — epoch identifier (used by the WASM engine for in-memory state, not by the storage layer)
- `Revision` enum: `Memory { epoch, counter }`, `Disk { hash }`, `Impure { hash, nonce }`
- `Trace { dependencies: BTreeSet<TraceEntry>, resources: Vec<ResourceSpec> }`
- `TraceEntry { cell_id: CellId, revision: Revision }`
- `Trace::validate(&self, state: &mut impl StateProvider) -> bool`
- `StateProvider` trait

**What stays in `exosuit-reactivity` (not core):**

- `TraceDigest` extension trait (`Trace::digest()`) — uses `sha2`, `hex`, `bincode` for Merkle hashing
- `Engine`, `Runtime`, WASM bindings, snapshot management

**Dependency graph:**

- `exosuit-storage` depends on `exosuit-reactivity-core` — `TraceScope` records into `core::Trace` via convenience constructors (`row_cell_id`, `table_membership_cell_id`, `digest_revision`, `counter_revision`)
- `exosuit-reactivity` depends on `exosuit-reactivity-core` — re-exports all core types, adds `TraceDigest`
- `tools/exo` (daemon) depends on `exosuit-storage` — will serialize `core::Trace` in snapshot responses

**Storage layer type mapping** (implemented):

| SQLite concept   | Core type                                                              |
| ---------------- | ---------------------------------------------------------------------- |
| Row content      | `CellId::new(table, rowid.to_string())` + `Revision::Disk { hash }`    |
| Table membership | `CellId::root(table)` + `Revision::Disk { hash: counter.to_string() }` |

The storage layer uses only `Revision::Disk`. `Revision::Memory` is not used — all SQLite revisions are persistent (see §6). `SqliteStateProvider` implements `core::StateProvider` for trace validation against the `RevisionStore`.

### 9. Reactive Extension Roots (Implemented)

The extension sidebar freshness path now uses daemon-trace-backed roots instead of TOML materializers or a rebuilt `DaemonRootService`.

#### Current Architecture

- The storage layer records organic dependencies with `TraceScope` while `SqliteLoader` reads through reactive virtual table names.
- The daemon returns root data through commands such as `context.snapshot`; the machine-channel `ResponseEnvelope.trace` carries the captured trace, and `context validate-trace` lets long-lived clients ask whether a cached root is still fresh.
- VS Code `TraceCache` owns sidebar root caching. After write notifications or write/exec responses, it validates cached traces through the daemon and refetches roots whose traces are stale.
- The WASM reactivity engine remains available as shared/future machinery, but it is not the active sidebar resync path.

#### Remaining Completion Bar

The reactivity layer reaches the intended durability/freshness bar as these hardening items close:

- **Shadow boundary — complete**: `xShadowName` makes `SQLITE_DBCONFIG_DEFENSIVE` reject ordinary writes to `*_data` and `*_rev` shadow tables.
- **Write semantics — complete**: `INSERT` default handling, bound `NULL`, literal-`NULL` INSERT boundaries, and `UPDATE` no-change semantics are tested and documented.
- **Revision metadata cleanup — complete**: normal writes update the affected row revisions and rowset counters directly. Deletes, `REPLACE` writes, and cascading deletes remove stale `*_rev` rows without rebuilding live-row digests, and startup/backfill remains the safety sweep for orphan revision metadata.
- **Physical SQLite maintenance — current**: new file-backed Exo databases enable incremental auto-vacuum before migrations, and explicit storage maintenance reports page/freelist/WAL state, reclaims a bounded number of freelist pages, and can opt an existing database into incremental auto-vacuum with a `VACUUM` rebuild.
- **Predicate precision**: predicate-aware invalidation remains a refinement path for reducing false-positive sidebar refreshes without weakening freshness.

#### Persisted Revision Maintenance

Reactive revision tables are derived metadata. The correctness contract is:

- row digests track the current content of rows observed through the virtual table;
- rowset counters track table membership and may bump conservatively;
- deleted rows do not keep valid-looking `*_rev` entries;
- cascading deletes either clear affected child-table revision rows eagerly or invalidate the child table through a conservative rowset bump;
- database open/backfill sweeps repair orphan revision rows left by trusted import, projection, migration, or interrupted maintenance.

The normal daemon/sidebar write path therefore avoids full-table digest rebuilds. It refreshes the written row, clears orphan revision rows for tables whose membership may have changed, and relies on persistent rowset counters to keep long-lived trace validation sound.

#### Physical SQLite Maintenance

SQLite file maintenance is a separate concern from revision metadata cleanup.
`*_rev` rows are derived freshness metadata; SQLite freelist pages and WAL pages
are physical storage debt in the main database file and its journal.

The physical maintenance contract is:

- newly-created file-backed Exo databases enable `auto_vacuum = INCREMENTAL`
  before migrations create tables, so later maintenance can reclaim free pages
  without a full-file rebuild;
- existing databases keep their current file layout until an explicit
  maintenance invocation enables incremental auto-vacuum and runs `VACUUM`;
- a maintenance pass reports `page_size`, `page_count`, `freelist_count`,
  reclaimable bytes, current auto-vacuum mode, and WAL checkpoint results when
  available;
- incremental databases reclaim freelist pages through a bounded loop of
  `PRAGMA incremental_vacuum(1)`, making compaction an explicit maintenance
  operation rather than work hidden inside ordinary writes;
- WAL checkpointing is reported as storage maintenance state and may run as
  part of the explicit maintenance path when the connection can complete it.

This preserves the reactive algebra while keeping long-lived Exo databases from
accumulating unbounded physical storage debt. Logical writes maintain trace
freshness immediately; explicit maintenance handles SQLite file compaction and
checkpoint visibility.

## Technology Evaluation

### Storage Engine: SQLite is Non-Negotiable

The algebra's **Organic Consumption** axiom requires that reading data automatically records a dependency — code must execute when the query engine fetches rows (`xFilter`) and column values (`xColumn`).

SQLite's virtual table API is the only embeddable option providing user-code interception of individual column reads within a query engine:

| Candidate          | Queries         | Embeddable | Rust       | WASM        | Read Intercept                       | Verdict                                                |
| ------------------ | --------------- | ---------- | ---------- | ----------- | ------------------------------------ | ------------------------------------------------------ |
| **DuckDB**         | SQL             | Yes        | Yes        | No `wasip1` | vtab (batch-level, not column-level) | WASM kill — ~40MB binary, no `wasm32-wasip1` target    |
| **CozoDB**         | Datalog         | Yes        | Native     | Browser     | **None**                             | No read interception; dormant (last release Dec 2023)  |
| **redb**           | **None** (KV)   | Yes        | Native     | Untested    | **None**                             | No query engine at all                                 |
| **Irmin**          | **None** (path) | Yes        | **OCaml**  | Partial     | **None**                             | Content-addressed diffs but wrong language, no queries |
| **Automerge**      | **None** (doc)  | Yes        | Rust+WASM  | Yes         | **None**                             | CRDT diffs but no tables, no queries                   |
| **cr-sqlite**      | SQL             | Yes        | Via SQLite | Browser     | **None** (write tracking only)       | It _is_ SQLite — an extension, not an alternative      |
| **libSQL** (Turso) | SQL             | Yes        | Yes        | Browser     | Same as SQLite (vtab)                | Fork of SQLite — same vtab API, not a different answer |

DuckDB comes closest with table functions, but those return entire Arrow RecordBatch results — you cannot distinguish "I read column A" from "I read column B" for a given row. The content-addressable stores (Irmin, Automerge) provide native diffability but lack structured queries and read interception.

**Conclusion**: Virtual tables are the only technology that satisfies all requirements: read interception (Organic Consumption), write interception (Mediated Access), structured queries (SQL), embeddable, WASM-compatible, and Rust-native.

### Client Crate: rusqlite

Three Rust crates can _author_ SQLite virtual tables (as opposed to merely querying SQLite):

| Crate                           | vtab Authoring                  | xShadowName                         | Custom Functions | WASM    | Status                             |
| ------------------------------- | ------------------------------- | ----------------------------------- | ---------------- | ------- | ---------------------------------- |
| **rusqlite** (v0.38)            | Yes: `VTab` → `TransactionVTab` | **No** (sets `iVersion: 1`)         | Yes              | Yes     | Very active                        |
| **sqlite3_ext** (v0.1.3)        | Yes + `SHADOW_NAMES` const      | **Yes** (auto-wires, `iVersion: 3`) | Yes              | Unclear | Dormant (~3 years)                 |
| **sqlite-loadable-rs** (v0.0.5) | Yes                             | **No**                              | Yes              | **No**  | Eliminated (can't compile to WASM) |

All other SQLite crates (`sqlx`, `diesel`, `sea-orm`, `libsql`) are _consumers_ — they send queries and get results, with zero support for extending SQLite.

rusqlite's `iVersion: 1` means the `xShadowName` field in `ffi::sqlite3_module` is zero-initialized. This is a configuration knob rusqlite doesn't expose because most vtab authors don't need shadow table protection. The fix is a trivial struct patch (see §L0 below).

sqlite3_ext is the only crate that natively supports xShadowName, but it's dormant (v0.1.3, ~3 years, unclear WASM story). The gap is ~20 lines of FFI.

**Conclusion**: rusqlite with a 20-line xShadowName shim via `libsqlite3-sys` FFI.

### SQLite Mechanisms: What We Use and What We Don't

SQLite provides many native mechanisms beyond virtual tables. We investigated them against the algebra's requirements:

**What we use (four mechanisms working together):**

| Mechanism                                               | Purpose                                                    | Algebra Concept                      |
| ------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------ |
| Virtual table callbacks (`xFilter`/`xColumn`/`xUpdate`) | Read interception + write entry point                      | Organic Consumption, Mediated Access |
| Shadow tables (`_data`, `_rev`)                         | Real SQLite storage protected from ordinary direct writes by `xShadowName` and defensive mode | Backing store                     |
| Custom scalar function (`content_hash()`)               | Content digest computation via `sqlite3_create_function()` | $\mathcal{R}_{disk}$ (row revision)  |
| `PRAGMA data_version`                                   | Cross-process invalidation detection                       | External change notification         |

**What we don't use (and why):**

| Mechanism                     | Why Not                                                                                                                            |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| **Session Extension**         | Explicitly excludes virtual tables: "There is no support for virtual tables." Designed for replication, not reactive invalidation. |
| **`sqlite3_preupdate_hook`**  | Fires on _real_ tables only (would fire on shadow writes). Redundant for ordinary Exo state writes, which are routed through `xUpdate`. |
| **`sqlite3_update_hook`**     | Post-write hook on real tables. Same redundancy as preupdate_hook.                                                                 |
| **Triggers on shadow tables** | Redundant for ordinary Exo state writes. Adding triggers would create a parallel notification path with no benefit.               |

SQLite has **no read-side hooks** at all (no `select_hook`, no `read_hook`, no trigger on SELECT). This confirms virtual tables as the only option for Organic Consumption.

## Schema

### Primary Key Policy

All shadow `_data` tables use **`INTEGER PRIMARY KEY`** (rowid alias). This is required by the algebra: Cell identity is $\langle \text{Table}, \text{Rowid} \rangle$, and rowid stability across VACUUM is only guaranteed when aliased by `INTEGER PRIMARY KEY`.[^sqlite-rowid][^sqlite-vacuum]

Human-readable identifiers are stored in a `text_id TEXT NOT NULL UNIQUE` column, indexed for fast lookup but not used as the row identity. This cleanly separates **identity** (rowid — stable, used by the reactive layer) from **equivalence** (content digest — used for change detection) from **naming** (text_id — used by humans and the CLI).

Every `_data` table has a companion `_rev` table storing content digests keyed by rowid.

### ID Conventions

| Entity Type      | `text_id` Format           | Example                |
| ---------------- | -------------------------- | ---------------------- |
| Epoch/Phase/Goal | ULID (new) or legacy slug  | `01hz3k...`, `my-goal` |
| Task             | ULID (new) or legacy slug  | `01hz4m...`            |
| Inbox            | `intent-<ulid>`            | `intent-01j5...`       |
| Idea             | UUID v4                    | `550e8400-...`         |
| Feedback thread  | `fb-<uuid>`                | `fb-a1b2c3...`         |
| Feedback message | `msg-<uuid>`               | `msg-d4e5f6...`        |
| RFC              | Zero-padded numeric string | `00228`                |
| Decision         | Slug                       | `sqlite-over-toml`     |

Legacy IDs, slugs, and aliases are stored in the `entity_aliases` table for backwards-compatible resolution (matching the existing `matches_id` resolution chain: canonical ref → ULID → slug → primary id → aliases).

### Shadow Table DDL

```sql
-- ═══════════════════════════════════════════════════════════
-- Revision tracking (one per data table)
-- ═══════════════════════════════════════════════════════════

-- Pattern: every _data table has a companion _rev table
-- CREATE TABLE <entity>_rev (
--     rowid INTEGER PRIMARY KEY,       -- matches _data rowid
--     digest BLOB NOT NULL             -- content hash of row bytes
-- );

-- ═══════════════════════════════════════════════════════════
-- Core plan hierarchy: epoch → phase → goal → task
-- ═══════════════════════════════════════════════════════════

CREATE TABLE epochs_data (
    id    INTEGER PRIMARY KEY,          -- rowid alias, stable identity
    text_id TEXT NOT NULL UNIQUE,       -- ULID or legacy id
    title   TEXT NOT NULL,
    slug    TEXT,
    reviewed INTEGER NOT NULL DEFAULT 0 -- boolean
);
CREATE TABLE epochs_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

CREATE TABLE phases_data (
    id       INTEGER PRIMARY KEY,
    text_id  TEXT NOT NULL UNIQUE,
    title    TEXT NOT NULL,
    status   TEXT NOT NULL DEFAULT 'pending',
    epoch_id INTEGER NOT NULL REFERENCES epochs_data(id),
    kind     TEXT NOT NULL DEFAULT 'regular',  -- 'regular' | 'chore'
    slug     TEXT
);
CREATE TABLE phases_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

CREATE TABLE goals_data (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    label          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending',
    phase_id       INTEGER NOT NULL REFERENCES phases_data(id),
    kind           TEXT DEFAULT 'regular',
    rfc            TEXT,                        -- RFC number (text), nullable
    target_stage   INTEGER,                     -- target RFC stage for promotion
    started_at     TEXT,                        -- RFC 3339 datetime
    description    TEXT,
    completion_log TEXT,
    slug           TEXT
);
CREATE TABLE goals_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

CREATE TABLE tasks_data (
    id             INTEGER PRIMARY KEY,
    text_id        TEXT NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending',
    goal_id        INTEGER NOT NULL REFERENCES goals_data(id),
    completed_at   TEXT,                        -- RFC 3339 datetime
    completion_log TEXT,
    tdd_status     TEXT,                        -- 'red' | 'green' | null
    test_file      TEXT,                        -- path to test file for TDD
    slug           TEXT
);
CREATE TABLE tasks_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Phase ↔ RFC associations
-- ═══════════════════════════════════════════════════════════

CREATE TABLE phase_rfcs_data (
    id       INTEGER PRIMARY KEY,
    phase_id INTEGER NOT NULL REFERENCES phases_data(id),
    rfc_id   TEXT NOT NULL,                    -- RFC number (text)
    target   INTEGER,                          -- target stage for promotion
    UNIQUE(phase_id, rfc_id)
);
CREATE TABLE phase_rfcs_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Ideas
-- ═══════════════════════════════════════════════════════════

CREATE TABLE ideas_data (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,           -- UUID v4
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending',
    created_at  TEXT NOT NULL,                  -- RFC 3339
    source      TEXT NOT NULL DEFAULT '',
    slug        TEXT
);
CREATE TABLE ideas_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- Junction: idea tags (replaces TOML array)
CREATE TABLE idea_tags (
    idea_id INTEGER NOT NULL REFERENCES ideas_data(id),
    tag     TEXT NOT NULL,
    PRIMARY KEY (idea_id, tag)
);

-- Junction: idea → related entity refs (replaces TOML array)
CREATE TABLE idea_related (
    idea_id INTEGER NOT NULL REFERENCES ideas_data(id),
    ref     TEXT NOT NULL,                     -- e.g. 'rfc:00228', 'goal:my-goal'
    PRIMARY KEY (idea_id, ref)
);

-- ═══════════════════════════════════════════════════════════
-- Inbox
-- ═══════════════════════════════════════════════════════════

CREATE TABLE inbox_data (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,           -- 'intent-<ulid>'
    created     TEXT NOT NULL,                  -- RFC 3339
    status      TEXT NOT NULL DEFAULT 'pending',-- pending|acknowledged|resolved|archived
    category    TEXT NOT NULL,                  -- correction|guidance|question|priority
    subject     TEXT NOT NULL,
    subject_ref TEXT,                           -- 'goal:X', 'task:X', 'phase:X', 'rfc:X'
    body        TEXT NOT NULL DEFAULT '',
    scope_kind  TEXT NOT NULL DEFAULT 'global', -- global|phase|file|rust|typescript
    scope_value TEXT,                           -- phase id or file path when scoped
    urgency     TEXT NOT NULL DEFAULT 'next-touch',
    action_kind TEXT,                           -- complete-goal|complete-task|verify-task|add-note
    action_data TEXT,                           -- JSON for action-specific fields (evidence, note)
    updated     TEXT,                           -- RFC 3339
    resolution  TEXT
);
CREATE TABLE inbox_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Feedback
-- ═══════════════════════════════════════════════════════════

CREATE TABLE feedback_threads_data (
    id           INTEGER PRIMARY KEY,
    text_id      TEXT NOT NULL UNIQUE,          -- 'fb-<uuid>'
    target_file  TEXT NOT NULL,
    target_id    TEXT NOT NULL,
    target_field TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'open',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE TABLE feedback_threads_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

CREATE TABLE feedback_messages_data (
    id        INTEGER PRIMARY KEY,
    text_id   TEXT NOT NULL UNIQUE,             -- 'msg-<uuid>'
    thread_id INTEGER NOT NULL REFERENCES feedback_threads_data(id),
    author    TEXT NOT NULL,
    content   TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE feedback_messages_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Decisions
-- ═══════════════════════════════════════════════════════════

CREATE TABLE decisions_data (
    id           INTEGER PRIMARY KEY,
    text_id      TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active',
    date         TEXT NOT NULL,
    context      TEXT NOT NULL DEFAULT '',
    decision     TEXT NOT NULL DEFAULT '',
    consequences TEXT                           -- JSON: { "pros": [...], "cons": [...] }
);
CREATE TABLE decisions_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Implementation plan (active phase snapshot)
-- ═══════════════════════════════════════════════════════════

CREATE TABLE acceptance_criteria_data (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,
    phase_id    INTEGER NOT NULL REFERENCES phases_data(id),
    description TEXT NOT NULL,
    satisfied   INTEGER NOT NULL DEFAULT 0,     -- boolean
    notes       TEXT
);
CREATE TABLE acceptance_criteria_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);

-- ═══════════════════════════════════════════════════════════
-- Cross-cutting: alias resolution
-- ═══════════════════════════════════════════════════════════

CREATE TABLE entity_aliases (
    entity_type TEXT NOT NULL,                 -- 'epoch', 'phase', 'goal', 'task', ...
    entity_id   INTEGER NOT NULL,              -- rowid in the corresponding _data table
    alias       TEXT NOT NULL,
    PRIMARY KEY (entity_type, alias)
);
CREATE INDEX idx_aliases_entity ON entity_aliases(entity_type, entity_id);
```

### Virtual Table Declarations

```sql
CREATE VIRTUAL TABLE epochs            USING reactive(epochs_data);
CREATE VIRTUAL TABLE phases            USING reactive(phases_data);
CREATE VIRTUAL TABLE goals             USING reactive(goals_data);
CREATE VIRTUAL TABLE tasks             USING reactive(tasks_data);
CREATE VIRTUAL TABLE phase_rfcs        USING reactive(phase_rfcs_data);
CREATE VIRTUAL TABLE ideas             USING reactive(ideas_data);
CREATE VIRTUAL TABLE inbox             USING reactive(inbox_data);
CREATE VIRTUAL TABLE feedback_threads  USING reactive(feedback_threads_data);
CREATE VIRTUAL TABLE feedback_messages USING reactive(feedback_messages_data);
CREATE VIRTUAL TABLE decisions         USING reactive(decisions_data);
CREATE VIRTUAL TABLE acceptance_criteria USING reactive(acceptance_criteria_data);
```

Queries target virtual tables, which delegate to shadow tables while recording traces. Junction tables (`idea_tags`, `idea_related`, `entity_aliases`) are plain tables — they don't need reactive tracking because they're denormalized indexes, not primary data sources.

### Full Table Set

| Virtual Table         | Shadow Tables                                         | Source TOML             |
| --------------------- | ----------------------------------------------------- | ----------------------- |
| `epochs`              | `epochs_data`, `epochs_rev`                           | canonical project state |
| `phases`              | `phases_data`, `phases_rev`                           | canonical project state |
| `goals`               | `goals_data`, `goals_rev`                             | canonical project state |
| `tasks`               | `tasks_data`, `tasks_rev`                             | canonical task state    |
| `phase_rfcs`          | `phase_rfcs_data`, `phase_rfcs_rev`                   | canonical project state |
| `ideas`               | `ideas_data`, `ideas_rev`                             | `ideas.toml`            |
| `inbox`               | `inbox_data`, `inbox_rev`                             | `inbox.toml`            |
| `feedback_threads`    | `feedback_threads_data`, `feedback_threads_rev`       | `feedback.toml`         |
| `feedback_messages`   | `feedback_messages_data`, `feedback_messages_rev`     | `feedback.toml`         |
| `decisions`           | `decisions_data`, `decisions_rev`                     | `decisions.toml`        |
| `acceptance_criteria` | `acceptance_criteria_data`, `acceptance_criteria_rev` | canonical task state    |

Each virtual table also maintains a row-set revision (persistent monotonic counter) for Membership tracking.

## Alignment with Core Algebra

| Core Algebra Concept (v5.0)         | SQLite Implementation                                                 |
| ----------------------------------- | --------------------------------------------------------------------- |
| Cell Identity `⟨Source, Pointer⟩`   | `CellId::new(table, rowid)` — rowid aliased by `INTEGER PRIMARY KEY`  |
| Collection Cell                     | `CellId::root(table)` — pointer is `""`, synthetic cell per table     |
| Observation Kind: Content           | `xColumn` → records `(CellId::new(T,R), Revision::Disk { digest })`   |
| Observation Kind: Membership        | `xFilter` → records `(CellId::root(T), Revision::Disk { counter })`   |
| Trace `T = {(c₁,r₁), ..., (cₙ,rₙ)}` | `BTreeSet<TraceEntry>` where `TraceEntry { cell_id, revision }`       |
| Organic Consumption                 | Side-effect of `xColumn` and `xFilter` — no app code changes          |
| Mediated Access                     | `xFilter`/`xColumn`/`xUpdate` mediate the command path; `xShadowName` and defensive mode protect shadow tables from ordinary direct writes |
| Existential Dependency              | Membership invalid → Content entries on that collection revalidate    |
| Write Bumping (conservative)        | Vtab DML bumps both row revision and Row-Set revision                 |
| Zero-Execution Validation           | `trace.validate(&mut provider)` compares revision values only         |
| One-Sided Error                     | Conservative UPDATE = over-tracking (wasted work, never stale)        |
| Adaptive Granularity                | Row-level default; normalize table for finer grain                    |
| Content Digest                      | Row revision = hash(row_bytes); Row-Set revision = persistent counter |
| Rowid Stability                     | All `_data` tables use `INTEGER PRIMARY KEY` (rowid alias)            |
| Identity / Equivalence Separation   | rowid = identity, content digest = equivalence, text_id = naming      |

## The Rust Stack

### Architectural Principle: Reactivity at the Bottom

**Constraint**: Reactivity must live at the SQLite virtual table layer, not in application code.

**Reasoning (Abstraction Independence, per SQLite algebra §1):**

1. SQLite executes queries against virtual tables by invoking `xFilter` (to select rows) and `xColumn` (to fetch values), regardless of the abstraction that produced the SQL.[^sqlite-vtab-api]
2. `xFilter` records a Membership observation. `xColumn` records a Content observation. Together, every SQL observation produces a trace entry with the appropriate kind.
3. Any higher-level abstraction (ORM, query builder, raw SQL) that compiles to SQL against a virtual table inherits the same trace behavior.
4. Therefore: reactivity is the bottom layer. Everything above it is a free choice.

```
┌─────────────────────────────────────────────────────────────┐
│  Application Layer                                          │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  CLI Commands / VS Code Extension / Agent Tools     │    │
│  └─────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────┘
                            │ uses
┌───────────────────────────▼─────────────────────────────────┐
│  Query Layer (L2)                                           │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  ORM (optional) / Query Builder / Prepared SQL      │    │
│  │  ─────────────────────────────────────────────────  │    │
│  │  Abstractions are FREE — they compile to SQL,       │    │
│  │  virtual tables intercept all reads automatically   │    │
│  └─────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles to SQL
┌───────────────────────────▼─────────────────────────────────┐
│  Migration Layer (L1)                                       │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Versioned .sql files / refinery / manual runner    │    │
│  │  Standard schema evolution with confidence          │    │
│  └─────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────┘
                            │ runs against
┌───────────────────────────▼─────────────────────────────────┐
│  Reactive SQLite (L0) ← REACTIVITY LIVES HERE              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  rusqlite + Virtual Table Extension + Shadow Tables │    │
│  │  ─────────────────────────────────────────────────  │    │
│  │  • xFilter → Record(Membership)                     │    │
│  │  • xColumn → Record(Content)                        │    │
│  │  • xUpdate → bump revisions (all DML bumps RowSet) │    │
│  │  • xShadowName → direct-shadow protection           │    │
│  └─────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────┘
                            │ persists to
┌───────────────────────────▼─────────────────────────────────┐
│  Git-Friendly Serialization                                 │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Sorted SQL statements, one INSERT per line         │    │
│  │  .sql files tracked in git with meaningful diffs    │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### Layer Breakdown

#### L0: Reactive SQLite (rusqlite + Virtual Tables + Shadow Tables)

```rust
/// The core abstraction.
pub struct ReactiveDb {
    conn: rusqlite::Connection,
    revision_store: RevisionStore,
}

/// Revision tracking for reactive virtual tables.
pub struct RevisionStore {
    /// Row revisions: content digest (BLAKE3 hash of row bytes).
    row_cache: HashMap<(TableName, Rowid), [u8; 32]>,
    /// Row-Set revisions: persistent monotonic counter per table.
    /// Loaded from `rowset_revisions` table on startup, not reset.
    rowset_cache: HashMap<TableName, u64>,
}

/// Virtual table implementation — the reactive mediator.
impl VTab for ReactiveVTab {
    /// Content observation: record what a column value is.
    fn column(&mut self, ctx: &mut Context, col: i32) -> Result<()> {
        let rowid = self.cursor.rowid();
        let digest = self.store.get_row_digest(&self.table, rowid);

        // Record (row cell, content digest) into the current trace
        TraceScope::record(
            row_cell_id(&self.table, rowid),
            digest_revision(digest),
        );

        // Read from shadow table and return
        ctx.set_result(&self.shadow.get_value(rowid, col))
    }

    /// Membership observation: record which rows exist.
    fn filter(&mut self, idx_num: i32, idx_str: &str, args: &[Value]) -> Result<()> {
        let counter = self.store.get_rowset_counter(&self.table);

        // Record (table membership cell, persistent counter) into the current trace
        TraceScope::record(
            table_membership_cell_id(&self.table),
            Revision::disk(counter.to_string()),
        );

        // Query shadow table with constraints
        self.shadow.apply_filter(idx_num, idx_str, args)?;
        Ok(())
    }

    /// Mutation: all DML bumps both row revision and Row-Set revision.
    fn update(&mut self, rowid: Option<i64>, values: &[Value]) -> Result<i64> {
        match (rowid, values.is_empty()) {
            (None, false) => {
                // INSERT: create row in shadow table, compute digest, bump Row-Set
                let new_rowid = self.shadow.insert(values)?;
                let digest = self.shadow.compute_digest(new_rowid);
                self.store.set_row_digest(&self.table, new_rowid, digest);
                self.store.bump_rowset_revision(&self.table);
                Ok(new_rowid)
            }
            (Some(id), false) => {
                // UPDATE: update shadow table, recompute digest, bump Row-Set
                self.shadow.update(id, values)?;
                let digest = self.shadow.compute_digest(id);
                self.store.set_row_digest(&self.table, id, digest);
                // Conservative: UPDATE bumps Row-Set too
                self.store.bump_rowset_revision(&self.table);
                Ok(id)
            }
            (Some(id), true) => {
                // DELETE: remove from shadow table, remove digest, bump Row-Set
                self.shadow.delete(id)?;
                self.store.remove_row_digest(&self.table, id);
                self.store.bump_rowset_revision(&self.table);
                Ok(id)
            }
        }
    }

    /// Shadow table declaration: enforces Mediated Access.
    fn shadow_name(name: &str) -> bool {
        // "data" and "revisions" suffixes are our shadow tables
        matches!(name, "data" | "rev")
    }
}

/// Thread-local trace recording.
pub struct TraceScope;

impl TraceScope {
    pub fn run<F, R>(f: F) -> (R, Trace)
    where F: FnOnce() -> R
    { /* installs thread-local scope, collects TraceEntry items, returns Trace */ }

    pub fn record(cell_id: CellId, revision: Revision) {
        /* inserts TraceEntry { cell_id, revision } into current scope */
    }
}

/// Validation: O(|trace|) revision comparisons. No re-execution.
/// Uses core::StateProvider trait — RevisionStore implements it.
impl StateProvider for RevisionStore {
    fn get_revision(&mut self, cell_id: &CellId) -> Option<Revision> {
        if cell_id.pointer.is_empty() {
            // Membership cell → persistent counter
            let counter = self.get_rowset_counter(&cell_id.source_id);
            Some(Revision::disk(counter.to_string()))
        } else {
            // Content cell → row digest
            let rowid = cell_id.pointer.parse().ok()?;
            let digest = self.get_row_digest(&cell_id.source_id, rowid)?;
            Some(Revision::disk(hex::encode(digest)))
        }
    }
}
```

**Dependencies**:

- [`rusqlite`](https://github.com/rusqlite/rusqlite) v0.38+ with `bundled`, `vtab`, and `functions` features[^rusqlite][^rusqlite-bundled]
- [`libsqlite3-sys`](https://github.com/rusqlite/rusqlite/tree/master/libsqlite3-sys) with `bundled` feature — required for raw FFI calls in vtab cursor methods (see RefCell note above)
- [`blake3`](https://github.com/BLAKE3-team/BLAKE3) for content digest computation

**xShadowName shim**: rusqlite sets `iVersion: 1` on all module structs and zero-initializes `xShadowName`. Since `Module<'vtab, T>` is `#[repr(transparent)]` over `ffi::sqlite3_module`, Exo patches the struct at registration time:

```rust
use rusqlite::ffi;
use std::ffi::{c_char, c_int, CStr};

/// Shadow table suffixes owned by our virtual table.
const SHADOW_NAMES: &[&str] = &["data", "rev"];

/// SQLite xShadowName callback — returns 1 if name is a recognized suffix.
unsafe extern "C" fn x_shadow_name(name: *const c_char) -> c_int {
    let Ok(s) = CStr::from_ptr(name).to_str() else { return 0 };
    SHADOW_NAMES.contains(&s) as c_int
}

/// Wrap a rusqlite module: copy struct, set iVersion=3, wire xShadowName.
pub fn with_shadow_names<'vtab, T: rusqlite::vtab::VTab<'vtab>>(
    base: &'static rusqlite::vtab::Module<'vtab, T>,
) -> &'static rusqlite::vtab::Module<'vtab, T> {
    let base_raw = base as *const _ as *const ffi::sqlite3_module;
    let mut patched: ffi::sqlite3_module = unsafe { *base_raw };
    patched.iVersion = 3;
    patched.xShadowName = Some(x_shadow_name);
    let leaked = Box::leak(Box::new(patched));
    unsafe { &*(leaked as *const _ as *const rusqlite::vtab::Module<'vtab, T>) }
}
```

Intended usage:

```rust
let module = rusqlite::vtab::update_module_with_tx::<ReactiveVTab>();
let module = with_shadow_names(module);
conn.create_module("reactive", module, None)?;
```

**Connection access in cursor methods**: SQLite's virtual table API passes the database connection to `xCreate`/`xConnect` but _not_ to cursor methods like `xFilter` or `xColumn`. Since the cursor must query the shadow table, the vtab struct must store a connection reference.

> **⚠️ Implementation Note (discovered during implementation):**
>
> rusqlite's `Connection` uses an internal `RefCell` for its statement cache. When SQLite invokes vtab callbacks (`xFilter`, `xColumn`), the connection is already borrowed by SQLite's vtab machinery. Calling `conn.prepare()` from within these callbacks causes a **RefCell borrow conflict** panic:
>
> ```
> thread 'main' panicked at 'already borrowed: BorrowMutError'
> ```
>
> **Solution**: Use raw SQLite FFI (`libsqlite3-sys`) to query shadow tables from within vtab callbacks. Store `*mut ffi::sqlite3` (the raw handle) instead of `*mut Connection`, and use `sqlite3_prepare_v2`/`sqlite3_step`/`sqlite3_column_*` directly. This bypasses rusqlite's RefCell entirely.
>
> The `Connection` pointer is still useful for schema queries during `xConnect` (before vtab callbacks are invoked), so we store both.

```rust
use libsqlite3_sys as ffi;

pub struct ReactiveVTab {
    table_name: String,
    shadow_table: String,
    columns: Vec<String>,
    /// Raw sqlite3 handle for cursor queries (bypasses rusqlite's RefCell).
    /// Safety: Valid for vtab lifetime (tied to connection that created it).
    db: *mut ffi::sqlite3,
}

// In connect():
fn connect(db: &mut VTabConnection, aux: Option<&Self::Aux>, ...) -> Result<(String, Self)> {
    let vtab_aux = aux.ok_or(...)?;
    Ok((schema, ReactiveVTab { ..., db: vtab_aux.db }))
}

// In cursor filter() - use raw FFI to avoid RefCell conflict:
fn filter(&mut self, ...) -> Result<()> {
    let sql = CString::new(format!("SELECT rowid, * FROM {}", self.vtab.shadow_table))?;
    let mut stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();
    unsafe {
        ffi::sqlite3_prepare_v2(self.vtab.db, sql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        // ... sqlite3_step, sqlite3_column_* to fetch rows ...
        ffi::sqlite3_finalize(stmt);
    }
        .query_map(...)?;
    // ...
}
```

The connection pointer is passed via `Aux` data at module registration:

```rust
let conn_ptr = conn as *const Connection as *mut Connection;
conn.create_module("reactive", module, Some(conn_ptr))?;
```

**Content hash function**: Row content digests are computed via a custom SQL scalar function registered at connection init, following the pattern established by SQLite's own `ext/misc/sha1.c`:

```rust
conn.create_scalar_function(
    "content_hash",
    -1,  // variadic
    FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS,
    |ctx| {
        // Hash all argument values (column bytes) into a content digest.
        // Algorithm choice (BLAKE3, xxHash, etc.) is an internal detail —
        // SQLITE_DETERMINISTIC ensures the query planner can optimize.
        let digest = compute_digest(ctx.args());
        ctx.set_result(digest)
    },
)?;
```

The `SQLITE_DETERMINISTIC` flag is critical — it tells SQLite the function always returns the same result for the same inputs, enabling query planner optimizations.

**Defensive mode**: At connection initialization:

```rust
conn.execute_batch("PRAGMA trusted_schema = OFF;")?;
conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
```

This activates SQLite's enforcement of shadow table protection.

#### L1: Migrations

Use standard SQL migration tools. Options:

| Tool       | Approach                    | Notes                             |
| ---------- | --------------------------- | --------------------------------- |
| `refinery` | SQL files, lightweight      | Pure Rust, no ORM coupling        |
| Manual     | Versioned `.sql` files      | Simple, full control              |
| `sqlx`     | `migrate!` macro, SQL files | Async, but supports blocking mode |

**Recommendation**: `refinery` for minimal dependency surface, or versioned SQL files for full control.

#### L2: Query Layer

Because reactivity lives at L0, the query layer is a free choice. All queries compile to SQL, hit virtual tables, and get traced automatically (Abstraction Independence).

Options:

| Tool        | Approach            | WASM? | Notes                                     |
| ----------- | ------------------- | ----- | ----------------------------------------- |
| Raw SQL     | Prepared statements | ✓     | Maximum control, no abstraction cost      |
| `sea-query` | Query builder       | ✓     | Type-safe SQL construction, no ORM        |
| `diesel`    | ORM                 | ✓\*   | Uses `rusqlite` directly, but opinionated |

\*Diesel's WASM support requires careful configuration.

**Recommendation**: Start with raw SQL or `sea-query`. Add richer abstractions as patterns emerge.

### WASM Compilation

The compilation target for the SQLite module is **`wasm32-wasip1`** (Rust tier 2). WASI provides the POSIX-like I/O layer that SQLite and `rusqlite` expect.[^rust-wasi-target]

`rusqlite` explicitly supports this via its `wasm32-wasi-vfs` feature.[^rusqlite-wasi]

> **Two-target strategy**: The existing crates (`exosuit-reactivity`, `exosuit-file-refs`, `exosuit-ulid`) compile to `wasm32-unknown-unknown` via `wasm-bindgen` and continue to do so. The new SQLite module targets `wasm32-wasip1` because it needs filesystem access. The two targets coexist — they are separate crates with separate compilation pipelines.

#### WASI Availability

VS Code provides native WASI support via the [`vscode-wasm`][vscode-wasm-gh] package from Microsoft. This gives WASM modules a virtual filesystem layer that maps WASI `fd_read`/`fd_write` calls to real workspace files.

| Host                | WASI Provider                               | Notes                                          |
| ------------------- | ------------------------------------------- | ---------------------------------------------- |
| **Desktop VS Code** | `vscode-wasm` WASI support[^vscode-wasm]    | Native WASI integration, maps to workspace fs  |
| **Web VS Code**     | `vscode-wasm` WASI shims[^vscode-wasm]      | Same API; VFS coverage may differ from desktop |
| **CLI (`exo`)**     | N/A — runs native `rusqlite` with `bundled` | No WASM involved                               |

[vscode-wasm-gh]: https://github.com/nicolo-ribaudo/vscode-wasm

#### What Compiles

- **SQLite**: `rusqlite` with `bundled` + `wasm32-wasi-vfs` compiles to `wasm32-wasip1`.[^rusqlite-wasi]
- **Virtual Tables**: Pure Rust, compiled alongside SQLite into the same WASM module.
- **Query Builders**: Pure Rust (`sea-query`) — no platform dependencies.
- **Migrations**: Pure Rust SQL runners — same compilation path.

#### Architecture

```
┌─────────────────────────────────────────────────┐
│            Host (VS Code Extension)             │
│  ┌─────────────────────────────────────────┐    │
│  │  vscode-wasm WASI Layer                 │    │
│  │  Maps virtual FS paths → workspace files│    │
│  └─────────────────────────────────────────┘    │
└───────────────────────┬─────────────────────────┘
                        │ WASI fd_read/fd_write
┌───────────────────────▼─────────────────────────┐
│      SQLite WASM Module (wasm32-wasip1)         │
│  ┌─────────────────────────────────────────┐    │
│  │  L0: ReactiveDb (rusqlite + vtables)    │    │
│  │  L1: Migrations                          │    │
│  │  L2: Query Layer                         │    │
│  └─────────────────────────────────────────┘    │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│  Existing WASM Modules (wasm32-unknown-unknown) │
│  exosuit-reactivity, exosuit-file-refs,         │
│  exosuit-ulid — via wasm-bindgen (unchanged)    │
└─────────────────────────────────────────────────┘
```

#### CLI vs WASM

The CLI runs natively with normal file I/O. The VS Code extension runs as WASM with WASI-mediated I/O. **The core logic (L0-L2) is identical** — WASI provides `std::fs` to the WASM module, so no `#[cfg]` branching is needed for the data layer.

## Pre-Migration Cleanup

Before migrating to SQLite, remove or simplify half-baked systems that would be painful to migrate.

### Systems to Remove

| System                    | Current State                                               | Migration Cost                                                 | Action                                        |
| ------------------------- | ----------------------------------------------------------- | -------------------------------------------------------------- | --------------------------------------------- |
| **TDD workflow**          | RFC 0092 demoted to Stage 0; tools removed from VS Code     | Would require `tdd_status`, `tests`, `tdd.active_task` columns | **Remove** schema remnants from TOML          |
| **TDD runners**           | RFC 0129 skeleton at Stage 3                                | Never implemented; would add complexity                        | **Demote** to Stage 0                         |
| **Walkthrough system**    | Deprecated hybrid (RFC 0132 → 0136)                         | Unclear schema requirements                                    | **Remove** if unused                          |
| **Legacy task-list.toml** | Superseded by implementation-plan.toml                      | Parallel structure to migrate                                  | **Delete** if present                         |
| **`satisfies` links**     | Goal→RFC relationship                                       | May be redundant with `phase_rfcs`                             | **Verify** usage, simplify if possible        |
| **Acceptance criteria**   | Defined in RFCs (0064, 10028) but never implemented in code | Would add tables with no exercised code paths                  | **Demote** to Stage 0; revisit post-migration |

### RFC Cluster Consolidation

Several RFC clusters must be consolidated before migration because they anchor authority in TOML files that this RFC removes.

#### Data Model Cluster (Priority 1)

These RFCs establish "where data lives" axioms that conflict with SQLite-first storage:

| RFC                                       | Stage | Conflict                                                                               |
| ----------------------------------------- | ----- | -------------------------------------------------------------------------------------- |
| **00177** (Goals/Tasks Unified Model)     | 1     | Anchors goal metadata in `plan.toml`, tasks in `implementation-plan.toml` as ephemeral |
| **10120** (Implementation Plan Canonical) | 1     | Makes `implementation-plan.toml` the "canonical execution artifact"                    |
| **00229** (Goal Status Authority)         | 1     | Declares `plan.toml` the "single authoritative source" for goal status                 |
| **0064/10028** (Phase State Machine)      | 2/1   | Defines acceptance criteria schema for TOML files                                      |

**Resolution**: Consolidate into a storage-agnostic "Project State Model" RFC. The authority rules (goal metadata vs execution details, status authority vs derived signals) remain valid but must reference SQLite tables/views, not TOML files.

#### Reactivity Cluster (Priority 2)

| RFC                                    | Stage | Status                          |
| -------------------------------------- | ----- | ------------------------------- |
| **0026** (Validation-Based Reactivity) | 4     | Keep — core algebra             |
| **10143**                              | 3     | **Deleted** — duplicate of 0026 |
| **0118** (Reactive Collections)        | 4     | Keep — collection algebra       |
| **10146**                              | 3     | **Deleted** — duplicate of 0118 |
| **0119** (Reactive File System)        | 4     | Keep — file algebra             |
| **10147**                              | 3     | **Deleted** — duplicate of 0119 |
| **10164** (Principled Invalidation)    | 0     | Verify alignment with this RFC  |

**Observation Kinds**: Observation kind (membership vs content) is implicit in the `CellId` — root cells (`pointer: ""`) are membership, row cells (`pointer: rowid`) are content (see §4). The conservative write bumping strategy (§5) means all DML bumps both row and row-set revisions. Precision refinement is deferred.

#### CLI Architecture Cluster (Priority 3)

| RFC                                     | Stage | Conflict                                            |
| --------------------------------------- | ----- | --------------------------------------------------- |
| **00223** (CLI Namespace Consolidation) | 1     | Assumes dual writes to legacy plan/task projections |
| **10163** (LM Tool Surface Reduction)   | 2     | References TOML-based command contracts             |

**Resolution**: Update CLI RFCs to target SQLite as canonical store. The dual-write pattern becomes a single transaction spanning both logical tables.

### Codebase Cleanup

The CLI is hardcoded to TOML files at multiple levels:

| File         | Hardcoding Level                                              | Migration Difficulty |
| ------------ | ------------------------------------------------------------- | -------------------- |
| `context.rs` | Surface (paths) + Deep (serde model)                          | Medium               |
| `phase.rs`   | Surface (paths) + Deep (TOML schema emission)                 | Medium               |
| `task.rs`    | Deep (pervasive `toml_edit` mutations, structure assumptions) | High                 |
| `goal.rs`    | Surface (dual-write pattern)                                  | Medium               |
| `strike.rs`  | Surface (dual-write pattern)                                  | Medium               |

**Approach**: Introduce a storage abstraction layer. Surface-level path changes are straightforward; deep changes in `task.rs` require a domain-level API that hides TOML/SQLite differences.

### Cleanup Checklist

1. **TDD schema remnants**: Remove `tdd_status`, `tests`, and `tdd.active_task` fields from `implementation-plan.toml` schema
2. **RFC 0129**: Demote from Stage 3 to Stage 0 (TDD runners never implemented)
3. **Acceptance criteria**: Demote RFC 0064 to Stage 0; remove from 10028 if present
4. **Walkthrough**: Audit `docs/agent-context/` for walkthrough references; remove if deprecated
5. **task-list.toml**: Delete if present (superseded by implementation-plan.toml)
6. **satisfies/criteria**: Audit actual usage in plan.toml and implementation-plan.toml; simplify or remove unused fields
7. **Migration duplicates**: Delete lower-stage duplicates (0023, 10143, 10146, 10147) — **Done**
8. **Authority RFCs**: Consolidate 00177/10120/00229 into storage-agnostic model

### Deferred Ideas

Systems demoted during cleanup that should be revisited post-migration:

| System              | RFC        | Rationale for Deferral                                     |
| ------------------- | ---------- | ---------------------------------------------------------- |
| TDD workflow        | 0092       | Needs stable storage layer first                           |
| TDD runners         | 0129       | Never implemented; design may change                       |
| Acceptance criteria | 0064       | Good idea, but no code exists; implement on SQLite         |
| Observation kinds   | (this RFC) | Conservative invalidation works; precision is optimization |

### Rationale

Removing these systems before migration reduces the schema surface and eliminates untested code paths from the migration scope. We migrate what we actually use, not what we once planned to use.

## Migration Path

### Phase 1: Schema & Import

1. **Define shadow table schemas** matching current TOML structure. Each entity type gets a `_data` table (storing values) and a `_rev` table (storing content digests).
2. **Build TOML → SQL importer** (one-time migration tool):
   - Read each `.toml` file
   - Map TOML keys to SQL columns (document the ID mapping: ULID ↔ slug ↔ legacy ID)
   - Validate foreign keys during import (fail loudly on broken references)
   - Import order: epochs → phases → goals → tasks → phase_rfcs → ideas → inbox (respects FK dependencies)
   - Generate initial content digests for all rows
3. **Produce sorted SQL dump** and verify round-trip: TOML → SQLite → SQL dump → SQLite → identical state.

### Phase 2: Virtual Table Layer

4. **Implement the `reactive` virtual table module** in Rust:
   - `xCreate`/`xConnect`: register shadow tables, call `sqlite3_declare_vtab`
   - `xShadowName`: return true for `"data"` and `"rev"` suffixes
   - `xColumn`: read from `_data` shadow table, record Content trace
   - `xFilter`: query `_data` shadow table, record Membership trace
   - `xUpdate`: write to `_data`, compute digest, update `_rev`, bump Row-Set counter
   - Enable `SQLITE_DBCONFIG_DEFENSIVE` on connection init
5. **Wire TraceScope**: thread-local trace collection with `(cell, kind, revision)` triples.
6. **Implement `is_trace_valid()`**: iterate trace entries, compare revisions.

### Phase 3: Query Migration

7. **Replace `find_*` methods** with SQL queries against virtual tables, one module at a time.
8. **Add sorted-SQL serializer** for git-friendly output.
9. **Wire file watcher** to reload backing store on external changes.

### Phase 4: Verification

10. **Round-trip test**: Load from SQL dump → query all entities → serialize → diff against original.
11. **Trace correctness test**: Execute queries, verify trace contains expected observation kinds.
12. **Rollback plan**: Keep TOML loader alongside SQL loader until migration is validated. Both produce identical `DeriveContext` output or the migration is wrong.

## Open Questions

- **RFC data**: RFCs live as markdown files with frontmatter. Should they be imported into SQLite or remain file-based with a join view?

### Resolved

- ~~**Aliases**~~: Resolved — `entity_aliases` table with `(entity_type, entity_id, alias)`. See Schema section.
- ~~**Implementation-plan.toml**~~: Resolved — tasks and acceptance criteria are first-class tables, not a separate file. Active phase identified by `phases_data.status = 'active'`.
- ~~**Shadow table naming**~~: Resolved — `{vtable}_data` and `{vtable}_rev`. Consistent convention for `xShadowName`.
- ~~**Primary key policy**~~: Resolved — `INTEGER PRIMARY KEY` (rowid alias) + `text_id TEXT UNIQUE`. See Schema section.
- ~~**Digest algorithm**~~: Resolved — registered as a custom SQL scalar function (`content_hash()`) via `sqlite3_create_function()` with `SQLITE_DETERMINISTIC` flag. Specific hash algorithm (BLAKE3, xxHash, etc.) is an implementation detail — dataset is small enough that choice barely matters. The mechanism is what needed deciding, not the algorithm.
- ~~**Storage engine**~~: Resolved — SQLite is non-negotiable. No other embeddable database provides column-level read interception. See Technology Evaluation section.
- ~~**Client crate**~~: Resolved — rusqlite with a 20-line xShadowName shim. Alternatives evaluated and eliminated. See Technology Evaluation section.

## Prior Art

- **RFC 10176**: Project State Model — defines the storage-agnostic entity hierarchy (epochs, phases, goals, tasks) that this RFC implements in SQLite
- Formal Algebra: `docs/specs/algebras/reactive-sqlite.md` (v2.1 — dual-mediator, observation kinds, enforced Mediated Access, content digest revision model)
- Core Algebra: `docs/specs/algebras/reactivity.md` (v5.0 — desiderata, observation kinds, Existential Dependency)
- Research: `docs/research/git-friendly-database-comparison.md`
- Research: `docs/research/sqlite-reactive-vtable-sketch.md`
- Inventory: `docs/research/sqlite-migration-inventory.md`
- File Algebra: `docs/specs/algebras/reactive-filesystem.md`
- Collection Algebra: `docs/specs/algebras/reactive-collections.md`

## Appendix: Open Considerations

- **Complete — revision refresh cost**: Normal virtual-table writes refresh affected row digests, clear stale revision rows for deletes/replace/cascades, and preserve conservative rowset invalidation without full-table digest rebuilds on the daemon/sidebar path.
- **Refinement — predicate-aware validators**: The conservative default is sound but coarse. Predicate-aware invalidation is a follow-up precision improvement that may reduce false-positive sidebar refreshes without weakening freshness.
- **Deferred — Web VS Code WASI VFS coverage**: `vscode-wasm` WASI shims may not cover all POSIX operations SQLite's default VFS uses (`flock`, `mmap`). The current sidebar path is daemon-trace-backed on desktop; web coverage needs a separate spike.
- **Non-blocking — query planner column elision**: Beyond `count(*)`, other optimizations may skip `xColumn`. All known cases still depend on row-set membership captured by `xFilter`. Per One-Sided Error, undiscovered elision causes over-invalidation, not staleness.
- **Non-blocking under current model — reactive transactions**: Current daemon access is monotonic for the traced state paths. If concurrent access is introduced later, use the SQLite algebra appendix's branching revision model.

## Footnotes

[^sqlite-vtab]: https://www.sqlite.org/vtab.html

[^sqlite-vtab-api]: https://www.sqlite.org/vtab.html#the_virtual_table_method_table

[^sqlite-rowid]: https://www.sqlite.org/rowidtable.html

[^sqlite-vacuum]: https://www.sqlite.org/lang_vacuum.html

[^sqlite-count]: https://www.sqlite.org/lang_select.html#the_count_aggregate_function

[^sqlite-foreign-keys]: https://www.sqlite.org/foreignkeys.html

[^sqlite-vfs]: https://www.sqlite.org/vfs.html

[^rusqlite]: https://docs.rs/rusqlite/latest/rusqlite/

[^rusqlite-bundled]: https://docs.rs/rusqlite/latest/rusqlite/#bundled

[^rust-wasi-target]: https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip1.html

[^rusqlite-wasi]: https://docs.rs/crate/rusqlite/latest/features (`wasm32-wasi-vfs` feature)

[^vscode-wasm]: https://github.com/microsoft/vscode-wasm
