use anyhow::{Context, Result, anyhow};
use indexmap::IndexMap;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::fileset::FilesetScope;

// ============================================================================
// Config Version Detection
// ============================================================================

/// Detected configuration format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigVersion {
    /// Legacy format (version = 1 or missing)
    V1,
    /// Current format (version = 2)
    V2,
    /// New simplified format (version = 3) - RFC 00215
    V3,
    /// Unknown version number
    Unknown(i64),
}

impl ConfigVersion {
    /// Parse the version from a hooks.toml document.
    pub fn from_doc(doc: &DocumentMut) -> Self {
        match doc.get("version").and_then(|v| v.as_integer()) {
            None | Some(1) => ConfigVersion::V1,
            Some(2) => ConfigVersion::V2,
            Some(3) => ConfigVersion::V3,
            Some(n) => ConfigVersion::Unknown(n),
        }
    }

    /// Returns true if this is a deprecated version that should emit a warning.
    pub fn is_deprecated(&self) -> bool {
        matches!(self, ConfigVersion::V1 | ConfigVersion::V2)
    }

    /// Returns the deprecation warning message, if applicable.
    pub fn deprecation_warning(&self) -> Option<&'static str> {
        match self {
            ConfigVersion::V1 => {
                Some("hooks.toml version 1 is deprecated. Run `exohook migrate v3` to upgrade.")
            }
            ConfigVersion::V2 => {
                Some("hooks.toml version 2 is deprecated. Run `exohook migrate v3` to upgrade.")
            }
            _ => None,
        }
    }
}

// ============================================================================
// V3 Schema Types (RFC 00215)
// ============================================================================

/// The type of git hook being executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    PreCommit,
    PrePush,
    CommitMsg,
    PreMergeCommit,
    /// Manual invocation via `exohook run <workflow>`
    Manual,
}

/// Whether a check observes or mutates files.
///
/// - `Observe` checks are read-only (linters, type checkers) - safe for continuous run.
/// - `Mutate` checks modify files (formatters, codemods) - need concurrency guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckCategory {
    #[default]
    Observe,
    Mutate,
}

impl HookType {
    /// Get the inferred file scope for this hook type.
    /// - PreCommit → Staged files
    /// - PrePush → Committed files not yet pushed
    /// - CommitMsg/PreMergeCommit → Staged (no files, but consistent)
    /// - Manual → All uncommitted files
    pub fn inferred_scope(&self) -> FilesetScope {
        match self {
            HookType::PreCommit => FilesetScope::Staged,
            HookType::PrePush => FilesetScope::CommittedNotPushed,
            HookType::CommitMsg => FilesetScope::Staged,
            HookType::PreMergeCommit => FilesetScope::Staged,
            HookType::Manual => FilesetScope::Uncommitted,
        }
    }

    /// Parse hook type from git hook name string.
    pub fn from_hook_name(name: &str) -> Option<Self> {
        match name {
            "pre-commit" | "pre_commit" => Some(HookType::PreCommit),
            "pre-push" | "pre_push" => Some(HookType::PrePush),
            "commit-msg" | "commit_msg" => Some(HookType::CommitMsg),
            "pre-merge-commit" | "pre_merge_commit" => Some(HookType::PreMergeCommit),
            _ => None,
        }
    }
}

/// Runtime context for check execution.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub hook_type: HookType,
    pub is_interactive: bool, // TTY detected
    pub force_fix: bool,      // --fix flag
    pub force_no_fix: bool,   // --no-fix flag
}

impl ExecutionContext {
    /// Create a new execution context.
    pub fn new(hook_type: HookType, is_interactive: bool) -> Self {
        Self {
            hook_type,
            is_interactive,
            force_fix: false,
            force_no_fix: false,
        }
    }

    /// Determine if a check should run its fix command.
    ///
    /// Logic:
    /// - If check.category is observe, never fix
    /// - If --no-fix flag, never fix
    /// - If --fix flag, always fix (if check supports it)
    /// - Otherwise: fix only for pre_commit in interactive mode
    pub fn should_fix(&self, check: &CheckV3) -> bool {
        if check.category != CheckCategory::Mutate {
            return false;
        }
        if self.force_no_fix {
            return false;
        }
        if self.force_fix {
            return true;
        }

        match self.hook_type {
            HookType::PreCommit => self.is_interactive,
            HookType::Manual => true,
            _ => false, // PrePush, CommitMsg, etc. never auto-fix
        }
    }

    /// Determine if fixed files should be restaged.
    /// Only restage for pre_commit hooks (where we're modifying staged files).
    pub fn should_restage(&self, check: &CheckV3) -> bool {
        self.should_fix(check) && self.hook_type == HookType::PreCommit
    }
}

/// V3 configuration root structure.
///
/// The v3 format puts `[hooks]` first and infers context from hook type.
/// Lanes are optional (for power users who need custom workflows).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigV3 {
    /// Config format version (must be 3)
    pub version: i64,

    /// Hook mappings: which checks run on which git hooks
    #[serde(default)]
    pub hooks: HooksV3,

    /// Check definitions
    #[serde(default)]
    pub check: IndexMap<String, CheckV3>,

    /// Optional workflow definitions (power user feature)
    #[serde(default)]
    pub workflow: IndexMap<String, WorkflowV3>,

    /// Optional defaults section
    #[serde(default)]
    pub defaults: DefaultsV3,

    /// Optional projection metadata, such as `[projections.github_actions]`.
    #[serde(default)]
    pub projections: toml::Table,
}

