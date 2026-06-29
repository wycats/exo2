//! Reactive virtual table implementation.
//!
//! This module implements the core virtual table that wraps shadow tables
//! and records observations into TraceScope.

use libsqlite3_sys as ffi;
use rusqlite::types::{Value, ValueRef};
use rusqlite::vtab::{
    update_module, ConflictMode, Context, CreateVTab, Filters, IndexInfo, Inserts, UpdateVTab,
    Updates, VTab, VTabConfig, VTabConnection, VTabCursor, VTabKind, Values,
};
use rusqlite::{Connection, Error, Result};
use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::rc::Rc;

use crate::revisions::{compute_row_digest, RevisionStore};
use crate::trace::{
    counter_revision, digest_revision, row_cell_id, table_membership_cell_id, TraceScope,
};
use crate::vtab::shadow::with_reactive_shadow_names;
use crate::DatabaseError;

const REACTIVE_SHADOW_TABLES: &[&str] = &[
    "epochs_data",
    "phases_data",
    "goals_data",
    "tasks_data",
    "phase_rfcs_data",
    "ideas_data",
    "inbox_data",
    "rfcs_data",
    "workspace_active_phase_data",
    "phase_ownership_data",
];

/// Auxiliary data passed to the virtual table module.
///
/// Contains the raw sqlite3 handle and revision store needed for
/// querying shadow tables and tracking revisions.
pub struct VTabAux {
    /// Raw sqlite3 handle for querying shadow table (bypasses rusqlite's RefCell)
    db: *mut ffi::sqlite3,
    /// Revision store for tracking row and rowset revisions
    revision_store: Rc<RevisionStore>,
}

/// The reactive virtual table module.
///
/// This is registered with SQLite and used to create virtual tables
/// that wrap shadow tables.
#[repr(C)]
#[allow(dead_code)]
pub struct ReactiveModule;

/// A reactive virtual table instance.
///
/// Each virtual table wraps a single shadow table and records all
/// access into TraceScope.
#[repr(C)]
pub struct ReactiveVTab {
    /// The name of the virtual table (e.g., "epochs")
    table_name: String,
    /// The name of the shadow table (e.g., "epochs_data")
    shadow_table: String,
    /// Column names from the shadow table
    columns: Vec<String>,
    /// Column default expressions from the shadow table, by column index.
    column_defaults: Vec<Option<String>>,
    /// Column index for an INTEGER PRIMARY KEY rowid alias, when present.
    rowid_column_index: Option<usize>,
    /// Raw sqlite3 handle for querying shadow table (bypasses rusqlite's RefCell)
    /// Safety: This pointer is valid for the lifetime of the vtab,
    /// which is tied to the connection that created it.
    db: *mut ffi::sqlite3,
    /// Revision store for tracking row and rowset revisions
    revision_store: Rc<RevisionStore>,
}

impl ReactiveVTab {
    /// Get the shadow table name.
    pub fn shadow_table(&self) -> &str {
        &self.shadow_table
    }

    fn column_values(&self, args: &rusqlite::vtab::Values<'_>) -> Result<Vec<Value>> {
        let expected = self.columns.len() + 2;
        if args.len() != expected {
            return Err(Error::ModuleError(format!(
                "Expected {} xUpdate arguments for '{}', got {}",
                expected,
                self.table_name,
                args.len()
            )));
        }

        let mut values = Vec::with_capacity(self.columns.len());
        for idx in 0..self.columns.len() {
            values.push(args.get::<Value>(idx + 2)?);
        }
        Ok(values)
    }

