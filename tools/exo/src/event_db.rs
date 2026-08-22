//! Shared connection cache for the `agent_events` table.
//!
//! Event logging and activity projections run inside the daemon request
//! path. Opening a fresh connection per call (with WAL pragma + migrations)
//! caused lock contention under concurrent requests: one open wedged inside
//! SQLite while every other request queued behind it until clients timed
//! out. All `agent_events` access goes through this cache instead.
//!
//! The cache never creates database files. If the DB doesn't exist yet
//! (init hasn't run), callers get `None` — schema creation belongs to
//! init/migrations, not event logging.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use exosuit_storage::FencedConnection;
use exosuit_storage::rusqlite::{Connection, ErrorCode};

/// Per-path connections. The map lock is only held to look up or insert an
/// entry; queries run under the per-connection lock so different DBs don't
/// serialize each other and `f` can't deadlock against the map.
static CONNECTIONS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<FencedConnection>>>>> =
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

/// Run `f` with the cached connection for `db_path`, opening it if needed.
///
/// Returns `None` if the DB file doesn't exist, the connection can't be
/// opened, or `f` fails. The connection is evicted only when the error
/// indicates the DB file itself is broken or replaced — ordinary SQL errors
/// keep it cached so persistent failures don't reintroduce per-call opens.
pub fn with_event_db<T>(
    db_path: &Path,
    f: impl FnOnce(&Connection) -> exosuit_storage::rusqlite::Result<T>,
) -> anyhow::Result<Option<T>> {
    let entry = {
        let mut connections = lock_unpoisoned(&CONNECTIONS);
        match connections.get(db_path) {
            Some(entry) => Arc::clone(entry),
            None => {
                if !db_path.exists() {
                    return Ok(None);
                }
                // No-create open: event access must never mint a fresh DB.
                let Some(conn) = exosuit_storage::open_fenced_existing_connection(db_path)
                    .map_err(crate::storage_compatibility::map_database_error)?
                else {
                    return Ok(None);
                };
                let entry = Arc::new(Mutex::new(conn));
                connections.insert(db_path.to_path_buf(), Arc::clone(&entry));
                entry
            }
        }
        // Map lock released here; only the per-connection lock is held below.
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

#[cfg(test)]
mod tests {
    use super::*;

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
