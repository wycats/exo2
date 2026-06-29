//! Plugin to ensure the axiom SQL dump exists.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use std::path::PathBuf;

/// Ensures the axiom SQL dump exists.
///
/// # Severity
///
/// **Warning** - Missing scoped axioms may cause agent confusion.
#[derive(Debug, Clone, Copy)]
pub struct EnsureScopedAxiomsPlugin;

impl EnsureScopedAxiomsPlugin {
    fn axioms_dump_path(&self, context: &AgentContext) -> Option<PathBuf> {
        crate::context::sql_projection_dir(&context.root, context.project.as_ref())
            .map(|projection_dir| projection_dir.join("axioms.sql"))
    }

    fn axioms_dump_exists(&self, context: &AgentContext) -> bool {
        self.axioms_dump_path(context)
            .is_some_and(|path| path.exists())
    }
}

impl UpgradePlugin for EnsureScopedAxiomsPlugin {
    fn id(&self) -> &str {
        "ensure-scoped-axioms-v1"
    }

    fn description(&self) -> &str {
        "Ensures the axiom SQL dump exists"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let Some(axioms_dump_path) = self.axioms_dump_path(context) else {
            return Ok(UpgradeStatus::NotNeeded);
        };

        if self.axioms_dump_exists(context) {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::warning(format!(
                "Missing axiom SQL dump: {}",
                axioms_dump_path.display()
            )))
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let Some(axioms_dump_path) = self.axioms_dump_path(context) else {
            return Ok(UpgradeReport::no_changes(self.id()));
        };

        if self.axioms_dump_exists(context) {
            Ok(UpgradeReport::no_changes(self.id()))
        } else {
            crate::context::write_sql_dump_with_project_result(
                &context.root,
                context.project.as_ref(),
            )?;
            Ok(UpgradeReport::with_changes(
                self.id(),
                vec![format!("Created {}", axioms_dump_path.display())],
            ))
        }
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        if self.axioms_dump_path(context).is_none() {
            return Ok(());
        }

        if self.axioms_dump_exists(context) {
            Ok(())
        } else {
            anyhow::bail!("Verification failed: axiom SQL dump is missing")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, ProjectId, SidecarAutoPushPolicy, StatePolicy};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        // Create directory structure
        fs::create_dir_all(root.join("docs/agent-context")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let db_path = root.join(crate::context::SQLITE_DB_PATH);
        exosuit_storage::open_database(&db_path).unwrap();

        let context = AgentContext::new_for_testing(root);
        (temp_dir, context)
    }

    fn create_dump_file(context: &AgentContext) {
        fs::write(
            context.root.join("docs/agent-context/axioms.sql"),
            "-- SQL dump\n",
        )
        .unwrap();
    }

    fn setup_sidecar_context() -> (TempDir, TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let sidecar_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        let state_root = root.join("state");
        let db_path = state_root.join("cache/exo.db");
        fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        exosuit_storage::open_database(&db_path).unwrap();

        let project = Project {
            id: ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Sidecar,
            projects_config_path: None,
            state_root,
            sidecar_key: Some("demo".to_string()),
            sidecar_root: Some(sidecar_dir.path().to_path_buf()),
            sidecar_auto_commit: true,
            sidecar_auto_push: SidecarAutoPushPolicy::IfRemote,
        };
        let context = AgentContext {
            root,
            project: Some(project),
            plan: crate::context::ExoState::default(),
        };
        (temp_dir, sidecar_dir, context)
    }

    fn setup_shadow_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        let state_root = root.join("state");
        let db_path = state_root.join("cache/exo.db");
        fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        exosuit_storage::open_database(&db_path).unwrap();

        let project = Project {
            id: ProjectId::from_git_common_dir(&root.join(".git")),
            git_common_dir: root.join(".git"),
            workspace_root: Some(root.clone()),
            policy: StatePolicy::Shadow,
            projects_config_path: None,
            state_root,
            sidecar_key: None,
            sidecar_root: None,
            sidecar_auto_commit: false,
            sidecar_auto_push: SidecarAutoPushPolicy::Never,
        };
        let context = AgentContext {
            root,
            project: Some(project),
            plan: crate::context::ExoState::default(),
        };
        (temp_dir, context)
    }

    fn sidecar_axioms_path(sidecar_root: &Path) -> PathBuf {
        sidecar_root.join("projects/demo/agent-context/axioms.sql")
    }

    #[test]
    fn test_needed_when_no_scoped_files() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Warning));
    }

    #[test]
    fn test_not_needed_when_dump_exists() {
        let (_temp, context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        create_dump_file(&context);

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_apply_creates_missing_dump() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        let report = plugin.apply(&mut context).unwrap();

        assert!(report.applied);
        assert_eq!(report.changes.len(), 1);
        assert!(context.root.join("docs/agent-context/axioms.sql").exists());
    }

    #[test]
    fn test_apply_uses_sidecar_projection_for_sidecar_policy() {
        let (_temp, sidecar, mut context) = setup_sidecar_context();
        let plugin = EnsureScopedAxiomsPlugin;

        let report = plugin.apply(&mut context).unwrap();

        assert!(report.applied);
        assert!(sidecar_axioms_path(sidecar.path()).exists());
        assert!(!context.root.join("docs/agent-context/axioms.sql").exists());
        assert!(plugin.verify(&context).is_ok());
        assert!(!plugin.is_needed(&context).unwrap().is_needed());
    }

    #[test]
    fn test_shadow_policy_does_not_expect_sql_projection() {
        let (_temp, mut context) = setup_shadow_context();
        let plugin = EnsureScopedAxiomsPlugin;

        let status = plugin.is_needed(&context).unwrap();
        let report = plugin.apply(&mut context).unwrap();

        assert!(!status.is_needed());
        assert!(!report.applied);
        assert!(plugin.verify(&context).is_ok());
        assert!(!context.root.join("docs/agent-context/axioms.sql").exists());
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        create_dump_file(&context);

        // Apply twice
        plugin.apply(&mut context).unwrap();
        let report = plugin.apply(&mut context).unwrap();

        // Second apply should report no changes
        assert!(!report.applied);
    }

    #[test]
    fn test_verify_passes_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        create_dump_file(&context);
        plugin.apply(&mut context).unwrap();
        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_is_needed_false_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = EnsureScopedAxiomsPlugin;

        // Initially needed (no scoped files)
        assert!(plugin.is_needed(&context).unwrap().is_needed());

        // Creating the dump satisfies the plugin's current contract
        create_dump_file(&context);

        // Apply becomes a no-op once the dump exists
        plugin.apply(&mut context).unwrap();

        // Now should not be needed
        assert!(!plugin.is_needed(&context).unwrap().is_needed());
    }
}
