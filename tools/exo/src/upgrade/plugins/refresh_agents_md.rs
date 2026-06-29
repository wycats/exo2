//! Plugin to refresh AGENTS.md from template.
//!
//! This plugin regenerates the AGENTS.md file using the latest template
//! while preserving the project's mission statement.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::templates;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use std::fs;

/// Refreshes the AGENTS.md file from the current template.
///
/// Preserves the project's mission statement while updating the rest of
/// the file to match the latest template.
///
/// # Severity
///
/// **Info** - Template updates are non-breaking documentation changes.
#[derive(Debug, Clone, Copy)]
pub struct RefreshAgentsMdPlugin;

impl RefreshAgentsMdPlugin {
    /// Extract the mission statement from existing AGENTS.md content.
    fn extract_mission(content: &str) -> String {
        content.find("# Project Mission").map_or_else(
            || "Unknown Mission".to_string(),
            |start| {
                let mission_part = &content[start..];
                // Remove the header line
                let lines: Vec<&str> = mission_part.lines().skip(1).collect();
                lines.join("\n").trim().to_string()
            },
        )
    }
}

impl UpgradePlugin for RefreshAgentsMdPlugin {
    fn id(&self) -> &str {
        "refresh-agents-md-v1"
    }

    fn description(&self) -> &str {
        "Refreshes AGENTS.md from the current template while preserving mission"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let agents_path = context.root.join("AGENTS.md");

        if agents_path.exists() {
            Ok(UpgradeStatus::info("AGENTS.md may need template refresh"))
        } else {
            Ok(UpgradeStatus::NotNeeded)
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let agents_path = context.root.join("AGENTS.md");

        if !agents_path.exists() {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        let file_content = fs::read_to_string(&agents_path)?;
        let mission = Self::extract_mission(&file_content);

        // Re-generate from template
        let new_content = templates::AGENTS_MD.replace("{{MISSION}}", &mission);

        // Only write if content changed
        if new_content == file_content {
            Ok(UpgradeReport::no_changes(self.id()))
        } else {
            fs::write(&agents_path, &new_content)?;
            Ok(UpgradeReport::with_changes(
                self.id(),
                vec!["Refreshed AGENTS.md from template".to_string()],
            ))
        }
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let agents_path = context.root.join("AGENTS.md");

        if agents_path.exists() {
            let file_content = fs::read_to_string(&agents_path)?;
            // Verify it contains expected template markers
            if file_content.contains("# Project Mission") {
                Ok(())
            } else {
                anyhow::bail!("AGENTS.md missing expected '# Project Mission' section")
            }
        } else {
            // If file doesn't exist, that's okay - we only update existing files
            Ok(())
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
    fn test_not_needed_when_no_agents_md() {
        let (_temp, context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        let status = plugin.is_needed(&context).unwrap();
        assert!(!status.is_needed());
    }

    #[test]
    fn test_needed_when_agents_md_exists() {
        let (_temp, context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        // Create AGENTS.md
        let agents_path = context.root.join("AGENTS.md");
        fs::write(&agents_path, "# Project Mission\nTest Mission").unwrap();

        let status = plugin.is_needed(&context).unwrap();
        assert!(status.is_needed());
        assert_eq!(status.severity(), Some(Severity::Info));
    }

    #[test]
    fn test_apply_preserves_mission() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        // Create AGENTS.md with a mission
        let agents_path = context.root.join("AGENTS.md");
        fs::write(&agents_path, "# Project Mission\nMy Special Mission").unwrap();

        plugin.apply(&mut context).unwrap();

        // Verify mission is preserved
        let content = fs::read_to_string(&agents_path).unwrap();
        assert!(content.contains("My Special Mission"));
    }

    #[test]
    fn test_apply_is_idempotent() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        // Create AGENTS.md
        let agents_path = context.root.join("AGENTS.md");
        fs::write(&agents_path, "# Project Mission\nTest Mission").unwrap();

        // Apply twice
        plugin.apply(&mut context).unwrap();
        let content_after_first = fs::read_to_string(&agents_path).unwrap();

        plugin.apply(&mut context).unwrap();
        let content_after_second = fs::read_to_string(&agents_path).unwrap();

        // Content should be identical
        assert_eq!(content_after_first, content_after_second);
    }

    #[test]
    fn test_verify_passes_with_valid_agents_md() {
        let (_temp, context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        // Create valid AGENTS.md
        let agents_path = context.root.join("AGENTS.md");
        fs::write(&agents_path, "# Project Mission\nTest").unwrap();

        assert!(plugin.verify(&context).is_ok());
    }

    #[test]
    fn test_extract_mission() {
        let content = "Some header\n# Project Mission\nThis is my mission\nMore text";
        let mission = RefreshAgentsMdPlugin::extract_mission(content);
        assert!(mission.contains("This is my mission"));
    }

    #[test]
    fn test_is_needed_false_after_apply() {
        let (_temp, mut context) = setup_test_context();
        let plugin = RefreshAgentsMdPlugin;

        // Create AGENTS.md to trigger the need
        let agents_path = context.root.join("AGENTS.md");
        fs::write(&agents_path, "# Project Mission\nTest Mission").unwrap();

        // Apply the plugin
        plugin.apply(&mut context).unwrap();

        // After apply, content is from template so second apply should report no changes
        let report = plugin.apply(&mut context).unwrap();
        assert!(!report.applied, "Second apply should report no changes");
    }
}
