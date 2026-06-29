# SQLite Virtual Table for Reactive Tracking

**Research Date**: 2026-02-10  
**Goal**: Intercept row-level reads to enable organic trace recording per the Revision Algebra.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     User Query                               │
│         SELECT * FROM goals WHERE phase_id = ?               │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  ReactiveVTable                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ xFilter     │  │ xColumn     │  │ xRowid              │  │
│  │ Records     │  │ Records     │  │                     │  │
│  │ scan start  │  │ cell access │  │                     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              Trace Recorder (Thread-Local)                   │
│  trace.record(Cell::Row("goals", rowid), revision)          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   Backing Store                              │
│         (Real SQLite table or in-memory HashMap)            │
└─────────────────────────────────────────────────────────────┘
```

---

## Core Types

```rust
use rusqlite::vtab::{
    Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor,
    VTabKind, update_module,
};
use std::cell::RefCell;
use std::sync::Arc;

/// A Cell identifier in the reactive system
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum CellId {
    /// The table's schema version (columns added/removed)
    TableSchema { table: String },
    /// The set of row IDs in the table (for collection queries)
    TableRowSet { table: String },
    /// A specific row's current revision
    Row { table: String, rowid: i64 },
    /// A specific cell (row + column) - finest grain
    Cell { table: String, rowid: i64, column: String },
}

/// Revision identifier - matches your algebra
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Revision {
    /// Epoch UUID (process lifetime)
    pub epoch: u64,
    /// Monotonic counter within epoch
    pub counter: u64,
}

/// Thread-local trace recorder
thread_local! {
    static CURRENT_TRACE: RefCell<Option<Trace>> = RefCell::new(None);
}

#[derive(Debug, Default)]
pub struct Trace {
    pub reads: Vec<(CellId, Revision)>,
}

impl Trace {
    pub fn record(&mut self, cell: CellId, revision: Revision) {
        self.reads.push((cell, revision));
    }
}

/// RAII guard for trace recording
pub struct TraceScope;

impl TraceScope {
    pub fn begin() -> Self {
        CURRENT_TRACE.with(|t| *t.borrow_mut() = Some(Trace::default()));
        TraceScope
    }

    pub fn finish(self) -> Trace {
        CURRENT_TRACE.with(|t| t.borrow_mut().take().unwrap_or_default())
    }
}

fn record_read(cell: CellId, revision: Revision) {
    CURRENT_TRACE.with(|t| {
        if let Some(trace) = t.borrow_mut().as_mut() {
            trace.record(cell, revision);
        }
    });
}
```

---

## The Virtual Table Module

```rust
use rusqlite::{Connection, Result};
use rusqlite::vtab::eponymous_or_create_module;

/// Shared state for all reactive tables
pub struct ReactiveStore {
    /// The actual SQLite connection for backing storage
    backing: Connection,
    /// Current epoch (changes on process restart)
    epoch: u64,
    /// Revision counter (monotonic within epoch)
    counter: std::sync::atomic::AtomicU64,
    /// Row revisions: (table, rowid) -> revision
    row_revisions: dashmap::DashMap<(String, i64), Revision>,
    /// Table-level revisions (for row set changes)
    table_revisions: dashmap::DashMap<String, Revision>,
}

impl ReactiveStore {
    pub fn new(backing: Connection) -> Self {
        Self {
            backing,
            epoch: uuid::Uuid::new_v4().as_u128() as u64,
            counter: std::sync::atomic::AtomicU64::new(0),
            row_revisions: dashmap::DashMap::new(),
            table_revisions: dashmap::DashMap::new(),
        }
    }

    fn next_revision(&self) -> Revision {
        Revision {
            epoch: self.epoch,
            counter: self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        }
    }

    pub fn get_row_revision(&self, table: &str, rowid: i64) -> Revision {
        *self.row_revisions
            .entry((table.to_string(), rowid))
            .or_insert_with(|| self.next_revision())
    }

    pub fn bump_row_revision(&self, table: &str, rowid: i64) -> Revision {
        let rev = self.next_revision();
        self.row_revisions.insert((table.to_string(), rowid), rev);
        rev
    }

    pub fn get_table_revision(&self, table: &str) -> Revision {
        *self.table_revisions
            .entry(table.to_string())
            .or_insert_with(|| self.next_revision())
    }

