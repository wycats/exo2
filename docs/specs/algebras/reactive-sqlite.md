# **Algebra of Reactive SQLite**

RFC 10165 Working Group  
Version: 2.1 (Content Digest Revision Model)

## **0. Introduction**

This document formalizes the algebraic projection of **Validation-Based Reactivity** onto **SQLite via Virtual Tables**.

It proves that SQLite's virtual table API provides two complementary read interceptors — `xFilter` for **membership** and `xColumn` for **values** — plus a single write interceptor (`xUpdate`), which together are sufficient for **Organic Consumption** without requiring modifications to application-level query code.[^sqlite-vtab-api]

## **1. The Core Insight: Dual-Mediator Read Interception**

SQLite's virtual table architecture exposes a cursor-based API where two distinct operations mediate all data observation.[^sqlite-vtab-api]

### **Definition: The Virtual Table Projection**

Let $T$ be a virtual table backed by Revision Store $\mathcal{S}$.

$$T: \text{Schema} \times \mathcal{S}$$

A cursor $C$ over $T$ is initialized by `xFilter` (which selects a subset of rows) and then iterated. For each row $R$, column access is via `xColumn`.

These two callbacks mediate two fundamentally different kinds of observation:

| Callback  | Observation Kind | What is Observed             | Dependency Recorded                                                                        |
| --------- | ---------------- | ---------------------------- | ------------------------------------------------------------------------------------------ |
| `xFilter` | **Membership**   | Which rows exist in the scan | $\text{Record}(\text{TableRowSet}(T), \text{Membership}, \mathcal{S}_{set}.\text{rev}(T))$ |
| `xColumn` | **Content**      | What a row's column contains | $\text{Record}(\langle T, R \rangle, \text{Content}, \mathcal{S}.\text{rev}(R))$           |

Not all queries observe both kinds. A `COUNT(*)` observes only Membership (xFilter iterates, xColumn is never called). A `SELECT col FROM T WHERE rowid = ?` observes both Membership and Content.[^sqlite-count]

### **Axiom: Organic Consumption**

Both mediators record dependencies with their observation kind:

$$\text{xFilter}(T, \text{constraints}) \implies \text{Record}(\text{TableRowSet}(T), \text{Membership}, \mathcal{S}_{set}.\text{rev}(T))$$
$$\text{xColumn}(C, i) \implies \text{Record}(\langle T, R \rangle, \text{Content}, \mathcal{S}.\text{rev}(R))$$

**Crucially**: This happens inside the virtual table implementation, not in application code. SQL queries written against $T$ are reactive without modification.

### **Property: Abstraction Independence**

Any layer above SQLite (ORM, query builder, raw SQL) benefits from organic consumption because:

$$\text{ORM.query}(Q) \xrightarrow{\text{compiles to}} \text{SQL} \xrightarrow{\text{executes}} \text{xFilter + xColumn calls}$$

**Proof (step-by-step):**

1. SQLite executes SELECTs against virtual tables by invoking `xFilter` (to select rows), `xNext`/`xEof` (to iterate), and `xColumn` (to fetch column values).[^sqlite-vtab-api]
2. Any higher-level abstraction that compiles to SQL ultimately runs through these callbacks when it queries a virtual table.[^sqlite-vtab-api]
3. `xFilter` records the Membership dependency; `xColumn` records the Content dependency. Together, every observation performed by SQLite yields a trace entry with the appropriate observation kind, regardless of which abstraction generated the SQL.
4. Some queries observe only Membership (e.g., `COUNT(*)`, `EXISTS`). These never call `xColumn`, but their dependency is fully captured by `xFilter`'s row-set recording.
5. Therefore, reactivity is injected below the abstraction boundary, and all higher layers inherit it without modification.

## **2. Cell Identity in SQLite**

### **Definition: Row Cells**

A Cell in SQLite is identified by the tuple $\langle \text{Table}, \text{Rowid} \rangle$.

$$\text{CellId} = \text{Table} \times \text{Rowid}$$

