#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)] // CLI tool uses blocking I/O

mod check_runner;
mod ci_emit;
pub mod config;
mod discover;
mod fileset;
mod filter;
mod hooks;
mod jsonl;
mod lane;
mod legacy;
mod migration;
mod output_buffer;
mod pipe_runner;
mod shell;
mod terminal;
mod validate;

#[cfg(unix)]
mod pty_runner;

pub use check_runner::{CheckResult as RunnerCheckResult, OutputMode, spawn_check};
pub use config::{CheckV3, ConfigV3, ConfigVersion, HooksV3, WorkflowV3};
pub use exohook::{ColorMode, OutputFormat};
pub(crate) use legacy::{resolve_check_command_parts, validate_hooks_doc};
pub use output_buffer::{CheckProgressGroup, OutputBuffer};
pub use terminal::{
    DurationCategory, TerminalConfig, WidthTier, compact_progress_indicator, format_duration,
    format_lane_summary, format_result_line, truncate_for_display,
};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use console::Term;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::config::{
    ExecutionContext, HookType, canonical_doc_baseline, canonicalize_doc_if_needed,
    extract_inline_checks, get_check_table_mut, hooks_config_path, insert_check_run, insert_lane,
    migrate_v2_to_v3, read_hooks_doc, remove_check, remove_lane, remove_override, set_check_field,
    set_default_field, set_lane_field, set_override_field, set_override_field_canonical,
    unset_default_field, value_from_toml, write_hooks_doc,
};
use crate::hooks::hooks_install;
use crate::lane::show_lane_listing;
use crate::migration::migrate_lefthook;
use crate::validate::{validate_from_config, validate_v3_hook, validate_v3_workflow};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate a named lane (e.g. gate, ci)
    Validate {
        /// Validation lane name (run without args to see available lanes)
        lane: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "compact")]
        format: OutputFormat,

        /// Print per-check output even on success
        #[arg(long)]
        verbose: bool,

        /// Color output
        #[arg(long, value_enum, default_value = "auto")]
        color: ColorMode,

        /// Print what would run, without executing
        #[arg(long)]
        dry_run: bool,
    },

    /// Run a named workflow (defined in [workflow.*])
    Run {
        /// Workflow name (e.g., "full", "quick")
        workflow: String,

        /// Output format
        #[arg(long, value_enum, default_value = "compact")]
        format: OutputFormat,

        /// Print per-check output even on success
        #[arg(long)]
        verbose: bool,

        /// Color output
        #[arg(long, value_enum, default_value = "auto")]
        color: ColorMode,

        /// Print what would run, without executing
        #[arg(long)]
        dry_run: bool,
    },

    /// List configured hooks/workflows and checks
    Discover {
        /// Output format
        #[arg(long, value_enum, default_value = "human")]
        format: discover::DiscoverFormat,

        /// Filter to a specific hook/workflow
        #[arg(long)]
        lane: Option<String>,
    },

    /// Install git hook shims into .git/hooks
    Hooks {
        #[command(subcommand)]
        command: HookCommands,
    },

    /// Migrate external hook-runner config into .config/exo/hooks.toml
    Migrate {
        #[command(subcommand)]
        command: MigrateCommands,
    },

    /// Manage `.config/exo/hooks.toml`
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Generate CI workflows from hooks.toml
    Ci {
        #[command(subcommand)]
        command: CiCommands,
    },
}

#[derive(Subcommand)]
enum HookCommands {
    /// Install hook shims
    Install,
}

