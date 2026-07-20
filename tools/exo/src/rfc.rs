use anyhow::{Context, Result};
use gray_matter::{Matter, engine::YAML};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex, OnceLock};
use walkdir::WalkDir;

use crate::context::sqlite_loader::{
    RfcRecord, RfcWorkspaceDiagnostic, RfcWorkspaceObservation, RfcWorkspaceSnapshot,
};
use crate::context::{SqliteLoader, SqliteWriter};
use crate::project::Project;
use crate::utils;

const RFCS_DIR: &str = "docs/rfcs";
const SKIP_RFC_FILES: &[&str] = &["0000-template.md", "README.md"];
const DEFAULT_RFC_ID_WIDTH: usize = 5;

static ANCHOR_ULID_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^<!-- exo:\d+ ulid:([a-zA-Z0-9]+) -->"));
static ANCHOR_ANY_ULID_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^<!--\s*exo:[^\s>]+\s+ulid:([a-zA-Z0-9]+)\s*-->"));
static ANCHOR_RFC_NUMBER_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^<!-- exo:(\d+) ulid:[a-zA-Z0-9]+ -->"));
static PARTIAL_ANCHOR_RFC_NUMBER_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^<!--\s*exo:(\d+)(?:\s+[^>]*)?\s*-->"));
static ANCHOR_PREFIX_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^<!--\s*exo:"));
static H1_TITLE_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?m)^#\s+(?:RFC\s+\d+:\s*)?(.+)$"));
static RFC_RELATION_REF_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\bRFC\s*([0-9]{1,6})\b"));
static RFC_RELATION_BARE_ID_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\b([0-9]{4,6})\b"));
static SUPERSEDED_BY_TARGET_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(superseded\s+by\s+)(?:\[[^\]]*\bRFC\s*[0-9]{1,6}[^\]]*\]\([^)]+\)|RFC\s*[0-9]{1,6}(?::\s*[^.;\n]+)?)",
    )
});
static RECONCILED_RFC_KEYS: OnceLock<Mutex<HashSet<ReconcileKey>>> = OnceLock::new();

type WorkspaceRfcDocuments = Vec<(String, Vec<u8>)>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    root: PathBuf,
    project_id: Option<String>,
    db_path: PathBuf,
    source_version: Option<String>,
}

impl ReconcileKey {
    fn new(
        root: &Path,
        project: Option<&Project>,
        source: &CanonicalReconcileSource,
        workspace_documents: Option<&[(String, Vec<u8>)]>,
    ) -> Result<Self> {
        Ok(Self {
            root: normalize_key_path(root),
            project_id: project.map(|project| project.id.as_str().to_string()),
            db_path: normalize_key_path(&crate::context::db_path(root, project)),
            source_version: reconcile_source_cache_version(source, workspace_documents)?,
        })
    }
}

fn normalize_key_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Deserialize, Debug)]
struct FrontMatter {
    title: Option<String>,
    feature: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Rfc {
    #[serde(rename = "filename")]
    pub filename: String,
    pub number: String,
    pub title: String,
    pub stage: u8,
    pub feature: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileResult {
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
}

/// Provenance for an RFC record in the issuing workspace's effective view.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RfcViewProvenance {
    pub document_source: String,
    pub workspace_presence: String,
    pub canonical_presence: String,
    pub workspace_branch: Option<String>,
    pub workspace_head: Option<String>,
    pub canonical_ref: Option<String>,
    pub canonical_head: Option<String>,
    pub differs_from_canonical: bool,
}

/// RFC metadata composed from the issuing workspace and shared canonical state.
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveRfcRecord {
    #[serde(flatten)]
    pub record: RfcRecord,
    #[serde(flatten)]
    pub provenance: RfcViewProvenance,
}

/// Effective RFC records and diagnostics from one atomic workspace refresh.
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveRfcView {
    pub records: Vec<EffectiveRfcRecord>,
    pub workspace_diagnostics: Vec<RfcWorkspaceDiagnostic>,
    #[serde(skip)]
    pub(crate) repair_records: Vec<RfcRecord>,
    #[serde(skip)]
    snapshot: RfcWorkspaceSnapshot,
}

impl EffectiveRfcView {
    /// Return the persisted workspace/canonical identity for this coherent view.
    #[must_use]
    pub const fn workspace_snapshot(&self) -> &RfcWorkspaceSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone)]
struct DiskRfcRecord {
    text_id: String,
    rfc_number: i64,
    title: String,
    stage: u8,
    status: String,
    lifecycle_status_declared: bool,
    slug: String,
    file_path: String,
    superseded_by: Option<String>,
    supersedes: Option<String>,
    superseded_by_declared: bool,
    supersedes_declared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclaredRfcValue {
    value: Option<String>,
    declared: bool,
}

impl DeclaredRfcValue {
    const fn absent() -> Self {
        Self {
            value: None,
            declared: false,
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedRfcDocument {
    disk: DiskRfcRecord,
    filename_number: i64,
    stage_source: String,
    canonical_metadata_conflict: bool,
    feature: DeclaredRfcValue,
    withdrawal_reason: DeclaredRfcValue,
    archived_reason: DeclaredRfcValue,
    consolidated_into: DeclaredRfcValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalGitRef {
    ref_name: String,
    oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanonicalReconcileSource {
    Canonical(CanonicalGitRef),
    PreserveShared,
    WorkspaceFallback,
}

#[derive(Debug)]
struct CanonicalRfcBlob {
    file_path: String,
    content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskRfcRelationships {
    pub superseded_by: Option<String>,
    pub supersedes: Option<String>,
    pub superseded_by_declared: bool,
    pub supersedes_declared: bool,
}

#[derive(Debug, Clone)]
struct MalformedRfcRepairDebt {
    identity_number: i64,
    text_id: Option<String>,
    file_path: String,
    candidate: RfcRepairCandidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct RfcRepairCandidate {
    pub id: String,
    pub current_path: String,
    pub expected_path: String,
    pub title: String,
    pub reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored_metadata: Option<RfcRepairStoredMetadata>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RfcRepairStoredMetadata {
    pub path: String,
    pub stage: u8,
    pub status: String,
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RfcRepairCandidateMode {
    Manual,
    ReconcileExistingRow,
}

#[derive(Debug, Clone, Serialize)]
pub struct RfcRepairOutcome {
    pub id: String,
    pub old_path: String,
    pub new_path: String,
    pub title: String,
    pub reasons: Vec<String>,
    pub repaired: bool,
    pub renumbered_to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RfcSupersedeOutcome {
    pub superseded_path: PathBuf,
    pub superseding_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RfcSupersedeFileOutcome {
    pub superseded_path: PathBuf,
}

/// Extract RFC number from a filename like `10181-shared-perception-...md`.
#[must_use]
pub fn parse_rfc_number(filename: &str) -> Option<i64> {
    extract_rfc_number_from_filename(filename)?.parse().ok()
}

#[must_use]
pub fn format_rfc_number(number: i64) -> String {
    format!("{number:0width$}", width = DEFAULT_RFC_ID_WIDTH)
}

/// Extract slug from filename: `10181-shared-perception-foo.md` → `shared-perception-foo`.
#[must_use]
pub fn parse_slug(filename: &str) -> String {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    if let Some(pos) = stem.find('-') {
        stem[pos + 1..].to_string()
    } else {
        stem.to_string()
    }
}

/// Determine stage from directory path.
#[must_use]
pub fn parse_stage(path: &Path) -> u8 {
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str()
            && let Some(n) = s.strip_prefix("stage-")
            && let Ok(stage) = n.parse::<u8>()
        {
            return stage;
        }
    }
    0
}

/// Determine RFC status from directory path.
#[must_use]
pub fn parse_status(path: &Path) -> &'static str {
    for component in path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            match s {
                "archive" => return "archived",
                "withdrawn" => return "withdrawn",
                _ => {}
            }
        }
    }
    "active"
}

/// Check if a file starts with an RFC anchor comment.
///
/// Tolerant of whitespace variations (`<!--  exo:` etc.) so that slightly
/// non-canonical anchors are parsed rather than treated as missing.
#[must_use]
pub fn has_anchor(content: &str) -> bool {
    ANCHOR_PREFIX_RE
        .as_ref()
        .is_ok_and(|re| re.is_match(content))
}

/// Extract ULID from an RFC anchor comment.
#[must_use]
pub fn extract_anchor_ulid(content: &str) -> Option<String> {
    let re = ANCHOR_ULID_RE.as_ref().ok()?;
    re.captures(content)
        .or_else(|| ANCHOR_ANY_ULID_RE.as_ref().ok()?.captures(content))?
        .get(1)
        .map(|m| m.as_str().to_string())
}

/// Extract the RFC number from an RFC anchor comment.
///
/// Falls back to tolerant matching so anchors with extra whitespace parse
/// the same way `extract_anchor_ulid` does, instead of failing strictly.
#[must_use]
pub fn extract_anchor_rfc_number(content: &str) -> Option<i64> {
    let re = ANCHOR_RFC_NUMBER_RE.as_ref().ok()?;
    if let Some(captures) = re.captures(content) {
        return captures.get(1)?.as_str().parse().ok();
    }
    extract_partial_anchor_rfc_number(content)
}

/// Extract title from the first H1 heading.
#[must_use]
pub fn extract_h1_title(content: &str) -> Option<String> {
    let re = H1_TITLE_RE.as_ref().ok()?;
    re.captures(content)?
        .get(1)
        .map(|m| m.as_str().trim().to_string())
}

/// Strip leading YAML frontmatter from content.
#[must_use]
pub fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---\n") {
        return content.to_string();
    }
    if let Some(end) = content[4..].find("\n---") {
        let after = &content[4 + end + 4..];
        after.strip_prefix('\n').unwrap_or(after).to_string()
    } else {
        content.to_string()
    }
}

#[must_use]
pub fn extract_rfc_relationships(content: &str) -> DiskRfcRelationships {
    let mut relationships = DiskRfcRelationships::default();

    if let Some(frontmatter) = content.trim_start().strip_prefix("---\n")
        && let Some(end) = frontmatter.find("\n---")
    {
        for line in frontmatter[..end].lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = clean_relationship_value(value);
            match key.trim() {
                "superseded_by" | "superseded-by" => {
                    relationships.superseded_by_declared = true;
                    merge_relationship_value(&mut relationships.superseded_by, value);
                }
                "supersedes" => {
                    relationships.supersedes_declared = true;
                    merge_relationship_value(&mut relationships.supersedes, value);
                }
                _ => {}
            }
        }
    }

    let mut collect_supersedes = false;
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = line.trim();
        if collect_supersedes {
            if line.starts_with("- ") || line.starts_with("* ") {
                relationships.supersedes_declared = true;
                merge_relationship_value(
                    &mut relationships.supersedes,
                    clean_relationship_value(line),
                );
                continue;
            }
            if !line.is_empty() {
                collect_supersedes = false;
            }
        }

        if let Some(value) = relationship_line_cleaned_value(line, "Superseded by") {
            relationships.superseded_by_declared = true;
            if let Some(value) = value {
                merge_relationship_value(&mut relationships.superseded_by, value);
            }
        }
        if let Some(value) = relationship_line_cleaned_value(line, "Supersedes") {
            relationships.supersedes_declared = true;
            if let Some(value) = value {
                merge_relationship_value(&mut relationships.supersedes, value);
            }
        }
        if let Some(value) = supersedes_section_intro_value(line) {
            relationships.supersedes_declared = true;
            if let Some(value) = value {
                merge_relationship_value(&mut relationships.supersedes, value);
            } else {
                collect_supersedes = true;
            }
        }
        if let Some(value) = superseded_by_status_or_reason_value(line) {
            relationships.superseded_by_declared = true;
            merge_relationship_value(&mut relationships.superseded_by, value);
        }
        if let Some(value) = superseded_by_sentence_value(line) {
            relationships.superseded_by_declared = true;
            merge_relationship_value(&mut relationships.superseded_by, value);
        }
    }

    relationships
}

fn merge_relationship_value(slot: &mut Option<String>, value: String) {
    if value.is_empty() {
        return;
    }

    let mut ids = slot
        .as_deref()
        .map(|existing| {
            existing
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        let key = relationship_id_key(id);
        if !ids
            .iter()
            .any(|existing| relationship_id_key(existing) == key)
        {
            ids.push(id.to_string());
        }
    }

    if !ids.is_empty() {
        *slot = Some(ids.join(", "));
    }
}

fn relationship_line_cleaned_value(line: &str, label: &str) -> Option<Option<String>> {
    let line = normalize_relationship_line(line);
    let line = line
        .strip_prefix(label)
        .or_else(|| line.strip_prefix(&format!("**{label}**")))
        .or_else(|| line.strip_prefix(&format!("**⚠️ {label}")))?;
    let value = line
        .strip_prefix(':')
        .or_else(|| line.strip_prefix("**:"))
        .or_else(|| line.strip_prefix('|'))
        .or_else(|| line.strip_prefix(' '))
        .or_else(|| line.strip_prefix(": "))?;
    let value = clean_relationship_value(value);
    Some((!value.is_empty()).then_some(value))
}

fn supersedes_section_intro_value(line: &str) -> Option<Option<String>> {
    let line = normalize_relationship_line(line);
    let prefix = "This RFC supersedes";
    if !line.to_lowercase().starts_with(&prefix.to_lowercase()) {
        return None;
    }
    let value = line.get(prefix.len()..)?.trim_start().strip_prefix(':')?;
    let value = clean_relationship_value(value);
    Some((!value.is_empty()).then_some(value))
}

fn superseded_by_status_or_reason_value(line: &str) -> Option<String> {
    ["Status", "Reason", "Note"].into_iter().find_map(|label| {
        let value = metadata_line_value(line, label)?;
        if !value.to_lowercase().contains("superseded by") {
            return None;
        }
        superseded_by_target_value(&value)
    })
}

fn superseded_by_sentence_value(line: &str) -> Option<String> {
    let line = normalize_relationship_line(line);
    if !line
        .to_lowercase()
        .starts_with("this rfc has been superseded by")
    {
        return None;
    }
    superseded_by_target_value(&line)
}

fn superseded_by_target_value(line: &str) -> Option<String> {
    let re = SUPERSEDED_BY_TARGET_RE.as_ref().ok()?;
    let matched = re.find(line)?.as_str();
    Some(clean_relationship_value(matched)).filter(|value| !value.is_empty())
}

fn declared_lifecycle_status(content: &str) -> Option<&'static str> {
    for line in content.lines() {
        let Some(value) = metadata_line_value(line, "Status") else {
            continue;
        };
        let value = value.to_lowercase();
        if value.contains("archived") {
            return Some("archived");
        }
        if value.contains("withdrawn") {
            return Some("withdrawn");
        }
    }
    None
}

fn metadata_line_value(line: &str, label: &str) -> Option<String> {
    let line = normalize_relationship_line(line);
    let value = line
        .strip_prefix(label)
        .or_else(|| line.strip_prefix(&format!("**{label}**")))?;
    let value = value.trim_start();
    let value = value
        .strip_prefix(':')
        .or_else(|| value.strip_prefix("**:"))
        .or_else(|| value.strip_prefix('|'))?;
    Some(value.trim().trim_matches('|').trim().to_string()).filter(|value| !value.is_empty())
}

fn normalize_relationship_line(line: &str) -> String {
    line.trim()
        .trim_start_matches('>')
        .trim()
        .trim_start_matches('-')
        .trim()
        .trim_start_matches('|')
        .trim()
        .to_string()
}

fn clean_relationship_value(value: &str) -> String {
    let value = value
        .trim()
        .trim_matches('|')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    let value = value
        .trim_start_matches('*')
        .trim_start_matches(':')
        .trim_start_matches('|')
        .trim_start_matches('*')
        .trim()
        .trim_matches('|')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();

    let rfc_refs = extract_ordered_relation_ids(value, &RFC_RELATION_REF_RE);
    if !rfc_refs.is_empty() {
        return rfc_refs.join(", ");
    }

    let value = value
        .strip_prefix("RFC ")
        .or_else(|| value.strip_prefix("RFC"))
        .unwrap_or(value)
        .trim();
    let bare_refs = extract_ordered_relation_ids(value, &RFC_RELATION_BARE_ID_RE);
    if !bare_refs.is_empty() {
        return bare_refs.join(", ");
    }

    String::new()
}

fn extract_ordered_relation_ids(
    value: &str,
    pattern: &LazyLock<Result<Regex, regex::Error>>,
) -> Vec<String> {
    let Ok(pattern) = pattern.as_ref() else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for captures in pattern.captures_iter(value) {
        let Some(id) = captures.get(1).map(|match_| match_.as_str().to_string()) else {
            continue;
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

#[allow(clippy::missing_errors_doc)]
pub fn reconcile_rfcs(root: &Path) -> Result<ReconcileResult> {
    let project = Project::resolve(root).ok();
    reconcile_rfcs_with_project(root, project.as_ref())
}

fn reconcile_source_cache_version(
    source: &CanonicalReconcileSource,
    workspace_documents: Option<&[(String, Vec<u8>)]>,
) -> Result<Option<String>> {
    match source {
        CanonicalReconcileSource::Canonical(canonical) => Ok(Some(canonical.oid.clone())),
        CanonicalReconcileSource::WorkspaceFallback => {
            let documents = workspace_documents
                .context("Workspace RFC documents are required for fallback reconciliation")?;
            let digest = workspace_document_digest(documents)
                .into_iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            Ok(Some(format!("workspace-fallback:{digest}")))
        }
        CanonicalReconcileSource::PreserveShared => Ok(None),
    }
}

fn canonical_reconcile_source(root: &Path) -> Result<CanonicalReconcileSource> {
    #[cfg(test)]
    CANONICAL_SOURCE_OBSERVATIONS.with(|count| count.set(count.get() + 1));

    if let Some(canonical) = resolve_canonical_ref(root, "refs/remotes/origin/HEAD")? {
        return Ok(CanonicalReconcileSource::Canonical(canonical));
    }

    let Some(inside_worktree) = git_stdout(root, &["rev-parse", "--is-inside-work-tree"])? else {
        return Ok(CanonicalReconcileSource::WorkspaceFallback);
    };
    if inside_worktree.trim() != "true" {
        return Ok(CanonicalReconcileSource::WorkspaceFallback);
    }

    let other_remote_heads = git_stdout(
        root,
        &["for-each-ref", "--format=%(refname)", "refs/remotes/*/HEAD"],
    )?
    .unwrap_or_default()
    .lines()
    .filter(|ref_name| *ref_name != "refs/remotes/origin/HEAD")
    .map(str::to_string)
    .collect::<Vec<_>>();
    let mut resolved_remote_heads = Vec::new();
    for ref_name in other_remote_heads {
        if let Some(canonical) = resolve_canonical_ref(root, &ref_name)? {
            resolved_remote_heads.push(canonical);
        }
    }
    if resolved_remote_heads.len() == 1 {
        return Ok(CanonicalReconcileSource::Canonical(
            resolved_remote_heads.remove(0),
        ));
    }
    if resolved_remote_heads.len() > 1 {
        return Ok(CanonicalReconcileSource::PreserveShared);
    }

    for ref_name in ["refs/heads/main", "refs/heads/master"] {
        if let Some(canonical) = resolve_canonical_ref(root, ref_name)? {
            return Ok(CanonicalReconcileSource::Canonical(canonical));
        }
    }

    let worktree_count = git_stdout(root, &["worktree", "list", "--porcelain"])?
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count();
    if worktree_count == 1 {
        if let Some(canonical) = resolve_canonical_ref(root, "HEAD")? {
            return Ok(CanonicalReconcileSource::Canonical(canonical));
        }
        return Ok(CanonicalReconcileSource::WorkspaceFallback);
    }

    Ok(CanonicalReconcileSource::PreserveShared)
}

pub(crate) fn canonical_rfc_commit_oid(root: &Path) -> Result<Option<String>> {
    match canonical_reconcile_source(root)? {
        CanonicalReconcileSource::Canonical(canonical) => Ok(Some(canonical.oid)),
        CanonicalReconcileSource::WorkspaceFallback => {
            Ok(resolve_canonical_ref(root, "HEAD")?.map(|canonical| canonical.oid))
        }
        CanonicalReconcileSource::PreserveShared => Ok(None),
    }
}

#[cfg(test)]
thread_local! {
    static CANONICAL_SOURCE_OBSERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WORKSPACE_RFC_DOCUMENT_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_canonical_source_observation_count() {
    CANONICAL_SOURCE_OBSERVATIONS.with(|count| count.set(0));
}

#[cfg(test)]
fn canonical_source_observation_count() -> usize {
    CANONICAL_SOURCE_OBSERVATIONS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_workspace_rfc_document_load_count() {
    WORKSPACE_RFC_DOCUMENT_LOADS.with(|count| count.set(0));
}

#[cfg(test)]
fn workspace_rfc_document_load_count() -> usize {
    WORKSPACE_RFC_DOCUMENT_LOADS.with(std::cell::Cell::get)
}

fn resolve_canonical_ref(root: &Path, ref_name: &str) -> Result<Option<CanonicalGitRef>> {
    let revision = format!("{ref_name}^{{commit}}");
    let Some(oid) = git_stdout(root, &["rev-parse", "--verify", "--quiet", &revision])? else {
        return Ok(None);
    };
    let oid = oid.trim();
    if oid.is_empty() {
        return Ok(None);
    }
    Ok(Some(CanonicalGitRef {
        ref_name: ref_name.to_string(),
        oid: oid.to_string(),
    }))
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git in {}", root.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    String::from_utf8(output.stdout)
        .context("Git returned non-UTF-8 output")
        .map(|output| Some(output.trim_end_matches(['\r', '\n']).to_string()))
}

fn canonical_rfc_blobs(root: &Path, canonical: &CanonicalGitRef) -> Result<Vec<CanonicalRfcBlob>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-tree",
            "-r",
            "-z",
            "--full-tree",
            &canonical.oid,
            "--",
            RFCS_DIR,
        ])
        .output()
        .with_context(|| format!("Failed to enumerate RFCs from {}", canonical.ref_name))?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to enumerate RFCs from {} at {}",
            canonical.ref_name,
            canonical.oid
        );
    }

    let mut entries = Vec::new();
    for raw_entry in output.stdout.split(|byte| *byte == 0) {
        if raw_entry.is_empty() {
            continue;
        }
        let entry = std::str::from_utf8(raw_entry).context("Git tree entry was not UTF-8")?;
        let Some((metadata, file_path)) = entry.split_once('\t') else {
            anyhow::bail!("Unexpected git ls-tree entry: {entry}");
        };
        let mut metadata = metadata.split_whitespace();
        let _mode = metadata.next();
        let object_type = metadata.next();
        let object_id = metadata.next();
        if object_type != Some("blob") {
            continue;
        }
        let Some(object_id) = object_id else {
            anyhow::bail!("Git tree entry had no object ID: {entry}");
        };
        let path = Path::new(file_path);
        if !is_rfc_document_path(Path::new(RFCS_DIR), path) {
            continue;
        }
        let filename = path.file_name().and_then(|name| name.to_str());
        if filename.is_some_and(|filename| SKIP_RFC_FILES.contains(&filename)) {
            continue;
        }
        entries.push((object_id.to_string(), file_path.to_string()));
    }

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to start git cat-file --batch")?;
    let mut stdin = child
        .stdin
        .take()
        .context("Git cat-file stdin unavailable")?;
    let requested_object_ids = entries
        .iter()
        .map(|(object_id, _)| object_id.clone())
        .collect::<Vec<_>>();
    let request_writer = std::thread::spawn(move || -> Result<()> {
        for object_id in requested_object_ids {
            writeln!(stdin, "{object_id}").context("Failed to request Git RFC blob")?;
        }
        Ok(())
    });

    let stdout = child
        .stdout
        .take()
        .context("Git cat-file stdout unavailable")?;
    let mut reader = BufReader::new(stdout);
    let mut blobs = Vec::with_capacity(entries.len());
    for (expected_oid, file_path) in entries {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .context("Failed to read Git RFC blob header")?;
        let mut parts = header.split_whitespace();
        let actual_oid = parts.next().unwrap_or_default();
        let object_type = parts.next().unwrap_or_default();
        let size = parts
            .next()
            .with_context(|| format!("Git blob header had no size: {header:?}"))?
            .parse::<usize>()
            .with_context(|| format!("Git blob header had invalid size: {header:?}"))?;
        if actual_oid != expected_oid || object_type != "blob" {
            anyhow::bail!(
                "Git returned unexpected object for {file_path}: expected {expected_oid}, got {header:?}"
            );
        }
        let mut content = vec![0; size];
        reader
            .read_exact(&mut content)
            .with_context(|| format!("Failed to read RFC blob {file_path}"))?;
        let mut terminator = [0_u8; 1];
        reader
            .read_exact(&mut terminator)
            .with_context(|| format!("Failed to finish RFC blob {file_path}"))?;
        if terminator[0] != b'\n' {
            anyhow::bail!("Git RFC blob {file_path} had an invalid terminator");
        }
        blobs.push(CanonicalRfcBlob {
            file_path,
            content: String::from_utf8(content)
                .context("RFC document in canonical Git tree was not UTF-8")?,
        });
    }

    let status = child.wait().context("Failed to wait for git cat-file")?;
    request_writer
        .join()
        .map_err(|_| anyhow::anyhow!("Git RFC blob request writer panicked"))?
        .context("Failed to write Git RFC blob requests")?;
    if !status.success() {
        anyhow::bail!("git cat-file --batch failed for canonical RFC tree");
    }
    Ok(blobs)
}

fn canonical_history_contains_anchor(
    root: &Path,
    canonical: &CanonicalGitRef,
    text_id: &str,
) -> Result<bool> {
    let anchor = format!("ulid:{text_id}");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--format=%H",
            "-S",
            &anchor,
            &canonical.oid,
            "--",
            RFCS_DIR,
        ])
        .output()
        .with_context(|| format!("Failed to inspect RFC history from {}", canonical.ref_name))?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to inspect RFC history from {} at {}",
            canonical.ref_name,
            canonical.oid
        );
    }
    Ok(!output.stdout.is_empty())
}

#[allow(clippy::missing_errors_doc)]
pub fn reconcile_rfcs_once_with_project(
    root: &Path,
    project: Option<&Project>,
) -> Result<ReconcileResult> {
    with_reconcile_lock(root, project, || {
        let source = canonical_reconcile_source(root)?;
        reconcile_and_refresh_locked(root, project, &source, true)
    })
}

/// Reconcile and observe one coherent RFC view for a command request.
///
/// Canonical identity is resolved after taking the cross-process lock and is
/// reused for canonical reconciliation, workspace refresh, and effective-view
/// composition. The next request repeats this observation so ref advancement
/// remains visible without allowing one request to mix snapshot bases.
#[allow(clippy::missing_errors_doc)]
pub fn observe_effective_rfc_view_with_project(
    root: &Path,
    project: Option<&Project>,
) -> Result<(ReconcileResult, EffectiveRfcView)> {
    observe_effective_rfc_view(root, project, true)
}

fn observe_effective_rfc_view(
    root: &Path,
    project: Option<&Project>,
    reconcile_shared: bool,
) -> Result<(ReconcileResult, EffectiveRfcView)> {
    with_reconcile_lock(root, project, || {
        let source = canonical_reconcile_source(root)?;
        let result = reconcile_and_refresh_locked(root, project, &source, reconcile_shared)?;
        let view = compose_effective_rfc_view_locked(root, project, &source)?;
        Ok((result, view))
    })
}

fn reconcile_and_refresh_locked(
    root: &Path,
    project: Option<&Project>,
    source: &CanonicalReconcileSource,
    reconcile_shared: bool,
) -> Result<ReconcileResult> {
    let workspace_documents = matches!(source, CanonicalReconcileSource::WorkspaceFallback)
        .then(|| load_workspace_rfc_documents(root))
        .transpose()?;
    let key = ReconcileKey::new(root, project, source, workspace_documents.as_deref())?;
    let publish_reconciled_key = can_publish_reconciled_key(root, project)?;
    let reconciled_keys = RECONCILED_RFC_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
    let should_reconcile = {
        let reconciled_keys = reconciled_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reconcile_shared && !reconciled_keys.contains(&key)
    };
    let result = if should_reconcile {
        reconcile_rfcs_from_source(root, project, source)?
    } else {
        ReconcileResult::default()
    };
    refresh_workspace_rfc_snapshot_with_documents(
        root,
        project,
        source,
        workspace_documents.as_deref(),
    )?;
    if should_reconcile && key.source_version.is_some() && publish_reconciled_key {
        let mut reconciled_keys = reconciled_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reconciled_keys.insert(key);
        drop(reconciled_keys);
    }
    Ok(result)
}

