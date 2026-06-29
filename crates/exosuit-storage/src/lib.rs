//! SQLite storage layer for Exosuit
//!
//! This crate provides the SQLite-backed storage for the Exosuit project state model.
//! It implements the schema defined in RFC 10176 (Project State Model) with reactive
//! tracing support from RFC 10165.

pub mod dump;
mod functions;
pub mod maintenance;
mod migrations;
mod revisions;
mod schema;
mod trace;
mod vtab;

pub use dump::{dump_tables, import_tables, DumpError, ImportError, TABLE_ORDER};
pub use exosuit_reactivity_core::{CellId, Revision, Trace, TraceEntry};
pub use functions::register_functions;
pub use maintenance::{
    maintain_database, storage_maintenance_stats, AutoVacuumMode, StorageMaintenanceOptions,
    StorageMaintenanceReport, StorageMaintenanceStats, WalCheckpointReport,
    DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET,
};
pub use migrations::run_migrations;
pub use revisions::{RevisionStore, SqliteStateProvider};
pub use schema::{Database, DatabaseError};
pub use trace::{
    counter_revision, digest_revision, row_cell_id, table_membership_cell_id, TraceScope,
};
pub use vtab::{register_reactive_module, ReactiveVTab};

pub use rusqlite::{self, params, Connection, OptionalExtension, Row};
use std::path::Path;

/// Open or create a database at the given path, running migrations as needed.
///
/// Enables WAL journal mode for concurrent read/write access (the daemon
/// handles multiple requests in parallel, each opening its own connection)
/// and sets a 5-second busy timeout so transient lock contention doesn't
/// cause immediate failures.
pub fn open_database(path: impl AsRef<Path>) -> Result<Database, DatabaseError> {
    let path = path.as_ref();
    let should_enable_incremental_auto_vacuum = is_new_or_empty_database_file(path);

    let conn = Connection::open(path)?;
    if should_enable_incremental_auto_vacuum {
        conn.pragma_update(None, "auto_vacuum", AutoVacuumMode::Incremental.as_i64())?;
    }
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    run_migrations(&conn)?;
    Database::new(conn)
}

