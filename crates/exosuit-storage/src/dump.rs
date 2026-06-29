//! Sorted-SQL serializer and importer for git-friendly persistence.
//!
//! Produces deterministic SQL INSERT statements from the SQLite database,
//! one file per table. Foreign key rowids are resolved to `text_id` values
//! so the output is portable across database instances.
//!
//! See RFC 10178: Git-Friendly Serialization: Sorted SQL Text Dumps.
//!
//! # Axioms
//!
//! 1. **Lossless**: every value in the database must survive a
//!    serialize → deserialize round-trip unchanged. This includes arbitrary
//!    Unicode (multi-byte UTF-8), control characters, backslashes, single
//!    quotes, and the literal text `\n`.
//!
//! 2. **Deterministic**: the same database state always produces the same
//!    text output. Rows are sorted by `text_id` (entity tables) or composite
//!    natural key (junction tables), never by rowid.
//!
//! 3. **Single-line-per-entity**: each INSERT statement occupies exactly one
//!    line, so git diffs show one changed entity per hunk. This conflicts
//!    with arbitrary content (values may contain newlines), which is resolved
//!    by the escaping scheme below.
//!
//! # Escaping Scheme
//!
//! The serializer and importer use a symmetric escaping convention for text
//! values inside SQL string literals:
//!
//! | Original  | Serialized | Rule                                    |
//! |-----------|------------|-----------------------------------------|
//! | `\`      | `\\`     | Escape the escape char (must come first) |
//! | newline   | `\n`      | Preserve single-line invariant           |
//! | CR        | `\r`      | Preserve single-line invariant           |
//! | `'`       | `''`       | Standard SQL quote escaping              |
//!
//! The importer reverses these transformations. Because `\` is escaped
//! before `\n`/`\r`, the scheme is unambiguous: `\\n` in the dump means
//! "literal backslash followed by n", not "escaped newline".

use rusqlite::Connection;
use std::collections::HashMap;
use std::fmt::Write;

/// A single table's SQL dump: (table_name, sql_content).
pub type TableDump = (String, String);

/// All serializable tables in dependency order (parents before children).
///
/// Each entry is `(file_stem, table_name)` — the file stem is the `.sql` filename
/// without extension, and the table name is the SQLite table name.
///
/// Used by both the serializer (to determine dump order) and the importer
/// (to read `.sql` files in the correct dependency order).
pub const TABLE_ORDER: &[(&str, &str)] = &[
    ("epochs", "epochs_data"),
    ("phases", "phases_data"),
    ("goals", "goals_data"),
    ("tasks", "tasks_data"),
    ("ideas", "ideas_data"),
    ("inbox", "inbox_data"),
    ("rfcs", "rfcs_data"),
    ("task_logs", "task_logs"),
    ("task_verifications", "task_verifications"),
    ("axioms", "axioms"),
    ("axiom_implications", "axiom_implications"),
    ("axiom_tags", "axiom_tags"),
    ("phase_rfcs", "phase_rfcs_data"),
    ("entity_aliases", "entity_aliases"),
    ("idea_tags", "idea_tags"),
    ("idea_task_refs", "idea_task_refs"),
    ("rfc_relations", "rfc_relations"),
];

/// Dump all serializable tables from the database as sorted SQL INSERT statements.
///
/// Returns a `Vec<TableDump>` in dependency order (parents before children).
/// Each entry contains the table name and the complete SQL text for that table.
///
/// Foreign key columns are emitted using the referenced entity's `text_id`
/// instead of the internal rowid. The column name is also transformed
/// (e.g., `epoch_id` → `epoch_text_id`).
///
/// The `id` (rowid) column is omitted from all output — it is reassigned on import.
pub fn dump_tables(conn: &Connection) -> Result<Vec<TableDump>, DumpError> {
    let mut results = Vec::new();

    // Build text_id lookup maps for FK resolution.
    // We need these before serializing any table that references them.
    let epoch_ids = build_text_id_map(conn, "epochs_data")?;
    let phase_ids = build_text_id_map(conn, "phases_data")?;
    let goal_ids = build_text_id_map(conn, "goals_data")?;
    let task_ids = build_text_id_map(conn, "tasks_data")?;
    let idea_ids = build_text_id_map(conn, "ideas_data")?;
    let axiom_ids = build_text_id_map(conn, "axioms")?;
    let rfc_ids = build_text_id_map(conn, "rfcs_data")?;

    // Entity type → table name for entity_aliases resolution
    let entity_type_maps: HashMap<&str, &HashMap<i64, String>> = HashMap::from([
        ("epoch", &epoch_ids),
        ("phase", &phase_ids),
        ("goal", &goal_ids),
        ("task", &task_ids),
    ]);

    // === Entity tables (sorted by text_id) ===

    results.push(dump_entity_table(
        conn,
        "epochs_data",
        &["text_id", "title", "slug", "reviewed", "sort_key"],
        &[],
        &HashMap::new(),
    )?);

    results.push(dump_entity_table(
        conn,
        "phases_data",
        &[
            "text_id", "title", "status", "epoch_id", "kind", "slug", "sort_key",
        ],
        &[("epoch_id", "epoch_text_id")],
        &HashMap::from([("epoch_id", &epoch_ids)]),
    )?);

    results.push(dump_entity_table(
        conn,
        "goals_data",
        &[
            "text_id",
            "label",
            "status",
            "phase_id",
            "kind",
            "rfc",
            "target_stage",
            "started_at",
            "description",
            "completion_log",
            "slug",
            "sort_key",
        ],
        &[("phase_id", "phase_text_id")],
        &HashMap::from([("phase_id", &phase_ids)]),
    )?);

    results.push(dump_entity_table(
        conn,
        "tasks_data",
        &[
            "text_id",
            "title",
            "status",
            "goal_id",
            "completed_at",
            "completion_log",
            "slug",
            "sort_key",
            "notes",
            "started_at",
        ],
        &[("goal_id", "goal_text_id")],
        &HashMap::from([("goal_id", &goal_ids)]),
    )?);

    results.push(dump_entity_table(
        conn,
        "ideas_data",
        &[
            "text_id",
            "title",
            "description",
            "status",
            "created_at",
            "source",
        ],
        &[],
        &HashMap::new(),
    )?);

    results.push(dump_entity_table(
        conn,
        "inbox_data",
        &[
            "text_id",
            "created_at",
            "updated_at",
            "status",
            "entity_type",
            "entity_id",
            "source",
            "intent",
            "priority",
            "confidence",
            "agent_id",
            "subject",
            "body",
            "action_json",
            "resolution",
        ],
        &[],
        &HashMap::new(),
    )?);

    results.push(dump_entity_table(
        conn,
        "rfcs_data",
        &[
            "text_id",
            "rfc_number",
            "title",
            "stage",
            "status",
            "feature",
            "slug",
            "file_path",
            "superseded_by",
            "supersedes",
            "withdrawal_reason",
            "archived_reason",
            "consolidated_into",
            "created_at",
            "updated_at",
        ],
        &[],
        &HashMap::new(),
    )?);

    results.push(dump_entity_table(
        conn,
        "axioms",
        &[
            "text_id",
            "scope",
            "principle",
            "rationale",
            "notes",
            "created_at",
        ],
        &[],
        &HashMap::new(),
    )?);

    // === Operational tables (sorted by resolved task_text_id, then created_at) ===

    results.push(dump_operational_table(
        conn,
        "task_logs",
        &["task_id", "kind", "message", "created_at"],
        "task_id",
        "task_text_id",
        &task_ids,
    )?);

    results.push(dump_operational_table(
        conn,
        "task_verifications",
        &[
            "task_id",
            "kind",
            "command",
            "result",
            "details",
            "created_at",
        ],
        "task_id",
        "task_text_id",
        &task_ids,
    )?);

    // === Junction tables (sorted by composite natural key) ===

    results.push(dump_junction_table(
        conn,
        "phase_rfcs_data",
        &["phase_id", "rfc_id", "target", "relation"],
        &[("phase_id", "phase_text_id")],
        &HashMap::from([("phase_id", &phase_ids)]),
        &["phase_text_id", "rfc_id"], // sort key after FK resolution
    )?);

    results.push(dump_entity_aliases(conn, &entity_type_maps)?);

    results.push(dump_junction_table(
        conn,
        "axiom_implications",
        &["axiom_id", "implication", "sort_key"],
        &[("axiom_id", "axiom_text_id")],
        &HashMap::from([("axiom_id", &axiom_ids)]),
        &["axiom_text_id", "sort_key"],
    )?);

    results.push(dump_junction_table(
        conn,
        "axiom_tags",
        &["axiom_id", "tag"],
        &[("axiom_id", "axiom_text_id")],
        &HashMap::from([("axiom_id", &axiom_ids)]),
        &["axiom_text_id", "tag"],
    )?);

    results.push(dump_junction_table(
        conn,
        "idea_tags",
        &["idea_id", "tag"],
        &[("idea_id", "idea_text_id")],
        &HashMap::from([("idea_id", &idea_ids)]),
        &["idea_text_id", "tag"],
    )?);

    results.push(dump_junction_table(
        conn,
        "idea_task_refs",
        &["idea_id", "task_ref"],
        &[("idea_id", "idea_text_id")],
        &HashMap::from([("idea_id", &idea_ids)]),
        &["idea_text_id", "task_ref"],
    )?);

    results.push(dump_junction_table(
        conn,
        "rfc_relations",
        &["rfc_id", "related_rfc", "relation"],
        &[("rfc_id", "rfc_text_id")],
        &HashMap::from([("rfc_id", &rfc_ids)]),
        &["rfc_text_id", "related_rfc", "relation"],
    )?);

    Ok(results)
}