fn can_publish_reconciled_key(root: &Path, project: Option<&Project>) -> Result<bool> {
    let db_path = crate::context::db_path(root, project);
    Ok(exosuit_storage::active_request_database(&db_path)?.is_none())
}

fn with_reconcile_lock<T>(
    root: &Path,
    project: Option<&Project>,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    use fs2::FileExt;

    let db_path = crate::context::db_path(root, project);
    let lock_path = db_path.with_extension("rfc-reconcile.lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("Failed to open RFC reconcile lock {}", lock_path.display()))?;
    #[cfg(test)]
    if let Some(marker) = std::env::var_os("EXO_TEST_RFC_RECONCILE_LOCK_WAIT_MARKER") {
        std::fs::write(marker, b"waiting")
            .context("Failed to write RFC reconcile lock wait marker")?;
    }
    file.lock_exclusive()
        .with_context(|| format!("Failed to lock RFC reconcile lock {}", lock_path.display()))?;
    let result = f();
    let unlock_result = file.unlock();
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => {
            Err(error).with_context(|| format!("Failed to unlock {}", lock_path.display()))
        }
    }
}

fn reconciled_relationship_value<'a>(
    _existing: Option<&'a str>,
    parsed: Option<&'a str>,
    parsed_declared: bool,
) -> Option<&'a str> {
    if !parsed_declared {
        return None;
    }

    parsed
}

#[allow(clippy::missing_errors_doc)]
pub fn reconcile_rfcs_with_project(
    root: &Path,
    project: Option<&Project>,
) -> Result<ReconcileResult> {
    with_reconcile_lock(root, project, || {
        let source = canonical_reconcile_source(root)?;
        let result = reconcile_rfcs_from_source(root, project, &source)?;
        refresh_workspace_rfc_snapshot(root, project, &source)?;
        Ok(result)
    })
}

fn reconcile_rfcs_from_source(
    root: &Path,
    project: Option<&Project>,
    source: &CanonicalReconcileSource,
) -> Result<ReconcileResult> {
    match source {
        CanonicalReconcileSource::Canonical(canonical) => {
            reconcile_canonical_rfcs(root, project, &canonical)
        }
        CanonicalReconcileSource::PreserveShared => Ok(ReconcileResult::default()),
        CanonicalReconcileSource::WorkspaceFallback => reconcile_workspace_rfcs(root, project),
    }
}

fn reconcile_canonical_rfcs(
    root: &Path,
    project: Option<&Project>,
    canonical: &CanonicalGitRef,
) -> Result<ReconcileResult> {
    let db_path = crate::context::db_path(root, project);
    let loader = SqliteLoader::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
    let writer = SqliteWriter::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    let existing_by_text_id: HashMap<String, RfcRecord> = loader
        .load_rfcs()?
        .into_iter()
        .map(|row| (row.text_id.clone(), row))
        .collect();
    let existing_by_path = existing_by_text_id
        .values()
        .map(|record| (record.file_path.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut result = ReconcileResult::default();
    let blobs = canonical_rfc_blobs(root, canonical)?;
    let mut canonical_presence_anchors = BTreeSet::new();
    let mut parsed_documents = Vec::new();
    for blob in blobs {
        let anchor = extract_anchor_ulid(&blob.content);
        let existing_at_path = existing_by_path.get(&blob.file_path).copied();
        if let Some(text_id) = anchor.as_deref() {
            canonical_presence_anchors.insert(text_id.to_string());
        }
        let existing = anchor
            .as_deref()
            .and_then(|text_id| existing_by_text_id.get(text_id))
            .or_else(|| anchor.is_none().then_some(existing_at_path).flatten());
        match parse_rfc_document(&blob.file_path, &blob.content, existing) {
            Ok(parsed) => {
                if parsed.canonical_metadata_conflict
                    || parsed.filename_number != parsed.disk.rfc_number
                {
                    result.unchanged += 1;
                } else {
                    parsed_documents.push(parsed);
                }
            }
            Err(_) => result.unchanged += 1,
        }
    }

    let mut text_id_counts = HashMap::new();
    for parsed in &parsed_documents {
        *text_id_counts
            .entry(parsed.disk.text_id.clone())
            .or_insert(0_usize) += 1;
    }

    let mut changed_records = Vec::new();
    let mut relinked_canonical_anchors = BTreeSet::new();
    for parsed in parsed_documents {
        if text_id_counts.get(&parsed.disk.text_id) != Some(&1) {
            result.unchanged += 1;
            continue;
        }
        relinked_canonical_anchors.insert(parsed.disk.text_id.clone());
        let existing = existing_by_text_id.get(&parsed.disk.text_id);
        let record = canonical_rfc_record(&parsed, existing);
        match existing {
            Some(existing) if rfc_records_equal(existing, &record) => result.unchanged += 1,
            Some(_) => {
                result.updated += 1;
                changed_records.push(record);
            }
            None => {
                result.inserted += 1;
                changed_records.push(record);
            }
        }
    }

    let establish_baseline = !writer.has_rfc_canonical_baseline()?;
    let mut quarantined = Vec::new();
    for record in existing_by_text_id
        .values()
        .filter(|record| !canonical_presence_anchors.contains(&record.text_id))
    {
        let has_canonical_history = !establish_baseline
            && canonical_history_contains_anchor(root, canonical, &record.text_id)?;
        if establish_baseline || !has_canonical_history {
            quarantined.push(record.clone());
        }
    }
    result.deleted += quarantined.len();
    writer.reconcile_canonical_rfcs(
        &changed_records,
        &quarantined,
        &relinked_canonical_anchors,
        &canonical.ref_name,
        &canonical.oid,
        establish_baseline,
    )?;
    Ok(result)
}

fn canonical_rfc_record(parsed: &ParsedRfcDocument, existing: Option<&RfcRecord>) -> RfcRecord {
    let compatibility_value = |declared: &DeclaredRfcValue, existing: Option<&String>| {
        if declared.declared {
            declared.value.clone()
        } else {
            existing.cloned()
        }
    };
    RfcRecord {
        text_id: parsed.disk.text_id.clone(),
        rfc_number: parsed.disk.rfc_number,
        title: parsed.disk.title.clone(),
        stage: parsed.disk.stage,
        status: parsed.disk.status.clone(),
        feature: compatibility_value(
            &parsed.feature,
            existing.and_then(|row| row.feature.as_ref()),
        ),
        slug: parsed.disk.slug.clone(),
        file_path: parsed.disk.file_path.clone(),
        superseded_by: if parsed.disk.superseded_by_declared {
            parsed.disk.superseded_by.clone()
        } else {
            existing.and_then(|row| row.superseded_by.clone())
        },
        supersedes: if parsed.disk.supersedes_declared {
            parsed.disk.supersedes.clone()
        } else {
            existing.and_then(|row| row.supersedes.clone())
        },
        withdrawal_reason: if parsed.disk.status == "withdrawn" {
            compatibility_value(
                &parsed.withdrawal_reason,
                existing.and_then(|row| row.withdrawal_reason.as_ref()),
            )
        } else {
            None
        },
        archived_reason: if parsed.disk.status == "archived" {
            compatibility_value(
                &parsed.archived_reason,
                existing.and_then(|row| row.archived_reason.as_ref()),
            )
        } else {
            None
        },
        consolidated_into: compatibility_value(
            &parsed.consolidated_into,
            existing.and_then(|row| row.consolidated_into.as_ref()),
        ),
    }
}

fn rfc_records_equal(left: &RfcRecord, right: &RfcRecord) -> bool {
    left.text_id == right.text_id
        && left.rfc_number == right.rfc_number
        && left.title == right.title
        && left.stage == right.stage
        && left.status == right.status
        && left.feature == right.feature
        && left.slug == right.slug
        && left.file_path == right.file_path
        && left.superseded_by == right.superseded_by
        && left.supersedes == right.supersedes
        && left.withdrawal_reason == right.withdrawal_reason
        && left.archived_reason == right.archived_reason
        && left.consolidated_into == right.consolidated_into
}

fn reconcile_workspace_rfcs(root: &Path, project: Option<&Project>) -> Result<ReconcileResult> {
    let db_path = crate::context::db_path(root, project);
    let loader = SqliteLoader::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
    let writer = SqliteWriter::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    let mut existing_by_text_id: HashMap<String, RfcRecord> = loader
        .load_rfcs()?
        .into_iter()
        .map(|row| (row.text_id.clone(), row))
        .collect();
    let records_by_text_id = existing_by_text_id.clone();

    let mut result = ReconcileResult::default();
    let rfc_root = root.join(RFCS_DIR);

    if rfc_root.exists() {
        for path in walk_rfc_markdown_files(&rfc_root) {
            let parsed = match parse_disk_rfc(root, &path) {
                Ok(parsed) => parsed,
                Err(error) => {
                    if let Some(debt) =
                        malformed_rfc_repair_debt_for_path(root, &path, Some(&records_by_text_id))?
                    {
                        if let Some(text_id) = debt.text_id.as_deref() {
                            existing_by_text_id.remove(text_id);
                        }
                        result.unchanged += 1;
                        continue;
                    }

                    if rfc_filename_number(&path).is_none() {
                        if let Ok(content) = std::fs::read_to_string(&path)
                            && let Some(text_id) = extract_anchor_ulid(&content)
                        {
                            existing_by_text_id.remove(&text_id);
                        }
                        result.unchanged += 1;
                        continue;
                    }

                    return Err(error);
                }
            };

            if let Some(existing) = existing_by_text_id.remove(&parsed.text_id) {
                if rfc_repair_candidate_for_path(
                    root,
                    &path,
                    parsed.clone(),
                    Some(&records_by_text_id),
                    RfcRepairCandidateMode::ReconcileExistingRow,
                )?
                .is_some()
                {
                    result.unchanged += 1;
                    continue;
                }

                let superseded_by = reconciled_relationship_value(
                    existing.superseded_by.as_deref(),
                    parsed.superseded_by.as_deref(),
                    parsed.superseded_by_declared,
                );
                let supersedes = reconciled_relationship_value(
                    existing.supersedes.as_deref(),
                    parsed.supersedes.as_deref(),
                    parsed.supersedes_declared,
                );
                let relationship_drifted = existing.superseded_by.as_deref() != superseded_by
                    || existing.supersedes.as_deref() != supersedes;

                let drifted = existing.rfc_number != parsed.rfc_number
                    || existing.title != parsed.title
                    || existing.stage != parsed.stage
                    || existing.status != parsed.status
                    || existing.slug != parsed.slug
                    || existing.file_path != parsed.file_path
                    || relationship_drifted;

                if drifted {
                    writer.upsert_rfc(
                        &parsed.text_id,
                        parsed.rfc_number,
                        &parsed.title,
                        parsed.stage,
                        &parsed.status,
                        existing.feature.as_deref(),
                        &parsed.slug,
                        &parsed.file_path,
                        superseded_by,
                        supersedes,
                        existing.withdrawal_reason.as_deref(),
                        existing.archived_reason.as_deref(),
                        existing.consolidated_into.as_deref(),
                    )?;
                    result.updated += 1;
                } else {
                    result.unchanged += 1;
                }
            } else if rfc_repair_candidate_for_path(
                root,
                &path,
                parsed.clone(),
                Some(&records_by_text_id),
                RfcRepairCandidateMode::Manual,
            )?
            .is_some()
            {
                result.unchanged += 1;
            } else {
                writer.upsert_rfc(
                    &parsed.text_id,
                    parsed.rfc_number,
                    &parsed.title,
                    parsed.stage,
                    &parsed.status,
                    None,
                    &parsed.slug,
                    &parsed.file_path,
                    parsed.superseded_by.as_deref(),
                    parsed.supersedes.as_deref(),
                    None,
                    None,
                    None,
                )?;
                result.inserted += 1;
            }
        }
    }

    for stale in existing_by_text_id.into_values() {
        let stale_path = root.join(&stale.file_path);
        if stale_path.exists() && is_rfc_document_path(&rfc_root, &stale_path) {
            result.unchanged += 1;
            continue;
        }
        writer.delete_rfc(&stale.text_id)?;
        result.deleted += 1;
    }

    Ok(result)
}

fn workspace_git_identity(root: &Path) -> Result<(Option<String>, String)> {
    let branch_name = git_stdout(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
        .filter(|branch| !branch.is_empty());
    let head_oid = git_stdout(root, &["rev-parse", "--verify", "HEAD^{commit}"])?
        .filter(|oid| !oid.is_empty())
        .unwrap_or_else(|| "unborn".to_string());
    Ok((branch_name, head_oid))
}

fn canonical_source_identity(
    source: &CanonicalReconcileSource,
) -> (Option<String>, Option<String>) {
    match source {
        CanonicalReconcileSource::Canonical(canonical) => (
            Some(canonical.ref_name.clone()),
            Some(canonical.oid.clone()),
        ),
        CanonicalReconcileSource::PreserveShared | CanonicalReconcileSource::WorkspaceFallback => {
            (None, None)
        }
    }
}

fn workspace_collection_kind(relative_path: &str) -> String {
    let path = Path::new(relative_path);
    let collection = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    match collection {
        Some("withdrawn") => "withdrawn".to_string(),
        Some("archive") => "archive".to_string(),
        Some(stage) if stage.starts_with("stage-") => stage.to_string(),
        _ => "legacy-flat".to_string(),
    }
}

fn update_digest_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn workspace_document_digest(documents: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for (path, bytes) in documents {
        update_digest_field(&mut hasher, path.as_bytes());
        update_digest_field(&mut hasher, workspace_collection_kind(path).as_bytes());
        let document_hash = Sha256::digest(bytes);
        update_digest_field(&mut hasher, &document_hash);
    }
    hasher.finalize().to_vec()
}

fn load_workspace_rfc_documents(root: &Path) -> Result<WorkspaceRfcDocuments> {
    #[cfg(test)]
    WORKSPACE_RFC_DOCUMENT_LOADS.with(|count| count.set(count.get() + 1));

    let mut documents = walk_rfc_markdown_files(&root.join(RFCS_DIR))
        .into_iter()
        .map(|path| {
            let relative_path = relative_workspace_path(root, &path);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            Ok((relative_path, bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(documents)
}

fn workspace_observation_from_parsed(
    workspace_root: &str,
    branch_name: Option<&str>,
    head_oid: &str,
    observed_at: &str,
    parsed: &ParsedRfcDocument,
) -> RfcWorkspaceObservation {
    RfcWorkspaceObservation {
        workspace_root: workspace_root.to_string(),
        text_id: parsed.disk.text_id.clone(),
        rfc_number: parsed.disk.rfc_number,
        title: parsed.disk.title.clone(),
        stage: parsed.disk.stage,
        stage_source: parsed.stage_source.clone(),
        status: parsed.disk.status.clone(),
        feature: parsed.feature.value.clone(),
        feature_declared: parsed.feature.declared,
        slug: parsed.disk.slug.clone(),
        file_path: parsed.disk.file_path.clone(),
        superseded_by: parsed.disk.superseded_by.clone(),
        superseded_by_declared: parsed.disk.superseded_by_declared,
        supersedes: parsed.disk.supersedes.clone(),
        supersedes_declared: parsed.disk.supersedes_declared,
        withdrawal_reason: parsed.withdrawal_reason.value.clone(),
        withdrawal_reason_declared: parsed.withdrawal_reason.declared,
        archived_reason: parsed.archived_reason.value.clone(),
        archived_reason_declared: parsed.archived_reason.declared,
        consolidated_into: parsed.consolidated_into.value.clone(),
        consolidated_into_declared: parsed.consolidated_into.declared,
        branch_name: branch_name.map(str::to_string),
        head_oid: head_oid.to_string(),
        observed_at: observed_at.to_string(),
    }
}

fn workspace_diagnostic(
    workspace_root: &str,
    file_path: &str,
    code: &str,
    content: &str,
    message: impl Into<String>,
    observed_at: &str,
) -> RfcWorkspaceDiagnostic {
    RfcWorkspaceDiagnostic {
        workspace_root: workspace_root.to_string(),
        file_path: file_path.to_string(),
        diagnostic_code: code.to_string(),
        text_id: extract_anchor_ulid(content),
        rfc_number: extract_anchor_rfc_number(content),
        message: message.into(),
        observed_at: observed_at.to_string(),
    }
}

fn refresh_workspace_rfc_snapshot(
    root: &Path,
    project: Option<&Project>,
    source: &CanonicalReconcileSource,
) -> Result<()> {
    refresh_workspace_rfc_snapshot_with_documents(root, project, source, None)
}

fn refresh_workspace_rfc_snapshot_with_documents(
    root: &Path,
    project: Option<&Project>,
    source: &CanonicalReconcileSource,
    workspace_documents: Option<&[(String, Vec<u8>)]>,
) -> Result<()> {
    let db_path = crate::context::db_path(root, project);
    let loader = SqliteLoader::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
    let writer = SqliteWriter::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
    let workspace_root = slash_path_string(&normalize_key_path(root));
    let (branch_name, head_oid) = workspace_git_identity(root)?;
    let (canonical_ref, canonical_oid) = canonical_source_identity(source);
    let observed_at = chrono::Utc::now().to_rfc3339();

    let shared_by_text_id = loader
        .load_rfcs()?
        .into_iter()
        .map(|record| (record.text_id.clone(), record))
        .collect::<HashMap<_, _>>();
    let owned_documents = workspace_documents
        .is_none()
        .then(|| load_workspace_rfc_documents(root))
        .transpose()?;
    let documents = workspace_documents
        .or(owned_documents.as_deref())
        .context("Workspace RFC documents are required for snapshot refresh")?;
    let document_digest = workspace_document_digest(documents);

    let previous_snapshot = loader.load_rfc_workspace_snapshot(&workspace_root)?;
    if previous_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.branch_name == branch_name
            && snapshot.head_oid == head_oid
            && snapshot.document_digest == document_digest
            && snapshot.canonical_ref == canonical_ref
            && snapshot.canonical_oid == canonical_oid
    }) {
        return Ok(());
    }

    let previous_observations = if previous_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.branch_name == branch_name
            && (branch_name.is_some()
                || snapshot.head_oid == head_oid
                || snapshot.document_digest == document_digest)
    }) {
        loader
            .load_rfc_workspace_observations(&workspace_root)?
            .into_iter()
            .map(|observation| (observation.text_id.clone(), observation))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };
    let mut diagnostics = Vec::new();
    let mut parsed_documents = Vec::new();
    for (relative_path, bytes) in documents {
        let Ok(content) = String::from_utf8(bytes.clone()) else {
            diagnostics.push(RfcWorkspaceDiagnostic {
                workspace_root: workspace_root.clone(),
                file_path: relative_path.clone(),
                diagnostic_code: "invalid_utf8".to_string(),
                text_id: None,
                rfc_number: None,
                message: format!("RFC document contains non-UTF-8 bytes: {relative_path}"),
                observed_at: observed_at.clone(),
            });
            continue;
        };
        let existing = extract_anchor_ulid(&content)
            .as_deref()
            .and_then(|text_id| shared_by_text_id.get(text_id));
        match parse_rfc_document(relative_path, &content, existing) {
            Ok(parsed)
                if !parsed.canonical_metadata_conflict
                    && parsed.filename_number == parsed.disk.rfc_number =>
            {
                parsed_documents.push((parsed, content));
            }
            Ok(_parsed) => diagnostics.push(workspace_diagnostic(
                &workspace_root,
                relative_path,
                "metadata_conflict",
                &content,
                format!(
                    "RFC document metadata conflicts with its path or lifecycle at {relative_path}"
                ),
                &observed_at,
            )),
            Err(error) => diagnostics.push(workspace_diagnostic(
                &workspace_root,
                relative_path,
                "parse_error",
                &content,
                format!("{error:#}"),
                &observed_at,
            )),
        }
    }

    let mut anchor_counts = HashMap::new();
    for (parsed, _) in &parsed_documents {
        *anchor_counts
            .entry(parsed.disk.text_id.clone())
            .or_insert(0_usize) += 1;
    }

    let mut observations = Vec::new();
    for (parsed, content) in parsed_documents {
        if anchor_counts.get(&parsed.disk.text_id) != Some(&1) {
            diagnostics.push(workspace_diagnostic(
                &workspace_root,
                &parsed.disk.file_path,
                "duplicate_anchor",
                &content,
                format!(
                    "RFC anchor {} appears in multiple workspace documents",
                    parsed.disk.text_id
                ),
                &observed_at,
            ));
            continue;
        }
        let mut observation = workspace_observation_from_parsed(
            &workspace_root,
            branch_name.as_deref(),
            &head_oid,
            &observed_at,
            &parsed,
        );
        if let Some(previous) = previous_observations.get(&observation.text_id) {
            let canonical = shared_by_text_id.get(&observation.text_id);
            preserve_workspace_optional_override(
                &mut observation.feature,
                &mut observation.feature_declared,
                &previous.feature,
                previous.feature_declared,
                canonical.and_then(|record| record.feature.as_ref()),
            );
            preserve_workspace_optional_override(
                &mut observation.superseded_by,
                &mut observation.superseded_by_declared,
                &previous.superseded_by,
                previous.superseded_by_declared,
                canonical.and_then(|record| record.superseded_by.as_ref()),
            );
            preserve_workspace_optional_override(
                &mut observation.supersedes,
                &mut observation.supersedes_declared,
                &previous.supersedes,
                previous.supersedes_declared,
                canonical.and_then(|record| record.supersedes.as_ref()),
            );
            preserve_workspace_optional_override(
                &mut observation.withdrawal_reason,
                &mut observation.withdrawal_reason_declared,
                &previous.withdrawal_reason,
                previous.withdrawal_reason_declared,
                canonical.and_then(|record| record.withdrawal_reason.as_ref()),
            );
            preserve_workspace_optional_override(
                &mut observation.archived_reason,
                &mut observation.archived_reason_declared,
                &previous.archived_reason,
                previous.archived_reason_declared,
                canonical.and_then(|record| record.archived_reason.as_ref()),
            );
            preserve_workspace_optional_override(
                &mut observation.consolidated_into,
                &mut observation.consolidated_into_declared,
                &previous.consolidated_into,
                previous.consolidated_into_declared,
                canonical.and_then(|record| record.consolidated_into.as_ref()),
            );
        }
        observations.push(observation);
    }

    let snapshot = RfcWorkspaceSnapshot {
        workspace_root,
        branch_name,
        head_oid,
        document_digest,
        canonical_ref,
        canonical_oid,
        observed_at,
    };
    writer.replace_rfc_workspace_snapshot(&snapshot, &observations, &diagnostics)
}

fn preserve_workspace_optional_override(
    observed: &mut Option<String>,
    declared: &mut bool,
    previous: &Option<String>,
    previous_declared: bool,
    canonical: Option<&String>,
) {
    if !*declared && previous_declared && previous.as_ref() != canonical {
        observed.clone_from(previous);
        *declared = true;
    }
}

fn composed_optional_value(
    observed: &Option<String>,
    declared: bool,
    canonical: Option<&String>,
) -> Option<String> {
    if declared {
        observed.clone()
    } else {
        canonical.cloned()
    }
}

fn effective_record_from_observation(
    observation: RfcWorkspaceObservation,
    canonical: Option<&RfcRecord>,
) -> RfcRecord {
    RfcRecord {
        text_id: observation.text_id,
        rfc_number: observation.rfc_number,
        title: observation.title,
        stage: observation.stage,
        status: observation.status,
        feature: composed_optional_value(
            &observation.feature,
            observation.feature_declared,
            canonical.and_then(|record| record.feature.as_ref()),
        ),
        slug: observation.slug,
        file_path: observation.file_path,
        superseded_by: composed_optional_value(
            &observation.superseded_by,
            observation.superseded_by_declared,
            canonical.and_then(|record| record.superseded_by.as_ref()),
        ),
        supersedes: composed_optional_value(
            &observation.supersedes,
            observation.supersedes_declared,
            canonical.and_then(|record| record.supersedes.as_ref()),
        ),
        withdrawal_reason: composed_optional_value(
            &observation.withdrawal_reason,
            observation.withdrawal_reason_declared,
            canonical.and_then(|record| record.withdrawal_reason.as_ref()),
        ),
        archived_reason: composed_optional_value(
            &observation.archived_reason,
            observation.archived_reason_declared,
            canonical.and_then(|record| record.archived_reason.as_ref()),
        ),
        consolidated_into: composed_optional_value(
            &observation.consolidated_into,
            observation.consolidated_into_declared,
            canonical.and_then(|record| record.consolidated_into.as_ref()),
        ),
    }
}

/// Load RFC metadata as observed by the issuing workspace.
#[allow(clippy::missing_errors_doc)]
pub fn load_effective_rfcs(
    root: &Path,
    project: Option<&Project>,
) -> Result<Vec<EffectiveRfcRecord>> {
    Ok(load_effective_rfc_view(root, project)?.records)
}

/// Reconcile canonical RFC metadata and return the issuing workspace's view.
///
/// Public RFC read commands use this entry point so a valid canonical document
/// can relink missing shared metadata before the response is composed. Internal
/// derived-state callers can continue using [`load_effective_rfcs`] when they
/// need workspace observations refreshed without reconciling shared canonical
/// metadata.
#[allow(clippy::missing_errors_doc)]
pub fn observe_effective_rfcs(
    root: &Path,
    project: Option<&Project>,
) -> Result<Vec<EffectiveRfcRecord>> {
    Ok(observe_effective_rfc_view_with_project(root, project)?
        .1
        .records)
}

/// Load one complete effective RFC view for the issuing workspace.
#[allow(clippy::missing_errors_doc)]
pub fn load_effective_rfc_view(root: &Path, project: Option<&Project>) -> Result<EffectiveRfcView> {
    observe_effective_rfc_view(root, project, false).map(|(_, view)| view)
}

fn compose_effective_rfc_view_locked(
    root: &Path,
    project: Option<&Project>,
    source: &CanonicalReconcileSource,
) -> Result<EffectiveRfcView> {
    let loader = SqliteLoader::open(crate::context::db_path(root, project))?;
    let workspace_root = slash_path_string(&normalize_key_path(root));
    let snapshot = loader
        .load_rfc_workspace_snapshot(&workspace_root)?
        .with_context(|| format!("RFC workspace snapshot missing for {workspace_root}"))?;
    let canonical_records = loader.load_rfcs()?;
    let mut shared = canonical_records
        .iter()
        .cloned()
        .map(|record| (record.text_id.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut effective = Vec::new();

    for observation in loader.load_rfc_workspace_observations(&workspace_root)? {
        let canonical = shared.remove(&observation.text_id);
        let workspace_branch = observation.branch_name.clone();
        let workspace_head = Some(observation.head_oid.clone());
        let record = effective_record_from_observation(observation, canonical.as_ref());
        let differs_from_canonical = canonical
            .as_ref()
            .is_none_or(|canonical| !rfc_records_equal(canonical, &record));
        effective.push(EffectiveRfcRecord {
            record,
            provenance: RfcViewProvenance {
                document_source: "workspace".to_string(),
                workspace_presence: "present".to_string(),
                canonical_presence: if canonical.is_some() {
                    "present".to_string()
                } else {
                    "unpublished".to_string()
                },
                workspace_branch,
                workspace_head,
                canonical_ref: snapshot.canonical_ref.clone(),
                canonical_head: snapshot.canonical_oid.clone(),
                differs_from_canonical,
            },
        });
    }

    for record in shared.into_values() {
        effective.push(EffectiveRfcRecord {
            record,
            provenance: RfcViewProvenance {
                document_source: "canonical".to_string(),
                workspace_presence: "absent".to_string(),
                canonical_presence: "present".to_string(),
                workspace_branch: snapshot.branch_name.clone(),
                workspace_head: Some(snapshot.head_oid.clone()),
                canonical_ref: snapshot.canonical_ref.clone(),
                canonical_head: snapshot.canonical_oid.clone(),
                differs_from_canonical: false,
            },
        });
    }

    effective.sort_by(|left, right| {
        left.record
            .rfc_number
            .cmp(&right.record.rfc_number)
            .then(left.record.file_path.cmp(&right.record.file_path))
    });
    let workspace_diagnostics = loader.load_rfc_workspace_diagnostics(&workspace_root)?;
    let repair_records = if matches!(source, CanonicalReconcileSource::WorkspaceFallback) {
        canonical_records
    } else {
        effective
            .iter()
            .map(|record| record.record.clone())
            .collect()
    };
    Ok(EffectiveRfcView {
        records: effective,
        workspace_diagnostics,
        repair_records,
        snapshot,
    })
}

/// Load parse and identity diagnostics for the issuing workspace's RFC snapshot.
#[allow(clippy::missing_errors_doc)]
pub fn load_effective_rfc_diagnostics(
    root: &Path,
    project: Option<&Project>,
) -> Result<Vec<RfcWorkspaceDiagnostic>> {
    Ok(load_effective_rfc_view(root, project)?.workspace_diagnostics)
}

/// Resolve one RFC number in the issuing workspace's effective view.
#[allow(clippy::missing_errors_doc)]
pub fn load_effective_rfc_by_number(
    root: &Path,
    project: Option<&Project>,
    rfc_number: i64,
) -> Result<Option<EffectiveRfcRecord>> {
    select_effective_rfc_by_number(load_effective_rfcs(root, project)?, rfc_number)
}

/// Reconcile canonical RFC metadata and resolve one RFC in the workspace view.
#[allow(clippy::missing_errors_doc)]
pub fn observe_effective_rfc_by_number(
    root: &Path,
    project: Option<&Project>,
    rfc_number: i64,
) -> Result<Option<EffectiveRfcRecord>> {
    with_reconcile_lock(root, project, || {
        let transaction =
            exosuit_storage::RequestTransaction::begin(crate::context::db_path(root, project))?;
        let source = canonical_reconcile_source(root)?;
        reconcile_and_refresh_locked(root, project, &source, true)?;
        let view = compose_effective_rfc_view_locked(root, project, &source)?;
        let record = select_effective_rfc_by_number(view.records, rfc_number)?;
        if record.is_some() {
            transaction.commit()?;
        }
        Ok(record)
    })
}

fn select_effective_rfc_by_number(
    records: Vec<EffectiveRfcRecord>,
    rfc_number: i64,
) -> Result<Option<EffectiveRfcRecord>> {
    let mut matches = records
        .into_iter()
        .filter(|record| record.record.rfc_number == rfc_number)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        let identities = matches
            .iter()
            .map(|record| format!("{} ({})", record.record.file_path, record.record.text_id))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("RFC {rfc_number} is ambiguous in this workspace: {identities}");
    }
    Ok(matches.pop())
}

#[allow(clippy::missing_errors_doc)]
pub fn get_rfcs(path: &Path, verify: bool) -> Result<(Vec<Rfc>, bool)> {
    let mut rfcs = Vec::new();
    let mut has_errors = false;

    for entry in WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let filename = match path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            // Skip template and README
            if filename == "0000-template.md" || filename == "README.md" {
                continue;
            }

            // Infer stage from directory structure
            let parent_dir = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str());
            let dir_stage = parent_dir.and_then(|name| {
                if name.starts_with("stage-") {
                    name.strip_prefix("stage-")?.parse::<u8>().ok()
                } else {
                    None
                }
            });

            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;

            let rfc = parse_rfc(&filename, &content, dir_stage);

            if verify {
                let mut file_errors = Vec::new();
                // Verification logic could be expanded here
                if rfc.title == "Untitled" {
                    file_errors.push("Missing title");
                }

                if !file_errors.is_empty() {
                    has_errors = true;
                    eprintln!("Error in {filename}:");
                    for err in file_errors {
                        eprintln!("  - {err}");
                    }
                }
            }

            rfcs.push(rfc);
        }
    }

    // Sort by Stage (descending), then Number (ascending)
    rfcs.sort_by(|a, b| b.stage.cmp(&a.stage).then(a.number.cmp(&b.number)));

    Ok((rfcs, has_errors))
}

#[allow(clippy::missing_errors_doc)]
pub fn get_next_rfc_id(rfc_dir: &Path) -> Result<String> {
    if !rfc_dir.exists() {
        return Ok(format!("{:05}", 1));
    }

    let mut max_num = 0u32;

    for entry in WalkDir::new(rfc_dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        if !is_rfc_document_path(rfc_dir, path) {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Skip template and README
        if filename == "0000-template.md" || filename == "README.md" {
            continue;
        }

        let Some(prefix) = extract_rfc_number_from_filename(filename) else {
            continue;
        };
        let Ok(value) = prefix.parse::<u32>() else {
            continue;
        };
        max_num = max_num.max(value);
    }

    Ok(format!("{:05}", max_num + 1))
}

fn get_next_effective_rfc_id(
    rfc_dir: &Path,
    effective_rfcs: &[EffectiveRfcRecord],
) -> Result<String> {
    let next_file_number = get_next_rfc_id(rfc_dir)?
        .parse::<i64>()
        .context("Generated RFC ID was not numeric")?;
    let next_effective_number = effective_rfcs
        .iter()
        .map(|effective| effective.record.rfc_number)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .context("RFC number space is exhausted")?;
    Ok(format!(
        "{:05}",
        next_file_number.max(next_effective_number)
    ))
}

pub fn create(
    root: &Path,
    title: &str,
    id: Option<&str>,
    feature: &str,
    stage: u8,
    body: Option<&str>,
) -> Result<std::path::PathBuf> {
    let rfc_dir = root.join(RFCS_DIR);
    let stage_dir = rfc_dir.join(format!("stage-{stage}"));

    let project = Project::resolve(root).ok();
    let effective_rfcs = if let Some(project) = project.as_ref() {
        load_effective_rfcs(root, Some(project))?
    } else {
        Vec::new()
    };

    if !stage_dir.exists() {
        std::fs::create_dir_all(&stage_dir)?;
    }

    let number = if let Some(id) = id {
        id.to_string()
    } else {
        get_next_effective_rfc_id(&rfc_dir, &effective_rfcs)?
    };

    let slug = slugify_title(title);
    let filename = format!("{number}-{slug}.md");
    let file_path = stage_dir.join(&filename);

    let rfc_number = number
        .parse::<i64>()
        .with_context(|| format!("RFC IDs must be numeric: {number}"))?;
    if effective_rfcs
        .iter()
        .any(|effective| effective.record.rfc_number == rfc_number)
    {
        anyhow::bail!("RFC {number} already exists in the effective workspace view");
    }
    let text_id = ulid::Ulid::new().to_string().to_lowercase();
    let body_content = body.unwrap_or("Write your RFC content here.");
    let content = render_anchor_rfc_content(rfc_number, &text_id, title, body_content);

    std::fs::write(&file_path, content)?;

    let relative_path = relative_workspace_path(root, &file_path);
    let record = RfcRecord {
        text_id,
        rfc_number,
        title: title.to_string(),
        stage,
        status: "active".to_string(),
        feature: Some(feature.to_string()),
        slug,
        file_path: relative_path,
        superseded_by: None,
        supersedes: None,
        withdrawal_reason: None,
        archived_reason: None,
        consolidated_into: None,
    };
    persist_rfc_record(root, &record)?;

    Ok(file_path)
}

/// Convert an RFC title into a stable filesystem slug.
///
/// This is used for naming RFC files (`{number}-{slug}.md`) and for verifiers
/// that check common papercuts (e.g. `*-untitled.md` after a real title exists).
pub fn slugify_title(title: &str) -> String {
    crate::utils::slugify(title)
}

/// Edit an existing RFC.
///
/// This is a tool-mediated mutation intended to replace manual edits.
///
/// If `body` is provided, the RFC body is replaced (the file is re-rendered)
/// using the provided body content under a canonical H1.
///
/// If `title`, `feature`, or `stage` are provided without `body`, we update
/// frontmatter and (for `title`) also update the first H1 line when present.
///
/// # Errors
///
/// Returns an error if:
/// - The RFC cannot be uniquely located.
/// - The file cannot be read or written.
#[allow(clippy::missing_errors_doc)]
pub fn edit(
    root: &Path,
    id: &str,
    title: Option<&str>,
    feature: Option<&str>,
    stage: Option<u8>,
    body: Option<&str>,
) -> Result<std::path::PathBuf> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        anyhow::bail!("RFC root not found at {}", rfc_root.display());
    }

    let file_path = find_rfc_file(&rfc_root, id)?;
    ensure_no_rfc_identity_repair_debt(root, &file_path)?;
    edit_file(root, file_path, id, title, feature, stage, body)
}

