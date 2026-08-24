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
        Severity::Critical
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        if context
            .project
            .as_ref()
            .is_some_and(|project| project.policy != crate::project::StatePolicy::Repo)
        {
            return Ok(UpgradeStatus::NotNeeded);
        }
        let gitattributes = context.root.join(".gitattributes");
        if !gitattributes.exists() {
            return Ok(UpgradeStatus::critical("Missing .gitattributes file"));
        }

        let content = std::fs::read_to_string(&gitattributes)?;
        if !has_sql_dump_attribute(&content) {
            return Ok(UpgradeStatus::critical(
                ".gitattributes missing generated SQL dump merge policy",
            ));
        }

        if !templates::sql_dump_merge_driver_configured(&context.root)? {
            return Ok(UpgradeStatus::critical(
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
        if !has_sql_dump_attribute(&content) {
            anyhow::bail!("Verification failed: .gitattributes missing SQL dump merge attribute")
        }
        if !templates::sql_dump_merge_driver_configured(&context.root)? {
            anyhow::bail!("Verification failed: SQL dump merge driver is not configured")
        }
        Ok(())
    }
}

fn has_sql_dump_attribute(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim() == templates::SQL_DUMP_MERGE_ATTRIBUTE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_spawn::CommandSpawnExt as _;

    fn context_with_git_repo() -> (tempfile::TempDir, AgentContext) {
        let temp = tempfile::tempdir().expect("tempdir");
        let output = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(temp.path())
            .output_guarded()
            .expect("initialize git repository");
        assert!(output.status.success());
        let context = AgentContext::new_for_testing(temp.path().to_path_buf());
        (temp, context)
    }

    #[test]
    fn missing_merge_driver_is_a_blocking_upgrade() {
        let (_temp, context) = context_with_git_repo();
        std::fs::write(
            context.root.join(".gitattributes"),
            format!("{}\n", templates::SQL_DUMP_MERGE_ATTRIBUTE),
        )
        .expect("write attributes");

        let plugin = EnsureGitattributesPlugin;
        let status = plugin.is_needed(&context).expect("check upgrade");
        assert_eq!(status.severity(), Some(Severity::Critical));
    }

    #[test]
    fn apply_installs_and_verifies_the_merge_driver() {
        let (_temp, mut context) = context_with_git_repo();
        let plugin = EnsureGitattributesPlugin;

        plugin.apply(&mut context).expect("apply upgrade");
        plugin.verify(&context).expect("verify upgrade");
        assert_eq!(
            plugin.is_needed(&context).expect("recheck upgrade"),
            UpgradeStatus::NotNeeded
        );
    }
}
