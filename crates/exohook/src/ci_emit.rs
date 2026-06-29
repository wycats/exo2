//! CI workflow generation from hooks.toml configuration.
//!
//! This module generates CI workflow files (e.g., GitHub Actions) from the
//! declarative configuration in `.config/exo/hooks.toml`. Each check in the
//! specified lane becomes a separate job in the generated workflow.

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use toml_edit::DocumentMut;

use crate::config::{CheckRefV3, CheckV3, ConfigV3, ConfigVersion};

/// Target CI platforms for workflow generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiTarget {
    GitHubActions,
}

/// Toolchain requirements inferred from a check command.
#[derive(Debug, Default, Clone)]
struct ToolchainRequirements {
    needs_rust: bool,
    needs_node: bool,
    needs_pnpm: bool,
    needs_cargo_llvm_cov: bool,
    rust_components: HashSet<String>,
}

/// A single check extracted from the lane configuration.
#[derive(Debug, Clone)]
struct CheckSpec {
    id: String,
    label: String,
    run: String,
    cwd: Option<String>,
    reqs: ToolchainRequirements,
}

/// Generate a CI workflow from hooks.toml configuration.
///
/// # Arguments
///
/// * `doc` - Parsed hooks.toml document
/// * `target` - Target CI platform
/// * `lane_id` - Lane to generate workflow for
///
/// # Returns
///
/// The generated workflow content as a string.
pub fn emit_workflow(doc: &DocumentMut, target: CiTarget, lane_id: &str) -> Result<String> {
    match target {
        CiTarget::GitHubActions => emit_github_actions(doc, lane_id),
    }
}

/// Get the default lane for CI projection.
///
/// Looks for `[projections.github_actions]` in the config, falls back to "ci".
pub fn get_ci_lane(doc: &DocumentMut) -> String {
    doc.get("projections")
        .and_then(|p| p.get("github_actions"))
        .and_then(|ga| ga.get("ci"))
        .and_then(|v| v.as_str())
        .unwrap_or("ci")
        .to_string()
}

/// Infer toolchain requirements from a single run command.
fn infer_requirements_from_run(run: &str) -> ToolchainRequirements {
    let mut reqs = ToolchainRequirements::default();

    if run.contains("cargo") {
        reqs.needs_rust = true;
    }
    if run.contains("pnpm") {
        reqs.needs_pnpm = true;
        reqs.needs_node = true;
    }
    if run.contains("npm") && !run.contains("pnpm") {
        reqs.needs_node = true;
    }
    if run.contains("cargo llvm-cov") || run.contains("cargo-llvm-cov") {
        reqs.needs_cargo_llvm_cov = true;
        reqs.rust_components
            .insert("llvm-tools-preview".to_string());
    }
    if run.contains("cargo fmt") {
        reqs.rust_components.insert("rustfmt".to_string());
    }
    if run.contains("cargo clippy") {
        reqs.rust_components.insert("clippy".to_string());
    }

    reqs
}

/// Extract all checks from a lane or workflow configuration.
#[cfg(test)]
fn extract_checks(doc: &DocumentMut, lane_id: &str) -> Result<Vec<CheckSpec>> {
    extract_checks_for_version(doc, lane_id, ConfigVersion::from_doc(doc))
}

fn extract_checks_for_version(
    doc: &DocumentMut,
    lane_id: &str,
    version: ConfigVersion,
) -> Result<Vec<CheckSpec>> {
    match version {
        ConfigVersion::V3 => extract_v3_checks(doc, lane_id),
        ConfigVersion::V1 | ConfigVersion::V2 => extract_legacy_checks(doc, lane_id),
        ConfigVersion::Unknown(version) => Err(anyhow!(
            "unsupported hooks.toml version {}; exohook ci emit supports version = 3 and legacy lane configs",
            version
        )),
    }
}

fn extract_legacy_checks(doc: &DocumentMut, lane_id: &str) -> Result<Vec<CheckSpec>> {
    let lane = doc
        .get("lane")
        .and_then(|l| l.get(lane_id))
        .and_then(|l| l.as_table())
        .with_context(|| format!("lane '{}' not found", lane_id))?;

    let checks = lane
        .get("checks")
        .and_then(|c| c.as_array())
        .with_context(|| format!("lane '{}' has no checks array", lane_id))?;

    let mut specs = Vec::new();
    let mut used_job_ids = HashSet::new();

    for check_item in checks.iter() {
        let Some(check_id) = check_item.as_str() else {
            continue;
        };

        let Some(check) = doc.get("check").and_then(|c| c.get(check_id)) else {
            continue;
        };

        let label = check
            .get("label")
            .and_then(|l| l.as_str())
            .unwrap_or(check_id)
            .to_string();

        let run = check
            .get("run")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        let reqs = infer_requirements_from_run(&run);

        let spec = CheckSpec {
            id: check_id.to_string(),
            label,
            run,
            cwd: None,
            reqs,
        };
        reserve_job_id(lane_id, &spec.id, &mut used_job_ids)?;
        specs.push(spec);
    }

    Ok(specs)
}

