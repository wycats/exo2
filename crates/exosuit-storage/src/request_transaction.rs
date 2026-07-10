//! Request-scoped SQLite transactions.
//!
//! A daemon request executes synchronously on one blocking worker thread. This
//! scope lets every loader and writer opened by that request reuse one
//! connection and therefore participate in one SQLite transaction.

use crate::{open_database, Database, DatabaseError};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const REQUEST_TRANSACTION_BUSY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct RequestDatabaseBinding {
    path: PathBuf,
    database: Rc<Database>,
}

thread_local! {
    static REQUEST_DATABASE: RefCell<Option<RequestDatabaseBinding>> = const { RefCell::new(None) };
}

/// A request-owned SQLite transaction installed for loaders and writers on
/// the current thread.
#[derive(Debug)]
pub struct RequestTransaction {
    database: Rc<Database>,
    active: bool,
}

impl RequestTransaction {
    /// Open the project database, start an immediate transaction, and publish
    /// the connection to request-scoped loaders and writers.
    pub fn begin(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = normalized_database_path(path.as_ref())?;
        let occupied = REQUEST_DATABASE.with(|slot| slot.borrow().is_some());
        if occupied {
            return Err(DatabaseError::RequestScope(
                "a request transaction is already active on this thread".to_string(),
            ));
        }

        let database = Rc::new(open_database(&path)?);
        database
            .connection()
            .busy_timeout(REQUEST_TRANSACTION_BUSY_TIMEOUT)?;
        database.connection().execute_batch("BEGIN IMMEDIATE")?;
        REQUEST_DATABASE.with(|slot| {
            *slot.borrow_mut() = Some(RequestDatabaseBinding {
                path: path.clone(),
                database: Rc::clone(&database),
            });
        });

        Ok(Self {
            database,
            active: true,
        })
    }

    /// Return the database owned by this request transaction.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Commit the transaction and remove it from the current request scope.
    pub fn commit(mut self) -> Result<(), DatabaseError> {
        self.database.connection().execute_batch("COMMIT")?;
        self.clear_binding();
        self.active = false;
        Ok(())
    }

    /// Roll back the transaction and remove it from the current request scope.
    pub fn rollback(mut self) -> Result<(), DatabaseError> {
        self.database.connection().execute_batch("ROLLBACK")?;
        self.clear_binding();
        self.active = false;
        Ok(())
    }

    fn clear_binding(&self) {
        REQUEST_DATABASE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_some_and(|binding| Rc::ptr_eq(&binding.database, &self.database))
            {
                *slot = None;
            }
        });
    }
}

impl Drop for RequestTransaction {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.database.connection().execute_batch("ROLLBACK");
        self.clear_binding();
        self.active = false;
    }
}

/// Open a database for a loader or writer, reusing the active request
/// transaction when the path matches.
pub fn open_request_database(path: impl AsRef<Path>) -> Result<Rc<Database>, DatabaseError> {
    let path = normalized_database_path(path.as_ref())?;
    let active = REQUEST_DATABASE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|binding| (binding.path.clone(), Rc::clone(&binding.database)))
    });

    match active {
        Some((active_path, database)) if active_path == path => Ok(database),
        Some((active_path, _)) => Err(DatabaseError::RequestScope(format!(
            "request transaction for {} cannot open {}",
            active_path.display(),
            path.display()
        ))),
        None => Ok(Rc::new(open_database(&path)?)),
    }
}

/// Return the active request database when it owns `path`.
pub fn active_request_database(
    path: impl AsRef<Path>,
) -> Result<Option<Rc<Database>>, DatabaseError> {
    let path = normalized_database_path(path.as_ref())?;
    REQUEST_DATABASE.with(|slot| {
        let slot = slot.borrow();
        match slot.as_ref() {
            Some(binding) if binding.path == path => Ok(Some(Rc::clone(&binding.database))),
            Some(binding) => Err(DatabaseError::RequestScope(format!(
                "request transaction for {} cannot open {}",
                binding.path.display(),
                path.display()
            ))),
            None => Ok(None),
        }
    })
}

fn normalized_database_path(path: &Path) -> Result<PathBuf, DatabaseError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| DatabaseError::RequestScope(error.to_string()))?
            .join(path)
    };

    if let Ok(canonical) = absolute.canonicalize() {
        return Ok(canonical);
    }

    let Some(parent) = absolute.parent() else {
        return Ok(absolute);
    };
    let canonical_parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    Ok(absolute
        .file_name()
        .map_or(canonical_parent.clone(), |name| canonical_parent.join(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_database_reuses_one_connection_and_commits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");
        let transaction = RequestTransaction::begin(&path).expect("begin request transaction");
        let first = open_request_database(&path).expect("first request database");
        let second = open_request_database(&path).expect("second request database");
        assert!(Rc::ptr_eq(&first, &second));

        first
            .connection()
            .execute(
                "INSERT INTO epochs (text_id, title) VALUES ('epoch-a', 'Epoch A')",
                [],
            )
            .expect("insert epoch");
        transaction.commit().expect("commit request transaction");

        let reopened = open_database(&path).expect("reopen database");
        let count: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .expect("count epochs");
        assert_eq!(count, 1);
    }

    #[test]
    fn dropped_request_transaction_rolls_back() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("exo.db");
        {
            let _transaction = RequestTransaction::begin(&path).expect("begin request transaction");
            let database = open_request_database(&path).expect("request database");
            database
                .connection()
                .execute(
                    "INSERT INTO epochs (text_id, title) VALUES ('epoch-a', 'Epoch A')",
                    [],
                )
                .expect("insert epoch");
        }

        let reopened = open_database(&path).expect("reopen database");
        let count: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .expect("count epochs");
        assert_eq!(count, 0);
    }
}
