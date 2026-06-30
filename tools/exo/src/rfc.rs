use anyhow::{Context, Result};
use gray_matter::{Matter, engine::YAML};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex, OnceLock};
use walkdir::WalkDir;

use crate::context::sqlite_loader::RfcRecord;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    root: PathBuf,
    db_path: PathBuf,
}

impl ReconcileKey {
    fn new(root: &Path, project: Option<&Project>) -> Self {
        Self {
            root: normalize_key_path(root),
            db_path: normalize_key_path(&crate::context::db_path(root, project)),
        }
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

    if let Some(frontmatter) = content.strip_prefix("---\n")
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
    for line in content.lines() {
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

#[allow(clippy::missing_errors_doc)]
pub fn reconcile_rfcs_once_with_project(
    root: &Path,
    project: Option<&Project>,
) -> Result<ReconcileResult> {
    let key = ReconcileKey::new(root, project);
    let reconciled_keys = RECONCILED_RFC_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut reconciled_keys = reconciled_keys
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if reconciled_keys.contains(&key) {
        return Ok(ReconcileResult::default());
    }

    let result = with_reconcile_lock(root, project, || reconcile_rfcs_with_project(root, project))?;
    reconciled_keys.insert(key);
    drop(reconciled_keys);
    Ok(result)
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
    let db_path = crate::context::db_path(root, project);
    let loader = SqliteLoader::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;
    let writer = SqliteWriter::open(&db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    let mut existing_by_text_id: HashMap<String, crate::context::sqlite_loader::RfcRecord> = loader
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

    if !stage_dir.exists() {
        std::fs::create_dir_all(&stage_dir)?;
    }

    let number = if let Some(id) = id {
        id.to_string()
    } else {
        get_next_rfc_id(&rfc_dir)?
    };

    let slug = slugify_title(title);
    let filename = format!("{number}-{slug}.md");
    let file_path = stage_dir.join(&filename);

    let rfc_number = number
        .parse::<i64>()
        .with_context(|| format!("RFC IDs must be numeric: {number}"))?;
    let text_id = ulid::Ulid::new().to_string().to_lowercase();
    let body_content = body.unwrap_or("Write your RFC content here.");
    let content = render_anchor_rfc_content(rfc_number, &text_id, title, body_content);

    std::fs::write(&file_path, content)?;

    if let Some(writer) = maybe_open_rfc_writer(root)? {
        let relative_path = relative_workspace_path(root, &file_path);
        writer.upsert_rfc(
            &text_id,
            rfc_number,
            title,
            stage,
            "active",
            Some(feature),
            &slug,
            &relative_path,
            None,
            None,
            None,
            None,
            None,
        )?;
    }

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

    let records_by_text_id: HashMap<String, RfcRecord> = maybe_open_rfc_loader(root)?
        .map(|loader| loader.load_rfcs())
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.text_id.clone(), record))
        .collect();

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

    let records_by_text_id: HashMap<String, RfcRecord> = maybe_open_rfc_loader(root)?
        .map(|loader| loader.load_rfcs())
        .transpose()?
        .unwrap_or_default()
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
    if let Some(loader) = maybe_open_rfc_loader(root)? {
        for record in loader.load_rfcs()? {
            if record.rfc_number == target_number && record.file_path != current_rel {
                anyhow::bail!(
                    "Refusing to renumber RFC to {}: metadata already exists for {} at {}",
                    format_rfc_number(target_number),
                    record.title,
                    record.file_path
                );
            }
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
    let records_by_text_id: HashMap<String, RfcRecord> = maybe_open_rfc_loader(root)?
        .map(|loader| loader.load_rfcs())
        .transpose()?
        .unwrap_or_default()
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

    let Some(loader) = maybe_open_rfc_loader(root)? else {
        return Ok(None);
    };
    let records = loader.load_rfcs()?;
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
    ensure_no_rfc_identity_repair_debt(workspace_root, &file_path)?;
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

    let text_id = extract_anchor_ulid(&file_content).with_context(|| {
        format!(
            "RFC {id} is missing an anchor ULID in {}",
            file_path.display()
        )
    })?;

    // 2. Move File using git mv to preserve history
    // Note: We no longer update content — stage is determined by directory, not frontmatter.
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

    if let Some(writer) = maybe_open_rfc_writer(workspace_root)? {
        let relative_path = relative_workspace_path(workspace_root, &new_path);
        writer.update_rfc_stage(&text_id, new_stage, &relative_path)?;
    }

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
    let original_stage = parse_stage(&file_path);

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

    sync_rfc_withdrawal(workspace_root, &new_path, original_stage, reason)?;

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
    let original_stage = parse_stage(&file_path);

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

    sync_rfc_archive(workspace_root, &new_path, original_stage, reason)?;

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
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("Invalid RFC filename: {}", path.display()))?;
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    if !has_anchor(&content) {
        anyhow::bail!("RFC file missing anchor comment: {}", path.display());
    }

    parse_rfc_number(filename)
        .with_context(|| format!("Could not parse RFC number from {filename}"))?;
    let text_id = extract_anchor_ulid(&content)
        .with_context(|| format!("RFC file has invalid anchor ULID: {}", path.display()))?;
    let rfc_number = extract_anchor_rfc_number(&content)
        .with_context(|| format!("RFC file has invalid anchor number: {}", path.display()))?;
    let title = extract_h1_title(&content).unwrap_or_else(|| format!("RFC {rfc_number}"));
    let relationships = extract_rfc_relationships(&content);
    let status = parse_status(path).to_string();
    let lifecycle_status_declared =
        declared_lifecycle_status(&content).is_some_and(|declared| declared == status);
    let relative_path = relative_workspace_path(root, path);

    Ok(DiskRfcRecord {
        text_id,
        rfc_number,
        title,
        stage: parse_stage(path),
        status,
        lifecycle_status_declared,
        slug: parse_slug(filename),
        file_path: relative_path,
        superseded_by: relationships.superseded_by,
        supersedes: relationships.supersedes,
        superseded_by_declared: relationships.superseded_by_declared,
        supersedes_declared: relationships.supersedes_declared,
    })
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

fn maybe_open_rfc_writer(root: &Path) -> Result<Option<SqliteWriter>> {
    let project = Project::resolve(root).ok();
    let db_path = crate::context::db_path(root, project.as_ref());
    if !db_path.exists() {
        if root.join("exosuit.toml").exists() {
            let _ = crate::context::AgentContext::load(root.to_path_buf())?;
        } else {
            return Ok(None);
        }
    }

    if !db_path.exists() {
        return Ok(None);
    }

    Ok(Some(SqliteWriter::open(&db_path)?))
}

fn maybe_open_rfc_loader(root: &Path) -> Result<Option<SqliteLoader>> {
    let project = Project::resolve(root).ok();
    let db_path = crate::context::db_path(root, project.as_ref());
    if !db_path.exists() {
        if root.join("exosuit.toml").exists() {
            let _ = crate::context::AgentContext::load(root.to_path_buf())?;
        } else {
            return Ok(None);
        }
    }

    if !db_path.exists() {
        return Ok(None);
    }

    Ok(Some(SqliteLoader::open(&db_path)?))
}

fn load_rfc_record(root: &Path, rfc_number: i64) -> Result<Option<RfcRecord>> {
    let Some(loader) = maybe_open_rfc_loader(root)? else {
        return Ok(None);
    };
    loader.load_rfc_by_number(rfc_number)
}

fn load_rfc_record_by_text_id(root: &Path, text_id: &str) -> Result<Option<RfcRecord>> {
    let Some(loader) = maybe_open_rfc_loader(root)? else {
        return Ok(None);
    };
    Ok(loader
        .load_rfcs()?
        .into_iter()
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
    let Some(writer) = maybe_open_rfc_writer(root)? else {
        return Ok(());
    };

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
    )
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
    record.stage = record.stage.max(stage);
    record.status = "withdrawn".to_string();
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
    record.withdrawal_reason = reason.map(std::string::ToString::to_string);

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
    record.stage = record.stage.max(stage);
    record.status = "archived".to_string();
    record.slug = parsed.slug;
    record.file_path = parsed.file_path;
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
        assert_eq!(row.stage, 0);
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
        assert_eq!(second, ReconcileResult::default());

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
    fn test_withdraw_updates_sqlite_without_modifying_file_content() {
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
        assert_eq!(fs::read_to_string(&new_path).unwrap(), original);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.status, "withdrawn");
        assert_eq!(row.stage, 2);
        assert_eq!(row.withdrawal_reason.as_deref(), Some("obsolete"));
        assert_eq!(row.file_path, "docs/rfcs/withdrawn/00001-withdraw-me.md");
    }

    #[test]
    fn test_archive_updates_sqlite_without_modifying_file_content() {
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
        assert_eq!(fs::read_to_string(&new_path).unwrap(), original);

        let loader = SqliteLoader::open(root.join(SQLITE_DB_PATH)).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.status, "archived");
        assert_eq!(row.stage, 3);
        assert_eq!(row.archived_reason.as_deref(), Some("shipped and replaced"));
        assert_eq!(row.file_path, "docs/rfcs/archive/00001-archive-me.md");
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
