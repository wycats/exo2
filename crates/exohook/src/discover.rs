use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use std::io::{self, Write};

use crate::config::{
    CheckCategory, CheckRefV3, ConfigV3, ConfigVersion, hooks_config_path, read_hooks_doc,
};
use crate::jsonl::DiscoveryItem;
use crate::migration::human_label;

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub(crate) enum DiscoverFormat {
    Human,
    Jsonl,
}

#[derive(Clone, Debug)]
struct CheckPlan {
    id: String,
    label: String,
    command: String,
    lane: String,
    category: CheckCategory,
    filters: Vec<String>,
}

#[derive(Clone, Debug)]
struct SuitePlan {
    id: String,
    label: String,
    checks: Vec<CheckPlan>,
    aliases: Vec<String>,
}

pub(crate) fn discover(format: DiscoverFormat, lane: Option<&str>) -> Result<()> {
    let config_path = hooks_config_path()?;
    if !config_path.exists() {
        return Err(anyhow!(
            "hooks config not found at {}",
            config_path.display()
        ));
    }

    let doc = read_hooks_doc(&config_path)?;
    let version = ConfigVersion::from_doc(&doc);
    if version != ConfigVersion::V3 {
        return Err(anyhow!(
            "discover requires hooks.toml version 3 (found {:?})",
            version
        ));
    }

    let config = ConfigV3::from_doc(&doc)?;
    config.validate()?;

    let mut suites = collect_suites(&config)?;
    if let Some(lane) = lane {
        suites = filter_suites(&suites, lane)?;
    }

    match format {
        DiscoverFormat::Human => emit_human(&suites),
        DiscoverFormat::Jsonl => emit_jsonl(&suites)?,
    }

    Ok(())
}

fn collect_suites(config: &ConfigV3) -> Result<Vec<SuitePlan>> {
    let mut suites = Vec::new();

    // Collect workflow lanes first — these are the user-facing interactive lanes.
    for (workflow_id, workflow) in &config.workflow {
        let checks = collect_checks(workflow_id, &workflow.checks, config)?;
        let label = workflow
            .label
            .clone()
            .unwrap_or_else(|| human_label(workflow_id));
        let mut aliases = vec![workflow_id.to_string()];
        let normalized = workflow_id.replace('_', "-");
        if normalized != *workflow_id {
            aliases.push(normalized);
        }
        suites.push(SuitePlan {
            id: workflow_id.to_string(),
            label,
            checks,
            aliases,
        });
    }

    Ok(suites)
}

fn collect_checks(
    suite_id: &str,
    refs: &[CheckRefV3],
    config: &ConfigV3,
) -> Result<Vec<CheckPlan>> {
    let mut checks = Vec::new();

    for (idx, check_ref) in refs.iter().enumerate() {
        let (check_id, check) = match check_ref {
            CheckRefV3::Ref(name) => {
                let check = config
                    .check
                    .get(name)
                    .ok_or_else(|| anyhow!("unknown check '{name}'"))?;
                (name.clone(), check)
            }
            CheckRefV3::Inline(check) => (format!("{suite_id}-inline-{idx}"), check),
        };

        let label = check.label.as_deref().unwrap_or(&check_id).to_string();
        let command = check
            .command
            .clone()
            .or_else(|| check.tool.clone())
            .ok_or_else(|| anyhow!("check '{check_id}' has no command or tool"))?;

        checks.push(CheckPlan {
            id: check_id,
            label,
            command,
            lane: suite_id.to_string(),
            category: check.category,
            filters: check.filters.clone(),
        });
    }

    Ok(checks)
}

fn filter_suites(suites: &[SuitePlan], lane: &str) -> Result<Vec<SuitePlan>> {
    let normalized = lane.replace('_', "-");
    let filtered: Vec<SuitePlan> = suites
        .iter()
        .filter(|suite| {
            suite
                .aliases
                .iter()
                .any(|alias| alias == lane || alias == &normalized)
        })
        .cloned()
        .collect();

    if filtered.is_empty() {
        let available: Vec<String> = suites.iter().map(|suite| suite.id.clone()).collect();
        if available.is_empty() {
            return Err(anyhow!("no hooks or workflows configured"));
        }
        return Err(anyhow!(
            "lane '{}' not found. Available: {}",
            lane,
            available.join(", ")
        ));
    }

    Ok(filtered)
}

fn emit_jsonl(suites: &[SuitePlan]) -> Result<()> {
    for suite in suites {
        emit_item(&DiscoveryItem::Suite {
            id: suite.id.clone(),
            label: suite.label.clone(),
            checks: suite.checks.iter().map(|check| check.id.clone()).collect(),
        })
        .context("failed to emit suite")?;

        for check in &suite.checks {
            emit_item(&DiscoveryItem::Check {
                id: check.id.clone(),
                label: check.label.clone(),
                command: check.command.clone(),
                lane: check.lane.clone(),
                category: match check.category {
                    CheckCategory::Observe => "observe".to_string(),
                    CheckCategory::Mutate => "mutate".to_string(),
                },
                filters: check.filters.clone(),
            })
            .context("failed to emit check")?;
        }
    }

    Ok(())
}

fn emit_item(item: &DiscoveryItem) -> Result<()> {
    let json = serde_json::to_string(item)?;
    let mut stdout = io::stdout();
    writeln!(stdout, "{json}")?;
    Ok(())
}

fn emit_human(suites: &[SuitePlan]) {
    for (idx, suite) in suites.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        println!("{}: {}", suite.id, suite.label);
        for check in &suite.checks {
            if check.label == check.id {
                println!("  - {}: {}", check.id, check.command);
            } else {
                println!("  - {} ({}): {}", check.label, check.id, check.command);
            }
        }
    }
}
