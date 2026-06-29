//! Plugin to ensure generated SQL dumps have a real merge driver.

use crate::ExoResult;
use crate::context::AgentContext;
use crate::templates;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};

#[derive(Debug, Clone, Copy)]
pub struct EnsureGitattributesPlugin;

impl UpgradePlugin for EnsureGitattributesPlugin {
    fn id(&self) -> &str {
        "ensure-gitattributes-v1"
    }

    fn description(&self) -> &str {
        "Ensures .gitattributes and SQL dump merge driver are configured"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let gitattributes = context.root.join(".gitattributes");
        if !gitattributes.exists() {
            return Ok(UpgradeStatus::info("Missing .gitattributes file"));
        }

        let content = std::fs::read_to_string(&gitattributes)?;
        if !has_sql_dump_attribute(&content) {
            return Ok(UpgradeStatus::info(
                ".gitattributes missing generated SQL dump merge policy",
            ));
        }

        if !templates::sql_dump_merge_driver_configured(&context.root)? {
            return Ok(UpgradeStatus::info(
                "SQL dump merge driver is not configured in repo-local git config",
            ));
        }

        Ok(UpgradeStatus::NotNeeded)
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let gitattributes = context.root.join(".gitattributes");
        let mut changes = Vec::new();

        if !gitattributes.exists() {
            templates::install_gitattributes(&context.root)?;
            changes.push("Created .gitattributes with SQL dump merge attributes".to_string());
        } else {
            let mut content = std::fs::read_to_string(&gitattributes)?;
            if !has_sql_dump_attribute(&content) {
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str("\n# Exo generated SQL projections\n");
                content.push_str(templates::SQL_DUMP_MERGE_ATTRIBUTE);
                content.push('\n');
                std::fs::write(&gitattributes, content)?;
                changes.push("Added SQL dump merge attribute to .gitattributes".to_string());
            }
        }

        if templates::configure_sql_dump_merge_driver(&context.root)? {
            changes.push("Configured repo-local SQL dump merge driver".to_string());
        }

        if changes.is_empty() {
            Ok(UpgradeReport::no_changes(self.id()))
        } else {
            Ok(UpgradeReport::with_changes(self.id(), changes))
        }
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let gitattributes = context.root.join(".gitattributes");
        let content = std::fs::read_to_string(&gitattributes)?;
        if has_sql_dump_attribute(&content) {
            Ok(())
        } else {
            anyhow::bail!("Verification failed: .gitattributes missing SQL dump merge attribute")
        }
    }
}

fn has_sql_dump_attribute(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim() == templates::SQL_DUMP_MERGE_ATTRIBUTE)
}