    pub fn bump_table_revision(&self, table: &str) -> Revision {
        let rev = self.next_revision();
        self.table_revisions.insert(table.to_string(), rev);
        rev
    }
}

/// The virtual table implementation
#[repr(C)]
pub struct ReactiveVTab {
    /// Base vtab (required by SQLite)
    base: rusqlite::vtab::sqlite3_vtab,
    /// Table name
    table_name: String,
    /// Columns in the table
    columns: Vec<String>,
    /// Shared store reference
    store: Arc<ReactiveStore>,
}

impl VTab<'_> for ReactiveVTab {
    type Aux = Arc<ReactiveStore>;
    type Cursor = ReactiveVTabCursor;

    fn connect(
        db: &mut VTabConnection,
        aux: Option<&Self::Aux>,
        args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let store = aux.expect("ReactiveStore required").clone();
        let table_name = std::str::from_utf8(args[2]).unwrap().to_string();

        // Query backing table for schema
        let columns = get_backing_columns(&store.backing, &table_name)?;

        // Build CREATE TABLE statement for SQLite's parser
        let column_defs: Vec<String> = columns
            .iter()
            .map(|c| format!("{c} TEXT"))  // Simplification: all TEXT
            .collect();
        let schema = format!(
            "CREATE TABLE x({})",
            column_defs.join(", ")
        );

        Ok((schema, ReactiveVTab {
            base: Default::default(),
            table_name,
            columns,
            store,
        }))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        // Accept any query plan - we'll handle filtering in xFilter
        info.set_estimated_cost(1000.0);
        Ok(())
    }

    fn open(&mut self) -> Result<Self::Cursor> {
        Ok(ReactiveVTabCursor {
            table_name: self.table_name.clone(),
            columns: self.columns.clone(),
            store: self.store.clone(),
            rows: vec![],
            current_idx: 0,
        })
    }
}
```

---

## The Cursor: Where Reads Are Tracked

```rust
#[repr(C)]
pub struct ReactiveVTabCursor {
    /// Base cursor (required by SQLite)
    base: rusqlite::vtab::sqlite3_vtab_cursor,
    table_name: String,
    columns: Vec<String>,
    store: Arc<ReactiveStore>,
    /// Fetched rows from backing store
    rows: Vec<BackingRow>,
    /// Current position
    current_idx: usize,
}

struct BackingRow {
    rowid: i64,
    values: Vec<rusqlite::types::Value>,
}

impl VTabCursor for ReactiveVTabCursor {
    /// Called when query starts - load matching rows from backing store
    fn filter(
        &mut self,
        _idx_num: i32,
        _idx_str: Option<&str>,
        _args: &rusqlite::vtab::Values<'_>,
    ) -> Result<()> {
        // Record dependency on the table's row set
        // (If rows are added/removed, queries over this table invalidate)
        let table_rev = self.store.get_table_revision(&self.table_name);
        record_read(
            CellId::TableRowSet { table: self.table_name.clone() },
            table_rev,
        );

        // Fetch all rows from backing table
        // (In production, you'd push constraints down to the backing query)
        self.rows = self.fetch_from_backing()?;
        self.current_idx = 0;

        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.current_idx += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        self.current_idx >= self.rows.len()
    }

    fn rowid(&self) -> Result<i64> {
        Ok(self.rows[self.current_idx].rowid)
    }

    /// 🎯 THE KEY METHOD: Called when SQLite reads a column value
    fn column(&self, ctx: &mut Context, col_idx: i32) -> Result<()> {
        let row = &self.rows[self.current_idx];
        let rowid = row.rowid;

        // ═══════════════════════════════════════════════════════════
        // ORGANIC CONSUMPTION: Record that this cell was read
        // ═══════════════════════════════════════════════════════════
        let row_revision = self.store.get_row_revision(&self.table_name, rowid);
        record_read(
            CellId::Row {
                table: self.table_name.clone(),
                rowid,
            },
            row_revision,
        );

        // Optional: fine-grained cell tracking
        // (Usually row-level is sufficient and cheaper)
        // let column_name = &self.columns[col_idx as usize];
        // record_read(
        //     CellId::Cell { table: self.table_name.clone(), rowid, column: column_name.clone() },
        //     row_revision,
        // );

        // Return the actual value
        let value = &row.values[col_idx as usize];
        ctx.set_result(value)?;

        Ok(())
    }
}