/// Git hook mappings in v3 format.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksV3 {
    /// Workflow to run on pre-commit (staged files, fix+restage)
    #[serde(default)]
    pub pre_commit: Option<String>,

    /// Workflow to run on pre-push (committed-not-pushed, verify only)
    #[serde(default)]
    pub pre_push: Option<String>,

    /// Workflow to run on commit-msg hook
    #[serde(default)]
    pub commit_msg: Option<String>,

    /// Workflow to run on pre-merge-commit
    #[serde(default)]
    pub pre_merge_commit: Option<String>,
}

/// A check reference in v3 format.
///
/// Can be either a string (reference to `[check.id]`) or an inline definition.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CheckRefV3 {
    /// Reference to a named check: `"fmt"`
    Ref(String),
    /// Inline check definition
    Inline(CheckV3),
}

/// Check definition in v3 format.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckV3 {
    /// Human-readable label for the check
    pub label: Option<String>,

    /// Working directory (relative to repo root) to run this check from.
    ///
    /// When set, `{{files}}` paths are rebased to be relative to this directory,
    /// and the check command is executed with this as its current directory.
    pub cwd: Option<String>,

    /// Shell command to run (mutually exclusive with `tool`)
    pub command: Option<String>,

    /// Exo tool to invoke (mutually exclusive with `command`)
    /// Format: "exo.docs.links.check" or similar
    pub tool: Option<String>,

    /// Glob patterns for file filtering
    #[serde(default)]
    pub filters: Vec<String>,

    /// Whether this check can fix issues
    /// When mutate, the check may modify files. Behavior depends on context:
    /// - pre_commit: fix and restage
    /// - pre_push/CI: verify only (no fixing)
    #[serde(default)]
    pub category: CheckCategory,

    /// Explicit command to run for fixing (if different from main command)
    pub fix_command: Option<String>,

    /// Skip silently if no files match filters (default: true when filters present)
    pub skip_if_empty: Option<bool>,

    /// Timeout in seconds (overrides defaults.timeout_seconds)
    pub timeout_seconds: Option<u64>,
}

/// Custom workflow definition (power user feature).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowV3 {
    /// Human-readable label
    pub label: Option<String>,

    /// Checks to run in this workflow
    #[serde(default)]
    pub checks: Vec<CheckRefV3>,

    /// Whether to run checks in parallel (default: true)
    #[serde(default = "default_true")]
    pub parallel: bool,

    /// Override the default scope
    /// Values: "staged", "uncommitted", "committed_not_pushed", "head", "all"
    pub scope: Option<String>,

    /// Override fix behavior
    /// Values: "auto" (context-aware), "always", "never"
    pub fix_policy: Option<String>,
}

/// Defaults section in v3 format.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultsV3 {
    /// Timeout for check execution
    pub timeout_seconds: Option<u64>,

    /// Run checks in parallel (default: true)
    pub parallel: Option<bool>,

    /// Silence warning threshold
    #[serde(rename = "silence-warning-seconds")]
    pub silence_warning_seconds: Option<u64>,

    /// Force simple (non-PTY) output
    #[serde(rename = "simple-output")]
    pub simple_output: Option<bool>,

    /// Show parallel output interleaved
    #[serde(rename = "show-parallel-output")]
    pub show_parallel_output: Option<bool>,
}

fn default_true() -> bool {
    true
}

impl ConfigV3 {
    /// Parse a v3 configuration from a TOML string.
    pub fn parse(content: &str) -> Result<Self> {
        toml::from_str(content).map_err(|e| {
            // toml::de::Error's Display includes line/column info — preserve it
            anyhow!(
                "failed to parse v3 config\n\n{e}\n\n\
                 → Check .config/exo/hooks.toml for syntax errors\n\
                 → Run `exohook config validate` after fixing"
            )
        })
    }

    /// Parse a v3 configuration from a DocumentMut.
    pub fn from_doc(doc: &DocumentMut) -> Result<Self> {
        Self::parse(&doc.to_string())
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.version != 3 {
            return Err(anyhow!(
                "expected version = 3, got version = {}",
                self.version
            ));
        }

        // Validate checks
        for (id, check) in &self.check {
            check.validate(id)?;
        }

        // Validate hook workflow references
        let hook_refs = [
            ("pre_commit", &self.hooks.pre_commit),
            ("pre_push", &self.hooks.pre_push),
            ("commit_msg", &self.hooks.commit_msg),
            ("pre_merge_commit", &self.hooks.pre_merge_commit),
        ];
        for (hook_name, workflow_name) in hook_refs {
            if let Some(workflow_name) = workflow_name.as_deref()
                && !self.workflow.contains_key(workflow_name)
            {
                let available: Vec<&str> = self.workflow.keys().map(String::as_str).collect();
                return Err(anyhow!(
                    "hooks.{hook_name} references unknown workflow '{workflow_name}'\n\
                     → Available workflows: {available:?}\n\
                     → Define [workflow.{workflow_name}] in .config/exo/hooks.toml"
                ));
            }
        }

        // Validate workflows
        for (id, workflow) in &self.workflow {
            self.validate_workflow_refs(id, &workflow.checks)?;
        }

        Ok(())
    }

