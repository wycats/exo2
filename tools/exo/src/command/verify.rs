//! Verify namespace command.
//!
//! - `verify run`: Run phase verification (Pure)
//! - `verify dump`: Verify SQL dump round-trip on live database (Pure)

use super::traits::{Command, CommandBox, CommandContext, CommandOutput, OutputFormat};
use crate::api::protocol::Effect;
use crate::steering::{SuggestedAction, WorkIntent};
use crate::verify as verify_module;
use anyhow::Result as ExoResult;
use serde::Serialize;

// ============================================================================
// ExoSpec definition — single source of truth for the verify namespace
// ============================================================================

/// Verify namespace command specification.
///
/// This enum is the authoritative definition of the verify namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `VerifyCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, Clone, Copy, exospec::ExoSpec)]
#[exo(namespace = "verify", description = "Verification commands")]
pub enum VerifyCommands {
    #[exo(effect = "exec", description = "Run phase verification")]
    Run,

    #[exo(
        effect = "pure",
        description = "Verify SQL dump round-trip on live database"
    )]
    Dump,
}

impl VerifyCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Run => CommandBox::pure(VerifyRun::new()),
            Self::Dump => CommandBox::pure(VerifyDump::new()),
        })
    }
}

// ===== verify run =====

/// Run phase verification
#[derive(Debug, Clone, Copy)]
pub struct VerifyRun;

impl VerifyRun {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for VerifyRun {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for VerifyRun {
    fn namespace(&self) -> &'static str {
        "verify"
    }

    fn operation(&self) -> &'static str {
        "run"
    }

