# Git-Friendly Database Solutions for Rust Projects

**Research Date**: 2026-02-10  
**Context**: Replacing bespoke TOML flat files (~8600 lines) with a database solution that remains git-friendly.

---

## Executive Summary

After evaluating 15+ solutions across 6 categories, the **top recommendations** are:

| Rank | Solution                                | Fit Score  | Best For                                        |
| ---- | --------------------------------------- | ---------- | ----------------------------------------------- |
| 1    | **Automerge + Custom Schema Layer**     | ⭐⭐⭐⭐⭐ | Full solution with merge conflict elimination   |
| 2    | **salsa + TOML** (keep current storage) | ⭐⭐⭐⭐½  | Reactive queries without storage migration      |
| 3    | **SQLite + git-sqlite**                 | ⭐⭐⭐⭐   | Maximum query power, acceptable diff quality    |
| 4    | **SurrealDB Embedded**                  | ⭐⭐⭐½    | Graph + document hybrid, but binary storage     |
| 5    | **Custom petgraph + serde**             | ⭐⭐⭐½    | Full control, significant implementation effort |

---

## Requirements Recap

| Requirement                 | Weight   | Notes                                      |
| --------------------------- | -------- | ------------------------------------------ |
| Git-friendly storage        | Critical | Text-based, meaningful line diffs          |
| Foreign key / relationships | High     | Epoch→Phase→Goal hierarchy, RFC references |
| Rust support                | Critical | Native or high-quality bindings            |
| Query capabilities          | High     | Filter, traverse, aggregate                |
| Reactivity foundation       | Medium   | Change detection for incremental updates   |
| Schema evolution            | Medium   | Version-aware parsing, migrations          |

---

## Category 1: Git-Native Databases

### Dolt

**What it is**: MySQL-compatible database with git-like version control built-in.

| Aspect               | Assessment                                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------------ |
| **Git friendliness** | ⚠️ Parallel to git, not integrated. Has its own branching/merging. Stores as proprietary format. |
| **Diff quality**     | ✅ Excellent semantic diffs (`dolt diff` shows row-level changes)                                |
| **FK support**       | ✅ Full SQL foreign keys                                                                         |
| **Rust support**     | ⚠️ MySQL client libraries work, no native Rust driver                                            |
| **Query**            | ✅ Full SQL                                                                                      |
| **Reactivity**       | ❌ No built-in change detection                                                                  |
| **Schema evolution** | ✅ Standard SQL migrations                                                                       |
| **Maintenance**      | ✅ Well-funded (DoltHub), active development                                                     |

**Verdict**: Good for data versioning but **doesn't integrate with git**—you'd have two VCS systems to manage.

### TerminusDB

**What it is**: Graph database with git-like semantics, stores as JSON-LD.

| Aspect               | Assessment                                                                |
| -------------------- | ------------------------------------------------------------------------- |
| **Git friendliness** | ⚠️ Has git-like ops but stores binary. Push/pull to TerminusHub, not git. |
| **Diff quality**     | ✅ Semantic graph diffs                                                   |
| **FK support**       | ✅ First-class graph relationships                                        |
| **Rust support**     | ❌ HTTP API only, no Rust client                                          |
| **Query**            | ✅ WOQL (graph query language)                                            |
| **Reactivity**       | ❌ No                                                                     |
| **Schema evolution** | ✅ Schema as data, evolvable                                              |
| **Maintenance**      | ⚠️ Smaller community                                                      |

**Verdict**: Wrong paradigm—it's a hosted graph DB, not a git-integrated file format.

### git-sqlite / sqlite-diffable

**What it is**: Tools to make SQLite work better with git via text dumps or custom diff drivers.

