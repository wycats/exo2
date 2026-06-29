//! Plugin to import legacy `docs/agent-context/plan.toml` roadmap state.

use crate::ExoResult;
use crate::context::{AgentContext, ExoState, SqliteLoader};
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use anyhow::Context as _;
use std::path::PathBuf;

const LEGACY_PLAN_RELATIVE_PATH: &str = "docs/agent-context/plan.toml";

/// Imports the legacy roadmap TOML into SQLite when the SQLite roadmap is empty.
///
/// Older Exosuit workspaces can have a populated `docs/agent-context/plan.toml`
/// plus empty SQL projections. In that state `exo status` incorrectly reports a
/// brand-new project. This upgrade preserves the legacy roadmap by importing it
/// into the active project database and regenerating the active SQL projection
/// according to the repo's state policy.
#[derive(Debug, Clone, Copy)]
pub struct MigrateLegacyPlanPlugin;

impl MigrateLegacyPlanPlugin {
    fn legacy_plan_path(context: &AgentContext) -> PathBuf {
        context.root.join(LEGACY_PLAN_RELATIVE_PATH)
    }

    fn load_legacy_plan(context: &AgentContext) -> ExoResult<Option<ExoState>> {
        let path = Self::legacy_plan_path(context);
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read legacy plan {}", path.display()))?;
        let plan = toml::from_str::<ExoState>(&content)
            .with_context(|| format!("Failed to parse legacy plan {}", path.display()))?;

        if plan.epochs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(plan))
        }
    }

    fn sqlite_epoch_count(context: &AgentContext) -> ExoResult<usize> {
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        if !db_path.exists() {
            return Ok(0);
        }

        let loader = SqliteLoader::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
        let count: i64 = loader
            .database()
            .connection()
            .query_row("SELECT COUNT(*) FROM epochs_data", [], |row| row.get(0))
            .with_context(|| "Failed to count SQLite roadmap epochs")?;
        usize::try_from(count)
            .with_context(|| format!("SQLite roadmap epoch count {count} does not fit in usize"))
    }

    fn legacy_epoch_count_needed(context: &AgentContext) -> ExoResult<Option<usize>> {
        if Self::sqlite_epoch_count(context)? > 0 {
            return Ok(None);
        }

        Ok(Self::load_legacy_plan(context)?.map(|plan| plan.epochs.len()))
    }
}

impl UpgradePlugin for MigrateLegacyPlanPlugin {
    fn id(&self) -> &str {
        "migrate-legacy-plan-v1"
    }

    fn description(&self) -> &str {
        "Imports legacy docs/agent-context/plan.toml roadmap state into SQLite"
    }

    fn severity(&self) -> Severity {
        Severity::Critical
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        if let Some(epoch_count) = Self::legacy_epoch_count_needed(context)? {
            Ok(UpgradeStatus::critical(format!(
                "Legacy {LEGACY_PLAN_RELATIVE_PATH} contains {epoch_count} epoch(s), but SQLite has no roadmap state"
            )))
        } else {
            Ok(UpgradeStatus::NotNeeded)
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        if Self::sqlite_epoch_count(context)? > 0 {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        let Some(plan) = Self::load_legacy_plan(context)? else {
            return Ok(UpgradeReport::no_changes(self.id()));
        };

        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let loader = SqliteLoader::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
        let result = loader
            .import_plan(&plan)
            .with_context(|| "Failed to import legacy roadmap into SQLite")?;

        crate::context::write_sql_dump_with_project_result(&context.root, context.project.as_ref())
            .with_context(|| "Failed to write SQL projections after legacy roadmap import")?;

        Ok(UpgradeReport::with_changes(
            self.id(),
            vec![format!(
                "Imported {} epoch(s), {} phase(s), and {} goal(s) from {LEGACY_PLAN_RELATIVE_PATH}",
                result.epochs_imported, result.phases_imported, result.goals_imported
            )],
        ))
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        if Self::legacy_epoch_count_needed(context)?.is_some() {
            anyhow::bail!(
                "Verification failed: legacy {LEGACY_PLAN_RELATIVE_PATH} still needs import"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SQLITE_DB_PATH;
    use std::fs;

    fn setup_context() -> (tempfile::TempDir, AgentContext) {
        let temp = tempfile::TempDir::new().expect("create tempdir");
        let root = temp.path().to_path_buf();
        fs::create_dir_all(root.join("docs/agent-context")).expect("create agent-context dir");
        fs::create_dir_all(root.join(".cache")).expect("create cache dir");
        exosuit_storage::open_database(root.join(SQLITE_DB_PATH)).expect("create sqlite db");
        let context = AgentContext::new_for_testing(root);
        (temp, context)
    }

    fn write_legacy_plan(context: &AgentContext) {
        fs::write(
            context.root.join(LEGACY_PLAN_RELATIVE_PATH),
            r#"[[epochs]]
id = "legacy-epoch"
title = "Legacy Epoch"
status = "active"

[[epochs.phases]]
id = "legacy-phase"
title = "Legacy Phase"
status = "active"
tasks = ["Legacy Goal"]
"#,
        )
        .expect("write legacy plan");
    }

    #[test]
    fn needed_when_sqlite_has_no_epochs_and_legacy_plan_has_epochs() {
        let (_temp, context) = setup_context();
        write_legacy_plan(&context);
        let plugin = MigrateLegacyPlanPlugin;

        let status = plugin.is_needed(&context).expect("check status");

        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Critical));
    }

    #[test]
    fn apply_imports_legacy_plan_and_becomes_idempotent() {
        let (_temp, mut context) = setup_context();
        write_legacy_plan(&context);
        let plugin = MigrateLegacyPlanPlugin;

        let report = plugin.apply(&mut context).expect("apply migration");
        assert!(report.applied);
        assert!(
            !plugin
                .is_needed(&context)
                .expect("check status")
                .is_needed()
        );

        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let loader = SqliteLoader::open(&db_path).expect("open sqlite db");
        let state = loader.load_state().expect("load state");
        assert_eq!(state.epochs.len(), 1);
        assert_eq!(state.epochs[0].title, "Legacy Epoch");
        assert_eq!(state.epochs[0].phases[0].status, "in-progress");
        assert_eq!(state.epochs[0].phases[0].goals[0].label, "Legacy Goal");

        let second_report = plugin.apply(&mut context).expect("reapply migration");
        assert!(!second_report.applied);
    }

    #[test]
    fn sqlite_epoch_count_uses_direct_epoch_count() {
        let (_temp, context) = setup_context();
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let loader = SqliteLoader::open(&db_path).expect("open sqlite db");
        loader
            .database()
            .connection()
            .execute(
                "INSERT INTO epochs (text_id, title) VALUES ('counted-epoch', 'Counted Epoch')",
                [],
            )
            .expect("insert epoch");

        assert_eq!(
            MigrateLegacyPlanPlugin::sqlite_epoch_count(&context).unwrap(),
            1
        );
    }
}
