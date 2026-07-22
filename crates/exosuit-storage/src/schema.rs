//! Database wrapper and error types

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OptionalExtension};
use std::sync::Arc;
use thiserror::Error;

use crate::revisions::RevisionStore;
use crate::vtab::register_reactive_module;

/// Errors that can occur during database operations.
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Request database scope error: {0}")]
    RequestScope(String),
}

/// Shadow tables that need reactive virtual table wrappers.
/// Each entry is (virtual_table_name, shadow_table_name).
pub(crate) const REACTIVE_TABLES: &[(&str, &str)] = &[
    ("epochs", "epochs_data"),
    ("phases", "phases_data"),
    ("goals", "goals_data"),
    ("tasks", "tasks_data"),
    ("phase_rfcs", "phase_rfcs_data"),
    ("ideas", "ideas_data"),
    ("inbox", "inbox_data"),
    ("rfcs", "rfcs_data"),
    ("rfc_workspace_snapshots", "rfc_workspace_snapshots_data"),
    (
        "rfc_workspace_observations",
        "rfc_workspace_observations_data",
    ),
    (
        "rfc_workspace_diagnostics",
        "rfc_workspace_diagnostics_data",
    ),
    ("workspace_active_phase", "workspace_active_phase_data"),
    ("phase_ownership", "phase_ownership_data"),
];

/// A wrapper around a SQLite connection with schema guarantees.
///
/// This type ensures that the database has been migrated to the latest schema
/// before any operations are performed.
pub struct Database {
    conn: Connection,
    /// Revision store for tracking row and rowset revisions.
    /// Shared with virtual tables via Arc.
    revision_store: Arc<RevisionStore>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("conn", &"<Connection>")
            .finish()
    }
}

impl Database {
    /// Create a new Database wrapper around an already-migrated connection.
    ///
    /// This is called by `open_database` and `open_memory_database` after
    /// migrations have been run. Sets up:
    /// 1. Defensive mode to protect shadow tables
    /// 2. RevisionStore for tracking row/rowset revisions
    /// 3. Reactive virtual tables wrapping all shadow tables
    pub(crate) fn new(conn: Connection) -> Result<Self, DatabaseError> {
        // Create revision store first (needs connection for persistence)
        let revision_store = Arc::new(RevisionStore::new(&conn)?);

        // Register the reactive module with the revision store
        register_reactive_module(&conn, "reactive", Arc::clone(&revision_store))?;

        // Ensure reactive virtual tables exist and still match their shadow
        // tables. Opening a database must not rewrite schema on every
        // connection: direct CLI, daemon, MCP, and extension reads can overlap,
        // and read paths should not touch schema locks when the vtabs are
        // already current. After migrations add shadow columns, however, stale
        // vtabs need a one-time rebuild so readers see the new schema.
        for (vtab_name, shadow_name) in REACTIVE_TABLES {
            if !table_exists(&conn, shadow_name)? {
                continue;
            }
            if !table_exists(&conn, vtab_name)? {
                create_reactive_table(&conn, vtab_name, shadow_name)?;
            } else if reactive_table_is_stale(&conn, vtab_name, shadow_name)? {
                conn.execute(&format!("DROP TABLE {}", vtab_name), [])?;
                create_reactive_table(&conn, vtab_name, shadow_name)?;
            }
        }

        for (_, shadow_name) in REACTIVE_TABLES {
            if table_exists(&conn, shadow_name)?
                && table_exists(&conn, &rev_table_name(shadow_name))?
            {
                revision_store.backfill_row_digests(&conn, shadow_name)?;
            }
        }

        // Enable defensive mode to protect shadow tables from direct modification.
        // Per RFC 10165: "SQLITE_DBCONFIG_DEFENSIVE makes shadow tables read-only to ordinary SQL"
        //
        // PRAGMA trusted_schema = OFF: Prevents untrusted SQL from using virtual tables
        // SQLITE_DBCONFIG_DEFENSIVE: Makes shadow tables read-only to ordinary SQL
        //
        // NOTE: We enable defensive mode AFTER creating virtual tables, because
        // CREATE VIRTUAL TABLE needs to access the shadow table schema.
        conn.execute_batch("PRAGMA trusted_schema = OFF;")?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;

        Ok(Self {
            conn,
            revision_store,
        })
    }

    /// Get a reference to the underlying connection.
    ///
    /// This is useful for running raw queries during development and testing.
    /// Production code should use typed methods instead.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Get a mutable reference to the underlying connection.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Get a reference to the revision store.
    ///
    /// This is used for trace validation and testing.
    pub fn revision_store(&self) -> &Arc<RevisionStore> {
        &self.revision_store
    }
}

fn rev_table_name(shadow_name: &str) -> String {
    shadow_name.replace("_data", "_rev")
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, DatabaseError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn create_reactive_table(
    conn: &Connection,
    vtab_name: &str,
    shadow_name: &str,
) -> Result<(), DatabaseError> {
    conn.execute(
        &format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS {} USING reactive({})",
            vtab_name, shadow_name
        ),
        [],
    )?;
    Ok(())
}

fn reactive_table_is_stale(
    conn: &Connection,
    vtab_name: &str,
    shadow_name: &str,
) -> Result<bool, DatabaseError> {
    Ok(table_columns(conn, vtab_name)? != table_columns(conn, shadow_name)?)
}

fn table_columns(conn: &Connection, name: &str) -> Result<Vec<String>, DatabaseError> {
    let sql = format!("PRAGMA table_info({})", name);
    let columns = conn
        .prepare(&sql)?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_display() {
        let err = DatabaseError::NotFound {
            entity_type: "goal".to_string(),
            id: "abc123".to_string(),
        };
        assert_eq!(err.to_string(), "Entity not found: goal with id abc123");
    }
}
