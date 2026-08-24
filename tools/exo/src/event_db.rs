//! Shared active-connection registry for the `agent_events` table.
//!
//! Event logging and activity projections run inside the daemon request
//! path. Opening independent connections for concurrent calls (with WAL pragma
//! + migrations) caused lock contention: one open wedged inside SQLite while
//! every other request queued behind it until clients timed out. Concurrent
//! `agent_events` access therefore shares a per-path connection, while the
//! registry retains only a weak reference so an idle daemon releases its
//! storage compatibility lease between calls.
//!
//! The cache never creates database files. If the DB doesn't exist yet
//! (init hasn't run), callers get `None` — schema creation belongs to
//! init/migrations, not event logging.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use exosuit_storage::FencedConnection;
use exosuit_storage::rusqlite::{Connection, ErrorCode};

/// Per-path active connections. The map lock is only held to look up or insert
/// a weak entry; queries run under the per-connection lock so different DBs
/// don't serialize each other and `f` can't deadlock against the map. Once the
/// final active caller returns, the connection and its compatibility lease are
/// released.
static CONNECTIONS: LazyLock<Mutex<HashMap<PathBuf, Weak<Mutex<FencedConnection>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How long a resolved root → DB path stays valid before re-resolving.
///
/// Project policy can change while the daemon is alive (`sidecar unlink`,
/// edits to projects.toml). Re-resolving on a short interval bounds how long
/// reads can lag a policy change without paying the git subprocess cost on
/// every projection query.
const DB_PATH_TTL: Duration = Duration::from_secs(5);

/// Resolved DB path per workspace root, with the resolution timestamp.
static DB_PATHS: LazyLock<Mutex<HashMap<PathBuf, (Instant, PathBuf)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Recover the inner value from a poisoned mutex.
///
/// A panic inside a callback must not permanently disable event access; the
/// cached state is a plain map and stays structurally valid.
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The project-resolved `agent_events` DB path for a workspace root.
///
/// Events are written to the project DB (sidecar/shadow policies move it out
/// of the repo), so reads must resolve the same path rather than assuming
/// the legacy repo-relative location. Resolutions are cached for
/// [`DB_PATH_TTL`] to avoid a git subprocess per projection query.
pub fn event_db_path(root: &Path) -> PathBuf {
    let mut cache = lock_unpoisoned(&DB_PATHS);
    if let Some((resolved_at, path)) = cache.get(root)
        && resolved_at.elapsed() < DB_PATH_TTL
    {
        return path.clone();
    }
    let path = crate::context::db_path_resolving_project(root);
    cache.insert(root.to_path_buf(), (Instant::now(), path.clone()));
    path
}

/// Whether a rusqlite error means the cached connection itself is unusable
/// (DB file replaced or corrupted), as opposed to an ordinary SQL failure.
///
/// Busy/locked are deliberately not here: they're transient contention and
/// the connection stays valid.
fn connection_is_broken(error: &exosuit_storage::rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(
            ErrorCode::NotADatabase
                | ErrorCode::DatabaseCorrupt
                | ErrorCode::CannotOpen
                | ErrorCode::SystemIoFailure,
        )
    )
}

/// Run `f` with the active connection for `db_path`, opening it if needed.
///
/// Returns `None` if the DB file doesn't exist, the connection can't be
/// opened, or `f` fails. Concurrent callers share a connection, but the
/// registry does not keep it alive after the final active call returns.
pub fn with_event_db<T>(
    db_path: &Path,
    f: impl FnOnce(&Connection) -> exosuit_storage::rusqlite::Result<T>,
) -> anyhow::Result<Option<T>> {
    with_event_db_before_open(db_path, || {}, f)
}

fn with_event_db_before_open<T>(
    db_path: &Path,
    before_open: impl FnOnce(),
    f: impl FnOnce(&Connection) -> exosuit_storage::rusqlite::Result<T>,
) -> anyhow::Result<Option<T>> {
    let cached = lock_unpoisoned(&CONNECTIONS)
        .get(db_path)
        .and_then(Weak::upgrade);
    let entry = match cached {
        Some(entry) => entry,
        None => {
            if !db_path.exists() {
                return Ok(None);
            }
            before_open();
            // No-create open: event access must never mint a fresh DB. This
            // may wait on per-database writer authority, so it remains outside
            // the process-global registry lock.
            let Some(conn) = exosuit_storage::open_fenced_existing_connection(db_path)
                .map_err(crate::storage_compatibility::map_database_error)?
            else {
                return Ok(None);
            };
            let candidate = Arc::new(Mutex::new(conn));
            let mut connections = lock_unpoisoned(&CONNECTIONS);
            match connections.get(db_path).and_then(Weak::upgrade) {
                Some(entry) => entry,
                None => {
                    connections.insert(db_path.to_path_buf(), Arc::downgrade(&candidate));
                    candidate
                }
            }
        }
    };

    let conn = lock_unpoisoned(&entry);
    match f(&conn) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            if connection_is_broken(&error) {
                lock_unpoisoned(&CONNECTIONS).remove(db_path);
            }
            Ok(None)
        }
    }
}