impl ReactiveVTabCursor {
    fn fetch_from_backing(&self) -> Result<Vec<BackingRow>> {
        let sql = format!("SELECT rowid, * FROM {}_backing", self.table_name);
        let mut stmt = self.store.backing.prepare(&sql)?;

        let rows: Vec<BackingRow> = stmt
            .query_map([], |row| {
                let rowid: i64 = row.get(0)?;
                let values: Vec<rusqlite::types::Value> = (1..=self.columns.len())
                    .map(|i| row.get(i).unwrap())
                    .collect();
                Ok(BackingRow { rowid, values })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }
}
```

---

## Writes: Bumping Revisions

```rust
impl ReactiveVTab {
    /// Handle INSERT/UPDATE/DELETE
    pub fn update(
        &mut self,
        args: &rusqlite::vtab::Values<'_>,
    ) -> Result<i64> {
        let argc = args.len();

        if argc == 1 {
            // DELETE: args[0] = rowid
            let rowid = args.get::<i64>(0)?;
            self.delete_row(rowid)?;
            self.store.bump_row_revision(&self.table_name, rowid);
            self.store.bump_table_revision(&self.table_name); // Row set changed
            Ok(0)
        } else if args.get::<Option<i64>>(0)?.is_none() {
            // INSERT: args[0] = NULL, args[1] = requested rowid or NULL, args[2..] = values
            let rowid = self.insert_row(args)?;
            self.store.bump_row_revision(&self.table_name, rowid);
            self.store.bump_table_revision(&self.table_name); // Row set changed
            Ok(rowid)
        } else {
            // UPDATE: args[0] = old rowid, args[1] = new rowid, args[2..] = values
            let old_rowid = args.get::<i64>(0)?;
            let new_rowid = args.get::<i64>(1)?;
            self.update_row(old_rowid, new_rowid, args)?;
            self.store.bump_row_revision(&self.table_name, old_rowid);
            if old_rowid != new_rowid {
                self.store.bump_row_revision(&self.table_name, new_rowid);
                self.store.bump_table_revision(&self.table_name);
            }
            Ok(0)
        }
    }
}
```

---

## Usage: Reactive Queries

```rust
pub struct ReactiveDb {
    conn: Connection,
    store: Arc<ReactiveStore>,
}

impl ReactiveDb {
    pub fn new(path: &str) -> Result<Self> {
        let backing = Connection::open(path)?;
        let store = Arc::new(ReactiveStore::new(backing));

        let conn = Connection::open_in_memory()?;

        // Register the virtual table module
        conn.create_module(
            "reactive",
            eponymous_or_create_module::<ReactiveVTab>(),
            Some(store.clone()),
        )?;

        // Create virtual tables for each backing table
        conn.execute("CREATE VIRTUAL TABLE goals USING reactive(goals)", [])?;
        conn.execute("CREATE VIRTUAL TABLE phases USING reactive(phases)", [])?;
        conn.execute("CREATE VIRTUAL TABLE epochs USING reactive(epochs)", [])?;

        Ok(Self { conn, store })
    }

    /// Execute a query and capture its trace
    pub fn query_traced<T, F>(&self, sql: &str, params: &[&dyn rusqlite::ToSql], mut f: F) -> Result<(Vec<T>, Trace)>
    where
        F: FnMut(&rusqlite::Row) -> Result<T>,
    {
        let scope = TraceScope::begin();

        let mut stmt = self.conn.prepare(sql)?;
        let results: Vec<T> = stmt
            .query_map(params, |row| f(row))?
            .filter_map(|r| r.ok())
            .collect();

        let trace = scope.finish();
        Ok((results, trace))
    }

    /// Check if a trace is still valid
    pub fn is_trace_valid(&self, trace: &Trace) -> bool {
        for (cell, recorded_revision) in &trace.reads {
            let current_revision = match cell {
                CellId::TableRowSet { table } => self.store.get_table_revision(table),
                CellId::Row { table, rowid } => self.store.get_row_revision(table, *rowid),
                CellId::Cell { table, rowid, .. } => self.store.get_row_revision(table, *rowid),
                CellId::TableSchema { table } => self.store.get_table_revision(table),
            };

            if current_revision != *recorded_revision {
                return false;
            }
        }
        true
    }
}
```

---

## Example: Memoized Query

```rust
use std::collections::HashMap;

pub struct MemoizedQuery<T> {
    sql: String,
    cached: Option<(Vec<T>, Trace)>,
}

impl<T: Clone> MemoizedQuery<T> {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            cached: None,
        }
    }