SQLite assigns a `rowid` to each row of a rowid table, and the `rowid` can change after `VACUUM` unless the table uses an `INTEGER PRIMARY KEY` (which aliases the `rowid`).[^sqlite-rowid][^sqlite-vacuum]

### **Definition: Table Row-Set Cells**

To track INSERT/DELETE (collection membership), we introduce a synthetic cell per table:

$$\text{TableRowSet}(T) = \langle T, \bot \rangle$$

This cell's revision is bumped whenever the set of rowids in $T$ changes.

### **Property: Adaptive Granularity**

Row-level granularity is the natural unit. If finer granularity is needed:

1. **Normalize**: Split wide rows into related tables.
2. **The trace machinery is unchanged**.

$$\text{Granularity}(T) = \text{Row} \implies \text{Normalize}(T) \to T_1, T_2, \dots$$

Each normalized table still tracks at row level. No new concepts required.

## **3. The Revision Model**

### **Definition: Per-Row Revisions (Content Digest)**

The Revision Store $\mathcal{S}$ maintains a **content digest** per row:

$$\mathcal{S}: (\text{Table}, \text{Rowid}) \to \mathcal{R}_{disk}$$

Where $\mathcal{R}_{disk} \subseteq \{0,1\}^k$ is a content hash of the row's bytes (per the core Revision Algebra §2).

This choice instantiates the Identity-Equivalence desideratum (core algebra §1):

- **Identity** is the rowid — how the cell is addressed.
- **Equivalence** is the digest — whether the content has changed.

Content digests have three properties that epoch-scoped counters lack:

1. **Survive process restarts**: No epoch pairing needed. The hash of the data IS the revision.
2. **Detect no-op UPDATEs**: `UPDATE t SET col = col` does not change the digest, so dependents do not revalidate.
3. **Cross-process validity**: Two processes that independently read the same data compute the same digest.

The trade-off is $O(\text{row\_size})$ per write to compute the digest, vs. $O(1)$ for a counter. This is acceptable for the current workload (hundreds of rows, not millions).

### **Definition: Row-Set Revisions (Persistent Monotonic Counter)**

Row-Set revisions track _which rows exist_, not _what they contain_. Since SQLite is persistent storage, Row-Set revisions use persistent monotonic counters:

$$\mathcal{S}_{set}: \text{Table} \to \mathcal{R}_{disk}$$

Where $\mathcal{R}_{disk}$ is a monotonic counter stored in the `rowset_revisions` table, encoded as a string for the `Revision::Disk { hash }` variant.

The counter survives process restarts — it is loaded from the database on startup, not reset. There is no epoch-scoped UUID; the counter is the sole revision.

This is bumped on every mutation (INSERT, UPDATE, DELETE) — see Write Bumping below.

### **Property: Write Bumping**

| Operation | Row Revision           | Row-Set Revision |
| --------- | ---------------------- | ---------------- |
| `UPDATE`  | Bumped                 | Bumped           |
| `INSERT`  | Created (new revision) | Bumped           |
| `DELETE`  | Removed                | Bumped           |

**Design Note (One-Sided Error)**: Bumping the Row-Set revision on UPDATE is conservative. An UPDATE that changes column values without affecting any query's predicate membership causes unnecessary revalidation. Per the core algebra's One-Sided Error property (§3), this is _over-tracking_: wasted work, always correct.

The alternative — leaving Row-Set unchanged on UPDATE — creates _under-tracking_: if an UPDATE changes a value used in a `WHERE` predicate, a row may enter or leave a query's result set without the trace detecting it. Under-tracking produces staleness (stale cached results served as current).

The conservative default eliminates this gap entirely. §8 (Refinement Paths) formalizes techniques for reducing the false-positive rate without sacrificing soundness.

### **Corollary: Query Stability**

A query $Q$ that scans a table and filters by predicate depends on:

1. **Row-Set** (to detect any mutation — all writes bump this revision)
2. **Each returned row** (to detect value changes in returned data)