    fn insert_row(
        &self,
        requested_rowid: Option<i64>,
        values: &[Value],
        explicit_nulls: &[bool],
        conflict_mode: ConflictMode,
    ) -> Result<i64> {
        if let Some(requested_rowid) = requested_rowid {
            match self.rowid_column_index.and_then(|index| values.get(index)) {
                Some(Value::Integer(column_rowid)) if *column_rowid == requested_rowid => {}
                Some(Value::Null) | None => {
                    return Err(Error::ModuleError(format!(
                        "Reactive virtual table '{}' does not support rowid-only inserts",
                        self.table_name
                    )));
                }
                Some(Value::Integer(_)) => {
                    return Err(Error::ModuleError(format!(
                        "Reactive virtual table '{}' received conflicting rowid values",
                        self.table_name
                    )));
                }
                Some(_) => {
                    return Err(Error::ModuleError(format!(
                        "Reactive virtual table '{}' received a non-integer rowid value",
                        self.table_name
                    )));
                }
            }
        }

        let replace_mode = conflict_mode == ConflictMode::Replace;
        // SQLite passes omitted insert columns as NULL to xUpdate. For columns
        // with shadow-table defaults, omit unbound NULLs so the backing table
        // applies its default; bound NULLs stay explicit and hit constraints.
        let included = self
            .columns
            .iter()
            .zip(values.iter())
            .zip(explicit_nulls.iter())
            .zip(self.column_defaults.iter())
            .filter_map(|(((column, value), explicit_null), default)| {
                (!matches!(value, Value::Null) || *explicit_null || default.is_none())
                    .then_some((column, value))
            })
            .collect::<Vec<_>>();
        let columns = included
            .iter()
            .map(|(column, _)| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = (0..included.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let included_values = included
            .iter()
            .map(|(_, value)| (*value).clone())
            .collect::<Vec<_>>();
        let sql = if included_values.is_empty() {
            format!(
                "{} INTO {} DEFAULT VALUES",
                insert_keyword(conflict_mode),
                quote_identifier(&self.shadow_table)
            )
        } else {
            format!(
                "{} INTO {} ({}) VALUES ({})",
                insert_keyword(conflict_mode),
                quote_identifier(&self.shadow_table),
                columns,
                placeholders
            )
        };

        let before = replace_mode.then(|| row_counts(self.db)).transpose()?;
        let changed = execute_raw(self.db, &sql, &included_values)?;
        if changed == 0 {
            return Err(ignored_insert_error(&self.table_name));
        }

        let rowid = unsafe { ffi::sqlite3_last_insert_rowid(self.db) };
        if let Some(before) = before {
            refresh_row_revision(self.db, &self.revision_store, &self.shadow_table, rowid)?;
            clear_stale_row_revisions(self.db, &self.revision_store, &self.shadow_table)?;
            self.refresh_cascaded_revision_metadata(&before)?;
        } else {
            refresh_row_revision(self.db, &self.revision_store, &self.shadow_table, rowid)?;
        }
        self.bump_rowset_revision_for(&self.shadow_table)?;
        Ok(rowid)
    }

    fn update_row(
        &self,
        old_rowid: i64,
        values: &[Value],
        no_change: &[bool],
        conflict_mode: ConflictMode,
    ) -> Result<()> {
        if values.len() != no_change.len() {
            return Err(Error::ModuleError(format!(
                "Mismatched update values for '{}': {} values, {} no-change markers",
                self.table_name,
                values.len(),
                no_change.len()
            )));
        }

        let replace_mode = conflict_mode == ConflictMode::Replace;
        let changed = self
            .columns
            .iter()
            .zip(values.iter())
            .zip(no_change.iter())
            .filter_map(|((column, value), unchanged)| (!*unchanged).then_some((column, value)))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(());
        }

        let assignments = changed
            .iter()
            .map(|(column, _)| format!("{} = ?", quote_identifier(column)))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE {} {} SET {} WHERE rowid = ?",
            conflict_clause(conflict_mode),
            quote_identifier(&self.shadow_table),
            assignments
        );

        let mut params = changed
            .iter()
            .map(|(_, value)| (*value).clone())
            .collect::<Vec<_>>();
        params.push(Value::Integer(old_rowid));
        let before = replace_mode.then(|| row_counts(self.db)).transpose()?;
        let changed = execute_raw(self.db, &sql, &params)?;
        if changed == 0 {
            return Ok(());
        }

        if let Some(before) = before {
            refresh_row_revision(self.db, &self.revision_store, &self.shadow_table, old_rowid)?;
            clear_stale_row_revisions(self.db, &self.revision_store, &self.shadow_table)?;
            self.refresh_cascaded_revision_metadata(&before)?;
        } else {
            refresh_row_revision(self.db, &self.revision_store, &self.shadow_table, old_rowid)?;
        }
        self.bump_rowset_revision_for(&self.shadow_table)?;
        Ok(())
    }

    fn delete_row(&self, rowid: i64) -> Result<()> {
        let sql = format!(
            "DELETE FROM {} WHERE rowid = ?",
            quote_identifier(&self.shadow_table)
        );
        let before = row_counts(self.db)?;
        let changed = execute_raw(self.db, &sql, &[Value::Integer(rowid)])?;
        if changed == 0 {
            return Ok(());
        }

        clear_row_revision(self.db, &self.revision_store, &self.shadow_table, rowid)?;
        self.bump_rowset_revision_for(&self.shadow_table)?;
        self.refresh_cascaded_revision_metadata(&before)?;
        Ok(())
    }

    fn refresh_cascaded_revision_metadata(&self, before: &[(String, i64)]) -> Result<()> {
        for table in REACTIVE_SHADOW_TABLES {
            if *table == self.shadow_table {
                continue;
            }
            let before_count = before
                .iter()
                .find_map(|(name, count)| (name == table).then_some(*count))
                .unwrap_or(0);
            let after_count = table_row_count(self.db, table)?;
            if before_count != after_count {
                clear_stale_row_revisions(self.db, &self.revision_store, table)?;
                self.bump_rowset_revision_for(table)?;
            }
        }
        Ok(())
    }

    fn bump_rowset_revision_for(&self, table: &str) -> Result<()> {
        let update = "UPDATE rowset_revisions SET counter = counter + 1 WHERE table_name = ?";
        let changed = execute_raw(self.db, update, &[Value::Text(table.to_string())])?;
        if changed == 0 {
            execute_raw(
                self.db,
                "INSERT INTO rowset_revisions (table_name, counter) VALUES (?, 1)",
                &[Value::Text(table.to_string())],
            )?;
        }

        let counter = rowset_counter_raw(self.db, table)?;
        self.revision_store.cache_rowset_counter(table, counter);
        Ok(())
    }
}