/// Edit an RFC by explicit path (absolute or workspace-relative).
pub fn edit_by_path(
    root: &Path,
    path: &str,
    id: Option<&str>,
    title: Option<&str>,
    feature: Option<&str>,
    stage: Option<u8>,
    body: Option<&str>,
) -> Result<std::path::PathBuf> {
    let file_path = {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(path)
        }
    };

    if !file_path.exists() {
        anyhow::bail!("RFC file not found: {}", file_path.display());
    }
    ensure_no_rfc_identity_repair_debt(root, &file_path)?;

    // If the ID isn't provided, infer it from filename.
    let inferred_id = id.map_or_else(
        || {
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split(['-', '_']).next())
                .unwrap_or("????")
                .to_string()
        },
        std::string::ToString::to_string,
    );

    edit_file(root, file_path, &inferred_id, title, feature, stage, body)
}

/// Rename an RFC file to match the slugified title.
///
/// This does not modify the RFC contents; it only renames the file within its
/// existing stage directory.
pub fn rename(root: &Path, id: &str) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        anyhow::bail!("RFC root not found at {}", rfc_root.display());
    }

    let old_path = find_rfc_file(&rfc_root, id)?;
    ensure_no_blocking_rfc_rename_debt(root, &old_path)?;
    let original = std::fs::read_to_string(&old_path)
        .with_context(|| format!("Failed to read {}", old_path.display()))?;

    let dir_stage = infer_stage_from_path(&old_path);
    let filename = old_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("????.md");
    let parsed = parse_rfc(filename, &original, dir_stage);

    let slug = slugify_title(&parsed.title);
    let slug = if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    };
    let new_filename = format!("{}-{slug}.md", parsed.number);

    let Some(parent) = old_path.parent() else {
        anyhow::bail!("RFC path had no parent directory: {}", old_path.display());
    };
    let new_path = parent.join(new_filename);

    if new_path == old_path {
        sync_rfc_rename(root, &new_path)?;
        return Ok((old_path, new_path));
    }

    std::fs::rename(&old_path, &new_path).with_context(|| {
        format!(
            "Failed to rename RFC {} from {} to {}",
            id,
            old_path.display(),
            new_path.display()
        )
    })?;

    sync_rfc_rename(root, &new_path)?;

    Ok((old_path, new_path))
}

#[allow(clippy::missing_errors_doc)]
pub fn detect_rfc_repair_candidates(root: &Path) -> Result<Vec<RfcRepairCandidate>> {
    let rfc_root = root.join(RFCS_DIR);
    if !rfc_root.exists() {
        return Ok(Vec::new());
    }

    let records = load_effective_rfc_records_if_available(root)?;
    detect_rfc_repair_candidates_with_records_inner(root, &records)
}

#[allow(clippy::missing_errors_doc)]
pub(crate) fn detect_rfc_repair_candidates_with_records(
    root: &Path,
    records: &[RfcRecord],
) -> Result<Vec<RfcRepairCandidate>> {
    detect_rfc_repair_candidates_with_records_inner(root, records)
}

fn detect_rfc_repair_candidates_with_records_inner(
    root: &Path,
    records: &[RfcRecord],
) -> Result<Vec<RfcRepairCandidate>> {
    let rfc_root = root.join(RFCS_DIR);
    if !rfc_root.exists() {
        return Ok(Vec::new());
    }
    let records_by_text_id = records
        .iter()
        .cloned()
        .map(|record| (record.text_id.clone(), record))
        .collect::<HashMap<_, _>>();

    let mut candidates = Vec::new();
    for path in walk_rfc_markdown_files(&rfc_root) {
        let parsed = match parse_disk_rfc(root, &path) {
            Ok(parsed) => parsed,
            Err(_) => {
                if let Some(debt) =
                    malformed_rfc_repair_debt_for_path(root, &path, Some(&records_by_text_id))?
                {
                    candidates.push(debt.candidate);
                }
                continue;
            }
        };
        let candidate = rfc_repair_candidate_for_path(
            root,
            &path,
            parsed,
            Some(&records_by_text_id),
            RfcRepairCandidateMode::Manual,
        )?;
        if let Some(candidate) = candidate {
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|a, b| a.id.cmp(&b.id).then(a.current_path.cmp(&b.current_path)));
    Ok(candidates)
}

#[allow(clippy::missing_errors_doc)]
pub fn detect_rfc_repair_candidate_for_text_id(
    root: &Path,
    text_id: &str,
) -> Result<Option<RfcRepairCandidate>> {
    let rfc_root = root.join(RFCS_DIR);
    if !rfc_root.exists() {
        return Ok(None);
    }

    let records_by_text_id: HashMap<String, RfcRecord> =
        load_effective_rfc_records_if_available(root)?
            .into_iter()
            .map(|record| (record.text_id.clone(), record))
            .collect();

    for path in walk_rfc_markdown_files(&rfc_root) {
        let Ok(parsed) = parse_disk_rfc(root, &path) else {
            if let Some(debt) =
                malformed_rfc_repair_debt_for_path(root, &path, Some(&records_by_text_id))?
                && debt.text_id.as_deref() == Some(text_id)
            {
                return Ok(Some(debt.candidate));
            }
            continue;
        };
        if parsed.text_id != text_id {
            continue;
        }
        return rfc_repair_candidate_for_path(
            root,
            &path,
            parsed,
            Some(&records_by_text_id),
            RfcRepairCandidateMode::Manual,
        );
    }

    Ok(None)
}

#[allow(clippy::missing_errors_doc)]
pub fn repair(root: &Path, id: &str) -> Result<RfcRepairOutcome> {
    repair_with_options(root, id, None, None)
}

#[allow(clippy::missing_errors_doc)]
pub fn repair_with_options(
    root: &Path,
    id: &str,
    path: Option<&str>,
    renumber_to: Option<&str>,
) -> Result<RfcRepairOutcome> {
    let rfc_root = root.join(RFCS_DIR);
    if !rfc_root.exists() {
        anyhow::bail!("RFC root not found at {}", rfc_root.display());
    }

    let old_path = match path {
        Some(path) => resolve_rfc_file_for_repair_path(root, &rfc_root, path, id)?,
        None => find_rfc_file_for_repair(root, &rfc_root, id)?,
    };
    let (parsed, candidate, malformed_repaired) = match parse_disk_rfc(root, &old_path) {
        Ok(parsed) => {
            let candidate = rfc_repair_candidate_for_path(
                root,
                &old_path,
                parsed.clone(),
                None,
                RfcRepairCandidateMode::Manual,
            )?;
            (parsed, candidate, false)
        }
        Err(error) => {
            let Some(debt) = malformed_rfc_repair_debt_for_path(root, &old_path, None)? else {
                return Err(error);
            };
            repair_malformed_rfc_anchor(root, &old_path, &debt)?;
            let parsed = parse_disk_rfc(root, &old_path)?;
            (parsed, Some(debt.candidate), true)
        }
    };
    let mut identity = rfc_repair_identity(root, &old_path, &parsed)?;
    let renumbered_to = match renumber_to {
        Some(value) => {
            let target_number = validate_rfc_id(value)
                .with_context(|| format!("Invalid target RFC ID: {value}"))?;
            ensure_rfc_number_available_for_renumber(root, &rfc_root, target_number, &old_path)?;
            identity.rfc_number = target_number;
            Some(format_rfc_number(target_number))
        }
        None => None,
    };
    let expected_path = expected_rfc_path(
        root,
        &old_path,
        &parsed,
        Some(identity.rfc_number),
        identity.preferred_width,
    )?;
    let old_rel = relative_workspace_path(root, &old_path);
    let new_rel = relative_workspace_path(root, &expected_path);
    let mut reasons = candidate
        .as_ref()
        .map_or_else(Vec::new, |candidate| candidate.reasons.clone());
    if renumbered_to.is_some()
        && !reasons
            .iter()
            .any(|reason| reason == "rfc_number_reassigned")
    {
        reasons.push("rfc_number_reassigned".to_string());
    }

    let mut final_path = old_path.clone();
    let moved = expected_path != old_path;
    if moved {
        if expected_path.exists() {
            anyhow::bail!(
                "Refusing to repair RFC {id}: expected path already exists: {}",
                expected_path.display()
            );
        }
        if let Some(parent) = expected_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        move_rfc_file(root, &old_path, &expected_path)?;
        final_path = expected_path;
    }

    let final_parsed = parse_disk_rfc(root, &final_path)?;
    let metadata_sync_needed = rfc_metadata_sync_needed(root, &final_parsed)?;
    if moved && reasons.is_empty() {
        reasons.push("filename_policy_drift".to_string());
    }
    if metadata_sync_needed && reasons.is_empty() {
        reasons.push("metadata_relink".to_string());
    }
    let expected_text_id = identity.text_id.as_deref().unwrap_or(&final_parsed.text_id);
    if let Some(display_id) = renumbered_to.as_deref() {
        repair_rfc_file_identity(
            &final_path,
            identity.rfc_number,
            expected_text_id,
            display_id,
            &final_parsed.title,
        )?;
    } else if final_parsed.rfc_number != identity.rfc_number
        || final_parsed.text_id != expected_text_id
    {
        repair_anchor_identity(&final_path, identity.rfc_number, expected_text_id)?;
    }
    if renumbered_to.is_some() {
        sync_rfc_renumber(root, &final_path)?;
    } else {
        sync_rfc_rename(root, &final_path)?;
    }
    let outcome_id = renumbered_to.clone().unwrap_or_else(|| {
        candidate.as_ref().map_or_else(
            || format_rfc_number(identity.rfc_number),
            |candidate| candidate.id.clone(),
        )
    });

    Ok(RfcRepairOutcome {
        id: outcome_id,
        old_path: old_rel,
        new_path: new_rel,
        title: parsed.title,
        reasons,
        repaired: candidate.is_some()
            || moved
            || metadata_sync_needed
            || malformed_repaired
            || renumbered_to.is_some(),
        renumbered_to,
    })
}

fn resolve_rfc_file_for_repair_path(
    root: &Path,
    rfc_root: &Path,
    path: &str,
    id: &str,
) -> Result<PathBuf> {
    let requested_number = validate_rfc_id(id)?;
    let file_path = {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    };

    if !file_path.exists() {
        anyhow::bail!("RFC file not found: {}", file_path.display());
    }
    if !is_rfc_document_path(rfc_root, &file_path) {
        anyhow::bail!(
            "Path is not a managed RFC document: {}",
            file_path.display()
        );
    }

    let filename_matches = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse_rfc_number)
        .is_some_and(|number| number == requested_number);
    let path_matches = match parse_disk_rfc(root, &file_path) {
        Ok(parsed) => {
            let metadata_matches = load_rfc_record_by_text_id(root, &parsed.text_id)?
                .is_some_and(|record| record.rfc_number == requested_number);
            parsed.rfc_number == requested_number || metadata_matches
        }
        Err(_) => malformed_rfc_repair_debt_for_path(root, &file_path, None)?
            .is_some_and(|debt| debt.identity_number == requested_number),
    };

    if !filename_matches && !path_matches {
        anyhow::bail!(
            "RFC path {} does not match requested RFC ID {id}",
            relative_workspace_path(root, &file_path)
        );
    }

    Ok(file_path)
}

fn ensure_rfc_number_available_for_renumber(
    root: &Path,
    rfc_root: &Path,
    target_number: i64,
    current_path: &Path,
) -> Result<()> {
    if let Some(existing_path) = find_optional_rfc_file(rfc_root, &target_number.to_string())?
        && existing_path != current_path
    {
        anyhow::bail!(
            "Refusing to renumber RFC to {}: target path already exists at {}",
            format_rfc_number(target_number),
            existing_path.display()
        );
    }

    let current_rel = relative_workspace_path(root, current_path);
    for record in load_effective_rfc_records_if_available(root)? {
        if record.rfc_number == target_number && record.file_path != current_rel {
            anyhow::bail!(
                "Refusing to renumber RFC to {}: metadata already exists for {} at {}",
                format_rfc_number(target_number),
                record.title,
                record.file_path
            );
        }
    }

    Ok(())
}

struct RfcRepairIdentity {
    rfc_number: i64,
    preferred_width: Option<usize>,
    text_id: Option<String>,
}

fn rfc_repair_identity(
    root: &Path,
    path: &Path,
    parsed: &DiskRfcRecord,
) -> Result<RfcRepairIdentity> {
    let current_path = relative_workspace_path(root, path);
    let Some(record) = matching_rfc_record_for_repair_debt(
        root,
        Some(&parsed.text_id),
        Some(parsed.rfc_number),
        &current_path,
        None,
        Some(&parsed.title),
    )?
    else {
        return Ok(RfcRepairIdentity {
            rfc_number: parsed.rfc_number,
            preferred_width: None,
            text_id: None,
        });
    };
    let visible_number = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse_rfc_number);
    let preferred_width = if visible_number.is_some_and(|number| number != record.rfc_number) {
        rfc_number_width_from_path(&record.file_path)
    } else {
        None
    };
    Ok(RfcRepairIdentity {
        rfc_number: record.rfc_number,
        preferred_width,
        text_id: Some(record.text_id),
    })
}

fn find_rfc_file_for_repair(root: &Path, rfc_root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("Invalid RFC ID: {id}");
    }
    let requested_number = parse_rfc_number(id).with_context(|| format!("Invalid RFC ID: {id}"))?;
    let records_by_text_id: HashMap<String, RfcRecord> =
        load_effective_rfc_records_if_available(root)?
            .into_iter()
            .map(|record| (record.text_id.clone(), record))
            .collect();

    let mut matches = Vec::new();
    for path in walk_rfc_markdown_files(rfc_root) {
        let filename_matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_rfc_number)
            .is_some_and(|number| number == requested_number);
        let parsed = parse_disk_rfc(root, &path).ok();
        let malformed_debt = if parsed.is_none() {
            malformed_rfc_repair_debt_for_path(root, &path, Some(&records_by_text_id))?
        } else {
            None
        };
        let anchor_matches = parsed
            .as_ref()
            .is_some_and(|parsed| parsed.rfc_number == requested_number);
        let metadata_matches = parsed
            .as_ref()
            .and_then(|parsed| records_by_text_id.get(&parsed.text_id).cloned())
            .is_some_and(|record| record.rfc_number == requested_number);
        let malformed_matches = malformed_debt
            .as_ref()
            .is_some_and(|debt| debt.identity_number == requested_number);

        if filename_matches || anchor_matches || metadata_matches || malformed_matches {
            matches.push(path);
        }
    }

    match matches.len() {
        0 => anyhow::bail!("RFC {id} not found under {}", rfc_root.display()),
        1 => Ok(matches.remove(0)),
        _ => {
            let rendered = matches
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n  - ");
            anyhow::bail!("RFC {id} is ambiguous; multiple files matched:\n  - {rendered}")
        }
    }
}

fn rfc_repair_candidate_for_path(
    root: &Path,
    path: &Path,
    parsed: DiskRfcRecord,
    records_by_text_id: Option<&HashMap<String, RfcRecord>>,
    mode: RfcRepairCandidateMode,
) -> Result<Option<RfcRepairCandidate>> {
    let current_path = relative_workspace_path(root, path);
    let record = matching_rfc_record_for_repair_debt(
        root,
        Some(&parsed.text_id),
        Some(parsed.rfc_number),
        &current_path,
        records_by_text_id,
        Some(&parsed.title),
    )?;
    let identity_number = record
        .as_ref()
        .map_or(parsed.rfc_number, |record| record.rfc_number);
    let visible_number = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse_rfc_number);
    let preferred_width = record.as_ref().and_then(|record| {
        visible_number
            .is_some_and(|number| number != record.rfc_number)
            .then(|| rfc_number_width_from_path(&record.file_path))
            .flatten()
    });
    let expected_path =
        expected_rfc_path(root, path, &parsed, Some(identity_number), preferred_width)?;
    let expected_rel = relative_workspace_path(root, &expected_path);
    let expected_filename = expected_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let expected_prefix = extract_rfc_number_from_filename(expected_filename).unwrap_or_default();
    let expected_slug = parse_slug(expected_filename);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let current_prefix = extract_rfc_number_from_filename(filename).unwrap_or_default();
    let current_visible_number = parse_rfc_number(filename);
    let current_slug = filename_slug(filename).unwrap_or_default();

    let mut reasons = Vec::new();
    if rfc_prefix_width_drifted(&current_prefix, &expected_prefix) {
        reasons.push("unexpected_rfc_number_width".to_string());
    }
    if current_slug != expected_slug && is_unexpected_slug_drift(&current_slug) {
        reasons.push("filename_slug_drift".to_string());
    }
    let stored_metadata = record.as_ref().map(rfc_repair_stored_metadata);

    match record.as_ref() {
        Some(record) => {
            if parsed.text_id != record.text_id {
                reasons.push("anchor_ulid_drift".to_string());
            }
            if parsed.rfc_number != record.rfc_number {
                reasons.push("anchor_rfc_number_drift".to_string());
            }
            if current_visible_number.is_some_and(|number| number != record.rfc_number) {
                reasons.push("filename_rfc_number_drift".to_string());
            }
            if record.file_path != current_path {
                reasons.push("metadata_path_drift".to_string());
            }
        }
        None => {
            if current_visible_number.is_some_and(|number| number != parsed.rfc_number) {
                reasons.push("filename_rfc_number_drift".to_string());
            }
            reasons.push("metadata_relink".to_string());
        }
    }

    if reasons.is_empty() {
        return Ok(None);
    }
    if mode == RfcRepairCandidateMode::ReconcileExistingRow
        && is_safe_metadata_only_rfc_drift(&parsed, &reasons, &current_path, &expected_rel)
    {
        return Ok(None);
    }

    Ok(Some(RfcRepairCandidate {
        id: format_rfc_number(identity_number),
        current_path,
        expected_path: expected_rel,
        title: parsed.title,
        reasons,
        stored_metadata,
    }))
}

fn rfc_repair_stored_metadata(record: &RfcRecord) -> RfcRepairStoredMetadata {
    RfcRepairStoredMetadata {
        path: record.file_path.clone(),
        stage: record.stage,
        status: record.status.clone(),
        slug: record.slug.clone(),
        title: record.title.clone(),
    }
}

fn is_safe_metadata_only_rfc_drift(
    parsed: &DiskRfcRecord,
    reasons: &[String],
    current_path: &str,
    expected_path: &str,
) -> bool {
    let safe_status = parsed.status == "active"
        || (matches!(parsed.status.as_str(), "archived" | "withdrawn")
            && parsed.lifecycle_status_declared);
    safe_status
        && current_path == expected_path
        && reasons.iter().all(|reason| reason == "metadata_path_drift")
}

fn malformed_rfc_repair_debt_for_path(
    root: &Path,
    path: &Path,
    records_by_text_id: Option<&HashMap<String, RfcRecord>>,
) -> Result<Option<MalformedRfcRepairDebt>> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("Invalid RFC filename: {}", path.display()))?;
    let Some(visible_number) = parse_rfc_number(filename) else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let text_id = extract_anchor_ulid(&content);
    let anchor_number = extract_partial_anchor_rfc_number(&content);

    if text_id.is_some() && anchor_number.is_some() {
        return Ok(None);
    }

    let current_path = relative_workspace_path(root, path);
    let title = extract_h1_title(&content).unwrap_or_else(|| format!("RFC {visible_number}"));
    let record = matching_rfc_record_for_repair_debt(
        root,
        text_id.as_deref(),
        anchor_number.or(Some(visible_number)),
        &current_path,
        records_by_text_id,
        Some(&title),
    )?;
    let identity_number = record
        .as_ref()
        .map_or(visible_number, |record| record.rfc_number);
    let preferred_width = record.as_ref().and_then(|record| {
        (visible_number != record.rfc_number)
            .then(|| rfc_number_width_from_path(&record.file_path))
            .flatten()
    });
    let parsed = DiskRfcRecord {
        text_id: record
            .as_ref()
            .map(|record| record.text_id.clone())
            .or_else(|| text_id.clone())
            .unwrap_or_default(),
        rfc_number: identity_number,
        title: title.clone(),
        stage: parse_stage(path),
        status: parse_status(path).to_string(),
        lifecycle_status_declared: false,
        slug: parse_slug(filename),
        file_path: current_path.clone(),
        superseded_by: None,
        supersedes: None,
        superseded_by_declared: false,
        supersedes_declared: false,
    };
    let expected_path =
        expected_rfc_path(root, path, &parsed, Some(identity_number), preferred_width)?;
    let expected_rel = relative_workspace_path(root, &expected_path);
    let expected_filename = expected_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let expected_prefix = extract_rfc_number_from_filename(expected_filename).unwrap_or_default();
    let expected_slug = parse_slug(expected_filename);
    let current_prefix = extract_rfc_number_from_filename(filename).unwrap_or_default();
    let current_slug = filename_slug(filename).unwrap_or_default();

    let mut reasons = Vec::new();
    if !has_anchor(&content) {
        reasons.push("missing_anchor".to_string());
    } else if text_id.is_none() {
        reasons.push(anchor_ulid_repair_reason(&content).to_string());
    }
    if has_anchor(&content) && anchor_number.is_none() {
        reasons.push("invalid_anchor_number".to_string());
    }
    if anchor_number.is_some_and(|number| number != identity_number) {
        reasons.push("anchor_rfc_number_drift".to_string());
    }
    if visible_number != identity_number {
        reasons.push("filename_rfc_number_drift".to_string());
    }
    if rfc_prefix_width_drifted(&current_prefix, &expected_prefix) {
        reasons.push("unexpected_rfc_number_width".to_string());
    }
    if current_slug != expected_slug && is_unexpected_slug_drift(&current_slug) {
        reasons.push("filename_slug_drift".to_string());
    }
    let stored_metadata = record.as_ref().map(rfc_repair_stored_metadata);

    match record.as_ref() {
        Some(record) if record.file_path != current_path => {
            reasons.push("metadata_path_drift".to_string());
        }
        None => reasons.push("metadata_relink".to_string()),
        Some(_) => {}
    }

    if reasons.is_empty() {
        return Ok(None);
    }

    let id = if visible_number == identity_number && !current_prefix.is_empty() {
        current_prefix
    } else {
        format_rfc_number(identity_number)
    };
    Ok(Some(MalformedRfcRepairDebt {
        identity_number,
        text_id: record.map(|record| record.text_id).or(text_id),
        file_path: current_path.clone(),
        candidate: RfcRepairCandidate {
            id,
            current_path,
            expected_path: expected_rel,
            title,
            reasons,
            stored_metadata,
        },
    }))
}