fn extract_v3_checks(doc: &DocumentMut, workflow_id: &str) -> Result<Vec<CheckSpec>> {
    let config = ConfigV3::from_doc(doc)?;
    config.validate()?;

    let workflow = config
        .workflow
        .get(workflow_id)
        .with_context(|| format!("workflow '{}' not found", workflow_id))?;

    let mut specs = Vec::new();
    let mut used_job_ids = HashSet::new();
    for (idx, check_ref) in workflow.checks.iter().enumerate() {
        match check_ref {
            CheckRefV3::Ref(check_id) => {
                let check = config.check.get(check_id).with_context(|| {
                    format!(
                        "workflow '{}' references missing check '{}'",
                        workflow_id, check_id
                    )
                })?;
                let spec = v3_check_spec(check_id, check)?;
                reserve_job_id(workflow_id, &spec.id, &mut used_job_ids)?;
                specs.push(spec);
            }
            CheckRefV3::Inline(check) => {
                let check_id = unique_inline_check_id(&config, &used_job_ids, idx);
                let spec = v3_check_spec(&check_id, check)?;
                reserve_job_id(workflow_id, &spec.id, &mut used_job_ids)?;
                specs.push(spec);
            }
        }
    }

    Ok(specs)
}

fn unique_inline_check_id(config: &ConfigV3, used_job_ids: &HashSet<String>, idx: usize) -> String {
    let named_job_ids: HashSet<String> = config
        .check
        .keys()
        .map(|id| github_actions_job_id(id))
        .collect();
    let base = format!("inline-{idx}");
    let mut candidate = base.clone();
    let mut suffix = 1usize;

    loop {
        let job_id = github_actions_job_id(&candidate);
        if !config.check.contains_key(&candidate)
            && !named_job_ids.contains(&job_id)
            && !used_job_ids.contains(&job_id)
        {
            return candidate;
        }

        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
}

fn reserve_job_id(
    workflow_id: &str,
    check_id: &str,
    used_job_ids: &mut HashSet<String>,
) -> Result<()> {
    let job_id = github_actions_job_id(check_id);
    if !used_job_ids.insert(job_id.clone()) {
        return Err(anyhow!(
            "CI workflow '{}' emits duplicate GitHub Actions job id '{}' from check '{}'",
            workflow_id,
            job_id,
            check_id
        ));
    }
    Ok(())
}

fn github_actions_job_id(check_id: &str) -> String {
    let mut job_id = String::with_capacity(check_id.len().max(1));
    for ch in check_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            job_id.push(ch);
        } else {
            job_id.push('_');
        }
    }

    if !job_id
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        job_id.insert(0, '_');
    }

    job_id
}

fn v3_check_spec(check_id: &str, check: &CheckV3) -> Result<CheckSpec> {
    let command = check.command.clone().with_context(|| {
        format!(
            "check '{}' cannot be emitted to CI because it has no shell command",
            check_id
        )
    })?;
    let run = render_v3_ci_command(check_id, &command)
        .with_context(|| format!("failed to prepare CI command for check '{}'", check_id))?;
    let label = check.label.clone().unwrap_or_else(|| check_id.to_string());
    let reqs = infer_requirements_from_run(&run);

    Ok(CheckSpec {
        id: check_id.to_string(),
        label,
        run,
        cwd: check.cwd.clone(),
        reqs,
    })
}

fn render_v3_ci_command(check_id: &str, command: &str) -> Result<String> {
    if !command.contains("{{files}}") {
        return Ok(command.to_string());
    }

    Err(anyhow!(
        "check '{}' cannot be emitted to CI because its command uses '{{{{files}}}}'; expanding file placeholders during ci emit would freeze a stale file list in the workflow",
        check_id
    ))
}