/// Cursor for iterating over a reactive virtual table.
#[repr(C)]
pub struct ReactiveVTabCursor<'vtab> {
    /// Reference to the virtual table
    vtab: &'vtab ReactiveVTab,
    /// Current row data (rowid, columns...)
    current_row: Option<Vec<rusqlite::types::Value>>,
    /// All rows from the query (simple implementation)
    rows: Vec<Vec<rusqlite::types::Value>>,
    /// Current position in rows
    position: usize,
    /// Phantom for lifetime
    _phantom: PhantomData<&'vtab ()>,
}

impl<'vtab> ReactiveVTabCursor<'vtab> {
    fn new(vtab: &'vtab ReactiveVTab) -> Self {
        Self {
            vtab,
            current_row: None,
            rows: Vec::new(),
            position: 0,
            _phantom: PhantomData,
        }
    }

    /// Fetch all rows from the shadow table using raw SQLite FFI.
    ///
    /// This bypasses rusqlite's Connection wrapper to avoid RefCell conflicts
    /// when called from within vtab callbacks.
    fn fetch_rows(&mut self) -> Result<()> {
        let db = self.vtab.db;
        let shadow_table = &self.vtab.shadow_table;
        let column_count = self.vtab.columns.len();

        let sql = format!("SELECT rowid, * FROM {}", shadow_table);
        let sql_cstr =
            CString::new(sql).map_err(|e| Error::ModuleError(format!("Invalid SQL: {}", e)))?;

        let mut stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();
        let mut tail: *const i8 = ptr::null();

        // Prepare the statement using raw FFI
        let rc =
            unsafe { ffi::sqlite3_prepare_v2(db, sql_cstr.as_ptr(), -1, &mut stmt, &mut tail) };

        if rc != ffi::SQLITE_OK {
            let errmsg = unsafe {
                let msg = ffi::sqlite3_errmsg(db);
                if msg.is_null() {
                    "Unknown error".to_string()
                } else {
                    CStr::from_ptr(msg).to_string_lossy().into_owned()
                }
            };
            return Err(Error::ModuleError(format!(
                "Failed to prepare statement: {}",
                errmsg
            )));
        }

        // Ensure we finalize the statement when done
        struct StmtGuard(*mut ffi::sqlite3_stmt);
        impl Drop for StmtGuard {
            fn drop(&mut self) {
                unsafe {
                    ffi::sqlite3_finalize(self.0);
                }
            }
        }
        let _guard = StmtGuard(stmt);

        // Fetch all rows
        let mut rows = Vec::new();
        loop {
            let rc = unsafe { ffi::sqlite3_step(stmt) };
            match rc {
                ffi::SQLITE_ROW => {
                    let mut values = Vec::with_capacity(column_count + 1);

                    // Get column count from statement (rowid + columns)
                    let col_count = unsafe { ffi::sqlite3_column_count(stmt) } as usize;

                    for i in 0..col_count {
                        let col_type = unsafe { ffi::sqlite3_column_type(stmt, i as c_int) };
                        let value = match col_type {
                            ffi::SQLITE_INTEGER => {
                                let v = unsafe { ffi::sqlite3_column_int64(stmt, i as c_int) };
                                rusqlite::types::Value::Integer(v)
                            }
                            ffi::SQLITE_FLOAT => {
                                let v = unsafe { ffi::sqlite3_column_double(stmt, i as c_int) };
                                rusqlite::types::Value::Real(v)
                            }
                            ffi::SQLITE_TEXT => {
                                let ptr = unsafe { ffi::sqlite3_column_text(stmt, i as c_int) };
                                if ptr.is_null() {
                                    rusqlite::types::Value::Null
                                } else {
                                    let len = unsafe { ffi::sqlite3_column_bytes(stmt, i as c_int) }
                                        as usize;
                                    let s = unsafe {
                                        std::slice::from_raw_parts(ptr as *const u8, len)
                                    };
                                    let s = String::from_utf8_lossy(s).into_owned();
                                    rusqlite::types::Value::Text(s)
                                }
                            }
                            ffi::SQLITE_BLOB => {
                                let ptr = unsafe { ffi::sqlite3_column_blob(stmt, i as c_int) };
                                let len =
                                    unsafe { ffi::sqlite3_column_bytes(stmt, i as c_int) } as usize;
                                if ptr.is_null() || len == 0 {
                                    rusqlite::types::Value::Blob(vec![])
                                } else {
                                    let slice = unsafe {
                                        std::slice::from_raw_parts(ptr as *const u8, len)
                                    };
                                    rusqlite::types::Value::Blob(slice.to_vec())
                                }
                            }
                            _ => rusqlite::types::Value::Null,
                        };
                        values.push(value);
                    }
                    rows.push(values);
                }
                ffi::SQLITE_DONE => break,
                _ => {
                    let errmsg = unsafe {
                        let msg = ffi::sqlite3_errmsg(db);
                        if msg.is_null() {
                            "Unknown error".to_string()
                        } else {
                            CStr::from_ptr(msg).to_string_lossy().into_owned()
                        }
                    };
                    return Err(Error::ModuleError(format!(
                        "Failed to step statement: {}",
                        errmsg
                    )));
                }
            }
        }

        self.rows = rows;
        self.position = 0;
        self.current_row = self.rows.first().cloned();
        Ok(())
    }
}

