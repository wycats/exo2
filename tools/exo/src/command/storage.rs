//! Storage maintenance commands.
//!
//! - `storage maintain`: Run bounded physical SQLite maintenance

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::steering::SuggestedAction;
use anyhow::{Context, Result as ExoResult};
use exosuit_storage::{
    AutoVacuumMode, Connection, DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET, StorageMaintenanceOptions,
    StorageMaintenanceReport, StorageMaintenanceStats, WalCheckpointReport, maintain_database,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, exospec::ExoSpec)]
#[exo(
    namespace = "storage",
    description = "SQLite storage maintenance commands"
)]
pub enum StorageCommands {
    #[exo(
        effect = "exec",
        description = "Run bounded physical SQLite maintenance for the current project"
    )]
    Maintain {
        #[exo(
            flag,
            description = "Convert the current DB to incremental auto-vacuum before maintenance"
        )]
        enable_incremental_vacuum: bool,
        #[exo(
            long,
            optional,
            default = "256",
            description = "Maximum incremental vacuum pages to reclaim"
        )]
        vacuum_pages: Option<i64>,
    },
}

impl StorageCommands {
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Maintain {
                enable_incremental_vacuum,
                vacuum_pages,
            } => {
                let vacuum_pages = vacuum_pages
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(DEFAULT_INCREMENTAL_VACUUM_PAGE_BUDGET);
                CommandBox::mutable(StorageMaintain::new(
                    enable_incremental_vacuum,
                    vacuum_pages,
                ))
            }
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageMaintain {
    enable_incremental_vacuum: bool,
    vacuum_pages: u32,
}

impl StorageMaintain {
    pub const fn new(enable_incremental_vacuum: bool, vacuum_pages: u32) -> Self {
        Self {
            enable_incremental_vacuum,
            vacuum_pages,
        }
    }
}

#[derive(Debug, Serialize)]
struct StorageMaintainOutput {
    kind: &'static str,
    ok: bool,
    db_path: PathBuf,
    maintenance: StorageMaintenanceOutput,
}

#[derive(Debug, Serialize)]
struct StorageMaintenanceOutput {
    before: StorageMaintenanceStatsOutput,
    after: StorageMaintenanceStatsOutput,
    conversion_performed: bool,
    vacuum_performed: bool,
    incremental_vacuum_pages_requested: u32,
    incremental_vacuum_steps_run: u32,
    wal_checkpoint: Option<WalCheckpointOutput>,
}

#[derive(Debug, Serialize)]
struct StorageMaintenanceStatsOutput {
    page_size: i64,
    page_count: i64,
    freelist_count: i64,
    reclaimable_bytes: i64,
    auto_vacuum: &'static str,
    auto_vacuum_code: i64,
}

#[derive(Debug, Serialize)]
struct WalCheckpointOutput {
    mode: &'static str,
    busy: i64,
    log_pages: i64,
    checkpointed_pages: i64,
}

impl From<StorageMaintenanceReport> for StorageMaintenanceOutput {
    fn from(report: StorageMaintenanceReport) -> Self {
        Self {
            before: report.before.into(),
            after: report.after.into(),
            conversion_performed: report.conversion_performed,
            vacuum_performed: report.vacuum_performed,
            incremental_vacuum_pages_requested: report.incremental_vacuum_pages_requested,
            incremental_vacuum_steps_run: report.incremental_vacuum_steps_run,
            wal_checkpoint: report.wal_checkpoint.map(Into::into),
        }
    }
}

impl From<StorageMaintenanceStats> for StorageMaintenanceStatsOutput {
    fn from(stats: StorageMaintenanceStats) -> Self {
        Self {
            page_size: stats.page_size,
            page_count: stats.page_count,
            freelist_count: stats.freelist_count,
            reclaimable_bytes: stats.reclaimable_bytes,
            auto_vacuum: stats.auto_vacuum.as_str(),
            auto_vacuum_code: stats.auto_vacuum.as_i64(),
        }
    }
}

impl From<WalCheckpointReport> for WalCheckpointOutput {
    fn from(report: WalCheckpointReport) -> Self {
        Self {
            mode: report.mode,
            busy: report.busy,
            log_pages: report.log_pages,
            checkpointed_pages: report.checkpointed_pages,
        }
    }
}

impl Command for StorageMaintain {
    fn namespace(&self) -> &'static str {
        "storage"
    }

    fn operation(&self) -> &'static str {
        "maintain"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn description(&self) -> &'static str {
        "Run bounded physical SQLite maintenance for the current project"
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        Vec::new()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("StorageMaintain should be dispatched via execute_mut")
    }
}

impl MutableCommand for StorageMaintain {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let db_path = ctx.db_path();
        let conn = open_physical_maintenance_connection(&db_path)?;
        let report = maintain_database(
            &conn,
            StorageMaintenanceOptions {
                enable_incremental_vacuum: self.enable_incremental_vacuum,
                vacuum_page_budget: self.vacuum_pages,
                checkpoint_wal: true,
            },
        )
        .context("run SQLite storage maintenance")?;