/// Run `f` once against an existing event database without retaining a cached
/// connection or compatibility lease.
///
/// Startup maintenance uses this path before the daemon advertises readiness.
/// The connection and its shared writer authority are dropped before this
/// function returns, allowing a newer compatible writer to migrate the same
/// database immediately afterward. Like [`with_event_db`], this never creates
/// a missing database and treats event-query failures as best-effort misses.
pub fn with_uncached_existing_event_db<T>(
    db_path: &Path,
    f: impl FnOnce(&Connection) -> exosuit_storage::rusqlite::Result<T>,
) -> anyhow::Result<Option<T>> {
    let Some(conn) = exosuit_storage::open_fenced_existing_connection(db_path)
        .map_err(crate::storage_compatibility::map_database_error)?
    else {
        return Ok(None);
    };

    match f(&conn) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncached_event_access_does_not_create_or_retain_a_connection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");

        assert_eq!(
            with_uncached_existing_event_db(&path, |_| Ok(())).expect("missing database"),
            None
        );
        assert!(!path.exists(), "one-shot access must not create a database");

        drop(exosuit_storage::open_database(&path).expect("create compatible database"));
        assert_eq!(
            with_uncached_existing_event_db(&path, |conn| {
                conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            })
            .expect("read existing database"),
            Some(1)
        );
        assert!(
            !lock_unpoisoned(&CONNECTIONS).contains_key(&path),
            "one-shot access must not populate the shared connection cache"
        );

        drop(
            exosuit_storage::acquire_exclusive_compatibility_authority(&path)
                .expect("one-shot access releases writer authority before returning"),
        );
    }

    #[test]
    fn normal_event_access_releases_its_compatibility_lease_after_use() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");
        drop(exosuit_storage::open_database(&path).expect("create compatible database"));

        assert_eq!(
            with_event_db(&path, |conn| {
                conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            })
            .expect("read through event cache"),
            Some(1)
        );
        let connections = lock_unpoisoned(&CONNECTIONS);
        let cached = connections.get(&path).expect("active registry entry");
        assert!(
            cached.upgrade().is_none(),
            "normal event access must not retain an idle connection"
        );
        drop(connections);

        lock_unpoisoned(&CONNECTIONS).remove(&path);
        drop(
            exosuit_storage::acquire_exclusive_compatibility_authority(&path)
                .expect("event access releases writer authority before returning"),
        );
    }

    #[test]
    fn event_open_does_not_hold_the_global_registry_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");
        drop(exosuit_storage::open_database(&path).expect("create compatible database"));
        lock_unpoisoned(&CONNECTIONS).remove(&path);

        assert_eq!(
            with_event_db_before_open(
                &path,
                || {
                    assert!(
                        CONNECTIONS.try_lock().is_ok(),
                        "a per-database compatibility wait must not hold the global registry"
                    );
                },
                |conn| conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0)),
            )
            .expect("read through event cache"),
            Some(1)
        );
        lock_unpoisoned(&CONNECTIONS).remove(&path);
    }

    #[test]
    fn event_consumer_preserves_writer_compatibility_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");
        let connection = exosuit_storage::Connection::open(&path).expect("create database");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("raise writer generation");
        drop(connection);

        let error = with_event_db(&path, |_| Ok(())).expect_err("reject newer writer");
        let failure = error
            .downcast_ref::<crate::failure::ExoFailure>()
            .expect("typed compatibility failure");
        assert_eq!(
            failure.error.details.as_ref().unwrap()["kind"],
            "storage.writer_incompatible"
        );
        assert_eq!(
            failure.error.details.as_ref().unwrap()["request_outcome_checked"],
            false
        );
        assert_eq!(
            failure.error.details.as_ref().unwrap()["retry_with_same_request_id"],
            true
        );
    }
}