unsafe impl<'vtab> VTab<'vtab> for ReactiveVTab {
    type Aux = VTabAux;
    type Cursor = ReactiveVTabCursor<'vtab>;

    fn connect(
        db: &mut VTabConnection,
        aux: Option<&Self::Aux>,
        args: &[&[u8]],
    ) -> Result<(String, Self)> {
        // Args: [module_name, db_name, table_name, shadow_table_name]
        if args.len() < 4 {
            return Err(Error::ModuleError(
                "Usage: CREATE VIRTUAL TABLE t USING reactive(shadow_table)".to_string(),
            ));
        }

        let table_name = std::str::from_utf8(args[2])
            .map_err(|e| Error::ModuleError(e.to_string()))?
            .to_string();

        let shadow_table = std::str::from_utf8(args[3])
            .map_err(|e| Error::ModuleError(e.to_string()))?
            .to_string();

        // Get the aux data
        let vtab_aux =
            aux.ok_or_else(|| Error::ModuleError("No aux data available".to_string()))?;

        // Query shadow table schema using raw FFI to avoid RefCell conflicts.
        // The connect callback runs while rusqlite already holds a borrow on
        // the connection, so we must bypass its RefCell wrapper.
        let raw_db = vtab_aux.db;
        let sql = format!("PRAGMA table_info({})", shadow_table);
        let sql_cstr =
            CString::new(sql).map_err(|e| Error::ModuleError(format!("Invalid SQL: {}", e)))?;

        let mut stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();
        let mut tail: *const i8 = ptr::null();

        let rc =
            unsafe { ffi::sqlite3_prepare_v2(raw_db, sql_cstr.as_ptr(), -1, &mut stmt, &mut tail) };

        if rc != ffi::SQLITE_OK {
            let errmsg = unsafe {
                let msg = ffi::sqlite3_errmsg(raw_db);
                if msg.is_null() {
                    "Unknown error".to_string()
                } else {
                    CStr::from_ptr(msg).to_string_lossy().into_owned()
                }
            };
            return Err(Error::ModuleError(format!(
                "Failed to query schema for '{}': {}",
                shadow_table, errmsg
            )));
        }

        struct StmtGuard(*mut ffi::sqlite3_stmt);
        impl Drop for StmtGuard {
            fn drop(&mut self) {
                unsafe {
                    ffi::sqlite3_finalize(self.0);
                }
            }
        }
        let _guard = StmtGuard(stmt);

        let mut columns = Vec::new();
        let mut column_defaults = Vec::new();
        let mut schema_parts = Vec::new();
        let mut rowid_column_index = None;

        loop {
            let rc = unsafe { ffi::sqlite3_step(stmt) };
            match rc {
                ffi::SQLITE_ROW => {
                    // PRAGMA table_info columns: cid, name, type, notnull, dflt_value, pk
                    let name_ptr = unsafe { ffi::sqlite3_column_text(stmt, 1) };
                    let type_ptr = unsafe { ffi::sqlite3_column_text(stmt, 2) };
                    let notnull = unsafe { ffi::sqlite3_column_int(stmt, 3) };
                    let default_ptr = unsafe { ffi::sqlite3_column_text(stmt, 4) };
                    let pk = unsafe { ffi::sqlite3_column_int(stmt, 5) };

                    let name = if name_ptr.is_null() {
                        String::new()
                    } else {
                        unsafe { CStr::from_ptr(name_ptr as *const i8) }
                            .to_string_lossy()
                            .into_owned()
                    };

                    let col_type = if type_ptr.is_null() {
                        String::new()
                    } else {
                        unsafe { CStr::from_ptr(type_ptr as *const i8) }
                            .to_string_lossy()
                            .into_owned()
                    };
                    let default_value = if default_ptr.is_null() {
                        None
                    } else {
                        Some(
                            unsafe { CStr::from_ptr(default_ptr as *const i8) }
                                .to_string_lossy()
                                .into_owned(),
                        )
                    };

                    if pk > 0 && col_type.eq_ignore_ascii_case("INTEGER") {
                        rowid_column_index = Some(columns.len());
                    }

                    let mut schema_part = if pk > 0 {
                        format!("{} {} PRIMARY KEY", quote_identifier(&name), col_type)
                    } else {
                        format!("{} {}", quote_identifier(&name), col_type)
                    };
                    if notnull != 0 {
                        schema_part.push_str(" NOT NULL");
                    }
                    column_defaults.push(default_value.clone());

                    if let Some(default_value) = default_value {
                        schema_part.push_str(" DEFAULT ");
                        schema_part.push_str(&vtab_default_sql(&default_value));
                    }
                    schema_parts.push(schema_part);

                    columns.push(name);
                }
                ffi::SQLITE_DONE => break,
                _ => {
                    let errmsg = unsafe {
                        let msg = ffi::sqlite3_errmsg(raw_db);
                        if msg.is_null() {
                            "Unknown error".to_string()
                        } else {
                            CStr::from_ptr(msg).to_string_lossy().into_owned()
                        }
                    };
                    return Err(Error::ModuleError(format!(
                        "Failed to read schema for '{}': {}",
                        shadow_table, errmsg
                    )));
                }
            }
        }

        if columns.is_empty() {
            return Err(Error::ModuleError(format!(
                "Shadow table '{}' not found or has no columns",
                shadow_table
            )));
        }

        let schema = format!("CREATE TABLE x({})", schema_parts.join(", "));

        // Configure vtab
        db.config(VTabConfig::DirectOnly)?;
        unsafe {
            check_sqlite(
                ffi::sqlite3_vtab_config(raw_db, ffi::SQLITE_VTAB_CONSTRAINT_SUPPORT, 1),
                raw_db,
                "Failed to enable virtual-table constraint support",
            )?;
        }

        Ok((
            schema,
            ReactiveVTab {
                table_name,
                shadow_table,
                columns,
                column_defaults,
                rowid_column_index,
                db: vtab_aux.db,
                revision_store: Rc::clone(&vtab_aux.revision_store),
            },
        ))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        // For now, always do a full table scan
        // Future: pass through constraints to shadow table
        info.set_estimated_cost(1_000_000.0);
        info.set_estimated_rows(1000);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<Self::Cursor> {
        Ok(ReactiveVTabCursor::new(self))
    }
}

impl<'vtab> CreateVTab<'vtab> for ReactiveVTab {
    const KIND: VTabKind = VTabKind::Default;
}

impl<'vtab> UpdateVTab<'vtab> for ReactiveVTab {
    fn delete(&mut self, arg: ValueRef<'_>) -> Result<()> {
        let rowid = arg
            .as_i64()
            .map_err(|err| Error::ModuleError(format!("Invalid delete rowid: {}", err)))?;
        self.delete_row(rowid)
    }

