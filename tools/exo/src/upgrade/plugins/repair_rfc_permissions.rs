//! Plugin to repair RFC file permissions.
//!
//! This plugin ensures RFC files are user-writable so they can be edited
//! by VS Code and agents without permission issues.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use walkdir::WalkDir;

/// Repairs RFC file permissions to ensure they are user-writable.
///
/// RFCs are intentionally kept writable (unlike other CLI-managed files)
/// because they need to be editable by VS Code and agents. Correctness
/// is enforced at the verification/CI boundary rather than file permissions.
///
/// # Severity
///
/// **Info** - Permission repair is a convenience fix.
#[derive(Debug, Clone, Copy)]
pub struct RepairRfcPermissionsPlugin;

impl RepairRfcPermissionsPlugin {
    const RFCS_DIR: &'static str = "docs/rfcs";

    /// Files that should be skipped (not actual RFCs).
    const SKIP_FILES: &'static [&'static str] = &["README.md", "0000-template.md"];

    /// Check if a file lacks user-write permission.
    #[cfg(unix)]
    fn is_not_user_writable(path: &std::path::Path) -> std::io::Result<bool> {
        let metadata = std::fs::metadata(path)?;
        let mode = metadata.permissions().mode();
        // Check if user-write bit is NOT set
        Ok(mode & 0o200 == 0)
    }

    #[cfg(not(unix))]
    fn is_not_user_writable(path: &std::path::Path) -> std::io::Result<bool> {
        Ok(std::fs::metadata(path)?.permissions().readonly())
    }

    /// Find RFC files that are not user-writable.
    fn find_readonly_rfcs(&self, context: &AgentContext) -> Vec<std::path::PathBuf> {
        let rfcs_root = context.root.join(Self::RFCS_DIR);

        if !rfcs_root.exists() {
            return Vec::new();
        }

        WalkDir::new(&rfcs_root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("md"))
            .filter(|entry| {
                // Skip known non-RFC files
                !matches!(
                    entry.file_name().to_str(),
                    Some(name) if Self::SKIP_FILES.contains(&name)
                )
            })
            .filter(|entry| Self::is_not_user_writable(entry.path()).unwrap_or(false))
            .map(|entry| entry.path().to_path_buf())
            .collect()
    }
}

impl UpgradePlugin for RepairRfcPermissionsPlugin {
    fn id(&self) -> &str {
        "repair-rfc-permissions-v1"
    }

    fn description(&self) -> &str {
        "Ensures RFC files are user-writable"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let readonly_rfcs = self.find_readonly_rfcs(context);

        if readonly_rfcs.is_empty() {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::info(format!(
                "{} RFC file(s) are not user-writable",
                readonly_rfcs.len()
            )))
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let readonly_rfcs = self.find_readonly_rfcs(context);

        if readonly_rfcs.is_empty() {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        let mut changes = Vec::new();

        for path in readonly_rfcs {
            crate::utils::ensure_writable(&path)?;

            let rel_path = path
                .strip_prefix(&context.root)
                .unwrap_or(&path)
                .display()
                .to_string();
            changes.push(format!("Made writable: {rel_path}"));
        }

        Ok(UpgradeReport::with_changes(self.id(), changes))
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let readonly_rfcs = self.find_readonly_rfcs(context);

        if readonly_rfcs.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Verification failed: {} RFC file(s) are still not user-writable",
                readonly_rfcs.len()
            )
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, AgentContext) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();

        // Create directory structure
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();

        let context = AgentContext::new_for_testing(root);
        (temp_dir, context)
    }

    #[test]
    fn test_not_needed_when_no_rfcs_dir() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        let context = AgentContext::new_for_testing(root);

        let plugin = RepairRfcPermissionsPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_not_needed_when_all_writable() {
        let (_temp, context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create writable RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_needed_when_rfc_is_readonly() {
        let (_temp, context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();
        let mut perms = fs::metadata(&rfc_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&rfc_path, perms).unwrap();

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Info));
    }

    #[test]
    fn test_apply_makes_rfcs_writable() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();
        let mut perms = fs::metadata(&rfc_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&rfc_path, perms).unwrap();

        let report = plugin.apply(&mut context).unwrap();

        assert!(report.applied);
        assert_eq!(report.changes.len(), 1);

        // Verify file is now writable
        let mode = fs::metadata(&rfc_path).unwrap().permissions().mode();
        assert_ne!(mode & 0o200, 0, "RFC should be user-writable");
    }

    #[test]
    fn test_apply_skips_readme_and_template() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only README.md and template
        for name in &["README.md", "0000-template.md"] {
            let path = context.root.join("docs/rfcs").join(name);
            fs::write(&path, "# Not an RFC").unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o444);
            fs::set_permissions(&path, perms).unwrap();
        }

        let report = plugin.apply(&mut context).unwrap();

        // Should not make any changes to these files
        assert!(!report.applied);
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();
        let mut perms = fs::metadata(&rfc_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&rfc_path, perms).unwrap();

        // Apply twice
        plugin.apply(&mut context).unwrap();
        let report = plugin.apply(&mut context).unwrap();

        // Second apply should report no changes
        assert!(!report.applied);
    }

    #[test]
    fn test_verify_passes_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();
        let mut perms = fs::metadata(&rfc_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&rfc_path, perms).unwrap();

        plugin.apply(&mut context).unwrap();
        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_is_needed_false_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RepairRfcPermissionsPlugin;

        // Create read-only RFC file
        let rfc_path = context.root.join("docs/rfcs/stage-1/test-rfc.md");
        fs::write(&rfc_path, "# RFC").unwrap();
        let mut perms = fs::metadata(&rfc_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&rfc_path, perms).unwrap();

        // Initially needed
        assert!(plugin.is_needed(&context).unwrap().is_needed());

        // Apply makes file writable
        plugin.apply(&mut context).unwrap();

        // Now should not be needed
        assert!(!plugin.is_needed(&context).unwrap().is_needed());
    }
}
