use crate::ExoResult;
use crate::context::AgentContext;
use crate::steering;
use crate::upgrade::UpgradeRegistry;
use crate::world_state::WorldState;
use serde::Serialize;
use std::path::Path;

pub fn build_map_json(
    context: &AgentContext,
    next: bool,
    why: Option<&str>,
    agent_id: Option<&str>,
) -> ExoResult<serde_json::Value> {
    // Check for critical upgrades first - they take absolute priority
    let registry = UpgradeRegistry::new();
    let upgrade_check = registry.check_all(context)?;

    if upgrade_check.has_blocking() {
        let steering = steering::upgrade_required_steering(&upgrade_check.critical);
        return Ok(serde_json::to_value(&steering)?);
    }

    let world = WorldState::probe(context)?;

    let steering = steering::derive_world_steering(&world, agent_id);

    // 2. Header (and early repair suggestion if we're not in a phase)
    if world.active_phase.is_none() {
        return Ok(serde_json::to_value(&steering)?);
    }

    if next {
        let prefer_repair = steering::world_needs_repair(&world);
        let action = if prefer_repair {
            steering
                .repair_actions
                .first()
                .or_else(|| steering.next_actions.first())
        } else {
            steering
                .next_actions
                .first()
                .or_else(|| steering.repair_actions.first())
        };

        return Ok(serde_json::to_value(action)?);
    }

    if let Some(why_cmd) = why {
        #[derive(Serialize)]
        struct WhyOutput {
            command: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            suggested: Option<crate::steering::SuggestedAction>,
            preconditions: Vec<String>,
            effects: Vec<String>,
        }

        let why_cmd = why_cmd.trim().to_string();
        let suggested = steering
            .next_actions
            .iter()
            .chain(steering.repair_actions.iter())
            .find(|a| a.command == why_cmd)
            .cloned();

        let (preconditions, effects) = explain_command(&why_cmd);
        let out = WhyOutput {
            command: why_cmd,
            suggested,
            preconditions,
            effects,
        };

        return Ok(serde_json::to_value(&out)?);
    }

    Ok(serde_json::to_value(&steering)?)
}