    fn validate_workflow_refs(&self, workflow_id: &str, refs: &[CheckRefV3]) -> Result<()> {
        for (i, check_ref) in refs.iter().enumerate() {
            match check_ref {
                CheckRefV3::Ref(name) => {
                    if !self.check.contains_key(name) {
                        let available: Vec<&str> = self.check.keys().map(String::as_str).collect();
                        return Err(anyhow!(
                            "workflow.{workflow_id}.checks[{i}]: unknown check '{name}'\n\
                             → Available checks: {available:?}\n\
                             → Define [check.\"{name}\"] in .config/exo/hooks.toml"
                        ));
                    }
                }
                CheckRefV3::Inline(check) => {
                    check.validate(&format!("workflow.{workflow_id}.checks[{i}]"))?;
                }
            }
        }
        Ok(())
    }
}

impl CheckV3 {
    /// Validate a check definition.
    pub fn validate(&self, context: &str) -> Result<()> {
        // Must have exactly one of command or tool
        match (&self.command, &self.tool) {
            (None, None) => {
                return Err(anyhow!(
                    "check '{context}': must specify either 'command' or 'tool'"
                ));
            }
            (Some(_), Some(_)) => {
                return Err(anyhow!(
                    "check '{context}': cannot specify both 'command' and 'tool'"
                ));
            }
            _ => {}
        }

        // fix_command only makes sense with category = "mutate"
        if self.fix_command.is_some() && self.category != CheckCategory::Mutate {
            return Err(anyhow!(
                "check '{context}': 'fix_command' requires 'category = \"mutate\"'"
            ));
        }

        if let Some(cwd) = &self.cwd {
            let cwd_path = std::path::Path::new(cwd);
            if cwd.trim().is_empty() {
                return Err(anyhow!("check '{context}': 'cwd' must not be empty"));
            }
            if cwd_path.is_absolute() {
                return Err(anyhow!(
                    "check '{context}': 'cwd' must be a repo-root-relative path"
                ));
            }
            if cwd_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(anyhow!("check '{context}': 'cwd' must not contain '..'"));
            }
        }

        Ok(())
    }

    /// Returns whether skip_if_empty should be true for this check.
    /// Default is true when filters are present, false otherwise.
    pub fn effective_skip_if_empty(&self) -> bool {
        self.skip_if_empty.unwrap_or(!self.filters.is_empty())
    }
}

/// Runner configuration parsed from hooks.toml [defaults] section.
///
/// These settings control the behavior of the check runner, particularly
/// around output and timing feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerConfig {
    /// Seconds of silence before showing a warning message.
    /// Default: 30. Set to 0 to disable silence warnings.
    pub silence_warning_seconds: u64,

    /// Run checks in parallel.
    /// Default: true.
    pub parallel: bool,

    /// Force pipe-based execution instead of PTY.
    /// Default: false. Useful for CI or when PTY causes issues.
    pub simple_output: bool,

    /// Show parallel check output interleaved.
    /// Default: false. When true, streams output from parallel checks.
    pub show_parallel_output: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            silence_warning_seconds: 30,
            parallel: true,
            simple_output: false,
            show_parallel_output: false,
        }
    }
}

/// Parse runner configuration from hooks.toml [defaults] section.
///
/// Reads the following hyphenated keys:
/// - `silence-warning-seconds`: u64 (default 30, 0 = disabled)
/// - `simple-output`: bool (default false)
/// - `show-parallel-output`: bool (default false)
pub fn parse_runner_config(doc: &DocumentMut) -> RunnerConfig {
    let mut config = RunnerConfig::default();

    let Some(defaults) = doc.get("defaults").and_then(|d| d.as_table()) else {
        return config;
    };

    // Parse silence-warning-seconds
    if let Some(val) = defaults.get("silence-warning-seconds")
        && let Some(n) = val.as_integer()
    {
        config.silence_warning_seconds = n.max(0) as u64;
    }

    // Parse simple-output
    if let Some(val) = defaults.get("simple-output")
        && let Some(b) = val.as_bool()
    {
        config.simple_output = b;
    }

    // Parse show-parallel-output
    if let Some(val) = defaults.get("show-parallel-output")
        && let Some(b) = val.as_bool()
    {
        config.show_parallel_output = b;
    }

    config
}

pub(crate) fn hooks_config_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to determine current dir")?;
    Ok(cwd.join(".config/exo/hooks.toml"))
}

pub(crate) fn read_hooks_doc(path: &Path) -> Result<DocumentMut> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    content.parse::<DocumentMut>().map_err(|e| {
        // toml_edit::TomlError's Display includes line/column info — preserve it
        anyhow!(
            "failed to parse TOML at {}\n\n{e}\n\n\
             → Check the file for syntax errors\n\
             → Run `exohook config validate` after fixing",
            path.display()
        )
    })
}

