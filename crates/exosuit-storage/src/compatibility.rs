//! Cross-version writer compatibility for canonical Exo storage.

use rusqlite::{Connection, OpenFlags};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::maintenance::AutoVacuumMode;
use crate::migrations::{has_pending_migrations, run_migrations};
use crate::DatabaseError;

pub const SUPPORTED_WRITER_GENERATION: i32 = 0;
pub const MAX_WRITER_GENERATION: i32 = i32::MAX;
pub const PROJECTION_GENERATION_PREFIX: &str = "-- exo:minimum-writer-generation=";

const COMPATIBILITY_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const COMPATIBILITY_LOCK_POLL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSurface {
    Database,
    Projection,
}

impl StateSurface {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Projection => "projection",
        }
    }
}

#[derive(Debug, Error)]
pub enum WriterCompatibilityError {
    #[error(
        "storage requires writer generation {required_generation}, but this Exo supports {supported_generation}"
    )]
    Incompatible {
        required_generation: i32,
        supported_generation: i32,
        surface: StateSurface,
    },

    #[error("invalid {surface:?} writer compatibility metadata: {reason}")]
    MetadataInvalid {
        surface: StateSurface,
        reason: String,
    },

    #[error("timed out waiting for storage compatibility lock {}", lock_path.display())]
    Busy { lock_path: PathBuf },

    #[error("storage compatibility I/O failed at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl WriterCompatibilityError {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Incompatible { .. } => "storage.writer_incompatible",
            Self::MetadataInvalid { .. } => "storage.writer_metadata_invalid",
            Self::Busy { .. } => "storage.compatibility_busy",
            Self::Io { .. } => "storage.compatibility_io",
        }
    }

    pub const fn surface(&self) -> Option<StateSurface> {
        match self {
            Self::Incompatible { surface, .. } | Self::MetadataInvalid { surface, .. } => {
                Some(*surface)
            }
            Self::Busy { .. } | Self::Io { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LeaseMode {
    Shared,
    Exclusive,
}

#[derive(Debug)]
pub(crate) struct CompatibilityLease {
    file: File,
}

impl Drop for CompatibilityLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl CompatibilityLease {
    fn acquire(database_path: &Path, mode: LeaseMode) -> Result<Self, WriterCompatibilityError> {
        Self::acquire_with_timeout(database_path, mode, COMPATIBILITY_LOCK_TIMEOUT)
    }

    fn acquire_with_timeout(
        database_path: &Path,
        mode: LeaseMode,
        timeout: Duration,
    ) -> Result<Self, WriterCompatibilityError> {
        let path = database_path.with_extension("writer-compat.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| WriterCompatibilityError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| WriterCompatibilityError::Io {
                path: path.clone(),
                source,
            })?;

        let deadline = Instant::now() + timeout;
        loop {
            let result = match mode {
                LeaseMode::Shared => fs2::FileExt::try_lock_shared(&file),
                LeaseMode::Exclusive => fs2::FileExt::try_lock_exclusive(&file),
            };
            match result {
                Ok(()) => return Ok(Self { file }),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(WriterCompatibilityError::Busy { lock_path: path });
                    }
                    std::thread::park_timeout(COMPATIBILITY_LOCK_POLL);
                }
                Err(source) => {
                    return Err(WriterCompatibilityError::Io {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
    }
}

/// Exclusive writer authority retained across an orchestrated storage mutation.
///
/// Sidecar setup and semantic merge use this guard to keep the serialized
/// compatibility decision live from their final preflight through the first
/// policy/Git mutation and into the resulting semantic database connection.
#[derive(Debug)]
pub struct ExclusiveCompatibilityAuthority {
    database_path: PathBuf,
    lease: CompatibilityLease,
}

pub fn acquire_exclusive_compatibility_authority(
    path: impl AsRef<Path>,
) -> Result<ExclusiveCompatibilityAuthority, DatabaseError> {
    let database_path = normalize_database_identity(path.as_ref())?;
    let initial_generation = probe_database_generation_at_identity(&database_path)?;
    ensure_supported(initial_generation, StateSurface::Database)?;

    let lease = CompatibilityLease::acquire(&database_path, LeaseMode::Exclusive)?;
    let serialized_generation = probe_database_generation_at_identity(&database_path)?;
    ensure_supported(serialized_generation, StateSurface::Database)?;

    Ok(ExclusiveCompatibilityAuthority {
        database_path,
        lease,
    })
}

/// Resolve one canonical identity for both SQLite and its writer lock.
///
/// Existing database symlinks resolve to their target. For a database that
/// does not exist yet, the nearest existing parent is canonicalized and the
/// missing suffix is appended without creating anything.
pub fn normalize_database_identity(path: &Path) -> Result<PathBuf, WriterCompatibilityError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| WriterCompatibilityError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };

    match std::fs::canonicalize(&absolute) {
        Ok(canonical) => return Ok(canonical),
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
            return Err(WriterCompatibilityError::Io {
                path: absolute,
                source,
            });
        }
        Err(source) if std::fs::symlink_metadata(&absolute).is_ok() => {
            return Err(WriterCompatibilityError::Io {
                path: absolute,
                source,
            });
        }
        Err(_) => {}
    }

    let mut cursor = absolute.as_path();
    let mut missing = Vec::<OsString>::new();
    loop {
        match std::fs::canonicalize(cursor) {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = cursor.file_name() else {
                    return Err(WriterCompatibilityError::Io {
                        path: absolute,
                        source,
                    });
                };
                missing.push(name.to_os_string());
                let Some(parent) = cursor.parent() else {
                    return Err(WriterCompatibilityError::Io {
                        path: absolute,
                        source,
                    });
                };
                cursor = parent;
            }
            Err(source) => {
                return Err(WriterCompatibilityError::Io {
                    path: cursor.to_path_buf(),
                    source,
                });
            }
        }
    }
}

/// A raw canonical connection that retains shared writer authority.
pub struct FencedConnection {
    conn: Connection,
    _compatibility_lease: CompatibilityLease,
}

impl std::fmt::Debug for FencedConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FencedConnection")
            .finish_non_exhaustive()
    }
}