| Aspect               | Assessment                                                        |
| -------------------- | ----------------------------------------------------------------- |
| **Git friendliness** | ✅ `sqlite-diffable` stores as sorted SQL statements; clean diffs |
| **Diff quality**     | ✅ Line-per-row if properly formatted                             |
| **FK support**       | ✅ Full SQL FKs                                                   |
| **Rust support**     | ✅ `rusqlite` is excellent                                        |
| **Query**            | ✅ Full SQL                                                       |
| **Reactivity**       | ⚠️ Need to layer on top (file watch + query)                      |
| **Schema evolution** | ✅ SQL migrations                                                 |
| **Maintenance**      | ⚠️ sqlite-diffable is small project; rusqlite is very stable      |

**Verdict**: **Strong contender**. Trade-off: slightly more complex tooling setup, but full SQL power.

**Example diff with sqlite-diffable**:

```diff
 INSERT INTO goals VALUES('goal-1','Fix RFC promote',1,'completed');
-INSERT INTO goals VALUES('goal-2','Add axiom support',2,'pending');
+INSERT INTO goals VALUES('goal-2','Add axiom support',2,'in-progress');
 INSERT INTO goals VALUES('goal-3','Schema migration',3,'pending');
```

---

## Category 2: Embedded Document Stores

### SurrealDB (Embedded Mode)

**What it is**: Multi-model database (document, graph, relational) with embedded and server modes.

| Aspect               | Assessment                                              |
| -------------------- | ------------------------------------------------------- |
| **Git friendliness** | ❌ Binary RocksDB storage in embedded mode              |
| **Diff quality**     | ❌ Binary blobs, no meaningful diffs                    |
| **FK support**       | ✅ Excellent—record links, graph edges, typed relations |
| **Rust support**     | ✅ Native Rust, developed in Rust                       |
| **Query**            | ✅ SurrealQL is powerful (SQL-like + graph traversal)   |
| **Reactivity**       | ✅ LIVE queries built-in                                |
| **Schema evolution** | ⚠️ Flexible schema, but migration tooling immature      |
| **Maintenance**      | ✅ Well-funded, active development                      |

**Verdict**: Feature-perfect except for the **critical git-friendliness requirement failure**.

### redb

**What it is**: Pure Rust embedded key-value store, ACID-compliant.

| Aspect               | Assessment                                   |
| -------------------- | -------------------------------------------- |
| **Git friendliness** | ❌ Binary file format                        |
| **Diff quality**     | ❌ No                                        |
| **FK support**       | ❌ Manual—it's key-value only                |
| **Rust support**     | ✅ Pure Rust, excellent ergonomics           |
| **Query**            | ❌ Key-value only, no query language         |
| **Reactivity**       | ❌ No                                        |
| **Schema evolution** | ⚠️ Manual versioning                         |
| **Maintenance**      | ✅ cberner (author) is responsive, 2k+ stars |

**Verdict**: Wrong tool for the job—too low-level, no git friendliness.

### sled

**What it is**: Embedded database aiming for high performance, ordered key-value.

| Aspect               | Assessment                                 |
| -------------------- | ------------------------------------------ |
| **Git friendliness** | ❌ Binary log-structured storage           |
| **Diff quality**     | ❌ No                                      |
| **FK support**       | ❌ Manual                                  |
| **Rust support**     | ✅ Pure Rust                               |
| **Query**            | ⚠️ Key-prefix scans, no query language     |
| **Reactivity**       | ✅ `watch_prefix` for change notifications |
| **Schema evolution** | ⚠️ Manual                                  |
| **Maintenance**      | ⚠️ Development has slowed significantly    |

**Verdict**: Has reactivity but fails git requirement. Also, maintenance concerns.

---

## Category 3: Graph Databases

### Neo4j (with Cypher)

| Aspect               | Assessment                                 |
| -------------------- | ------------------------------------------ |
| **Git friendliness** | ❌ Server-based, binary storage            |
| **FK support**       | ✅ First-class relationships               |
| **Rust support**     | ⚠️ `neo4rs` async driver exists            |
| **Query**            | ✅ Cypher is excellent for graph traversal |

**Verdict**: Overkill for this use case, not git-friendly.

### petgraph (Rust library)

**What it is**: Graph data structure library for Rust.

