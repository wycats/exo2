//! Database migrations
//!
//! Simple migration runner that embeds SQL files at compile time.

use rusqlite::Connection;
use std::collections::HashSet;

use crate::compatibility::{
    read_database_generation, set_database_generation, StateSurface, WriterCompatibilityError,
    SUPPORTED_WRITER_GENERATION,
};
use crate::DatabaseError;

/// A migration with version, name, and SQL content.
struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
    required_writer_generation: i32,
}

/// All migrations, embedded at compile time.
/// Add new migrations here in order.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "core_tables",
        sql: include_str!("../migrations/V001__core_tables.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 2,
        name: "shadow_tables",
        sql: include_str!("../migrations/V002__shadow_tables.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 3,
        name: "revision_tables",
        sql: include_str!("../migrations/V003__revision_tables.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 4,
        name: "ideas_table",
        sql: include_str!("../migrations/V004__ideas_table.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 5,
        name: "inbox_table",
        sql: include_str!("../migrations/V005__inbox_table.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 6,
        name: "expand_status_constraints",
        sql: include_str!("../migrations/V006__expand_status_constraints.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 7,
        name: "add_sort_key",
        sql: include_str!("../migrations/V007__add_sort_key.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 8,
        name: "task_logs_and_verifications",
        sql: include_str!("../migrations/V008__task_logs_and_verifications.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 9,
        name: "task_notes_and_started_at",
        sql: include_str!("../migrations/V009__task_notes_and_started_at.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 10,
        name: "epoch_sort_key",
        sql: include_str!("../migrations/V010__epoch_sort_key.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 11,
        name: "persistent_rowset_counters",
        sql: include_str!("../migrations/V011__persistent_rowset_counters.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 12,
        name: "axioms_table",
        sql: include_str!("../migrations/V012__axioms_table.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 13,
        name: "perception_event_schema",
        sql: include_str!("../migrations/V013__perception_event_schema.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 14,
        name: "agent_id",
        sql: include_str!("../migrations/V014__agent_id.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 15,
        name: "rfcs_table",
        sql: include_str!("../migrations/V015__rfcs_table.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 16,
        name: "agent_events",
        sql: include_str!("../migrations/V016__agent_events.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 17,
        name: "workspace_active_phase",
        sql: include_str!("../migrations/V017__workspace_active_phase.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 18,
        name: "inbox_action_payload",
        sql: include_str!("../migrations/V018__inbox_action_payload.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 19,
        name: "phase_ownership",
        sql: include_str!("../migrations/V019__phase_ownership.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 20,
        name: "reactive_revision_coverage",
        sql: include_str!("../migrations/V020__reactive_revision_coverage.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 21,
        name: "atomic_request_outcomes",
        sql: include_str!("../migrations/V021__atomic_request_outcomes.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 22,
        name: "rfc_workspace_observations",
        sql: include_str!("../migrations/V022__rfc_workspace_observations.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 23,
        name: "workbench_lanes",
        sql: include_str!("../migrations/V023__workbench_lanes.sql"),
        required_writer_generation: 0,
    },
    Migration {
        version: 24,
        name: "phase_completion_time",
        sql: include_str!("../migrations/V024__phase_completion_time.sql"),
        required_writer_generation: 0,
    },
];

/// Run all pending migrations on the given connection.
pub fn run_migrations(conn: &Connection) -> Result<(), DatabaseError> {
    run_migrations_with_hook(conn, MIGRATIONS, SUPPORTED_WRITER_GENERATION, |_| Ok(()))
}

fn run_migrations_with_hook(
    conn: &Connection,
    migrations: &[Migration],
    supported_writer_generation: i32,
    after_generation_raise: impl FnOnce(i32) -> Result<(), DatabaseError>,
) -> Result<(), DatabaseError> {
    let applied = applied_versions(conn)?;
    let current_generation = read_database_generation(conn)?;
    let required_generation = migrations
        .iter()
        .map(|migration| migration.required_writer_generation)
        .max()
        .map_or(current_generation, |required_generation| {
            required_generation.max(current_generation)
        });
    if required_generation > supported_writer_generation {
        return Err(WriterCompatibilityError::Incompatible {
            required_generation,
            supported_generation: supported_writer_generation,
            surface: StateSurface::Database,
        }
        .into());
    }
    if required_generation > current_generation {
        // The compatibility floor is committed before any migration effect.
        // A crash can leave state over-fenced, never under-fenced.
        set_database_generation(conn, required_generation)?;
        after_generation_raise(required_generation)?;
    }

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

    // Apply pending migrations in order
    for migration in migrations {
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

pub(crate) fn has_pending_migrations(conn: &Connection) -> Result<bool, DatabaseError> {
    let applied = applied_versions(conn)?;
    Ok(MIGRATIONS
        .iter()
        .any(|migration| !applied.contains(&migration.version)))
}

fn applied_versions(conn: &Connection) -> Result<HashSet<u32>, DatabaseError> {
    let history_exists = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = '__schema_history'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !history_exists {
        return Ok(HashSet::new());
    }
    Ok(conn
        .prepare("SELECT version FROM __schema_history")?
        .query_map([], |row| row.get::<_, i32>(0).map(|version| version as u32))?
        .collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const GENERATION_ONE_MIGRATION: &[Migration] = &[Migration {
        version: 1_001,
        name: "generation_one_fixture",
        sql: "CREATE TABLE generation_one_state(value TEXT NOT NULL);\n\
              INSERT INTO generation_one_state(value) VALUES('migrated');",
        required_writer_generation: 1,
    }];

    #[test]
    fn synthetic_n_rejects_n_plus_one_migration_before_effect() {
        let conn = Connection::open_in_memory().expect("open database");

        let error = run_migrations_with_hook(&conn, GENERATION_ONE_MIGRATION, 0, |_| Ok(()))
            .expect_err("generation-zero writer must reject generation-one migration");

        assert!(matches!(
            error,
            DatabaseError::WriterCompatibility(WriterCompatibilityError::Incompatible {
                required_generation: 1,
                supported_generation: 0,
                surface: StateSurface::Database,
            })
        ));
        assert_eq!(read_database_generation(&conn).unwrap(), 0);
        assert!(!test_table_exists(&conn, "generation_one_state"));
        assert!(!test_table_exists(&conn, "__schema_history"));
    }

    #[test]
    fn synthetic_n_plus_one_migrates_n_state_invisibly() {
        let conn = Connection::open_in_memory().expect("open database");

        run_migrations_with_hook(&conn, GENERATION_ONE_MIGRATION, 1, |_| Ok(()))
            .expect("generation-one writer migrates generation-zero state");

        assert_eq!(read_database_generation(&conn).unwrap(), 1);
        assert_eq!(
            conn.query_row("SELECT value FROM generation_one_state", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "migrated"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM __schema_history WHERE version = 1001",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn interrupted_generation_raise_is_durable_and_resumable() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let path = temp.path().join("exo.db");
        let conn = Connection::open(&path).expect("open database");

        let error = run_migrations_with_hook(&conn, GENERATION_ONE_MIGRATION, 1, |_| {
            Err(DatabaseError::Migration(
                "injected interruption after generation raise".to_string(),
            ))
        })
        .expect_err("interruption must stop before migration effect");
        assert!(matches!(error, DatabaseError::Migration(_)));
        assert_eq!(read_database_generation(&conn).unwrap(), 1);
        assert!(!test_table_exists(&conn, "generation_one_state"));
        drop(conn);

        let reopened = Connection::open(&path).expect("reopen over-fenced database");
        assert_eq!(read_database_generation(&reopened).unwrap(), 1);
        run_migrations_with_hook(&reopened, GENERATION_ONE_MIGRATION, 1, |_| Ok(()))
            .expect("compatible writer resumes interrupted migration");
        assert!(test_table_exists(&reopened, "generation_one_state"));
    }

    #[test]
    fn applied_migration_history_restores_a_missing_generation_fence() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            "CREATE TABLE generation_one_state(value TEXT NOT NULL);
             CREATE TABLE __schema_history (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO __schema_history (version, name)
             VALUES (1001, 'generation_one_fixture');",
        )
        .expect("restore generation-one schema and history without user_version");
        assert_eq!(read_database_generation(&conn).unwrap(), 0);

        run_migrations_with_hook(&conn, GENERATION_ONE_MIGRATION, 1, |_| Ok(()))
            .expect("compatible writer restores the generation fence");

        assert_eq!(read_database_generation(&conn).unwrap(), 1);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM __schema_history WHERE version = 1001",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "the applied migration must not execute or record twice"
        );
    }

    #[test]
    fn v021_applies_when_v022_is_already_recorded() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            "CREATE TABLE __schema_history (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO __schema_history (version, name)
             VALUES (22, 'rfc_workspace_observations');",
        )
        .expect("seed later migration record");

        run_migrations(&conn).expect("apply missing V021 migration");

        let table: String = conn
            .query_row(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name = 'atomic_request_outcomes'",
                [],
                |row| row.get(0),
            )
            .expect("V021 outcome table");
        assert_eq!(table, "atomic_request_outcomes");
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM __schema_history WHERE version = 21",
                [],
                |row| row.get(0),
            )
            .expect("V021 schema history");
        assert_eq!(applied, 1);
    }

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

    #[test]
    fn v023_adds_workbench_lane_storage_to_existing_databases() {
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
                .filter(|migration| migration.version <= 22)
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
            "workbench_lanes_data",
            "workbench_lanes",
            "workbench_lanes_rev",
            "workspace_lane_focus_data",
            "workspace_lane_focus",
            "workspace_lane_focus_rev",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("query sqlite_master");
            assert_eq!(exists, 1, "{table} should exist after V023");
        }

        let migration_applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM __schema_history
                 WHERE version = 23 AND name = 'workbench_lanes'",
                [],
                |row| row.get(0),
            )
            .expect("query migration history");
        assert_eq!(migration_applied, 1, "V023 should be recorded");
    }

    #[test]
    fn v024_adds_and_backfills_phase_completion_time() {
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
                .filter(|migration| migration.version <= 23)
            {
                conn.execute_batch(migration.sql)
                    .unwrap_or_else(|err| panic!("apply V{:03}: {err}", migration.version));
                conn.execute(
                    "INSERT INTO __schema_history (version, name) VALUES (?1, ?2)",
                    (migration.version, migration.name),
                )
                .unwrap_or_else(|err| panic!("record V{:03}: {err}", migration.version));
            }

            conn.execute_batch(
                "INSERT INTO epochs_data(text_id, title) VALUES('epoch', 'Epoch');
                 INSERT INTO phases_data(text_id, title, status, epoch_id)
                    VALUES('phase', 'Completed Phase', 'completed', 1);
                 INSERT INTO goals_data(text_id, label, status, phase_id)
                    VALUES('goal', 'Goal', 'completed', 1);
                 INSERT INTO tasks_data(text_id, title, status, goal_id, completed_at)
                    VALUES('older', 'Older', 'completed', 1, '2026-01-01T10:00:00+00:00');
                 INSERT INTO tasks_data(text_id, title, status, goal_id, completed_at)
                    VALUES('newer', 'Newer', 'completed', 1, '2026-02-01T10:00:00+00:00');",
            )
            .expect("seed completed phase");
        }

        let db = crate::open_database(&db_path).expect("upgrade db");
        let conn = db.connection();
        let columns = table_columns(conn, "phases_data");
        assert!(columns.iter().any(|column| column == "completed_at"));

        let completed_at: String = conn
            .query_row(
                "SELECT completed_at FROM phases_data WHERE text_id = 'phase'",
                [],
                |row| row.get(0),
            )
            .expect("read backfilled completion time");
        assert_eq!(completed_at, "2026-02-01T10:00:00+00:00");

        let migration_applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM __schema_history
                 WHERE version = 24 AND name = 'phase_completion_time'",
                [],
                |row| row.get(0),
            )
            .expect("query migration history");
        assert_eq!(migration_applied, 1, "V024 should be recorded");
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

    fn test_table_exists(conn: &Connection, table: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
             )",
            [table],
            |row| row.get(0),
        )
        .expect("query table existence")
    }
}
