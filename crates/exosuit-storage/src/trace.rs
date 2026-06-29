//! Thread-local trace recording for reactive observation tracking.
//!
//! This module implements the TraceScope infrastructure from RFC 10165.
//! Virtual table callbacks (xFilter, xColumn, xUpdate) record observations
//! into the current trace scope, which can later be used for invalidation.
//!
//! Types (`CellId`, `Revision`, `Trace`, `TraceEntry`) come from
//! `exosuit-reactivity-core`. This module provides the SQLite-specific
//! `TraceScope` and convenience constructors for building core types
//! from SQLite-level concepts (table names, rowids, BLAKE3 digests).

use exosuit_reactivity_core::{CellId, Revision, Trace, TraceEntry};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Convenience: build a `CellId` for a specific row in a shadow table.
pub fn row_cell_id(table: &str, rowid: i64) -> CellId {
    CellId::new(table, rowid.to_string())
}

/// Convenience: build a `CellId` for table membership (which rows exist).
pub fn table_membership_cell_id(table: &str) -> CellId {
    CellId::root(table)
}

/// Convenience: build a `Revision::Disk` from a BLAKE3 digest.
pub fn digest_revision(hash: [u8; 32]) -> Revision {
    Revision::disk(hex::encode(hash))
}

/// Convenience: build a `Revision::Counter` from a persistent rowset counter.
pub fn counter_revision(counter: u64) -> Revision {
    Revision::counter(counter)
}

thread_local! {
    static CURRENT_SCOPE: RefCell<Option<BTreeSet<TraceEntry>>> = const { RefCell::new(None) };
}

/// Thread-local trace recording scope.
///
/// Use `TraceScope::run()` to execute a computation while recording
/// all observations made by virtual table callbacks.
pub struct TraceScope;

impl TraceScope {
    /// Run a computation and capture its trace.
    ///
    /// Any calls to `TraceScope::record()` during the execution of `f`
    /// will be captured in the returned trace.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (result, trace) = TraceScope::run(|| {
    ///     db.query("SELECT * FROM epochs WHERE id = 1")
    /// });
    /// // trace now contains all observations made during the query
    /// ```
    pub fn run<F, R>(f: F) -> (R, Trace)
    where
        F: FnOnce() -> R,
    {
        CURRENT_SCOPE.with(|scope| {
            // Save any existing scope (for nested calls)
            let previous = scope.borrow_mut().take();

            // Install a fresh scope
            *scope.borrow_mut() = Some(BTreeSet::new());

            // Run the computation
            let result = f();

            // Extract the trace and restore previous scope
            let entries = scope.borrow_mut().take().unwrap_or_default();
            *scope.borrow_mut() = previous;

            let trace = Trace {
                dependencies: entries,
                resources: Vec::new(),
            };

            (result, trace)
        })
    }

    /// Record an observation into the current trace.
    ///
    /// If no trace scope is active, this is a no-op.
    /// This is called by virtual table callbacks (xFilter, xColumn).
    pub fn record(cell_id: CellId, revision: Revision) {
        CURRENT_SCOPE.with(|scope| {
            if let Some(ref mut entries) = *scope.borrow_mut() {
                entries.insert(TraceEntry { cell_id, revision });
            }
        });
    }

    /// Check if a trace scope is currently active.
    pub fn is_active() -> bool {
        CURRENT_SCOPE.with(|scope| scope.borrow().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_scope_basic() {
        let (result, trace) = TraceScope::run(|| {
            TraceScope::record(row_cell_id("epochs_data", 1), digest_revision([0u8; 32]));
            42
        });

        assert_eq!(result, 42);
        assert_eq!(trace.dependencies.len(), 1);
    }

    #[test]
    fn test_trace_scope_no_active_scope() {
        // Recording without an active scope should be a no-op
        TraceScope::record(row_cell_id("epochs_data", 1), digest_revision([0u8; 32]));
        // No panic, no effect
        assert!(!TraceScope::is_active());
    }

    #[test]
    fn test_trace_scope_nested() {
        let (outer_result, outer_trace) = TraceScope::run(|| {
            TraceScope::record(row_cell_id("epochs_data", 1), digest_revision([1u8; 32]));

            let (inner_result, inner_trace) = TraceScope::run(|| {
                TraceScope::record(row_cell_id("phases_data", 2), digest_revision([2u8; 32]));
                "inner"
            });

            assert_eq!(inner_result, "inner");
            assert_eq!(inner_trace.dependencies.len(), 1);

            // Record another observation in outer scope
            TraceScope::record(table_membership_cell_id("goals_data"), counter_revision(5));

            "outer"
        });

        assert_eq!(outer_result, "outer");
        // Outer trace should have 2 entries (not including inner)
        assert_eq!(outer_trace.dependencies.len(), 2);
    }

    #[test]
    fn test_trace_deduplication() {
        let (_, trace) = TraceScope::run(|| {
            // Record the same observation twice
            TraceScope::record(row_cell_id("epochs_data", 1), digest_revision([0u8; 32]));
            TraceScope::record(row_cell_id("epochs_data", 1), digest_revision([0u8; 32]));
        });

        // Should be deduplicated to 1 entry
        assert_eq!(trace.dependencies.len(), 1);
    }

    #[test]
    fn test_cell_id_constructors() {
        let row = row_cell_id("epochs_data", 42);
        assert_eq!(row.source_id, "epochs_data");
        assert_eq!(row.pointer, "42");

        let table_set = table_membership_cell_id("phases_data");
        assert_eq!(table_set.source_id, "phases_data");
        assert_eq!(table_set.pointer, "");
    }
}