    fn insert(&mut self, args: &Inserts<'_>) -> Result<i64> {
        let values = self.column_values(args)?;
        let explicit_nulls = explicit_bound_null_columns(args, self.columns.len())?;
        let requested_rowid = match args.get::<Value>(1)? {
            Value::Integer(rowid) => Some(rowid),
            Value::Null => None,
            _ => {
                return Err(Error::ModuleError(
                    "Invalid insert rowid argument".to_string(),
                ));
            }
        };
        let conflict_mode = unsafe { args.on_conflict(self.db) };
        self.insert_row(requested_rowid, &values, &explicit_nulls, conflict_mode)
    }

    fn update(&mut self, args: &Updates<'_>) -> Result<()> {
        let old_rowid = args.get::<Value>(0).and_then(|value| match value {
            Value::Integer(rowid) => Ok(rowid),
            _ => Err(Error::ModuleError("Invalid update rowid".to_string())),
        })?;
        let new_rowid = match args.get::<Value>(1)? {
            Value::Integer(rowid) => rowid,
            Value::Null => old_rowid,
            _ => return Err(Error::ModuleError("Invalid update rowid".to_string())),
        };
        if new_rowid != old_rowid {
            return Err(Error::ModuleError(format!(
                "Reactive virtual table '{}' does not support rowid updates",
                self.table_name
            )));
        }
        let values = self.column_values(args)?;
        let no_change = update_no_change_columns(args, self.columns.len());
        if let Some(rowid_column_index) = self.rowid_column_index {
            if !no_change[rowid_column_index] {
                match values.get(rowid_column_index) {
                    Some(Value::Integer(new_rowid)) if *new_rowid == old_rowid => {}
                    Some(_) => {
                        return Err(Error::ModuleError(format!(
                            "Reactive virtual table '{}' does not support rowid updates",
                            self.table_name
                        )));
                    }
                    None => {}
                }
            }
        }
        let conflict_mode = unsafe { args.on_conflict(self.db) };
        self.update_row(old_rowid, &values, &no_change, conflict_mode)
    }
}

unsafe impl<'vtab> VTabCursor for ReactiveVTabCursor<'vtab> {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _args: &Filters<'_>,
    ) -> Result<()> {
        // Read the persisted rowset counter so long-lived connections observe
        // writes committed by other processes before recording a new trace.
        let counter = rowset_counter_raw(self.vtab.db, &self.vtab.shadow_table)?;
        self.vtab
            .revision_store
            .cache_rowset_counter(&self.vtab.shadow_table, counter);

        // Record membership observation with persistent counter
        TraceScope::record(
            table_membership_cell_id(&self.vtab.shadow_table),
            counter_revision(counter),
        );

        // Fetch rows after recording the membership counter so the trace never
        // pairs pre-counter rows with a newer membership revision.
        self.fetch_rows()?;

        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.position += 1;
        if self.position < self.rows.len() {
            self.current_row = Some(self.rows[self.position].clone());
        } else {
            self.current_row = None;
        }
        Ok(())
    }

    fn eof(&self) -> bool {
        self.position >= self.rows.len()
    }

    fn column(&self, ctx: &mut Context, col: c_int) -> Result<()> {
        if ctx.no_change() {
            return Ok(());
        }

        // Record content observation
        if let Some(ref row) = self.current_row {
            let rowid = if let Some(rusqlite::types::Value::Integer(id)) = row.first() {
                *id
            } else {
                0
            };

            // Compute content hash for this row
            let digest = compute_row_digest(row);

            TraceScope::record(
                row_cell_id(&self.vtab.shadow_table, rowid),
                digest_revision(digest),
            );

            // Return the column value
            // col + 1 because row[0] is rowid
            if let Some(value) = row.get((col + 1) as usize) {
                ctx.set_result(value)?;
            }
        }
        Ok(())
    }