pub(crate) fn write_hooks_doc(path: &Path, doc: &DocumentMut) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Extract inline check definitions from hooks arrays to [check.*] tables.
pub fn extract_inline_checks(
    doc: &toml_edit::DocumentMut,
    prefix: &str,
) -> Result<(toml_edit::DocumentMut, Vec<String>)> {
    use std::collections::HashSet;
    use toml_edit::{Item, Table};

    let mut result = doc.clone();
    let mut changes = Vec::new();

    let version = result
        .get("version")
        .and_then(|v| v.as_integer())
        .unwrap_or(0);
    if version != 3 {
        return Err(anyhow!(
            "extract only works on v3 configs (found version {})",
            version
        ));
    }

    if result.get("check").is_none() {
        result["check"] = Item::Table(Table::new());
    }

    let mut existing_ids: HashSet<String> = HashSet::new();
    if let Some(check_root) = result.get("check").and_then(Item::as_table) {
        for (id, _) in check_root.iter() {
            existing_ids.insert(id.to_string());
        }
    }

    let Some(workflow_root) = result.get_mut("workflow").and_then(Item::as_table_mut) else {
        return Ok((result, changes));
    };

    let mut extracted: Vec<(String, Table, String)> = Vec::new();
    let mut inline_index = 0usize;

    for (workflow_id, workflow_item) in workflow_root.iter_mut() {
        let Some(workflow_table) = workflow_item.as_table_mut() else {
            continue;
        };
        let Some(checks_item) = workflow_table.get_mut("checks") else {
            continue;
        };
        let Some(checks_array) = checks_item.as_array_mut() else {
            continue;
        };

        for value in checks_array.iter_mut() {
            let inline: toml_edit::InlineTable = match value.as_inline_table() {
                Some(inline) => inline.clone(),
                None => continue,
            };

            let label = inline.get("label").and_then(Value::as_str).unwrap_or("");
            let mut base = slugify_check_id(label);
            if base.is_empty() {
                base = format!("inline-{inline_index}");
                inline_index += 1;
            }
            if !prefix.is_empty() {
                base = format!("{prefix}-{base}");
            }

            let mut id = base.clone();
            let mut suffix = 0usize;
            while existing_ids.contains(&id) {
                suffix += 1;
                id = format!("{base}-{suffix}");
            }
            existing_ids.insert(id.clone());

            let mut check_table = Table::new();
            for (key, value) in inline.iter() {
                let value: toml_edit::Value = value.clone();
                check_table.insert(key, Item::Value(value));
            }

            let id_value = match value_from_toml(&format!("\"{}\"", escape_toml_string(&id)))? {
                Item::Value(value) => value,
                _ => return Err(anyhow!("internal error: expected string value")),
            };
            *value = id_value;

            extracted.push((id.clone(), check_table, workflow_id.to_string()));
        }
    }

    if !extracted.is_empty() {
        let Some(check_root) = result.get_mut("check").and_then(Item::as_table_mut) else {
            return Err(anyhow!("internal error: missing [check] table"));
        };
        for (id, check_table, workflow_id) in extracted {
            check_root.insert(&id, Item::Table(check_table));
            changes.push(format!(
                "Extracted inline check '{id}' from workflow '{workflow_id}'."
            ));
        }
    }

    Ok((result, changes))
}