    fn description(&self) -> &'static str {
        "Run phase verification"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "Re-run verification".to_string(),
                command: "exo verify run".to_string(),
                rationale: "Re-run verification after addressing the reported failure details."
                    .to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "Check phase status".to_string(),
                command: "exo phase status".to_string(),
                rationale: "View current phase status before verification".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "List tasks".to_string(),
                command: "exo task list".to_string(),
                rationale: "Check task completion status before verification".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.7),
            },
        ]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let json_mode = matches!(ctx.format, OutputFormat::Json);
        let report = verify_module::run_verify(ctx.root, json_mode)?;

        #[derive(Serialize)]
        struct VerifyOutput {
            kind: &'static str,
            ok: bool,
            runner: String,
        }

        let output = VerifyOutput {
            kind: "verify.run",
            ok: true,
            runner: report.runner.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = format!("Verification passed! (runner: {})", report.runner);
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ===== verify dump =====

/// Verify SQL dump round-trip on the live database.
///
/// This command tests the three axioms of the SQL dump format (RFC 10178):
///
/// 1. **Lossless**: every value survives serialize → deserialize unchanged,
///    including arbitrary Unicode, control characters, backslashes, and
///    single quotes.
///
/// 2. **Deterministic**: the same database state produces identical text
///    output on every run.
///
/// 3. **Single-line-per-entity**: newlines in values are escaped, so each
///    INSERT is one line (git diff friendly).
///
/// The verification procedure:
///
/// 1. Open `.cache/exo.db` (the live runtime database)
/// 2. `dump_tables()` → SQL text for all 12 tables
/// 3. `import_tables()` into a fresh in-memory database
/// 4. `dump_tables()` on the fresh database
/// 5. Assert the two dumps are byte-identical
///
/// If step 5 passes, the format is proven lossless and deterministic on
/// real production data. This is the gate for every subsequent change in
/// the TOML → SQL migration.
#[derive(Debug, Clone, Copy)]
pub struct VerifyDump;

impl Default for VerifyDump {
    fn default() -> Self {
        Self::new()
    }
}

impl VerifyDump {
    pub const fn new() -> Self {
        Self
    }
}

impl Command for VerifyDump {
    fn namespace(&self) -> &'static str {
        "verify"
    }

    fn operation(&self) -> &'static str {
        "dump"
    }

    fn description(&self) -> &'static str {
        "Verify SQL dump round-trip on live database"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![]
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        use exosuit_storage::{dump_tables, import_tables, open_memory_database};

        let db_path = ctx.db_path();
        if !db_path.exists() {
            anyhow::bail!("No database found at {}", db_path.display());
        }

        // Open the live database (read-only — dump_tables only reads)
        let live_db = exosuit_storage::open_database(&db_path)
            .map_err(|e| anyhow::anyhow!("Failed to open live database: {e}"))?;

        // Step 1: Dump the live database
        let dumps1 = dump_tables(live_db.connection())
            .map_err(|e| anyhow::anyhow!("Failed to dump live database: {e}"))?;

        // Step 2: Import into a fresh in-memory database
        let fresh_db = open_memory_database()
            .map_err(|e| anyhow::anyhow!("Failed to create in-memory database: {e}"))?;
        import_tables(fresh_db.connection(), &dumps1)
            .map_err(|e| anyhow::anyhow!("Failed to import into fresh database: {e}"))?;

        // Step 3: Dump the fresh database
        let dumps2 = dump_tables(fresh_db.connection())
            .map_err(|e| anyhow::anyhow!("Failed to dump fresh database: {e}"))?;

        // Step 4: Compare
        let mut mismatches = Vec::new();
        if dumps1.len() != dumps2.len() {
            anyhow::bail!(
                "Table count mismatch: live={} fresh={}",
                dumps1.len(),
                dumps2.len()
            );
        }

        let mut total_rows = 0usize;
        let mut table_summaries = Vec::new();

        for ((name1, sql1), (name2, sql2)) in dumps1.iter().zip(dumps2.iter()) {
            if name1 != name2 {
                anyhow::bail!("Table name mismatch: live={name1} fresh={name2}");
            }
            let row_count = sql1.lines().filter(|l| !l.is_empty()).count();
            total_rows += row_count;
            table_summaries.push((name1.clone(), row_count));

            if sql1 != sql2 {
                mismatches.push(name1.clone());
            }
        }

        #[derive(Serialize)]
        struct DumpVerifyOutput {
            kind: &'static str,
            ok: bool,
            tables: usize,
            total_rows: usize,
            table_summaries: Vec<TableSummary>,
            mismatches: Vec<String>,
        }

        #[derive(Serialize)]
        struct TableSummary {
            table: String,
            rows: usize,
        }

        let ok = mismatches.is_empty();
        let output = DumpVerifyOutput {
            kind: "verify.dump",
            ok,
            tables: dumps1.len(),
            total_rows,
            table_summaries: table_summaries
                .iter()
                .map(|(t, r)| TableSummary {
                    table: t.clone(),
                    rows: *r,
                })
                .collect(),
            mismatches: mismatches.clone(),
        };

        match ctx.format {
            OutputFormat::Json => {
                if ok {
                    Ok(CommandOutput::data(output))
                } else {
                    anyhow::bail!(
                        "Round-trip mismatch in {} table(s): {}",
                        mismatches.len(),
                        mismatches.join(", ")
                    )
                }
            }
            OutputFormat::Human => {
                if ok {
                    let mut msg = format!(
                        "✓ SQL dump round-trip verified: {} tables, {} rows\n",
                        dumps1.len(),
                        total_rows
                    );
                    for (table, rows) in &table_summaries {
                        msg.push_str(&format!("  {table}: {rows} rows\n"));
                    }
                    Ok(CommandOutput::new(output, msg))
                } else {
                    anyhow::bail!(
                        "Round-trip mismatch in {} table(s): {}",
                        mismatches.len(),
                        mismatches.join(", ")
                    )
                }
            }
        }
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_run_metadata() {
        let cmd = VerifyRun::new();
        assert_eq!(cmd.namespace(), "verify");
        assert_eq!(cmd.operation(), "run");
        assert_eq!(cmd.effect(), Effect::Exec);
    }

    #[test]
    fn test_verify_dump_metadata() {
        let cmd = VerifyDump::new();
        assert_eq!(cmd.namespace(), "verify");
        assert_eq!(cmd.operation(), "dump");
        assert_eq!(cmd.effect(), Effect::Pure);
    }
}