impl Deref for FencedConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl DerefMut for FencedConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
    }
}

pub fn probe_database_generation(path: &Path) -> Result<i32, DatabaseError> {
    let path = normalize_database_identity(path)?;
    probe_database_generation_at_identity(&path)
}

fn probe_database_generation_at_identity(path: &Path) -> Result<i32, DatabaseError> {
    match path.metadata() {
        Ok(metadata) if metadata.len() == 0 => return Ok(0),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => {
            return Err(WriterCompatibilityError::Io {
                path: path.to_path_buf(),
                source,
            }
            .into());
        }
    }

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    read_database_generation(&conn).map_err(Into::into)
}

/// Read and validate database writer metadata without creating or migrating it.
pub fn preflight_database(path: &Path) -> Result<i32, DatabaseError> {
    let generation = probe_database_generation(path)?;
    ensure_supported(generation, StateSurface::Database)?;
    Ok(generation)
}

pub fn parse_projection_generation(content: &str) -> Result<i32, WriterCompatibilityError> {
    let Some(first_line) = content.lines().next() else {
        return Ok(0);
    };
    if let Some(value) = first_line.strip_prefix(PROJECTION_GENERATION_PREFIX) {
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(projection_metadata_invalid(first_line));
        }
        let generation = value
            .parse::<i64>()
            .map_err(|_| projection_metadata_invalid(first_line))?;
        if !(0..=i64::from(MAX_WRITER_GENERATION)).contains(&generation) {
            return Err(projection_metadata_invalid(first_line));
        }
        return Ok(generation as i32);
    }
    if first_line.starts_with("-- exo:minimum-writer-generation") {
        return Err(projection_metadata_invalid(first_line));
    }
    Ok(0)
}

pub fn render_projection_generation_header(
    generation: i32,
) -> Result<String, WriterCompatibilityError> {
    if !(0..=MAX_WRITER_GENERATION).contains(&generation) {
        return Err(WriterCompatibilityError::MetadataInvalid {
            surface: StateSurface::Projection,
            reason: format!("generation {generation} is outside 0..={MAX_WRITER_GENERATION}"),
        });
    }
    Ok(format!("{PROJECTION_GENERATION_PREFIX}{generation}"))
}

pub fn with_projection_generation(
    content: &str,
    generation: i32,
) -> Result<String, WriterCompatibilityError> {
    let header = render_projection_generation_header(generation)?;
    let body = if content
        .lines()
        .next()
        .is_some_and(|line| line.starts_with(PROJECTION_GENERATION_PREFIX))
    {
        content.split_once('\n').map_or("", |(_, body)| body)
    } else {
        content
    };
    if body.is_empty() {
        Ok(format!("{header}\n"))
    } else {
        Ok(format!("{header}\n{body}"))
    }
}