fn matching_rfc_record_for_repair_debt(
    root: &Path,
    text_id: Option<&str>,
    rfc_number: Option<i64>,
    current_path: &str,
    records_by_text_id: Option<&HashMap<String, RfcRecord>>,
    title: Option<&str>,
) -> Result<Option<RfcRecord>> {
    if let Some(records_by_text_id) = records_by_text_id {
        return Ok(select_rfc_record_for_repair_debt(
            records_by_text_id.values(),
            text_id,
            rfc_number,
            current_path,
            title,
        ));
    }

    let records = load_effective_rfc_records_if_available(root)?;
    Ok(select_rfc_record_for_repair_debt(
        records.iter(),
        text_id,
        rfc_number,
        current_path,
        title,
    ))
}

/// Choose the metadata row a repair should target.
///
/// A `text_id` match normally wins. But if the anchor's ULID was copied from
/// another RFC, trusting it would rewrite *that* RFC's metadata to this file's
/// path. So when the text_id-matched row points at a different path and a
/// different row already owns this file's path, the path match wins and the
/// copied anchor gets repaired back to the file's own identity.
fn select_rfc_record_for_repair_debt<'a>(
    records: impl Iterator<Item = &'a RfcRecord> + Clone,
    text_id: Option<&str>,
    rfc_number: Option<i64>,
    current_path: &str,
    title: Option<&str>,
) -> Option<RfcRecord> {
    let text_id_match = text_id.and_then(|text_id| {
        records
            .clone()
            .find(|record| record.text_id == text_id)
            .cloned()
    });
    let path_match = records
        .clone()
        .find(|record| record.file_path == current_path)
        .cloned();

    match (text_id_match, path_match) {
        (Some(by_ulid), Some(by_path)) if by_ulid.text_id != by_path.text_id => {
            // Conflicting claims: the anchor ULID belongs to a row that lives
            // elsewhere, while another row owns this exact path. Treat the
            // ULID as copied and repair toward the path owner.
            if by_ulid.file_path == current_path {
                Some(by_ulid)
            } else {
                Some(by_path)
            }
        }
        (Some(by_ulid), _) => Some(by_ulid),
        (None, Some(by_path)) => Some(by_path),
        (None, None) => {
            rfc_number.and_then(|number| unique_rfc_record_by_number(records, number, title))
        }
    }
}

fn unique_rfc_record_by_number<'a>(
    records: impl Iterator<Item = &'a RfcRecord>,
    rfc_number: i64,
    title: Option<&str>,
) -> Option<RfcRecord> {
    let mut matches = records.filter(|record| {
        record.rfc_number == rfc_number && title.is_none_or(|title| record.title == title)
    });
    let first = matches.next()?.clone();
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn extract_partial_anchor_rfc_number(content: &str) -> Option<i64> {
    let re = PARTIAL_ANCHOR_RFC_NUMBER_RE.as_ref().ok()?;
    re.captures(content)?.get(1)?.as_str().parse().ok()
}

fn rfc_prefix_width_drifted(current_prefix: &str, expected_prefix: &str) -> bool {
    current_prefix != expected_prefix
        && current_prefix.len() != expected_prefix.len()
        && parse_rfc_number(current_prefix) == parse_rfc_number(expected_prefix)
}

fn anchor_ulid_repair_reason(content: &str) -> &'static str {
    let anchor_line = content.lines().next().unwrap_or_default();
    if anchor_line.contains("ulid:") {
        "invalid_anchor_ulid"
    } else {
        "missing_anchor_ulid"
    }
}

fn repair_malformed_rfc_anchor(
    root: &Path,
    path: &Path,
    debt: &MalformedRfcRepairDebt,
) -> Result<()> {
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let text_id = debt
        .text_id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::new().to_string().to_lowercase());
    let anchor = format!("<!-- exo:{} ulid:{} -->", debt.identity_number, text_id);
    let repaired = if has_anchor(&original) {
        let (_, body) = split_anchor_and_body(&original)?;
        render_anchor_line_content(&anchor, body)
    } else {
        render_anchor_line_content(&anchor, &original)
    };
    utils::edit_cli_managed_file(path, move |_| Ok(repaired)).with_context(|| {
        format!(
            "Failed to repair malformed RFC anchor for {} at {}",
            debt.file_path,
            root.display()
        )
    })
}

fn expected_rfc_path(
    root: &Path,
    path: &Path,
    parsed: &DiskRfcRecord,
    canonical_number: Option<i64>,
    preferred_width: Option<usize>,
) -> Result<PathBuf> {
    let Some(parent) = path.parent() else {
        anyhow::bail!("RFC path had no parent directory: {}", path.display());
    };
    let rfc_number = canonical_number.unwrap_or(parsed.rfc_number);
    let prefix = if let Some(width) = preferred_width {
        let raw = rfc_number.to_string();
        format!("{rfc_number:0width$}", width = width.max(raw.len()))
    } else {
        let current_width = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(extract_rfc_number_from_filename)
            .map(|prefix| prefix.len());
        canonical_rfc_prefix(root, rfc_number, current_width)
    };
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let tail = if let Some((separator, current_slug)) = rfc_filename_tail(filename) {
        if is_unexpected_slug_drift(&current_slug) {
            let title_slug = slugify_title(&parsed.title);
            format!(
                "-{}",
                if title_slug.is_empty() {
                    "untitled".to_string()
                } else {
                    title_slug
                }
            )
        } else {
            format!("{separator}{current_slug}")
        }
    } else {
        let title_slug = slugify_title(&parsed.title);
        format!(
            "-{}",
            if title_slug.is_empty() {
                "untitled".to_string()
            } else {
                title_slug
            }
        )
    };
    Ok(parent.join(format!("{prefix}{tail}.md")))
}

fn canonical_rfc_prefix(root: &Path, number: i64, current_width: Option<usize>) -> String {
    let raw = number.to_string();
    let width = if number < 10_000 {
        infer_low_number_rfc_width(root, number, current_width)
            .or(current_width)
            .unwrap_or(DEFAULT_RFC_ID_WIDTH)
    } else {
        raw.len()
    };
    format!("{number:0width$}", width = width.max(raw.len()))
}

fn infer_low_number_rfc_width(
    root: &Path,
    target_number: i64,
    current_width: Option<usize>,
) -> Option<usize> {
    let rfc_root = root.join(RFCS_DIR);
    if !rfc_root.exists() {
        return None;
    }

    let mut candidates = Vec::new();
    for path in walk_rfc_markdown_files(&rfc_root) {
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(prefix) = extract_rfc_number_from_filename(filename) else {
            continue;
        };
        let Ok(number) = prefix.parse::<i64>() else {
            continue;
        };
        if number == target_number || !(0..10_000).contains(&number) {
            continue;
        }
        candidates.push((target_number.abs_diff(number), prefix.len()));
    }

    candidates
        .into_iter()
        .min_by(|(distance_a, width_a), (distance_b, width_b)| {
            distance_a
                .cmp(distance_b)
                .then_with(|| match current_width {
                    Some(current_width) if *width_a == current_width => std::cmp::Ordering::Less,
                    Some(current_width) if *width_b == current_width => std::cmp::Ordering::Greater,
                    _ => width_a.cmp(width_b),
                })
        })
        .map(|(_, width)| width)
}

fn filename_slug(filename: &str) -> Option<String> {
    rfc_filename_tail(filename).map(|(_, slug)| slug)
}

fn rfc_filename_tail(filename: &str) -> Option<(char, String)> {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let separator_index = stem.find(['-', '_'])?;
    let separator = stem[separator_index..].chars().next()?;
    Some((
        separator,
        stem[separator_index + separator.len_utf8()..].to_string(),
    ))
}

fn rfc_number_width_from_path(path: &str) -> Option<usize> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(extract_rfc_number_from_filename)
        .map(|prefix| prefix.len())
}

fn rfc_filename_number(path: &Path) -> Option<i64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(parse_rfc_number)
}

fn is_unexpected_slug_drift(current_slug: &str) -> bool {
    current_slug.is_empty() || current_slug == "untitled"
}

fn rfc_metadata_sync_needed(root: &Path, parsed: &DiskRfcRecord) -> Result<bool> {
    let Some(record) = load_rfc_record_by_text_id(root, &parsed.text_id)? else {
        return Ok(true);
    };
    Ok(record.rfc_number != parsed.rfc_number
        || record.title != parsed.title
        || record.stage != parsed.stage
        || record.status != parsed.status
        || record.slug != parsed.slug
        || record.file_path != parsed.file_path)
}

fn move_rfc_file(root: &Path, old_path: &Path, new_path: &Path) -> Result<()> {
    let old_rel = relative_workspace_path(root, old_path);
    let new_rel = relative_workspace_path(root, new_path);
    let git_mv = Command::new("git")
        .args(["mv", "-f"])
        .arg(&old_rel)
        .arg(&new_rel)
        .current_dir(root)
        .status();

    if git_mv.is_ok_and(|status| status.success()) {
        return Ok(());
    }

    std::fs::rename(old_path, new_path).with_context(|| {
        format!(
            "Failed to rename RFC from {} to {}",
            old_path.display(),
            new_path.display()
        )
    })
}

fn repair_anchor_identity(path: &Path, rfc_number: i64, text_id: &str) -> Result<()> {
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let (_, body) = split_anchor_and_body(&original)?;
    let repaired =
        render_anchor_line_content(&format!("<!-- exo:{rfc_number} ulid:{text_id} -->"), body);
    utils::edit_cli_managed_file(path, move |_| Ok(repaired))
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn repair_rfc_file_identity(
    path: &Path,
    rfc_number: i64,
    text_id: &str,
    display_id: &str,
    title: &str,
) -> Result<()> {
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let (_, body) = split_anchor_and_body(&original)?;
    let updated_body = update_rfc_identity_headings(body, display_id, title);
    let repaired = render_anchor_line_content(
        &format!("<!-- exo:{rfc_number} ulid:{text_id} -->"),
        updated_body.trim_start_matches('\n'),
    );
    utils::edit_cli_managed_file(path, move |_| Ok(repaired))
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn update_rfc_identity_headings(body: &str, id: &str, title: &str) -> String {
    let replacement = format!("# RFC {id}: {title}");
    let mut out = String::with_capacity(body.len() + replacement.len());
    let mut replaced = false;

    for line in body.split_inclusive('\n') {
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        if trimmed.starts_with("# ") {
            if !replaced {
                out.push_str(&replacement);
                out.push('\n');
                replaced = true;
                continue;
            }
            if extract_h1_title(trimmed).is_some_and(|line_title| line_title == title) {
                continue;
            }
        }
        out.push_str(line);
    }

    if replaced {
        out
    } else {
        format!("\n\n{replacement}\n{body}")
    }
}

fn edit_file(
    root: &Path,
    file_path: std::path::PathBuf,
    id: &str,
    title: Option<&str>,
    feature: Option<&str>,
    stage: Option<u8>,
    body: Option<&str>,
) -> Result<std::path::PathBuf> {
    let original = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;

    // Note: stage parameter is accepted but ignored — directory is the sole authority.
    let _ = stage;

    if title.is_some() || body.is_some() {
        let (anchor, existing_body) = split_anchor_and_body(&original)?;
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("????.md");
        let dir_stage = infer_stage_from_path(&file_path);
        let parsed = parse_rfc(filename, &original, dir_stage);
        let new_title = title.unwrap_or(&parsed.title);

        let rendered = body.map_or_else(
            || {
                let updated_body = update_first_h1_title(existing_body, id, new_title);
                render_anchor_line_content(anchor, updated_body.trim_start_matches('\n'))
            },
            |body_content| render_existing_anchor_rfc_content(anchor, id, new_title, body_content),
        );

        utils::edit_cli_managed_file(&file_path, move |_| Ok(rendered))
            .with_context(|| format!("Failed to write {}", file_path.display()))?;
    } else if feature.is_none() {
        open_rfc_in_editor(&file_path)?;
    }

    let clear_absent_relationships = body.is_some() || (title.is_none() && feature.is_none());
    sync_rfc_edit(root, &file_path, feature, clear_absent_relationships)?;

    Ok(file_path)
}

fn infer_stage_from_path(path: &Path) -> Option<u8> {
    let parent_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    parent_dir.and_then(|name| {
        if name.starts_with("stage-") {
            name.strip_prefix("stage-")?.parse::<u8>().ok()
        } else {
            None
        }
    })
}

fn update_first_h1_title(body: &str, id: &str, title: &str) -> String {
    let replacement = format!("# RFC {id}: {title}");

    if body.lines().any(|line| line.starts_with("# ")) {
        return replace_first_h1_line(body, &replacement);
    }

    // No H1; prepend.
    format!("\n\n{replacement}\n{body}")
}

fn replace_first_h1_line(body: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(body.len() + replacement.len());
    let mut replaced = false;

    for line in body.split_inclusive('\n') {
        if !replaced {
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            if trimmed.starts_with("# ") {
                out.push_str(replacement);
                out.push('\n');
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
    }

    if !replaced {
        // Body had no trailing newline; handle single-line input.
        let trimmed = body;
        if trimmed.starts_with("# ") {
            return format!("{replacement}\n");
        }
    }

    out
}

fn update_declared_stage_marker(content: &str, stage: u8) -> String {
    let mut saw_h1 = false;
    let mut in_fence = false;
    let mut in_preamble = true;
    let mut replaced = false;
    let mut out = String::with_capacity(content.len());

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !saw_h1 {
            saw_h1 = trimmed.starts_with("# ");
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if !in_fence && trimmed.starts_with("## ") {
            in_preamble = false;
        }
        if in_preamble && !in_fence && !replaced && declared_metadata_value(line, "Stage").declared
        {
            let label_end = line.find("Stage").map(|index| index + "Stage".len());
            if let Some(label_end) = label_end
                && let Some(relative_start) = line[label_end..].find(|ch: char| ch.is_ascii_digit())
            {
                let number_start = label_end + relative_start;
                let number_end = line[number_start..]
                    .find(|ch: char| !ch.is_ascii_digit())
                    .map_or(line.len(), |offset| number_start + offset);
                out.push_str(&line[..number_start]);
                out.push_str(&stage.to_string());
                out.push_str(&line[number_end..]);
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
    }

    out
}

fn format_rfc_metadata_marker(label: &str, value: &str) -> String {
    if value.is_empty() {
        format!("- **{label}**:")
    } else {
        format!("- **{label}**: {value}")
    }
}

fn metadata_marker_is_replaced(line: &str, label: &str) -> bool {
    let labels = if label == "Reason" {
        &["Reason", "Withdrawal reason", "Archived reason"][..]
    } else {
        std::slice::from_ref(&label)
    };
    labels
        .iter()
        .any(|candidate| declared_metadata_value(line, candidate).declared)
}

fn upsert_rfc_metadata_markers(content: &str, markers: &[(&str, String)]) -> String {
    if markers.is_empty() {
        return content.to_string();
    }

    let mut rendered = String::new();
    let mut inserted = false;
    let mut skip_original_spacing = false;
    let mut saw_h1 = false;
    let mut in_fence = false;
    let mut in_preamble = true;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !saw_h1 {
            saw_h1 = line.starts_with("# ");
            rendered.push_str(line);
            rendered.push('\n');
            if saw_h1 {
                rendered.push('\n');
                for (label, value) in markers {
                    rendered.push_str(&format_rfc_metadata_marker(label, value));
                    rendered.push('\n');
                }
                rendered.push('\n');
                inserted = true;
                skip_original_spacing = true;
            }
            continue;
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence && trimmed.starts_with("## ") {
            in_preamble = false;
        }

        if in_preamble
            && !in_fence
            && markers
                .iter()
                .any(|(label, _)| metadata_marker_is_replaced(line, label))
        {
            continue;
        }

        if skip_original_spacing && line.trim().is_empty() {
            continue;
        }
        skip_original_spacing = false;

        rendered.push_str(line);
        rendered.push('\n');
    }

    if !inserted {
        let mut prefixed = String::new();
        let (leading_anchor, remaining_content) = content
            .strip_prefix("<!-- exo:")
            .and_then(|suffix| suffix.split_once('\n'))
            .map_or((None, content), |(anchor_suffix, remaining)| {
                (Some(format!("<!-- exo:{anchor_suffix}")), remaining)
            });
        if let Some(anchor) = leading_anchor {
            prefixed.push_str(&anchor);
            prefixed.push_str("\n\n");
            if let Some(number) = extract_anchor_rfc_number(content) {
                prefixed.push_str(&format!("# RFC {number}: RFC {number}\n\n"));
            }
        }
        for (label, value) in markers {
            prefixed.push_str(&format_rfc_metadata_marker(label, value));
            prefixed.push('\n');
        }
        prefixed.push('\n');
        prefixed.push_str(remaining_content.trim_start_matches('\n'));
        return prefixed;
    }

    rendered
}

pub(crate) fn materialize_rfc_lifecycle_metadata_content(
    content: &str,
    status: &str,
    stage: u8,
    reason: Option<&str>,
) -> String {
    upsert_rfc_metadata_markers(
        content,
        &[
            ("Status", status.to_string()),
            ("Stage", stage.to_string()),
            ("Reason", reason.unwrap_or_default().to_string()),
        ],
    )
}

pub(crate) fn backfill_rfc_lifecycle_metadata_content(
    content: &str,
    status: &str,
    stage: u8,
    reason: Option<&str>,
) -> String {
    let metadata = rfc_metadata_preamble(content);
    let mut markers = Vec::new();

    let specific_reason_label = if status.eq_ignore_ascii_case("Withdrawn") {
        "Withdrawal reason"
    } else {
        "Archived reason"
    };
    let specific_reason = declared_metadata_value(&metadata, specific_reason_label);
    let generic_reason = declared_metadata_value(&metadata, "Reason");
    let available_reason = specific_reason
        .value
        .as_deref()
        .or(generic_reason.value.as_deref())
        .or(reason.filter(|value| !value.trim().is_empty()));
    if let Some(value) = available_reason {
        if specific_reason.declared && specific_reason.value.is_none() {
            markers.push((specific_reason_label, value.to_string()));
        }
        if generic_reason.declared && generic_reason.value.is_none() {
            markers.push(("Reason", value.to_string()));
        }
        if !specific_reason.declared && !generic_reason.declared {
            markers.push(("Reason", value.to_string()));
        }
    } else if !specific_reason.declared && !generic_reason.declared {
        markers.push(("Reason", String::new()));
    }
    if declared_metadata_value(&metadata, "Stage")
        .value
        .as_deref()
        .and_then(parse_declared_stage)
        .is_none()
    {
        markers.push(("Stage", stage.to_string()));
    }
    if declared_lifecycle_status(&metadata)
        != Some(if status.eq_ignore_ascii_case("Withdrawn") {
            "withdrawn"
        } else {
            "archived"
        })
    {
        markers.push(("Status", status.to_string()));
    }

    markers.sort_by_key(|(label, _)| match *label {
        "Status" => 0,
        "Stage" => 1,
        _ => 2,
    });
    upsert_rfc_metadata_markers(content, &markers)
}

pub(crate) fn retired_rfc_lifecycle_metadata_is_portable(content: &str, status: &str) -> bool {
    if !matches!(status, "withdrawn" | "archived") {
        return true;
    }

    let metadata = rfc_metadata_preamble(content);
    declared_lifecycle_status(&metadata) == Some(status)
        && declared_metadata_value(&metadata, "Stage")
            .value
            .as_deref()
            .and_then(parse_declared_stage)
            .is_some()
        && first_declared_value(
            declared_metadata_value(
                &metadata,
                if status == "withdrawn" {
                    "Withdrawal reason"
                } else {
                    "Archived reason"
                },
            ),
            Some(declared_metadata_value(&metadata, "Reason")),
        )
        .declared
}

pub(crate) fn retired_rfc_reason_from_document(content: &str, status: &str) -> Option<String> {
    let metadata = rfc_metadata_preamble(content);
    declared_metadata_value(
        &metadata,
        if status == "withdrawn" {
            "Withdrawal reason"
        } else {
            "Archived reason"
        },
    )
    .value
    .or_else(|| declared_metadata_value(&metadata, "Reason").value)
}

fn write_rfc_lifecycle_metadata(
    file_path: &Path,
    status: &str,
    stage: u8,
    reason: Option<&str>,
) -> Result<()> {
    utils::edit_cli_managed_file(file_path, move |content| {
        Ok(materialize_rfc_lifecycle_metadata_content(
            content, status, stage, reason,
        ))
    })
    .with_context(|| {
        format!(
            "Failed to write lifecycle metadata to {}",
            file_path.display()
        )
    })
}

/// Promotes an RFC to the next stage.
///
/// # Errors
///
/// Returns an error if:
/// - The RFC cannot be found.
/// - The file cannot be read or written.
/// - The directory structure is invalid.
pub fn promote(path: &Path, id: &str) -> Result<()> {
    let file_path = find_rfc_file(path, id)?;
    let workspace_root = workspace_root_from_rfc_root(path)?;
    ensure_no_rfc_repair_debt_matching(
        workspace_root,
        &file_path,
        is_blocking_rfc_promote_candidate,
    )?;
    let file_content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;
    let filename = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("Invalid RFC filename: {}", file_path.display()))?;
    let rfc = parse_rfc(filename, &file_content, infer_stage_from_path(&file_path));

    if rfc.stage >= 4 {
        anyhow::bail!("RFC {} is already at Stage 4 (Stable).", rfc.number);
    }

    let new_stage = rfc.stage + 1;
    let new_stage_dir = format!("stage-{new_stage}");

    extract_anchor_ulid(&file_content).with_context(|| {
        format!(
            "RFC {id} is missing an anchor ULID in {}",
            file_path.display()
        )
    })?;

    // 2. Move File using git mv to preserve history. The directory is authoritative,
    // but an explicit stage marker must agree with it.
    let new_dir = path.join(new_stage_dir);
    if !new_dir.exists() {
        std::fs::create_dir_all(&new_dir)?;
    }
    let new_path = new_dir.join(filename);

    // Then use git mv to move the file (preserves history, avoids verifier issues)
    if new_path != file_path {
        let status = std::process::Command::new("git")
            .args(["mv", "--force"])
            .arg(&file_path)
            .arg(&new_path)
            .current_dir(path)
            .status();

        match status {
            Ok(s) if s.success() => {}
            _ => {
                // Fallback to manual move if git mv fails (e.g., not in a git repo)
                std::fs::rename(&file_path, &new_path)?;
            }
        }
    }

    let promoted_content = update_declared_stage_marker(&file_content, new_stage);
    if promoted_content != file_content {
        utils::edit_cli_managed_file(&new_path, move |_| Ok(promoted_content))
            .with_context(|| format!("Failed to write {}", new_path.display()))?;
    }

    sync_rfc_edit(workspace_root, &new_path, None, false)?;

    Ok(())
}

/// Withdraws an RFC by moving it to the `withdrawn` folder.
///
/// This is used for RFCs that are obsolete, superseded, or no longer relevant.
/// Unlike `supersede`, this doesn't require specifying a replacement RFC.
///
/// # Errors
///
/// Returns an error if:
/// - The RFC cannot be found.
/// - The file cannot be moved.
pub fn withdraw(path: &Path, id: &str, reason: Option<&str>) -> Result<PathBuf> {
    let rfc_root = path;
    let file_path = find_rfc_file(rfc_root, id)?;
    let workspace_root = workspace_root_from_rfc_root(path)?;
    ensure_no_rfc_identity_repair_debt(workspace_root, &file_path)?;
    let original_content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;
    let source_retired_status = match parse_status(&file_path) {
        status @ ("withdrawn" | "archived") => Some(status),
        _ => None,
    };
    let original_stage = if source_retired_status.is_some() {
        retired_rfc_stage_for_mutation(workspace_root, &file_path, &original_content)?
    } else {
        parse_stage(&file_path)
    };
    let lifecycle_reason = reason.map(str::to_string).or_else(|| {
        source_retired_status
            .and_then(|status| retired_rfc_reason_from_document(&original_content, status))
    });

    let withdrawn_dir = rfc_root.join("withdrawn");
    if !withdrawn_dir.exists() {
        std::fs::create_dir_all(&withdrawn_dir)?;
    }

    let filename = file_path
        .file_name()
        .context("Invalid file path")?
        .to_string_lossy()
        .to_string();
    let new_path = withdrawn_dir.join(&filename);

    // Use git mv to preserve history
    if new_path != file_path {
        let status = std::process::Command::new("git")
            .args(["mv", "--force"])
            .arg(&file_path)
            .arg(&new_path)
            .current_dir(path)
            .status();

        match status {
            Ok(s) if s.success() => {}
            _ => {
                // Fallback to manual move if git mv fails
                std::fs::rename(&file_path, &new_path)?;
            }
        }
    }

    write_rfc_lifecycle_metadata(
        &new_path,
        "Withdrawn",
        original_stage,
        lifecycle_reason.as_deref(),
    )?;
    sync_rfc_withdrawal(
        workspace_root,
        &new_path,
        original_stage,
        lifecycle_reason.as_deref(),
    )?;

    Ok(new_path)
}

/// Archive an RFC (move to archive folder for shipped-then-superseded RFCs).
///
/// Unlike `withdraw`, archive is for RFCs that were implemented (Stage 3+)
/// but are now superseded by newer work. The distinction:
/// - `withdrawn/`: Never shipped — rejected, abandoned, or superseded before implementation
/// - `archive/`: Did ship — implemented but later replaced
///
/// # Errors
///
/// Returns an error if:
/// - The RFC cannot be found.
/// - The file cannot be moved.
pub fn archive(path: &Path, id: &str, reason: Option<&str>) -> Result<PathBuf> {
    let rfc_root = path;
    let file_path = find_rfc_file(rfc_root, id)?;
    let workspace_root = workspace_root_from_rfc_root(path)?;
    ensure_no_rfc_identity_repair_debt(workspace_root, &file_path)?;
    let original_content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;
    let source_retired_status = match parse_status(&file_path) {
        status @ ("withdrawn" | "archived") => Some(status),
        _ => None,
    };
    let original_stage = if source_retired_status.is_some() {
        retired_rfc_stage_for_mutation(workspace_root, &file_path, &original_content)?
    } else {
        parse_stage(&file_path)
    };
    let lifecycle_reason = reason.map(str::to_string).or_else(|| {
        source_retired_status
            .and_then(|status| retired_rfc_reason_from_document(&original_content, status))
    });

    let archive_dir = rfc_root.join("archive");
    if !archive_dir.exists() {
        std::fs::create_dir_all(&archive_dir)?;
    }

    let filename = file_path
        .file_name()
        .context("Invalid file path")?
        .to_string_lossy()
        .to_string();
    let new_path = archive_dir.join(&filename);

    // Use git mv to preserve history
    if new_path != file_path {
        let status = std::process::Command::new("git")
            .args(["mv", "--force"])
            .arg(&file_path)
            .arg(&new_path)
            .current_dir(path)
            .status();

        match status {
            Ok(s) if s.success() => {}
            _ => {
                // Fallback to manual move if git mv fails
                std::fs::rename(&file_path, &new_path)?;
            }
        }
    }

    write_rfc_lifecycle_metadata(
        &new_path,
        "Archived",
        original_stage,
        lifecycle_reason.as_deref(),
    )?;
    sync_rfc_archive(
        workspace_root,
        &new_path,
        original_stage,
        lifecycle_reason.as_deref(),
    )?;

    Ok(new_path)
}

/// Marks an RFC as superseded by another RFC.
///
/// This is intentionally deterministic and minimally invasive:
/// - Ensures YAML frontmatter contains `superseded_by: "<by>"`.
/// - Ensures the body contains (or updates) a single line:
///   `- **Superseded by**: RFC <by>`
///
/// # Errors
///
/// Returns an error if:
/// - The RFC cannot be uniquely located.
/// - The file cannot be read or written.
pub fn supersede(root: &Path, id: &str, by: &str) -> Result<RfcSupersedeOutcome> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        anyhow::bail!("RFC root not found at {}", rfc_root.display());
    }

    let superseding_path = find_optional_rfc_file(&rfc_root, by)?;
    let old_file_path = find_rfc_file(&rfc_root, id)?;
    ensure_no_rfc_identity_repair_debt(root, &old_file_path)?;
    sync_rfc_superseded_by(root, &old_file_path, by)?;

    if let Some(new_file_path) = &superseding_path {
        ensure_no_rfc_identity_repair_debt(root, &new_file_path)?;
        sync_rfc_supersedes(root, &new_file_path, id)?;
    }

    Ok(RfcSupersedeOutcome {
        superseded_path: old_file_path,
        superseding_path,
    })
}

/// Marks an RFC file (by explicit path) as superseded by another RFC.
///
/// The `path` may be absolute or workspace-relative.
pub fn supersede_file(root: &Path, path: &str, by: &str) -> Result<RfcSupersedeFileOutcome> {
    validate_rfc_id(by)?;
    let file_path = {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(path)
        }
    };

    if !file_path.exists() {
        anyhow::bail!("RFC file not found: {}", file_path.display());
    }
    ensure_no_rfc_identity_repair_debt(root, &file_path)?;

    sync_rfc_superseded_by(root, &file_path, by)?;

    Ok(RfcSupersedeFileOutcome {
        superseded_path: file_path,
    })
}

fn find_rfc_file(rfc_root: &Path, id: &str) -> Result<std::path::PathBuf> {
    find_optional_rfc_file(rfc_root, id)?
        .ok_or_else(|| anyhow::anyhow!("RFC {id} not found under {}", rfc_root.display()))
}

fn find_optional_rfc_file(rfc_root: &Path, id: &str) -> Result<Option<std::path::PathBuf>> {
    let mut matches = Vec::new();
    let requested_number = validate_rfc_id(id)?;

    for entry in WalkDir::new(rfc_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        if !is_rfc_document_path(rfc_root, path) {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if SKIP_RFC_FILES.contains(&filename) {
            continue;
        }

        if parse_rfc_number(filename).is_some_and(|number| number == requested_number) {
            matches.push(path.to_path_buf());
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.remove(0))),
        _ => {
            let rendered = matches
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n  - ");
            anyhow::bail!("RFC {id} is ambiguous; multiple files matched:\n  - {rendered}")
        }
    }
}

fn validate_rfc_id(id: &str) -> Result<i64> {
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("Invalid RFC ID: {id}");
    }
    parse_rfc_number(id).with_context(|| format!("Invalid RFC ID: {id}"))
}

fn ensure_no_rfc_identity_repair_debt(root: &Path, path: &Path) -> Result<()> {
    ensure_no_rfc_repair_debt_matching(root, path, is_blocking_rfc_repair_candidate)
}

fn ensure_no_blocking_rfc_rename_debt(root: &Path, path: &Path) -> Result<()> {
    ensure_no_rfc_repair_debt_matching(root, path, is_blocking_rfc_rename_candidate)
}

fn ensure_no_rfc_repair_debt_matching(
    root: &Path,
    path: &Path,
    is_blocking: impl Fn(&RfcRepairCandidate) -> bool,
) -> Result<()> {
    let candidate = match parse_disk_rfc(root, path) {
        Ok(parsed) => {
            rfc_repair_candidate_for_path(root, path, parsed, None, RfcRepairCandidateMode::Manual)?
        }
        Err(error) => match malformed_rfc_repair_debt_for_path(root, path, None)? {
            Some(debt) => Some(debt.candidate),
            None => return Err(error),
        },
    };

    let Some(candidate) = candidate else {
        return Ok(());
    };

    if !is_blocking(&candidate) {
        return Ok(());
    }

    anyhow::bail!(
        "RFC {} has identity repair debt at {}. Run: exo rfc repair {}",
        candidate.id,
        candidate.current_path,
        candidate.id
    )
}

pub(crate) fn is_blocking_rfc_repair_candidate(candidate: &RfcRepairCandidate) -> bool {
    if !candidate
        .reasons
        .iter()
        .any(|reason| reason == "filename_slug_drift")
    {
        return true;
    }
    candidate
        .reasons
        .iter()
        .any(|reason| reason != "filename_slug_drift" && reason != "metadata_path_drift")
}

pub(crate) fn is_blocking_rfc_promote_candidate(candidate: &RfcRepairCandidate) -> bool {
    is_blocking_rfc_repair_candidate(candidate)
        && !(candidate.current_path == candidate.expected_path
            && !candidate.reasons.is_empty()
            && candidate
                .reasons
                .iter()
                .all(|reason| reason == "metadata_relink" || reason == "metadata_path_drift"))
}

fn is_blocking_rfc_rename_candidate(candidate: &RfcRepairCandidate) -> bool {
    !candidate
        .reasons
        .iter()
        .all(|reason| reason == "filename_slug_drift" || reason == "metadata_path_drift")
}

fn clean_title(title: &str) -> String {
    title.trim().trim_matches('*').trim().to_string()
}

fn walk_rfc_markdown_files(rfc_root: &Path) -> Vec<PathBuf> {
    WalkDir::new(rfc_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.is_file())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter(|path| is_rfc_document_path(rfc_root, path))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_none_or(|name| !SKIP_RFC_FILES.contains(&name))
        })
        .collect()
}

