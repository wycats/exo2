use anyhow::{Result, anyhow};
use toml_edit::{DocumentMut, Item, Table};

use crate::config::{iter_checks, iter_lanes};
use crate::fileset::{substitute_files, substitute_files_in_argv};
use crate::shell::shell_command_parts;

pub(crate) fn resolve_check_command_parts(
    check_id: &str,
    check_table: &Table,
    input_mode: &str,
    files: Option<&[String]>,
) -> Result<Vec<String>> {
    if let Some(run) = check_table.get("run").and_then(Item::as_str) {
        let run = if input_mode == "paths" {
            let Some(files) = files else {
                return Err(anyhow!(
                    "check '{check_id}' expects files but none were provided"
                ));
            };
            substitute_files(run, files)
        } else {
            run.to_string()
        };
        return Ok(shell_command_parts(run));
    }

    let argv = check_table
        .get("argv")
        .and_then(Item::as_array)
        .ok_or_else(|| anyhow!("check '{check_id}' is missing argv or run"))?;

    let mut parts: Vec<String> = Vec::new();
    for a in argv.iter() {
        parts.push(
            a.as_str()
                .ok_or_else(|| anyhow!("check '{check_id}' argv contains non-string"))?
                .to_string(),
        );
    }

    if input_mode == "paths" {
        let Some(files) = files else {
            return Err(anyhow!(
                "check '{check_id}' expects files but none were provided"
            ));
        };
        return Ok(substitute_files_in_argv(&parts, files));
    }

    Ok(parts)
}

pub(crate) fn validate_hooks_doc(doc: &DocumentMut) -> Result<()> {
    // Version
    let version = doc
        .get("version")
        .and_then(Item::as_integer)
        .ok_or_else(|| anyhow!("missing required integer `version`"))?;
    if version != 1 && version != 2 {
        return Err(anyhow!("unsupported hooks.toml version: {version}"));
    }

    // Unique IDs
    let mut lane_ids = std::collections::HashSet::<String>::new();
    for (id, lane) in iter_lanes(doc)? {
        if !lane_ids.insert(id.clone()) {
            return Err(anyhow!("duplicate lane id '{id}'"));
        }

        if lane.get("checks").and_then(Item::as_array).is_none() {
            return Err(anyhow!("lane '{id}' missing checks array"));
        }

        if let Some(item) = lane.get("parallel")
            && item.as_bool().is_none()
        {
            return Err(anyhow!("lane '{id}' parallel must be a boolean"));
        }

        // overrides shape (legacy or canonical)
        if let Some(ovt) = lane.get("overrides").and_then(Item::as_table) {
            for (check_id, item) in ovt.iter() {
                let Some(_) = item.as_table() else {
                    return Err(anyhow!("lane '{id}' overrides.{check_id} must be a table"));
                };
            }
        }
        if let Some(overrides) = lane.get("overrides").and_then(Item::as_array_of_tables) {
            for ov in overrides.iter() {
                let _check = ov
                    .get("check")
                    .and_then(Item::as_str)
                    .ok_or_else(|| anyhow!("lane '{id}' override missing check id"))?;
            }
        }
    }

    let mut check_ids = std::collections::HashSet::<String>::new();
    for (id, check) in iter_checks(doc)? {
        if !check_ids.insert(id.clone()) {
            return Err(anyhow!("duplicate check id '{id}'"));
        }

        let input_mode = check
            .get("input_mode")
            .and_then(Item::as_str)
            .unwrap_or("none");

        if input_mode != "none" && input_mode != "paths" {
            return Err(anyhow!(
                "check '{id}' has unsupported input_mode='{input_mode}'"
            ));
        }

        // Must provide either run or argv
        let has_run = check.get("run").and_then(Item::as_str).is_some();
        let has_argv = check.get("argv").and_then(Item::as_array).is_some();
        if !has_run && !has_argv {
            return Err(anyhow!("check '{id}' must have either run or argv"));
        }

        if let Some(argv) = check.get("argv").and_then(Item::as_array) {
            for a in argv.iter() {
                if a.as_str().is_none() {
                    return Err(anyhow!("check '{id}' argv must be strings"));
                }
            }
        }

        if input_mode == "paths" {
            if let Some(run) = check.get("run").and_then(Item::as_str) {
                if !run.contains("{{files}}") {
                    return Err(anyhow!(
                        "check '{id}' has input_mode='paths' but run lacks '{{{{files}}}}'"
                    ));
                }
            } else {
                let argv = check.get("argv").and_then(Item::as_array).ok_or_else(|| {
                    anyhow!("check '{id}' has input_mode='paths' but argv lacks '{{{{files}}}}'")
                })?;
                let has_files_placeholder = argv
                    .iter()
                    .any(|a| a.as_str().is_some_and(|s| s == "{{files}}"));
                if !has_files_placeholder {
                    return Err(anyhow!(
                        "check '{id}' has input_mode='paths' but argv lacks '{{{{files}}}}'"
                    ));
                }
            }
        }

        // injection list sanity (optional)
        if let Some(injection) = check.get("injection").and_then(Item::as_array) {
            for item in injection.iter() {
                let s = item
                    .as_str()
                    .ok_or_else(|| anyhow!("check '{id}' injection values must be strings"))?;
                if s != "argv_placeholder" && s != "response_file" && s != "stdin" {
                    return Err(anyhow!("check '{id}' has unknown injection '{s}'"));
                }
            }
        }
    }

    // Lane -> check references
    for (lane_id, lane) in iter_lanes(doc)? {
        let lane_checks = lane
            .get("checks")
            .and_then(Item::as_array)
            .ok_or_else(|| anyhow!("lane '{lane_id}' missing checks array"))?;
        for check_id_item in lane_checks.iter() {
            let check_id = check_id_item
                .as_str()
                .ok_or_else(|| anyhow!("lane '{lane_id}' contains non-string check id"))?;
            if !check_ids.contains(check_id) {
                return Err(anyhow!(
                    "lane '{lane_id}' references unknown check '{check_id}'"
                ));
            }
        }

        // Overrides references (canonical)
        if let Some(ovt) = lane.get("overrides").and_then(Item::as_table) {
            for (check_id, _item) in ovt.iter() {
                if !check_ids.contains(check_id) {
                    return Err(anyhow!(
                        "lane '{lane_id}' override references unknown check '{check_id}'"
                    ));
                }
            }
        }

        // Overrides references (legacy)
        if let Some(overrides) = lane.get("overrides").and_then(Item::as_array_of_tables) {
            for ov in overrides.iter() {
                let check = ov
                    .get("check")
                    .and_then(Item::as_str)
                    .ok_or_else(|| anyhow!("lane '{lane_id}' override missing check id"))?;
                if !check_ids.contains(check) {
                    return Err(anyhow!(
                        "lane '{lane_id}' override references unknown check '{check}'"
                    ));
                }
            }
        }
    }

    Ok(())
}