pub fn ensure_projection_supported(generation: i32) -> Result<(), WriterCompatibilityError> {
    ensure_supported(generation, StateSurface::Projection)
}

pub fn open_fenced_connection(path: impl AsRef<Path>) -> Result<FencedConnection, DatabaseError> {
    let (conn, lease) = open_database_connection(path.as_ref())?;
    Ok(FencedConnection {
        conn,
        _compatibility_lease: lease,
    })
}

/// Open a canonical database for physical maintenance without running logical
/// migrations, while still retaining compatible shared writer authority.
pub fn open_fenced_physical_connection(
    path: impl AsRef<Path>,
) -> Result<FencedConnection, DatabaseError> {
    let path = normalize_database_identity(path.as_ref())?;
    let initial_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(initial_generation, StateSurface::Database)?;
    let lease = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
    let serialized_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(serialized_generation, StateSurface::Database)?;

    let is_new = path.metadata().map_or(true, |metadata| metadata.len() == 0);
    let conn = Connection::open(&path)?;
    if is_new {
        conn.pragma_update(None, "auto_vacuum", AutoVacuumMode::Incremental.as_i64())?;
    }
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(FencedConnection {
        conn,
        _compatibility_lease: lease,
    })
}

pub fn open_fenced_connection_for_import(
    path: impl AsRef<Path>,
    projection_generation: i32,
) -> Result<FencedConnection, DatabaseError> {
    ensure_supported(projection_generation, StateSurface::Projection)?;
    let path = normalize_database_identity(path.as_ref())?;
    let initial_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(initial_generation, StateSurface::Database)?;

    let exclusive = CompatibilityLease::acquire(&path, LeaseMode::Exclusive)?;
    let serialized_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(serialized_generation, StateSurface::Database)?;
    let conn = open_configured_connection(&path)?;
    run_migrations(&conn)?;
    let resulting_generation = read_database_generation(&conn)?.max(projection_generation);
    set_database_generation(&conn, resulting_generation)?;
    drop(conn);
    drop(exclusive);

    let shared = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
    let generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = open_configured_connection(&path)?;
    Ok(FencedConnection {
        conn,
        _compatibility_lease: shared,
    })
}

