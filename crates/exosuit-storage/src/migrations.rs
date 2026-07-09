//! Database migrations
//!
//! Simple migration runner that embeds SQL files at compile time.

use rusqlite::Connection;
use std::collections::HashSet;

use crate::DatabaseError;

/// A migration with version, name, and SQL content.
struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

/// All migrations, embedded at compile time.
/// Add new migrations here in order.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "core_tables",
        sql: include_str!("../migrations/V001__core_tables.sql"),
    },
    Migration {
        version: 2,
        name: "shadow_tables",
        sql: include_str!("../migrations/V002__shadow_tables.sql"),
    },
    Migration {
        version: 3,
        name: "revision_tables",
        sql: include_str!("../migrations/V003__revision_tables.sql"),
    },
    Migration {
        version: 4,
        name: "ideas_table",
        sql: include_str!("../migrations/V004__ideas_table.sql"),
    },
    Migration {
        version: 5,
        name: "inbox_table",
        sql: include_str!("../migrations/V005__inbox_table.sql"),
    },
    Migration {
        version: 6,
        name: "expand_status_constraints",
        sql: include_str!("../migrations/V006__expand_status_constraints.sql"),
    },
    Migration {
        version: 7,
        name: "add_sort_key",
        sql: include_str!("../migrations/V007__add_sort_key.sql"),
    },
    Migration {
        version: 8,
        name: "task_logs_and_verifications",
        sql: include_str!("../migrations/V008__task_logs_and_verifications.sql"),
    },
    Migration {
        version: 9,
        name: "task_notes_and_started_at",
        sql: include_str!("../migrations/V009__task_notes_and_started_at.sql"),
    },
    Migration {
        version: 10,
        name: "epoch_sort_key",
        sql: include_str!("../migrations/V010__epoch_sort_key.sql"),
    },
    Migration {
        version: 11,
        name: "persistent_rowset_counters",
        sql: include_str!("../migrations/V011__persistent_rowset_counters.sql"),
    },
    Migration {
        version: 12,
        name: "axioms_table",
        sql: include_str!("../migrations/V012__axioms_table.sql"),
    },
    Migration {
        version: 13,
        name: "perception_event_schema",
        sql: include_str!("../migrations/V013__perception_event_schema.sql"),
    },
    Migration {
        version: 14,
        name: "agent_id",
        sql: include_str!("../migrations/V014__agent_id.sql"),
    },
    Migration {
        version: 15,
        name: "rfcs_table",
        sql: include_str!("../migrations/V015__rfcs_table.sql"),
    },
    Migration {
        version: 16,
        name: "agent_events",
        sql: include_str!("../migrations/V016__agent_events.sql"),
    },
    Migration {
        version: 17,
        name: "workspace_active_phase",
        sql: include_str!("../migrations/V017__workspace_active_phase.sql"),
    },
    Migration {
        version: 18,
        name: "inbox_action_payload",
        sql: include_str!("../migrations/V018__inbox_action_payload.sql"),
    },
    Migration {
        version: 19,
        name: "phase_ownership",
        sql: include_str!("../migrations/V019__phase_ownership.sql"),
    },
    Migration {
        version: 20,
        name: "reactive_revision_coverage",
        sql: include_str!("../migrations/V020__reactive_revision_coverage.sql"),
    },
    Migration {
        version: 22,
        name: "rfc_workspace_observations",
        sql: include_str!("../migrations/V022__rfc_workspace_observations.sql"),
    },
];

/// Run all pending migrations on the given connection.
pub fn run_migrations(conn: &Connection) -> Result<(), DatabaseError> {
    // Enable foreign keys before running migrations
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    // Create migration tracking table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS __schema_history (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    // Get already applied versions
    let applied: HashSet<u32> = conn
        .prepare("SELECT version FROM __schema_history")?
        .query_map([], |row| row.get::<_, i32>(0).map(|v| v as u32))?
        .collect::<Result<_, _>>()?;

    // Apply pending migrations in order
    for migration in MIGRATIONS {
        if applied.contains(&migration.version) {
            continue;
        }

        // Migration progress is intentionally silent. The storage library
        // should not write to stderr — that's the CLI's responsibility.
        // Callers can detect applied migrations via the return value if needed.
        // Execute migration SQL
        conn.execute_batch(migration.sql)?;

        // Record migration
        conn.execute(
            "INSERT INTO __schema_history (version, name) VALUES (?1, ?2)",
            (migration.version, migration.name),
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn v018_recreates_existing_inbox_vtab_with_action_payload_column() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        {
            let conn = Connection::open(&db_path).expect("open db");
            conn.execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS __schema_history (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );",
            )
            .expect("create history");

            for migration in MIGRATIONS
                .iter()
                .filter(|migration| migration.version <= 17)
            {
                conn.execute_batch(migration.sql)
                    .unwrap_or_else(|err| panic!("apply V{:03}: {err}", migration.version));
                conn.execute(
                    "INSERT INTO __schema_history (version, name) VALUES (?1, ?2)",
                    (migration.version, migration.name),
                )
                .unwrap_or_else(|err| panic!("record V{:03}: {err}", migration.version));
            }

            let db = crate::Database::new(conn).expect("create v17 reactive tables");
            let columns = table_columns(db.connection(), "inbox");
            assert!(
                !columns.iter().any(|column| column == "action_json"),
                "v17 inbox vtab should not expose action_json yet"
            );
        }

        let db = crate::open_database(&db_path).expect("upgrade db");
        let columns = table_columns(db.connection(), "inbox");
        assert!(
            columns.iter().any(|column| column == "action_json"),
            "V018 should refresh the inbox vtab schema"
        );

        db.connection()
            .prepare("SELECT action_json FROM inbox")
            .expect("inbox vtab exposes action_json after V018");
    }

    #[test]
    fn v022_adds_rfc_workspace_storage_to_existing_databases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("exo.db");

        {
            let conn = Connection::open(&db_path).expect("open db");
            conn.execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS __schema_history (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );",
            )
            .expect("create history");

            for migration in MIGRATIONS
                .iter()
                .filter(|migration| migration.version <= 20)
            {
                conn.execute_batch(migration.sql)
                    .unwrap_or_else(|err| panic!("apply V{:03}: {err}", migration.version));
                conn.execute(
                    "INSERT INTO __schema_history (version, name) VALUES (?1, ?2)",
                    (migration.version, migration.name),
                )
                .unwrap_or_else(|err| panic!("record V{:03}: {err}", migration.version));
            }
        }

        let db = crate::open_database(&db_path).expect("upgrade db");
        let conn = db.connection();

        for table in [
            "rfc_workspace_snapshots_data",
            "rfc_workspace_observations_data",
            "rfc_workspace_diagnostics_data",
            "rfc_workspace_snapshots",
            "rfc_workspace_observations",
            "rfc_workspace_diagnostics",
            "rfc_canonical_baseline",
            "rfc_canonical_quarantine",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("query sqlite_master");
            assert_eq!(exists, 1, "{table} should exist after V022");
        }

        let migration_applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM __schema_history
                 WHERE version = 22 AND name = 'rfc_workspace_observations'",
                [],
                |row| row.get(0),
            )
            .expect("query migration history");
        assert_eq!(migration_applied, 1, "V022 should be recorded");
    }

    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let sql = format!("PRAGMA table_info({table})");
        conn.prepare(&sql)
            .expect("prepare table_info")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table_info")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect columns")
    }
}
