use anyhow::{Result, anyhow};
use std::io::IsTerminal;
use toml_edit::Item;

use crate::ColorMode;
use crate::config::{hooks_config_path, iter_lanes, read_hooks_doc};
use crate::migration::human_label;

/// Metadata about a lane for display purposes.
#[derive(Debug)]
pub(crate) struct LaneDisplayInfo {
    id: String,
    label: String,
    description: String,
    parallel: bool,
    checks: Vec<String>,
    git_hook: Option<String>,
}

/// Show a rich, colorful listing of available validation lanes.
///
/// If `invalid_lane` is Some, this is being called because the user provided an invalid lane name.
pub(crate) fn show_lane_listing(color: ColorMode, invalid_lane: Option<&str>) -> Result<()> {
    use comfy_table::modifiers::UTF8_ROUND_CORNERS;
    use comfy_table::presets::UTF8_FULL;
    use comfy_table::{Cell, Color, ContentArrangement, Table};
    use console::Style;

    let use_color = colors_enabled(color);

    // Styles for non-table output
    let error_style = if use_color {
        Style::new().red().bold()
    } else {
        Style::new()
    };
    let dim_style = if use_color {
        Style::new().dim()
    } else {
        Style::new()
    };
    let green_style = if use_color {
        Style::new().green()
    } else {
        Style::new()
    };

    // Print error header
    if let Some(invalid) = invalid_lane {
        eprintln!(
            "{} unknown lane '{}'",
            error_style.apply_to("error:"),
            invalid
        );
    } else {
        eprintln!("{} no lane specified", error_style.apply_to("error:"));
    }
    eprintln!();

    // Print explanation
    eprintln!(
        "{}",
        dim_style.apply_to(
            "A \"lane\" is a predefined validation context—it determines which checks run"
        )
    );
    eprintln!(
        "{}",
        dim_style
            .apply_to("and which files they examine. Choose the lane that matches your workflow.")
    );
    eprintln!();

    // Load lane info from config
    let lanes = load_lane_display_info()?;

    if lanes.is_empty() {
        eprintln!(
            "{}",
            dim_style.apply_to("No lanes configured. Create .config/exo/hooks.toml or run:")
        );
        eprintln!("   {}", green_style.apply_to("exohook config init"));
        return Err(anyhow!("no lane specified"));
    }

    // Build table with comfy-table
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::DynamicFullWidth)
        .set_header(vec![
            if use_color {
                Cell::new("Lane").fg(Color::Cyan)
            } else {
                Cell::new("Lane")
            },
            Cell::new("Description"),
            if use_color {
                Cell::new("Hook").fg(Color::Yellow)
            } else {
                Cell::new("Hook")
            },
        ]);

    for lane in &lanes {
        // Build description with label + check info
        let check_count = lane.checks.len();
        let parallel_note = if lane.parallel { " (parallel)" } else { "" };
        let check_info = format!(
            "{} check{}{}",
            check_count,
            if check_count == 1 { "" } else { "s" },
            parallel_note
        );

        let description = if lane.description.is_empty() {
            format!("{}\n{}", lane.label, check_info)
        } else {
            format!("{}\n{}\n{}", lane.label, lane.description, check_info)
        };

        let hook = lane.git_hook.as_deref().unwrap_or("-");

        let lane_cell = if use_color {
            Cell::new(&lane.id).fg(Color::Cyan)
        } else {
            Cell::new(&lane.id)
        };

        let hook_cell = if use_color {
            Cell::new(hook).fg(Color::Yellow)
        } else {
            Cell::new(hook)
        };

        table.add_row(vec![lane_cell, Cell::new(description), hook_cell]);
    }

    eprintln!("{table}");
    eprintln!();

    // Quick start section
    eprintln!("{} Quick start:", dim_style.apply_to("💡"));

    // Show example commands based on available lanes
    let lane_ids: Vec<&str> = lanes.iter().map(|l| l.id.as_str()).collect();

    if lane_ids.contains(&"dev") {
        eprintln!(
            "   {}        {}",
            green_style.apply_to("exohook validate dev"),
            dim_style.apply_to("# Check your current work")
        );
    }
    if lane_ids.contains(&"coherence") {
        eprintln!(
            "   {}  {}",
            green_style.apply_to("exohook validate coherence"),
            dim_style.apply_to("# Run what pre-commit runs")
        );
    }
    if lane_ids.contains(&"gate") {
        eprintln!(
            "   {}       {}",
            green_style.apply_to("exohook validate gate"),
            dim_style.apply_to("# Run what pre-push runs")
        );
    }

    // If none of the common lanes exist, show the first available
    if !lane_ids.contains(&"dev")
        && !lane_ids.contains(&"coherence")
        && !lane_ids.contains(&"gate")
        && let Some(first) = lanes.first()
    {
        eprintln!(
            "   {}",
            green_style.apply_to(format!("exohook validate {}", first.id))
        );
    }

    if let Some(invalid) = invalid_lane {
        Err(anyhow!("unknown lane '{}'", invalid))
    } else {
        Err(anyhow!("no lane specified"))
    }
}

/// Load lane display info from hooks.toml.
pub(crate) fn load_lane_display_info() -> Result<Vec<LaneDisplayInfo>> {
    let config_path = hooks_config_path()?;
    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let doc = read_hooks_doc(&config_path)?;

    // Build git hook reverse mapping: lane_id -> hook_name
    let mut git_hook_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(projections) = doc.get("projections").and_then(Item::as_table)
        && let Some(git_hooks) = projections.get("git_hooks").and_then(Item::as_table)
    {
        for (hook_name, lane_item) in git_hooks.iter() {
            if let Some(lane_id) = lane_item.as_str() {
                // Convert hook_name from snake_case to kebab-case for display
                let display_hook = hook_name.replace('_', "-");
                git_hook_map.insert(lane_id.to_string(), display_hook);
            }
        }
    }

    let lanes = iter_lanes(&doc)?;
    let mut result = Vec::new();

    for (id, lane_table) in lanes {
        let label = lane_table
            .get("label")
            .and_then(Item::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| human_label(&id));

        let description = lane_table
            .get("description")
            .and_then(Item::as_str)
            .map(|s| s.to_string())
            .unwrap_or_default();

        let parallel = lane_table
            .get("parallel")
            .and_then(Item::as_bool)
            .unwrap_or(false);

        let checks: Vec<String> = lane_table
            .get("checks")
            .and_then(Item::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let git_hook = git_hook_map.get(&id).cloned();

        result.push(LaneDisplayInfo {
            id,
            label,
            description,
            parallel,
            checks,
            git_hook,
        });
    }

    Ok(result)
}

/// Check if colors should be enabled based on the color mode and environment.
pub(crate) fn colors_enabled(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Never => false,
        ColorMode::Always => true,
        ColorMode::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                return false;
            }
            std::io::stdout().is_terminal() || std::io::stderr().is_terminal()
        }
    }
}