Since all writes bump the Row-Set revision, $Q$ is guaranteed to revalidate after any mutation to $T$. This is conservative: an UPDATE that doesn't affect predicate membership still triggers revalidation. Per One-Sided Error (core algebra §3), this over-tracking produces wasted work but never stale data. §8 formalizes refinement paths to reduce false positives.

## **4. The Virtual Table API Projection**

### **Exhaustive Callback Surface**

SQLite's virtual table interface defines the following callbacks.[^sqlite-vtab-api] Each is classified by its algebraic role in the dependency tracking system.

| Callback                                     | Category        | Algebraic Role                                                                                                              |
| -------------------------------------------- | --------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `xColumn`                                    | **Read**        | Content observation — records $\text{Record}(\langle T, R \rangle, \text{Content}, \mathcal{S}.\text{rev}(R))$              |
| `xRowid`                                     | **Read**        | Identity read — returns the current row's `rowid`. See below.                                                               |
| `xFilter`                                    | **Collection**  | Membership observation — records $\text{Record}(\text{TableRowSet}(T), \text{Membership}, \mathcal{S}_{set}.\text{rev}(T))$ |
| `xUpdate`                                    | **Write**       | Mutation — applies the write-bumping rules from §3                                                                          |
| `xBestIndex`                                 | Planning        | Query planner cost estimation. No data access.                                                                              |
| `xNext`                                      | Cursor          | Advance cursor to next row. No data access.                                                                                 |
| `xEof`                                       | Cursor          | Check cursor exhaustion. No data access.                                                                                    |
| `xOpen` / `xClose`                           | Cursor          | Cursor lifecycle. No data access.                                                                                           |
| `xCreate` / `xConnect`                       | Lifecycle       | Table creation/connection. No data access.                                                                                  |
| `xDisconnect` / `xDestroy`                   | Lifecycle       | Table disconnection/destruction. No data access.                                                                            |
| `xBegin` / `xSync` / `xCommit` / `xRollback` | Transaction     | Transaction boundaries. Orthogonal to dependency tracking.                                                                  |
| `xSavepoint` / `xRelease` / `xRollbackTo`    | Transaction     | Savepoint management. Orthogonal to dependency tracking.                                                                    |
| `xFindFunction`                              | Miscellaneous   | Function overloading. No data access.                                                                                       |
| `xRename`                                    | Miscellaneous   | Table rename. No data access.                                                                                               |
| `xShadowName`                                | **Enforcement** | Shadow table declaration. See below.                                                                                        |
| `xIntegrity`                                 | Diagnostic      | Integrity checking (`PRAGMA integrity_check`). No data access.                                                              |

Of these, exactly **four** are algebraically active (record traces or bump revisions): `xColumn`, `xRowid`, `xFilter`, `xUpdate`. One — `xShadowName` — is algebraically _significant_ without being active.

### **Property: xShadowName Enforces Mediated Access**

Virtual tables that store data in real SQLite tables ("shadow tables") must implement `xShadowName` to declare those tables as belonging to the virtual table. When `SQLITE_DBCONFIG_DEFENSIVE` is enabled, SQLite makes recognized shadow tables **read-only to ordinary SQL** — only code executing inside the virtual table's own methods can write to them.

This elevates Mediated Access from an asserted axiom to an **enforced** axiom: the database engine itself blocks unmediated writes to the backing store. Without `xShadowName`, any SQL statement could bypass the virtual table layer and write directly to the underlying data, violating the Mediated Access guarantee.

$$\text{xShadowName} + \text{DEFENSIVE} \implies \forall \text{ writes to backing store}: \text{must pass through } \mathtt{xUpdate}$$

This is not trace-recording infrastructure — `xShadowName` never fires during query execution. But it is the mechanism that makes the three active callbacks a **complete** mediation surface rather than a merely conventional one.

### **Property: xRowid is Subsumed by xFilter**

`xRowid` returns the identity of the current row. When all tables use `INTEGER PRIMARY KEY` (required — see Appendix), the `rowid` is immutable and serves as the cell identity key.