    fn rowid(&self) -> Result<i64> {
        if let Some(ref row) = self.current_row {
            if let Some(rusqlite::types::Value::Integer(id)) = row.first() {
                return Ok(*id);
            }
        }
        Ok(0)
    }
}

/// Register the reactive module with a connection.
///
/// # Arguments
///
/// * `conn` - The database connection
/// * `name` - The module name (e.g., "reactive")
/// * `revision_store` - The revision store for tracking row and rowset revisions
///
/// # Safety
/// The connection must outlive any virtual tables created with this module.
/// This is guaranteed by SQLite's lifecycle management.
pub fn register_module(
    conn: &Connection,
    name: &str,
    revision_store: Rc<RevisionStore>,
) -> Result<(), DatabaseError> {
    let module = with_reactive_shadow_names(update_module::<ReactiveVTab>());

    // Create aux data with raw sqlite3 handle and revision store
    // Safety: The connection pointer is valid for the lifetime of the module,
    // which is tied to the connection itself.
    // We use the raw handle to bypass rusqlite's RefCell during vtab callbacks.
    let db = unsafe { conn.handle() };
    let aux = VTabAux { db, revision_store };
    conn.create_module(name, module, Some(aux))?;

    Ok(())
}

fn insert_keyword(conflict_mode: ConflictMode) -> &'static str {
    match conflict_mode {
        ConflictMode::Rollback => "INSERT OR ROLLBACK",
        ConflictMode::Ignore => "INSERT OR IGNORE",
        ConflictMode::Fail => "INSERT OR FAIL",
        ConflictMode::Abort => "INSERT OR ABORT",
        ConflictMode::Replace => "INSERT OR REPLACE",
        _ => "INSERT",
    }
}

fn conflict_clause(conflict_mode: ConflictMode) -> &'static str {
    match conflict_mode {
        ConflictMode::Rollback => "OR ROLLBACK",
        ConflictMode::Ignore => "OR IGNORE",
        ConflictMode::Fail => "OR FAIL",
        ConflictMode::Abort => "OR ABORT",
        ConflictMode::Replace => "OR REPLACE",
        _ => "",
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn explicit_bound_null_columns(args: &Values<'_>, column_count: usize) -> Result<Vec<bool>> {
    let raw_values = raw_sqlite_values(args)?;
    let mut explicit_nulls = Vec::with_capacity(column_count);
    for idx in 0..column_count {
        let arg_index = idx + 2;
        let value = args.get::<Value>(arg_index)?;
        let from_bind = unsafe { ffi::sqlite3_value_frombind(raw_values[arg_index]) != 0 };
        explicit_nulls.push(matches!(value, Value::Null) && from_bind);
    }
    Ok(explicit_nulls)
}

fn raw_sqlite_values<'a>(args: &'a Values<'a>) -> Result<&'a [*mut ffi::sqlite3_value]> {
    if args.is_empty() {
        return Ok(&[]);
    }

    // rusqlite does not currently expose sqlite3_value_frombind through Values.
    // Values is a one-field wrapper around the SQLite argv slice; this remains
    // local to insert handling so bound NULLs are not mistaken for omitted
    // defaulted columns.
    let raw_values =
        unsafe { *(args as *const Values<'_> as *const &'a [*mut ffi::sqlite3_value]) };
    if raw_values.len() != args.len() {
        return Err(Error::ModuleError(
            "Unexpected virtual table argument layout".to_string(),
        ));
    }
    Ok(raw_values)
}

fn update_no_change_columns(args: &Updates<'_>, column_count: usize) -> Vec<bool> {
    (0..column_count)
        .map(|idx| args.no_change(idx + 2))
        .collect()
}

fn vtab_default_sql(default_value: &str) -> String {
    let trimmed = default_value.trim();
    if trimmed.contains('(') && !trimmed.starts_with('(') {
        format!("({trimmed})")
    } else {
        trimmed.to_string()
    }
}

fn ignored_insert_error(table_name: &str) -> Error {
    Error::SqliteFailure(
        ffi::Error::new(ffi::SQLITE_CONSTRAINT_VTAB),
        Some(format!(
            "Reactive virtual table '{}' ignored insert due to conflict",
            table_name
        )),
    )
}

fn check_sqlite(rc: c_int, db: *mut ffi::sqlite3, prefix: &str) -> Result<()> {
    if rc == ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(sqlite_error(db, prefix))
    }
}

fn execute_raw(db: *mut ffi::sqlite3, sql: &str, params: &[Value]) -> Result<usize> {
    let stmt = prepare_raw(db, sql)?;
    let _guard = StmtGuard(stmt);
    bind_values(db, stmt, params)?;

    let rc = unsafe { ffi::sqlite3_step(stmt) };
    if rc != ffi::SQLITE_DONE {
        return Err(sqlite_error(db, "Failed to execute statement"));
    }
    Ok(unsafe { ffi::sqlite3_changes(db) as usize })
}

fn row_counts(db: *mut ffi::sqlite3) -> Result<Vec<(String, i64)>> {
    REACTIVE_SHADOW_TABLES
        .iter()
        .map(|table| Ok(((*table).to_string(), table_row_count(db, table)?)))
        .collect()
}