pub fn show_map_human(
    context: &AgentContext,
    next: bool,
    why: Option<&str>,
    agent_id: Option<&str>,
) -> ExoResult<()> {
    let root = &context.root;

    // Check for critical upgrades first - they take absolute priority
    let registry = UpgradeRegistry::new();
    let upgrade_check = registry.check_all(context)?;

    if upgrade_check.has_blocking() {
        println!("# Critical Upgrades Required");
        println!();
        for upgrade in &upgrade_check.critical {
            println!("⚠️  {}", upgrade.reason);
        }
        println!();
        println!(
            "Run `exo update` to apply {} critical upgrade(s).",
            upgrade_check.critical.len()
        );
        println!();
        println!("All operations are blocked until upgrades are applied.");
        return Ok(());
    }

    let world = WorldState::probe(context)?;
    let steering = steering::derive_world_steering(&world, agent_id);

    if world.active_phase.is_none() {
        println!("# No Active Phase");
        println!("Primary intent: {}", steering.primary_intent.as_str());
        println!("\n[Next]");
        for a in &steering.next_actions {
            println!("- {}: {}", a.label, a.command);
            println!("  {}", a.rationale);
        }
        println!("\n[Repair]");
        for a in &steering.repair_actions {
            println!("- {}: {}", a.label, a.command);
            println!("  {}", a.rationale);
        }
        return Ok(());
    }

    let tasks = &world.tasks;
    let goals = &world.goals;
    let Some(phase) = &world.active_phase else {
        return Ok(());
    };

    println!("# Phase {}: {} [Active]", phase.id, phase.title);
    println!("Epoch: {}", phase.epoch_title);

    println!("----------------------------------------------------------------");

    println!("## Active Tasks");
    if tasks.is_empty() {
        println!("(No tasks defined)");
    } else {
        for (_id, label, status) in tasks {
            let icon = match status.as_str() {
                "completed" => "[x]",
                "in-progress" => "[/]",
                _ => "[ ]",
            };
            println!("{icon} {label} ({status})");
        }
    }
    println!();

    // Goals section - shows goals from SQLite
    println!("## Goals");
    if goals.is_empty() {
        println!("(No goals defined)");
    } else {
        for (i, goal) in goals.iter().enumerate() {
            let status = goal.status.as_str();
            let title = if goal.label.is_empty() {
                goal.id.as_str()
            } else {
                goal.label.as_str()
            };

            let icon = match status {
                "completed" => "✅",
                _ => "⚪",
            };

            println!("{}. {} {}", i + 1, icon, title);
        }
    }
    println!();

    println!("## Context Health");
    check_file(root, crate::context::SQLITE_DB_PATH);
    println!();

    println!("----------------------------------------------------------------");

    if next {
        let prefer_repair = steering::world_needs_repair(&world);
        let action = if prefer_repair {
            steering
                .repair_actions
                .first()
                .or_else(|| steering.next_actions.first())
        } else {
            steering
                .next_actions
                .first()
                .or_else(|| steering.repair_actions.first())
        };

        if let Some(a) = action {
            println!("# Next Action");
            println!("{}", a.command);
            println!("\n{}", a.rationale);
        } else {
            println!("(No suggested actions)");
        }

        return Ok(());
    }

    if let Some(why_cmd) = why {
        #[derive(Serialize)]
        struct WhyOutput {
            command: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            suggested: Option<crate::steering::SuggestedAction>,
            preconditions: Vec<String>,
            effects: Vec<String>,
        }

        let why_cmd = why_cmd.trim().to_string();
        let suggested = steering
            .next_actions
            .iter()
            .chain(steering.repair_actions.iter())
            .find(|a| a.command == why_cmd)
            .cloned();

        let (preconditions, effects) = explain_command(&why_cmd);
        let out = WhyOutput {
            command: why_cmd,
            suggested,
            preconditions,
            effects,
        };

        println!("# Why: {}", out.command);
        if let Some(s) = &out.suggested {
            println!("\nRationale: {}", s.rationale);
        }
        if !out.preconditions.is_empty() {
            println!("\n[Preconditions]");
            for p in &out.preconditions {
                println!("- {p}");
            }
        }
        if !out.effects.is_empty() {
            println!("\n[Effects]");
            for e in &out.effects {
                println!("- {e}");
            }
        }

        return Ok(());
    }

    println!("Primary intent: {}", steering.primary_intent.as_str());
    println!("\n[Next]");
    for a in steering.next_actions.iter().take(4) {
        println!("- {}: {}", a.label, a.command);
        println!("  {}", a.rationale);
    }
    if !steering.repair_actions.is_empty() {
        println!("\n[Repair]");
        for a in steering.repair_actions.iter().take(4) {
            println!("- {}: {}", a.label, a.command);
            println!("  {}", a.rationale);
        }
    }

    Ok(())
}

fn check_file(root: &Path, path: &str) {
    let full_path = root.join(path);
    let filename = full_path.file_name().unwrap_or_default().to_string_lossy();
    if full_path.exists() {
        println!("[OK] {filename}");
    } else {
        println!("[!!] {filename} (Missing)");
    }
}

fn explain_command(command: &str) -> (Vec<String>, Vec<String>) {
    let mut preconditions = Vec::new();
    let mut effects = Vec::new();

    if command.starts_with("exo phase start") {
        preconditions.push("Phase id exists in SQLite state".to_string());
        effects.push("Marks the phase as active in SQLite state".to_string());
        effects.push("Initializes active phase state in the daemon-backed workspace".to_string());
    } else if command == "exo phase status" {
        effects.push(
            "Reads SQLite state and daemon-backed context to present current state".to_string(),
        );
    } else if command == "exo phase finish" {
        preconditions.push("An active phase exists".to_string());
        effects.push("Marks the active phase as completed in SQLite state".to_string());
        effects.push("Optionally commits changes if invoked with --message".to_string());
    } else if command == "exo update" {
        effects.push("Refreshes project scaffolding (including .github/prompts)".to_string());
        effects.push(
            "Ensures SQLite-backed workspace scaffolding is initialized when possible".to_string(),
        );
    } else if command.starts_with("exo task complete") {
        preconditions.push("An active phase exists".to_string());
        preconditions.push("Task id exists in the active phase".to_string());
        effects.push("Updates task status in SQLite state".to_string());
    }

    (preconditions, effects)
}
