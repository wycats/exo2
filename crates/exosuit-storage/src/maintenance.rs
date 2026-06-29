//! Physical SQLite storage maintenance.
//!
//! This module handles file-level maintenance for Exo SQLite databases. It is
//! separate from reactive revision maintenance: revision rows describe logical
//! freshness, while SQLite page/WAL maintenance describes physical file shape.

use rusqlite::Connection;

/// Default number of pages to reclaim in one explicit maintenance pass.
pub const DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET: u32 = 256;

/// SQLite auto-vacuum mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoVacuumMode {
    None,
    Full,
    Incremental,
    Unknown(i64),
}

impl AutoVacuumMode {
    pub fn from_pragma(value: i64) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Full,
            2 => Self::Incremental,
            other => Self::Unknown(other),
        }
    }

    pub fn as_i64(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Full => 1,
            Self::Incremental => 2,
            Self::Unknown(value) => value,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Full => "full",
            Self::Incremental => "incremental",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// Options for a physical SQLite maintenance pass.
#[derive(Debug, Clone, Copy)]
pub struct StorageMaintenanceOptions {
    pub enable_incremental_vacuum: bool,
    pub vacuum_page_budget: u32,
    pub checkpoint_wal: bool,
}

impl Default for StorageMaintenanceOptions {
    fn default() -> Self {
        Self {
            enable_incremental_vacuum: false,
            vacuum_page_budget: DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET,
            checkpoint_wal: true,
        }
    }
}

/// A snapshot of physical SQLite storage metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageMaintenanceStats {
    pub page_size: i64,
    pub page_count: i64,
    pub freelist_count: i64,
    pub reclaimable_bytes: i64,
    pub auto_vacuum: AutoVacuumMode,
}

/// Result from a WAL checkpoint operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalCheckpointReport {
    pub mode: &'static str,
    pub busy: i64,
    pub log_pages: i64,
    pub checkpointed_pages: i64,
}

/// Report from a physical SQLite maintenance pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageMaintenanceReport {
    pub before: StorageMaintenanceStats,
    pub after: StorageMaintenanceStats,
    pub conversion_performed: bool,
    pub vacuum_performed: bool,
    pub incremental_vacuum_pages_requested: u32,
    pub incremental_vacuum_steps_run: u32,
    pub wal_checkpoint: Option<WalCheckpointReport>,
}

/// Read physical SQLite storage metrics.
pub fn storage_maintenance_stats(conn: &Connection) -> rusqlite::Result<StorageMaintenanceStats> {
    let page_size = pragma_i64(conn, "page_size")?;
    let page_count = pragma_i64(conn, "page_count")?;
    let freelist_count = pragma_i64(conn, "freelist_count")?;
    let auto_vacuum = AutoVacuumMode::from_pragma(pragma_i64(conn, "auto_vacuum")?);

    Ok(StorageMaintenanceStats {
        page_size,
        page_count,
        freelist_count,
        reclaimable_bytes: page_size.saturating_mul(freelist_count),
        auto_vacuum,
    })
}

/// Run bounded physical SQLite maintenance.
///
/// Existing databases are converted to incremental auto-vacuum only when
/// `enable_incremental_vacuum` is set. Conversion requires `VACUUM` so that
/// SQLite rebuilds the file with pointer-map pages.
pub fn maintain_database(
    conn: &Connection,
    options: StorageMaintenanceOptions,
) -> rusqlite::Result<StorageMaintenanceReport> {
    let before = storage_maintenance_stats(conn)?;
    let mut conversion_performed = false;
    let mut vacuum_performed = false;

    if options.enable_incremental_vacuum && before.auto_vacuum != AutoVacuumMode::Incremental {
        conn.pragma_update(None, "auto_vacuum", AutoVacuumMode::Incremental.as_i64())?;
        conn.execute_batch("VACUUM")?;
        conversion_performed = true;
        vacuum_performed = true;
    }

    let mut incremental_vacuum_steps_run = 0;
    let mut stats_after_conversion = storage_maintenance_stats(conn)?;

    if stats_after_conversion.auto_vacuum == AutoVacuumMode::Incremental {
        let budget = options
            .vacuum_page_budget
            .min(u32::try_from(stats_after_conversion.freelist_count).unwrap_or(u32::MAX));

        for _ in 0..budget {
            if stats_after_conversion.freelist_count <= 0 {
                break;
            }

            conn.execute_batch("PRAGMA incremental_vacuum(1)")?;
            incremental_vacuum_steps_run += 1;

            let next_stats = storage_maintenance_stats(conn)?;
            if next_stats.freelist_count >= stats_after_conversion.freelist_count {
                break;
            }
            stats_after_conversion = next_stats;
        }
    }

    let wal_checkpoint = if options.checkpoint_wal {
        Some(wal_checkpoint(
            conn,
            if conversion_performed {
                "TRUNCATE"
            } else {
                "PASSIVE"
            },
        )?)
    } else {
        None
    };

    let after = storage_maintenance_stats(conn)?;

    Ok(StorageMaintenanceReport {
        before,
        after,
        conversion_performed,
        vacuum_performed,
        incremental_vacuum_pages_requested: options.vacuum_page_budget,
        incremental_vacuum_steps_run,
        wal_checkpoint,
    })
}

fn pragma_i64(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
}

fn wal_checkpoint(conn: &Connection, mode: &'static str) -> rusqlite::Result<WalCheckpointReport> {
    conn.query_row(&format!("PRAGMA wal_checkpoint({mode})"), [], |row| {
        Ok(WalCheckpointReport {
            mode,
            busy: row.get(0)?,
            log_pages: row.get(1)?,
            checkpointed_pages: row.get(2)?,
        })
    })
}
