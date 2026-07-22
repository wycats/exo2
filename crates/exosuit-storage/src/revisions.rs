//! Revision tracking for reactive virtual tables.
//!
//! This module implements the RevisionStore which tracks:
//! - Row revisions: BLAKE3 content digests per row
//! - Row-Set revisions: Persistent monotonic counters per table
//!
//! Per RFC 10165 §6, all revisions survive process restarts.
//! Row revisions are content digests. Row-set revisions are persistent
//! counters stored in the `rowset_revisions` table.

use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::DatabaseError;

/// Revision store for tracking row and row-set revisions.
///
/// Maintains an in-memory cache backed by SQLite tables for persistence.
/// Uses interior mutability to allow caching from immutable references.
pub struct RevisionStore {
    /// In-memory cache of row digests: (table_name, rowid) -> digest
    row_cache: Mutex<HashMap<(String, i64), [u8; 32]>>,
    /// In-memory cache of rowset counters: table_name -> counter
    /// Loaded from `rowset_revisions` table on startup, not reset.
    rowset_cache: Mutex<HashMap<String, u64>>,
}

impl RevisionStore {
    fn row_cache(&self) -> MutexGuard<'_, HashMap<(String, i64), [u8; 32]>> {
        self.row_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn rowset_cache(&self) -> MutexGuard<'_, HashMap<String, u64>> {
        self.rowset_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Create a RevisionStore, loading persisted counters from the database.
    ///
    /// Counters are NOT reset on startup — they persist across process restarts.
    pub fn new(conn: &Connection) -> Result<Self, DatabaseError> {
        let mut rowset_cache = HashMap::new();

        // Load existing rowset counters from database
        let mut stmt = conn.prepare("SELECT table_name, counter FROM rowset_revisions")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;
        for row in rows {
            let (table, counter) = row?;
            rowset_cache.insert(table, counter);
        }

        Ok(Self {
            row_cache: Mutex::new(HashMap::new()),
            rowset_cache: Mutex::new(rowset_cache),
        })
    }

    /// Get the row digest for a specific row.
    ///
    /// Returns None if no digest is stored (row hasn't been written through vtab).
    ///
    /// SQLite is authoritative here. The in-memory cache is only a local hint:
    /// vtab callbacks can run inside transactions that later roll back, and
    /// other open connections can commit newer revision rows.
    pub fn get_row_digest(
        &self,
        conn: &Connection,
        table: &str,
        rowid: i64,
    ) -> Result<Option<[u8; 32]>, DatabaseError> {
        let key = (table.to_string(), rowid);

        // Query from *_rev table
        let rev_table = table.replace("_data", "_rev");
        let sql = format!("SELECT digest FROM {} WHERE rowid = ?1", rev_table);
        let digest: Option<Vec<u8>> = conn.query_row(&sql, [rowid], |row| row.get(0)).optional()?;

        if let Some(bytes) = digest {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                self.row_cache().insert(key, arr);
                return Ok(Some(arr));
            }
        }