fn slugify_check_id(label: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

pub(crate) fn canonical_doc_baseline() -> Result<DocumentMut> {
    let mut doc = DocumentMut::new();
    doc["version"] = value_from_toml("2")?;

    let defaults = doc.entry("defaults").or_insert(Item::Table(Table::new()));
    defaults["non_interactive"] = value_from_toml("true")?;
    defaults["timeout_seconds"] = value_from_toml("900")?;
    defaults["chunk_target_bytes"] = value_from_toml("48000")?;
    defaults["chunk_id"] = value_from_toml("\"index\"")?;

    let projections = doc
        .entry("projections")
        .or_insert(Item::Table(Table::new()));
    projections
        .as_table_mut()
        .ok_or_else(|| anyhow!("[projections] must be a table"))?
        .insert("git_hooks", Item::Table(Table::new()));
    projections["git_hooks"]["pre_commit"] = value_from_toml("\"coherence\"")?;
    projections["git_hooks"]["pre_push"] = value_from_toml("\"gate\"")?;

    doc.entry("lane").or_insert(Item::Table(Table::new()));
    doc.entry("check").or_insert(Item::Table(Table::new()));

    Ok(doc)
}

pub(crate) fn insert_lane(
    doc: &mut DocumentMut,
    id: &str,
    scope_toml: &str,
    checks_toml: &str,
) -> Result<()> {
    let lane_root = doc
        .entry("lane")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[lane] must be a table"))?;
    lane_root.insert(id, Item::Table(Table::new()));

    let lane = lane_root
        .get_mut(id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow!("internal error: failed to create lane '{id}'"))?;
    lane["scope"] = value_from_toml(scope_toml)?;
    lane["checks"] = value_from_toml(checks_toml)?;
    Ok(())
}

pub(crate) fn insert_check_run(
    doc: &mut DocumentMut,
    id: &str,
    label: &str,
    input_mode: &str,
    run: &str,
) -> Result<()> {
    let check_root = doc
        .entry("check")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[check] must be a table"))?;
    check_root.insert(id, Item::Table(Table::new()));

    let check = check_root
        .get_mut(id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow!("internal error: failed to create check '{id}'"))?;
    check["label"] = value_from_toml(&format!("\"{label}\""))?;
    check["input_mode"] = value_from_toml(&format!("\"{input_mode}\""))?;
    check["run"] = value_from_toml(&format!("\"{}\"", escape_toml_string(run)))?;
    Ok(())
}

pub(crate) fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn value_from_toml(s: &str) -> Result<Item> {
    let v = s
        .parse::<Value>()
        .with_context(|| format!("invalid TOML value: {s}"))?;
    Ok(Item::Value(v))
}

pub(crate) fn set_lane_parallel(doc: &mut DocumentMut, lane_id: &str, enabled: bool) -> Result<()> {
    let lane = get_lane_table_mut(doc, lane_id)
        .ok_or_else(|| anyhow!("internal error: lane '{lane_id}' missing"))?;
    lane["parallel"] = value_from_toml(if enabled { "true" } else { "false" })?;
    Ok(())
}

pub(crate) fn set_default_field(doc: &mut DocumentMut, field: &str, toml: &str) -> Result<()> {
    let defaults = doc.entry("defaults").or_insert(Item::Table(Table::new()));
    defaults[field] = value_from_toml(toml)?;
    Ok(())
}

pub(crate) fn unset_default_field(doc: &mut DocumentMut, field: &str) -> Result<()> {
    let Some(defaults) = doc.get_mut("defaults").and_then(Item::as_table_mut) else {
        return Ok(());
    };
    defaults.remove(field);
    Ok(())
}

pub(crate) fn set_check_field(
    doc: &mut DocumentMut,
    id: &str,
    field: &str,
    toml: &str,
) -> Result<()> {
    let check = get_check_table_mut(doc, id).ok_or_else(|| anyhow!("unknown check id '{id}'"))?;
    check[field] = value_from_toml(toml)?;
    Ok(())
}

pub(crate) fn set_lane_field(
    doc: &mut DocumentMut,
    id: &str,
    field: &str,
    toml: &str,
) -> Result<()> {
    let lane = get_lane_table_mut(doc, id).ok_or_else(|| anyhow!("unknown lane id '{id}'"))?;
    let canonical_field = if field == "fileset" { "scope" } else { field };
    lane[canonical_field] = value_from_toml(toml)?;
    Ok(())
}

pub(crate) fn set_override_field(
    doc: &mut DocumentMut,
    lane_id: &str,
    check_id: &str,
    field: &str,
    toml: &str,
) -> Result<()> {
    set_override_field_canonical(doc, lane_id, check_id, field, toml)?;

    Ok(())
}

pub(crate) fn get_lane_table_mut<'a>(doc: &'a mut DocumentMut, id: &str) -> Option<&'a mut Table> {
    if let Some(lane_root) = doc.get_mut("lane").and_then(Item::as_table_mut) {
        return lane_root.get_mut(id).and_then(Item::as_table_mut);
    }
    None
}

pub(crate) fn set_override_field_canonical(
    doc: &mut DocumentMut,
    lane_id: &str,
    check_id: &str,
    field: &str,
    toml: &str,
) -> Result<()> {
    let lane =
        get_lane_table_mut(doc, lane_id).ok_or_else(|| anyhow!("unknown lane id '{lane_id}'"))?;

    let overrides_item = lane.entry("overrides").or_insert(Item::Table(Table::new()));
    let overrides = overrides_item
        .as_table_mut()
        .ok_or_else(|| anyhow!("lane '{lane_id}' overrides must be a table"))?;

    if !overrides.contains_key(check_id) {
        overrides.insert(check_id, Item::Table(Table::new()));
    }
    let ov = overrides
        .get_mut(check_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow!("lane '{lane_id}' overrides.{check_id} must be a table"))?;
    ov[field] = value_from_toml(toml)?;
    Ok(())
}

pub(crate) fn remove_lane(doc: &mut DocumentMut, id: &str) -> Result<()> {
    let Some(lanes) = doc.get_mut("lane").and_then(Item::as_table_mut) else {
        return Err(anyhow!("no [lane] section found"));
    };
    if lanes.remove(id).is_none() {
        return Err(anyhow!("unknown lane id '{id}'"));
    }
    Ok(())
}

pub(crate) fn remove_check(doc: &mut DocumentMut, id: &str) -> Result<()> {
    let Some(checks) = doc.get_mut("check").and_then(Item::as_table_mut) else {
        return Err(anyhow!("no [check] section found"));
    };
    if checks.remove(id).is_none() {
        return Err(anyhow!("unknown check id '{id}'"));
    }
    Ok(())
}

pub(crate) fn remove_override(doc: &mut DocumentMut, lane_id: &str, check_id: &str) -> Result<()> {
    let lane =
        get_lane_table_mut(doc, lane_id).ok_or_else(|| anyhow!("unknown lane id '{lane_id}'"))?;
    let Some(overrides) = lane.get_mut("overrides").and_then(Item::as_table_mut) else {
        return Err(anyhow!("lane '{lane_id}' has no overrides"));
    };
    if overrides.remove(check_id).is_none() {
        return Err(anyhow!(
            "override not found for lane '{lane_id}' check '{check_id}'"
        ));
    }
    Ok(())
}

fn find_by_id<'a>(item: Option<&'a Item>, id: &str) -> Option<&'a Table> {
    let aot = item?.as_array_of_tables()?;
    aot.iter()
        .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
}

