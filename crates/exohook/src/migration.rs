use anyhow::{Context, Result, anyhow};
use indexmap::IndexMap;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use toml_edit::DocumentMut;

use crate::config::{
    canonical_doc_baseline, get_check_table_mut, insert_check_run, insert_lane, set_lane_parallel,
    set_override_field_canonical, toml_string_array, value_from_toml, write_hooks_doc,
};
use crate::validate_hooks_doc;

#[derive(Debug, Deserialize)]
struct LefthookConfig {
    #[serde(rename = "pre-commit")]
    pre_commit: Option<LefthookHook>,

    #[serde(rename = "pre-push")]
    pre_push: Option<LefthookHook>,
}

#[derive(Debug, Deserialize)]
struct LefthookHook {
    #[serde(default)]
    parallel: bool,
    #[serde(default)]
    commands: IndexMap<String, LefthookCommand>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct LefthookCommand {
    pub(crate) run: String,
    #[serde(default)]
    pub(crate) stage_fixed: bool,
}

pub(crate) fn migrate_lefthook(
    input: &Path,
    output: &Path,
    report: &Path,
    force: bool,
) -> Result<()> {
    let content =
        fs::read_to_string(input).with_context(|| format!("failed to read {}", input.display()))?;
    let cfg: LefthookConfig =
        serde_yaml::from_str(&content).context("failed to parse lefthook.yml")?;

    if output.exists() && !force {
        write_lefthook_migration_report(
            report,
            input,
            output,
            false,
            Some("output already exists; no files were written (use --force to overwrite)"),
            &cfg,
        )?;
        return Err(anyhow!(
            "{} already exists (use --force to overwrite)",
            output.display()
        ));
    }

    let mut doc = canonical_doc_baseline()?;

    // Collect checks from lefthook in first-seen order.
    let mut checks: IndexMap<String, LefthookCommand> = IndexMap::new();

    let pre_commit_ids: Vec<String> = cfg
        .pre_commit
        .as_ref()
        .map(|h| h.commands.keys().cloned().collect())
        .unwrap_or_default();
    let pre_push_ids: Vec<String> = cfg
        .pre_push
        .as_ref()
        .map(|h| h.commands.keys().cloned().collect())
        .unwrap_or_default();

    let pre_commit_parallel = cfg.pre_commit.as_ref().map(|h| h.parallel).unwrap_or(false);
    let pre_push_parallel = cfg.pre_push.as_ref().map(|h| h.parallel).unwrap_or(false);

    if let Some(h) = cfg.pre_commit.as_ref() {
        for (id, cmd) in &h.commands {
            checks.insert(id.clone(), cmd.clone());
        }
    }
    if let Some(h) = cfg.pre_push.as_ref() {
        for (id, cmd) in &h.commands {
            if !checks.contains_key(id) {
                checks.insert(id.clone(), cmd.clone());
            }
        }
    }

    // dev mirrors pre-commit but runs on uncommitted
    if !pre_commit_ids.is_empty() {
        insert_lane(
            &mut doc,
            "dev",
            "{ op = \"base\", base = \"uncommitted\" }",
            &toml_string_array(&pre_commit_ids),
        )?;

        if pre_commit_parallel {
            set_lane_parallel(&mut doc, "dev", true)?;
        }

        // coherence is the true pre-commit lane (staged)
        insert_lane(
            &mut doc,
            "coherence",
            "{ op = \"base\", base = \"staged\" }",
            &toml_string_array(&pre_commit_ids),
        )?;

        if pre_commit_parallel {
            set_lane_parallel(&mut doc, "coherence", true)?;
        }

        add_stage_fixed_overrides_canonical(&mut doc, "coherence", &checks)?;
    }

    // gate mirrors pre-push but runs on committed-not-pushed
    if !pre_push_ids.is_empty() {
        insert_lane(
            &mut doc,
            "gate",
            "{ op = \"base\", base = \"committed_not_pushed\" }",
            &toml_string_array(&pre_push_ids),
        )?;

        if pre_push_parallel {
            set_lane_parallel(&mut doc, "gate", true)?;
        }

        // ci mirrors pre-push but runs on head (best-effort v0 behavior)
        insert_lane(
            &mut doc,
            "ci",
            "{ op = \"base\", base = \"head\" }",
            &toml_string_array(&pre_push_ids),
        )?;

        if pre_push_parallel {
            set_lane_parallel(&mut doc, "ci", true)?;
        }
    }

    for (id, cmd) in &checks {
        let label = human_label(id);
        insert_check_run(&mut doc, id, &label, "none", &cmd.run)?;

        if cmd.stage_fixed {
            // Stage-fixed implies this check is a mutate check.
            let Some(check) = get_check_table_mut(&mut doc, id) else {
                return Err(anyhow!("internal error: migrated check '{id}' missing"));
            };
            check["category"] = value_from_toml("\"mutate\"")?;
        }
    }

    // Validate what we generated.
    validate_hooks_doc(&doc)?;

    write_hooks_doc(output, &doc)?;

    write_lefthook_migration_report(report, input, output, true, None, &cfg)?;
    println!("Wrote migration report: {}", report.display());

    Ok(())
}

fn write_lefthook_migration_report(
    report_path: &Path,
    input: &Path,
    output: &Path,
    wrote_output: bool,
    status_note: Option<&str>,
    cfg: &LefthookConfig,
) -> Result<()> {
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut lines: Vec<String> = Vec::new();
    lines.push("exohook migrate lefthook report".to_string());
    lines.push(format!("input:  {}", input.display()));
    lines.push(format!("output: {}", output.display()));
    lines.push(format!(
        "wrote hooks.toml: {}",
        if wrote_output { "yes" } else { "no" }
    ));
    if let Some(note) = status_note {
        lines.push(format!("note: {note}"));
    }
    lines.push(String::new());

    lines.push("Lane mapping:".to_string());
    lines.push("- pre-commit -> coherence (staged) + dev (uncommitted mirror)".to_string());
    lines.push("- pre-push   -> gate (committed_not_pushed) + ci (head mirror)".to_string());
    lines.push(String::new());

    lines.push("Assumptions / conservative choices:".to_string());
    lines.push(
        "- lefthook `run:` preserved as check.run (executed via bash -lc internally)".to_string(),
    );
    lines.push("- checks default to input_mode=\"none\" (no file-level guessing)".to_string());
    lines.push("- lefthook `parallel: true` is represented as lane.parallel=true".to_string());
    lines.push(
        "- stage_fixed -> check.category=\"mutate\" + coherence override restage=auto (containment=fail)"
            .to_string(),
    );
    lines.push(String::new());

    let pre_commit = cfg
        .pre_commit
        .as_ref()
        .map(|h| h.commands.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let pre_push = cfg
        .pre_push
        .as_ref()
        .map(|h| h.commands.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    lines.push("Migrated hooks:".to_string());
    lines.push(format!(
        "- pre-commit commands: {}",
        if pre_commit.is_empty() {
            "(none)".to_string()
        } else {
            pre_commit.join(", ")
        }
    ));
    lines.push(format!(
        "- pre-push commands:   {}",
        if pre_push.is_empty() {
            "(none)".to_string()
        } else {
            pre_push.join(", ")
        }
    ));
    lines.push(String::new());

    lines.push("Next steps:".to_string());
    lines.push("- exohook config validate".to_string());
    lines.push("- exohook validate coherence --dry-run".to_string());
    lines.push("- exohook validate gate --dry-run".to_string());
    lines.push(
        "- review argv wrappers; replace bash -lc with structured argv where safe".to_string(),
    );

    fs::write(report_path, lines.join("\n") + "\n")
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    Ok(())
}

pub(crate) fn human_label(id: &str) -> String {
    let mut s = String::new();
    for (i, part) in id.split('-').enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let mut chars = part.chars();
        if let Some(c) = chars.next() {
            s.push(c.to_ascii_uppercase());
            s.extend(chars);
        }
    }
    if s.is_empty() { id.to_string() } else { s }
}

fn add_stage_fixed_overrides_canonical(
    doc: &mut DocumentMut,
    lane_id: &str,
    checks: &IndexMap<String, LefthookCommand>,
) -> Result<()> {
    let stage_fixed: Vec<String> = checks
        .iter()
        .filter_map(|(id, cmd)| {
            if cmd.stage_fixed {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();

    if stage_fixed.is_empty() {
        return Ok(());
    }

    for id in stage_fixed {
        set_override_field_canonical(doc, lane_id, &id, "restage", "\"auto\"")?;
        set_override_field_canonical(doc, lane_id, &id, "restage_containment", "\"fail\"")?;
    }

    Ok(())
}