#[derive(Subcommand)]
enum CiCommands {
    /// Emit CI workflow file from hooks.toml
    Emit {
        /// Target CI platform
        #[arg(value_enum, default_value = "github-actions")]
        target: CiTargetArg,

        /// Output path (defaults to .github/workflows/exo-ci.yml for github-actions)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Lane to generate workflow for (defaults to 'ci' or projections.github_actions.ci)
        #[arg(long)]
        lane: Option<String>,

        /// Print to stdout without writing file
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CiTargetArg {
    /// GitHub Actions workflow
    #[value(name = "github-actions")]
    GitHubActions,
}

#[derive(Subcommand)]
enum MigrateCommands {
    /// Migrate hooks.toml from v2 to v3 format
    V3 {
        /// Target version (currently only "v3")
        #[arg(default_value = "v3")]
        version: String,

        /// Input config path
        #[arg(short, long, default_value = ".config/exo/hooks.toml")]
        input: PathBuf,

        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Edit the file in-place
        #[arg(long)]
        in_place: bool,
    },

    /// Migrate lefthook.yml into .config/exo/hooks.toml
    Lefthook {
        /// Path to lefthook.yml
        #[arg(long, default_value = "lefthook.yml")]
        input: PathBuf,

        /// Output path for hooks.toml
        #[arg(long, default_value = ".config/exo/hooks.toml")]
        output: PathBuf,

        /// Write a migration report (always written; this sets the path)
        #[arg(long, default_value = ".config/exo/migrate-lefthook.report.txt")]
        report: PathBuf,

        /// Overwrite output if it already exists
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Create a starter `.config/exo/hooks.toml`
    Init {
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },

    /// Validate `.config/exo/hooks.toml` (schema + references)
    Validate,

    /// Extract inline checks to named [check.*] definitions
    Extract {
        /// Print changes without modifying the file
        #[arg(long)]
        dry_run: bool,

        /// Prefix for generated check IDs (default: "inline")
        #[arg(long, default_value = "inline")]
        prefix: String,
    },

    /// Remove a lane (identified by `lanes[].id`)
    RemoveLane {
        /// Lane ID (e.g. `coherence`)
        #[arg(long)]
        id: String,
    },

    /// Remove a check (identified by `checks[].id`)
    RemoveCheck {
        /// Check ID (e.g. `rust-clippy`)
        #[arg(long)]
        id: String,
    },

    /// Set a default knob under `[defaults]`
    SetDefault {
        /// Field name under `[defaults]` (e.g. `timeout_seconds`)
        field: String,
        /// TOML value (e.g. `600`, `true`, `"index"`)
        #[arg(long)]
        toml: String,
    },

    /// Remove a default field under `[defaults]`
    UnsetDefault {
        /// Field name under `[defaults]`
        field: String,
    },

    /// Set a field on a check (identified by `checks[].id`)
    SetCheck {
        /// Check ID (e.g. `rust-clippy`)
        #[arg(long)]
        id: String,
        /// Field to set (e.g. `argv`, `input_mode`, `batchable`)
        field: String,
        /// TOML value (e.g. `true`, `"paths"`, `["pnpm","-r","run","lint"]`)
        #[arg(long)]
        toml: String,
    },

    /// Set a field on a lane (identified by `lanes[].id`)
    SetLane {
        /// Lane ID (e.g. `coherence`)
        #[arg(long)]
        id: String,
        /// Field to set (e.g. `checks`, `fileset`)
        field: String,
        /// TOML value
        #[arg(long)]
        toml: String,
    },

    /// Set a field on a per-lane override (identified by lane id + override check id)
    SetOverride {
        /// Lane ID (e.g. `coherence`)
        #[arg(long)]
        lane: String,
        /// Check ID this override targets (e.g. `rust-fmt`)
        #[arg(long)]
        check: String,
        /// Field to set on the override (e.g. `restage_containment`)
        field: String,
        /// TOML value
        #[arg(long)]
        toml: String,
    },

    /// Remove a per-lane override (identified by lane id + override check id)
    RemoveOverride {
        /// Lane ID (e.g. `coherence`)
        #[arg(long)]
        lane: String,
        /// Check ID this override targets
        #[arg(long)]
        check: String,
    },
}

fn main() -> Result<()> {
    exo_reexec::maybe_reexec();

    let cli = Cli::parse();

    // Track whether this is the ci emit command (skip auto-regeneration for it)
    let is_ci_emit = matches!(cli.command, Commands::Ci { .. });

    let result = match cli.command {
        Commands::Validate {
            lane,
            dry_run,
            format,
            verbose,
            color,
        } => {
            let Some(lane) = lane else {
                // Early return for lane listing (no actual validation)
                return show_lane_listing(color, None);
            };
            validate(&lane, dry_run, format, verbose, color)
        }
        Commands::Run {
            workflow,
            dry_run,
            format,
            verbose,
            color,
        } => {
            let config_path = hooks_config_path()?;
            validate_v3_workflow(&config_path, &workflow, dry_run, format, verbose, color)
        }
        Commands::Discover { format, lane } => discover::discover(format, lane.as_deref()),
        Commands::Hooks {
            command: HookCommands::Install,
        } => hooks_install(),
        Commands::Migrate {
            command:
                MigrateCommands::V3 {
                    version,
                    input,
                    output,
                    in_place,
                },
        } => {
            if version != "v3" {
                return Err(anyhow!("only 'v3' migration is supported"));
            }

            if in_place && output.is_some() {
                return Err(anyhow!("--in-place cannot be used with --output"));
            }

            let doc = read_hooks_doc(&input)?;
            let migrated = migrate_v2_to_v3(&doc)?;

            if in_place {
                fs::write(&input, &migrated)?;
                eprintln!("Migrated {} to v3", input.display());
            } else if let Some(out) = output {
                fs::write(&out, &migrated)?;
                eprintln!("Wrote v3 config to {}", out.display());
            } else {
                println!("{}", migrated);
            }
            Ok(())
        }

        Commands::Migrate {
            command:
                MigrateCommands::Lefthook {
                    input,
                    output,
                    report,
                    force,
                },
        } => migrate_lefthook(&input, &output, &report, force),

        Commands::Config { command } => handle_config_command(command),

        Commands::Ci { command } => handle_ci_command(command),
    };

    // Auto-regenerate CI workflow after successful commands (except ci emit itself)
    if result.is_ok() && !is_ci_emit {
        maybe_regenerate_ci_workflow();
    }

    result
}

/// Regenerate CI workflow if hooks.toml exists and has a CI lane configured.
/// This is best-effort; failures are silently ignored.
fn maybe_regenerate_ci_workflow() {
    let Ok(config_path) = hooks_config_path() else {
        return;
    };

    if !config_path.exists() {
        return;
    }

    let Ok(doc) = read_hooks_doc(&config_path) else {
        return;
    };

    // Check if auto-generation is disabled via projections config
    if let Some(projections) = doc.get("projections").and_then(|p| p.as_table())
        && let Some(gh_actions) = projections.get("github_actions").and_then(|g| g.as_table())
        && let Some(enabled) = gh_actions.get("enabled").and_then(|e| e.as_bool())
        && !enabled
    {
        return;
    }

    // Check if there's a CI lane configured
    let lane_id = ci_emit::get_ci_lane(&doc);
    if !has_ci_projection_source(&doc, &lane_id) {
        return;
    }

    // Generate and write workflow
    let ci_target = ci_emit::CiTarget::GitHubActions;
    let Ok(workflow) = ci_emit::emit_workflow(&doc, ci_target, &lane_id) else {
        return;
    };

    let out_path = PathBuf::from(ci_emit::default_output_path(ci_target));

    // Only overwrite if the file doesn't exist or has the AUTO-GENERATED header
    if out_path.exists()
        && let Ok(existing) = fs::read_to_string(&out_path)
        && !existing.starts_with("# AUTO-GENERATED by exohook")
    {
        // File exists but wasn't generated by us - don't clobber it
        return;
    }

    // Create parent directories
    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Write workflow (silently)
    let _ = fs::write(&out_path, &workflow);
}

fn has_ci_projection_source(doc: &toml_edit::DocumentMut, lane_id: &str) -> bool {
    let source_table = match ConfigVersion::from_doc(doc) {
        ConfigVersion::V3 => "workflow",
        ConfigVersion::V1 | ConfigVersion::V2 => "lane",
        ConfigVersion::Unknown(_) => return false,
    };

    doc.get(source_table)
        .and_then(|sources| sources.get(lane_id))
        .and_then(|source| source.as_table())
        .is_some()
}

fn validate(
    lane: &str,
    dry_run: bool,
    format: OutputFormat,
    verbose: bool,
    color: ColorMode,
) -> Result<()> {
    // Prefer config-driven behavior when `.config/exo/hooks.toml` exists.
    let config_path = hooks_config_path()?;
    if config_path.exists() {
        let doc = read_hooks_doc(&config_path)?;
        if ConfigVersion::from_doc(&doc) == ConfigVersion::V3 {
            if let Some(hook_type) = HookType::from_hook_name(lane) {
                let is_interactive = Term::stdout().is_term();
                let context = ExecutionContext::new(hook_type, is_interactive);
                return validate_v3_hook(
                    &config_path,
                    lane,
                    context,
                    dry_run,
                    format,
                    verbose,
                    color,
                );
            }
            // V3 workflow lane (dev, coherence, gate, ci, etc.)
            return validate_v3_workflow(&config_path, lane, dry_run, format, verbose, color);
        }
        return validate_from_config(&config_path, lane, dry_run, format, verbose, color);
    }

    // Temporary bootstrap: wire a couple of lanes directly to the current
    // repository checks, so contributors can use `exohook` without `lefthook`.
    let commands: Vec<&'static str> = match lane {
        // "gate" is intended to be the strict local/CI gate.
        "gate" | "pre-commit" => vec![
            "pnpm run verify:toml",
            "cargo fmt --all",
            "cargo clippy --workspace -- -D warnings",
            "pnpm -r run lint",
            "pnpm -r run check",
        ],
        "ci" | "pre-push" => vec![
            "pnpm -r run test:unit",
            "cargo llvm-cov --workspace --lcov --output-path lcov.info",
        ],
        other => {
            // Show the lane listing with the invalid lane name
            return show_lane_listing(color, Some(other));
        }
    };

    if dry_run {
        for cmd in commands {
            println!("{cmd}");
        }
        return Ok(());
    }

    for cmd in commands {
        let status = run_shell(cmd).with_context(|| format!("command failed: {cmd}"))?;
        if !status.success() {
            return Err(anyhow!("command exited non-zero: {cmd}"));
        }
    }

    Ok(())
}

fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Init { force } => config_init(force),
        ConfigCommands::Validate => {
            let config_path = hooks_config_path()?;
            let doc = read_hooks_doc(&config_path)?;
            validate_hooks_doc(&doc)
        }
        ConfigCommands::Extract { dry_run, prefix } => {
            let config_path = hooks_config_path()?;
            let doc = read_hooks_doc(&config_path)?;
            let (extracted, changes) = extract_inline_checks(&doc, &prefix)?;

            if changes.is_empty() {
                println!("No inline checks found.");
                return Ok(());
            }

            if dry_run {
                for change in &changes {
                    println!("{change}");
                }
                return Ok(());
            }

            std::fs::write(&config_path, extracted.to_string())?;
            println!("Extracted {} inline check(s).", changes.len());
            Ok(())
        }
        ConfigCommands::RemoveLane { id } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            remove_lane(&mut doc, &id)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::RemoveCheck { id } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            remove_check(&mut doc, &id)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::SetDefault { field, toml } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            set_default_field(&mut doc, &field, &toml)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::UnsetDefault { field } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            unset_default_field(&mut doc, &field)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::SetCheck { id, field, toml } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            set_check_field(&mut doc, &id, &field, &toml)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::SetLane { id, field, toml } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            set_lane_field(&mut doc, &id, &field, &toml)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::SetOverride {
            lane,
            check,
            field,
            toml,
        } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            set_override_field(&mut doc, &lane, &check, &field, &toml)?;
            write_hooks_doc(&config_path, &doc)
        }
        ConfigCommands::RemoveOverride { lane, check } => {
            let config_path = hooks_config_path()?;
            let mut doc = canonicalize_doc_if_needed(&read_hooks_doc(&config_path)?)?;
            remove_override(&mut doc, &lane, &check)?;
            write_hooks_doc(&config_path, &doc)
        }
    }
}