/// Errors that can occur during SQL dump generation.
#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Foreign key resolution failed: {table}.{column} rowid {rowid} not found")]
    FkResolution {
        table: String,
        column: String,
        rowid: i64,
    },

    #[error("Unknown entity type in entity_aliases: {0}")]
    UnknownEntityType(String),
}

// ─── Internal helpers ────────────────────────────────────────────────

/// Build a map of rowid → text_id for a table.
fn build_text_id_map(conn: &Connection, table: &str) -> Result<HashMap<i64, String>, DumpError> {
    let sql = format!("SELECT id, text_id FROM {table}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, text_id) = row?;
        map.insert(id, text_id);
    }
    Ok(map)
}

/// Resolve a rowid FK to a text_id, returning a DumpError on failure.
fn resolve_fk(
    id_map: &HashMap<i64, String>,
    table: &str,
    column: &str,
    rowid: i64,
) -> Result<String, DumpError> {
    id_map
        .get(&rowid)
        .cloned()
        .ok_or_else(|| DumpError::FkResolution {
            table: table.to_string(),
            column: column.to_string(),
            rowid,
        })
}

/// Escape a SQL string value for single-line INSERT statements.
///
/// - Single quotes are doubled: `'` → `''` (standard SQL)
/// - Backslashes are escaped: `\` → `\\` (so our escapes are unambiguous)
/// - Newlines are escaped: `\n` → `\n`, `\r` → `\r` (keeps output single-line)
fn sql_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

/// Format a `rusqlite::types::Value` as a SQL literal.
fn value_to_sql(val: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match val {
        Value::Null => "NULL".to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => format!("'{}'", sql_escape(s)),
        Value::Blob(b) => format!("X'{}'", hex_encode(b)),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{:02X}", b).unwrap();
    }
    s
}

