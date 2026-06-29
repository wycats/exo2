//! Plugin to ensure .gitignore exists with exosuit patterns.
//!
//! This plugin checks for a .gitignore file and creates one with
//! standard exosuit patterns if missing. If a .gitignore already
//! exists, it ensures `.cache/` and `.runtime/` entries are present.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::templates;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};

/// Required entries that must be present in .gitignore.
const REQUIRED_ENTRIES: &[&str] = &[".cache/", ".runtime/"];

/// Ensures .gitignore exists with exosuit patterns.
///
/// - If no .gitignore exists, creates one from the template.
/// - If .gitignore exists but is missing `.cache/` or `.runtime/`,
///   appends them.
///
/// # Severity
///
/// **Info** - Missing or incomplete .gitignore is informational.
#[derive(Debug, Clone, Copy)]
pub struct EnsureGitignorePlugin;

impl UpgradePlugin for EnsureGitignorePlugin {
    fn id(&self) -> &str {
        "ensure-gitignore-v1"
    }

    fn description(&self) -> &str {
        "Ensures .gitignore exists with exosuit patterns"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let gitignore = context.root.join(".gitignore");
        if !gitignore.exists() {
            return Ok(UpgradeStatus::info("Missing .gitignore file"));
        }

        let gitignore_content = std::fs::read_to_string(&gitignore)?;
        let missing_entries: Vec<&&str> = REQUIRED_ENTRIES
            .iter()
            .filter(|entry| !gitignore_content.lines().any(|line| line.trim() == **entry))
            .collect();

        if missing_entries.is_empty() {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::info(format!(
                ".gitignore missing entries: {}",
                missing_entries
                    .iter()
                    .map(|entry| **entry)
                    .collect::<Vec<_>>()
                    .join(", ")
            )))
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let gitignore = context.root.join(".gitignore");

        if !gitignore.exists() {
            templates::install_gitignore(&context.root)?;
            return Ok(UpgradeReport::with_changes(
                self.id(),
                vec!["Created .gitignore with exosuit patterns".to_string()],
            ));
        }

        // Append missing entries to existing .gitignore
        let gitignore_content = std::fs::read_to_string(&gitignore)?;
        let missing_entries: Vec<&&str> = REQUIRED_ENTRIES
            .iter()
            .filter(|entry| !gitignore_content.lines().any(|line| line.trim() == **entry))
            .collect();

        if missing_entries.is_empty() {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        let mut new_content = gitignore_content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str("\n# Exosuit local state\n");
        for entry in &missing_entries {
            new_content.push_str(entry);
            new_content.push('\n');
        }

        std::fs::write(&gitignore, new_content)?;

        Ok(UpgradeReport::with_changes(
            self.id(),
            missing_entries
                .iter()
                .map(|entry| format!("Added {entry} to .gitignore"))
                .collect(),
        ))
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let gitignore = context.root.join(".gitignore");
        if gitignore.exists() {
            Ok(())
        } else {
            anyhow::bail!("Verification failed: .gitignore not found")
        }
    }
}