        self.row_cache().remove(&key);
        Ok(None)
    }

    /// Set the row digest for a specific row.
    ///
    /// Updates both the cache and the persistent *_rev table.
    pub fn set_row_digest(
        &self,
        conn: &Connection,
        table: &str,
        rowid: i64,
        digest: [u8; 32],
    ) -> Result<(), DatabaseError> {
        // Update cache
        let key = (table.to_string(), rowid);
        self.row_cache().insert(key, digest);

        // Persist to *_rev table
        let rev_table = table.replace("_data", "_rev");
        let sql = format!(
            "INSERT OR REPLACE INTO {} (rowid, digest) VALUES (?1, ?2)",
            rev_table
        );
        conn.execute(&sql, rusqlite::params![rowid, &digest[..]])?;

        Ok(())
    }

    /// Remove the row digest for a deleted row.
    pub fn remove_row_digest(
        &self,
        conn: &Connection,
        table: &str,
        rowid: i64,
    ) -> Result<(), DatabaseError> {
        // Remove from cache
        let key = (table.to_string(), rowid);
        self.row_cache().remove(&key);

        // Remove from *_rev table
        let rev_table = table.replace("_data", "_rev");
        let sql = format!("DELETE FROM {} WHERE rowid = ?1", rev_table);
        conn.execute(&sql, [rowid])?;

        Ok(())
    }

    /// Update the in-memory row digest cache after a raw virtual-table write.
    ///
    /// Virtual table callbacks run while rusqlite already has an outstanding
    /// connection borrow, so those callbacks update SQLite through raw FFI and
    /// then use this method to keep the shared revision cache coherent.
    pub(crate) fn cache_row_digest(&self, table: &str, rowid: i64, digest: [u8; 32]) {
        let key = (table.to_string(), rowid);
        self.row_cache().insert(key, digest);
    }

    /// Remove one cached row digest after a raw virtual-table delete.
    pub(crate) fn uncache_row_digest(&self, table: &str, rowid: i64) {
        let key = (table.to_string(), rowid);
        self.row_cache().remove(&key);
    }

    /// Clear all cached row digests for a shadow table.
    pub(crate) fn clear_table_row_cache(&self, table: &str) {
        self.row_cache()
            .retain(|(cached_table, _), _| cached_table != table);
    }

    /// Get the cached rowset counter for a table.
    pub fn get_rowset_counter(&self, table: &str) -> u64 {
        self.rowset_cache().get(table).copied().unwrap_or(0)
    }

    /// Get the persisted rowset counter for a table and refresh the cache.
    pub fn get_rowset_counter_from_db(
        &self,
        conn: &Connection,
        table: &str,
    ) -> Result<u64, DatabaseError> {
        let counter: Option<i64> = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        let counter = counter.unwrap_or(0) as u64;
        self.cache_rowset_counter(table, counter);
        Ok(counter)
    }

    /// Bump the rowset revision counter for a table.
    ///
    /// Called on INSERT or DELETE to indicate membership changed.
    pub fn bump_rowset_counter(
        &self,
        conn: &Connection,
        table: &str,
    ) -> Result<u64, DatabaseError> {
        // Persist an atomic increment. The cache is refreshed from SQLite after
        // the write so multiple open connections do not overwrite each other.
        conn.execute(
            "INSERT INTO rowset_revisions (table_name, counter) VALUES (?1, 1) \
             ON CONFLICT(table_name) DO UPDATE SET counter = counter + 1",
            [table],
        )?;

        self.get_rowset_counter_from_db(conn, table)
    }

    /// Update the in-memory rowset cache after a raw virtual-table write.
    pub(crate) fn cache_rowset_counter(&self, table: &str, counter: u64) {
        self.rowset_cache().insert(table.to_string(), counter);
    }

    /// Backfill row revision digests for existing rows in a shadow table.
    pub(crate) fn backfill_row_digests(
        &self,
        conn: &Connection,
        table: &str,
    ) -> Result<(), DatabaseError> {
        if !conn.is_autocommit() {
            return self.backfill_row_digests_locked(conn, table);
        }

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = self.backfill_row_digests_locked(conn, table);
        match result {
            Ok(()) => {
                if let Err(err) = conn.execute_batch("COMMIT") {
                    let _ = conn.execute_batch("ROLLBACK");
                    self.clear_table_row_cache(table);
                    return Err(err.into());
                }
                Ok(())
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                self.clear_table_row_cache(table);
                Err(err)
            }
        }
    }

    fn backfill_row_digests_locked(
        &self,
        conn: &Connection,
        table: &str,
    ) -> Result<(), DatabaseError> {
        self.clear_table_row_cache(table);

        let mut current_digests = HashMap::new();
        {
            let sql = format!("SELECT rowid, * FROM {}", table);
            let mut stmt = conn.prepare(&sql)?;
            let column_count = stmt.column_count();
            let rows = stmt.query_map([], |row| {
                let rowid: i64 = row.get(0)?;
                let mut values = Vec::with_capacity(column_count);
                for col in 0..column_count {
                    values.push(row.get::<_, rusqlite::types::Value>(col)?);
                }
                Ok((rowid, values))
            })?;

            for row in rows {
                let (rowid, values) = row?;
                let digest = compute_row_digest(&values);
                current_digests.insert(rowid, digest);
            }
        }

        let rev_table = table.replace("_data", "_rev");
        let mut existing_digests = HashMap::new();
        {
            let existing_sql = format!("SELECT rowid, digest FROM {}", rev_table);
            let mut existing_stmt = conn.prepare(&existing_sql)?;
            let existing_rows = existing_stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            for row in existing_rows {
                let (rowid, digest) = row?;
                existing_digests.insert(rowid, digest);
            }
        }

        let rowset_may_have_changed =
            existing_digests
                .iter()
                .any(|(rowid, existing)| match current_digests.get(rowid) {
                    Some(current) => existing.as_slice() != current,
                    None => true,
                })
                || current_digests
                    .keys()
                    .any(|rowid| !existing_digests.contains_key(rowid));

        conn.execute(&format!("DELETE FROM {}", rev_table), [])?;
        for (rowid, digest) in current_digests {
            self.set_row_digest(conn, table, rowid, digest)?;
        }
        if rowset_may_have_changed {
            self.bump_rowset_counter(conn, table)?;
        }

        Ok(())
    }

    /// Clear all cached data (useful for testing).
    #[cfg(test)]
    pub fn clear_cache(&self) {
        self.row_cache().clear();
        self.rowset_cache().clear();
    }
}

