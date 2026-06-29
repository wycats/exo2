//! Virtual table layer for reactive tracing.
//!
//! This module implements RFC 10165's reactive virtual table architecture.
//! Virtual tables wrap shadow tables (`*_data`) and intercept all access
//! to record observations into TraceScope.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌─────────────────┐
//! │  epochs (vtab)  │────▶│  epochs_data    │
//! └─────────────────┘     └─────────────────┘
//!         │
//!         ▼
//!    TraceScope::record()
//! ```
//!
//! # Callbacks
//!
//! - `xFilter`: Records Membership observations (which rows exist)
//! - `xColumn`: Records Content observations (column values)
//! - `xUpdate`: Intercepts writes for invalidation
//! - `xShadowName`: Protects shadow tables from direct access

mod reactive;
mod shadow;

pub use reactive::ReactiveVTab;

// These are used internally by the reactive virtual table layer.
#[allow(unused_imports)]
pub(crate) use reactive::{ReactiveModule, ReactiveVTabCursor};
#[allow(unused_imports)]
pub(crate) use shadow::is_shadow_name;

use rusqlite::Connection;
use std::rc::Rc;

use crate::revisions::RevisionStore;
use crate::DatabaseError;

/// Register a reactive virtual table module for a shadow table.
///
/// This creates a virtual table that wraps the shadow table and records
/// all access into TraceScope.
///
/// # Arguments
///
/// * `conn` - The database connection
/// * `module_name` - The name for the virtual table module (e.g., "reactive")
/// * `revision_store` - The revision store for tracking row and rowset revisions
///
/// # Example
///
/// ```ignore
/// let store = Rc::new(RevisionStore::new(&conn)?);
/// register_reactive_module(&conn, "reactive", Rc::clone(&store))?;
/// conn.execute("CREATE VIRTUAL TABLE epochs USING reactive(epochs_data)", [])?;
/// ```
pub fn register_reactive_module(
    conn: &Connection,
    module_name: &str,
    revision_store: Rc<RevisionStore>,
) -> Result<(), DatabaseError> {
    reactive::register_module(conn, module_name, revision_store)?;
    Ok(())
}
