//! Plugin to update project prompts.
//!
//! This plugin ensures that the project's prompt files in `.github/prompts`
//! are up-to-date with the latest templates.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::templates;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};

/// Updates project prompt files in `.github/prompts`.
///
/// This plugin always reports as needed since prompts may have been updated
/// in the CLI and the project should receive the latest versions.
///
/// # Severity
///
/// **Info** - Prompt updates are non-breaking and purely additive.
#[derive(Debug, Clone, Copy)]
pub struct UpdatePromptsPlugin;

impl UpgradePlugin for UpdatePromptsPlugin {
    fn id(&self) -> &str {
        "update-prompts-v1"
    }

    fn description(&self) -> &str {
        "Updates project prompt files in .github/prompts"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, _context: &AgentContext) -> ExoResult<UpgradeStatus> {
        // Prompts should always be refreshed to ensure latest templates
        Ok(UpgradeStatus::info("Prompt files may need updating"))
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let written = templates::install_project_prompts(&context.root)?;

        if written > 0 {
            Ok(UpgradeReport::with_changes(
                self.id(),
                vec![format!("Installed/updated {written} prompt files")],
            ))
        } else {
            Ok(UpgradeReport::no_changes(self.id()))
        }
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let prompts_dir = context.root.join(".github/prompts");
        if prompts_dir.exists() && prompts_dir.is_dir() {
            Ok(())
        } else {
            anyhow::bail!("Verification failed: .github/prompts directory not found")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        let context = AgentContext::new_for_testing(root);
        (temp_dir, context)
    }

    #[test]
    fn test_always_reports_needed() {
        let (_temp, context) = setup_test_context();
        let plugin = UpdatePromptsPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Info));
    }

    #[test]
    fn test_apply_creates_prompts_directory() {
        let (_temp, mut context) = setup_test_context();
        let plugin = UpdatePromptsPlugin;

        let report = plugin.apply(&mut context).unwrap();

        // Should have created prompts
        assert!(report.applied);
        assert!(!report.changes.is_empty());

        // Verify directory exists
        let prompts_dir = context.root.join(".github/prompts");
        assert!(prompts_dir.exists());
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = UpdatePromptsPlugin;

        // Apply twice
        let report1 = plugin.apply(&mut context).unwrap();
        let _report2 = plugin.apply(&mut context).unwrap();

        // Both should succeed
        assert!(report1.applied);
        // Second apply may or may not report changes depending on template logic,
        // but it should not fail
        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_verify_passes_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = UpdatePromptsPlugin;

        plugin.apply(&mut context).unwrap();
        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_verify_fails_without_prompts_dir() {
        let (_temp, context) = setup_test_context();
        let plugin = UpdatePromptsPlugin;

        // Don't apply - verify should fail
        assert!(plugin.verify(&context).is_err());
    }
}