/// Generate GitHub Actions workflow YAML.
fn emit_github_actions(doc: &DocumentMut, lane_id: &str) -> Result<String> {
    let version = ConfigVersion::from_doc(doc);
    let checks = extract_checks_for_version(doc, lane_id, version)?;
    let parallel = workflow_parallel(doc, lane_id, version);
    let source_table = if version == ConfigVersion::V3 {
        "workflow"
    } else {
        "lane"
    };

    // Get lane/workflow label for workflow name.
    let lane_label = if version == ConfigVersion::V3 {
        doc.get("workflow")
            .and_then(|l| l.get(lane_id))
            .and_then(|l| l.get("label"))
            .and_then(|v| v.as_str())
            .unwrap_or("CI")
    } else {
        doc.get("lane")
            .and_then(|l| l.get(lane_id))
            .and_then(|l| l.get("label"))
            .and_then(|v| v.as_str())
            .unwrap_or("CI")
    };

    let mut yaml = String::new();

    // Header
    yaml.push_str(&format!(
        r#"# AUTO-GENERATED by exohook ci emit github-actions
# Source: .config/exo/hooks.toml [{}.{}]
# Regenerate with: exohook ci emit github-actions
#
# This file is regenerated automatically when you run exohook commands.
# To disable auto-generation, add to .config/exo/hooks.toml:
#
#   [projections.github_actions]
#   enabled = false
#
# You can also delete this file and create your own .github/workflows/exo-ci.yml
# without the AUTO-GENERATED header - exohook will not overwrite it.

name: {}

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always

jobs:
"#,
        source_table,
        lane_id,
        yaml_double_quoted(lane_label)
    ));

    // Generate a job for each check
    let mut previous_job_id: Option<String> = None;
    for check in &checks {
        let needs = if parallel {
            None
        } else {
            previous_job_id.as_deref()
        };
        emit_job(&mut yaml, check, needs);
        previous_job_id = Some(github_actions_job_id(&check.id));
    }

    Ok(yaml)
}