pub(crate) fn toml_string_array(values: &[String]) -> String {
    let mut out = String::from("[");
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        // Minimal escaping for TOML strings.
        out.push_str(&v.replace('\\', "\\\\").replace('"', "\\\""));
        out.push('"');
    }
    out.push(']');
    out
}

pub(crate) fn iter_lanes(doc: &DocumentMut) -> Result<Vec<(String, &Table)>> {
    if let Some(lane_root) = doc.get("lane").and_then(Item::as_table) {
        let mut out: Vec<(String, &Table)> = Vec::new();
        for (id, item) in lane_root.iter() {
            let id = id.to_string();
            let t = item
                .as_table()
                .ok_or_else(|| anyhow!("[lane.{id}] must be a table"))?;
            out.push((id, t));
        }
        return Ok(out);
    }

    if let Some(lanes) = doc.get("lanes").and_then(Item::as_array_of_tables) {
        let mut out: Vec<(String, &Table)> = Vec::new();
        for lane in lanes.iter() {
            let id = lane
                .get("id")
                .and_then(Item::as_str)
                .ok_or_else(|| anyhow!("each [[lanes]] must have string id"))?;
            out.push((id.to_string(), lane));
        }
        return Ok(out);
    }

    Ok(Vec::new())
}

pub(crate) fn iter_checks(doc: &DocumentMut) -> Result<Vec<(String, &Table)>> {
    if let Some(check_root) = doc.get("check").and_then(Item::as_table) {
        let mut out: Vec<(String, &Table)> = Vec::new();
        for (id, item) in check_root.iter() {
            let id = id.to_string();
            let t = item
                .as_table()
                .ok_or_else(|| anyhow!("[check.{id}] must be a table"))?;
            out.push((id, t));
        }
        return Ok(out);
    }

    if let Some(checks) = doc.get("checks").and_then(Item::as_array_of_tables) {
        let mut out: Vec<(String, &Table)> = Vec::new();
        for check in checks.iter() {
            let id = check
                .get("id")
                .and_then(Item::as_str)
                .ok_or_else(|| anyhow!("each [[checks]] must have string id"))?;
            out.push((id.to_string(), check));
        }
        return Ok(out);
    }

    Ok(Vec::new())
}

pub(crate) fn get_lane_table<'a>(doc: &'a DocumentMut, id: &str) -> Option<&'a Table> {
    if let Some(lane_root) = doc.get("lane").and_then(Item::as_table) {
        return lane_root.get(id).and_then(Item::as_table);
    }
    find_by_id(doc.get("lanes"), id)
}

pub(crate) fn get_check_table<'a>(doc: &'a DocumentMut, id: &str) -> Option<&'a Table> {
    if let Some(check_root) = doc.get("check").and_then(Item::as_table) {
        return check_root.get(id).and_then(Item::as_table);
    }
    find_by_id(doc.get("checks"), id)
}

pub(crate) fn get_check_table_mut<'a>(doc: &'a mut DocumentMut, id: &str) -> Option<&'a mut Table> {
    if let Some(check_root) = doc.get_mut("check").and_then(Item::as_table_mut) {
        return check_root.get_mut(id).and_then(Item::as_table_mut);
    }
    None
}

pub(crate) fn lane_override_table<'a>(
    doc: &'a DocumentMut,
    lane_id: &str,
    check_id: &str,
) -> Option<&'a Table> {
    // Canonical: [lane.<id>.overrides.<check_id>]
    // Note: toml_edit may represent dotted subtables in a "flattened" way under the parent
    // table to preserve formatting/order, so we attempt multiple access patterns.

    // 1) Nested access (common case): lane.<id>.overrides.<check_id>
    if let Some(lane_root) = doc.get("lane").and_then(Item::as_table) {
        if let Some(lane_tbl) = lane_root.get(lane_id).and_then(Item::as_table) {
            if let Some(ov) = lane_tbl
                .get("overrides")
                .and_then(Item::as_table)
                .and_then(|t| t.get(check_id))
                .and_then(Item::as_table)
            {
                return Some(ov);
            }

            // 2) Legacy: [[...overrides]] array-of-tables on the lane.
            if let Some(overrides) = lane_tbl.get("overrides").and_then(Item::as_array_of_tables) {
                for ov in overrides.iter() {
                    if ov.get("check").and_then(Item::as_str) == Some(check_id) {
                        return Some(ov);
                    }
                }
            }
        }

        // 3) Flattened dotted keys under [lane]
        //    e.g. keys like "coherence.overrides" or "coherence.overrides.fix".
        let dotted_overrides = format!("{lane_id}.overrides");
        if let Some(ov_root) = lane_root.get(&dotted_overrides).and_then(Item::as_table)
            && let Some(ov) = ov_root.get(check_id).and_then(Item::as_table)
        {
            return Some(ov);
        }

        let dotted_override = format!("{lane_id}.overrides.{check_id}");
        if let Some(ov) = lane_root.get(&dotted_override).and_then(Item::as_table) {
            return Some(ov);
        }
    }

    None
}

pub(crate) fn is_legacy_format(doc: &DocumentMut) -> bool {
    doc.get("lanes")
        .and_then(Item::as_array_of_tables)
        .is_some()
        || doc
            .get("checks")
            .and_then(Item::as_array_of_tables)
            .is_some()
}