pub(crate) fn is_rfc_document_path(rfc_root: &Path, path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "md") {
        return false;
    }
    let Ok(relative) = path.strip_prefix(rfc_root) else {
        return false;
    };
    let mut components = relative.components();
    let Some(first) = components.next().and_then(|part| part.as_os_str().to_str()) else {
        return false;
    };

    let Some(_filename) = components.next() else {
        return is_legacy_flat_rfc_filename(first);
    };
    if !is_managed_rfc_collection(first) {
        return false;
    }
    components.next().is_none()
}

fn is_managed_rfc_collection(collection: &str) -> bool {
    if matches!(collection, "archive" | "withdrawn") {
        return true;
    }
    collection
        .strip_prefix("stage-")
        .and_then(|stage| stage.parse::<u8>().ok())
        .is_some_and(|stage| stage <= 4)
}

fn is_legacy_flat_rfc_filename(filename: &str) -> bool {
    !SKIP_RFC_FILES.contains(&filename) && parse_rfc_number(filename).is_some()
}

fn parse_disk_rfc(root: &Path, path: &Path) -> Result<DiskRfcRecord> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let relative_path = relative_workspace_path(root, path);
    Ok(parse_rfc_document(&relative_path, &content, None)?.disk)
}

pub(crate) fn workspace_rfc_record(root: &Path, id: &str) -> Result<Option<RfcRecord>> {
    let Some(file_path) = find_optional_rfc_file(&root.join(RFCS_DIR), id)? else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;
    let relative_path = relative_workspace_path(root, &file_path);
    let parsed = parse_rfc_document(&relative_path, &content, None)?;
    Ok(Some(canonical_rfc_record(&parsed, None)))
}

fn parse_rfc_document(
    relative_path: &str,
    content: &str,
    existing: Option<&RfcRecord>,
) -> Result<ParsedRfcDocument> {
    let path = Path::new(relative_path);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("Invalid RFC filename: {relative_path}"))?;

    if !has_anchor(&content) {
        anyhow::bail!("RFC file missing anchor comment: {relative_path}");
    }

    let filename_number = parse_rfc_number(filename)
        .with_context(|| format!("Could not parse RFC number from {filename}"))?;
    let text_id = extract_anchor_ulid(&content)
        .with_context(|| format!("RFC file has invalid anchor ULID: {relative_path}"))?;
    let rfc_number = extract_anchor_rfc_number(&content)
        .with_context(|| format!("RFC file has invalid anchor number: {relative_path}"))?;
    let title = extract_h1_title(&content).unwrap_or_else(|| format!("RFC {rfc_number}"));
    let metadata = rfc_metadata_preamble(content);
    let relationships = extract_rfc_relationships(content);
    let status = parse_status(path).to_string();
    let declared_status = declared_lifecycle_status(&metadata);
    let lifecycle_status_conflicts = declared_status.is_some_and(|declared| declared != status);
    let lifecycle_status_declared = declared_status.is_some_and(|declared| declared == status);
    let stage_marker = declared_metadata_value_with_yaml(&metadata, content, "Stage", "stage");
    let declared_stage = stage_marker.value.as_deref().and_then(parse_declared_stage);
    let invalid_stage_marker = stage_marker.declared && declared_stage.is_none();
    let legacy_stage = (status != "active" && !stage_marker.declared)
        .then(|| legacy_stage_from_status(&metadata))
        .flatten();
    let resolved_stage = declared_stage.or(legacy_stage);
    let path_stage = parse_stage(path);
    let stage_marker_conflicts = status == "active"
        && declared_stage.is_some_and(|declared_stage| declared_stage != path_stage);
    let stage = if status == "active" {
        path_stage
    } else if let Some(stage) = resolved_stage {
        stage
    } else if let Some(existing) = existing {
        existing.stage
    } else {
        0
    };
    let stage_source = if status == "active" {
        "path"
    } else if declared_stage.is_some() {
        "marker"
    } else {
        "legacy"
    };

    let feature = declared_metadata_value_with_yaml(&metadata, content, "Feature", "feature");
    let reason = declared_metadata_value(&metadata, "Reason");
    let withdrawal_reason = first_declared_value(
        declared_metadata_value(&metadata, "Withdrawal reason"),
        (status == "withdrawn").then_some(reason.clone()),
    );
    let archived_reason = first_declared_value(
        declared_metadata_value(&metadata, "Archived reason"),
        (status == "archived").then_some(reason),
    );
    let mut consolidated_into = declared_metadata_value(&metadata, "Consolidated into");
    if consolidated_into.declared {
        consolidated_into.value = consolidated_into
            .value
            .as_deref()
            .map(clean_relationship_value)
            .filter(|value| !value.is_empty());
    }

    Ok(ParsedRfcDocument {
        disk: DiskRfcRecord {
            text_id,
            rfc_number,
            title,
            stage,
            status,
            lifecycle_status_declared,
            slug: parse_slug(filename),
            file_path: relative_path.to_string(),
            superseded_by: relationships.superseded_by,
            supersedes: relationships.supersedes,
            superseded_by_declared: relationships.superseded_by_declared,
            supersedes_declared: relationships.supersedes_declared,
        },
        filename_number,
        stage_source: stage_source.to_string(),
        canonical_metadata_conflict: lifecycle_status_conflicts
            || invalid_stage_marker
            || stage_marker_conflicts,
        feature,
        withdrawal_reason,
        archived_reason,
        consolidated_into,
    })
}

fn rfc_metadata_preamble(content: &str) -> String {
    let mut saw_h1 = false;
    let mut in_fence = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        if !saw_h1 {
            if line.starts_with("# ") {
                saw_h1 = true;
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("## ") {
            break;
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn parse_declared_stage(value: &str) -> Option<u8> {
    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .find_map(|part| part.parse::<u8>().ok().filter(|stage| *stage <= 4))
}

fn legacy_stage_from_status(metadata: &str) -> Option<u8> {
    let value = declared_metadata_value(metadata, "Status").value?;
    let normalized = value.to_ascii_lowercase();
    if let Some(stage) = normalized.match_indices("stage").find_map(|(index, _)| {
        normalized[index + "stage".len()..]
            .trim_start_matches(|ch: char| {
                ch.is_ascii_whitespace() || matches!(ch, ':' | '=' | '-')
            })
            .split(|ch: char| !ch.is_ascii_digit())
            .next()
            .filter(|part| !part.is_empty())
            .and_then(|part| part.parse::<u8>().ok())
            .filter(|stage| *stage <= 4)
    }) {
        return Some(stage);
    }

    if has_positive_lifecycle_keyword(&normalized, "stable") {
        Some(4)
    } else if has_positive_lifecycle_keyword(&normalized, "candidate")
        || has_positive_lifecycle_keyword(&normalized, "implemented")
    {
        Some(3)
    } else if has_positive_lifecycle_keyword(&normalized, "draft") {
        Some(2)
    } else if has_positive_lifecycle_keyword(&normalized, "proposal")
        || has_positive_lifecycle_keyword(&normalized, "accepted")
    {
        Some(1)
    } else if has_positive_lifecycle_keyword(&normalized, "idea")
        || has_positive_lifecycle_keyword(&normalized, "strawman")
    {
        Some(0)
    } else {
        None
    }
}

fn has_positive_lifecycle_keyword(value: &str, keyword: &str) -> bool {
    let words = value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    words.iter().enumerate().any(|(index, word)| {
        *word == keyword
            && !words[index.saturating_sub(2)..index]
                .iter()
                .any(|word| matches!(*word, "not" | "never" | "no"))
    })
}

pub(crate) fn retired_rfc_stage_from_document(
    content: &str,
    historical_stage: Option<u8>,
    fallback: u8,
) -> u8 {
    let metadata = rfc_metadata_preamble(content);
    let stage_marker = declared_metadata_value(&metadata, "Stage");
    let declared_stage = stage_marker.value.as_deref().and_then(parse_declared_stage);
    if stage_marker.declared {
        return declared_stage.or(historical_stage).unwrap_or(fallback);
    }

    historical_stage
        .or_else(|| {
            declared_metadata_value_with_yaml(&metadata, content, "Stage", "stage")
                .value
                .as_deref()
                .and_then(parse_declared_stage)
        })
        .or_else(|| legacy_stage_from_status(&metadata))
        .unwrap_or(fallback)
}

fn retired_rfc_stage_for_mutation(root: &Path, file_path: &Path, content: &str) -> Result<u8> {
    let stored_stage = extract_anchor_ulid(content)
        .map(|text_id| load_rfc_record_by_text_id(root, &text_id))
        .transpose()?
        .flatten()
        .map(|record| record.stage);
    Ok(retired_rfc_stage_from_document(
        content,
        stored_stage,
        parse_stage(file_path),
    ))
}

fn declared_metadata_value(content: &str, label: &str) -> DeclaredRfcValue {
    for line in content.lines() {
        let line = normalize_relationship_line(line);
        let Some(value) = line
            .strip_prefix(label)
            .or_else(|| line.strip_prefix(&format!("**{label}**")))
        else {
            continue;
        };
        let value = value.trim_start();
        let Some(value) = value
            .strip_prefix(':')
            .or_else(|| value.strip_prefix("**:"))
            .or_else(|| value.strip_prefix('|'))
        else {
            continue;
        };
        let value = value.trim().trim_matches('|').trim();
        return DeclaredRfcValue {
            value: (!value.is_empty()).then(|| value.to_string()),
            declared: true,
        };
    }
    DeclaredRfcValue::absent()
}

fn declared_metadata_value_with_yaml(
    metadata: &str,
    content: &str,
    label: &str,
    yaml_key: &str,
) -> DeclaredRfcValue {
    let markdown = declared_metadata_value(metadata, label);
    if markdown.declared {
        return markdown;
    }
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return DeclaredRfcValue::absent();
    }
    for line in lines.take_while(|line| line.trim() != "---") {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() == yaml_key {
            let value = value.trim().trim_matches(['"', '\'']);
            return DeclaredRfcValue {
                value: (!value.is_empty()).then(|| value.to_string()),
                declared: true,
            };
        }
    }
    DeclaredRfcValue::absent()
}

fn first_declared_value(
    primary: DeclaredRfcValue,
    fallback: Option<DeclaredRfcValue>,
) -> DeclaredRfcValue {
    if primary.declared {
        primary
    } else {
        fallback.unwrap_or_else(DeclaredRfcValue::absent)
    }
}

fn render_anchor_rfc_content(rfc_number: i64, text_id: &str, title: &str, body: &str) -> String {
    let body = body.trim_start_matches('\n');
    format!("<!-- exo:{rfc_number} ulid:{text_id} -->\n\n# RFC {rfc_number}: {title}\n\n{body}\n")
}

fn render_existing_anchor_rfc_content(anchor: &str, id: &str, title: &str, body: &str) -> String {
    let body = body.trim_start_matches('\n').trim_end_matches('\n');
    format!("{anchor}\n\n# RFC {id}: {title}\n\n{body}\n")
}

fn render_anchor_line_content(anchor: &str, body: &str) -> String {
    let body = body.trim_start_matches('\n').trim_end_matches('\n');
    format!("{anchor}\n\n{body}\n")
}

fn split_anchor_and_body(content: &str) -> Result<(&str, &str)> {
    if !has_anchor(content) {
        anyhow::bail!("RFC file is missing the required anchor comment")
    }

    let newline_index = content.find('\n').unwrap_or(content.len());
    let anchor = &content[..newline_index];
    let body = content[newline_index..].trim_start_matches('\n');
    Ok((anchor, body))
}

fn open_rfc_in_editor(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .context("Set $VISUAL or $EDITOR to use interactive RFC editing")?;

    let status = std::process::Command::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to launch editor '{editor}' for {}", path.display()))?;

    if !status.success() {
        anyhow::bail!("Editor exited unsuccessfully for {}", path.display());
    }

    Ok(())
}

fn resolve_rfc_storage(root: &Path) -> Result<Option<(Option<Project>, PathBuf)>> {
    let mut project = Project::resolve(root).ok();
    let mut db_path = crate::context::db_path(root, project.as_ref());
    if !db_path.exists() {
        if root.join("exosuit.toml").exists() {
            let _ = crate::context::AgentContext::load(root.to_path_buf())?;
            project = Project::resolve(root).ok();
            db_path = crate::context::db_path(root, project.as_ref());
        } else {
            return Ok(None);
        }
    }

    if !db_path.exists() {
        return Ok(None);
    }

    Ok(Some((project, db_path)))
}

fn load_rfc_record(root: &Path, rfc_number: i64) -> Result<Option<RfcRecord>> {
    let Some((project, db_path)) = resolve_rfc_storage(root)? else {
        return Ok(None);
    };
    if matches!(
        canonical_reconcile_source(root)?,
        CanonicalReconcileSource::WorkspaceFallback
    ) {
        return SqliteLoader::open(&db_path)?.load_rfc_by_number(rfc_number);
    }
    Ok(load_effective_rfc_by_number(root, project.as_ref(), rfc_number)?.map(|rfc| rfc.record))
}

fn load_effective_rfc_records_if_available(root: &Path) -> Result<Vec<RfcRecord>> {
    let Some((project, db_path)) = resolve_rfc_storage(root)? else {
        return Ok(Vec::new());
    };
    if matches!(
        canonical_reconcile_source(root)?,
        CanonicalReconcileSource::WorkspaceFallback
    ) {
        return SqliteLoader::open(&db_path)?.load_rfcs();
    }
    Ok(load_effective_rfcs(root, project.as_ref())?
        .into_iter()
        .map(|effective| effective.record)
        .collect())
}

fn load_rfc_record_by_text_id(root: &Path, text_id: &str) -> Result<Option<RfcRecord>> {
    let Some((project, db_path)) = resolve_rfc_storage(root)? else {
        return Ok(None);
    };
    if matches!(
        canonical_reconcile_source(root)?,
        CanonicalReconcileSource::WorkspaceFallback
    ) {
        return Ok(SqliteLoader::open(&db_path)?
            .load_rfcs()?
            .into_iter()
            .find(|record| record.text_id == text_id));
    }
    Ok(load_effective_rfcs(root, project.as_ref())?
        .into_iter()
        .map(|effective| effective.record)
        .find(|record| record.text_id == text_id))
}

fn load_rfc_record_by_text_or_number(
    root: &Path,
    parsed: &DiskRfcRecord,
) -> Result<Option<RfcRecord>> {
    if let Some(record) = load_rfc_record_by_text_id(root, &parsed.text_id)? {
        return Ok(Some(record));
    }
    load_rfc_record(root, parsed.rfc_number)
}

fn persist_rfc_record(root: &Path, record: &RfcRecord) -> Result<()> {
    let Some((project, db_path)) = resolve_rfc_storage(root)? else {
        return Ok(());
    };

    with_reconcile_lock(root, project.as_ref(), || {
        let source = canonical_reconcile_source(root)?;
        if matches!(source, CanonicalReconcileSource::WorkspaceFallback) {
            let writer = SqliteWriter::open(&db_path)?;
            writer.upsert_rfc(
                &record.text_id,
                record.rfc_number,
                &record.title,
                record.stage,
                &record.status,
                record.feature.as_deref(),
                &record.slug,
                &record.file_path,
                record.superseded_by.as_deref(),
                record.supersedes.as_deref(),
                record.withdrawal_reason.as_deref(),
                record.archived_reason.as_deref(),
                record.consolidated_into.as_deref(),
            )?;
            refresh_workspace_rfc_snapshot(root, project.as_ref(), &source)?;
            return Ok(());
        }

        refresh_workspace_rfc_snapshot(root, project.as_ref(), &source)?;
        let loader = SqliteLoader::open(&db_path)?;
        let writer = SqliteWriter::open(&db_path)?;
        let workspace_root = slash_path_string(&normalize_key_path(root));
        let snapshot = loader
            .load_rfc_workspace_snapshot(&workspace_root)?
            .with_context(|| format!("RFC workspace snapshot missing for {workspace_root}"))?;
        let diagnostics = loader.load_rfc_workspace_diagnostics(&workspace_root)?;
        let canonical = loader
            .load_rfcs()?
            .into_iter()
            .find(|candidate| candidate.text_id == record.text_id);
        let mut observations = loader.load_rfc_workspace_observations(&workspace_root)?;
        let observation = observations
            .iter_mut()
            .find(|candidate| candidate.text_id == record.text_id)
            .with_context(|| {
                format!(
                    "RFC {} ({}) is not a valid document in the current workspace snapshot",
                    record.rfc_number, record.text_id
                )
            })?;

        observation.rfc_number = record.rfc_number;
        observation.title.clone_from(&record.title);
        observation.stage = record.stage;
        observation.status.clone_from(&record.status);
        observation.slug.clone_from(&record.slug);
        observation.file_path.clone_from(&record.file_path);
        apply_workspace_optional_override(
            &mut observation.feature,
            &mut observation.feature_declared,
            &record.feature,
            canonical
                .as_ref()
                .and_then(|record| record.feature.as_ref()),
        );
        apply_workspace_optional_override(
            &mut observation.superseded_by,
            &mut observation.superseded_by_declared,
            &record.superseded_by,
            canonical
                .as_ref()
                .and_then(|record| record.superseded_by.as_ref()),
        );
        apply_workspace_optional_override(
            &mut observation.supersedes,
            &mut observation.supersedes_declared,
            &record.supersedes,
            canonical
                .as_ref()
                .and_then(|record| record.supersedes.as_ref()),
        );
        apply_workspace_optional_override(
            &mut observation.withdrawal_reason,
            &mut observation.withdrawal_reason_declared,
            &record.withdrawal_reason,
            canonical
                .as_ref()
                .and_then(|record| record.withdrawal_reason.as_ref()),
        );
        apply_workspace_optional_override(
            &mut observation.archived_reason,
            &mut observation.archived_reason_declared,
            &record.archived_reason,
            canonical
                .as_ref()
                .and_then(|record| record.archived_reason.as_ref()),
        );
        apply_workspace_optional_override(
            &mut observation.consolidated_into,
            &mut observation.consolidated_into_declared,
            &record.consolidated_into,
            canonical
                .as_ref()
                .and_then(|record| record.consolidated_into.as_ref()),
        );

        writer.replace_rfc_workspace_snapshot(&snapshot, &observations, &diagnostics)
    })
}

fn apply_workspace_optional_override(
    observed: &mut Option<String>,
    declared: &mut bool,
    desired: &Option<String>,
    canonical: Option<&String>,
) {
    let differs_from_canonical = desired.as_ref() != canonical;
    if *declared || differs_from_canonical {
        observed.clone_from(desired);
        *declared = true;
    }
}

fn relative_workspace_path(root: &Path, path: &Path) -> String {
    slash_path_string(path.strip_prefix(root).unwrap_or(path))
}

fn slash_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn workspace_root_from_rfc_root(rfc_root: &Path) -> Result<&Path> {
    rfc_root.parent().and_then(Path::parent).with_context(|| {
        format!(
            "RFC root {} had no workspace parent directory",
            rfc_root.display()
        )
    })
}

const fn default_rfc_record(
    text_id: String,
    rfc_number: i64,
    title: String,
    stage: u8,
    status: String,
    slug: String,
    file_path: String,
) -> RfcRecord {
    RfcRecord {
        text_id,
        rfc_number,
        title,
        stage,
        status,
        feature: None,
        slug,
        file_path,
        superseded_by: None,
        supersedes: None,
        withdrawal_reason: None,
        archived_reason: None,
        consolidated_into: None,
    }
}

fn sync_rfc_edit(
    root: &Path,
    file_path: &Path,
    feature_override: Option<&str>,
    clear_absent_relationships: bool,
) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_or_number(root, &parsed)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            parsed.stage,
            parsed.status.clone(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });

    record.text_id = parsed.text_id;
    record.title = parsed.title;
    record.stage = parsed.stage;
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    if parsed.superseded_by_declared || clear_absent_relationships {
        record.superseded_by = parsed.superseded_by;
    }
    if parsed.supersedes_declared || clear_absent_relationships {
        record.supersedes = parsed.supersedes;
    }
    if let Some(feature) = feature_override {
        record.feature = Some(feature.to_string());
    }

    persist_rfc_record(root, &record)
}

fn sync_parsed_relationships(
    record: &mut RfcRecord,
    parsed: &DiskRfcRecord,
    clear_absent_relationships: bool,
) {
    if parsed.superseded_by_declared || clear_absent_relationships {
        record.superseded_by = parsed.superseded_by.clone();
    }
    if parsed.supersedes_declared || clear_absent_relationships {
        record.supersedes = parsed.supersedes.clone();
    }
}

fn sync_rfc_rename(root: &Path, file_path: &Path) -> Result<()> {
    sync_rfc_identity(root, file_path, false)
}

fn sync_rfc_renumber(root: &Path, file_path: &Path) -> Result<()> {
    sync_rfc_identity(root, file_path, true)
}

fn sync_rfc_identity(root: &Path, file_path: &Path, update_number: bool) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_id(root, &parsed.text_id)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            parsed.stage,
            parsed.status.clone(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });

    sync_parsed_relationships(&mut record, &parsed, false);
    record.text_id = parsed.text_id;
    if update_number {
        record.rfc_number = parsed.rfc_number;
    }
    record.title = parsed.title;
    record.stage = parsed.stage;
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;

    persist_rfc_record(root, &record)
}

fn sync_rfc_withdrawal(
    root: &Path,
    file_path: &Path,
    stage: u8,
    reason: Option<&str>,
) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_or_number(root, &parsed)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            stage.max(parsed.stage),
            "withdrawn".to_string(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });

    sync_parsed_relationships(&mut record, &parsed, false);
    record.text_id = parsed.text_id;
    record.title = parsed.title;
    record.stage = parsed.stage.max(stage);
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    record.withdrawal_reason = reason.map(std::string::ToString::to_string);
    record.archived_reason = None;

    persist_rfc_record(root, &record)
}

fn sync_rfc_archive(root: &Path, file_path: &Path, stage: u8, reason: Option<&str>) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_or_number(root, &parsed)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            stage.max(parsed.stage),
            "archived".to_string(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });

    sync_parsed_relationships(&mut record, &parsed, false);
    record.text_id = parsed.text_id;
    record.title = parsed.title;
    record.stage = parsed.stage.max(stage);
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    record.withdrawal_reason = None;
    record.archived_reason = reason.map(std::string::ToString::to_string);

    persist_rfc_record(root, &record)
}

fn sync_rfc_superseded_by(root: &Path, file_path: &Path, by: &str) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_or_number(root, &parsed)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            parsed.stage,
            parsed.status.clone(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });
    sync_parsed_relationships(&mut record, &parsed, false);
    write_rfc_relationship_marker(file_path, "Superseded by", by)?;

    record.text_id = parsed.text_id;
    record.title = parsed.title;
    record.stage = parsed.stage;
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    record.superseded_by = Some(by.to_string());

    persist_rfc_record(root, &record)
}