/// Read all rows from a table, returning each row as a Vec of (column_name, Value).
/// Skips the `id` column (rowid).
fn read_rows(
    conn: &Connection,
    table: &str,
    columns: &[&str],
) -> Result<Vec<Vec<(String, rusqlite::types::Value)>>, DumpError> {
    let col_list = columns.join(", ");
    let sql = format!("SELECT {col_list} FROM {table}");
    let mut stmt = conn.prepare(&sql)?;

    let col_count = columns.len();
    let rows = stmt.query_map([], |row| {
        let mut values = Vec::with_capacity(col_count);
        for (i, col) in columns.iter().enumerate().take(col_count) {
            let val: rusqlite::types::Value = row.get(i)?;
            values.push((col.to_string(), val));
        }
        Ok(values)
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Transform a row's FK columns: replace rowid values with text_id values,
/// and rename the column (e.g., `epoch_id` → `epoch_text_id`).
fn resolve_row_fks(
    row: &mut [(String, rusqlite::types::Value)],
    fk_renames: &[(&str, &str)],
    fk_maps: &HashMap<&str, &HashMap<i64, String>>,
    table: &str,
) -> Result<(), DumpError> {
    for (old_col, new_col) in fk_renames {
        for entry in row.iter_mut() {
            if entry.0 == *old_col {
                if let rusqlite::types::Value::Integer(rowid) = &entry.1 {
                    let id_map = fk_maps.get(old_col).expect("FK map missing");
                    let text_id = resolve_fk(id_map, table, old_col, *rowid)?;
                    entry.0 = new_col.to_string();
                    entry.1 = rusqlite::types::Value::Text(text_id);
                }
                // If NULL, just rename the column (nullable FK)
                else if matches!(entry.1, rusqlite::types::Value::Null) {
                    entry.0 = new_col.to_string();
                }
                break;
            }
        }
    }
    Ok(())
}

/// Format a row as an INSERT statement.
fn row_to_insert(table: &str, row: &[(String, rusqlite::types::Value)]) -> String {
    let col_names: Vec<&str> = row.iter().map(|(name, _)| name.as_str()).collect();
    let values: Vec<String> = row.iter().map(|(_, val)| value_to_sql(val)).collect();
    format!(
        "INSERT INTO {}({}) VALUES({});",
        table,
        col_names.join(", "),
        values.join(", ")
    )
}

/// Get the string value of a named column from a row, for sorting.
fn sort_key_from_row(row: &[(String, rusqlite::types::Value)], col: &str) -> String {
    for (name, val) in row {
        if name == col {
            return match val {
                rusqlite::types::Value::Text(s) => s.clone(),
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Null => String::new(),
                other => value_to_sql(other),
            };
        }
    }
    String::new()
}

/// Dump an entity table (has text_id, sorted by text_id).
fn dump_entity_table(
    conn: &Connection,
    table: &str,
    columns: &[&str],
    fk_renames: &[(&str, &str)],
    fk_maps: &HashMap<&str, &HashMap<i64, String>>,
) -> Result<TableDump, DumpError> {
    let mut rows = read_rows(conn, table, columns)?;

    for row in &mut rows {
        resolve_row_fks(row, fk_renames, fk_maps, table)?;
    }

    // Sort by text_id (first column for entity tables)
    rows.sort_by_key(|a| sort_key_from_row(a, "text_id"));

    let mut output = String::new();
    for row in &rows {
        writeln!(output, "{}", row_to_insert(table, row)).unwrap();
    }

    Ok((table.to_string(), output))
}

/// Dump an operational table (task_logs, task_verifications).
/// These have a task_id FK but no text_id of their own.
/// Sorted by (resolved task_text_id, created_at).
fn dump_operational_table(
    conn: &Connection,
    table: &str,
    columns: &[&str],
    fk_col: &str,
    fk_new_col: &str,
    task_id_map: &HashMap<i64, String>,
) -> Result<TableDump, DumpError> {
    let mut rows = read_rows(conn, table, columns)?;

    let fk_renames: &[(&str, &str)] = &[(fk_col, fk_new_col)];
    let fk_maps: HashMap<&str, &HashMap<i64, String>> = HashMap::from([(fk_col, task_id_map)]);

    for row in &mut rows {
        resolve_row_fks(row, fk_renames, &fk_maps, table)?;
    }

    // Sort by (task_text_id, created_at)
    rows.sort_by(|a, b| {
        let a_task = sort_key_from_row(a, fk_new_col);
        let b_task = sort_key_from_row(b, fk_new_col);
        a_task.cmp(&b_task).then_with(|| {
            sort_key_from_row(a, "created_at").cmp(&sort_key_from_row(b, "created_at"))
        })
    });

    let mut output = String::new();
    for row in &rows {
        writeln!(output, "{}", row_to_insert(table, row)).unwrap();
    }

    Ok((table.to_string(), output))
}

/// Dump a junction table with FK resolution and composite sort key.
fn dump_junction_table(
    conn: &Connection,
    table: &str,
    columns: &[&str],
    fk_renames: &[(&str, &str)],
    fk_maps: &HashMap<&str, &HashMap<i64, String>>,
    sort_cols: &[&str],
) -> Result<TableDump, DumpError> {
    let mut rows = read_rows(conn, table, columns)?;

    for row in &mut rows {
        resolve_row_fks(row, fk_renames, fk_maps, table)?;
    }

    // Sort by composite key
    rows.sort_by(|a, b| {
        for col in sort_cols {
            let ord = sort_key_from_row(a, col).cmp(&sort_key_from_row(b, col));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });

    let mut output = String::new();
    for row in &rows {
        writeln!(output, "{}", row_to_insert(table, row)).unwrap();
    }

    Ok((table.to_string(), output))
}

/// Dump entity_aliases with dynamic FK resolution based on entity_type.
fn dump_entity_aliases(
    conn: &Connection,
    entity_type_maps: &HashMap<&str, &HashMap<i64, String>>,
) -> Result<TableDump, DumpError> {
    let table = "entity_aliases";
    let columns = &["entity_type", "entity_id", "alias"];
    let rows = read_rows(conn, table, columns)?;

    // Resolve entity_id → entity_text_id based on entity_type.
    //
    // Orphaned aliases (whose target entity was deleted without removing the
    // alias row) cannot be resolved to a text_id. Such a row is not portable
    // and must not abort the entire dump — that would silently freeze sidecar
    // state for every subsequent write. We skip the orphan and warn instead,
    // so durable state keeps flowing while the stale row is surfaced.
    let mut resolved_rows = Vec::with_capacity(rows.len());
    for mut row in rows {
        let alias = match &row[2].1 {
            rusqlite::types::Value::Text(s) => s.as_str(),
            _ => "<unknown>",
        };

        // Every emitted entity_aliases row must carry a resolved
        // `entity_text_id` column — the importer requires it. A row we cannot
        // fully resolve (malformed type or orphaned FK) must therefore be
        // skipped entirely; passing it through unchanged would emit an
        // `entity_id` column and make the whole dump non-importable.
        let rusqlite::types::Value::Text(entity_type) = &row[0].1 else {
            eprintln!(
                "warning: skipping malformed entity_aliases row \
                 (alias={alias}): entity_type is not text"
            );
            continue;
        };
        let entity_type = entity_type.clone();
        let rusqlite::types::Value::Integer(rowid) = &row[1].1 else {
            eprintln!(
                "warning: skipping malformed entity_aliases row \
                 (entity_type={entity_type}, alias={alias}): entity_id is not an integer"
            );
            continue;
        };
        let rowid = *rowid;

        // An unknown entity_type is a schema/version mismatch, not data
        // corruption: this binary is older than the database, or a new entity
        // type started emitting aliases. Skipping here would silently drop a
        // whole class of alias data and propagate an incomplete projection to
        // git and other machines — strictly worse than failing. Abort so the
        // mismatch is surfaced and the prior good projection is preserved.
        let id_map = entity_type_maps
            .get(entity_type.as_str())
            .ok_or_else(|| DumpError::UnknownEntityType(entity_type.clone()))?;

        match resolve_fk(id_map, table, "entity_id", rowid) {
            Ok(text_id) => {
                row[1].0 = "entity_text_id".to_string();
                row[1].1 = rusqlite::types::Value::Text(text_id);
                resolved_rows.push(row);
            }
            Err(_) => {
                eprintln!(
                    "warning: skipping orphaned entity_aliases row \
                     (entity_type={entity_type}, entity_id={rowid}, alias={alias}): \
                     target entity no longer exists"
                );
            }
        }
    }
    let mut rows = resolved_rows;

    // Sort by (entity_type, alias)
    rows.sort_by(|a, b| {
        sort_key_from_row(a, "entity_type")
            .cmp(&sort_key_from_row(b, "entity_type"))
            .then_with(|| sort_key_from_row(a, "alias").cmp(&sort_key_from_row(b, "alias")))
    });

    let mut output = String::new();
    for row in &rows {
        writeln!(output, "{}", row_to_insert(table, row)).unwrap();
    }

    Ok((table.to_string(), output))
}

// ─── SQL Importer ────────────────────────────────────────────────────

/// Import table dumps into a database, resolving text_id FKs back to rowids.
///
/// Takes the output of `dump_tables()` and populates the database.
/// Tables must be provided in dependency order (parents before children),
/// which is the order `dump_tables()` produces.
///
/// Defensive mode is temporarily disabled during import to allow direct
/// writes to shadow tables, then re-enabled afterward.
pub fn import_tables(conn: &Connection, dumps: &[TableDump]) -> Result<(), ImportError> {
    // Disable defensive mode to write to shadow tables directly
    conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
        .map_err(ImportError::Sqlite)?;

    // Build text_id → rowid maps as we import parent tables
    let mut text_id_maps: HashMap<String, HashMap<String, i64>> = HashMap::new();

    let result = (|| {
        for (table, sql) in dumps {
            if sql.trim().is_empty() {
                continue;
            }
            import_single_table(conn, table, sql, &mut text_id_maps)?;
        }
        Ok(())
    })();

    // Re-enable defensive mode regardless of success/failure
    conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(ImportError::Sqlite)?;

    result
}

/// Errors that can occur during SQL import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Parse error in {table} line {line}: {message}")]
    Parse {
        table: String,
        line: usize,
        message: String,
    },

    #[error("FK resolution failed during import: {table}.{column} text_id '{text_id}' not found")]
    FkResolution {
        table: String,
        column: String,
        text_id: String,
    },
}

/// FK column mappings: (dump_column_name, real_column_name, lookup_table)
/// The dump uses `epoch_text_id` but the real column is `epoch_id` referencing `epochs_data`.
const ENTITY_FK_MAPPINGS: &[(&str, &str, &str)] = &[
    ("epoch_text_id", "epoch_id", "epochs_data"),
    ("phase_text_id", "phase_id", "phases_data"),
    ("goal_text_id", "goal_id", "goals_data"),
    ("task_text_id", "task_id", "tasks_data"),
    ("idea_text_id", "idea_id", "ideas_data"),
    ("axiom_text_id", "axiom_id", "axioms"),
];

/// Import a single table's SQL dump.
fn import_single_table(
    conn: &Connection,
    table: &str,
    sql: &str,
    text_id_maps: &mut HashMap<String, HashMap<String, i64>>,
) -> Result<(), ImportError> {
    for (line_num, line) in sql.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("--") {
            continue;
        }

        let parsed = parse_insert(line, table, line_num + 1)?;

        if table == "entity_aliases" {
            import_entity_alias_row(conn, &parsed, text_id_maps)?;
        } else {
            import_regular_row(conn, table, &parsed, text_id_maps)?;
        }
    }

    // After importing an entity table, build its text_id → rowid map
    // for use by child tables.
    if has_text_id_column(table) {
        let map = build_reverse_text_id_map(conn, table)?;
        text_id_maps.insert(table.to_string(), map);
    }

    Ok(())
}

/// Tables that have a text_id column (entity tables).
fn has_text_id_column(table: &str) -> bool {
    matches!(
        table,
        "epochs_data"
            | "phases_data"
            | "goals_data"
            | "tasks_data"
            | "ideas_data"
            | "inbox_data"
            | "rfcs_data"
            | "axioms"
    )
}

/// Build a text_id → rowid map for a table (inverse of build_text_id_map).
fn build_reverse_text_id_map(
    conn: &Connection,
    table: &str,
) -> Result<HashMap<String, i64>, ImportError> {
    let sql = format!("SELECT text_id, id FROM {table}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (text_id, id) = row?;
        map.insert(text_id, id);
    }
    Ok(map)
}

/// A parsed INSERT statement: column names and their values.
struct ParsedInsert {
    columns: Vec<String>,
    values: Vec<SqlValue>,
}

/// A parsed SQL value.
#[derive(Debug, Clone)]
enum SqlValue {
    Null,
    Integer(i64),
    Text(String),
}

impl SqlValue {
    fn as_text(&self) -> Option<&str> {
        match self {
            SqlValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Parse an INSERT statement into column names and values.
///
/// Expected format: `INSERT INTO table_name(col1, col2, ...) VALUES(val1, val2, ...);`
fn parse_insert(line: &str, table: &str, line_num: usize) -> Result<ParsedInsert, ImportError> {
    let err = |msg: &str| ImportError::Parse {
        table: table.to_string(),
        line: line_num,
        message: msg.to_string(),
    };

    // Find the column list between first ( and )
    let col_start = line
        .find('(')
        .ok_or_else(|| err("no opening paren for columns"))?
        + 1;
    let col_end = line[col_start..]
        .find(')')
        .ok_or_else(|| err("no closing paren for columns"))?
        + col_start;
    let col_str = &line[col_start..col_end];
    let columns: Vec<String> = col_str.split(',').map(|s| s.trim().to_string()).collect();

    // Find VALUES(...) — look for "VALUES(" after the column list
    let after_cols = &line[col_end + 1..];
    let values_prefix = "VALUES(";
    let val_offset = after_cols
        .find(values_prefix)
        .ok_or_else(|| err("no VALUES( found"))?;
    let val_start = col_end + 1 + val_offset + values_prefix.len();

    // Find the matching closing paren, accounting for quoted strings
    let val_str = &line[val_start..];
    let val_end = find_closing_paren(val_str).ok_or_else(|| err("no closing paren for values"))?;
    let val_content = &val_str[..val_end];

    let values = parse_values(val_content, table, line_num)?;

    if columns.len() != values.len() {
        return Err(err(&format!(
            "column count ({}) != value count ({})",
            columns.len(),
            values.len()
        )));
    }

    Ok(ParsedInsert { columns, values })
}

/// Find the closing paren that matches the implicit opening paren,
/// respecting single-quoted strings (with '' escapes).
fn find_closing_paren(s: &str) -> Option<usize> {
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b')' => return Some(i),
            b'\'' => {
                // Skip quoted string
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        // Check for escaped quote ''
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                // i now points at the closing quote
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse a comma-separated list of SQL values.
fn parse_values(s: &str, table: &str, line_num: usize) -> Result<Vec<SqlValue>, ImportError> {
    let err = |msg: String| ImportError::Parse {
        table: table.to_string(),
        line: line_num,
        message: msg,
    };

    let mut values = Vec::new();
    let mut i = 0;
    let bytes = s.as_bytes();

    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        if bytes[i] == b'\'' {
            // Quoted string — unescape '' → ', \\→\, \n→newline, \r→CR
            i += 1;
            let mut text = String::new();
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                        text.push('\'');
                        i += 2;
                    } else {
                        break;
                    }
                } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'n' => {
                            text.push('\n');
                            i += 2;
                        }
                        b'r' => {
                            text.push('\r');
                            i += 2;
                        }
                        b'\\' => {
                            text.push('\\');
                            i += 2;
                        }
                        _ => {
                            text.push('\\');
                            i += 1;
                        }
                    }
                } else {
                    // Handle UTF-8: decode the next character from the byte slice.
                    // Our special chars (' and \) are ASCII, so any byte >= 0x80
                    // is part of a multi-byte UTF-8 sequence.
                    let rest = &s[i..];
                    let ch = rest.chars().next().unwrap();
                    text.push(ch);
                    i += ch.len_utf8();
                }
            }
            if i >= bytes.len() {
                return Err(err("unterminated string literal".to_string()));
            }
            i += 1; // skip closing quote
            values.push(SqlValue::Text(text));
        } else if s[i..].starts_with("NULL") {
            values.push(SqlValue::Null);
            i += 4;
        } else if bytes[i] == b'-' || bytes[i].is_ascii_digit() {
            // Integer
            let start = i;
            if bytes[i] == b'-' {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num_str = &s[start..i];
            let n: i64 = num_str
                .parse()
                .map_err(|_| err(format!("invalid integer: {num_str}")))?;
            values.push(SqlValue::Integer(n));
        } else {
            return Err(err(format!(
                "unexpected character at position {i}: '{}'",
                bytes[i] as char
            )));
        }

        // Skip whitespace and comma
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b',') {
            i += 1;
        }
    }

    Ok(values)
}

/// Import a regular row (non-entity_aliases), resolving FK text_ids to rowids.
fn import_regular_row(
    conn: &Connection,
    table: &str,
    parsed: &ParsedInsert,
    text_id_maps: &HashMap<String, HashMap<String, i64>>,
) -> Result<(), ImportError> {
    let mut real_columns = Vec::with_capacity(parsed.columns.len());
    let mut real_values = Vec::with_capacity(parsed.values.len());

    for (col, val) in parsed.columns.iter().zip(parsed.values.iter()) {
        // Check if this column is an FK that needs resolution
        if let Some((_, real_col, lookup_table)) = ENTITY_FK_MAPPINGS
            .iter()
            .find(|(dump_col, _, _)| dump_col == col)
        {
            real_columns.push(real_col.to_string());
            match val {
                SqlValue::Text(text_id) => {
                    let map = text_id_maps.get(*lookup_table).ok_or_else(|| {
                        ImportError::FkResolution {
                            table: table.to_string(),
                            column: col.to_string(),
                            text_id: text_id.clone(),
                        }
                    })?;
                    let rowid = map.get(text_id).ok_or_else(|| ImportError::FkResolution {
                        table: table.to_string(),
                        column: col.to_string(),
                        text_id: text_id.clone(),
                    })?;
                    real_values.push(SqlValue::Integer(*rowid));
                }
                SqlValue::Null => {
                    real_values.push(SqlValue::Null);
                }
                SqlValue::Integer(_) => {
                    // Shouldn't happen in dump output, but pass through
                    real_values.push(val.clone());
                }
            }
        } else {
            real_columns.push(col.clone());
            real_values.push(val.clone());
        }
    }

    execute_insert(conn, table, &real_columns, &real_values)
}

/// Import an entity_aliases row with dynamic FK resolution.
fn import_entity_alias_row(
    conn: &Connection,
    parsed: &ParsedInsert,
    text_id_maps: &HashMap<String, HashMap<String, i64>>,
) -> Result<(), ImportError> {
    // Find entity_type and entity_text_id columns
    let entity_type_idx = parsed
        .columns
        .iter()
        .position(|c| c == "entity_type")
        .ok_or_else(|| ImportError::Parse {
            table: "entity_aliases".to_string(),
            line: 0,
            message: "missing entity_type column".to_string(),
        })?;
    let entity_text_id_idx = parsed
        .columns
        .iter()
        .position(|c| c == "entity_text_id")
        .ok_or_else(|| ImportError::Parse {
            table: "entity_aliases".to_string(),
            line: 0,
            message: "missing entity_text_id column".to_string(),
        })?;

    let entity_type =
        parsed.values[entity_type_idx]
            .as_text()
            .ok_or_else(|| ImportError::Parse {
                table: "entity_aliases".to_string(),
                line: 0,
                message: "entity_type is not a string".to_string(),
            })?;

    let entity_text_id =
        parsed.values[entity_text_id_idx]
            .as_text()
            .ok_or_else(|| ImportError::Parse {
                table: "entity_aliases".to_string(),
                line: 0,
                message: "entity_text_id is not a string".to_string(),
            })?;

    // Map entity_type to table name
    let lookup_table = match entity_type {
        "epoch" => "epochs_data",
        "phase" => "phases_data",
        "goal" => "goals_data",
        "task" => "tasks_data",
        other => {
            return Err(ImportError::Parse {
                table: "entity_aliases".to_string(),
                line: 0,
                message: format!("unknown entity_type: {other}"),
            });
        }
    };

    let map = text_id_maps
        .get(lookup_table)
        .ok_or_else(|| ImportError::FkResolution {
            table: "entity_aliases".to_string(),
            column: "entity_text_id".to_string(),
            text_id: entity_text_id.to_string(),
        })?;

    let rowid = map
        .get(entity_text_id)
        .ok_or_else(|| ImportError::FkResolution {
            table: "entity_aliases".to_string(),
            column: "entity_text_id".to_string(),
            text_id: entity_text_id.to_string(),
        })?;

    // Build the real columns/values with entity_id instead of entity_text_id
    let mut real_columns = Vec::new();
    let mut real_values = Vec::new();
    for (col, val) in parsed.columns.iter().zip(parsed.values.iter()) {
        if col == "entity_text_id" {
            real_columns.push("entity_id".to_string());
            real_values.push(SqlValue::Integer(*rowid));
        } else {
            real_columns.push(col.clone());
            real_values.push(val.clone());
        }
    }

    execute_insert(conn, "entity_aliases", &real_columns, &real_values)
}

/// Execute an INSERT statement with the given columns and values.
fn execute_insert(
    conn: &Connection,
    table: &str,
    columns: &[String],
    values: &[SqlValue],
) -> Result<(), ImportError> {
    let col_list = columns.join(", ");
    let placeholders: Vec<String> = (1..=values.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "INSERT INTO {table}({col_list}) VALUES({})",
        placeholders.join(", ")
    );

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .iter()
        .map(|v| -> Box<dyn rusqlite::types::ToSql> {
            match v {
                SqlValue::Null => Box::new(rusqlite::types::Null),
                SqlValue::Integer(i) => Box::new(*i),
                SqlValue::Text(s) => Box::new(s.clone()),
            }
        })
        .collect();

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, param_refs.as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_memory_database;

    /// Insert a complete fixture hierarchy into the database for testing.
    /// Uses the shadow tables directly (defensive mode is on, but we insert
    /// before vtab creation in tests, or we use a raw connection).
    fn insert_fixture(conn: &Connection) {
        // We need to temporarily disable defensive mode to write to shadow tables
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();

        // Epochs
        conn.execute_batch(
            "INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
                 VALUES('01EPOCH_AAA', 'First Epoch', 'first-epoch', 0, '00000001');
             INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
                 VALUES('01EPOCH_BBB', 'Second Epoch', 'second-epoch', 1, '00000002');",
        )
        .unwrap();

        // Phases
        conn.execute_batch(
            "INSERT INTO phases_data(text_id, title, status, epoch_id, kind, slug, sort_key)
                 VALUES('01PHASE_AAA', 'Phase Alpha', 'completed', 1, 'regular', 'phase-alpha', 'a');
             INSERT INTO phases_data(text_id, title, status, epoch_id, kind, slug, sort_key)
                 VALUES('01PHASE_BBB', 'Phase Beta', 'in-progress', 2, 'chore', 'phase-beta', 'b');",
        )
        .unwrap();

        // Goals
        conn.execute_batch(
            "INSERT INTO goals_data(text_id, label, status, phase_id, kind, rfc, target_stage, started_at, description, completion_log, slug, sort_key)
                 VALUES('01GOAL_AAAA', 'Goal One', 'completed', 1, 'regular', '10178', 3, '2026-01-15T10:00:00Z', 'First goal', 'Done!', 'goal-one', 'a');
             INSERT INTO goals_data(text_id, label, status, phase_id, kind, rfc, target_stage, started_at, description, completion_log, slug, sort_key)
                 VALUES('01GOAL_BBBB', 'Goal Two', 'in-progress', 2, 'strike', NULL, NULL, NULL, NULL, NULL, 'goal-two', 'b');",
        )
        .unwrap();

        // Tasks
        conn.execute_batch(
            "INSERT INTO tasks_data(text_id, title, status, goal_id, completed_at, completion_log, slug, sort_key, notes, started_at)
                 VALUES('01TASK_AAAA', 'Task Alpha', 'completed', 1, '2026-01-16T12:00:00Z', 'Finished', 'task-alpha', 'a', 'Some notes', '2026-01-15T10:00:00Z');
             INSERT INTO tasks_data(text_id, title, status, goal_id, completed_at, completion_log, slug, sort_key, notes, started_at)
                 VALUES('01TASK_BBBB', 'Task Beta', 'pending', 2, NULL, NULL, 'task-beta', 'b', NULL, NULL);",
        )
        .unwrap();

        // Ideas
        conn.execute_batch(
            "INSERT INTO ideas_data(text_id, title, description, status, created_at, source)
                 VALUES('01IDEA_AAAA', 'Great Idea', 'An idea description', 'new', '2026-01-10T08:00:00Z', 'user');
             INSERT INTO ideas_data(text_id, title, description, status, created_at, source)
                 VALUES('01IDEA_BBBB', 'Another Idea', NULL, 'archived', '2026-01-11T09:00:00Z', 'agent');",
        )
        .unwrap();

        // Inbox
        conn.execute_batch(
            "INSERT INTO inbox_data(text_id, created_at, updated_at, status, entity_type, entity_id, source, intent, priority, confidence, agent_id, subject, body, resolution)
                 VALUES('01INBOX_AAA', '2026-02-01T10:00:00Z', NULL, 'pending', 'rfc', '10178', 'user-feedback', 'fyi', 'next-touch', NULL, NULL, 'Review this', 'Please review the RFC', NULL);",
        )
        .unwrap();

        // Phase RFCs
        conn.execute_batch(
            "INSERT INTO phase_rfcs_data(phase_id, rfc_id, target, relation)
                 VALUES(1, '10178', 3, 'driving');
             INSERT INTO phase_rfcs_data(phase_id, rfc_id, target, relation)
                 VALUES(2, '10165', NULL, 'related');",
        )
        .unwrap();

        // Entity aliases
        conn.execute_batch(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 VALUES('epoch', 1, 'e1');
             INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 VALUES('task', 2, 'tb');",
        )
        .unwrap();

        // Idea tags
        conn.execute_batch(
            "INSERT INTO idea_tags(idea_id, tag) VALUES(1, 'storage');
             INSERT INTO idea_tags(idea_id, tag) VALUES(1, 'sqlite');
             INSERT INTO idea_tags(idea_id, tag) VALUES(2, 'ux');",
        )
        .unwrap();

        // Idea task refs
        conn.execute_batch(
            "INSERT INTO idea_task_refs(idea_id, task_ref) VALUES(1, '01TASK_AAAA');",
        )
        .unwrap();

        // Task logs
        conn.execute_batch(
            "INSERT INTO task_logs(task_id, kind, message, created_at)
                 VALUES(1, 'note', 'Started working on this', '2026-01-15T10:30:00Z');
             INSERT INTO task_logs(task_id, kind, message, created_at)
                 VALUES(1, 'progress', 'Halfway done', '2026-01-15T14:00:00Z');",
        )
        .unwrap();

        // Task verifications
        conn.execute_batch(
            "INSERT INTO task_verifications(task_id, kind, command, result, details, created_at)
                 VALUES(1, 'test', 'cargo test', 'pass', '42 tests passed', '2026-01-16T11:00:00Z');",
        )
        .unwrap();

        // Agent events
        conn.execute_batch(
            "INSERT INTO agent_events(
                 text_id, timestamp, agent_id, event_type, namespace, operation,
                 entity_type, entity_id, effect, duration_ms, summary
             ) VALUES(
                 '01EVENT_AAA', '2026-01-17T10:00:00Z', 'agent-1', 'command',
                 'task', 'list', 'task', '01TASK_AAAA', 'read', 15, 'Listed tasks'
             );",
        )
        .unwrap();

        // Re-enable defensive mode
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();
    }

    #[test]
    fn test_dump_produces_deterministic_output() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");

        assert_eq!(dumps.len(), 17, "expected 17 table dumps");

        // Verify table order (dependency order)
        let table_names: Vec<&str> = dumps.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            table_names,
            vec![
                "epochs_data",
                "phases_data",
                "goals_data",
                "tasks_data",
                "ideas_data",
                "inbox_data",
                "rfcs_data",
                "axioms",
                "task_logs",
                "task_verifications",
                "phase_rfcs_data",
                "entity_aliases",
                "axiom_implications",
                "axiom_tags",
                "idea_tags",
                "idea_task_refs",
                "rfc_relations",
            ]
        );

        // Verify determinism: dump twice, get identical output
        let dumps2 = dump_tables(conn).expect("second dump should succeed");
        for ((name1, sql1), (name2, sql2)) in dumps.iter().zip(dumps2.iter()) {
            assert_eq!(name1, name2);
            assert_eq!(sql1, sql2, "non-deterministic output for table {name1}");
        }
    }

    #[test]
    fn test_epochs_dump_format() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, epochs_sql) = &dumps[0];

        // Should be sorted by text_id
        let lines: Vec<&str> = epochs_sql.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("'01EPOCH_AAA'"));
        assert!(lines[1].contains("'01EPOCH_BBB'"));

        // Should NOT contain 'id' column
        assert!(!lines[0].contains("INSERT INTO epochs_data(id,"));

        // Should contain all expected columns
        assert!(lines[0].contains("text_id"));
        assert!(lines[0].contains("title"));
        assert!(lines[0].contains("sort_key"));
    }

    #[test]
    fn test_fk_resolution_in_phases() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, phases_sql) = &dumps[1];

        // Should use epoch_text_id, not epoch_id
        assert!(phases_sql.contains("epoch_text_id"));
        assert!(!phases_sql.contains("epoch_id"));

        // Should resolve to the epoch's text_id
        assert!(phases_sql.contains("'01EPOCH_AAA'") || phases_sql.contains("01EPOCH_"));
    }

    #[test]
    fn test_fk_resolution_in_goals() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, goals_sql) = &dumps[2];

        assert!(goals_sql.contains("phase_text_id"));
        assert!(!goals_sql.contains(", phase_id,"));
    }

    #[test]
    fn test_fk_resolution_in_tasks() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, tasks_sql) = &dumps[3];

        assert!(tasks_sql.contains("goal_text_id"));
        assert!(!tasks_sql.contains(", goal_id,"));
    }

    #[test]
    fn agent_events_is_not_dumped() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");

        // The fixture inserts an agent_events row; it's local telemetry and
        // must not appear in the git-friendly dump.
        assert!(
            dumps.iter().all(|(name, _)| name != "agent_events"),
            "agent_events should not be dumped"
        );
        assert!(
            !TABLE_ORDER
                .iter()
                .any(|(_, table)| *table == "agent_events"),
            "agent_events should not be in TABLE_ORDER"
        );
    }

    #[test]
    fn test_null_values_preserved() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");

        // Goal Two has NULL rfc, target_stage, started_at, description, completion_log
        let (_, goals_sql) = &dumps[2];
        let goal_two_line = goals_sql
            .lines()
            .find(|l| l.contains("01GOAL_BBBB"))
            .unwrap();
        assert!(
            goal_two_line.contains("NULL"),
            "NULL values should be preserved"
        );
    }

    #[test]
    fn test_entity_aliases_dynamic_fk() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, aliases_sql) = dumps
            .iter()
            .find(|(name, _)| name == "entity_aliases")
            .unwrap();

        // Should use entity_text_id, not entity_id
        assert!(aliases_sql.contains("entity_text_id"));
        assert!(!aliases_sql.contains(", entity_id,"));

        // epoch alias should resolve to epoch text_id
        assert!(aliases_sql.contains("'01EPOCH_AAA'"));
        // task alias should resolve to task text_id
        assert!(aliases_sql.contains("'01TASK_BBBB'"));
    }

    #[test]
    fn test_orphaned_entity_alias_is_skipped_not_fatal() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        // Insert an orphaned alias: a goal alias whose entity_id points at a
        // rowid that does not exist in goals_data (simulating a goal that was
        // deleted without removing its alias).
        conn.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 VALUES('goal', 99999, 'orphan')",
            [],
        )
        .unwrap();

        // The dump must still succeed — one orphaned alias should not abort the
        // entire projection (which would silently freeze sidecar persistence).
        let dumps = dump_tables(conn).expect("dump should succeed despite orphan");

        let (_, aliases_sql) = dumps
            .iter()
            .find(|(name, _)| name == "entity_aliases")
            .unwrap();

        // The orphaned alias is skipped.
        assert!(
            !aliases_sql.contains("'orphan'"),
            "orphaned alias should be skipped from the dump"
        );
        // Valid aliases are still present.
        assert!(aliases_sql.contains("'01EPOCH_AAA'"));
        assert!(aliases_sql.contains("'01TASK_BBBB'"));

        // Every emitted row must carry the resolved entity_text_id column, never
        // a raw entity_id — otherwise the dump would not be importable.
        assert!(aliases_sql.contains("entity_text_id"));
        assert!(!aliases_sql.contains(", entity_id,"));

        // And the resulting dump must round-trip through import cleanly: a
        // skipped orphan must not leave the projection non-importable.
        let fresh = open_memory_database().expect("fresh db");
        import_tables(fresh.connection(), &dumps).expect("dump must be importable despite orphan");
    }

    #[test]
    fn test_unknown_entity_type_is_a_hard_error() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        // An alias for an entity_type this binary does not know about is a
        // schema/version mismatch, not data corruption. Unlike an orphaned or
        // malformed row, silently skipping it would write an *incomplete*
        // projection that drops a whole entity type's aliases — and then push
        // that loss to other machines. The dump must instead fail loudly so the
        // version mismatch surfaces and the prior good projection is preserved.
        //
        // The CHECK constraint normally restricts entity_type to the known set,
        // so we disable check constraints to simulate a future schema that has
        // added a new aliasable entity type this older binary doesn't map.
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        conn.execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
                 VALUES('rfc', 1, 'future-type')",
            [],
        )
        .unwrap();
        conn.execute_batch("PRAGMA ignore_check_constraints = OFF;")
            .unwrap();

        let err = dump_tables(conn).expect_err("unknown entity_type must abort the dump");
        assert!(
            matches!(err, DumpError::UnknownEntityType(ref t) if t == "rfc"),
            "expected UnknownEntityType(\"rfc\"), got {err:?}"
        );
    }

    #[test]
    fn test_junction_tables_sorted_by_composite_key() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");

        // idea_tags: sorted by (idea_text_id, tag)
        let (_, tags_sql) = dumps.iter().find(|(name, _)| name == "idea_tags").unwrap();
        let lines: Vec<&str> = tags_sql.lines().collect();
        assert_eq!(lines.len(), 3);
        // All idea_id=1 tags (01IDEA_AAAA) should come before idea_id=2 (01IDEA_BBBB)
        // Within same idea, sorted by tag: 'sqlite' < 'storage'
        assert!(lines[0].contains("'01IDEA_AAAA'") && lines[0].contains("'sqlite'"));
        assert!(lines[1].contains("'01IDEA_AAAA'") && lines[1].contains("'storage'"));
        assert!(lines[2].contains("'01IDEA_BBBB'") && lines[2].contains("'ux'"));
    }

    #[test]
    fn test_task_logs_sorted_by_task_and_time() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();
        insert_fixture(conn);

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, logs_sql) = dumps.iter().find(|(name, _)| name == "task_logs").unwrap();

        let lines: Vec<&str> = logs_sql.lines().collect();
        assert_eq!(lines.len(), 2);
        // Both are for task 1, sorted by created_at
        assert!(lines[0].contains("'2026-01-15T10:30:00Z'"));
        assert!(lines[1].contains("'2026-01-15T14:00:00Z'"));
        // Should use task_text_id
        assert!(lines[0].contains("task_text_id"));
        assert!(lines[0].contains("'01TASK_AAAA'"));
    }

    #[test]
    fn test_sql_escaping() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        conn.execute(
            "INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
             VALUES('01ESCAPE_TEST', 'It''s a test', 'escape-test', 0, '00000001')",
            [],
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, epochs_sql) = &dumps[0];

        // Single quote in title should be escaped as ''
        assert!(epochs_sql.contains("'It''s a test'"));
    }

    #[test]
    fn test_empty_tables_produce_empty_output() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        let dumps = dump_tables(conn).expect("dump should succeed");

        // All registered projection tables should be present but empty.
        assert_eq!(dumps.len(), TABLE_ORDER.len());
        for (name, sql) in &dumps {
            assert!(
                sql.is_empty(),
                "table {name} should be empty but got: {sql}"
            );
        }
    }

    #[test]
    fn workspace_active_phase_is_not_dumped() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        conn.execute_batch(
            "INSERT INTO epochs_data(text_id, title, sort_key)
                 VALUES('01EPOCH_PIN', 'Pinned Epoch', 'a');
             INSERT INTO phases_data(text_id, title, status, epoch_id, kind, sort_key)
                 VALUES('01PHASE_PIN', 'Pinned Phase', 'in-progress', 1, 'regular', 'a');
             INSERT INTO workspace_active_phase_data(workspace_root, phase_id)
                 VALUES('/tmp/exo-workspace', 1);",
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        let dumps = dump_tables(conn).expect("dump should succeed");

        assert!(
            dumps
                .iter()
                .all(|(name, _)| name != "workspace_active_phase_data"),
            "workspace_active_phase_data should not be dumped"
        );
        assert!(
            !TABLE_ORDER
                .iter()
                .any(|(_, table)| *table == "workspace_active_phase_data"),
            "workspace_active_phase_data should not be in TABLE_ORDER"
        );
    }

    #[test]
    fn phase_ownership_is_not_dumped() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        conn.execute_batch(
            "INSERT INTO epochs_data(text_id, title, sort_key)
                 VALUES('01EPOCH_OWNER', 'Owned Epoch', 'a');
             INSERT INTO phases_data(text_id, title, status, epoch_id, kind, sort_key)
                 VALUES('01PHASE_OWNER', 'Owned Phase', 'in-progress', 1, 'regular', 'a');
             INSERT INTO phase_ownership_data(phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root)
                 VALUES(1, 'workspace', 'workspace:project:abc123', 'workspace:project:abc123', '/tmp/exo-workspace');",
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        let dumps = dump_tables(conn).expect("dump should succeed");

        assert!(
            dumps.iter().all(|(name, _)| name != "phase_ownership_data"),
            "phase_ownership_data should not be dumped"
        );
        assert!(
            !TABLE_ORDER
                .iter()
                .any(|(_, table)| *table == "phase_ownership_data"),
            "phase_ownership_data should not be in TABLE_ORDER"
        );
    }

    // === Helpers ===

    /// Prepend the same comment header that `write_sql_dump` adds to each
    /// `.sql` file on disk. Import tests should use this so they exercise
    /// the real on-disk format, not a test-only variant without headers.
    fn with_file_headers(dumps: Vec<TableDump>) -> Vec<TableDump> {
        dumps
            .into_iter()
            .map(|(name, sql)| {
                let with_header =
                    format!("-- Auto-generated by exo. Regenerate: exo status\n{sql}");
                (name, with_header)
            })
            .collect()
    }

    // === Round-trip test ===
    // Validates the full cycle: fixture → dump → reimport → identical dump.

    #[test]
    fn test_round_trip_dump_import_dump() {
        let db1 = open_memory_database().expect("should create db1");
        insert_fixture(db1.connection());

        // Dump from db1
        let dumps1 = dump_tables(db1.connection()).expect("first dump should succeed");

        // Add file headers (matching write_sql_dump) before import
        let dumps1_with_headers = with_file_headers(dumps1.clone());

        // Import into a fresh db2
        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &dumps1_with_headers).expect("import should succeed");

        // Dump from db2
        let dumps2 = dump_tables(db2.connection()).expect("second dump should succeed");

        // Verify identical output (comparing raw SQL without headers)
        assert_eq!(dumps1.len(), dumps2.len(), "table count mismatch");
        for ((name1, sql1), (name2, sql2)) in dumps1.iter().zip(dumps2.iter()) {
            assert_eq!(name1, name2, "table name mismatch");
            assert_eq!(sql1, sql2, "round-trip mismatch for table {name1}");
        }
    }

    #[test]
    fn test_import_resolves_fks_correctly() {
        let db1 = open_memory_database().expect("should create db1");
        insert_fixture(db1.connection());
        let dumps = dump_tables(db1.connection()).expect("dump should succeed");

        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &with_file_headers(dumps)).expect("import should succeed");

        // Verify FK integrity by querying with JOINs
        let conn = db2.connection();

        // Phase → Epoch FK
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM phases_data p
                 JOIN epochs_data e ON p.epoch_id = e.id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "all phases should join to epochs");

        // Goal → Phase FK
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM goals_data g
                 JOIN phases_data p ON g.phase_id = p.id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "all goals should join to phases");

        // Task → Goal FK
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks_data t
                 JOIN goals_data g ON t.goal_id = g.id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "all tasks should join to goals");

        // Task logs → Task FK
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_logs tl
                 JOIN tasks_data t ON tl.task_id = t.id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "all task_logs should join to tasks");

        // Entity aliases → dynamic FK
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM entity_aliases", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2, "entity_aliases should be imported");
    }

    #[test]
    fn test_import_empty_tables() {
        let db = open_memory_database().expect("should create db");
        let empty_dumps = dump_tables(db.connection()).expect("dump should succeed");

        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &with_file_headers(empty_dumps))
            .expect("import of empty tables should succeed");

        let dumps2 = dump_tables(db2.connection()).expect("second dump should succeed");
        for (name, sql) in &dumps2 {
            assert!(sql.is_empty(), "table {name} should still be empty");
        }
    }

    #[test]
    fn test_parse_insert_with_escaped_quotes() {
        let line = "INSERT INTO epochs_data(text_id, title) VALUES('01ABC', 'It''s a test');";
        let parsed = parse_insert(line, "epochs_data", 1).unwrap();
        assert_eq!(parsed.columns, vec!["text_id", "title"]);
        assert_eq!(parsed.values.len(), 2);
        match &parsed.values[1] {
            SqlValue::Text(s) => assert_eq!(s, "It's a test"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_insert_with_null_and_integer() {
        let line = "INSERT INTO goals_data(text_id, target_stage, rfc) VALUES('01XYZ', 3, NULL);";
        let parsed = parse_insert(line, "goals_data", 1).unwrap();
        assert_eq!(parsed.values.len(), 3);
        match &parsed.values[0] {
            SqlValue::Text(s) => assert_eq!(s, "01XYZ"),
            other => panic!("expected Text, got {:?}", other),
        }
        match &parsed.values[1] {
            SqlValue::Integer(n) => assert_eq!(*n, 3),
            other => panic!("expected Integer, got {:?}", other),
        }
        assert!(matches!(parsed.values[2], SqlValue::Null));
    }

    #[test]
    fn test_newline_round_trip() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        conn.execute(
            "INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
             VALUES('01NEWLINE', 'Line one\nLine two\nLine three', 'newline-test', 0, '00000001')",
            [],
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        // Dump should produce single-line output with escaped newlines
        let dumps = dump_tables(conn).expect("dump should succeed");
        let (_, epochs_sql) = &dumps[0];
        let lines: Vec<&str> = epochs_sql.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "should be exactly one line (newlines escaped)"
        );
        assert!(lines[0].contains("\\n"), "should contain escaped newline");
        assert!(
            !lines[0].contains('\n'),
            "should not contain literal newline"
        );

        // Round-trip: import into fresh DB (with headers), dump again, compare
        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &with_file_headers(dumps.clone()))
            .expect("import should succeed");
        let dumps2 = dump_tables(db2.connection()).expect("second dump should succeed");

        assert_eq!(
            dumps[0].1, dumps2[0].1,
            "round-trip should preserve newlines"
        );

        // Verify the actual data has real newlines
        let title: String = db2
            .connection()
            .query_row(
                "SELECT title FROM epochs_data WHERE text_id = '01NEWLINE'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Line one\nLine two\nLine three");
    }

    #[test]
    fn test_backslash_round_trip() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        conn.execute(
            "INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
             VALUES('01BACKSLASH', 'path\\to\\file', 'backslash-test', 0, '00000001')",
            [],
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        let dumps = dump_tables(conn).expect("dump should succeed");
        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &with_file_headers(dumps)).expect("import should succeed");

        let title: String = db2
            .connection()
            .query_row(
                "SELECT title FROM epochs_data WHERE text_id = '01BACKSLASH'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "path\\to\\file");
    }

    #[test]
    fn test_mixed_escapes_round_trip() {
        let db = open_memory_database().expect("should create in-memory database");
        let conn = db.connection();

        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, false)
            .unwrap();
        // Text with quotes, newlines, backslashes, and parens
        conn.execute(
            "INSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key)
             VALUES('01MIXED', 'It''s a\ntest with (parens) and \\backslash', 'mixed', 0, '00000001')",
            [],
        )
        .unwrap();
        conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
            .unwrap();

        let dumps = dump_tables(conn).expect("dump should succeed");
        let db2 = open_memory_database().expect("should create db2");
        import_tables(db2.connection(), &with_file_headers(dumps.clone()))
            .expect("import should succeed");
        let dumps2 = dump_tables(db2.connection()).expect("second dump should succeed");

        assert_eq!(dumps[0].1, dumps2[0].1, "round-trip should be identical");

        let title: String = db2
            .connection()
            .query_row(
                "SELECT title FROM epochs_data WHERE text_id = '01MIXED'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "It's a\ntest with (parens) and \\backslash");
    }
}