    pub fn get<F>(&mut self, db: &ReactiveDb, params: &[&dyn rusqlite::ToSql], f: F) -> Result<Vec<T>>
    where
        F: FnMut(&rusqlite::Row) -> Result<T>,
    {
        // Check if cached result is still valid
        if let Some((cached_result, trace)) = &self.cached {
            if db.is_trace_valid(trace) {
                return Ok(cached_result.clone()); // Cache hit!
            }
        }

        // Cache miss - execute and capture trace
        let (results, trace) = db.query_traced(&self.sql, params, f)?;
        self.cached = Some((results.clone(), trace));
        Ok(results)
    }
}

// Usage:
fn get_active_phase(db: &ReactiveDb) -> Result<Option<Phase>> {
    static mut QUERY: Option<MemoizedQuery<Phase>> = None;

    let query = unsafe {
        QUERY.get_or_insert_with(|| {
            MemoizedQuery::new(
                "SELECT * FROM phases WHERE status = 'active' LIMIT 1"
            )
        })
    };

    let phases = query.get(db, &[], |row| {
        Ok(Phase {
            id: row.get("id")?,
            title: row.get("title")?,
            status: row.get("status")?,
            // ...
        })
    })?;

    Ok(phases.into_iter().next())
}
```

---

## Integration with Git-Friendly Storage

The backing store remains diffable:

```
docs/agent-context/
├── goals.sql      # One INSERT per line, sorted
├── phases.sql     # One INSERT per line, sorted
├── epochs.sql     # One INSERT per line, sorted
└── schema.sql     # CREATE TABLE statements
```

On startup:

1. Parse `.sql` files into backing SQLite
2. Create virtual tables wrapping each backing table
3. Queries hit virtual tables, capturing traces

On save:

1. Dump backing tables to sorted SQL statements
2. Write to `.sql` files

---

## Alignment with Your Algebra

| Algebra Concept                  | Virtual Table Implementation                           |
| -------------------------------- | ------------------------------------------------------ |
| **Cell Identity**                | `CellId::Row { table, rowid }`                         |
| **Revision**                     | `Revision { epoch, counter }`                          |
| **Organic Consumption**          | `xColumn` calls `record_read()` automatically          |
| **Trace**                        | Thread-local `Vec<(CellId, Revision)>`                 |
| **Zero-Execution Validation**    | `is_trace_valid()` compares revisions without querying |
| **Collection Cells**             | `CellId::TableRowSet` tracks INSERT/DELETE             |
| **Content-Addressed (optional)** | Could hash row content instead of using counter        |

---

## Effort Estimate

| Component                  | Effort        | Notes                                     |
| -------------------------- | ------------- | ----------------------------------------- |
| Virtual table skeleton     | 2-3 days      | `rusqlite::vtab` is tricky but documented |
| Read tracking (`xColumn`)  | 1 day         | Core logic is simple                      |
| Write tracking (revisions) | 1 day         | Hook INSERT/UPDATE/DELETE                 |
| Trace validation           | 0.5 days      | Straightforward comparison                |
| Memoization layer          | 1-2 days      | Depends on desired ergonomics             |
| Git-friendly serialization | 1-2 days      | Sorted SQL dump                           |
| **Total**                  | **7-10 days** | Conservative estimate                     |

---

## Open Questions

1. **Granularity trade-off**: Row-level vs cell-level tracking? Row is simpler, cell is more precise.

2. **Collection semantics**: For `SELECT * FROM goals WHERE phase_id = ?`, should it depend on:
   - Just the rows that match? (Precise, but misses deletions)
   - The full table row set? (Coarse, but catches everything)
3. **JOIN tracking**: `SELECT * FROM goals JOIN phases ON ...` — need to track reads from both tables.

4. **Epoch persistence**: If you want traces to survive restarts, need content-hashing instead of counters.