The dependency recorded by `xRowid` is: "I observed that row $R$ exists in this scan." But this membership fact is already captured by `xFilter`, which records $\text{TableRowSet}(T)$. Since `xRowid` is only callable on rows returned by a prior `xFilter` call, the row-set dependency subsumes the identity dependency:

$$\text{xFilter}(T) \implies \text{Record}(\text{TableRowSet}(T), \ldots) \implies \text{membership of } R \text{ is tracked}$$

Therefore, `xRowid` does not require its own trace entry. The algebraically _active_ callbacks reduce to three: `xColumn`, `xFilter`, `xUpdate`.

**Note**: If tables with mutable `rowid` (non-`INTEGER PRIMARY KEY`) were permitted, `xRowid` would need an independent trace entry recording identity observation. The `INTEGER PRIMARY KEY` requirement (Appendix) avoids this.

### **Axiom: Mediated Access (SQLite Projection)**

All SQL-visible data access is mediated by kind-specific interceptors. Content observations (value reads) are mediated by `xColumn`. Membership observations (row-set scans) are mediated by `xFilter`. All mutations are mediated by `xUpdate`.

$$\forall \text{ Content observation on } T: \text{occurs via } \mathtt{xColumn}$$
$$\forall \text{ Membership observation on } T: \text{occurs via } \mathtt{xFilter}$$
$$\forall \text{ mutation of } T: \text{occurs via } \mathtt{xUpdate}$$

This is the SQLite-specific instance of the core algebra's Mediated Access axiom (§3), instantiating both the Content and Membership sub-axioms. Combined with Organic Consumption, no SQL query can observe table state without producing a trace entry with the appropriate observation kind.

### **Property: Completeness of Interception**

The three active callbacks are _sufficient_ to implement the core algebra's Organic Consumption axiom:

- Every column value read passes through `xColumn` → satisfies the Read recording requirement.
- Every row-set scan begins with `xFilter` → satisfies the Collection Cell requirement.
- Every mutation passes through `xUpdate` → satisfies the Write Bumping property.

The remaining callbacks (cursor mechanics, lifecycle, transaction, planning) are either internal bookkeeping or orthogonal to dependency tracking. The virtual table API is a _complete_ projection surface for Validation-Based Reactivity.

## **5. Composition with Memoization**

### **Definition: Reactive Query**

A Reactive Query $Q$ is a memoized computation whose trace is populated by `xFilter` (Membership) and `xColumn` (Content) observations.

$$Q: \text{SQL} \to (v, \mathcal{T})$$

Where $\mathcal{T}$ is the trace of all observations during execution — Membership observations on Row-Set cells (from `xFilter`) and Content observations on Row cells (from `xColumn`).

### **Property: Validation Cost**

Validating $Q$ costs $O(|\mathcal{T}|)$ revision comparisons.

$$\text{Cost}(\text{Validate}(Q)) = O(|\mathcal{T}|)$$

For a query returning $N$ rows with $C$ columns each:

$$|\mathcal{T}| \leq 1 + N \cdot C$$

Where the $1$ is the Row-Set cell.

### **Optimization: Row-Level Coalescing**

Since we track at row granularity (not column), we can coalesce:

$$|\mathcal{T}| \leq 1 + N$$

Multiple column reads from the same row share one revision.

## **6. Trace Composition in SQL**

### **Property: Trace Flattening (from Core Algebra §9)**

A SQL query $Q$ may contain subqueries, CTEs, or be invoked from a higher-level derived computation. Per the Transient Computation rule from the core algebra:

If $P$ is a parent computation and $Q$ is a query executed within $P$:

$$\mathcal{T}_P' = \mathcal{T}_P \cup \mathcal{T}_Q$$

The query $Q$ has no persistent identity in the dependency graph. Its trace entries (from `xColumn` and `xFilter`) are recorded directly into the enclosing scope.

### **Implication: The SQL Inlining Equivalence**

$$\text{Trace}(P \to Q \to \{\text{xFilter, xColumn}\}) \equiv \text{Trace}(P \to \{\text{xFilter, xColumn}\})$$

This means:

