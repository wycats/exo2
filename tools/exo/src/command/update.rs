//! The `exo update` command implementation.
//!
//! This module applies all upgrade plugins to bring the project up to date.
//! The actual upgrade logic is implemented in `crate::upgrade::plugins`.

use super::traits::{
    Command, CommandContext, CommandOutput, MutableCommand, MutableCommandContext, OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::AgentContext;
use crate::project::Project;
use crate::upgrade::{UpgradeRegistry, UpgradeSummary};
use anyhow::{Context as _, Result as ExoResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Root command: `exo update`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateCommand;

impl UpdateCommand {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateAppliedReport {
    plugin_id: String,
    changes: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateHumanOutput {
    applied_count: usize,
    skipped_count: usize,
    applied: Vec<UpdateAppliedReport>,
}

#[derive(Debug, Serialize)]
struct UpdateOutput {
    kind: &'static str,
    ok: bool,
    applied_count: usize,
    skipped_count: usize,
    applied: Vec<UpdateAppliedReport>,
    reports: Vec<crate::upgrade::UpgradeReport>,
}

impl UpdateOutput {
    fn from_summary(summary: &UpgradeSummary) -> Self {
        let applied = summary
            .reports
            .iter()
            .filter(|report| report.applied)
            .map(|report| UpdateAppliedReport {
                plugin_id: report.plugin_id.clone(),
                changes: report.changes.clone(),
                warnings: report.warnings.clone(),
            })
            .collect();

        Self {
            kind: "update",
            ok: true,
            applied_count: summary.applied_count,
            skipped_count: summary.skipped_count,
            applied,
            reports: summary.reports.clone(),
        }
    }
}

fn format_update_human_output(
    root: &Path,
    applied_count: usize,
    skipped_count: usize,
    applied: &[UpdateAppliedReport],
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Updating Exosuit project in {}", root.display()));

    if applied_count > 0 {
        lines.push(String::new());
        lines.push(format!("Applied {applied_count} upgrade(s):"));
        for report in applied {
            lines.push(format!("  ✓ {}", report.plugin_id));
            for change in &report.changes {
                lines.push(format!("    - {change}"));
            }
            for warning in &report.warnings {
                lines.push(format!("    ⚠ {warning}"));
            }
        }
    }

    if skipped_count > 0 && applied_count > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Skipped {skipped_count} already up-to-date upgrade(s)"
        ));
    }

    lines.push(String::new());
    lines.push("Project updated successfully!".to_string());
    lines.join("\n")
}

pub(crate) fn format_update_human_data(root: &Path, data: &serde_json::Value) -> Option<String> {
    let output = serde_json::from_value::<UpdateHumanOutput>(data.clone()).ok()?;
    Some(format_update_human_output(
        root,
        output.applied_count,
        output.skipped_count,
        &output.applied,
    ))
}

fn apply_upgrades(context: &mut AgentContext) -> ExoResult<UpgradeSummary> {
    let registry = UpgradeRegistry::new();
    registry.apply_all(context)
}

fn has_sql_projection_files(root: &Path, project: Option<&Project>) -> bool {
    crate::context::sql_projection_dir(root, project).is_some_and(|sql_dir| {
        exosuit_storage::TABLE_ORDER
            .iter()
            .any(|(file_stem, _)| sql_dir.join(format!("{file_stem}.sql")).exists())
    })
}

/// Return whether the workspace has enough existing Exo state to be updated.
#[must_use]
pub fn is_update_workspace(root: &Path, project: Option<&Project>) -> bool {
    let db_path = crate::context::db_path(root, project);
    db_path.exists()
        || root.join("exosuit.toml").exists()
        || has_sql_projection_files(root, project)
}

fn ensure_update_workspace(root: &Path, project: Option<&Project>) -> ExoResult<()> {
    if is_update_workspace(root, project) {
        return Ok(());
    }

    anyhow::bail!(
        "Failed to update Exosuit project: no exosuit.toml, SQLite database, or SQL projection files found at {}\n\nRun 'exo init' to initialize a new workspace.",
        root.display()
    );
}

fn ensure_update_database(root: &Path, project: Option<&Project>) -> ExoResult<()> {
    ensure_update_workspace(root, project)?;

    let db_path = crate::context::db_path(root, project);
    if !db_path.exists()
        && let Some(sql_dir) = crate::context::sql_projection_dir(root, project)
        && has_sql_projection_files(root, project)
    {
        crate::context::import_sql_dumps(&sql_dir, &db_path)?;
        return Ok(());
    }

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create SQLite database directory {}",
                parent.display()
            )
        })?;
    }
    let _db = exosuit_storage::open_database(&db_path)
        .with_context(|| format!("Failed to create SQLite database at {}", db_path.display()))?;
    Ok(())
}

fn resolve_update_project(root: &Path, project: Option<&Project>) -> Option<Project> {
    project.cloned().or_else(|| Project::resolve(root).ok())
}

fn load_update_context(root: PathBuf, project: Option<&Project>) -> ExoResult<AgentContext> {
    let project = resolve_update_project(&root, project);
    ensure_update_database(&root, project.as_ref())?;
    Ok(AgentContext {
        root,
        project,
        plan: crate::context::ExoState::default(),
    })
}

fn reload_after_upgrade(context: &mut AgentContext) -> ExoResult<()> {
    let loaded = AgentContext::load(context.root.clone())
        .with_context(|| "Failed to load agent context after applying upgrades")?;
    *context = loaded;
    Ok(())
}

impl Command for UpdateCommand {
    fn namespace(&self) -> &'static str {
        ""
    }

    fn operation(&self) -> &'static str {
        "update"
    }

    fn description(&self) -> &'static str {
        "Apply all project upgrades"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("UpdateCommand should be dispatched via execute_mut")
    }
}

impl MutableCommand for UpdateCommand {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let mut agent_ctx = load_update_context(ctx.root.to_path_buf(), ctx.project)?;
        let summary = apply_upgrades(&mut agent_ctx)?;
        reload_after_upgrade(&mut agent_ctx)?;
        let output = UpdateOutput::from_summary(&summary);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format_update_human_output(
                    ctx.root,
                    output.applied_count,
                    output.skipped_count,
                    &output.applied,
                );
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

/// Run all project upgrades via the plugin registry.
///
/// This replaces the previous sequential update steps with a unified
/// plugin-based system. Each plugin is idempotent and self-contained.
pub fn run_update(context: &mut AgentContext) -> Result<(), Box<dyn std::error::Error>> {
    if context.project.is_none() {
        context.project = Project::resolve(&context.root).ok();
    }
    ensure_update_database(&context.root, context.project.as_ref())?;
    let summary = apply_upgrades(context)?;
    reload_after_upgrade(context)?;
    let output = UpdateOutput::from_summary(&summary);
    println!(
        "{}",
        format_update_human_output(
            &context.root,
            output.applied_count,
            output.skipped_count,
            &output.applied,
        )
    );
    Ok(())
}