/// Compute a BLAKE3 digest of a row's values.
///
/// Callers pass the same shape used by reactive reads: `rowid` followed by the
/// table's visible columns. Keeping this in one place ensures read-time trace
/// digests and write-time stored digests use the same equivalence relation.
pub(crate) fn compute_row_digest(row: &[rusqlite::types::Value]) -> [u8; 32] {
    use blake3::Hasher;

    let mut hasher = Hasher::new();
    for value in row {
        match value {
            rusqlite::types::Value::Null => {
                hasher.update(b"\x00");
            }
            rusqlite::types::Value::Integer(i) => {
                hasher.update(b"\x01");
                hasher.update(&i.to_le_bytes());
            }
            rusqlite::types::Value::Real(f) => {
                hasher.update(b"\x02");
                hasher.update(&f.to_le_bytes());
            }
            rusqlite::types::Value::Text(s) => {
                hasher.update(b"\x03");
                hash_variable_bytes(&mut hasher, s.as_bytes());
            }
            rusqlite::types::Value::Blob(b) => {
                hasher.update(b"\x04");
                hash_variable_bytes(&mut hasher, b);
            }
        }
    }
    *hasher.finalize().as_bytes()
}

fn hash_variable_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// A `StateProvider` backed by a SQLite connection and `RevisionStore`.
///
/// This bridges the core `Trace::validate()` API with the SQLite-specific
/// revision storage. Errors during lookup are treated as "cell missing"
/// (returning `None`), which causes validation to fail conservatively.
pub struct SqliteStateProvider<'a> {
    conn: &'a Connection,
    store: &'a RevisionStore,
}

impl<'a> SqliteStateProvider<'a> {
    pub fn new(conn: &'a Connection, store: &'a RevisionStore) -> Self {
        Self { conn, store }
    }
}