| Aspect               | Assessment                                            |
| -------------------- | ----------------------------------------------------- |
| **Git friendliness** | ✅ If you serialize to text (JSON/TOML)               |
| **Diff quality**     | ⚠️ Depends on serialization format                    |
| **FK support**       | ✅ Edges ARE relationships                            |
| **Rust support**     | ✅ Native, mature, widely used                        |
| **Query**            | ⚠️ Algorithms (BFS, DFS, Dijkstra) not query language |
| **Reactivity**       | ❌ No—it's a data structure                           |
| **Schema evolution** | ❌ Manual with serde                                  |
| **Maintenance**      | ✅ Part of rust-unofficial, very stable               |

**Verdict**: **Good building block** but requires significant work to build query/reactivity layers.

---

## Category 4: Relational Text Formats

### Recfile (GNU recutils)

**What it is**: Plain text database format with `recsel` query tool.

| Aspect               | Assessment                                            |
| -------------------- | ----------------------------------------------------- |
| **Git friendliness** | ✅ Excellent—text records, line-based                 |
| **Diff quality**     | ✅ Minimal, meaningful diffs                          |
| **FK support**       | ⚠️ `%rec: Goal` with `%key: id` and manual references |
| **Rust support**     | ⚠️ `recutils` crate exists but minimal                |
| **Query**            | ⚠️ `recsel` command-line, limited in-process          |
| **Reactivity**       | ❌ No                                                 |
| **Schema evolution** | ⚠️ Flexible but no version markers                    |
| **Maintenance**      | ⚠️ GNU project, stable but slow-moving                |

**Example**:

```rec
%rec: Goal
%key: id

id: goal-1
label: Fix RFC promotion
status: completed
phase: phase-14

id: goal-2
label: Add axiom support
status: in-progress
phase: phase-14
```

**Verdict**: Great diff quality but **Rust support is weak** and no reactivity story.

### NDJSON (Newline-Delimited JSON)

**What it is**: One JSON object per line.

| Aspect               | Assessment                                 |
| -------------------- | ------------------------------------------ |
| **Git friendliness** | ✅ Append-only friendly, line = record     |
| **Diff quality**     | ✅ Single-line changes for single records  |
| **FK support**       | ❌ Manual via ID fields                    |
| **Rust support**     | ✅ `serde_json`                            |
| **Query**            | ❌ Manual iteration or external tools (jq) |
| **Reactivity**       | ❌ No                                      |
| **Schema evolution** | ⚠️ JSON schema, manual versioning          |

**Verdict**: Too primitive—loses TOML's readability without gaining query power.

---

## Category 5: CRDT / Event-Sourced Approaches

### Automerge (Rust)

**What it is**: CRDT library for conflict-free collaborative editing.