fn workflow_parallel(doc: &DocumentMut, lane_id: &str, version: ConfigVersion) -> bool {
    let table_name = if version == ConfigVersion::V3 {
        "workflow"
    } else {
        "lane"
    };
    doc.get(table_name)
        .and_then(|table| table.get(lane_id))
        .and_then(|workflow| workflow.get("parallel"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

/// Emit a single job for a check.
fn emit_job(yaml: &mut String, check: &CheckSpec, needs: Option<&str>) {
    // Normalize check id to a valid GitHub Actions job key.
    let job_id = github_actions_job_id(&check.id);
    let needs_line = needs
        .map(|job_id| format!("    needs: {job_id}\n"))
        .unwrap_or_default();

    yaml.push_str(&format!(
        r#"  {}:
    name: {}
    runs-on: ubuntu-latest
{}    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          persist-credentials: false
"#,
        job_id,
        yaml_double_quoted(&check.label),
        needs_line
    ));

    // Rust setup
    if check.reqs.needs_rust {
        let components = if check.reqs.rust_components.is_empty() {
            String::new()
        } else {
            let mut comps: Vec<_> = check.reqs.rust_components.iter().collect();
            comps.sort();
            let comps_str: Vec<&str> = comps.iter().map(|s| s.as_str()).collect();
            format!(
                "\n        with:\n          components: {}",
                comps_str.join(", ")
            )
        };

        yaml.push_str(&format!(
            r#"
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable{}
"#,
            components
        ));

        // Cargo cache
        yaml.push_str(
            r#"
      - name: Cache Cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-
"#,
        );
    }

    // pnpm setup
    if check.reqs.needs_pnpm {
        yaml.push_str(
            r#"
      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9
"#,
        );
    }

    // Node setup
    if check.reqs.needs_node {
        let cache = if check.reqs.needs_pnpm { "pnpm" } else { "npm" };
        let cache_dependency_path = check
            .cwd
            .as_deref()
            .map(|cwd| {
                let lockfile_path = if check.reqs.needs_pnpm {
                    "pnpm-lock.yaml".to_string()
                } else {
                    cwd_child_path(cwd, "package-lock.json")
                };
                format!(
                    "\n          cache-dependency-path: {}",
                    yaml_double_quoted(&lockfile_path)
                )
            })
            .unwrap_or_default();
        yaml.push_str(&format!(
            r#"
      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: '22'
          cache: '{}'{}
"#,
            cache, cache_dependency_path
        ));

        // Install dependencies
        let install_cmd = if check.reqs.needs_pnpm {
            "pnpm install"
        } else {
            "npm ci"
        };
        let install_working_directory = check
            .cwd
            .as_deref()
            .map(|cwd| format!("        working-directory: {}\n", yaml_double_quoted(cwd)))
            .unwrap_or_default();
        yaml.push_str(&format!(
            r#"
      - name: Install dependencies
        run: {}
"#,
            install_cmd
        ));
        yaml.push_str(&install_working_directory);
    }

    // cargo-llvm-cov
    if check.reqs.needs_cargo_llvm_cov {
        yaml.push_str(
            r#"
      - name: Install cargo-llvm-cov
        uses: taiki-e/install-action@cargo-llvm-cov
"#,
        );
    }

    // Run the check command
    yaml.push_str(&format!(
        r#"
      - name: {}
        run: {}
"#,
        yaml_double_quoted(&check.label),
        yaml_double_quoted(&check.run)
    ));

    if let Some(cwd) = &check.cwd {
        yaml.push_str(&format!(
            "        working-directory: {}\n",
            yaml_double_quoted(cwd)
        ));
    }

    yaml.push('\n');
}

fn cwd_child_path(cwd: &str, child: &str) -> String {
    let cwd = cwd.trim_end_matches('/');
    if cwd.is_empty() {
        child.to_string()
    } else {
        format!("{cwd}/{child}")
    }
}

fn yaml_double_quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

/// Get the default output path for a CI target.
pub fn default_output_path(target: CiTarget) -> &'static str {
    match target {
        CiTarget::GitHubActions => ".github/workflows/exo-ci.yml",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> DocumentMut {
        r#"
[lane.ci]
label = "CI"
checks = ["test", "coverage"]

[check.test]
label = "Test"
run = "pnpm -r run test:unit"

[check.coverage]
label = "Coverage"
run = "cargo llvm-cov --workspace"
"#
        .parse()
        .unwrap()
    }

    fn sample_v3_doc() -> DocumentMut {
        r#"
version = 3

[workflow.ci]
label = "CI (HEAD)"
checks = ["test", "rust-coverage"]

[check.test]
label = "Test"
command = "pnpm -r run test:unit"

[check."rust-coverage"]
label = "Rust Coverage"
command = "cargo llvm-cov --workspace --lcov --output-path lcov.info"
"#
        .parse()
        .unwrap()
    }

    #[test]
    fn test_extract_checks() {
        let doc = sample_doc();
        let checks = extract_checks(&doc, "ci").unwrap();

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].id, "test");
        assert_eq!(checks[0].label, "Test");
        assert!(checks[0].reqs.needs_pnpm);
        assert!(checks[0].reqs.needs_node);
        assert!(!checks[0].reqs.needs_rust);

        assert_eq!(checks[1].id, "coverage");
        assert_eq!(checks[1].label, "Coverage");
        assert!(checks[1].reqs.needs_rust);
        assert!(checks[1].reqs.needs_cargo_llvm_cov);
    }

    #[test]
    fn test_extract_v3_checks() {
        let doc = sample_v3_doc();
        let checks = extract_checks(&doc, "ci").unwrap();

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].id, "test");
        assert_eq!(checks[0].label, "Test");
        assert_eq!(checks[0].run, "pnpm -r run test:unit");
        assert!(checks[0].reqs.needs_pnpm);

        assert_eq!(checks[1].id, "rust-coverage");
        assert_eq!(checks[1].label, "Rust Coverage");
        assert_eq!(
            checks[1].run,
            "cargo llvm-cov --workspace --lcov --output-path lcov.info"
        );
        assert!(checks[1].reqs.needs_cargo_llvm_cov);
    }

    #[test]
    fn test_extract_checks_rejects_unknown_config_versions() {
        let doc: DocumentMut = r#"
version = 99

[workflow.ci]
checks = []
"#
        .parse()
        .unwrap();

        let err = extract_checks(&doc, "ci").expect_err("unsupported version should fail");
        assert!(
            err.to_string()
                .contains("unsupported hooks.toml version 99")
        );
    }

    #[test]
    fn test_extract_legacy_rejects_duplicate_named_job_ids() {
        let doc: DocumentMut = r#"
[lane.ci]
checks = ["foo-bar", "foo_bar"]

[check."foo-bar"]
run = "echo dash"

[check.foo_bar]
run = "echo underscore"
"#
        .parse()
        .unwrap();

        let err = extract_checks(&doc, "ci").expect_err("duplicate job id should fail");
        assert!(
            err.to_string()
                .contains("duplicate GitHub Actions job id 'foo_bar'")
        );
    }

    #[test]
    fn test_extract_v3_inline_check_ids_avoid_normalized_named_check_collisions() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = [{ label = "Inline", command = "echo inline" }, "inline_0"]