fn table_row_count(db: *mut ffi::sqlite3, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(table));
    let row = query_row_raw(db, &sql, &[], 1)?
        .ok_or_else(|| Error::ModuleError(format!("Could not read row count for '{}'", table)))?;
    match row.first() {
        Some(Value::Integer(count)) => Ok(*count),
        _ => Err(Error::ModuleError(format!(
            "Invalid row count for '{}'",
            table
        ))),
    }
}

fn rowset_counter_raw(db: *mut ffi::sqlite3, table: &str) -> Result<u64> {
    let row = query_row_raw(
        db,
        "SELECT counter FROM rowset_revisions WHERE table_name = ?",
        &[Value::Text(table.to_string())],
        1,
    )?;
    match row.and_then(|row| row.into_iter().next()) {
        Some(Value::Integer(counter)) => Ok(counter as u64),
        Some(_) => Err(Error::ModuleError(format!(
            "Invalid rowset counter for '{}'",
            table
        ))),
        None => Ok(0),
    }
}

fn refresh_row_revision(
    db: *mut ffi::sqlite3,
    revision_store: &RevisionStore,
    table: &str,
    rowid: i64,
) -> Result<()> {
    let sql = format!(
        "SELECT rowid, * FROM {} WHERE rowid = ?",
        quote_identifier(table)
    );
    let mut rows = query_all_raw(db, &sql, &[Value::Integer(rowid)])?;
    let Some(row) = rows.pop() else {
        clear_row_revision(db, revision_store, table, rowid)?;
        return Ok(());
    };

    let digest = compute_row_digest(&row);
    let rev_table = table.replace("_data", "_rev");
    execute_raw(
        db,
        &format!(
            "INSERT OR REPLACE INTO {} (rowid, digest) VALUES (?, ?)",
            quote_identifier(&rev_table)
        ),
        &[Value::Integer(rowid), Value::Blob(digest.to_vec())],
    )?;
    revision_store.cache_row_digest(table, rowid, digest);
    Ok(())
}

fn clear_row_revision(
    db: *mut ffi::sqlite3,
    revision_store: &RevisionStore,
    table: &str,
    rowid: i64,
) -> Result<()> {
    let rev_table = table.replace("_data", "_rev");
    execute_raw(
        db,
        &format!(
            "DELETE FROM {} WHERE rowid = ?",
            quote_identifier(&rev_table)
        ),
        &[Value::Integer(rowid)],
    )?;
    revision_store.uncache_row_digest(table, rowid);
    Ok(())
}

fn clear_stale_row_revisions(
    db: *mut ffi::sqlite3,
    revision_store: &RevisionStore,
    table: &str,
) -> Result<usize> {
    let rev_table = table.replace("_data", "_rev");
    let removed = execute_raw(
        db,
        &format!(
            "DELETE FROM {} WHERE NOT EXISTS (SELECT 1 FROM {} WHERE {}.rowid = {}.rowid)",
            quote_identifier(&rev_table),
            quote_identifier(table),
            quote_identifier(table),
            quote_identifier(&rev_table)
        ),
        &[],
    )?;
    if removed > 0 {
        revision_store.clear_table_row_cache(table);
    }

    Ok(removed)
}

fn query_row_raw(
    db: *mut ffi::sqlite3,
    sql: &str,
    params: &[Value],
    column_count: usize,
) -> Result<Option<Vec<Value>>> {
    let stmt = prepare_raw(db, sql)?;
    let _guard = StmtGuard(stmt);
    bind_values(db, stmt, params)?;

    let rc = unsafe { ffi::sqlite3_step(stmt) };
    match rc {
        ffi::SQLITE_ROW => {
            let mut values = Vec::with_capacity(column_count);
            for col in 0..column_count {
                values.push(column_value(stmt, col as c_int));
            }
            Ok(Some(values))
        }
        ffi::SQLITE_DONE => Ok(None),
        _ => Err(sqlite_error(db, "Failed to query row")),
    }
}

fn query_all_raw(db: *mut ffi::sqlite3, sql: &str, params: &[Value]) -> Result<Vec<Vec<Value>>> {
    let stmt = prepare_raw(db, sql)?;
    let _guard = StmtGuard(stmt);
    bind_values(db, stmt, params)?;

    let column_count = unsafe { ffi::sqlite3_column_count(stmt) } as usize;
    let mut rows = Vec::new();
    loop {
        let rc = unsafe { ffi::sqlite3_step(stmt) };
        match rc {
            ffi::SQLITE_ROW => {
                let mut values = Vec::with_capacity(column_count);
                for col in 0..column_count {
                    values.push(column_value(stmt, col as c_int));
                }
                rows.push(values);
            }
            ffi::SQLITE_DONE => return Ok(rows),
            _ => return Err(sqlite_error(db, "Failed to query rows")),
        }
    }
}