| Aspect               | Assessment                                                                            |
| -------------------- | ------------------------------------------------------------------------------------- |
| **Git friendliness** | ⚠️ Binary format OR JSON export for snapshots                                         |
| **Diff quality**     | ✅ With JSON snapshots: meaningful diffs. Native format: merge without conflicts ever |
| **FK support**       | ⚠️ References by ID, no built-in validation                                           |
| **Rust support**     | ✅ `automerge` crate is first-class (it's written in Rust)                            |
| **Query**            | ⚠️ Document traversal, no query language                                              |
| **Reactivity**       | ✅ Built-in—`Patch` describes exactly what changed                                    |
| **Schema evolution** | ⚠️ Flexible documents, no schema enforcement                                          |
| **Maintenance**      | ✅ Ink & Switch backing, very active                                                  |

**Key insight**: Automerge's killer feature is **merge conflict elimination**. Two branches can make concurrent edits and merge cleanly. For git-checked files with multiple contributors (or AI agents), this is valuable.

**Hybrid approach**:

```
# Storage strategy
docs/agent-context/plan.automerge  # Binary, CRDT ops
docs/agent-context/plan.json       # JSON snapshot (auto-generated, gitignored or tracked)
```

Or: Store only the JSON snapshot, use automerge for in-memory operations.

**Verdict**: **Top contender** if paired with JSON snapshots for diffing. Unique merge guarantees.

### yrs (Yjs Rust port)

**What it is**: Rust port of Yjs CRDT library, used by collaborative editors.

| Aspect               | Assessment                                              |
| -------------------- | ------------------------------------------------------- |
| **Git friendliness** | ⚠️ Similar to Automerge—binary updates, can export JSON |
| **Diff quality**     | Same as Automerge                                       |
| **FK support**       | ❌ Designed for rich text, not structured data          |
| **Rust support**     | ✅ Pure Rust                                            |
| **Query**            | ❌ No                                                   |
| **Reactivity**       | ✅ Built-in change observation                          |
| **Schema evolution** | ❌ Schema-less                                          |
| **Maintenance**      | ✅ Active, used by AppFlowy                             |

**Verdict**: Better suited for collaborative text editing than structured data.

### Event Sourcing (custom)

**What it is**: Store append-only events, derive current state.

| Aspect               | Assessment                                 |
| -------------------- | ------------------------------------------ |
| **Git friendliness** | ✅ Event log is append-only, perfect diffs |
| **Diff quality**     | ✅ Each commit adds events, never modifies |
| **FK support**       | ⚠️ Encode in event schema                  |
| **Rust support**     | ✅ Build with serde                        |
| **Query**            | ⚠️ Replay or maintain materialized views   |
| **Reactivity**       | ✅ Events ARE changes                      |
| **Schema evolution** | ⚠️ Event versioning patterns exist         |

**Example**:

```toml
# events.toml (append-only)
[[events]]
id = "evt-001"
timestamp = "2026-02-10T12:00:00Z"
type = "goal.created"
[events.payload]
goal_id = "goal-1"
label = "Fix RFC promotion"

[[events]]
id = "evt-002"
timestamp = "2026-02-10T14:30:00Z"
type = "goal.status_changed"
[events.payload]
goal_id = "goal-1"
new_status = "completed"
```

**Verdict**: **Excellent git ergonomics** but requires building replay/projection system.

---

## Category 6: Custom Rust Solutions

### salsa (Incremental Computation)

**What it is**: Framework for incremental computation, inspired by rustc's query system.

| Aspect               | Assessment                                      |
| -------------------- | ----------------------------------------------- |
| **Git friendliness** | N/A—it's a computation layer, not storage       |
| **Diff quality**     | N/A                                             |
| **FK support**       | ✅ Model relationships in query functions       |
| **Rust support**     | ✅ Pure Rust, production-proven (rust-analyzer) |
| **Query**            | ✅ Memoized query functions                     |
| **Reactivity**       | ✅ Core purpose—invalidation & recomputation    |
| **Schema evolution** | N/A                                             |
| **Maintenance**      | ✅ Used by rust-analyzer, very stable           |

**Key insight**: salsa is the **reactivity solution**, not the storage solution. Use it on TOP of any storage format.

**Example architecture**:

```rust
#[salsa::input]
struct PlanFile {
    #[return_ref]
    content: String,
}

#[salsa::tracked]
fn parsed_epochs(db: &dyn Db, file: PlanFile) -> Vec<Epoch> {
    // Parse TOML, cached until content changes
}

#[salsa::tracked]
fn find_active_phase(db: &dyn Db, file: PlanFile) -> Option<Phase> {
    parsed_epochs(db, file)
        .iter()
        .flat_map(|e| &e.phases)
        .find(|p| p.status == "active")
        .cloned()
}
```

**Verdict**: **Highly recommended as the reactive query layer**, regardless of storage choice.

### hecs / bevy_ecs (Entity-Component-System)

**What it is**: ECS libraries for game-style entity management.

| Aspect               | Assessment                                          |
| -------------------- | --------------------------------------------------- |
| **Git friendliness** | ⚠️ Need to serialize World to text                  |
| **FK support**       | ⚠️ Entity IDs as references, no built-in validation |
| **Rust support**     | ✅ Excellent                                        |
| **Query**            | ✅ Very fast component queries                      |
| **Reactivity**       | ⚠️ Change detection in bevy_ecs, not hecs           |
| **Schema evolution** | ❌ Component changes break serialization            |

**Verdict**: Wrong paradigm—ECS optimizes for iteration over many entities with same components, not relationship traversal.

### rusqlite + Custom Text Layer

**What it is**: Use SQLite in-memory, serialize to text format for git.

```rust
// On load: parse plan.toml → insert into SQLite
// Queries: use SQL with rusqlite
// On save: serialize back to TOML with preserved formatting
```

| Aspect               | Assessment                             |
| -------------------- | -------------------------------------- |
| **Git friendliness** | ✅ Keep current TOML format            |
| **Query**            | ✅ Full SQL power in-memory            |
| **Reactivity**       | ⚠️ Manual or layer salsa on top        |
| **Complexity**       | ⚠️ Two representations to keep in sync |

**Verdict**: Pragmatic but adds complexity. Consider if query needs justify it.

---

## Detailed Comparison Matrix

| Solution              | Git Diffs | FK/Relations | Rust | Query | Reactive | Migrations | Maint. | Score  |
| --------------------- | --------- | ------------ | ---- | ----- | -------- | ---------- | ------ | ------ |
| **Automerge + JSON**  | ✅        | ⚠️           | ✅   | ⚠️    | ✅       | ⚠️         | ✅     | 8/10   |
| **salsa + TOML**      | ✅        | ⚠️           | ✅   | ✅    | ✅       | ⚠️         | ✅     | 8.5/10 |
| **SQLite + diffable** | ✅        | ✅           | ✅   | ✅    | ⚠️       | ✅         | ⚠️     | 8/10   |
| **Event Sourcing**    | ✅        | ⚠️           | ✅   | ⚠️    | ✅       | ⚠️         | N/A    | 7/10   |
| **petgraph + serde**  | ⚠️        | ✅           | ✅   | ⚠️    | ❌       | ❌         | ✅     | 6/10   |
| **SurrealDB**         | ❌        | ✅           | ✅   | ✅    | ✅       | ⚠️         | ✅     | 5/10   |
| **Recfile**           | ✅        | ⚠️           | ❌   | ⚠️    | ❌       | ⚠️         | ⚠️     | 4/10   |
| **Dolt**              | ⚠️        | ✅           | ⚠️   | ✅    | ❌       | ✅         | ✅     | 5/10   |
| **redb/sled**         | ❌        | ❌           | ✅   | ❌    | ⚠️       | ⚠️         | ⚠️     | 3/10   |

---

## Top 5 Recommendations (Ranked)

### 1. 🥇 Automerge + Custom Schema Layer

**Architecture**:

```
plan.json          # Human-readable, git-diffable snapshot
plan.automerge     # (Optional) CRDT state for branch merging
```

**Why**:

- Merge conflicts become **impossible** (CRDT semantics)
- JSON snapshots provide readable git diffs
- Excellent Rust support (automerge is written in Rust)
- Built-in change tracking (Patch API)
- Future-ready for collaborative editing

**Implementation path**:

1. Define schema types with serde
2. Wrap in automerge Document
3. Export JSON on save (for git diffs)
4. Build typed query helpers on top

**Effort**: Medium-High (3-4 weeks to core implementation)

---

### 2. 🥈 salsa + Current TOML (No Storage Migration)

**Architecture**:

```
plan.toml          # Keep existing format
src/queries.rs     # salsa query functions
```

**Why**:

- **Zero migration risk**—keep current storage
- Immediate reactivity benefits
- Queries become memoized (O(1) for repeated calls)
- Proven in rust-analyzer (millions of users)
- Incremental—change one file, recompute only affected queries

**Implementation path**:

1. Add `salsa` crate
2. Wrap `ExoState` as salsa input
3. Convert `find_*` methods to salsa tracked functions
4. Wire up file watching to trigger input changes

**Effort**: Low-Medium (1-2 weeks)

**Limitation**: Doesn't improve FK validation or query expressiveness beyond current.

---

### 3. 🥉 SQLite + sqlite-diffable

**Architecture**:

```
plan.sql           # Text file of sorted SQL statements
src/db.rs          # rusqlite for queries
```

**Why**:

- Full SQL query power (joins, aggregates, window functions)
- Real foreign key constraints with validation
- Excellent Rust support (rusqlite)
- Line-based diffs (one row per line)
- Battle-tested, zero surprises

**Implementation path**:

1. Define SQLite schema matching current TOML structure
2. Build parser: TOML → SQLite (or start fresh with SQL)
3. Use `sqlite-diffable dump` as git pre-commit hook
4. Add salsa layer for reactivity if needed

**Effort**: Medium (2-3 weeks)

**Trade-offs**:

- SQL is less readable than TOML for humans
- Extra tooling dependency (sqlite-diffable)

---

### 4. SurrealDB Embedded (If Git Compromise Acceptable)

**Architecture**:

```
.context/           # Directory with SurrealDB files (gitignored)
plan.surql          # SurrealQL schema + data export (committed)
```

**Why**:

- Graph + document + relational in one
- LIVE queries for reactivity out of the box
- Beautiful query language (SurrealQL)
- Native Rust

**Trade-off**: Need a "git export" step or accept binary files in repo.

**Effort**: Medium (2-3 weeks)

---

### 5. Custom petgraph + serde Solution

**Architecture**:

```
plan.json          # Graph serialized as adjacency list
src/graph.rs       # petgraph Graph<Node, Edge>
```

**Why**:

- Full control over data model
- petgraph has excellent algorithms built-in
- Can add validation, versioning, reactivity exactly as needed

**Trade-off**: Significant implementation effort for query lang & reactivity.

**Effort**: High (4-6 weeks for full solution)

---

## Recommendation Summary

| If You Want...                          | Choose                               |
| --------------------------------------- | ------------------------------------ |
| Lowest risk, incremental improvement    | **salsa + TOML**                     |
| Maximum query power, don't mind SQL     | **SQLite + diffable**                |
| Future-proof merge conflict elimination | **Automerge**                        |
| Full-featured graph queries now         | **SurrealDB** (accept git trade-off) |
| Complete control, time to build         | **petgraph custom**                  |

---

## Appendix: Implementation Sketch for salsa Integration

Since this is lowest-effort highest-impact, here's a sketch:

```rust
// database.rs
#[salsa::db]
pub trait ExoDb: salsa::Database {
    fn plan_file(&self) -> PlanFile;
}

#[salsa::input]
pub struct PlanFile {
    #[return_ref]
    pub content: String,
    pub path: PathBuf,
}

#[salsa::tracked]
pub fn parsed_plan(db: &dyn ExoDb, file: PlanFile) -> ExoState {
    let content = file.content(db);
    toml::from_str(&content).expect("valid TOML")
}

#[salsa::tracked]
pub fn active_phase(db: &dyn ExoDb, file: PlanFile) -> Option<Phase> {
    let plan = parsed_plan(db, file);
    plan.epochs.iter()
        .flat_map(|e| &e.phases)
        .find(|p| p.status == "active")
        .cloned()
}

#[salsa::tracked]
pub fn goals_by_status(db: &dyn ExoDb, file: PlanFile, status: String) -> Vec<Goal> {
    let plan = parsed_plan(db, file);
    plan.epochs.iter()
        .flat_map(|e| e.phases.iter())
        .flat_map(|p| p.goals.iter())
        .filter(|g| g.status == status)
        .cloned()
        .collect()
}
```

Benefits:

- `active_phase()` called 100 times = 1 parse + 99 cache hits
- Changing file content invalidates only dependent queries
- Thread-safe, cancellation-safe

---

## Next Steps

1. **Spike salsa integration** (1 day) - Wrap one query, verify ergonomics
2. **Evaluate Automerge** (2 days) - Build POC with plan.toml data model
3. **Benchmark sqlite-diffable** (1 day) - Test diff quality with real plan.toml data

Would you like me to create a spike RFC for any of these approaches?