fn is_new_or_empty_database_file(path: &Path) -> bool {
    match path.metadata() {
        Ok(metadata) => metadata.len() == 0,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

/// Create an in-memory database for testing.
pub fn open_memory_database() -> Result<Database, DatabaseError> {
    let conn = Connection::open_in_memory()?;
    run_migrations(&conn)?;
    Database::new(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DefensiveModeGuard<'conn> {
        conn: &'conn Connection,
        restore: bool,
    }

    impl Drop for DefensiveModeGuard<'_> {
        fn drop(&mut self) {
            let _ = self.conn.set_db_config(
                rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE,
                self.restore,
            );
        }
    }

    fn defensive_mode_disabled(conn: &Connection) -> DefensiveModeGuard<'_> {
        let restore = conn
            .db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE)
            .expect("should query defensive config");
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .expect("should disable defensive mode for trusted test setup");
        DefensiveModeGuard { conn, restore }
    }

    #[test]
    fn test_open_memory_database() {
        let db = open_memory_database().expect("should create in-memory database");

        // Verify tables exist by querying sqlite_master
        // Note: After V002 migration, core tables use *_data suffix (shadow table convention)
        let tables: Vec<String> = db
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // Shadow tables (reactive data sources)
        assert!(tables.contains(&"epochs_data".to_string()));
        assert!(tables.contains(&"phases_data".to_string()));
        assert!(tables.contains(&"goals_data".to_string()));
        assert!(tables.contains(&"tasks_data".to_string()));
        assert!(tables.contains(&"phase_rfcs_data".to_string()));
        // Plain tables (not reactive)
        assert!(tables.contains(&"entity_aliases".to_string()));
    }

    #[test]
    fn reopening_database_does_not_rewrite_schema() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        let db = open_database(&db_path).expect("should create database");
        let schema_version = db
            .connection()
            .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
            .expect("read schema version");
        drop(db);

        let db = open_database(&db_path).expect("should reopen database");
        let reopened_schema_version = db
            .connection()
            .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
            .expect("read reopened schema version");

        assert_eq!(reopened_schema_version, schema_version);
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = open_memory_database().expect("should create in-memory database");

        let fk_enabled: i32 = db
            .connection()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();

        assert_eq!(fk_enabled, 1, "foreign keys should be enabled");
    }

    #[test]
    fn test_insert_and_query_epoch() {
        let db = open_memory_database().expect("should create in-memory database");

        db.connection()
            .execute(
                "INSERT INTO epochs (text_id, title, slug) VALUES (?1, ?2, ?3)",
                ["01ABC123", "Test Epoch", "test-epoch"],
            )
            .expect("should insert epoch");

        let title: String = db
            .connection()
            .query_row(
                "SELECT title FROM epochs_data WHERE text_id = ?1",
                ["01ABC123"],
                |row| row.get(0),
            )
            .expect("should query epoch");

        assert_eq!(title, "Test Epoch");
    }

    #[test]
    fn test_cascade_delete() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Insert epoch → phase → goal → task (using shadow table names)
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Epoch 1')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO phases_data (text_id, title, epoch_id) VALUES ('p1', 'Phase 1', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO goals_data (text_id, label, phase_id) VALUES ('g1', 'Goal 1', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tasks_data (text_id, title, goal_id) VALUES ('t1', 'Task 1', 1)",
                [],
            )
            .unwrap();

            // Delete epoch - should cascade to all children
            conn.execute("DELETE FROM epochs_data WHERE text_id = 'e1'", [])
                .unwrap();
        }

        // Verify all children are deleted
        let phase_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM phases_data", [], |row| row.get(0))
            .unwrap();
        let goal_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM goals_data", [], |row| row.get(0))
            .unwrap();
        let task_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM tasks_data", [], |row| row.get(0))
            .unwrap();

        assert_eq!(phase_count, 0, "phases should be deleted");
        assert_eq!(goal_count, 0, "goals should be deleted");
        assert_eq!(task_count, 0, "tasks should be deleted");
    }

    #[test]
    fn test_reactive_vtab_queries_shadow_table() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Virtual tables are now created automatically by Database::new()
        // Insert test data into shadow table
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug) VALUES ('e1', 'First Epoch', 'first')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug) VALUES ('e2', 'Second Epoch', 'second')",
                [],
            )
            .unwrap();
        }

        // Query through the virtual table (created automatically)
        let titles: Vec<String> = conn
            .prepare("SELECT title FROM epochs ORDER BY text_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0], "First Epoch");
        assert_eq!(titles[1], "Second Epoch");
    }

    #[test]
    fn test_reactive_vtab_records_observations() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Virtual tables and revision store are now created automatically
        // Insert test data
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Test Epoch')",
                [],
            )
            .unwrap();
        }

        // Query within a TraceScope (virtual table created automatically)
        let (_, trace) = TraceScope::run(|| {
            let _title: String = conn
                .query_row("SELECT title FROM epochs WHERE text_id = 'e1'", [], |row| {
                    row.get(0)
                })
                .unwrap();
        });

        // Verify observations were recorded
        assert!(
            !trace.dependencies.is_empty(),
            "should have recorded observations"
        );

        // Should have at least a membership observation (pointer is empty)
        let has_membership = trace.entries().any(|e| e.cell_id.pointer.is_empty());
        assert!(has_membership, "should have membership observation");
    }

    #[test]
    fn test_trace_membership_invalidation() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        let store = db.revision_store();

        // Virtual table created automatically (no data yet)
        // Query within a TraceScope - just check membership (no rows)
        let (_, trace) = TraceScope::run(|| {
            let _: Vec<String> = conn
                .prepare("SELECT title FROM epochs")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        });

        // Filter to just membership observations for this test
        let membership_only = Trace {
            dependencies: trace
                .entries()
                .filter(|e| e.cell_id.pointer.is_empty())
                .cloned()
                .collect(),
            resources: Vec::new(),
        };

        // Trace should be valid (nothing changed)
        assert!(
            membership_only.validate(&mut SqliteStateProvider::new(conn, store)),
            "trace should be valid immediately after query"
        );

        // Bump the rowset counter (simulating an INSERT)
        store
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump counter");

        // Now trace should be invalid (membership changed)
        assert!(
            !membership_only.validate(&mut SqliteStateProvider::new(conn, store)),
            "trace should be invalid after membership change"
        );
    }

    #[test]
    fn test_rowset_revision_in_trace() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Virtual table created automatically
        // Query within a TraceScope
        let (_, trace) = TraceScope::run(|| {
            let _: Vec<String> = conn
                .prepare("SELECT title FROM epochs")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        });

        // Should have a membership observation with Disk revision (persistent counter)
        let membership_entry = trace.entries().find(|e| e.cell_id.pointer.is_empty());
        assert!(
            membership_entry.is_some(),
            "should have membership observation"
        );

        let entry = membership_entry.unwrap();
        match &entry.revision {
            Revision::Counter(counter) => {
                // Counter should be 0 (no inserts/deletes yet)
                assert_eq!(*counter, 0);
            }
            _ => panic!("membership observation should have Counter revision"),
        }
    }

    #[test]
    fn reactive_trace_reads_persisted_rowset_counter_before_cache() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        db.revision_store().cache_rowset_counter("epochs_data", 0);
        conn.execute(
            "UPDATE rowset_revisions SET counter = 7 WHERE table_name = 'epochs_data'",
            [],
        )
        .expect("should update persisted rowset counter");

        let (_, trace) = TraceScope::run(|| {
            let _: Vec<String> = conn
                .prepare("SELECT title FROM epochs")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        });

        let membership_entry = trace
            .entries()
            .find(|entry| entry.cell_id.pointer.is_empty())
            .expect("should have membership observation");
        match &membership_entry.revision {
            Revision::Counter(counter) => assert_eq!(*counter, 7),
            _ => panic!("membership observation should have Counter revision"),
        }
    }

    #[test]
    fn reactive_trace_invalidates_across_connections_on_rowset_change() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        let writer = open_database(&db_path).expect("should create writer database");
        let reader = open_database(&db_path).expect("should create reader database");

        writer
            .connection()
            .execute(
                "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
                 VALUES ('e1', 'Epoch One', 'one', 0, '')",
                [],
            )
            .expect("writer insert should succeed");

        reader
            .revision_store()
            .cache_rowset_counter("epochs_data", 0);
        let (_, trace) = TraceScope::run(|| {
            reader
                .connection()
                .prepare("SELECT title FROM epochs ORDER BY text_id")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        });

        let membership_entry = trace
            .entries()
            .find(|entry| entry.cell_id.pointer.is_empty())
            .expect("should have membership observation");
        match &membership_entry.revision {
            Revision::Counter(counter) => assert_eq!(
                *counter, 1,
                "trace capture should refresh the reader cache from persisted rowset state"
            ),
            _ => panic!("membership observation should have Counter revision"),
        }

        writer
            .connection()
            .execute(
                "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
                 VALUES ('e2', 'Epoch Two', 'two', 0, '')",
                [],
            )
            .expect("second writer insert should succeed");

        let mut provider = SqliteStateProvider::new(reader.connection(), reader.revision_store());
        assert!(
            !trace.validate(&mut provider),
            "reader trace should invalidate after another connection changes table membership"
        );
    }

    #[test]
    fn reactive_trace_invalidates_across_connections_on_content_change() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        let writer = open_database(&db_path).expect("should create writer database");
        let reader = open_database(&db_path).expect("should create reader database");

        writer
            .connection()
            .execute(
                "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
                 VALUES ('e1', 'Epoch One', 'one', 0, '')",
                [],
            )
            .expect("writer insert should succeed");

        let (_, trace) = TraceScope::run(|| {
            reader
                .connection()
                .query_row("SELECT title FROM epochs WHERE text_id = 'e1'", [], |row| {
                    row.get::<_, String>(0)
                })
                .expect("reader query should succeed")
        });

        let mut provider = SqliteStateProvider::new(reader.connection(), reader.revision_store());
        assert!(
            trace.validate(&mut provider),
            "reader trace should validate before the other connection updates the row"
        );

        writer
            .connection()
            .execute(
                "UPDATE epochs SET title = 'Epoch One Updated' WHERE text_id = 'e1'",
                [],
            )
            .expect("writer update should succeed");

        let mut provider = SqliteStateProvider::new(reader.connection(), reader.revision_store());
        assert!(
            !trace.validate(&mut provider),
            "reader trace should invalidate after another connection changes row content"
        );
    }

    #[test]
    fn defensive_mode_blocks_direct_shadow_table_writes() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        let defensive_enabled = conn
            .db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE)
            .expect("should query defensive config");
        assert!(defensive_enabled, "defensive mode should be enabled");

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('via-vtab', 'Via Vtab', 'via-vtab', 0, '')",
            [],
        )
        .expect("vtab write should succeed");

        let direct_insert = conn.execute(
            "INSERT INTO epochs_data (text_id, title, slug)
             VALUES ('direct-shadow', 'Direct Shadow', 'direct-shadow')",
            [],
        );
        assert!(direct_insert.is_err(), "defensive mode should block INSERT");

        let direct_update = conn.execute(
            "UPDATE epochs_data SET title = 'Direct Update' WHERE text_id = 'via-vtab'",
            [],
        );
        assert!(direct_update.is_err(), "defensive mode should block UPDATE");

        let direct_delete = conn.execute("DELETE FROM epochs_data WHERE text_id = 'via-vtab'", []);
        assert!(direct_delete.is_err(), "defensive mode should block DELETE");

        let direct_rev_insert = conn.execute(
            "INSERT INTO epochs_rev (rowid, digest) VALUES (1, zeroblob(32))",
            [],
        );
        assert!(
            direct_rev_insert.is_err(),
            "defensive mode should block *_rev writes"
        );
    }

    #[test]
    fn trusted_defensive_mode_window_can_write_shadow_tables() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug)
                 VALUES ('trusted-shadow', 'Trusted Shadow', 'trusted-shadow')",
                [],
            )
            .expect("trusted shadow write should succeed");
        }

        let direct_insert = conn.execute(
            "INSERT INTO epochs_data (text_id, title, slug)
             VALUES ('direct-shadow', 'Direct Shadow', 'direct-shadow')",
            [],
        );
        assert!(
            direct_insert.is_err(),
            "defensive mode should be restored after trusted write"
        );
    }

    #[test]
    fn test_vtab_callbacks_record_observations() {
        // This test verifies that vtab callbacks (xFilter, xColumn) properly
        // record observations into TraceScope.
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Insert test data
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug) VALUES ('e1', 'Epoch One', 'one')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO epochs_data (text_id, title, slug) VALUES ('e2', 'Epoch Two', 'two')",
                [],
            )
            .unwrap();
        }

        // Query through virtual table within TraceScope
        let (result, trace) = TraceScope::run(|| {
            conn.prepare("SELECT text_id, title FROM epochs ORDER BY text_id")
                .unwrap()
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        });

        // Verify query results
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("e1".to_string(), "Epoch One".to_string()));
        assert_eq!(result[1], ("e2".to_string(), "Epoch Two".to_string()));

        // Verify observations
        // Should have 1 membership observation (from xFilter) — pointer is empty
        let membership_count = trace
            .entries()
            .filter(|e| e.cell_id.pointer.is_empty())
            .count();
        assert_eq!(
            membership_count, 1,
            "should have exactly 1 membership observation"
        );

        // Should have content observations (from xColumn) — pointer is non-empty
        let content_count = trace
            .entries()
            .filter(|e| !e.cell_id.pointer.is_empty())
            .count();
        assert!(content_count > 0, "should have content observations");
    }

    #[test]
    fn reactive_vtab_writes_maintain_revisions_and_invalidate_traces() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', 'Epoch One', 'one', 0, '')",
            [],
        )
        .expect("vtab insert should succeed");

        let rowid: i64 = conn
            .query_row(
                "SELECT id FROM epochs_data WHERE text_id = 'e1'",
                [],
                |row| row.get(0),
            )
            .expect("inserted row should exist");
        let digest_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM epochs_rev WHERE rowid = ?1",
                [rowid],
                |row| row.get(0),
            )
            .expect("should read row digest count");
        assert_eq!(digest_count, 1, "insert should persist row digest");

        let counter: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'epochs_data'",
                [],
                |row| row.get(0),
            )
            .expect("should read rowset counter");
        assert_eq!(counter, 1, "insert should bump rowset counter");

        let (_, trace) = TraceScope::run(|| {
            conn.query_row("SELECT title FROM epochs WHERE text_id = 'e1'", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("query should succeed")
        });

        let mut provider = SqliteStateProvider::new(conn, db.revision_store());
        assert!(trace.validate(&mut provider), "fresh trace should validate");

        conn.execute(
            "UPDATE epochs SET title = 'Epoch One Updated' WHERE text_id = 'e1'",
            [],
        )
        .expect("vtab update should succeed");

        let mut provider = SqliteStateProvider::new(conn, db.revision_store());
        assert!(
            !trace.validate(&mut provider),
            "trace captured before update should be invalid"
        );

        let counter: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'epochs_data'",
                [],
                |row| row.get(0),
            )
            .expect("should read rowset counter");
        assert_eq!(counter, 2, "update should bump rowset counter");

        conn.execute("DELETE FROM epochs WHERE text_id = 'e1'", [])
            .expect("vtab delete should succeed");

        let digest_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM epochs_rev WHERE rowid = ?1",
                [rowid],
                |row| row.get(0),
            )
            .expect("should read row digest count");
        assert_eq!(digest_count, 0, "delete should remove row digest");

        let counter: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'epochs_data'",
                [],
                |row| row.get(0),
            )
            .expect("should read rowset counter");
        assert_eq!(counter, 3, "delete should bump rowset counter");
    }

    #[test]
    fn reactive_vtab_replace_clears_stale_revisions_without_rebuilding_live_rows() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute_batch(
                "
                CREATE TABLE custom_data (
                    id INTEGER PRIMARY KEY,
                    text_id TEXT NOT NULL UNIQUE,
                    title TEXT NOT NULL
                );
                CREATE TABLE custom_rev (
                    rowid INTEGER PRIMARY KEY,
                    digest BLOB NOT NULL CHECK(length(digest) = 32)
                );
                INSERT INTO rowset_revisions (table_name, counter) VALUES ('custom_data', 0);
                CREATE VIRTUAL TABLE custom USING reactive(custom_data);
                CREATE TRIGGER custom_rev_reject_live_delete
                    BEFORE DELETE ON custom_rev
                    WHEN EXISTS (SELECT 1 FROM custom_data WHERE rowid = OLD.rowid)
                    BEGIN
                        SELECT RAISE(ABORT, 'live revision row deleted');
                    END;
                ",
            )
            .expect("trusted setup should create custom reactive table");
        }

        conn.execute("INSERT INTO custom (text_id, title) VALUES ('a', 'A')", [])
            .expect("first custom insert should succeed");
        conn.execute("INSERT INTO custom (text_id, title) VALUES ('b', 'B')", [])
            .expect("second custom insert should succeed");

        let old_a_rowid: i64 = conn
            .query_row(
                "SELECT id FROM custom_data WHERE text_id = 'a'",
                [],
                |row| row.get(0),
            )
            .expect("should read old rowid");
        let b_rowid: i64 = conn
            .query_row(
                "SELECT id FROM custom_data WHERE text_id = 'b'",
                [],
                |row| row.get(0),
            )
            .expect("should read live peer rowid");
        let replacement_rowid = old_a_rowid + 100;

        conn.execute(
            "INSERT OR REPLACE INTO custom (id, text_id, title) VALUES (?1, 'a', 'A2')",
            [replacement_rowid],
        )
        .expect("replace should clear only stale revision rows");

        let old_rev_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM custom_rev WHERE rowid = ?1",
                [old_a_rowid],
                |row| row.get(0),
            )
            .expect("should count old revision row");
        assert_eq!(old_rev_count, 0, "replaced row revision should be gone");

        for rowid in [replacement_rowid, b_rowid] {
            let rev_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM custom_rev WHERE rowid = ?1",
                    [rowid],
                    |row| row.get(0),
                )
                .expect("should count live revision row");
            assert_eq!(rev_count, 1, "live row revision should remain");
        }
    }

    #[test]
    fn reactive_vtab_cascade_delete_clears_stale_child_revisions_only() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', 'Epoch One', 'one', 0, '')",
            [],
        )
        .expect("first epoch insert should succeed");
        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e2', 'Epoch Two', 'two', 0, '')",
            [],
        )
        .expect("second epoch insert should succeed");
        conn.execute(
            "INSERT INTO phases (text_id, title, epoch_id)
             VALUES ('p1', 'Phase One', 1)",
            [],
        )
        .expect("first phase insert should succeed");
        conn.execute(
            "INSERT INTO phases (text_id, title, epoch_id)
             VALUES ('p2', 'Phase Two', 2)",
            [],
        )
        .expect("second phase insert should succeed");

        let p1_rowid: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .expect("should read deleted child rowid");
        let p2_rowid: i64 = conn
            .query_row(
                "SELECT id FROM phases_data WHERE text_id = 'p2'",
                [],
                |row| row.get(0),
            )
            .expect("should read live child rowid");
        let phase_counter_before: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'phases_data'",
                [],
                |row| row.get(0),
            )
            .expect("should read phase rowset counter");

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute_batch(
                "
                CREATE TRIGGER phases_rev_reject_live_delete
                    BEFORE DELETE ON phases_rev
                    WHEN EXISTS (SELECT 1 FROM phases_data WHERE rowid = OLD.rowid)
                    BEGIN
                        SELECT RAISE(ABORT, 'live phase revision row deleted');
                    END;
                ",
            )
            .expect("trusted setup should create revision guard trigger");
        }

        conn.execute("DELETE FROM epochs WHERE text_id = 'e1'", [])
            .expect("parent delete should clear only stale child revisions");

        let deleted_rev_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM phases_rev WHERE rowid = ?1",
                [p1_rowid],
                |row| row.get(0),
            )
            .expect("should count deleted child revision");
        assert_eq!(
            deleted_rev_count, 0,
            "cascaded child revision should be gone"
        );

        let live_rev_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM phases_rev WHERE rowid = ?1",
                [p2_rowid],
                |row| row.get(0),
            )
            .expect("should count live child revision");
        assert_eq!(live_rev_count, 1, "live child revision should remain");

        let phase_counter_after: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'phases_data'",
                [],
                |row| row.get(0),
            )
            .expect("should read phase rowset counter after cascade");
        assert_eq!(
            phase_counter_after,
            phase_counter_before + 1,
            "cascade delete should bump child rowset counter"
        );
    }

    #[test]
    fn reactive_vtab_text_digests_preserve_embedded_nuls() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', ?1, 'one', 0, '')",
            params!["Epoch\0One"],
        )
        .expect("vtab insert should succeed");

        let (_, trace) = TraceScope::run(|| {
            conn.query_row("SELECT title FROM epochs WHERE text_id = 'e1'", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("query should succeed")
        });
        let mut provider = SqliteStateProvider::new(conn, db.revision_store());
        assert!(trace.validate(&mut provider), "fresh trace should validate");

        conn.execute(
            "UPDATE epochs SET title = ?1 WHERE text_id = 'e1'",
            params!["Epoch\0Two"],
        )
        .expect("vtab update should succeed");

        let mut provider = SqliteStateProvider::new(conn, db.revision_store());
        assert!(
            !trace.validate(&mut provider),
            "trace should invalidate when bytes after embedded NUL change"
        );
    }

    #[test]
    fn reactive_vtab_rejects_rowid_updates() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', 'Epoch One', 'one', 0, '')",
            [],
        )
        .expect("vtab insert should succeed");
        let old_rowid: i64 = conn
            .query_row(
                "SELECT id FROM epochs_data WHERE text_id = 'e1'",
                [],
                |row| row.get(0),
            )
            .expect("inserted row should exist");
        let new_rowid = old_rowid + 100;

        let result = conn.execute("UPDATE epochs SET id = id + 100 WHERE text_id = 'e1'", []);
        assert!(result.is_err(), "vtab rowid update should be rejected");

        let result = conn.execute(
            "UPDATE epochs SET rowid = rowid + 100 WHERE text_id = 'e1'",
            [],
        );
        assert!(
            result.is_err(),
            "vtab rowid pseudocolumn update should be rejected"
        );

        let result = conn.execute(
            "UPDATE epochs SET id = ?1 WHERE text_id = 'e1'",
            [new_rowid.to_string()],
        );
        assert!(
            result.is_err(),
            "coercible text rowid alias update should be rejected"
        );

        let current_rowid: i64 = conn
            .query_row(
                "SELECT id FROM epochs_data WHERE text_id = 'e1'",
                [],
                |row| row.get(0),
            )
            .expect("row should remain at original identity");
        assert_eq!(current_rowid, old_rowid);

        let digest_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM epochs_rev WHERE rowid = ?1",
                [old_rowid],
                |row| row.get(0),
            )
            .expect("should read row digest count");
        assert_eq!(digest_count, 1, "original row digest should remain");

        let (_, trace) = TraceScope::run(|| {
            conn.query_row(
                "SELECT title FROM epochs WHERE id = ?1",
                [old_rowid],
                |row| row.get::<_, String>(0),
            )
            .expect("query should succeed")
        });
        let mut provider = SqliteStateProvider::new(conn, db.revision_store());
        assert!(
            trace.validate(&mut provider),
            "trace for original row should validate after rejected rowid update"
        );

        let moved_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM epochs_data WHERE id = ?1",
                [new_rowid],
                |row| row.get(0),
            )
            .expect("should read moved row count");
        assert_eq!(moved_count, 0);
    }

    #[test]
    fn reactive_vtab_rejects_rowid_only_inserts() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        let result = conn.execute(
            "INSERT INTO epochs(rowid, text_id, title) VALUES(123, 'e1', 'Epoch')",
            [],
        );
        assert!(result.is_err(), "vtab rowid-only insert should be rejected");

        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .expect("should read row count");
        assert_eq!(row_count, 0);
    }

    #[test]
    fn reactive_vtab_preserves_defaults_and_bound_nulls_on_insert() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', 'Epoch One', 'one', 0, '')",
            [],
        )
        .expect("epoch insert should succeed");

        conn.execute(
            "INSERT INTO phases (text_id, title, epoch_id)
             VALUES ('p-default', 'Default Phase', 1)",
            [],
        )
        .expect("omitted status and kind should use their defaults");
        let (default_status, default_kind): (String, String) = conn
            .query_row(
                "SELECT status, kind FROM phases_data WHERE text_id = 'p-default'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("should read defaulted status and kind");
        assert_eq!(default_status, "pending");
        assert_eq!(default_kind, "regular");

        let explicit_null: Option<String> = None;
        let result = conn.execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind)
             VALUES ('p-null', 'Null Phase', 'pending', 1, ?1)",
            params![explicit_null],
        );
        assert!(
            result.is_err(),
            "bound NULL kind should hit the shadow table NOT NULL constraint"
        );

        let null_phase_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM phases_data WHERE text_id = 'p-null'",
                [],
                |row| row.get(0),
            )
            .expect("should read explicit null row count");
        assert_eq!(null_phase_count, 0);

        conn.execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind)
             VALUES ('p-literal-null', 'Literal Null Phase', 'pending', 1, NULL)",
            [],
        )
        .expect("literal NULL for a defaulted insert column follows omitted-column semantics");
        let literal_null_kind: String = conn
            .query_row(
                "SELECT kind FROM phases_data WHERE text_id = 'p-literal-null'",
                [],
                |row| row.get(0),
            )
            .expect("should read literal NULL insert kind");
        assert_eq!(literal_null_kind, "regular");
    }

    #[test]
    fn reactive_vtab_updates_preserve_no_change_columns_and_explicit_nulls() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute_batch(
                "
                CREATE TABLE custom_data (
                    id INTEGER PRIMARY KEY,
                    text_id TEXT NOT NULL UNIQUE,
                    changed TEXT NOT NULL,
                    preserved TEXT NOT NULL,
                    required TEXT NOT NULL,
                    nullable TEXT
                );
                CREATE TABLE custom_rev (rowid INTEGER PRIMARY KEY, digest BLOB NOT NULL);
                INSERT INTO rowset_revisions (table_name, counter) VALUES ('custom_data', 0);
                CREATE VIRTUAL TABLE custom USING reactive(custom_data);
                CREATE TRIGGER custom_preserved_no_update
                    BEFORE UPDATE OF preserved ON custom_data
                    BEGIN
                        SELECT RAISE(ABORT, 'preserved column was updated');
                    END;
                ",
            )
            .expect("trusted setup should create custom reactive table");
        }

        conn.execute(
            "INSERT INTO custom (text_id, changed, preserved, required, nullable)
             VALUES ('row-1', 'old', 'keep', 'required', 'value')",
            [],
        )
        .expect("custom insert should succeed");

        conn.execute(
            "UPDATE custom SET changed = 'new' WHERE text_id = 'row-1'",
            [],
        )
        .expect("updating one column should not touch omitted columns");
        let (changed, preserved, required, nullable): (
            String,
            String,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT changed, preserved, required, nullable FROM custom_data WHERE text_id = 'row-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("should read custom row");
        assert_eq!(changed, "new");
        assert_eq!(preserved, "keep");
        assert_eq!(required, "required");
        assert_eq!(nullable.as_deref(), Some("value"));

        conn.execute(
            "UPDATE custom SET nullable = NULL WHERE text_id = 'row-1'",
            [],
        )
        .expect("explicit NULL should clear nullable columns");
        let nullable_after_null: Option<String> = conn
            .query_row(
                "SELECT nullable FROM custom_data WHERE text_id = 'row-1'",
                [],
                |row| row.get(0),
            )
            .expect("should read nullable value");
        assert_eq!(nullable_after_null, None);

        let result = conn.execute(
            "UPDATE custom SET required = NULL WHERE text_id = 'row-1'",
            [],
        );
        assert!(
            result.is_err(),
            "explicit NULL should hit backing NOT NULL constraints"
        );
    }

    #[test]
    fn reactive_vtab_ignored_inserts_report_zero_rows() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        let inserted = conn
            .execute(
                "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
                 VALUES ('e1', 'Epoch One', 'one', 0, '')",
                [],
            )
            .expect("first insert should succeed");
        assert_eq!(inserted, 1);

        let ignored = conn
            .execute(
                "INSERT OR IGNORE INTO epochs (text_id, title, slug, reviewed, sort_key)
                 VALUES ('e1', 'Ignored Epoch', 'ignored', 0, '')",
                [],
            )
            .expect("ignored insert should not error");
        assert_eq!(ignored, 0, "ignored vtab insert should be a no-op");

        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .expect("should read row count");
        assert_eq!(row_count, 1);
    }

    #[test]
    fn reactive_revision_schema_covers_all_reactive_tables() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        for (_, shadow_table) in crate::schema::REACTIVE_TABLES {
            let rev_table = shadow_table.replace("_data", "_rev");
            let rev_exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [&rev_table],
                    |row| row.get(0),
                )
                .expect("should query sqlite_master");
            assert_eq!(rev_exists, 1, "{rev_table} should exist");

            let rowset_seed: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM rowset_revisions WHERE table_name = ?1",
                    [shadow_table],
                    |row| row.get(0),
                )
                .expect("should query rowset seed");
            assert_eq!(rowset_seed, 1, "{shadow_table} should have rowset seed");
        }
    }

    #[test]
    fn reactive_vtab_writes_cover_representative_table_shapes() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.execute(
            "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
             VALUES ('e1', 'Epoch', 'epoch', 0, '')",
            [],
        )
        .expect("epoch insert should succeed");
        conn.execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug)
             VALUES ('p1', 'Phase', 'pending', 1, 'regular', 'phase')",
            [],
        )
        .expect("phase insert should succeed");
        conn.execute(
            "INSERT INTO goals (text_id, label, status, phase_id, slug)
             VALUES ('g1', 'Goal', 'pending', 1, 'goal')",
            [],
        )
        .expect("goal insert should succeed");
        conn.execute(
            "INSERT INTO workspace_active_phase (workspace_root, phase_id, updated_at)
             VALUES ('/tmp/workspace', 1, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("workspace active phase insert should succeed");
        conn.execute(
            "INSERT INTO rfcs (text_id, rfc_number, title, stage, status, slug, file_path, created_at)
             VALUES ('r1', 10165, 'Reactive SQLite', 3, 'active', 'reactive-sqlite', 'docs/rfcs/stage-3/10165-reactive-sqlite.md', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("RFC insert should succeed");

        for table in [
            "epochs_data",
            "phases_data",
            "goals_data",
            "workspace_active_phase_data",
            "rfcs_data",
        ] {
            let rev_table = table.replace("_data", "_rev");
            let digest_count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {}", rev_table), [], |row| {
                    row.get(0)
                })
                .expect("should count row digests");
            assert_eq!(digest_count, 1, "{rev_table} should have one digest");

            let counter: i64 = conn
                .query_row(
                    "SELECT counter FROM rowset_revisions WHERE table_name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("should read rowset counter");
            assert_eq!(counter, 1, "{table} should have one rowset bump");
        }
    }

    #[test]
    fn test_ideas_table_exists() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Insert an idea
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO ideas_data (text_id, title, description, status, created_at, source) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                [
                    "c69552da-7be5-4f8d-8b68-6c95e90e424b",
                    "Test Idea",
                    "A test description",
                    "new",
                    "2026-02-23T12:00:00Z",
                    "user",
                ],
            )
            .expect("should insert idea");
        }

        // Query it back
        let (title, status): (String, String) = conn
            .query_row(
                "SELECT title, status FROM ideas_data WHERE text_id = ?1",
                ["c69552da-7be5-4f8d-8b68-6c95e90e424b"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("should query idea");

        assert_eq!(title, "Test Idea");
        assert_eq!(status, "new");
    }

    #[test]
    fn test_idea_tags_junction() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Insert an idea
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO ideas_data (text_id, title, status, created_at, source) \
                 VALUES ('idea-1', 'Tagged Idea', 'new', '2026-02-23T12:00:00Z', 'user')",
                [],
            )
            .expect("should insert idea");
        }

        // Add tags
        conn.execute(
            "INSERT INTO idea_tags (idea_id, tag) VALUES (1, 'papercut')",
            [],
        )
        .expect("should insert tag 1");
        conn.execute(
            "INSERT INTO idea_tags (idea_id, tag) VALUES (1, 'sidebar')",
            [],
        )
        .expect("should insert tag 2");

        // Query tags
        let mut stmt = conn
            .prepare("SELECT tag FROM idea_tags WHERE idea_id = 1 ORDER BY tag")
            .unwrap();
        let tags: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(tags, vec!["papercut", "sidebar"]);
    }

    #[test]
    fn test_idea_cascade_delete() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Insert idea with tags
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO ideas_data (text_id, title, status, created_at, source) \
                 VALUES ('idea-1', 'Idea to Delete', 'new', '2026-02-23T12:00:00Z', 'user')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO idea_tags (idea_id, tag) VALUES (1, 'test')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO idea_task_refs (idea_id, task_ref) VALUES (1, 'task-1')",
                [],
            )
            .unwrap();

            // Delete idea
            conn.execute("DELETE FROM ideas_data WHERE text_id = 'idea-1'", [])
                .unwrap();
        }

        // Verify tags and task refs are deleted
        let tag_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM idea_tags", [], |row| row.get(0))
            .unwrap();
        let ref_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM idea_task_refs", [], |row| row.get(0))
            .unwrap();

        assert_eq!(tag_count, 0, "tags should be cascade deleted");
        assert_eq!(ref_count, 0, "task refs should be cascade deleted");
    }

    #[test]
    fn test_inbox_table_exists() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Verify inbox_data table exists
        let result: Result<i32, _> =
            conn.query_row("SELECT COUNT(*) FROM inbox_data WHERE 1=0", [], |row| {
                row.get(0)
            });
        assert!(result.is_ok(), "inbox_data table should exist");

        // Verify we can insert a perception event
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO inbox_data (text_id, created_at, status, entity_type, source, intent, priority, subject, body)
                 VALUES ('inbox-1', '2024-01-15T10:00:00Z', 'pending', 'project', 'user-feedback', 'fyi', 'next-touch', 'Test subject', 'Test body')",
                [],
            )
            .expect("should insert inbox item");
        }

        // Verify we can query it back
        let subject: String = conn
            .query_row(
                "SELECT subject FROM inbox_data WHERE text_id = 'inbox-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(subject, "Test subject");
    }

    #[test]
    fn test_inbox_entity_scope() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Project-level (no entity_id)
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO inbox_data (text_id, created_at, status, entity_type, source, intent, priority, subject, body)
                 VALUES ('inbox-project', '2024-01-15T10:00:00Z', 'pending', 'project', 'user-feedback', 'fyi', 'next-touch', 'Global', '')",
                [],
            )
            .expect("should insert project-level item");

            // Goal-scoped (with entity_id)
            conn.execute(
                "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, subject, body)
                 VALUES ('inbox-goal', '2024-01-15T10:00:00Z', 'pending', 'goal', 'my-goal', 'user-feedback', 'concern', 'immediate', 'Goal feedback', '')",
                [],
            )
            .expect("should insert goal-scoped item");
        }

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM inbox_data WHERE entity_type = 'project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_inbox_intent_and_confidence() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        // Claim with confidence
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO inbox_data (text_id, created_at, status, entity_type, entity_id, source, intent, priority, confidence, subject, body)
                 VALUES ('inbox-claim', '2024-01-15T10:00:00Z', 'pending', 'goal', 'g1', 'user-feedback', 'claim', 'immediate', 'high', 'I think this is done', '')",
                [],
            )
            .expect("should insert claim with confidence");
        }

        let confidence: String = conn
            .query_row(
                "SELECT confidence FROM inbox_data WHERE text_id = 'inbox-claim'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(confidence, "high");
    }

    #[test]
    fn test_inbox_source_types() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        for (id, source) in [
            ("s1", "user-feedback"),
            ("s2", "system-observation"),
            ("s3", "plan-mutation"),
        ] {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                    "INSERT INTO inbox_data (text_id, created_at, status, entity_type, source, intent, priority, subject, body)
                     VALUES (?1, '2024-01-15T10:00:00Z', 'pending', 'project', ?2, 'fyi', 'next-touch', 'Test', '')",
                    (id, source),
                )
                .expect("should insert with source");
        }

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM inbox_data", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn workspace_active_phase_tables_exist() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        for table_name in [
            "workspace_active_phase_data",
            "workspace_active_phase",
            "workspace_active_phase_rev",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table_name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table_name} should exist");
        }

        let rowset_seed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rowset_revisions WHERE table_name = 'workspace_active_phase_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rowset_seed, 1, "rowset revision seed should exist");
    }

    #[test]
    fn phase_ownership_tables_exist() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        for table_name in [
            "phase_ownership_data",
            "phase_ownership",
            "phase_ownership_rev",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table_name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table_name} should exist");
        }

        let rowset_seed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rowset_revisions WHERE table_name = 'phase_ownership_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rowset_seed, 1, "rowset revision seed should exist");
    }

    #[test]
    fn new_file_databases_enable_incremental_auto_vacuum() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        let db = open_database(&db_path).expect("should create database");
        let auto_vacuum = storage_maintenance_stats(db.connection())
            .expect("read storage stats")
            .auto_vacuum;

        assert_eq!(auto_vacuum, AutoVacuumMode::Incremental);
    }

    #[test]
    fn existing_non_incremental_databases_are_detected_without_conversion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        {
            let conn = Connection::open(&db_path).expect("create legacy database");
            conn.execute("CREATE TABLE legacy_data (id INTEGER PRIMARY KEY)", [])
                .expect("create legacy table");
        }

        let db = open_database(&db_path).expect("should open legacy database");
        let report = maintain_database(
            db.connection(),
            StorageMaintenanceOptions {
                enable_incremental_vacuum: false,
                vacuum_page_budget: 4,
                checkpoint_wal: false,
            },
        )
        .expect("run maintenance");

        assert_eq!(report.before.auto_vacuum, AutoVacuumMode::None);
        assert_eq!(report.after.auto_vacuum, AutoVacuumMode::None);
        assert!(!report.conversion_performed);
    }

    #[test]
    fn bounded_maintenance_reduces_freelist_count_for_incremental_databases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");
        let db = open_database(&db_path).expect("should create database");
        let conn = db.connection();

        conn.execute_batch(
            "CREATE TABLE maintenance_blob (payload BLOB);
             WITH RECURSIVE seq(x) AS (
               VALUES(1)
               UNION ALL
               SELECT x + 1 FROM seq WHERE x < 512
             )
             INSERT INTO maintenance_blob (payload)
             SELECT zeroblob(4096) FROM seq;
             DELETE FROM maintenance_blob;",
        )
        .expect("create freelist pages");

        let before = storage_maintenance_stats(conn).expect("read stats before");
        assert!(
            before.freelist_count > 0,
            "fixture should create reclaimable pages"
        );

        let report = maintain_database(
            conn,
            StorageMaintenanceOptions {
                enable_incremental_vacuum: false,
                vacuum_page_budget: 8,
                checkpoint_wal: false,
            },
        )
        .expect("run maintenance");

        assert!(report.incremental_vacuum_steps_run > 0);
        assert!(report.after.freelist_count < before.freelist_count);
    }

    #[test]
    fn conversion_enables_incremental_auto_vacuum_and_preserves_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("legacy.db");

        {
            let conn = Connection::open(&db_path).expect("create legacy database");
            conn.execute(
                "CREATE TABLE legacy_data (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
                [],
            )
            .expect("create legacy table");
            conn.execute("INSERT INTO legacy_data (value) VALUES ('kept')", [])
                .expect("insert legacy row");
        }

        let conn = Connection::open(&db_path).expect("open legacy database");
        let before = storage_maintenance_stats(&conn).expect("read stats before");
        assert_eq!(before.auto_vacuum, AutoVacuumMode::None);

        let report = maintain_database(
            &conn,
            StorageMaintenanceOptions {
                enable_incremental_vacuum: true,
                vacuum_page_budget: 4,
                checkpoint_wal: true,
            },
        )
        .expect("run conversion maintenance");

        let value: String = conn
            .query_row("SELECT value FROM legacy_data WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("read preserved row");
        assert_eq!(value, "kept");
        assert!(report.conversion_performed);
        assert!(report.vacuum_performed);
        assert_eq!(report.after.auto_vacuum, AutoVacuumMode::Incremental);
        assert!(report.wal_checkpoint.is_some());
    }
}