fn sync_rfc_supersedes(root: &Path, file_path: &Path, id: &str) -> Result<()> {
    let parsed = parse_disk_rfc(root, file_path)?;
    let mut record = load_rfc_record_by_text_or_number(root, &parsed)?.unwrap_or_else(|| {
        default_rfc_record(
            parsed.text_id.clone(),
            parsed.rfc_number,
            parsed.title.clone(),
            parsed.stage,
            parsed.status.clone(),
            parsed.slug.clone(),
            parsed.file_path.clone(),
        )
    });
    sync_parsed_relationships(&mut record, &parsed, false);
    let existing_supersedes = if parsed.supersedes_declared {
        None
    } else {
        record.supersedes.as_deref()
    };
    let supersedes =
        merge_relationship_refs([parsed.supersedes.as_deref(), existing_supersedes, Some(id)]);
    write_rfc_relationship_marker(file_path, "Supersedes", &supersedes)?;

    record.text_id = parsed.text_id;
    record.title = parsed.title;
    record.stage = parsed.stage;
    record.status = parsed.status;
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    record.supersedes = Some(supersedes);

    persist_rfc_record(root, &record)
}

fn write_rfc_relationship_marker(file_path: &Path, label: &str, value: &str) -> Result<()> {
    utils::edit_cli_managed_file(file_path, move |content| {
        Ok(upsert_rfc_relationship_marker(content, label, value))
    })
    .with_context(|| {
        format!(
            "Failed to write relationship metadata to {}",
            file_path.display()
        )
    })
}

enum RelationshipSourceReplacement {
    CompactMarker,
    PreserveLine(String),
}

fn upsert_rfc_relationship_marker(content: &str, label: &str, value: &str) -> String {
    let marker = format_relationship_marker(label, value);
    let has_source_line = content
        .lines()
        .any(|line| relationship_source_replacement(line, label, value).is_some());
    let mut rendered = String::new();
    let mut inserted = false;
    for line in content.lines() {
        if let Some(replacement) = relationship_source_replacement(line, label, value) {
            match replacement {
                RelationshipSourceReplacement::CompactMarker => {
                    if !inserted {
                        rendered.push_str(&marker);
                        rendered.push('\n');
                        inserted = true;
                    }
                }
                RelationshipSourceReplacement::PreserveLine(line) => {
                    rendered.push_str(&line);
                    rendered.push('\n');
                    inserted = true;
                }
            }
            continue;
        }

        rendered.push_str(line);
        rendered.push('\n');
        if !has_source_line && !inserted && line.starts_with("# RFC ") {
            rendered.push('\n');
            rendered.push_str(&marker);
            rendered.push_str("\n\n");
            inserted = true;
        }
    }

    if !inserted {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push('\n');
        rendered.push_str(&marker);
        rendered.push('\n');
    }

    rendered
}

fn relationship_source_replacement(
    line: &str,
    label: &str,
    value: &str,
) -> Option<RelationshipSourceReplacement> {
    if relationship_table_row_cleaned_value(line, label).is_some() {
        return Some(RelationshipSourceReplacement::PreserveLine(
            render_relationship_table_row(line, label, value),
        ));
    }

    if label == "Superseded by"
        && (superseded_by_status_or_reason_value(line).is_some()
            || superseded_by_sentence_value(line).is_some())
    {
        return Some(RelationshipSourceReplacement::PreserveLine(
            replace_rfc_refs_in_line(line, value),
        ));
    }

    relationship_line_cleaned_value(line, label)
        .is_some()
        .then_some(RelationshipSourceReplacement::CompactMarker)
}

fn relationship_table_row_cleaned_value(line: &str, label: &str) -> Option<Option<String>> {
    line.trim()
        .starts_with('|')
        .then(|| relationship_line_cleaned_value(line, label))
        .flatten()
}

fn render_relationship_table_row(line: &str, label: &str, value: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    format!(
        "{indent}| **{label}** | {} |",
        format_rfc_relationship_refs(value)
    )
}

fn replace_rfc_refs_in_line(line: &str, value: &str) -> String {
    let formatted = format_rfc_relationship_refs(value);
    let Ok(re) = SUPERSEDED_BY_TARGET_RE.as_ref() else {
        return line.to_string();
    };
    re.replace(line, format!("${{1}}{formatted}")).to_string()
}

fn format_relationship_marker(label: &str, value: &str) -> String {
    format!("- **{label}**: {}", format_rfc_relationship_refs(value))
}

fn merge_relationship_refs<const N: usize>(values: [Option<&str>; N]) -> String {
    let mut ids: Vec<String> = Vec::new();
    for value in values.into_iter().flatten() {
        for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
            let id = id.strip_prefix("RFC ").unwrap_or(id).trim();
            let key = relationship_id_key(id);
            if !ids
                .iter()
                .any(|existing| relationship_id_key(existing) == key)
            {
                ids.push(id.to_string());
            }
        }
    }
    ids.join(", ")
}

fn relationship_id_key(id: &str) -> String {
    let id = id.trim().strip_prefix("RFC ").unwrap_or(id.trim()).trim();
    id.parse::<u64>()
        .map(|number| number.to_string())
        .unwrap_or_else(|_| id.to_string())
}