fn handle_ci_command(command: CiCommands) -> Result<()> {
    match command {
        CiCommands::Emit {
            target,
            output,
            lane,
            dry_run,
        } => {
            let config_path = hooks_config_path()?;
            let doc = read_hooks_doc(&config_path)?;

            // Determine lane
            let lane_id = lane.unwrap_or_else(|| ci_emit::get_ci_lane(&doc));

            // Map CLI arg to internal type
            let ci_target = match target {
                CiTargetArg::GitHubActions => ci_emit::CiTarget::GitHubActions,
            };

            // Generate workflow
            let workflow = ci_emit::emit_workflow(&doc, ci_target, &lane_id)?;

            if dry_run {
                print!("{}", workflow);
                return Ok(());
            }

            // Determine output path
            let out_path =
                output.unwrap_or_else(|| PathBuf::from(ci_emit::default_output_path(ci_target)));

            // Create parent directories
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }

            // Write workflow
            fs::write(&out_path, &workflow)
                .with_context(|| format!("failed to write {}", out_path.display()))?;

            println!("✅ Wrote CI workflow to {}", out_path.display());
            Ok(())
        }
    }
}

fn config_init(force: bool) -> Result<()> {
    let path = hooks_config_path()?;
    if path.exists() && !force {
        return Err(anyhow!(
            "{} already exists (use --force to overwrite)",
            path.display()
        ));
    }

    let mut doc = canonical_doc_baseline()?;

    insert_lane(
        &mut doc,
        "dev",
        "{ op = \"base\", base = \"uncommitted\" }",
        "[\"verify-toml\",\"rust-fmt\",\"rust-clippy\",\"lint\",\"check\"]",
    )?;

    insert_lane(
        &mut doc,
        "coherence",
        "{ op = \"base\", base = \"staged\" }",
        "[\"verify-toml\",\"rust-fmt\",\"rust-clippy\",\"lint\",\"check\"]",
    )?;

    // rust-fmt restages in coherence.
    set_override_field_canonical(&mut doc, "coherence", "rust-fmt", "restage", "\"auto\"")?;
    set_override_field_canonical(
        &mut doc,
        "coherence",
        "rust-fmt",
        "restage_containment",
        "\"fail\"",
    )?;

    insert_lane(
        &mut doc,
        "gate",
        "{ op = \"base\", base = \"committed_not_pushed\" }",
        "[\"test\",\"rust-coverage\"]",
    )?;

    insert_lane(
        &mut doc,
        "ci",
        "{ op = \"base\", base = \"head\" }",
        "[\"test\",\"rust-coverage\"]",
    )?;

    insert_check_run(
        &mut doc,
        "verify-toml",
        "Verify TOML",
        "none",
        "pnpm run verify:toml",
    )?;

    insert_check_run(&mut doc, "rust-fmt", "Rust fmt", "none", "cargo fmt --all")?;
    let Some(rust_fmt) = get_check_table_mut(&mut doc, "rust-fmt") else {
        return Err(anyhow!("internal error: rust-fmt missing"));
    };
    rust_fmt["category"] = value_from_toml("\"mutate\"")?;

    insert_check_run(
        &mut doc,
        "rust-clippy",
        "Clippy (strict)",
        "none",
        "cargo clippy --workspace -- -D warnings",
    )?;

    insert_check_run(&mut doc, "lint", "Lint", "none", "pnpm -r run lint")?;

    insert_check_run(&mut doc, "check", "Check", "none", "pnpm -r run check")?;

    insert_check_run(&mut doc, "test", "Test", "none", "pnpm -r run test:unit")?;

    insert_check_run(
        &mut doc,
        "rust-coverage",
        "Rust coverage",
        "none",
        "cargo llvm-cov --workspace --lcov --output-path lcov.info",
    )?;

    write_hooks_doc(&path, &doc)?;
    Ok(())
}

fn run_shell(command: &str) -> Result<ExitStatus> {
    let mut child = Command::new("bash")
        .args(["-lc", command])
        .spawn()
        .with_context(|| format!("failed to spawn shell for: {command}"))?;

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for: {command}"))?;

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::DocumentMut;

    #[test]
    fn ci_projection_source_accepts_v3_workflows() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["test"]

[check.test]
command = "npm test"
"#
        .parse()
        .unwrap();

        assert!(has_ci_projection_source(&doc, "ci"));
    }

    #[test]
    fn ci_projection_source_accepts_legacy_lanes() {
        let doc: DocumentMut = r#"
[lane.ci]
checks = ["test"]

[check.test]
run = "npm test"
"#
        .parse()
        .unwrap();

        assert!(has_ci_projection_source(&doc, "ci"));
    }

    #[test]
    fn ci_projection_source_rejects_missing_sources() {
        let v3_doc: DocumentMut = "version = 3\n".parse().unwrap();
        let legacy_doc: DocumentMut = "".parse().unwrap();

        assert!(!has_ci_projection_source(&v3_doc, "ci"));
        assert!(!has_ci_projection_source(&legacy_doc, "ci"));
    }
}