1. A subquery's dependencies (both membership and value) are merged into the outer query's trace.
2. A CTE's dependencies are merged into the statement's trace.
3. A derived signal that calls a SQL query gets the query's dependencies merged into its trace.

No separate memoization node is created for intermediate query structures. The reactivity system "sees through" SQL composition, just as it sees through function composition in the core algebra.

## **7. Trace Divergence in SQL**

### **Property: Predicate-Driven Divergence**

Per the core algebra's Dynamic Dependencies rule (§6), if a query's inputs change such that different rows are returned, the trace diverges.

Let $Q(\Sigma)$ be a query evaluated against state $\Sigma$, producing trace $\mathcal{T}$.
Let $\Sigma'$ be a new state where a row is inserted.

$$\text{Row-Set revision changed} \implies \mathcal{T} \text{ is invalid}$$

Re-executing $Q(\Sigma')$ produces a new trace $\mathcal{T}'$ that may contain different row cells (the new row) or fewer row cells (if a row was deleted).

### **Corollary: Conservative Row-Set Invalidation**

Since all writes (INSERT, UPDATE, DELETE) bump the $\text{TableRowSet}$ revision (§3), trace divergence is always _detected_. This guarantee extends to UPDATE-driven predicate membership changes: if an UPDATE causes a row to enter or leave a query's effective result set, the Row-Set bump ensures revalidation.

This is the conservative approximation from the core algebra's Adaptive Granularity property. §8 formalizes refinement paths — notably predicate-aware validators — that can reduce false-positive invalidation without weakening the soundness guarantee.

## **8. Delta Types and Refinement Paths**

### **Definition: SQLite Delta Types**

Per the core algebra's Delta definition (§10), each cell type has a specific change descriptor.

| Cell Type                            | Delta ($\Delta$)                     | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ |
| Row Cell $\langle T, R \rangle$      | Column bitmask                       | Set of columns changed by the UPDATE |
| Row-Set Cell $\text{TableRowSet}(T)$ | $\Delta_{set} = \{+R, -R, {\sim}R\}$ | Row added, removed, or mutated       |

The default delta for both cell types is $\delta = \top$ (opaque change), per the core algebra's Delta Consistency axiom: over-reporting is sound, under-reporting is forbidden.

### **Definition: Default Validator**

The default Validator for all SQLite cells is $V = \top$: any revision change triggers invalidation. This is sound but coarse — the false-positive rate (FPR) approaches 1.0 for workloads dominated by writes to unrelated rows.

### **Refinement Path: Conservative (Current Default)**

The write-bumping table in §3 combined with $V = \top$ represents the conservative configuration:

- **All writes** bump both the row revision and the Row-Set revision.
- **All queries** revalidate on any write to any table they depend on.
- **FPR**: High for mixed-write workloads. Sound by construction.

### **Refinement Path: Predicate-Aware Validation**

A predicate-aware validator uses the column bitmask delta to determine whether an UPDATE could affect predicate membership:

$$V_{pred}(\delta, P) = \begin{cases} \text{invalid} & \text{if } \text{columns}(\delta) \cap \text{columns}(P) \neq \emptyset \\ \text{valid} & \text{otherwise} \end{cases}$$

Where $P$ is the registered predicate and $\text{columns}(\delta)$ is the set of updated columns.

This reduces the false-positive rate for UPDATE-heavy workloads where most updates don't affect predicate columns. The soundness proof derives from the core algebra's Approximation Soundness (§11): $V_{pred}$ is a delta predicate on $\mathcal{P}(\Delta)$, and its FPR is bounded by the fraction of deltas whose column sets overlap the predicate columns.

**Note**: Predicate-aware validation is an optimization, not a correctness requirement. The conservative default (§3) is sound without it.

## **9. Algebraic Summary**

| Algebra Concept                      | SQLite Projection                                                |
| ------------------------------------ | ---------------------------------------------------------------- |
| Cell Identity $\langle S, P \rangle$ | $\langle \text{Table}, \text{Rowid} \rangle$                     |
| Collection Cell                      | `TableRowSet(T)`                                                 |
| Organic Consumption                  | `xColumn` → Content record, `xFilter` → Membership record        |
| Mediated Access                      | `xColumn` (Content) + `xFilter` (Membership), `xUpdate` (writes) |
| Write Bumping                        | `xUpdate` → `store.bump_revision()` (all writes bump Row-Set)    |
| Delta                                | Column bitmask (row), $\{+R, -R, {\sim}R\}$ (row-set)            |
| Default Validator                    | $V = \top$ (any change invalidates)                              |
| Trace                                | `HashSet<(CellId, ObservationKind, Revision)>`                   |
| Validation                           | $O(\|\mathcal{T}\|)$ revision comparisons                        |
| Memoization                          | `MemoizedQuery { sql, trace, result }`                           |

### **Theorem: Abstraction Independence**

Let $L$ be any abstraction layer (ORM, query builder, raw SQL) that compiles to SQLite queries.

$$\text{Reactive}(L) \iff \text{Reactive}(\text{VirtualTable})$$

**Proof**: All data access through $L$ ultimately invokes SQLite's virtual table callbacks — `xFilter` for Membership observations, `xColumn` for Content observations — when querying a virtual table.[^sqlite-vtab-api] The virtual table intercepts unconditionally and records both observation kinds into the trace. Therefore $L$ inherits reactivity without modification. ∎

### **Corollary: Stack Freedom**

The choice of query abstraction (raw SQL, query builder, ORM) is orthogonal to reactivity. All observation flows through two kind-specific interceptors: `xFilter` (Membership) and `xColumn` (Content).

## **Appendix: Open Considerations**

- **Query planner column elision**: Beyond `count(*)`, other SQLite optimizations may skip `xColumn` calls (covering indexes, `EXISTS` subqueries). All known cases still depend on row-set membership captured by `xFilter`. This is conservative — any undiscovered elision causes over-invalidation, not missed invalidation (per One-Sided Error, §3).
- **Predicate-aware validators**: Formalized in §8 as a refinement path. The Row-Set cell combined with conservative UPDATE bumping is sound but coarse. Predicate-aware validators reduce the false-positive rate for UPDATE-heavy workloads.
- **Rowid stability under VACUUM**: `rowid` is only stable when aliased by `INTEGER PRIMARY KEY`.[^sqlite-rowid][^sqlite-vacuum] All tables in this system must use `INTEGER PRIMARY KEY` or explicit text primary keys to ensure Cell Identity survives compaction. This constraint also enables the xRowid subsumption property (§4).
- **Reactive transactions**: The current system assumes **single-threaded, monotonic access** to the database. Every `xUpdate` publishes its revision change immediately; every `xColumn`/`xFilter` reads the latest state. The transaction callbacks (`xBegin`, `xCommit`, `xRollback`, etc.) are managed by SQLite for its own internal consistency but are transparent to the reactive layer — no branching revision model is needed. If the system ever requires concurrent or transactional access (multiple writers, read-your-writes within a batch), transactions would become algebraically significant: a transaction would create a speculative fork of the revision store (analogous to Datomic's `db.with(tx-data)`), and derived computations within the fork would need to be resolved on commit or discarded on rollback. This would require extending the core algebra's assumption of a single linear revision history.
- **Reactive schema collections**: The table lifecycle callbacks (`xCreate`, `xConnect`, `xDestroy`) are currently inert. If the system ever needs reactive "list of tables" support, these become mutations on a `SchemaRowSet` collection cell — `xCreate`/`xDestroy` as INSERT/DELETE on the schema collection, `xConnect` as a Membership observation. The vtable API already provides the hooks; only the reactive bookkeeping would need to be added.

## **Footnotes**

[^sqlite-vtab]: https://www.sqlite.org/vtab.html

[^sqlite-vtab-api]: https://www.sqlite.org/vtab.html#the_virtual_table_method_table

[^sqlite-rowid]: https://www.sqlite.org/rowidtable.html

[^sqlite-vacuum]: https://www.sqlite.org/lang_vacuum.html

[^sqlite-count]: https://www.sqlite.org/lang_select.html#the_count_aggregate_function