impl exosuit_reactivity_core::StateProvider for SqliteStateProvider<'_> {
    fn get_revision(
        &mut self,
        cell_id: &exosuit_reactivity_core::CellId,
    ) -> Option<exosuit_reactivity_core::Revision> {
        if cell_id.pointer.is_empty() {
            // Membership cell: pointer is empty → persistent monotonic counter
            let counter = self
                .store
                .get_rowset_counter_from_db(self.conn, &cell_id.source_id)
                .ok()?;
            Some(exosuit_reactivity_core::Revision::counter(counter))
        } else {
            // Content cell: pointer is rowid → row-level digest
            let rowid: i64 = cell_id.pointer.parse().ok()?;
            let digest = self
                .store
                .get_row_digest(self.conn, &cell_id.source_id, rowid)
                .ok()??;
            Some(crate::trace::digest_revision(digest))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_memory_database;
    use std::sync::Arc;

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
            .expect("should disable defensive mode for trusted revision test setup");
        DefensiveModeGuard { conn, restore }
    }

    #[test]
    fn test_revision_store_new() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");

        // Should have loaded rowset counters (persisted, not reset)
        let counter = store.get_rowset_counter("epochs_data");
        assert_eq!(counter, 0);
    }

    #[test]
    fn revision_caches_recover_after_mutex_poisoning() {
        let db = open_memory_database().expect("should create database");
        let store =
            Arc::new(RevisionStore::new(db.connection()).expect("should create revision store"));

        let row_store = Arc::clone(&store);
        assert!(std::thread::spawn(move || {
            let _guard = row_store.row_cache.lock().expect("lock row cache");
            panic!("poison row cache");
        })
        .join()
        .is_err());
        let rowset_store = Arc::clone(&store);
        assert!(std::thread::spawn(move || {
            let _guard = rowset_store.rowset_cache.lock().expect("lock rowset cache");
            panic!("poison rowset cache");
        })
        .join()
        .is_err());

        store.cache_row_digest("epochs_data", 1, [7; 32]);
        store.cache_rowset_counter("epochs_data", 9);
        assert_eq!(
            store.row_cache().get(&("epochs_data".to_string(), 1)),
            Some(&[7; 32])
        );
        assert_eq!(store.get_rowset_counter("epochs_data"), 9);
    }

    #[test]
    fn test_counters_survive_restart() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();

        // First "process": create store, bump counter
        let store1 = RevisionStore::new(conn).expect("should create store");
        store1
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump");
        store1
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump");
        assert_eq!(store1.get_rowset_counter("epochs_data"), 2);

        // Second "process": create a new store on the same connection
        // (simulates process restart — counter must NOT reset to 0)
        let store2 = RevisionStore::new(conn).expect("should create store");
        assert_eq!(
            store2.get_rowset_counter("epochs_data"),
            2,
            "counter should survive across RevisionStore instances"
        );

        // Bumping continues from where it left off
        store2
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump");
        assert_eq!(store2.get_rowset_counter("epochs_data"), 3);
    }

    #[test]
    fn test_row_digest_roundtrip() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        // Insert a row to get a valid rowid
        let digest = [42u8; 32];
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Test')",
                [],
            )
            .unwrap();

            // Set digest
            store
                .set_row_digest(conn, "epochs_data", 1, digest)
                .expect("should set digest");
        }

        // Get digest (from cache)
        let retrieved = store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should get digest");
        assert_eq!(retrieved, Some(digest));

        // Clear cache and get again (from DB)
        store.clear_cache();
        let retrieved = store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should get digest from DB");
        assert_eq!(retrieved, Some(digest));
    }

    #[test]
    fn row_digest_reads_persisted_state_before_cache() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        store.cache_row_digest("epochs_data", 1, [1u8; 32]);
        let persisted = [2u8; 32];
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Test')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO epochs_rev (rowid, digest) VALUES (1, ?1)",
                [persisted.as_slice()],
            )
            .unwrap();
        }

        let retrieved = store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should get digest");
        assert_eq!(retrieved, Some(persisted));
    }

    #[test]
    fn row_digest_cache_does_not_survive_rollback() {
        let db = open_memory_database().expect("should create database");
        let store = db.revision_store();
        let conn = db.connection();

        conn.execute_batch("BEGIN").unwrap();
        conn.execute(
            "INSERT INTO epochs (text_id, title, reviewed, sort_key)
             VALUES ('e1', 'Rolled back', 0, '')",
            [],
        )
        .unwrap();
        assert!(store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should read in-transaction digest")
            .is_some());

        conn.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            store
                .get_row_digest(conn, "epochs_data", 1)
                .expect("should read post-rollback digest"),
            None
        );
    }

    #[test]
    fn test_rowset_counter_bump() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        // Initial counter is 0
        let counter = store.get_rowset_counter("epochs_data");
        assert_eq!(counter, 0);

        // Bump counter
        let counter = store
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump");
        assert_eq!(counter, 1);

        // Bump again
        let counter = store
            .bump_rowset_counter(conn, "epochs_data")
            .expect("should bump");
        assert_eq!(counter, 2);

        // Verify persisted
        let persisted: i64 = conn
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'epochs_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, 2);
    }

    #[test]
    fn rowset_counter_from_db_refreshes_stale_cache() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        store.cache_rowset_counter("epochs_data", 0);
        conn.execute(
            "UPDATE rowset_revisions SET counter = 9 WHERE table_name = 'epochs_data'",
            [],
        )
        .unwrap();

        let counter = store
            .get_rowset_counter_from_db(conn, "epochs_data")
            .expect("should read persisted counter");
        assert_eq!(counter, 9);
        assert_eq!(store.get_rowset_counter("epochs_data"), 9);
    }

    #[test]
    fn test_remove_row_digest() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        // Insert a row
        let digest = [42u8; 32];
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Test')",
                [],
            )
            .unwrap();
            store
                .set_row_digest(conn, "epochs_data", 1, digest)
                .expect("should set");

            // Remove digest
            store
                .remove_row_digest(conn, "epochs_data", 1)
                .expect("should remove");
        }

        // Should be gone
        let retrieved = store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should query");
        assert_eq!(retrieved, None);
    }

    #[test]
    fn backfill_row_digests_clears_stale_persisted_rows_and_bumps_membership() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Test')",
                [],
            )
            .unwrap();
            store
                .backfill_row_digests(conn, "epochs_data")
                .expect("should backfill initial row");
        }

        let initial_counter = store.get_rowset_counter("epochs_data");
        assert_eq!(initial_counter, 1);
        assert!(store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should query row digest")
            .is_some());

        let store = RevisionStore::new(conn).expect("should recreate store");
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute("DELETE FROM epochs_data WHERE rowid = 1", [])
                .unwrap();
            store
                .backfill_row_digests(conn, "epochs_data")
                .expect("should backfill after direct delete");
        }

        assert_eq!(store.get_rowset_counter("epochs_data"), initial_counter + 1);
        assert_eq!(
            store
                .get_row_digest(conn, "epochs_data", 1)
                .expect("should query stale digest"),
            None
        );
        let persisted_rev_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM epochs_rev", [], |row| row.get(0))
            .unwrap();
        assert_eq!(persisted_rev_count, 0);
    }

    #[test]
    fn backfill_row_digests_bumps_rowset_on_content_change() {
        let db = open_memory_database().expect("should create database");
        let store = RevisionStore::new(db.connection()).expect("should create store");
        let conn = db.connection();

        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "INSERT INTO epochs_data (text_id, title) VALUES ('e1', 'Before')",
                [],
            )
            .unwrap();
            store
                .backfill_row_digests(conn, "epochs_data")
                .expect("should backfill initial row");
        }
        let initial_counter = store.get_rowset_counter("epochs_data");
        let initial_digest = store
            .get_row_digest(conn, "epochs_data", 1)
            .expect("should read initial digest");

        let store = RevisionStore::new(conn).expect("should recreate store");
        {
            let _guard = defensive_mode_disabled(conn);
            conn.execute(
                "UPDATE epochs_data SET title = 'After' WHERE text_id = 'e1'",
                [],
            )
            .unwrap();
            store
                .backfill_row_digests(conn, "epochs_data")
                .expect("should backfill changed row");
        }

        assert_eq!(store.get_rowset_counter("epochs_data"), initial_counter + 1);
        assert_ne!(
            store
                .get_row_digest(conn, "epochs_data", 1)
                .expect("should read changed digest"),
            initial_digest
        );
    }
}