[check.inline_0]
label = "Named Inline"
command = "echo named"
"#
        .parse()
        .unwrap();

        let checks = extract_checks(&doc, "ci").unwrap();
        assert_eq!(checks[0].id, "inline-0-1");
        assert_eq!(checks[1].id, "inline_0");

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        assert!(yaml.contains("  inline_0:\n"));
        assert!(yaml.contains("  inline_0_1:\n"));
    }

    #[test]
    fn test_extract_v3_rejects_duplicate_named_job_ids() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["foo-bar", "foo_bar"]

[check."foo-bar"]
command = "echo dash"

[check.foo_bar]
command = "echo underscore"
"#
        .parse()
        .unwrap();

        let err = extract_checks(&doc, "ci").expect_err("duplicate job id should fail");
        assert!(
            err.to_string()
                .contains("duplicate GitHub Actions job id 'foo_bar'")
        );
    }

    #[test]
    fn test_emit_github_actions_creates_separate_jobs() {
        let doc = sample_doc();
        let yaml = emit_github_actions(&doc, "ci").unwrap();

        // Workflow header
        assert!(yaml.contains(r#"name: "CI""#));
        assert!(yaml.contains("permissions:\n  contents: read\n"));

        // Separate jobs for each check
        assert!(yaml.contains("test:"));
        assert!(yaml.contains(r#"name: "Test""#));
        assert!(yaml.contains("pnpm -r run test:unit"));

        assert!(yaml.contains("coverage:"));
        assert!(yaml.contains(r#"name: "Coverage""#));
        assert!(yaml.contains("cargo llvm-cov --workspace"));
        assert_eq!(yaml.matches("persist-credentials: false").count(), 2);

        // No exohook build or validate (the old behavior)
        assert!(!yaml.contains("cargo build --release -p exohook"));
        assert!(!yaml.contains("exohook validate"));
    }

    #[test]
    fn test_emit_github_actions_from_v3_workflow() {
        let doc = sample_v3_doc();
        let yaml = emit_github_actions(&doc, "ci").unwrap();

        assert!(yaml.contains(r#"name: "CI (HEAD)""#));
        assert!(yaml.contains("test:"));
        assert!(yaml.contains(r#"name: "Test""#));
        assert!(yaml.contains("pnpm -r run test:unit"));
        assert!(yaml.contains("rust_coverage:"));
        assert!(yaml.contains(r#"name: "Rust Coverage""#));
        assert!(yaml.contains("cargo llvm-cov --workspace --lcov --output-path lcov.info"));
        assert!(yaml.contains(".github/workflows/exo-ci.yml"));
    }

    #[test]
    fn test_emit_github_actions_honors_sequential_v3_workflow() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
parallel = false
checks = ["first", "second", "third"]

[check.first]
command = "echo first"

[check.second]
command = "echo second"

[check.third]
command = "echo third"
"#
        .parse()
        .unwrap();

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        let first = yaml.find("  first:\n").expect("first job");
        let second = yaml.find("  second:\n").expect("second job");
        let third = yaml.find("  third:\n").expect("third job");
        let first_job = &yaml[first..second];
        let second_job = &yaml[second..third];
        let third_job = &yaml[third..];

        assert!(!first_job.contains("\n    needs:"));
        assert!(second_job.contains("\n    needs: first\n"));
        assert!(third_job.contains("\n    needs: second\n"));
    }

    #[test]
    fn test_emit_github_actions_allows_v3_projection_table() {
        let doc: DocumentMut = r#"
version = 3

[projections.github_actions]
ci = "gate"
enabled = true

[workflow.gate]
label = "Gate"
checks = ["test"]

[check.test]
command = "npm test"
"#
        .parse()
        .unwrap();

        let lane = get_ci_lane(&doc);
        assert_eq!(lane, "gate");

        let yaml = emit_github_actions(&doc, &lane).unwrap();
        assert!(yaml.contains("# Source: .config/exo/hooks.toml [workflow.gate]"));
        assert!(yaml.contains(r#"name: "Gate""#));
        assert!(yaml.contains("  test:\n"));
    }

    #[test]
    fn test_emit_github_actions_rejects_v3_files_placeholder() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["lint"]

[check.lint]
command = "echo {{files}}"
filters = ["crates/exohook/src/ci_emit.rs"]
cwd = "crates/exohook"
"#
        .parse()
        .unwrap();

        let err = emit_github_actions(&doc, "ci").expect_err("{{files}} should not be emitted");
        let err = format!("{err:#}");
        assert!(err.contains("cannot be emitted to CI because its command uses '{{files}}'"));
        assert!(err.contains("freeze a stale file list"));
    }

    #[test]
    fn test_emit_github_actions_quotes_dynamic_step_name_and_run_command() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["lint"]

[check.lint]
label = "Lint #1: package"
command = "echo packages/foo #1: bar"
"#
        .parse()
        .unwrap();

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        assert!(yaml.contains(r#"name: "Lint #1: package""#));
        assert!(yaml.contains(r#"run: "echo packages/foo #1: bar""#));
    }

    #[test]
    fn test_emit_github_actions_quotes_working_directory() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["lint"]

[check.lint]
command = "pnpm lint"
cwd = 'packages/foo #1: bar'
"#
        .parse()
        .unwrap();

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        assert!(yaml.contains(r#"working-directory: "packages/foo #1: bar""#));
        assert!(yaml.contains(r#"cache-dependency-path: "pnpm-lock.yaml""#));
        assert!(!yaml.contains(r#"packages/foo #1: bar/pnpm-lock.yaml"#));
        assert!(yaml.contains(
            "      - name: Install dependencies\n        run: pnpm install\n        working-directory: \"packages/foo #1: bar\"\n"
        ));
    }

    #[test]
    fn test_emit_github_actions_applies_cwd_to_npm_install_and_cache() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["test"]

[check.test]
command = "npm test"
cwd = "frontend"
"#
        .parse()
        .unwrap();

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        assert!(yaml.contains("          cache: 'npm'\n"));
        assert!(yaml.contains(r#"cache-dependency-path: "frontend/package-lock.json""#));
        assert!(yaml.contains(
            "      - name: Install dependencies\n        run: npm ci\n        working-directory: \"frontend\"\n"
        ));
    }

    #[test]
    fn test_emit_github_actions_sanitizes_v3_check_ids_for_job_keys() {
        let doc: DocumentMut = r#"
version = 3

[workflow.ci]
checks = ["docs.links", "lint/web", "1-start"]

[check."docs.links"]
command = "echo docs"

[check."lint/web"]
command = "echo lint"

[check."1-start"]
command = "echo start"
"#
        .parse()
        .unwrap();

        let yaml = emit_github_actions(&doc, "ci").unwrap();
        assert!(yaml.contains("  docs_links:\n"));
        assert!(yaml.contains("  lint_web:\n"));
        assert!(yaml.contains("  _1_start:\n"));
        assert!(!yaml.contains("  docs.links:\n"));
        assert!(!yaml.contains("  lint/web:\n"));
        assert!(!yaml.contains("  1-start:\n"));
    }

    #[test]
    fn test_emit_job_includes_toolchain_setup() {
        let doc = sample_doc();
        let yaml = emit_github_actions(&doc, "ci").unwrap();

        // The test job should have pnpm setup
        assert!(yaml.contains("uses: pnpm/action-setup@v4"));

        // The coverage job should have Rust and llvm-cov setup
        assert!(yaml.contains("uses: dtolnay/rust-toolchain@stable"));
        assert!(yaml.contains("uses: taiki-e/install-action@cargo-llvm-cov"));
    }

    #[test]
    fn test_get_ci_lane_default() {
        let doc: DocumentMut = "[lane.ci]\nchecks = []".parse().unwrap();
        assert_eq!(get_ci_lane(&doc), "ci");
    }

    #[test]
    fn test_get_ci_lane_from_projection() {
        let doc: DocumentMut = r#"
[projections.github_actions]
ci = "gate"
"#
        .parse()
        .unwrap();
        assert_eq!(get_ci_lane(&doc), "gate");
    }

    #[test]
    fn test_default_github_actions_output_path_is_canonical_exo_ci() {
        assert_eq!(
            default_output_path(CiTarget::GitHubActions),
            ".github/workflows/exo-ci.yml"
        );
    }

    #[test]
    fn test_yaml_double_quoted_escapes_special_characters() {
        assert_eq!(
            yaml_double_quoted("packages/quoted \"dir\" \\ name"),
            r#""packages/quoted \"dir\" \\ name""#
        );
    }
}