        let output = StorageMaintainOutput {
            kind: "storage.maintain",
            ok: true,
            db_path,
            maintenance: report.into(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let before = &output.maintenance.before;
                let after = &output.maintenance.after;
                let mut message = format!(
                    "Storage maintenance complete: reclaimable bytes {} -> {}, freelist pages {} -> {}, auto_vacuum {}.",
                    before.reclaimable_bytes,
                    after.reclaimable_bytes,
                    before.freelist_count,
                    after.freelist_count,
                    after.auto_vacuum
                );
                if output.maintenance.conversion_performed {
                    message.push_str(" Incremental auto-vacuum was enabled with VACUUM.");
                }
                if output.maintenance.incremental_vacuum_steps_run > 0 {
                    message.push_str(&format!(
                        " Reclaimed up to {} page(s).",
                        output.maintenance.incremental_vacuum_steps_run
                    ));
                }
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

fn open_physical_maintenance_connection(db_path: &Path) -> ExoResult<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create SQLite database directory {}", parent.display()))?;
    }

    let should_enable_incremental_auto_vacuum = is_new_or_empty_database_file(db_path);
    let conn = Connection::open(db_path)
        .with_context(|| format!("open SQLite database at {}", db_path.display()))?;

    if should_enable_incremental_auto_vacuum {
        conn.pragma_update(None, "auto_vacuum", AutoVacuumMode::Incremental.as_i64())
            .context("enable incremental auto-vacuum for new SQLite database")?;
    }
    conn.pragma_update(None, "busy_timeout", 5000)
        .context("configure SQLite busy timeout")?;

    Ok(conn)
}

fn is_new_or_empty_database_file(path: &Path) -> bool {
    match path.metadata() {
        Ok(metadata) => metadata.len() == 0,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;
    use crate::command::traits::OutputFormat;
    use exosuit_storage::AutoVacuumMode;
    use tempfile::tempdir;

    #[test]
    fn storage_maintain_metadata() {
        let cmd = StorageMaintain::new(false, 256);
        assert_eq!(cmd.namespace(), "storage");
        assert_eq!(cmd.operation(), "maintain");
        assert_eq!(cmd.effect(), Effect::Exec);
    }

    #[test]
    fn auto_vacuum_mode_labels_are_stable() {
        assert_eq!(AutoVacuumMode::None.as_str(), "none");
        assert_eq!(AutoVacuumMode::Full.as_str(), "full");
        assert_eq!(AutoVacuumMode::Incremental.as_str(), "incremental");
        assert_eq!(AutoVacuumMode::Unknown(99).as_str(), "unknown");
    }

    #[test]
    fn storage_maintain_json_reports_physical_metrics() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = MutableCommandContext {
            root: temp.path(),
            project: None,
            format: OutputFormat::Json,
            agent_id: None,
            workflow_confirmation: None,
        };

        let output = StorageMaintain::new(false, 4)
            .execute_mut(&mut ctx)
            .expect("run storage maintenance");

        assert_eq!(output.data["kind"], "storage.maintain");
        assert_eq!(output.data["ok"], true);
        assert_eq!(
            output.data["maintenance"]["after"]["auto_vacuum"],
            "incremental"
        );
        assert!(
            output.data["maintenance"]["after"]["page_size"]
                .as_i64()
                .expect("page_size")
                > 0
        );
        assert!(output.data["maintenance"]["wal_checkpoint"].is_object());
    }

    #[test]
    fn storage_maintain_does_not_run_logical_migrations() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = MutableCommandContext {
            root: temp.path(),
            project: None,
            format: OutputFormat::Json,
            agent_id: None,
            workflow_confirmation: None,
        };

        let output = StorageMaintain::new(false, 4)
            .execute_mut(&mut ctx)
            .expect("run storage maintenance");

        assert_eq!(output.data["kind"], "storage.maintain");
        let conn = exosuit_storage::Connection::open(temp.path().join(".cache/exo.db"))
            .expect("open maintained db");
        let migration_table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
                [],
                |row| row.get(0),
            )
            .expect("query schema_migrations");
        assert!(!migration_table_exists);
    }

    #[test]
    fn storage_maintain_conversion_flag_enables_incremental_auto_vacuum() {
        let temp = tempdir().expect("tempdir");
        let db_dir = temp.path().join(".cache");
        fs::create_dir_all(&db_dir).expect("create db dir");
        let db_path = db_dir.join("exo.db");
        {
            let conn = exosuit_storage::Connection::open(&db_path).expect("open legacy db");
            conn.execute("CREATE TABLE legacy_data (id INTEGER PRIMARY KEY)", [])
                .expect("create legacy table");
        }

        let mut ctx = MutableCommandContext {
            root: temp.path(),
            project: None,
            format: OutputFormat::Json,
            agent_id: None,
            workflow_confirmation: None,
        };

        let output = StorageMaintain::new(true, 4)
            .execute_mut(&mut ctx)
            .expect("run conversion maintenance");

        assert_eq!(output.data["maintenance"]["before"]["auto_vacuum"], "none");
        assert_eq!(
            output.data["maintenance"]["after"]["auto_vacuum"],
            "incremental"
        );
        assert_eq!(output.data["maintenance"]["conversion_performed"], true);
        assert_eq!(output.data["maintenance"]["vacuum_performed"], true);
    }
}