fn prepare_raw(db: *mut ffi::sqlite3, sql: &str) -> Result<*mut ffi::sqlite3_stmt> {
    let sql_cstr =
        CString::new(sql).map_err(|e| Error::ModuleError(format!("Invalid SQL: {}", e)))?;
    let mut stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();
    let mut tail: *const i8 = ptr::null();
    let rc = unsafe { ffi::sqlite3_prepare_v2(db, sql_cstr.as_ptr(), -1, &mut stmt, &mut tail) };
    if rc != ffi::SQLITE_OK {
        return Err(sqlite_error(db, "Failed to prepare statement"));
    }
    Ok(stmt)
}

fn bind_values(
    db: *mut ffi::sqlite3,
    stmt: *mut ffi::sqlite3_stmt,
    params: &[Value],
) -> Result<()> {
    for (idx, value) in params.iter().enumerate() {
        let rc = match value {
            Value::Null => unsafe { ffi::sqlite3_bind_null(stmt, (idx + 1) as c_int) },
            Value::Integer(value) => unsafe {
                ffi::sqlite3_bind_int64(stmt, (idx + 1) as c_int, *value)
            },
            Value::Real(value) => unsafe {
                ffi::sqlite3_bind_double(stmt, (idx + 1) as c_int, *value)
            },
            Value::Text(value) => unsafe {
                ffi::sqlite3_bind_text(
                    stmt,
                    (idx + 1) as c_int,
                    value.as_ptr() as *const i8,
                    value.len() as c_int,
                    sqlite_transient(),
                )
            },
            Value::Blob(value) => unsafe {
                ffi::sqlite3_bind_blob(
                    stmt,
                    (idx + 1) as c_int,
                    value.as_ptr() as *const c_void,
                    value.len() as c_int,
                    sqlite_transient(),
                )
            },
        };
        if rc != ffi::SQLITE_OK {
            return Err(sqlite_error(db, "Failed to bind statement parameter"));
        }
    }
    Ok(())
}

fn column_value(stmt: *mut ffi::sqlite3_stmt, col: c_int) -> Value {
    let col_type = unsafe { ffi::sqlite3_column_type(stmt, col) };
    match col_type {
        ffi::SQLITE_INTEGER => Value::Integer(unsafe { ffi::sqlite3_column_int64(stmt, col) }),
        ffi::SQLITE_FLOAT => Value::Real(unsafe { ffi::sqlite3_column_double(stmt, col) }),
        ffi::SQLITE_TEXT => {
            let ptr = unsafe { ffi::sqlite3_column_text(stmt, col) };
            if ptr.is_null() {
                Value::Null
            } else {
                let len = unsafe { ffi::sqlite3_column_bytes(stmt, col) } as usize;
                let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
                Value::Text(String::from_utf8_lossy(bytes).into_owned())
            }
        }
        ffi::SQLITE_BLOB => {
            let ptr = unsafe { ffi::sqlite3_column_blob(stmt, col) };
            let len = unsafe { ffi::sqlite3_column_bytes(stmt, col) } as usize;
            if ptr.is_null() || len == 0 {
                Value::Blob(vec![])
            } else {
                Value::Blob(unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }.to_vec())
            }
        }
        _ => Value::Null,
    }
}

fn sqlite_error(db: *mut ffi::sqlite3, prefix: &str) -> Error {
    let errmsg = unsafe {
        let msg = ffi::sqlite3_errmsg(db);
        if msg.is_null() {
            "Unknown error".to_string()
        } else {
            CStr::from_ptr(msg).to_string_lossy().into_owned()
        }
    };
    Error::ModuleError(format!("{}: {}", prefix, errmsg))
}

struct StmtGuard(*mut ffi::sqlite3_stmt);

impl Drop for StmtGuard {
    fn drop(&mut self) {
        unsafe {
            ffi::sqlite3_finalize(self.0);
        }
    }
}

fn sqlite_transient() -> ffi::sqlite3_destructor_type {
    unsafe { std::mem::transmute::<isize, ffi::sqlite3_destructor_type>(-1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_row_digest() {
        use rusqlite::types::Value;

        let row1 = vec![Value::Integer(1), Value::Text("hello".to_string())];
        let row2 = vec![Value::Integer(1), Value::Text("hello".to_string())];
        let row3 = vec![Value::Integer(1), Value::Text("world".to_string())];

        let digest1 = compute_row_digest(&row1);
        let digest2 = compute_row_digest(&row2);
        let digest3 = compute_row_digest(&row3);

        // Same content should produce same digest
        assert_eq!(digest1, digest2);
        // Different content should produce different digest
        assert_ne!(digest1, digest3);
    }

    #[test]
    fn test_compute_row_digest_length_prefixes_variable_values() {
        use rusqlite::types::Value;

        let row1 = vec![
            Value::Text(String::new()),
            Value::Text("a\x03b".to_string()),
        ];
        let row2 = vec![
            Value::Text("\x03a".to_string()),
            Value::Text("b".to_string()),
        ];

        assert_ne!(compute_row_digest(&row1), compute_row_digest(&row2));
    }

    #[test]
    fn test_compute_row_digest_all_types() {
        use rusqlite::types::Value;

        let row = vec![
            Value::Null,
            Value::Integer(42),
            Value::Real(3.14),
            Value::Text("test".to_string()),
            Value::Blob(vec![1, 2, 3]),
        ];

        let digest = compute_row_digest(&row);
        assert_eq!(digest.len(), 32);
    }
}