pub(crate) fn canonicalize_doc_if_needed(doc: &DocumentMut) -> Result<DocumentMut> {
    if !is_legacy_format(doc) {
        return Ok(doc.clone());
    }

    let mut out = canonical_doc_baseline()?;

    // Preserve defaults/projections if present (best-effort).
    if let Some(defaults) = doc.get("defaults") {
        out["defaults"] = defaults.clone();
    }
    if let Some(projections) = doc.get("projections") {
        out["projections"] = projections.clone();
    }

    // Migrate lanes.
    if let Some(lanes) = doc.get("lanes").and_then(Item::as_array_of_tables) {
        for lane in lanes.iter() {
            let id = lane
                .get("id")
                .and_then(Item::as_str)
                .ok_or_else(|| anyhow!("each [[lanes]] must have string id"))?;

            let mut lane_out = Table::new();

            for (k, v) in lane.iter() {
                if k == "id" {
                    continue;
                }

                if k == "fileset" {
                    lane_out.insert("scope", v.clone());
                    continue;
                }

                if k == "overrides" {
                    let Some(ovs) = v.as_array_of_tables() else {
                        return Err(anyhow!("lane '{id}' overrides must be an array of tables"));
                    };

                    let mut ov_root = Table::new();
                    for ov in ovs.iter() {
                        let check = ov
                            .get("check")
                            .and_then(Item::as_str)
                            .ok_or_else(|| anyhow!("lane '{id}' override missing check id"))?;
                        let mut ov_table = Table::new();
                        for (ok, ov_item) in ov.iter() {
                            if ok == "check" {
                                continue;
                            }
                            ov_table.insert(ok, ov_item.clone());
                        }
                        ov_root.insert(check, Item::Table(ov_table));
                    }
                    lane_out.insert("overrides", Item::Table(ov_root));
                    continue;
                }

                lane_out.insert(k, v.clone());
            }

            out["lane"][id] = Item::Table(lane_out);
        }
    }

    // Migrate checks.
    if let Some(checks) = doc.get("checks").and_then(Item::as_array_of_tables) {
        for check in checks.iter() {
            let id = check
                .get("id")
                .and_then(Item::as_str)
                .ok_or_else(|| anyhow!("each [[checks]] must have string id"))?;
            let mut check_table = check.clone();
            check_table.remove("id");

            // Prefer run if argv is bash -lc <cmd>
            if check_table.get("run").is_none()
                && let Some(argv) = check_table.get("argv").and_then(Item::as_array)
                && argv.len() == 3
                && argv.get(0).and_then(|i| i.as_str()) == Some("bash")
                && argv.get(1).and_then(|i| i.as_str()) == Some("-lc")
                && let Some(cmd) = argv.get(2).and_then(|i| i.as_str())
            {
                let cmd = cmd.to_string();
                check_table.remove("argv");
                check_table.insert(
                    "run",
                    value_from_toml(&format!("\"{}\"", escape_toml_string(&cmd)))?,
                );
            }

            out["check"][id] = Item::Table(check_table);
        }
    }

    Ok(out)
}