fn format_rfc_relationship_refs(value: &str) -> String {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.starts_with("RFC ") {
                part.to_string()
            } else {
                format!("RFC {part}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn extract_rfc_number_from_filename(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let prefix: String = stem.chars().take_while(char::is_ascii_digit).collect();
    if prefix.is_empty() {
        return None;
    }
    let rest = &stem[prefix.len()..];
    if rest.is_empty() || rest.starts_with(['-', '_']) {
        return Some(prefix);
    }
    None
}

#[must_use]
pub fn parse_rfc(filename: &str, content: &str, dir_stage: Option<u8>) -> Rfc {
    let matter = Matter::<YAML>::new();
    // Try parsing YAML Front Matter first
    let parsed = matter.parse(content);
    let fm_opt = if let Some(data) = parsed.data
        && let Ok(fm) = data.deserialize::<FrontMatter>()
    {
        Some(fm)
    } else {
        None
    };

    // Fallback: Parse Markdown Metadata (Title and **Key**: Value)
    let mut md_title = None;
    let mut md_feature = None;
    let mut md_stage = None;

    for line in content.lines() {
        if let Some(t) = line.strip_prefix("# ") {
            if md_title.is_none() {
                // Remove "RFC 000: " prefix if present
                let title_text = t.find(": ").map_or(t, |idx| &t[idx + 2..]);
                md_title = Some(clean_title(title_text));
            }
        } else if line.starts_with("**Status**:") {
            // Parse "**Status**: Stage 0 (Draft)"
            if let Some(status_part) = line.split(':').nth(1) {
                let status_text = status_part.trim();
                if status_text.starts_with("Stage ")
                    && let Some(digit) = status_text.chars().nth(6).and_then(|c| c.to_digit(10))
                {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        md_stage = Some(digit as u8);
                    }
                }
            }
        } else if (line.starts_with("**Context**:") || line.starts_with("**Feature**:"))
            && let Some(feat_part) = line.split(':').nth(1)
        {
            md_feature = Some(feat_part.trim().to_string());
        }
    }

    let title = fm_opt
        .as_ref()
        .and_then(|fm| fm.title.clone())
        .or(md_title)
        .map_or_else(|| "Untitled".to_string(), |t| clean_title(&t));

    let stage = dir_stage.or(md_stage).unwrap_or(0);

    let feature = fm_opt
        .as_ref()
        .and_then(|fm| fm.feature.clone())
        .or(md_feature)
        .unwrap_or_else(|| "Unknown".to_string());

    // Extract number from filename (e.g., 00001-project-name.md -> 00001)
    let number = extract_rfc_number_from_filename(filename).unwrap_or_else(|| "????".to_string());

    Rfc {
        filename: filename.to_string(),
        number,
        title,
        stage,
        feature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{SQLITE_DB_PATH, SqliteLoader, SqliteWriter};
    use std::fs;
    use tempfile::TempDir;

    fn run_git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn init_git_repository(root: &Path) {
        run_git(root, &["init", "--initial-branch=main"]);
        run_git(root, &["config", "user.name", "Exo Test"]);
        run_git(root, &["config", "user.email", "exo-test@example.invalid"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
    }

    fn commit_all(root: &Path, message: &str) -> String {
        run_git(root, &["add", "-A", "--", "docs/rfcs"]);
        run_git(root, &["commit", "-m", message]);
        run_git(root, &["rev-parse", "HEAD"])
    }

    fn publish_origin_main(root: &Path, oid: &str) {
        run_git(root, &["update-ref", "refs/remotes/origin/main", oid]);
        run_git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
    }

    #[test]
    fn request_transaction_defers_reconciled_key_publication() {
        let temp = TempDir::new().expect("tempdir");
        let db_path = crate::context::db_path(temp.path(), None);
        std::fs::create_dir_all(db_path.parent().expect("database parent"))
            .expect("create database parent");
        drop(exosuit_storage::open_database(&db_path).expect("initialize project database"));

        assert!(
            can_publish_reconciled_key(temp.path(), None)
                .expect("check publication outside request")
        );
        let transaction = exosuit_storage::RequestTransaction::begin(&db_path)
            .expect("begin request transaction");
        assert!(
            !can_publish_reconciled_key(temp.path(), None)
                .expect("check publication during request")
        );
        transaction
            .rollback()
            .expect("rollback request transaction");
        assert!(
            can_publish_reconciled_key(temp.path(), None).expect("check publication after request")
        );
    }

    fn shared_sidecar_project(root: &Path, git_common_dir: &Path, state_root: &Path) -> Project {
        Project {
            id: crate::project::ProjectId::from_git_common_dir(git_common_dir),
            git_common_dir: git_common_dir.to_path_buf(),
            workspace_root: Some(root.to_path_buf()),
            policy: crate::project::StatePolicy::Sidecar,
            projects_config_path: None,
            state_root: state_root.to_path_buf(),
            sidecar_key: Some("rfc-overlay-test".to_string()),
            sidecar_root: state_root.parent().map(Path::to_path_buf),
            sidecar_auto_commit: false,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::Never,
        }
    }

    #[test]
    fn request_observation_resolves_canonical_source_once_and_refreshes_next_request() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);
        let rfc_path = root.join("docs/rfcs/stage-1/00001-request-view.md");
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01requestview -->\n\n# RFC 1: Request View One\n\n## Summary\n\nFirst.\n",
        )
        .unwrap();
        let first_oid = commit_all(root, "first canonical view");
        publish_origin_main(root, &first_oid);
        let project = Project::resolve(root).unwrap();
        fs::create_dir_all(project.db_path().parent().unwrap()).unwrap();
        SqliteWriter::open(project.db_path()).unwrap();

        reset_canonical_source_observation_count();
        let (_, first_view) =
            observe_effective_rfc_view_with_project(root, Some(&project)).unwrap();
        assert_eq!(canonical_source_observation_count(), 1);
        assert_eq!(
            first_view.workspace_snapshot().canonical_oid.as_deref(),
            Some(first_oid.as_str())
        );
        assert!(first_view.records.iter().all(|record| {
            record.provenance.canonical_head.as_deref() == Some(first_oid.as_str())
        }));

        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01requestview -->\n\n# RFC 1: Request View Two\n\n## Summary\n\nSecond.\n",
        )
        .unwrap();
        let second_oid = commit_all(root, "second canonical view");
        publish_origin_main(root, &second_oid);

        let (_, second_view) =
            observe_effective_rfc_view_with_project(root, Some(&project)).unwrap();
        assert_eq!(canonical_source_observation_count(), 2);
        assert_eq!(
            second_view.workspace_snapshot().canonical_oid.as_deref(),
            Some(second_oid.as_str())
        );
        assert_eq!(second_view.records[0].record.title, "Request View Two");
        assert!(second_view.records.iter().all(|record| {
            record.provenance.canonical_head.as_deref() == Some(second_oid.as_str())
        }));
    }

    #[test]
    fn test_parse_rfc_frontmatter() {
        let content = r"---
title: My RFC
feature: Core
---
# Content
    ";
        let rfc = parse_rfc("0001-my-rfc.md", content, Some(2));
        assert_eq!(rfc.title, "My RFC");
        assert_eq!(rfc.stage, 2);
        assert_eq!(rfc.feature, "Core");
        assert_eq!(rfc.number, "0001");
    }

    #[test]
    fn retired_stage_reads_top_level_yaml_frontmatter() {
        let content = "---\nstage: 3\n---\n\n# RFC 1: Retired\n\nBody.\n";

        assert_eq!(retired_rfc_stage_from_document(content, None, 0), 3);
    }

    #[test]
    fn test_parse_rfc_markdown() {
        let content = r"
# RFC 0007: My RFC

**Status**: Stage 1 (Proposal)
**Feature**: Core

Content
";
        let rfc = parse_rfc("0001-my-rfc.md", content, None);
        assert_eq!(rfc.title, "My RFC");
        assert_eq!(rfc.stage, 1);
        assert_eq!(rfc.feature, "Core");
    }

    #[test]
    fn test_extract_rfc_relationships_from_metadata_bullets() {
        let content = r#"<!-- exo:22 ulid:01old -->

# RFC 0022: Unified Project State

- **Superseded by**: RFC 10176
- **Supersedes**: RFC 00177, RFC 00229, RFC 10161
"#;

        let relationships = extract_rfc_relationships(content);
        assert_eq!(relationships.superseded_by.as_deref(), Some("10176"));
        assert_eq!(
            relationships.supersedes.as_deref(),
            Some("00177, 00229, 10161")
        );
    }

    #[test]
    fn test_extract_rfc_relationships_from_existing_marker_formats() {
        let superseded = extract_rfc_relationships(
            "> **⚠️ Superseded by [RFC 00233: ExoSpec — Unified Command Definition](00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md)**",
        );
        assert_eq!(superseded.superseded_by.as_deref(), Some("00233"));

        let supersedes = extract_rfc_relationships("> Supersedes: RFC 0094, RFC 10169, RFC 00239");
        assert_eq!(supersedes.supersedes.as_deref(), Some("0094, 10169, 00239"));
    }

    #[test]
    fn test_extract_rfc_relationships_merges_duplicate_supersedes_metadata() {
        let content = r#"<!-- exo:124 ulid:01async -->

# RFC 0124: Inbox System (Async Intent Channel)

- **Supersedes**: RFC 0116 (Feedback System), RFC 00185 (Inbox-Driven Sidebar Actions), RFC 10071 (Context Inbox)

| Field          | Value                                    |
| -------------- | ---------------------------------------- |
| **Supersedes** | RFC 0116, RFC 00185, RFC 10071, RFC 0016 |
"#;

        let relationships = extract_rfc_relationships(content);
        assert_eq!(
            relationships.supersedes.as_deref(),
            Some("0116, 00185, 10071, 0016")
        );
    }

    #[test]
    fn test_extract_rfc_relationships_from_status_and_reason_metadata() {
        let reason = extract_rfc_relationships(
            "> **Reason**: Superseded by RFC 0111: Agent Guidance Architecture",
        );
        assert_eq!(reason.superseded_by.as_deref(), Some("0111"));

        let status = extract_rfc_relationships("> **Status**: Withdrawn (superseded by RFC 0122)");
        assert_eq!(status.superseded_by.as_deref(), Some("0122"));

        let note =
            extract_rfc_relationships("> **Note**: This RFC has been superseded by RFC 00225.");
        assert_eq!(note.superseded_by.as_deref(), Some("00225"));
    }

    #[test]
    fn test_extract_rfc_relationships_from_sentence_style_superseded_marker() {
        let relationships = extract_rfc_relationships("This RFC has been superseded by RFC 0057.");

        assert_eq!(relationships.superseded_by.as_deref(), Some("0057"));
    }

    #[test]
    fn test_extract_rfc_relationships_from_section_style_supersedes() {
        let content = r#"## Superseded Documents

This RFC supersedes:

- RFC 0131 (Implementation Plan as Canonical Execution Artifact) — `implementation-plan.toml` is no longer canonical.
- Any migration plan or design document that describes TOML projections of SQLite state as a supported pattern.
"#;

        let relationships = extract_rfc_relationships(content);
        assert_eq!(relationships.supersedes.as_deref(), Some("0131"));
    }

    #[test]
    fn test_extract_rfc_relationships_skips_prose_mentions() {
        let content = r#"<!-- exo:162 ulid:01prose -->

# RFC 0162: Copilot Resources

- **Rich Relationships**: RFCs are not islands. They relate to each other (Enforces, Refines, Supersedes).

| Legacy task-list.toml | Superseded by implementation-plan.toml |
| **Supersedes** | — |
"#;

        let relationships = extract_rfc_relationships(content);
        assert_eq!(relationships.superseded_by, None);
        assert_eq!(relationships.supersedes, None);
    }

    #[test]
    fn test_merge_relationship_refs_dedupes_padded_and_unpadded_ids() {
        assert_eq!(
            merge_relationship_refs([Some("0022"), Some("22"), Some("RFC 0022")]),
            "0022"
        );
    }

    #[test]
    fn test_get_next_rfc_id_from_filenames() {
        let temp = TempDir::new().unwrap();
        let rfc_root = temp.path().join("docs/rfcs");
        fs::create_dir_all(rfc_root.join("stage-1")).unwrap();
        fs::create_dir_all(rfc_root.join("stage-2")).unwrap();

        fs::write(rfc_root.join("README.md"), "ignore").unwrap();
        fs::write(rfc_root.join("0000-template.md"), "ignore").unwrap();
        fs::write(rfc_root.join("stage-1/0012-foo.md"), "").unwrap();
        fs::write(rfc_root.join("stage-2/0154-bar.md"), "").unwrap();
        fs::write(rfc_root.join("stage-2/not-an-rfc.md"), "").unwrap();

        let next = get_next_rfc_id(&rfc_root).unwrap();
        // Next RFC number after 0154 should be 00155 (5-digit zero-padded format)
        assert_eq!(next, "00155");
    }

    #[test]
    fn test_get_next_rfc_id_counts_legacy_flat_rfcs_not_evidence_notes() {
        let temp = TempDir::new().unwrap();
        let rfc_root = temp.path().join("docs/rfcs");
        fs::create_dir_all(rfc_root.join("stage-0")).unwrap();
        fs::create_dir_all(rfc_root.join("evidence/0008-served-transport")).unwrap();

        fs::write(rfc_root.join("stage-0/00007-modern.md"), "").unwrap();
        fs::write(rfc_root.join("00042-legacy-flat.md"), "").unwrap();
        fs::write(
            rfc_root.join("evidence/0008-served-transport/2026-06-11-live-probes.md"),
            "",
        )
        .unwrap();

        let next = get_next_rfc_id(&rfc_root).unwrap();
        assert_eq!(next, "00043");
    }

    #[test]
    fn test_get_next_rfc_id_empty_dir() {
        let temp = TempDir::new().unwrap();
        let rfc_root = temp.path().join("docs/rfcs");
        fs::create_dir_all(&rfc_root).unwrap();

        let next = get_next_rfc_id(&rfc_root).unwrap();
        assert_eq!(next, "00001");
    }

    #[test]
    fn test_create_generates_sequential_ids() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let first = create(root, "First RFC", None, "Core", 0, None).unwrap();
        let first_name = first.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(first_name, "00001-first-rfc.md");

        let second = create(root, "Second RFC", None, "Core", 0, None).unwrap();
        let second_name = second.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(second_name, "00002-second-rfc.md");

        let first_content = fs::read_to_string(first).unwrap();
        assert!(first_content.starts_with("<!-- exo:1 ulid:"));
        assert!(first_content.contains("\n\n# RFC 1: First RFC\n\n"));
        assert!(!first_content.starts_with("---\n"));
    }

    #[test]
    fn test_shared_rfc_parsers() {
        let path = Path::new("docs/rfcs/withdrawn/0101-example-rfc.md");
        let content = "<!-- exo:101 ulid:01abcxyz -->\n\n# RFC 101: Example RFC\n";

        assert_eq!(parse_rfc_number("0101-example-rfc.md"), Some(101));
        assert_eq!(parse_slug("0101-example-rfc.md"), "example-rfc");
        assert_eq!(parse_stage(path), 0);
        assert_eq!(parse_status(path), "withdrawn");
        assert!(has_anchor(content));
        assert_eq!(extract_anchor_ulid(content).as_deref(), Some("01abcxyz"));
        assert_eq!(extract_h1_title(content).as_deref(), Some("Example RFC"));
        assert_eq!(
            strip_frontmatter("---\ntitle: Example\n---\n\n# RFC 1: Example\n"),
            "\n# RFC 1: Example\n"
        );
    }

    #[test]
    fn canonical_reconciliation_reads_origin_head_instead_of_feature_branch() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let rfc_path = root.join("docs/rfcs/stage-1/00001-canonical.md");
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01canonical -->\n\n# RFC 1: Canonical Title\n\n**Feature**: core\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        let main_oid = commit_all(root, "canonical main");
        publish_origin_main(root, &main_oid);

        run_git(root, &["checkout", "-b", "feature"]);
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01canonical -->\n\n# RFC 1: Feature Title\n\n**Feature**: feature-only\n\n## Summary\n\nFeature.\n",
        )
        .unwrap();
        commit_all(root, "feature edit");

        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.inserted, 1);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let record = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(record.title, "Canonical Title");
        assert_eq!(record.feature.as_deref(), Some("core"));
        assert_eq!(record.file_path, "docs/rfcs/stage-1/00001-canonical.md");
        let rowset_before: i64 = loader
            .database()
            .connection()
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'rfcs_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        reconcile_rfcs_with_project(root, None).unwrap();
        let rowset_after: i64 = loader
            .database()
            .connection()
            .query_row(
                "SELECT counter FROM rowset_revisions WHERE table_name = 'rfcs_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rowset_after, rowset_before);
    }

    #[test]
    fn missing_observed_rfc_rolls_back_canonical_reconciliation() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);
        fs::write(
            root.join("docs/rfcs/stage-1/00001-transactional-show.md"),
            "<!-- exo:1 ulid:01transactionalshow -->\n\n# RFC 1: Transactional Show\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        let canonical_oid = commit_all(root, "canonical show RFC");
        publish_origin_main(root, &canonical_oid);
        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();

        assert!(
            observe_effective_rfc_by_number(root, None, 999)
                .unwrap()
                .is_none()
        );
        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        assert!(
            loader.load_rfc_by_number(1).unwrap().is_none(),
            "a failed lookup must not commit canonical reconciliation"
        );
        assert!(
            loader
                .load_rfc_workspace_snapshot(&slash_path_string(&normalize_key_path(root)))
                .unwrap()
                .is_none(),
            "a failed lookup must roll back its workspace snapshot"
        );

        let found = observe_effective_rfc_by_number(root, None, 1)
            .unwrap()
            .expect("canonical RFC should relink on a successful lookup");
        assert_eq!(found.record.title, "Transactional Show");
        assert!(loader.load_rfc_by_number(1).unwrap().is_some());
    }

    #[test]
    fn workspace_fallback_reconciles_each_observed_read() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        let rfc_path = root.join("docs/rfcs/stage-1/00001-fallback.md");
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01fallbackread -->\n\n# RFC 1: First Fallback View\n\n## Summary\n\nFirst.\n",
        )
        .unwrap();
        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01fallbackread",
                1,
                "Seeded Fallback View",
                1,
                "active",
                None,
                "fallback",
                "docs/rfcs/stage-1/00001-fallback.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        reset_workspace_rfc_document_load_count();
        let first = observe_effective_rfcs(root, None).unwrap();
        assert_eq!(workspace_rfc_document_load_count(), 1);
        assert_eq!(first[0].record.title, "First Fallback View");
        assert_eq!(
            SqliteLoader::open(root.join(SQLITE_DB_PATH))
                .unwrap()
                .load_rfc_by_number(1)
                .unwrap()
                .unwrap()
                .title,
            "First Fallback View"
        );

        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01fallbackread -->\n\n# RFC 1: Second Fallback View\n\n## Summary\n\nSecond.\n",
        )
        .unwrap();
        let second = observe_effective_rfcs(root, None).unwrap();
        assert_eq!(workspace_rfc_document_load_count(), 2);
        assert_eq!(second[0].record.title, "Second Fallback View");
        let shared = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(shared.title, "Second Fallback View");
    }

    #[test]
    fn linked_worktrees_compose_distinct_rfc_views_over_shared_canonical_state() {
        let temp = TempDir::new().unwrap();
        let main_root = temp.path().join("main");
        let feature_root = temp.path().join("feature");
        let state_root = temp.path().join("sidecar-state");
        fs::create_dir_all(main_root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(state_root.join("cache")).unwrap();
        init_git_repository(&main_root);

        let canonical_path = main_root.join("docs/rfcs/stage-1/00001-overlay.md");
        fs::write(
            &canonical_path,
            "<!-- exo:1 ulid:01overlay -->\n\n# RFC 1: Canonical Overlay\n\n**Feature**: core\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        fs::write(
            main_root.join("docs/rfcs/stage-1/00002-canonical-only.md"),
            "<!-- exo:2 ulid:01canonicalonly -->\n\n# RFC 2: Canonical Only\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        let canonical_oid = commit_all(&main_root, "canonical RFC");
        publish_origin_main(&main_root, &canonical_oid);
        run_git(
            &main_root,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_root.to_str().unwrap(),
                "main",
            ],
        );

        fs::create_dir_all(feature_root.join("docs/rfcs/stage-2")).unwrap();
        fs::rename(
            feature_root.join("docs/rfcs/stage-1/00001-overlay.md"),
            feature_root.join("docs/rfcs/stage-2/00001-overlay.md"),
        )
        .unwrap();
        fs::remove_file(feature_root.join("docs/rfcs/stage-1/00002-canonical-only.md")).unwrap();

        let git_common_dir = main_root.join(".git");
        let main_project = shared_sidecar_project(&main_root, &git_common_dir, &state_root);
        let feature_project = shared_sidecar_project(&feature_root, &git_common_dir, &state_root);
        SqliteWriter::open(main_project.db_path()).unwrap();
        reconcile_rfcs_with_project(&main_root, Some(&main_project)).unwrap();

        let main_view = load_effective_rfc_by_number(&main_root, Some(&main_project), 1)
            .unwrap()
            .unwrap();
        let feature_view = load_effective_rfc_by_number(&feature_root, Some(&feature_project), 1)
            .unwrap()
            .unwrap();
        let shared = SqliteLoader::open(main_project.db_path())
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        let absent_from_feature =
            load_effective_rfc_by_number(&feature_root, Some(&feature_project), 2)
                .unwrap()
                .unwrap();

        assert_eq!(main_view.record.stage, 1);
        assert!(!main_view.provenance.differs_from_canonical);
        assert_eq!(feature_view.record.stage, 2);
        assert_eq!(feature_view.provenance.document_source, "workspace");
        assert_eq!(
            feature_view.provenance.workspace_branch.as_deref(),
            Some("feature")
        );
        assert!(feature_view.provenance.differs_from_canonical);
        assert_eq!(absent_from_feature.provenance.document_source, "canonical");
        assert_eq!(absent_from_feature.provenance.workspace_presence, "absent");
        let feature_rfcs = load_effective_rfcs(&feature_root, Some(&feature_project)).unwrap();
        assert_eq!(
            get_next_effective_rfc_id(&feature_root.join(RFCS_DIR), &feature_rfcs).unwrap(),
            "00003",
            "number allocation must include canonical RFCs absent from the workspace"
        );
        assert_eq!(
            shared.stage, 1,
            "feature observation cannot mutate canonical state"
        );

        let loader = SqliteLoader::open(main_project.db_path()).unwrap();
        assert!(
            loader
                .load_rfc_workspace_snapshot(&slash_path_string(&normalize_key_path(&main_root)))
                .unwrap()
                .is_some()
        );
        assert!(
            loader
                .load_rfc_workspace_snapshot(&slash_path_string(&normalize_key_path(&feature_root)))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn linked_worktree_withdrawal_becomes_canonical_without_stale_restore() {
        let temp = TempDir::new().unwrap();
        let main_root = temp.path().join("main");
        let feature_root = temp.path().join("feature");
        let fresh_root = temp.path().join("fresh");
        let state_root = temp.path().join("sidecar-state");
        fs::create_dir_all(main_root.join("docs/rfcs/stage-3")).unwrap();
        fs::create_dir_all(state_root.join("cache")).unwrap();
        init_git_repository(&main_root);

        let active_path = main_root.join("docs/rfcs/stage-3/00129-runner.md");
        fs::write(
            &active_path,
            "<!-- exo:129 ulid:01linkedrunner -->\n\n# RFC 129: Configurable Runner\n\n**Stage**: 3\n\n## Summary\n\nActive.\n",
        )
        .unwrap();
        let active_oid = commit_all(&main_root, "active runner RFC");
        publish_origin_main(&main_root, &active_oid);
        run_git(
            &main_root,
            &[
                "worktree",
                "add",
                "-b",
                "feature/withdraw-runner",
                feature_root.to_str().unwrap(),
                "main",
            ],
        );

        let git_common_dir = main_root.join(".git");
        let main_project = shared_sidecar_project(&main_root, &git_common_dir, &state_root);
        let feature_project = shared_sidecar_project(&feature_root, &git_common_dir, &state_root);
        SqliteWriter::open(main_project.db_path()).unwrap();
        reconcile_rfcs_with_project(&main_root, Some(&main_project)).unwrap();

        fs::create_dir_all(feature_root.join("docs/rfcs/withdrawn")).unwrap();
        let withdrawn_path = feature_root.join("docs/rfcs/withdrawn/00129-runner.md");
        fs::rename(
            feature_root.join("docs/rfcs/stage-3/00129-runner.md"),
            &withdrawn_path,
        )
        .unwrap();
        fs::write(
            &withdrawn_path,
            "<!-- exo:129 ulid:01linkedrunner -->\n\n# RFC 129: Configurable Runner\n\n**Status**: Withdrawn\n**Stage**: 3\n**Reason**: The configurable runner surface was not implemented.\n\n## Summary\n\nHistorical.\n",
        )
        .unwrap();

        let feature_view = load_effective_rfc_by_number(&feature_root, Some(&feature_project), 129)
            .unwrap()
            .unwrap();
        let main_view = load_effective_rfc_by_number(&main_root, Some(&main_project), 129)
            .unwrap()
            .unwrap();
        let shared_before = SqliteLoader::open(main_project.db_path())
            .unwrap()
            .load_rfc_by_number(129)
            .unwrap()
            .unwrap();
        assert_eq!(feature_view.record.status, "withdrawn");
        assert!(feature_view.provenance.differs_from_canonical);
        assert_eq!(main_view.record.status, "active");
        assert_eq!(shared_before.status, "active");

        let withdrawn_oid = commit_all(&feature_root, "withdraw runner RFC");
        publish_origin_main(&main_root, &withdrawn_oid);

        reconcile_rfcs_with_project(&main_root, Some(&main_project)).unwrap();
        let shared_after_restart = SqliteLoader::open(main_project.db_path())
            .unwrap()
            .load_rfc_by_number(129)
            .unwrap()
            .unwrap();
        assert_eq!(shared_after_restart.stage, 3);
        assert_eq!(shared_after_restart.status, "withdrawn");
        assert_eq!(
            shared_after_restart.withdrawal_reason.as_deref(),
            Some("The configurable runner surface was not implemented.")
        );

        let stale_main_view = load_effective_rfc_by_number(&main_root, Some(&main_project), 129)
            .unwrap()
            .unwrap();
        assert_eq!(stale_main_view.record.status, "active");
        assert!(stale_main_view.provenance.differs_from_canonical);
        let shared_after_stale_read = SqliteLoader::open(main_project.db_path())
            .unwrap()
            .load_rfc_by_number(129)
            .unwrap()
            .unwrap();
        assert_eq!(
            shared_after_stale_read.status, "withdrawn",
            "a stale linked worktree view cannot restore older canonical metadata"
        );

        run_git(
            &main_root,
            &[
                "worktree",
                "add",
                "--detach",
                fresh_root.to_str().unwrap(),
                "refs/remotes/origin/main",
            ],
        );
        let fresh_project = shared_sidecar_project(&fresh_root, &git_common_dir, &state_root);
        let fresh_view = load_effective_rfc_by_number(&fresh_root, Some(&fresh_project), 129)
            .unwrap()
            .unwrap();
        assert_eq!(fresh_view.record.status, "withdrawn");
        assert!(!fresh_view.provenance.differs_from_canonical);

        let loader = SqliteLoader::open(main_project.db_path()).unwrap();
        let dumps = exosuit_storage::dump_tables(loader.database().connection()).unwrap();
        let rfcs_dump = dumps
            .iter()
            .find_map(|(table, sql)| (table == "rfcs_data").then_some(sql))
            .expect("portable RFC projection");
        assert!(rfcs_dump.contains("The configurable runner surface was not implemented."));
        for root in [&main_root, &feature_root, &fresh_root] {
            assert!(
                !rfcs_dump.contains(root.to_string_lossy().as_ref()),
                "portable RFC projection must omit workspace root {}",
                root.display()
            );
        }
        for local_table in [
            "rfc_workspace_snapshots_data",
            "rfc_workspace_observations_data",
            "rfc_workspace_diagnostics_data",
        ] {
            assert!(
                dumps.iter().all(|(table, _)| table != local_table),
                "portable dumps must omit {local_table}"
            );
        }
    }

    #[test]
    fn managed_branch_mutations_update_overlay_without_advancing_canonical_state() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let text_id = ulid::Ulid::new().to_string().to_lowercase();
        fs::write(
            root.join("docs/rfcs/stage-1/00001-managed.md"),
            format!(
                "<!-- exo:1 ulid:{text_id} -->\n\n# RFC 1: Managed\n\n**Feature**: core\n\n## Summary\n\nCanonical.\n"
            ),
        )
        .unwrap();
        let canonical_oid = commit_all(root, "canonical RFC");
        publish_origin_main(root, &canonical_oid);
        run_git(root, &["checkout", "-b", "feature"]);

        let project = Project::resolve(root).unwrap();
        fs::create_dir_all(project.db_path().parent().unwrap()).unwrap();
        SqliteWriter::open(project.db_path()).unwrap();
        reconcile_rfcs_with_project(root, Some(&project)).unwrap();
        let repairs = detect_rfc_repair_candidates(root).unwrap();
        assert!(repairs.is_empty(), "unexpected repair debt: {repairs:#?}");
        withdraw(
            &root.join(RFCS_DIR),
            "1",
            Some("The feature branch retired this proposal."),
        )
        .unwrap();
        let workspace_root = slash_path_string(&normalize_key_path(root));
        let persisted_withdrawal = SqliteLoader::open(project.db_path())
            .unwrap()
            .load_rfc_workspace_observations(&workspace_root)
            .unwrap()
            .into_iter()
            .find(|record| record.rfc_number == 1)
            .expect("withdraw should refresh the workspace observation before returning");
        assert_eq!(persisted_withdrawal.status, "withdrawn");
        assert_eq!(
            persisted_withdrawal.withdrawal_reason.as_deref(),
            Some("The feature branch retired this proposal.")
        );
        let created = create(
            root,
            "Workspace Only",
            Some("00002"),
            "workspace-feature",
            0,
            Some("This proposal is still local to the feature branch."),
        )
        .unwrap();

        let shared_loader = SqliteLoader::open(project.db_path()).unwrap();
        let shared = shared_loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(shared.status, "active");
        assert!(shared_loader.load_rfc_by_number(2).unwrap().is_none());

        let withdrawn = load_effective_rfc_by_number(root, Some(&project), 1)
            .unwrap()
            .unwrap();
        assert_eq!(withdrawn.record.status, "withdrawn");
        assert_eq!(
            withdrawn.record.withdrawal_reason.as_deref(),
            Some("The feature branch retired this proposal.")
        );
        assert!(withdrawn.provenance.differs_from_canonical);

        let workspace_only = load_effective_rfc_by_number(root, Some(&project), 2)
            .unwrap()
            .unwrap();
        assert_eq!(
            workspace_only.record.feature.as_deref(),
            Some("workspace-feature")
        );
        assert_eq!(workspace_only.provenance.canonical_presence, "unpublished");
        assert_eq!(
            workspace_only.record.file_path,
            relative_workspace_path(root, &created)
        );

        commit_all(root, "commit managed overlay changes");
        let committed_withdrawal = load_effective_rfc_by_number(root, Some(&project), 1)
            .unwrap()
            .unwrap();
        let committed_workspace_only = load_effective_rfc_by_number(root, Some(&project), 2)
            .unwrap()
            .unwrap();
        assert_eq!(
            committed_withdrawal.record.withdrawal_reason.as_deref(),
            Some("The feature branch retired this proposal."),
            "committing an unchanged RFC document must preserve its workspace lifecycle metadata"
        );
        assert_eq!(
            committed_workspace_only.record.feature.as_deref(),
            Some("workspace-feature"),
            "committing an unchanged RFC document must preserve its workspace feature metadata"
        );
    }

    #[test]
    fn managed_promotion_updates_declared_stage_marker_before_overlay_refresh() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        init_git_repository(root);

        fs::write(
            root.join("docs/rfcs/stage-0/00001-promote.md"),
            "<!-- exo:1 ulid:01promotemarker -->\n\n# RFC 1: Promote\n\n**Stage**: 0\n\n## Summary\n\nCandidate.\n",
        )
        .unwrap();
        let canonical_oid = commit_all(root, "canonical promotion candidate");
        publish_origin_main(root, &canonical_oid);
        run_git(root, &["checkout", "-b", "feature"]);

        let project = Project::resolve(root).unwrap();
        fs::create_dir_all(project.db_path().parent().unwrap()).unwrap();
        SqliteWriter::open(project.db_path()).unwrap();
        reconcile_rfcs_with_project(root, Some(&project)).unwrap();

        promote(&root.join(RFCS_DIR), "1").unwrap();

        let promoted_path = root.join("docs/rfcs/stage-1/00001-promote.md");
        let promoted_content = fs::read_to_string(&promoted_path).unwrap();
        assert!(promoted_content.contains("**Stage**: 1"));
        let effective = load_effective_rfc_by_number(root, Some(&project), 1)
            .unwrap()
            .unwrap();
        assert_eq!(effective.record.stage, 1);
        assert_eq!(
            effective.record.file_path,
            "docs/rfcs/stage-1/00001-promote.md"
        );
    }

    #[test]
    fn unborn_single_worktree_uses_workspace_reconciliation() {
        let temp = TempDir::new().unwrap();
        init_git_repository(temp.path());

        assert_eq!(
            canonical_reconcile_source(temp.path()).unwrap(),
            CanonicalReconcileSource::WorkspaceFallback
        );
    }

    #[test]
    fn direct_reconciliation_waits_for_the_cross_process_lock() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);
        fs::write(
            root.join("docs/rfcs/stage-1/00001-lock.md"),
            "<!-- exo:1 ulid:01lock -->\n\n# RFC 1: Lock\n\n## Summary\n\nLocked.\n",
        )
        .unwrap();
        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let thread_root = root.to_path_buf();
        let mut worker = None;

        with_reconcile_lock(root, None, || {
            worker = Some(std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let result = reconcile_rfcs_with_project(&thread_root, None)
                    .map_err(|error| format!("{error:#}"));
                finished_tx.send(result).unwrap();
            }));

            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            assert!(
                finished_rx
                    .recv_timeout(std::time::Duration::from_millis(200))
                    .is_err(),
                "reconciliation completed while another owner held the lock"
            );
            Ok(())
        })
        .unwrap();

        finished_rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .unwrap()
            .unwrap();
        worker.unwrap().join().unwrap();
    }

    #[test]
    fn observed_lookup_takes_reconcile_lock_before_database_transaction() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);
        fs::write(
            root.join("docs/rfcs/stage-1/00001-lock-order.md"),
            "<!-- exo:1 ulid:01lockorder -->\n\n# RFC 1: Lock Order\n\n## Summary\n\nLocked.\n",
        )
        .unwrap();
        let canonical_oid = commit_all(root, "canonical lock-order RFC");
        publish_origin_main(root, &canonical_oid);
        let db_path = root.join(SQLITE_DB_PATH);
        SqliteWriter::open(&db_path).unwrap();

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let thread_root = root.to_path_buf();
        let mut worker = None;

        with_reconcile_lock(root, None, || {
            worker = Some(std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let result = observe_effective_rfc_by_number(&thread_root, None, 999)
                    .map_err(|error| format!("{error:#}"));
                finished_tx.send(result).unwrap();
            }));
            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();

            let writer = SqliteWriter::open(&db_path).unwrap();
            writer
                .add_axiom(
                    "lock-order-probe",
                    "workflow",
                    "database remains writable",
                    None,
                    None,
                    &[],
                    &[],
                )
                .unwrap();
            assert!(
                finished_rx
                    .recv_timeout(std::time::Duration::from_millis(100))
                    .is_err(),
                "lookup should wait on the reconcile lock before opening its transaction"
            );
            Ok(())
        })
        .unwrap();

        assert!(
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .is_none()
        );
        worker.unwrap().join().unwrap();
    }

    #[test]
    fn once_reconciliation_resolves_canonical_ref_after_lock_acquisition() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let rfc_path = root.join("docs/rfcs/stage-1/00001-lock-ref.md");
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01lockref -->\n\n# RFC 1: Before Lock\n\n## Summary\n\nBefore.\n",
        )
        .unwrap();
        let before_oid = commit_all(root, "before lock");
        publish_origin_main(root, &before_oid);

        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01lockref -->\n\n# RFC 1: After Lock\n\n## Summary\n\nAfter.\n",
        )
        .unwrap();
        let after_oid = commit_all(root, "after lock");
        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let thread_root = root.to_path_buf();
        let mut worker = None;

        with_reconcile_lock(root, None, || {
            worker = Some(std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let result = reconcile_rfcs_once_with_project(&thread_root, None)
                    .map_err(|error| format!("{error:#}"));
                finished_tx.send(result).unwrap();
            }));
            started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            publish_origin_main(root, &after_oid);
            Ok(())
        })
        .unwrap();

        finished_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap()
            .unwrap();
        worker.unwrap().join().unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let record = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(record.title, "After Lock");
    }

    #[test]
    fn canonical_oid_advance_updates_shared_state_from_a_stale_worktree() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-3")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let active_path = root.join("docs/rfcs/stage-3/00129-runner.md");
        fs::write(
            &active_path,
            "<!-- exo:129 ulid:01runner -->\n\n# RFC 129: Runner\n\n## Summary\n\nActive.\n",
        )
        .unwrap();
        let active_oid = commit_all(root, "active candidate");
        publish_origin_main(root, &active_oid);
        run_git(root, &["branch", "stale-worktree"]);

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01runner",
                129,
                "Runner",
                3,
                "active",
                None,
                "runner",
                "docs/rfcs/stage-3/00129-runner.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        reconcile_rfcs_once_with_project(root, None).unwrap();

        fs::create_dir_all(root.join("docs/rfcs/withdrawn")).unwrap();
        let withdrawn_path = root.join("docs/rfcs/withdrawn/00129-runner.md");
        fs::rename(&active_path, &withdrawn_path).unwrap();
        fs::write(
            &withdrawn_path,
            "<!-- exo:129 ulid:01runner -->\n\n# RFC 129: Runner\n\n**Status**: Withdrawn\n**Stage**: 3\n**Reason**: The runner surface was not implemented.\n\n## Summary\n\nHistorical.\n",
        )
        .unwrap();
        let withdrawn_oid = commit_all(root, "withdraw candidate");
        publish_origin_main(root, &withdrawn_oid);
        run_git(root, &["checkout", "stale-worktree"]);

        let result = reconcile_rfcs_once_with_project(root, None).unwrap();
        assert_eq!(result.updated, 1);
        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let record = loader.load_rfc_by_number(129).unwrap().unwrap();
        assert_eq!(record.stage, 3);
        assert_eq!(record.status, "withdrawn");
        assert_eq!(
            record.withdrawal_reason.as_deref(),
            Some("The runner surface was not implemented.")
        );
        assert_eq!(record.file_path, "docs/rfcs/withdrawn/00129-runner.md");
    }

    #[test]
    fn canonical_baseline_quarantines_branch_only_rows_and_later_absence_preserves_state() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let canonical_path = root.join("docs/rfcs/stage-1/10200-canonical.md");
        fs::write(
            &canonical_path,
            "<!-- exo:10200 ulid:01canonical -->\n\n# RFC 10200: Canonical\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        let initial_oid = commit_all(root, "canonical RFC");
        publish_origin_main(root, &initial_oid);

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01branchonly",
                10999,
                "Branch Only",
                1,
                "active",
                None,
                "branch-only",
                "docs/rfcs/stage-1/10999-branch-only.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.deleted, 1);
        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        assert!(loader.load_rfc_by_number(10200).unwrap().is_some());
        assert!(loader.load_rfc_by_number(10999).unwrap().is_none());
        let quarantined: i64 = writer
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM rfc_canonical_quarantine WHERE text_id = '01branchonly'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, 1);

        let invalid_relink_path = root.join("docs/rfcs/stage-1/10999-branch-only.md");
        fs::write(
            &invalid_relink_path,
            "<!-- exo:10998 ulid:01branchonly -->\n\n# RFC 10998: Invalid Relink\n\n## Summary\n\nInvalid.\n",
        )
        .unwrap();
        let invalid_oid = commit_all(root, "invalid canonical relink");
        publish_origin_main(root, &invalid_oid);
        reconcile_rfcs_with_project(root, None).unwrap();
        assert!(loader.load_rfc_by_number(10999).unwrap().is_none());
        let quarantined: i64 = writer
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM rfc_canonical_quarantine WHERE text_id = '01branchonly'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quarantined, 1,
            "an invalid canonical document cannot clear quarantine"
        );

        writer
            .upsert_rfc(
                "01laterbranch",
                10998,
                "Later Branch Only",
                1,
                "active",
                None,
                "later-branch-only",
                "docs/rfcs/stage-1/10998-later-branch-only.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.deleted, 1);
        assert!(loader.load_rfc_by_number(10998).unwrap().is_none());
        let quarantine_reason: String = writer
            .database()
            .connection()
            .query_row(
                "SELECT quarantine_reason FROM rfc_canonical_quarantine
                 WHERE text_id = '01laterbranch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantine_reason, "missing_from_canonical_history");

        fs::remove_file(&canonical_path).unwrap();
        let absent_oid = commit_all(root, "remove canonical document");
        publish_origin_main(root, &absent_oid);
        reconcile_rfcs_with_project(root, None).unwrap();
        assert!(
            loader.load_rfc_by_number(10200).unwrap().is_some(),
            "canonical absence after baseline preserves shared identity"
        );
    }

    #[test]
    fn canonical_reconciliation_replaces_stale_path_owner_with_valid_anchor() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        let canonical_path = root.join("docs/rfcs/stage-1/00123-canonical.md");
        fs::write(
            &canonical_path,
            "<!-- exo:123 ulid:01canonicalpath -->\n\n# RFC 123: Canonical\n\n## Summary\n\nCanonical.\n",
        )
        .unwrap();
        let canonical_oid = commit_all(root, "canonical path owner");
        publish_origin_main(root, &canonical_oid);

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01stalepath",
                122,
                "Stale Path Owner",
                1,
                "active",
                None,
                "stale-path-owner",
                "docs/rfcs/stage-1/00123-canonical.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.deleted, 1);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let canonical = loader.load_rfc_by_number(123).unwrap().unwrap();
        assert_eq!(canonical.text_id, "01canonicalpath");
        assert!(loader.load_rfc_by_number(122).unwrap().is_none());
        let quarantined: i64 = writer
            .database()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM rfc_canonical_quarantine WHERE text_id = '01stalepath'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, 1);
    }

    #[test]
    fn canonical_reconciliation_relinks_legacy_lifecycle_rfc_without_stage_marker() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/withdrawn")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        fs::write(
            root.join("docs/rfcs/withdrawn/00129-legacy.md"),
            "<!-- exo:129 ulid:01legacy -->\n\n# RFC 129: Legacy\n\n**Reason**: Retired.\n\n## Summary\n\nHistorical.\n",
        )
        .unwrap();
        let oid = commit_all(root, "legacy withdrawn RFC");
        publish_origin_main(root, &oid);

        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.inserted, 1);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let record = loader.load_rfc_by_number(129).unwrap().unwrap();
        assert_eq!(record.status, "withdrawn");
        assert_eq!(record.stage, 0);
        assert_eq!(record.withdrawal_reason.as_deref(), Some("Retired."));
    }

    #[test]
    fn canonical_reconciliation_preserves_distinct_anchors_with_the_same_number() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join("docs/rfcs/withdrawn")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        init_git_repository(root);

        fs::write(
            root.join("docs/rfcs/stage-1/00100-current.md"),
            "<!-- exo:100 ulid:01current -->\n\n# RFC 100: Current\n\n## Summary\n\nCurrent.\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/rfcs/withdrawn/00100-historical.md"),
            "<!-- exo:100 ulid:01historical -->\n\n# RFC 100: Historical\n\n**Stage**: 1\n**Reason**: Replaced.\n\n## Summary\n\nHistorical.\n",
        )
        .unwrap();
        let oid = commit_all(root, "same-number RFCs");
        publish_origin_main(root, &oid);

        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        let result = reconcile_rfcs_with_project(root, None).unwrap();
        assert_eq!(result.inserted, 2);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let mut records = loader
            .load_rfcs()
            .unwrap()
            .into_iter()
            .filter(|record| record.rfc_number == 100)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.text_id.cmp(&right.text_id));
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].text_id, "01current");
        assert_eq!(records[1].text_id, "01historical");

        let effective = load_effective_rfcs(root, None).unwrap();
        assert_eq!(
            effective
                .iter()
                .filter(|record| record.record.rfc_number == 100)
                .count(),
            2
        );
        let error = load_effective_rfc_by_number(root, None, 100).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("00100-current.md"));
        assert!(message.contains("01current"));
        assert!(message.contains("00100-historical.md"));
        assert!(message.contains("01historical"));
    }

    #[test]
    fn canonical_parser_ignores_fenced_metadata_examples() {
        let parsed = parse_rfc_document(
            "docs/rfcs/stage-2/10196-overlays.md",
            "<!-- exo:10196 ulid:01overlay -->\n\n# RFC 10196: Overlays\n\n**Feature**: sidecar\n\n```markdown\n- **Superseded by**: RFC 99999\n```\n\n## Summary\n\nBody.\n",
            None,
        )
        .unwrap();
        assert_eq!(parsed.feature.value.as_deref(), Some("sidecar"));
        assert_eq!(parsed.disk.superseded_by, None);
        assert!(!parsed.disk.superseded_by_declared);
    }

    #[test]
    fn canonical_parser_preserves_section_style_relationships() {
        let parsed = parse_rfc_document(
            "docs/rfcs/stage-2/10196-overlays.md",
            "<!-- exo:10196 ulid:01overlay -->\n\n# RFC 10196: Overlays\n\n## Superseded Documents\n\nThis RFC supersedes:\n\n- RFC 10184\n\n```markdown\nThis RFC supersedes:\n\n- RFC 99999\n```\n",
            None,
        )
        .unwrap();

        assert_eq!(parsed.disk.supersedes.as_deref(), Some("10184"));
        assert!(parsed.disk.supersedes_declared);
    }

    #[test]
    fn canonical_parser_flags_invalid_stage_marker_even_with_legacy_stage_hint() {
        let parsed = parse_rfc_document(
            "docs/rfcs/withdrawn/00129-legacy.md",
            "<!-- exo:129 ulid:01legacy -->\n\n# RFC 129: Legacy\n\n**Status**: Withdrawn (Draft)\n**Stage**: not-a-number\n**Reason**: Retired.\n\n## Summary\n\nHistorical.\n",
            None,
        )
        .unwrap();

        assert_eq!(parsed.disk.stage, 0);
        assert!(parsed.canonical_metadata_conflict);
        assert_eq!(
            retired_rfc_stage_from_document(
                "# RFC 129: Legacy\n\n**Status**: Withdrawn (Draft)\n**Stage**: not-a-number\n",
                None,
                4,
            ),
            4,
            "an invalid declared Stage marker must not fall through to legacy status wording"
        );
    }

    #[test]
    fn legacy_status_stage_parsing_ignores_successor_rfc_numbers() {
        assert_eq!(
            legacy_stage_from_status(
                "**Status**: Withdrawn (superseded by RFC 00003; formerly Stage 1 Proposal)"
            ),
            Some(1)
        );
        assert_eq!(
            legacy_stage_from_status("**Status**: Withdrawn (superseded by RFC 00003)"),
            None
        );
        assert_eq!(
            legacy_stage_from_status("**Status**: Withdrawn (not implemented)"),
            None
        );
        assert_eq!(
            legacy_stage_from_status("**Status**: Withdrawn (unstable experiment)"),
            None
        );
        assert_eq!(
            legacy_stage_from_status("**Status**: Withdrawn (not stable)"),
            None
        );
    }

    #[test]
    fn canonical_metadata_preserves_undeclared_values_and_clears_declared_empty_values() {
        let existing = RfcRecord {
            text_id: "01overlay".to_string(),
            rfc_number: 10196,
            title: "Overlays".to_string(),
            stage: 2,
            status: "active".to_string(),
            feature: Some("sidecar".to_string()),
            slug: "overlays".to_string(),
            file_path: "docs/rfcs/stage-2/10196-overlays.md".to_string(),
            superseded_by: None,
            supersedes: Some("10184".to_string()),
            withdrawal_reason: Some("obsolete".to_string()),
            archived_reason: Some("old archive reason".to_string()),
            consolidated_into: None,
        };
        let inherited = parse_rfc_document(
            "docs/rfcs/stage-2/10196-overlays.md",
            "<!-- exo:10196 ulid:01overlay -->\n\n# RFC 10196: Overlays\n\n## Summary\n\nBody.\n",
            Some(&existing),
        )
        .unwrap();
        let inherited = canonical_rfc_record(&inherited, Some(&existing));
        assert_eq!(inherited.feature.as_deref(), Some("sidecar"));
        assert_eq!(inherited.supersedes.as_deref(), Some("10184"));
        assert_eq!(inherited.withdrawal_reason, None);
        assert_eq!(inherited.archived_reason, None);

        let cleared = parse_rfc_document(
            "docs/rfcs/stage-2/10196-overlays.md",
            "<!-- exo:10196 ulid:01overlay -->\n\n# RFC 10196: Overlays\n\n**Feature**:\n- **Supersedes**: -\n\n## Summary\n\nBody.\n",
            Some(&existing),
        )
        .unwrap();
        let cleared = canonical_rfc_record(&cleared, Some(&existing));
        assert_eq!(cleared.feature, None);
        assert_eq!(cleared.supersedes, None);
    }

    #[test]
    fn test_reconcile_rfcs_syncs_sqlite_with_disk() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join("docs/rfcs/stage-2")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();

        let unchanged_path = root.join("docs/rfcs/stage-0/00001-unchanged.md");
        fs::write(
            &unchanged_path,
            "<!-- exo:1 ulid:01unchanged -->\n\n# RFC 1: Unchanged\n\nBody\n",
        )
        .unwrap();
        writer
            .upsert_rfc(
                "01unchanged",
                1,
                "Unchanged",
                0,
                "active",
                Some("Core"),
                "unchanged",
                "docs/rfcs/stage-0/00001-unchanged.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let updated_path = root.join("docs/rfcs/stage-2/00002-updated.md");
        fs::write(
            &updated_path,
            "<!-- exo:2 ulid:01updated -->\n\n# RFC 2: Updated Title\n\nBody\n",
        )
        .unwrap();
        writer
            .upsert_rfc(
                "01updated",
                2,
                "Old Title",
                1,
                "active",
                Some("Core"),
                "updated",
                "docs/rfcs/stage-1/00002-updated.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let inserted_path = root.join("docs/rfcs/stage-0/00003-inserted.md");
        fs::write(
            &inserted_path,
            "<!-- exo:3 ulid:01inserted -->\n\n# RFC 3: Inserted\n\nBody\n",
        )
        .unwrap();

        writer
            .upsert_rfc(
                "01deleted",
                99,
                "Deleted",
                0,
                "active",
                None,
                "deleted",
                "docs/rfcs/stage-0/00099-deleted.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(
            result,
            ReconcileResult {
                inserted: 0,
                updated: 1,
                deleted: 1,
                unchanged: 2,
            }
        );

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let rows = loader.load_rfcs().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| {
            row.text_id == "01updated"
                && row.title == "Updated Title"
                && row.stage == 2
                && row.status == "active"
                && row.feature.as_deref() == Some("Core")
                && row.file_path == "docs/rfcs/stage-2/00002-updated.md"
        }));
        assert!(!rows.iter().any(|row| row.text_id == "01inserted"));
        assert!(!rows.iter().any(|row| row.text_id == "01deleted"));

        let repairs = detect_rfc_repair_candidates(root).unwrap();
        assert!(
            repairs
                .iter()
                .all(|repair| repair.current_path != "docs/rfcs/stage-2/00002-updated.md"),
            "safe metadata-only drift should not surface as manual repair debt: {repairs:#?}"
        );
        assert!(repairs.iter().any(|repair| {
            repair.current_path == "docs/rfcs/stage-0/00003-inserted.md"
                && repair
                    .reasons
                    .iter()
                    .any(|reason| reason == "metadata_relink")
        }));
    }

    #[test]
    fn test_reconcile_rfcs_syncs_relationship_metadata_from_disk() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-4")).unwrap();
        fs::create_dir_all(root.join("docs/rfcs/stage-3")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-4/00022-unified-project-state.md");
        fs::write(
            &old_path,
            "<!-- exo:22 ulid:01oldstate -->\n\n# RFC 22: Unified Project State\n\n- **Superseded by**: RFC 10176\n\nBody\n",
        )
        .unwrap();
        let new_path = root.join("docs/rfcs/stage-3/10176-project-state-model.md");
        fs::write(
            &new_path,
            "<!-- exo:10176 ulid:01newstate -->\n\n# RFC 10176: Project State Model\n\n- **Supersedes**: RFC 0022\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01oldstate",
                22,
                "Unified Project State",
                4,
                "active",
                None,
                "unified-project-state",
                "docs/rfcs/stage-4/00022-unified-project-state.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        writer
            .upsert_rfc(
                "01newstate",
                10176,
                "Project State Model",
                3,
                "active",
                None,
                "project-state-model",
                "docs/rfcs/stage-3/10176-project-state-model.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(result.updated, 2);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let old_row = loader.load_rfc_by_number(22).unwrap().unwrap();
        let new_row = loader.load_rfc_by_number(10176).unwrap().unwrap();
        assert_eq!(old_row.superseded_by.as_deref(), Some("10176"));
        assert_eq!(new_row.supersedes.as_deref(), Some("0022"));
    }

    #[test]
    fn test_reconcile_rfcs_honors_changed_disk_relationship_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        fs::write(
            root.join("docs/rfcs/stage-1/00001-old-rfc.md"),
            "<!-- exo:1 ulid:01old -->\n\n# RFC 1: Old RFC\n\n- **Superseded by**: RFC 00002\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01old",
                1,
                "Old RFC",
                1,
                "active",
                None,
                "old-rfc",
                "docs/rfcs/stage-1/00001-old-rfc.md",
                Some("00003"),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        reconcile_rfcs(root).unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.superseded_by.as_deref(), Some("00002"));
    }

    #[test]
    fn test_reconcile_rfcs_clears_relationship_metadata_from_empty_disk_marker() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/withdrawn")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        fs::write(
            root.join("docs/rfcs/withdrawn/00107-coherent-workflow-model.md"),
            "<!-- exo:107 ulid:01workflow -->\n\n# RFC 107: Coherent Workflow Model\n\n| **Supersedes** | - |\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01workflow",
                107,
                "Coherent Workflow Model",
                0,
                "withdrawn",
                None,
                "coherent-workflow-model",
                "docs/rfcs/withdrawn/00107-coherent-workflow-model.md",
                None,
                Some("00099"),
                None,
                None,
                None,
            )
            .unwrap();

        reconcile_rfcs(root).unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(107).unwrap().unwrap();
        assert_eq!(row.supersedes, None);
    }

    #[test]
    fn test_reconcile_rfcs_clears_relationship_metadata_removed_from_disk() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        fs::write(
            root.join("docs/rfcs/stage-1/00001-current-rfc.md"),
            "<!-- exo:1 ulid:01current -->\n\n# RFC 1: Current RFC\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01current",
                1,
                "Current RFC",
                1,
                "active",
                None,
                "current-rfc",
                "docs/rfcs/stage-1/00001-current-rfc.md",
                Some("00002"),
                Some("00003"),
                None,
                None,
                None,
            )
            .unwrap();

        reconcile_rfcs(root).unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.superseded_by, None);
        assert_eq!(row.supersedes, None);
    }

    #[test]
    fn test_reconcile_rfcs_keeps_archived_stage_drift_as_repair_debt() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/archive")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let archived_rel = "docs/rfcs/archive/00001-stable.md";
        fs::write(
            root.join(archived_rel),
            "<!-- exo:1 ulid:01stable -->\n\n# RFC 1: Stable\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01stable",
                1,
                "Stable",
                4,
                "active",
                Some("Core"),
                "stable",
                "docs/rfcs/stage-4/00001-stable.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(
            result,
            ReconcileResult {
                inserted: 0,
                updated: 0,
                deleted: 0,
                unchanged: 1,
            }
        );

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader
            .load_rfc_by_number(1)
            .unwrap()
            .expect("expected RFC 1 row");
        assert_eq!(row.stage, 4);
        assert_eq!(row.status, "active");
        assert_eq!(row.file_path, "docs/rfcs/stage-4/00001-stable.md");

        let repairs = detect_rfc_repair_candidates(root).unwrap();
        let repair = repairs
            .iter()
            .find(|repair| repair.current_path == archived_rel)
            .expect("archived path drift should remain repair debt");
        assert!(
            repair
                .reasons
                .iter()
                .any(|reason| reason == "metadata_path_drift"),
            "expected metadata path drift: {repair:#?}"
        );
        assert_eq!(
            repair
                .stored_metadata
                .as_ref()
                .expect("stored metadata")
                .stage,
            4
        );
    }

    #[test]
    fn test_reconcile_rfcs_archives_declared_lifecycle_drift() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/archive")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let archived_rel = "docs/rfcs/archive/00001-stable.md";
        fs::write(
            root.join(archived_rel),
            "<!-- exo:1 ulid:01stable -->\n\n# RFC 1: Stable\n\n- **Status**: Archived (superseded; formerly Stage 4 Stable)\n- **Superseded by**: RFC 00002\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01stable",
                1,
                "Stable",
                4,
                "active",
                Some("Core"),
                "stable",
                "docs/rfcs/stage-4/00001-stable.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(
            result,
            ReconcileResult {
                inserted: 0,
                updated: 1,
                deleted: 0,
                unchanged: 0,
            }
        );

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader
            .load_rfc_by_number(1)
            .unwrap()
            .expect("expected RFC 1 row");
        assert_eq!(row.stage, 4);
        assert_eq!(row.status, "archived");
        assert_eq!(row.file_path, archived_rel);
        assert_eq!(row.superseded_by.as_deref(), Some("00002"));

        let repairs = detect_rfc_repair_candidates(root).unwrap();
        assert!(
            repairs
                .iter()
                .all(|repair| repair.current_path != archived_rel),
            "declared archived lifecycle drift should reconcile, not remain repair debt: {repairs:#?}"
        );
    }

    #[test]
    fn test_reconcile_rfcs_keeps_duplicate_identity_as_repair_debt() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let original_rel = "docs/rfcs/stage-0/00001-duplicate.md";
        let duplicate_rel = "docs/rfcs/stage-1/00001-duplicate.md";
        let content = "<!-- exo:1 ulid:01duplicate -->\n\n# RFC 1: Duplicate\n\nBody\n";
        fs::write(root.join(original_rel), content).unwrap();
        fs::write(root.join(duplicate_rel), content).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01duplicate",
                1,
                "Duplicate",
                0,
                "active",
                Some("Core"),
                "duplicate",
                original_rel,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(
            result,
            ReconcileResult {
                inserted: 0,
                updated: 0,
                deleted: 0,
                unchanged: 2,
            }
        );

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader
            .load_rfc_by_number(1)
            .unwrap()
            .expect("expected RFC 1 row");
        assert_eq!(row.stage, 0);
        assert_eq!(row.file_path, original_rel);

        let repairs = detect_rfc_repair_candidates(root).unwrap();
        let repair = repairs
            .iter()
            .find(|repair| repair.current_path == duplicate_rel)
            .expect("duplicate RFC identity should remain repair debt");
        assert!(
            repair
                .reasons
                .iter()
                .any(|reason| reason == "metadata_path_drift"),
            "expected metadata path drift: {repair:#?}"
        );
        assert_eq!(
            repair
                .stored_metadata
                .as_ref()
                .expect("stored metadata")
                .path,
            original_rel
        );
    }

    #[test]
    fn test_reconcile_rfcs_deletes_metadata_for_evidence_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let evidence_rel = "docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md";
        let evidence_path = root.join(evidence_rel);
        fs::create_dir_all(evidence_path.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(
            &evidence_path,
            "<!-- exo:2026 ulid:01evidence -->\n\n# Live probes\n\nEvidence notes.\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01evidence",
                2026,
                "Live probes",
                0,
                "active",
                None,
                "06-11-live-probes",
                evidence_rel,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = reconcile_rfcs(root).unwrap();
        assert_eq!(
            result,
            ReconcileResult {
                inserted: 0,
                updated: 0,
                deleted: 1,
                unchanged: 0,
            }
        );

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        assert!(
            loader.load_rfc_by_number(2026).unwrap().is_none(),
            "evidence metadata row should be deleted during reconciliation"
        );
        assert!(
            evidence_path.exists(),
            "reconciliation must not delete the evidence note itself"
        );
    }

    #[test]
    fn test_reconcile_rfcs_once_runs_once_per_root_and_db() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();

        fs::write(
            root.join("docs/rfcs/stage-0/00001-first.md"),
            "<!-- exo:1 ulid:01first -->\n\n# RFC 1: First\n\nBody\n",
        )
        .unwrap();

        let first = reconcile_rfcs_once_with_project(root, None).unwrap();
        assert_eq!(first.inserted, 0);
        assert_eq!(first.unchanged, 1);

        fs::write(
            root.join("docs/rfcs/stage-0/00002-second.md"),
            "<!-- exo:2 ulid:01second -->\n\n# RFC 2: Second\n\nBody\n",
        )
        .unwrap();

        let second = reconcile_rfcs_once_with_project(root, None).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.unchanged, 2);

        let unchanged = reconcile_rfcs_once_with_project(root, None).unwrap();
        assert_eq!(unchanged, ReconcileResult::default());

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let rows = loader.load_rfcs().unwrap();
        assert_eq!(rows.len(), 0);

        let explicit = reconcile_rfcs(root).unwrap();
        assert_eq!(explicit.inserted, 0);
        assert_eq!(explicit.unchanged, 2);

        let rows = loader.load_rfcs().unwrap();
        assert_eq!(rows.len(), 0);

        let repairs = detect_rfc_repair_candidates(root).unwrap();
        assert_eq!(repairs.len(), 2);
        assert!(repairs.iter().any(|repair| {
            repair.id == "00001"
                && repair
                    .reasons
                    .iter()
                    .any(|reason| reason == "metadata_relink")
        }));
        assert!(repairs.iter().any(|repair| {
            repair.id == "00002"
                && repair
                    .reasons
                    .iter()
                    .any(|reason| reason == "metadata_relink")
        }));
    }

    #[test]
    fn test_promote_updates_sqlite_stage_and_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let file_path = root.join("docs/rfcs/stage-0/00001-promote-me.md");
        fs::write(
            &file_path,
            "<!-- exo:1 ulid:01promote -->\n\n# RFC 1: Promote Me\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01promote",
                1,
                "Promote Me",
                0,
                "active",
                Some("Core"),
                "promote-me",
                "docs/rfcs/stage-0/00001-promote-me.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        promote(&root.join("docs/rfcs"), "00001").unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.stage, 1);
        assert_eq!(row.file_path, "docs/rfcs/stage-1/00001-promote-me.md");
    }

    #[test]
    fn test_rename_updates_sqlite_slug_and_path_without_rewriting_file() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-0/00001-old-slug.md");
        let original = "<!-- exo:1 ulid:01rename -->\n\n# RFC 1: Renamed RFC\n\nBody\n";
        fs::write(&old_path, original).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01rename",
                1,
                "Renamed RFC",
                0,
                "active",
                Some("Core"),
                "old-slug",
                "docs/rfcs/stage-0/00001-old-slug.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let (_, new_path) = rename(root, "00001").unwrap();
        assert_eq!(
            relative_workspace_path(root, &new_path),
            "docs/rfcs/stage-0/00001-renamed-rfc.md"
        );
        assert_eq!(fs::read_to_string(&new_path).unwrap(), original);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.slug, "renamed-rfc");
        assert_eq!(row.file_path, "docs/rfcs/stage-0/00001-renamed-rfc.md");
        assert_eq!(row.title, "Renamed RFC");
    }

    #[test]
    fn test_edit_body_updates_sqlite_relationship_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let path = root.join("docs/rfcs/stage-1/00001-edit-me.md");
        fs::write(
            &path,
            "<!-- exo:1 ulid:01edit -->\n\n# RFC 1: Edit Me\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01edit",
                1,
                "Edit Me",
                1,
                "active",
                None,
                "edit-me",
                "docs/rfcs/stage-1/00001-edit-me.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        edit(
            root,
            "00001",
            None,
            None,
            None,
            Some("- **Supersedes**: RFC 00002, RFC 00003\n\nBody\n"),
        )
        .unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.supersedes.as_deref(), Some("00002, 00003"));
    }

    #[test]
    fn test_edit_body_clears_removed_relationship_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let path = root.join("docs/rfcs/stage-1/00001-edit-me.md");
        fs::write(
            &path,
            "<!-- exo:1 ulid:01edit -->\n\n# RFC 1: Edit Me\n\n- **Supersedes**: RFC 00002\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01edit",
                1,
                "Edit Me",
                1,
                "active",
                None,
                "edit-me",
                "docs/rfcs/stage-1/00001-edit-me.md",
                Some("00003"),
                Some("00002"),
                None,
                None,
                None,
            )
            .unwrap();

        edit(root, "00001", None, None, None, Some("Body\n")).unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.superseded_by, None);
        assert_eq!(row.supersedes, None);
    }

    #[test]
    fn test_metadata_sync_preserves_existing_rfc_number_when_anchor_drifts() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let path = root.join("docs/rfcs/stage-0/00001-drifted-anchor.md");
        fs::write(
            &path,
            "<!-- exo:4 ulid:01drifted -->\n\n# RFC 4: Drifted Anchor\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01drifted",
                1,
                "Original Identity",
                0,
                "active",
                Some("Core"),
                "drifted-anchor",
                "docs/rfcs/stage-0/00001-drifted-anchor.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        sync_rfc_rename(root, &path).unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader
            .load_rfcs()
            .unwrap()
            .into_iter()
            .find(|row| row.text_id == "01drifted")
            .expect("expected RFC row");
        assert_eq!(row.rfc_number, 1);
        assert_eq!(row.title, "Drifted Anchor");
    }

    #[test]
    fn test_detect_repair_candidate_for_text_id_surfaces_malformed_anchor() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let path = root.join("docs/rfcs/stage-0/00001-legacy-anchor.md");
        fs::write(&path, "<!-- exo:1 -->\n\n# RFC 1: Legacy Anchor\n\nBody\n").unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01legacyanchor",
                1,
                "Legacy Anchor",
                0,
                "active",
                Some("Core"),
                "legacy-anchor",
                "docs/rfcs/stage-0/00001-legacy-anchor.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let repair = detect_rfc_repair_candidate_for_text_id(root, "01legacyanchor")
            .unwrap()
            .expect("expected repair debt");
        assert_eq!(repair.id, "00001");
        assert!(
            repair
                .reasons
                .iter()
                .any(|reason| reason == "missing_anchor_ulid")
        );
    }

    #[test]
    fn test_withdraw_materializes_portable_lifecycle_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-2")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-2/00001-withdraw-me.md");
        let original = "<!-- exo:1 ulid:01withdraw -->\n\n# RFC 1: Withdraw Me\n\nBody\n";
        fs::write(&old_path, original).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01withdraw",
                1,
                "Withdraw Me",
                2,
                "active",
                Some("Core"),
                "withdraw-me",
                "docs/rfcs/stage-2/00001-withdraw-me.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let new_path = withdraw(&root.join("docs/rfcs"), "00001", Some("obsolete")).unwrap();
        let content = fs::read_to_string(&new_path).unwrap();
        assert!(content.contains("- **Status**: Withdrawn"));
        assert!(content.contains("- **Stage**: 2"));
        assert!(content.contains("- **Reason**: obsolete"));
        assert!(content.contains("Body"));

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.status, "withdrawn");
        assert_eq!(row.stage, 2);
        assert_eq!(row.withdrawal_reason.as_deref(), Some("obsolete"));
        assert_eq!(row.file_path, "docs/rfcs/withdrawn/00001-withdraw-me.md");

        let repeated_path = withdraw(&root.join("docs/rfcs"), "00001", None).unwrap();
        assert_eq!(repeated_path, new_path);
        assert_eq!(fs::read_to_string(&repeated_path).unwrap(), content);
        let repeated = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(repeated.stage, 2);
        assert_eq!(repeated.withdrawal_reason.as_deref(), Some("obsolete"));

        let stale_alias_content = content.replace(
            "- **Reason**: obsolete",
            "- **Withdrawal reason**: stale reason\n- **Reason**: obsolete",
        );
        fs::write(&repeated_path, stale_alias_content).unwrap();
        withdraw(&root.join("docs/rfcs"), "00001", Some("updated reason")).unwrap();
        let updated_content = fs::read_to_string(&repeated_path).unwrap();
        assert!(updated_content.contains("- **Reason**: updated reason"));
        assert!(!updated_content.contains("Withdrawal reason"));
        assert!(!updated_content.contains("stale reason"));
        let updated = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(updated.withdrawal_reason.as_deref(), Some("updated reason"));
    }

    #[test]
    fn repeated_withdraw_preserves_stored_stage_for_legacy_retired_rfc() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        fs::create_dir_all(&rfc_dir).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let rfc_path = rfc_dir.join("00001-retired.md");
        fs::write(
            &rfc_path,
            "<!-- exo:1 ulid:01retired -->\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Reason**: Obsolete.\n\nBody\n",
        )
        .unwrap();
        SqliteWriter::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .upsert_rfc(
                "01retired",
                1,
                "Retired",
                4,
                "withdrawn",
                None,
                "retired",
                "docs/rfcs/withdrawn/00001-retired.md",
                None,
                None,
                Some("Obsolete."),
                None,
                None,
            )
            .unwrap();

        withdraw(&root.join("docs/rfcs"), "00001", None).unwrap();

        let content = fs::read_to_string(&rfc_path).unwrap();
        assert!(content.contains("- **Stage**: 4"));
        let row = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(row.stage, 4);
    }

    #[test]
    fn test_archive_materializes_portable_lifecycle_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-3")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-3/00001-archive-me.md");
        let original = "<!-- exo:1 ulid:01archive -->\n\n# RFC 1: Archive Me\n\nBody\n";
        fs::write(&old_path, original).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01archive",
                1,
                "Archive Me",
                3,
                "active",
                Some("Core"),
                "archive-me",
                "docs/rfcs/stage-3/00001-archive-me.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let new_path = archive(
            &root.join("docs/rfcs"),
            "00001",
            Some("shipped and replaced"),
        )
        .unwrap();
        let content = fs::read_to_string(&new_path).unwrap();
        assert!(content.contains("- **Status**: Archived"));
        assert!(content.contains("- **Stage**: 3"));
        assert!(content.contains("- **Reason**: shipped and replaced"));
        assert!(content.contains("Body"));

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.status, "archived");
        assert_eq!(row.stage, 3);
        assert_eq!(row.archived_reason.as_deref(), Some("shipped and replaced"));
        assert_eq!(row.file_path, "docs/rfcs/archive/00001-archive-me.md");

        let repeated_path = archive(&root.join("docs/rfcs"), "00001", None).unwrap();
        assert_eq!(repeated_path, new_path);
        assert_eq!(fs::read_to_string(&repeated_path).unwrap(), content);
        let repeated = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(repeated.stage, 3);
        assert_eq!(
            repeated.archived_reason.as_deref(),
            Some("shipped and replaced")
        );

        let withdrawn_dir = root.join("docs/rfcs/withdrawn");
        fs::create_dir_all(&withdrawn_dir).unwrap();
        let withdrawn_path = withdrawn_dir.join("00001-archive-me.md");
        fs::rename(&repeated_path, &withdrawn_path).unwrap();
        write_rfc_lifecycle_metadata(
            &withdrawn_path,
            "Withdrawn",
            3,
            Some("shipped and replaced"),
        )
        .unwrap();
        sync_rfc_withdrawal(root, &withdrawn_path, 3, Some("shipped and replaced")).unwrap();
        let corrected_path = archive(&root.join("docs/rfcs"), "00001", None).unwrap();
        assert_eq!(corrected_path, new_path);
        let corrected_content = fs::read_to_string(&corrected_path).unwrap();
        assert!(corrected_content.contains("- **Status**: Archived"));
        assert!(corrected_content.contains("- **Stage**: 3"));
        assert!(corrected_content.contains("- **Reason**: shipped and replaced"));
        let corrected = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(corrected.stage, 3);
        assert_eq!(
            corrected.archived_reason.as_deref(),
            Some("shipped and replaced")
        );
    }

    #[test]
    fn lifecycle_metadata_materialization_is_preamble_scoped_and_idempotent() {
        let original = "<!-- exo:1 ulid:01withdraw -->\n\n# RFC 1: Withdraw Me\n\n- **Status**: Active\n\n## Example\n\n```markdown\n- **Status**: Example\n- **Stage**: 4\n- **Reason**: Example only\n```\n";

        let once = materialize_rfc_lifecycle_metadata_content(original, "Withdrawn", 1, None);
        let twice = materialize_rfc_lifecycle_metadata_content(&once, "Withdrawn", 1, None);

        assert_eq!(once, twice);
        assert!(once.contains("- **Status**: Withdrawn"));
        assert!(once.contains("- **Stage**: 1"));
        assert!(once.contains("- **Reason**:\n"));
        assert!(once.contains("- **Status**: Example"));
        assert!(once.contains("- **Stage**: 4"));
        assert!(once.contains("- **Reason**: Example only"));
        assert!(retired_rfc_lifecycle_metadata_is_portable(
            &once,
            "withdrawn"
        ));
    }

    #[test]
    fn lifecycle_metadata_materialization_preserves_leading_anchor_without_h1() {
        let original = "<!-- exo:129 ulid:01runner -->\n\nHistorical body.\n";
        let updated = materialize_rfc_lifecycle_metadata_content(original, "Withdrawn", 1, None);

        assert!(updated.starts_with("<!-- exo:129 ulid:01runner -->\n"));
        assert!(updated.contains("# RFC 129: RFC 129"));
        assert!(updated.contains("- **Status**: Withdrawn\n- **Stage**: 1\n- **Reason**:"));
        assert_eq!(updated.matches("<!-- exo:").count(), 1);
        assert_eq!(
            updated,
            materialize_rfc_lifecycle_metadata_content(&updated, "Withdrawn", 1, None)
        );
    }

    #[test]
    fn lifecycle_metadata_backfill_fills_empty_reason_from_shared_evidence() {
        let original = "<!-- exo:1 ulid:01withdraw -->\n\n# RFC 1: Withdraw Me\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Withdrawal reason**:\n\nBody.\n";

        let updated = backfill_rfc_lifecycle_metadata_content(
            original,
            "Withdrawn",
            1,
            Some("The proposal was replaced."),
        );

        assert!(updated.contains("- **Withdrawal reason**: The proposal was replaced."));
        assert!(!updated.contains("- **Withdrawal reason**:\n"));
        assert_eq!(
            updated,
            backfill_rfc_lifecycle_metadata_content(
                &updated,
                "Withdrawn",
                1,
                Some("The proposal was replaced."),
            )
        );
    }

    #[test]
    fn lifecycle_metadata_backfill_preserves_intentional_empty_reason() {
        let original = "<!-- exo:1 ulid:01archive -->\n\n# RFC 1: Archive Me\n\n- **Status**: Archived\n- **Stage**: 4\n- **Reason**:\n\nBody.\n";

        assert_eq!(
            original,
            backfill_rfc_lifecycle_metadata_content(original, "Archived", 4, None)
        );
    }

    #[test]
    fn lifecycle_metadata_backfill_aligns_empty_reason_aliases() {
        let original = "<!-- exo:1 ulid:01withdraw -->\n\n# RFC 1: Withdraw Me\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Withdrawal reason**:\n- **Reason**: The proposal was replaced.\n\nBody.\n";

        let updated = backfill_rfc_lifecycle_metadata_content(original, "Withdrawn", 1, None);

        assert_eq!(
            retired_rfc_reason_from_document(original, "withdrawn").as_deref(),
            Some("The proposal was replaced.")
        );

        assert!(updated.contains("- **Withdrawal reason**: The proposal was replaced."));
        assert!(updated.contains("- **Reason**: The proposal was replaced."));
    }

    #[test]
    fn test_archive_syncs_existing_markdown_relationship_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-3")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-3/00001-archive-me.md");
        fs::write(
            &old_path,
            "<!-- exo:1 ulid:01archive -->\n\n# RFC 1: Archive Me\n\n- **Supersedes**: RFC 00002\n\nBody\n",
        )
        .unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01archive",
                1,
                "Archive Me",
                3,
                "active",
                Some("Core"),
                "archive-me",
                "docs/rfcs/stage-3/00001-archive-me.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        archive(
            &root.join("docs/rfcs"),
            "00001",
            Some("shipped and replaced"),
        )
        .unwrap();

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.status, "archived");
        assert_eq!(row.supersedes.as_deref(), Some("00002"));
    }

    #[test]
    fn test_supersede_updates_sqlite_and_markdown_relationship_metadata() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        fs::create_dir_all(root.join("docs/rfcs/stage-1")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();

        let old_path = root.join("docs/rfcs/stage-1/00001-old-rfc.md");
        let new_path = root.join("docs/rfcs/stage-1/00002-new-rfc.md");
        let old_original = "<!-- exo:1 ulid:01old -->\n\n# RFC 1: Old RFC\n\n> **Status**: Withdrawn (superseded by RFC 00003; see RFC 00004 for watcher history)\n> **Reason**: Superseded by RFC 00003: Third RFC.\n> **Note**: Superseded by [RFC 00003: Third RFC](../stage-1/00003-third-rfc.md).\nThis RFC has been superseded by RFC 00003.\n\n- **Supersedes**: RFC 00009\n\nBody\n";
        let new_original = "<!-- exo:2 ulid:01new -->\n\n# RFC 2: New RFC\n\n- **Superseded by**: RFC 00004\n- **Supersedes**: RFC 00001\n\nBody\n";
        fs::write(&old_path, old_original).unwrap();
        fs::write(&new_path, new_original).unwrap();

        let writer = SqliteWriter::open(root.join(SQLITE_DB_PATH)).unwrap();
        writer
            .upsert_rfc(
                "01old",
                1,
                "Old RFC",
                1,
                "active",
                Some("Core"),
                "old-rfc",
                "docs/rfcs/stage-1/00001-old-rfc.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        writer
            .upsert_rfc(
                "01new",
                2,
                "New RFC",
                1,
                "active",
                Some("Core"),
                "new-rfc",
                "docs/rfcs/stage-1/00002-new-rfc.md",
                None,
                Some("00003"),
                None,
                None,
                None,
            )
            .unwrap();

        supersede(root, "00001", "00002").unwrap();
        let old_content = fs::read_to_string(&old_path).unwrap();
        assert!(old_content.contains("> **Status**: Withdrawn (superseded by RFC 00002;"));
        assert!(old_content.contains("> **Reason**: Superseded by RFC 00002."));
        assert!(old_content.contains("see RFC 00004 for watcher history"));
        assert!(old_content.contains("> **Note**: Superseded by RFC 00002."));
        assert!(old_content.contains("This RFC has been superseded by RFC 00002."));
        assert!(!old_content.contains("- **Superseded by**: RFC 00002"));
        assert!(!old_content.contains("00003"));
        assert!(!old_content.contains("../stage-1/00003-third-rfc.md"));

        let new_content = fs::read_to_string(&new_path).unwrap();
        assert!(new_content.contains("- **Supersedes**: RFC 00001"));
        assert_eq!(new_content.matches("RFC 00001").count(), 1);
        assert!(!new_content.contains("00003"));

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let old_row = loader.load_rfc_by_number(1).unwrap().unwrap();
        let new_row = loader.load_rfc_by_number(2).unwrap().unwrap();
        assert_eq!(old_row.superseded_by.as_deref(), Some("00002"));
        assert_eq!(old_row.supersedes.as_deref(), Some("00009"));
        assert_eq!(new_row.superseded_by.as_deref(), Some("00004"));
        assert_eq!(new_row.supersedes.as_deref(), Some("00001"));

        reconcile_rfcs(root).unwrap();
        let old_row = loader.load_rfc_by_number(1).unwrap().unwrap();
        let new_row = loader.load_rfc_by_number(2).unwrap().unwrap();
        assert_eq!(old_row.superseded_by.as_deref(), Some("00002"));
        assert_eq!(old_row.supersedes.as_deref(), Some("00009"));
        assert_eq!(new_row.superseded_by.as_deref(), Some("00004"));
        assert_eq!(new_row.supersedes.as_deref(), Some("00001"));
    }

    #[test]
    fn test_relationship_marker_update_preserves_table_rows() {
        let content = "# RFC 1: Table\n\n| Field | Value |\n| --- | --- |\n| **Supersedes** | — |\n| Related | RFC 00003 |\n\nBody\n";

        let updated = upsert_rfc_relationship_marker(content, "Supersedes", "00002");

        assert!(updated.contains("| **Supersedes** | RFC 00002 |"));
        assert!(updated.contains("| Related | RFC 00003 |"));
        assert!(!updated.contains("- **Supersedes**: RFC 00002"));
    }
}