pub fn open_fenced_connection_for_import_with_authority(
    authority: ExclusiveCompatibilityAuthority,
    projection_generation: i32,
) -> Result<FencedConnection, DatabaseError> {
    ensure_supported(projection_generation, StateSurface::Projection)?;
    let ExclusiveCompatibilityAuthority {
        database_path,
        lease,
    } = authority;
    let generation = probe_database_generation_at_identity(&database_path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = open_configured_connection(&database_path)?;
    run_migrations(&conn)?;
    let resulting_generation = read_database_generation(&conn)?.max(projection_generation);
    set_database_generation(&conn, resulting_generation)?;
    Ok(FencedConnection {
        conn,
        _compatibility_lease: lease,
    })
}

pub fn open_fenced_existing_connection(
    path: impl AsRef<Path>,
) -> Result<Option<FencedConnection>, DatabaseError> {
    let path = normalize_database_identity(path.as_ref())?;
    if !path.exists() {
        return Ok(None);
    }
    let generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let lease = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
    let generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(Some(FencedConnection {
        conn,
        _compatibility_lease: lease,
    }))
}

/// Open an existing canonical database read-only while retaining compatible
/// shared writer authority. This path performs no migrations or journal-mode
/// changes and never creates the database.
pub fn open_fenced_read_only_connection(
    path: impl AsRef<Path>,
) -> Result<Option<FencedConnection>, DatabaseError> {
    let path = normalize_database_identity(path.as_ref())?;
    if !path.exists() {
        return Ok(None);
    }
    let generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let lease = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
    let generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(Some(FencedConnection {
        conn,
        _compatibility_lease: lease,
    }))
}

pub(crate) fn open_database_connection(
    path: &Path,
) -> Result<(Connection, CompatibilityLease), DatabaseError> {
    open_database_connection_with_hook(path, || {})
}

pub(crate) fn open_database_connection_with_exclusive_authority(
    authority: ExclusiveCompatibilityAuthority,
) -> Result<(Connection, CompatibilityLease), DatabaseError> {
    let ExclusiveCompatibilityAuthority {
        database_path,
        lease,
    } = authority;
    let generation = probe_database_generation_at_identity(&database_path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = open_configured_connection(&database_path)?;
    run_migrations(&conn)?;
    let resulting_generation = read_database_generation(&conn)?;
    ensure_supported(resulting_generation, StateSurface::Database)?;
    Ok((conn, lease))
}

fn open_database_connection_with_hook(
    path: &Path,
    after_initial_probe: impl FnOnce(),
) -> Result<(Connection, CompatibilityLease), DatabaseError> {
    let path = normalize_database_identity(path)?;
    let initial_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(initial_generation, StateSurface::Database)?;
    after_initial_probe();

    let shared = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
    let serialized_generation = probe_database_generation_at_identity(&path)?;
    ensure_supported(serialized_generation, StateSurface::Database)?;

    if database_has_pending_migrations(&path)? {
        drop(shared);
        migrate_under_exclusive_lease(&path)?;
        let shared = CompatibilityLease::acquire(&path, LeaseMode::Shared)?;
        let generation = probe_database_generation_at_identity(&path)?;
        ensure_supported(generation, StateSurface::Database)?;
        let conn = open_configured_connection(&path)?;
        return Ok((conn, shared));
    }

    let conn = open_configured_connection(&path)?;
    Ok((conn, shared))
}

fn migrate_under_exclusive_lease(path: &Path) -> Result<(), DatabaseError> {
    let _exclusive = CompatibilityLease::acquire(path, LeaseMode::Exclusive)?;
    let generation = probe_database_generation_at_identity(path)?;
    ensure_supported(generation, StateSurface::Database)?;
    let conn = open_configured_connection(path)?;
    run_migrations(&conn)?;
    let resulting_generation = read_database_generation(&conn)?;
    ensure_supported(resulting_generation, StateSurface::Database)?;
    Ok(())
}

fn open_configured_connection(path: &Path) -> Result<Connection, DatabaseError> {
    let is_new = path.metadata().map_or(true, |metadata| metadata.len() == 0);
    let conn = Connection::open(path)?;
    if is_new {
        conn.pragma_update(None, "auto_vacuum", AutoVacuumMode::Incremental.as_i64())?;
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(conn)
}

fn database_has_pending_migrations(path: &Path) -> Result<bool, DatabaseError> {
    if !path.exists() || path.metadata().is_ok_and(|metadata| metadata.len() == 0) {
        return Ok(true);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    has_pending_migrations(&conn)
}

pub(crate) fn read_database_generation(conn: &Connection) -> Result<i32, WriterCompatibilityError> {
    let generation = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| WriterCompatibilityError::MetadataInvalid {
            surface: StateSurface::Database,
            reason: error.to_string(),
        })?;
    if !(0..=i64::from(MAX_WRITER_GENERATION)).contains(&generation) {
        return Err(WriterCompatibilityError::MetadataInvalid {
            surface: StateSurface::Database,
            reason: format!("generation {generation} is outside 0..={MAX_WRITER_GENERATION}"),
        });
    }
    Ok(generation as i32)
}

pub(crate) fn set_database_generation(
    conn: &Connection,
    generation: i32,
) -> Result<(), DatabaseError> {
    if !(0..=MAX_WRITER_GENERATION).contains(&generation) {
        return Err(WriterCompatibilityError::MetadataInvalid {
            surface: StateSurface::Database,
            reason: format!("generation {generation} is outside 0..={MAX_WRITER_GENERATION}"),
        }
        .into());
    }
    conn.pragma_update(None, "user_version", generation)?;
    Ok(())
}

fn ensure_supported(
    required_generation: i32,
    surface: StateSurface,
) -> Result<(), WriterCompatibilityError> {
    if required_generation > SUPPORTED_WRITER_GENERATION {
        return Err(WriterCompatibilityError::Incompatible {
            required_generation,
            supported_generation: SUPPORTED_WRITER_GENERATION,
            surface,
        });
    }
    Ok(())
}

fn projection_metadata_invalid(line: &str) -> WriterCompatibilityError {
    WriterCompatibilityError::MetadataInvalid {
        surface: StateSurface::Projection,
        reason: format!("invalid epochs.sql header {line:?}"),
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Cross-process storage tests use sync fs, process, and timing APIs.
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    const LOCK_HELPER_DB_ENV: &str = "EXO_TEST_COMPATIBILITY_LOCK_DB";
    const LOCK_HELPER_ATTEMPTED_ENV: &str = "EXO_TEST_COMPATIBILITY_LOCK_ATTEMPTED";
    const LOCK_HELPER_ACQUIRED_ENV: &str = "EXO_TEST_COMPATIBILITY_LOCK_ACQUIRED";

    #[test]
    fn compatibility_lock_subprocess_helper() {
        let Some(path) = std::env::var_os(LOCK_HELPER_DB_ENV).map(PathBuf::from) else {
            return;
        };
        let attempted = PathBuf::from(
            std::env::var_os(LOCK_HELPER_ATTEMPTED_ENV).expect("attempted marker path"),
        );
        let acquired = PathBuf::from(
            std::env::var_os(LOCK_HELPER_ACQUIRED_ENV).expect("acquired marker path"),
        );
        std::fs::write(&attempted, b"attempting").expect("write attempted marker");

        let authority = acquire_exclusive_compatibility_authority(&path)
            .expect("acquire cross-process exclusive authority");
        let advancing = Connection::open(&path).expect("open database while authority is held");
        set_database_generation(&advancing, 1).expect("advance writer generation");
        drop(advancing);
        std::fs::write(&acquired, b"acquired").expect("write acquired marker");
        drop(authority);
    }

    #[test]
    fn projection_generation_is_strict_and_legacy_defaults_to_zero() {
        assert_eq!(
            parse_projection_generation("INSERT INTO epochs_data VALUES (...);\n").unwrap(),
            0
        );
        assert_eq!(
            parse_projection_generation("-- exo:minimum-writer-generation=12\nINSERT;\n").unwrap(),
            12
        );
        for invalid in [
            "-- exo:minimum-writer-generation=01\n",
            "-- exo:minimum-writer-generation=-1\n",
            "-- exo:minimum-writer-generation=2147483648\n",
            "-- exo:minimum-writer-generation =1\n",
        ] {
            assert!(matches!(
                parse_projection_generation(invalid),
                Err(WriterCompatibilityError::MetadataInvalid { .. })
            ));
        }
    }

    #[test]
    fn incompatible_database_probe_does_not_mutate_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('same');",
        )
        .unwrap();
        drop(conn);
        let before = std::fs::read(&path).unwrap();

        let error = open_database_connection(&path).unwrap_err();
        assert!(matches!(
            error,
            DatabaseError::WriterCompatibility(WriterCompatibilityError::Incompatible {
                required_generation: 1,
                supported_generation: 0,
                surface: StateSurface::Database,
            })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!path.with_extension("writer-compat.lock").exists());
    }

    #[test]
    fn negative_database_generation_is_metadata_invalid_before_lock_creation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", -1_i64).unwrap();
        drop(conn);

        let error = open_database_connection(&path).unwrap_err();

        assert!(matches!(
            error,
            DatabaseError::WriterCompatibility(WriterCompatibilityError::MetadataInvalid {
                surface: StateSurface::Database,
                ..
            })
        ));
        assert!(!path.with_extension("writer-compat.lock").exists());
    }

    #[test]
    fn live_wal_incompatibility_rejects_without_logical_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "wal").unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('same');",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        assert!(path.with_extension("db-wal").exists());

        let error = open_database_connection(&path).unwrap_err();

        assert!(matches!(
            error,
            DatabaseError::WriterCompatibility(WriterCompatibilityError::Incompatible {
                required_generation: 1,
                supported_generation: 0,
                surface: StateSurface::Database,
            })
        ));
        assert!(
            conn.is_autocommit(),
            "rejection must not begin a write transaction"
        );
        assert_eq!(
            conn.query_row("SELECT value FROM sentinel", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "same"
        );
        assert_eq!(read_database_generation(&conn).unwrap(), 1);
        assert!(!path.with_extension("writer-compat.lock").exists());
    }

    #[test]
    fn exclusive_lock_is_bounded_while_shared_semantic_lease_is_live() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let shared = CompatibilityLease::acquire_with_timeout(
            &path,
            LeaseMode::Shared,
            Duration::from_millis(50),
        )
        .unwrap();
        let error = CompatibilityLease::acquire_with_timeout(
            &path,
            LeaseMode::Exclusive,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(matches!(error, WriterCompatibilityError::Busy { .. }));
        drop(shared);
        CompatibilityLease::acquire_with_timeout(
            &path,
            LeaseMode::Exclusive,
            Duration::from_millis(50),
        )
        .unwrap();
    }

    #[test]
    fn independent_process_generation_advance_waits_for_shared_semantic_lease() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let attempted = temp.path().join("attempted");
        let acquired = temp.path().join("acquired");
        let shared = open_fenced_connection(&path).expect("open shared semantic connection");

        let mut child = Command::new(std::env::current_exe().expect("current test executable"))
            .args([
                "--exact",
                "compatibility::tests::compatibility_lock_subprocess_helper",
                "--nocapture",
            ])
            .env(LOCK_HELPER_DB_ENV, &path)
            .env(LOCK_HELPER_ATTEMPTED_ENV, &attempted)
            .env(LOCK_HELPER_ACQUIRED_ENV, &acquired)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn compatibility helper");

        wait_for_test_marker(&mut child, &attempted);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            child
                .try_wait()
                .expect("poll compatibility helper")
                .is_none(),
            "exclusive helper must remain blocked while the shared lease is live"
        );
        assert!(!acquired.exists());

        drop(shared);
        let output = child
            .wait_with_output()
            .expect("collect compatibility helper output");
        assert!(
            output.status.success(),
            "compatibility helper failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(acquired.exists());
        let advanced = Connection::open(&path).expect("reopen advanced database");
        assert_eq!(read_database_generation(&advanced).unwrap(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn database_aliases_share_one_sqlite_and_lock_identity() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let alias_dir = temp.path().join("alias");
        symlink(&real_dir, &alias_dir).unwrap();

        let real_path = real_dir.join("exo.db");
        let alias_path = alias_dir.join("exo.db");
        assert_eq!(
            normalize_database_identity(&real_path).unwrap(),
            normalize_database_identity(&alias_path).unwrap()
        );

        let shared = CompatibilityLease::acquire_with_timeout(
            &normalize_database_identity(&real_path).unwrap(),
            LeaseMode::Shared,
            Duration::from_millis(50),
        )
        .unwrap();
        let error = CompatibilityLease::acquire_with_timeout(
            &normalize_database_identity(&alias_path).unwrap(),
            LeaseMode::Exclusive,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(matches!(error, WriterCompatibilityError::Busy { .. }));
        drop(shared);

        drop(Connection::open(&real_path).unwrap());
        let file_alias = temp.path().join("file-alias.db");
        symlink(&real_path, &file_alias).unwrap();
        assert_eq!(
            normalize_database_identity(&real_path).unwrap(),
            normalize_database_identity(&file_alias).unwrap()
        );
    }

    #[test]
    fn serialized_probe_rejects_generation_advanced_after_initial_probe() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('same');",
        )
        .unwrap();
        drop(conn);

        let error = open_database_connection_with_hook(&path, || {
            let advancing = Connection::open(&path).unwrap();
            advancing.pragma_update(None, "user_version", 1).unwrap();
        })
        .unwrap_err();

        assert!(matches!(
            error,
            DatabaseError::WriterCompatibility(WriterCompatibilityError::Incompatible {
                required_generation: 1,
                supported_generation: 0,
                surface: StateSurface::Database,
            })
        ));
        let unchanged = Connection::open(&path).unwrap();
        assert_eq!(
            unchanged
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "same"
        );
        assert_eq!(read_database_generation(&unchanged).unwrap(), 1);
    }

    #[test]
    fn exclusive_authority_transfers_into_database_semantic_lifetime() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let authority = acquire_exclusive_compatibility_authority(&path).unwrap();
        let (conn, lease) = open_database_connection_with_exclusive_authority(authority).unwrap();

        let error = CompatibilityLease::acquire_with_timeout(
            &normalize_database_identity(&path).unwrap(),
            LeaseMode::Exclusive,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(matches!(error, WriterCompatibilityError::Busy { .. }));

        drop(conn);
        drop(lease);
        CompatibilityLease::acquire_with_timeout(
            &normalize_database_identity(&path).unwrap(),
            LeaseMode::Exclusive,
            Duration::from_millis(50),
        )
        .unwrap();
    }

    #[test]
    fn read_only_fenced_connection_preserves_existing_journal_mode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("exo.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "journal_mode", "delete")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('same');",
            )
            .unwrap();
        drop(connection);

        let read_only = open_fenced_read_only_connection(&path)
            .unwrap()
            .expect("existing database");
        assert_eq!(
            read_only
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "delete"
        );
        assert_eq!(
            read_only
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "same"
        );
        assert!(!path.with_extension("db-wal").exists());
    }

    fn wait_for_test_marker(child: &mut std::process::Child, marker: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if marker.exists() {
                return;
            }
            if child
                .try_wait()
                .expect("poll compatibility helper")
                .is_some()
            {
                panic!(
                    "compatibility helper exited before marker {}",
                    marker.display()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for marker {}", marker.display());
    }
}
