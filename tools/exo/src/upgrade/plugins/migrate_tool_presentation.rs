//! Plugin to migrate tool presentation configuration.
//!
//! This plugin moves the legacy `tool-presentation.toml` from
//! `docs/agent-context/` to `.config/exo/`.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use std::fs;

/// Migrates tool presentation configuration to the new location.
///
/// Moves `docs/agent-context/tool-presentation.toml` to `.config/exo/tool-presentation.toml`.
///
/// # Severity
///
/// **Info** - File location change for better organization.
#[derive(Debug, Clone, Copy)]
pub struct MigrateToolPresentationPlugin;

impl MigrateToolPresentationPlugin {
    const LEGACY_PATH: &'static str = "docs/agent-context/tool-presentation.toml";
    const NEW_PATH: &'static str = ".config/exo/tool-presentation.toml";
}

impl UpgradePlugin for MigrateToolPresentationPlugin {
    fn id(&self) -> &str {
        "migrate-tool-presentation-v1"
    }

    fn description(&self) -> &str {
        "Migrates tool-presentation.toml to .config/exo/"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let legacy = context.root.join(Self::LEGACY_PATH);
        let preferred = context.root.join(Self::NEW_PATH);

        if legacy.exists() && !preferred.exists() {
            Ok(UpgradeStatus::info(
                "Legacy tool-presentation.toml needs migration",
            ))
        } else {
            Ok(UpgradeStatus::NotNeeded)
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let legacy = context.root.join(Self::LEGACY_PATH);
        let preferred = context.root.join(Self::NEW_PATH);

        if !legacy.exists() || preferred.exists() {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        // Create parent directory
        if let Some(parent) = preferred.parent() {
            fs::create_dir_all(parent)?;
        }

        // Try rename first, fall back to copy+delete for cross-device moves
        match fs::rename(&legacy, &preferred) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(18) => {
                // EXDEV: cross-device link
                fs::copy(&legacy, &preferred)?;
                fs::remove_file(&legacy)?;
            }
            Err(e) => return Err(e.into()),
        }

        // Tool presentation is user-configurable; keep it writable
        crate::utils::ensure_writable(&preferred)?;

        Ok(UpgradeReport::with_changes(
            self.id(),
            vec![format!(
                "Migrated {} -> {}",
                Self::LEGACY_PATH,
                Self::NEW_PATH
            )],
        ))
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let legacy = context.root.join(Self::LEGACY_PATH);

        // Legacy file should not exist after migration
        if legacy.exists() && context.root.join(Self::NEW_PATH).exists() {
            anyhow::bail!("Verification failed: both legacy and new tool-presentation.toml exist")
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        // Create directory structure
        fs::create_dir_all(root.join("docs/agent-context")).unwrap();

        let context = AgentContext::new_for_testing(root);
        (temp_dir, context)
    }

    #[test]
    fn test_not_needed_when_no_legacy_file() {
        let (_temp, context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_not_needed_when_new_file_exists() {
        let (_temp, context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        // Create both files
        fs::create_dir_all(context.root.join(".config/exo")).unwrap();
        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();
        fs::write(
            context.root.join(".config/exo/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_needed_when_only_legacy_exists() {
        let (_temp, context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Info));
    }

    #[test]
    fn test_apply_migrates_file() {
        let (_temp, mut context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]\ntest = true",
        )
        .unwrap();

        let report = plugin.apply(&mut context).unwrap();

        assert!(report.applied);
        assert!(
            !context
                .root
                .join("docs/agent-context/tool-presentation.toml")
                .exists()
        );
        assert!(
            context
                .root
                .join(".config/exo/tool-presentation.toml")
                .exists()
        );

        // Verify content preserved
        let content =
            fs::read_to_string(context.root.join(".config/exo/tool-presentation.toml")).unwrap();
        assert!(content.contains("test = true"));
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();

        // Apply twice
        plugin.apply(&mut context).unwrap();
        let report = plugin.apply(&mut context).unwrap();

        // Second apply should report no changes
        assert!(!report.applied);
    }

    #[test]
    fn test_verify_passes_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();

        plugin.apply(&mut context).unwrap();
        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_is_needed_false_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = MigrateToolPresentationPlugin;

        // Create legacy file
        fs::write(
            context
                .root
                .join("docs/agent-context/tool-presentation.toml"),
            "[tools]",
        )
        .unwrap();

        // Initially needed
        assert!(plugin.is_needed(&context).unwrap().is_needed());

        // Apply migrates the file
        plugin.apply(&mut context).unwrap();

        // Now should not be needed
        assert!(!plugin.is_needed(&context).unwrap().is_needed());
    }
}
