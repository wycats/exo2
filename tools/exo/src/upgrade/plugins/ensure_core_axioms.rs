//! Plugin to ensure core workflow axioms exist.
//!
//! This plugin validates that essential axioms like "context-is-king" are
//! present in `SQLite`.

use crate::ExoResult;
use crate::axiom;
use crate::context::AgentContext;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};

/// Ensures core workflow axioms are present.
///
/// Checks for the presence of essential axioms like "context-is-king" and
/// logs a warning if they are missing.
///
/// # Severity
///
/// **Warning** - Missing core axioms may lead to suboptimal agent behavior.
#[derive(Debug, Clone, Copy)]
pub struct EnsureCoreAxiomsPlugin;

impl EnsureCoreAxiomsPlugin {
    /// Check if the context-is-king axiom exists in workflow axioms.
    fn has_context_is_king(&self, context: &AgentContext) -> ExoResult<bool> {
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        if !db_path.exists() {
            return Ok(false);
        }

        let axioms =
            axiom::list_axioms_with_project(&context.root, context.project.as_ref(), "workflow")?;

        Ok(axioms.iter().any(|a| {
            matches!(a.id.as_str(), "context-is-king" | "1-context-is-king")
                || a.principle.contains("Context is King")
        }))
    }
}

impl UpgradePlugin for EnsureCoreAxiomsPlugin {
    fn id(&self) -> &str {
        "ensure-core-axioms-v1"
    }

    fn description(&self) -> &str {
        "Validates core workflow axioms like 'context-is-king' are present"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        if self.has_context_is_king(context)? {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::warning(
                "Core workflow axiom 'context-is-king' is missing",
            ))
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        if self.has_context_is_king(context)? {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        // Log a warning about missing core axiom
        // This plugin doesn't automatically add axioms - that requires user action
        let report = UpgradeReport::no_changes(self.id()).with_warning(
            "Core axiom 'context-is-king' is missing. Consider adding it via `exo axiom add`",
        );

        Ok(report)
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        if db_path.exists() {
            Ok(())
        } else {
            anyhow::bail!("Verification failed: SQLite database does not exist")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SQLITE_DB_PATH;
    use crate::context::SqliteWriter;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        // Create directory structure
        fs::create_dir_all(root.join("docs/agent-context")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        SqliteWriter::open(&db_path).unwrap();

        let context = AgentContext::new_for_testing(root);
        (temp_dir, context)
    }

    fn insert_workflow_axiom(context: &AgentContext, id: &str, principle: &str) {
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let writer = SqliteWriter::open(&db_path).unwrap();
        writer
            .add_axiom(
                id,
                "workflow",
                principle,
                Some("test rationale"),
                None,
                &["test implication".to_string()],
                &["core".to_string(), "workflow".to_string()],
            )
            .unwrap();
    }

    #[test]
    fn test_needed_when_database_has_no_context_is_king_axiom() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Warning));
    }

    #[test]
    fn test_needed_when_other_workflow_axioms_exist_but_not_context_is_king() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        insert_workflow_axiom(&context, "some-other-axiom", "Some other principle");

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Warning));
    }

    #[test]
    fn test_not_needed_when_context_is_king_exists() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        insert_workflow_axiom(&context, "context-is-king", "Context is King");

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_apply_adds_warning_for_missing_axiom() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        insert_workflow_axiom(&context, "some-axiom", "Some principle");

        let report = plugin.apply(&mut context).unwrap();

        // Should have warning about missing axiom
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_verify_passes_with_sqlite_database() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        insert_workflow_axiom(&context, "some-axiom", "Some principle");

        // Apply twice - should not fail
        let report1 = plugin.apply(&mut context).unwrap();
        let report2 = plugin.apply(&mut context).unwrap();

        // Both should be no-op with a warning (this plugin doesn't auto-add axioms)
        assert!(!report1.applied);
        assert!(!report2.applied);
    }

    #[test]
    fn test_is_needed_false_when_axiom_present() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureCoreAxiomsPlugin;

        insert_workflow_axiom(&context, "1-context-is-king", "Context is King");

        // Should not be needed since axiom is present
        assert!(!plugin.is_needed(&context).unwrap().is_needed());
    }
}