pub fn migrate_v2_to_v3(doc: &DocumentMut) -> Result<String> {
    let source = canonicalize_doc_if_needed(doc)?;
    if ConfigVersion::from_doc(&source) != ConfigVersion::V2 {
        return Err(anyhow!(
            "expected hooks.toml version = 2, got {:?}",
            ConfigVersion::from_doc(&source)
        ));
    }

    let mut out = DocumentMut::new();
    out["version"] = value_from_toml("3")?;
    out.entry("hooks").or_insert(Item::Table(Table::new()));
    out.entry("workflow").or_insert(Item::Table(Table::new()));
    out.entry("check").or_insert(Item::Table(Table::new()));

    let mut pre_commit_workflow: Option<String> = None;
    let mut pre_push_workflow: Option<String> = None;
    let mut lane_parallel: Vec<bool> = Vec::new();

    let Some(workflow_root) = out.get_mut("workflow").and_then(Item::as_table_mut) else {
        return Err(anyhow!("internal error: failed to create [workflow]"));
    };

    for (lane_id, lane_table) in iter_lanes(&source)? {
        let checks = lane_table
            .get("checks")
            .and_then(Item::as_array)
            .ok_or_else(|| anyhow!("lane '{lane_id}' missing checks array"))?;

        match lane_id.as_str() {
            "coherence" => {
                pre_commit_workflow = Some(lane_id.clone());
            }
            "gate" => {
                pre_push_workflow = Some(lane_id.clone());
            }
            _ => {
                if pre_commit_workflow.is_none() {
                    pre_commit_workflow = Some(lane_id.clone());
                }
            }
        }

        let mut check_ids: Vec<String> = Vec::new();
        for check in checks.iter() {
            let Some(check_id) = check.as_str() else {
                return Err(anyhow!("lane '{lane_id}' has non-string check id"));
            };
            check_ids.push(check_id.to_string());
        }

        let mut workflow_table = Table::new();
        workflow_table["checks"] = value_from_toml(&toml_string_array(&check_ids))?;
        if let Some(parallel) = lane_table.get("parallel").and_then(Item::as_bool) {
            workflow_table["parallel"] = value_from_toml(if parallel { "true" } else { "false" })?;
        }
        if let Some(scope) = lane_table.get("scope").and_then(Item::as_str) {
            workflow_table["scope"] =
                value_from_toml(&format!("\"{}\"", escape_toml_string(scope)))?;
        }
        workflow_root.insert(&lane_id, Item::Table(workflow_table));

        if let Some(parallel) = lane_table.get("parallel").and_then(Item::as_bool) {
            lane_parallel.push(parallel);
        }
    }

    if let Some(pre_commit_workflow) = pre_commit_workflow {
        out["hooks"]["pre_commit"] =
            value_from_toml(&format!("\"{}\"", escape_toml_string(&pre_commit_workflow)))?;
    }

    if let Some(pre_push_workflow) = pre_push_workflow {
        out["hooks"]["pre_push"] =
            value_from_toml(&format!("\"{}\"", escape_toml_string(&pre_push_workflow)))?;
    }

    let mut defaults = Table::new();
    if let Some(defaults_v2) = source.get("defaults").and_then(Item::as_table)
        && let Some(timeout) = defaults_v2
            .get("timeout_seconds")
            .and_then(Item::as_integer)
    {
        defaults["timeout_seconds"] = value_from_toml(&timeout.to_string())?;
    }

    if !lane_parallel.is_empty() {
        let parallel = lane_parallel.iter().all(|v| *v);
        defaults["parallel"] = value_from_toml(if parallel { "true" } else { "false" })?;
    }

    if !defaults.is_empty() {
        out["defaults"] = Item::Table(defaults);
    }

    let Some(check_root) = out.get_mut("check").and_then(Item::as_table_mut) else {
        return Err(anyhow!("internal error: failed to create [check]"));
    };

    for (check_id, check_table) in iter_checks(&source)? {
        let mut out_check = Table::new();

        if let Some(label) = check_table.get("label").and_then(Item::as_str) {
            out_check["label"] = value_from_toml(&format!("\"{}\"", escape_toml_string(label)))?;
        }

        let command = if let Some(run) = check_table.get("run").and_then(Item::as_str) {
            run.to_string()
        } else if let Some(argv) = check_table.get("argv").and_then(Item::as_array) {
            if argv.len() == 3
                && argv.get(0).and_then(|i| i.as_str()) == Some("bash")
                && argv.get(1).and_then(|i| i.as_str()) == Some("-lc")
                && let Some(cmd) = argv.get(2).and_then(|i| i.as_str())
            {
                cmd.to_string()
            } else {
                return Err(anyhow!(
                    "check '{check_id}' uses argv; only bash -lc argv is supported"
                ));
            }
        } else {
            return Err(anyhow!("check '{check_id}' missing run/argv"));
        };

        out_check["command"] = value_from_toml(&format!("\"{}\"", escape_toml_string(&command)))?;

        if let Some(run_fix) = check_table.get("run_fix").and_then(Item::as_str) {
            out_check["fix_command"] =
                value_from_toml(&format!("\"{}\"", escape_toml_string(run_fix)))?;
        }

        let mut fix = check_table
            .get("autofix")
            .and_then(Item::as_bool)
            .unwrap_or(false);
        if check_table.get("run_fix").and_then(Item::as_str).is_some() {
            fix = true;
        }
        if fix {
            out_check["category"] = value_from_toml("\"mutate\"")?;
        }

        let input_mode = check_table
            .get("input_mode")
            .and_then(Item::as_str)
            .unwrap_or("none");
        if input_mode == "paths" {
            out_check["filters"] = value_from_toml(&toml_string_array(&["**/*".to_string()]))?;
        }

        check_root.insert(&check_id, Item::Table(out_check));
    }

    Ok(out.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_config_default_values() {
        let doc: DocumentMut = "".parse().unwrap();
        let config = parse_runner_config(&doc);

        assert_eq!(config.silence_warning_seconds, 30);
        assert!(!config.simple_output);
        assert!(!config.show_parallel_output);
    }

    #[test]
    fn test_runner_config_custom_values() {
        let toml = r#"
[defaults]
silence-warning-seconds = 60
simple-output = true
show-parallel-output = true
"#;
        let doc: DocumentMut = toml.parse().unwrap();
        let config = parse_runner_config(&doc);

        assert_eq!(config.silence_warning_seconds, 60);
        assert!(config.simple_output);
        assert!(config.show_parallel_output);
    }

    #[test]
    fn test_runner_config_zero_disables_warning() {
        let toml = r#"
[defaults]
silence-warning-seconds = 0
"#;
        let doc: DocumentMut = toml.parse().unwrap();
        let config = parse_runner_config(&doc);

        assert_eq!(config.silence_warning_seconds, 0);
    }

    #[test]
    fn test_runner_config_partial_values() {
        let toml = r#"
[defaults]
simple-output = true
"#;
        let doc: DocumentMut = toml.parse().unwrap();
        let config = parse_runner_config(&doc);

        // simple-output is set, others should be default
        assert_eq!(config.silence_warning_seconds, 30);
        assert!(config.simple_output);
        assert!(!config.show_parallel_output);
    }

    #[test]
    fn test_runner_config_negative_seconds_clamped() {
        let toml = r#"
[defaults]
silence-warning-seconds = -10
"#;
        let doc: DocumentMut = toml.parse().unwrap();
        let config = parse_runner_config(&doc);

        // Negative values should be clamped to 0
        assert_eq!(config.silence_warning_seconds, 0);
    }
}
