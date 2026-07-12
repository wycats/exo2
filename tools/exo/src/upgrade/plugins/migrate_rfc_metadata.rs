//! Plugin to migrate RFC metadata from YAML frontmatter to `SQLite`.
//!
//! For each RFC file in `docs/rfcs/`:
//! 1. Extracts metadata from YAML frontmatter
//! 2. Inserts/updates a row in `rfcs_data`
//! 3. Adds an HTML comment anchor with the ULID
//! 4. Strips the YAML frontmatter from the file

use crate::ExoResult;
use crate::context::{AgentContext, SqliteWriter};
use crate::rfc::{
    backfill_rfc_lifecycle_metadata_content, extract_anchor_ulid, extract_h1_title,
    extract_rfc_relationships, has_anchor, parse_rfc_number, parse_slug, parse_stage, parse_status,
    retired_rfc_lifecycle_metadata_is_portable, retired_rfc_reason_from_document,
    retired_rfc_stage_from_document, strip_frontmatter,
};
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const RFCS_DIR: &str = "docs/rfcs";
const SKIP_FILES: &[&str] = &["README.md", "0000-template.md"];

/// Known duplicate ULID: RFC 0148 has two files sharing the same legacy ulid.
/// The stage-2 version keeps it; the withdrawn copy gets a fresh one.
const DUPLICATE_ULID_WITHDRAWN: &str = "0148-implicit-walkthrough-via-task-logs-stage1.md";

#[derive(Debug, Clone, Copy)]
pub struct MigrateRfcMetadataPlugin;

/// Parse YAML frontmatter into a `serde_json::Value` using `gray_matter`.
fn parse_frontmatter(content: &str) -> serde_json::Value {
    use gray_matter::{Matter, engine::YAML};
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content);
    match parsed.data {
        Some(pod) => {
            // gray_matter's Pod can deserialize into serde_json::Value
            pod.deserialize().unwrap_or(serde_json::Value::Null)
        }
        None => serde_json::Value::Null,
    }
}

/// Get a string field from parsed frontmatter.
fn fm_string(data: &serde_json::Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        // Handle numeric values that YAML might parse as numbers
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Get a string field, trying multiple keys (for normalization).
fn fm_string_any(data: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| fm_string(data, k))
}

fn canonical_ulid(value: &str) -> Option<String> {
    ulid::Ulid::from_string(value)
        .ok()
        .map(|id| id.to_string().to_ascii_lowercase())
}

fn insert_missing_relationship_metadata_lines(
    body_content: &str,
    superseded_by: Option<&str>,
    supersedes: Option<&str>,
) -> String {
    let body_relationships = extract_rfc_relationships(body_content);
    let mut relationship_lines = Vec::new();
    if body_relationships.superseded_by.is_none()
        && let Some(value) = superseded_by
    {
        relationship_lines.push(format!(
            "- **Superseded by**: {}",
            format_rfc_relationship_refs(value)
        ));
    }
    if body_relationships.supersedes.is_none()
        && let Some(value) = supersedes
    {
        relationship_lines.push(format!(
            "- **Supersedes**: {}",
            format_rfc_relationship_refs(value)
        ));
    }
    if relationship_lines.is_empty() {
        return body_content.to_string();
    }

    let mut rendered = String::new();
    let mut inserted = false;
    for line in body_content.lines() {
        rendered.push_str(line);
        rendered.push('\n');
        if !inserted && line.starts_with("# RFC ") {
            rendered.push('\n');
            rendered.push_str(&relationship_lines.join("\n"));
            rendered.push_str("\n\n");
            inserted = true;
        }
    }

    if !inserted
        && let Some((anchor, body)) = body_content.split_once('\n')
        && anchor.trim_start().starts_with("<!-- exo:")
    {
        return format!(
            "{anchor}\n\n{}\n\n{}",
            relationship_lines.join("\n"),
            body.trim_start_matches('\n')
        );
    }

    if !inserted {
        return format!(
            "{}\n\n{}",
            relationship_lines.join("\n"),
            body_content.trim_start_matches('\n')
        );
    }

    rendered
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

// ─── File discovery ──────────────────────────────────────────────────

fn find_rfc_files(root: &Path) -> Vec<PathBuf> {
    let rfcs_root = root.join(RFCS_DIR);
    if !rfcs_root.exists() {
        return Vec::new();
    }

    WalkDir::new(&rfcs_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .filter(|e| crate::rfc::is_rfc_document_path(&rfcs_root, e.path()))
        .filter(|e| {
            !matches!(
                e.file_name().to_str(),
                Some(name) if SKIP_FILES.contains(&name)
            )
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn historical_retired_stages(root: &Path) -> HashMap<String, u8> {
    let retained_history_ref = "refs/remotes/private-history/HEAD";
    let history_refs = [retained_history_ref, "HEAD"]
        .into_iter()
        .filter(|history_ref| {
            Command::new("git")
                .args(["rev-parse", "--verify", "--quiet", history_ref])
                .current_dir(root)
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .collect::<Vec<_>>();
    if history_refs.is_empty() {
        return HashMap::new();
    }

    let mut command = Command::new("git");
    command.arg("log").args(history_refs).args([
        "--find-renames",
        "--name-status",
        "--format=",
        "--diff-filter=R",
        "--",
        RFCS_DIR,
    ]);
    let Ok(output) = command.current_dir(root).output() else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let mut stages = HashMap::new();
    let mut retired_renames = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split('\t');
        let Some(change) = fields.next() else {
            continue;
        };
        let Some(source) = fields.next() else {
            continue;
        };
        let Some(destination) = fields.next() else {
            continue;
        };
        let source = source.replace('\\', "/");
        let destination = destination.replace('\\', "/");
        if !change.starts_with('R')
            || !(destination.starts_with("docs/rfcs/withdrawn/")
                || destination.starts_with("docs/rfcs/archive/"))
        {
            continue;
        }
        if let Some(stage) = source
            .strip_prefix("docs/rfcs/stage-")
            .and_then(|path| path.split('/').next())
            .and_then(|stage| stage.parse::<u8>().ok())
            .filter(|stage| *stage <= 4)
        {
            stages.entry(destination).or_insert(stage);
        } else if source.starts_with("docs/rfcs/withdrawn/")
            || source.starts_with("docs/rfcs/archive/")
        {
            retired_renames.push((source, destination));
        }
    }

    for _ in 0..retired_renames.len() {
        let mut changed = false;
        for (source, destination) in &retired_renames {
            if let Some(stage) = stages.get(source).copied()
                && !stages.contains_key(destination)
            {
                stages.insert(destination.clone(), stage);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    stages
}

fn historical_stage_for_path(stages: &HashMap<String, u8>, path: &str) -> Option<u8> {
    stages.get(&path.replace('\\', "/")).copied()
}

fn document_matches_canonical(
    root: &Path,
    canonical_oid: Option<&str>,
    relative_path: &str,
) -> bool {
    let in_worktree = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !in_worktree {
        return true;
    }

    let Some(canonical_oid) = canonical_oid else {
        return false;
    };
    let canonical_path = format!("{canonical_oid}:{relative_path}");
    let tracked = Command::new("git")
        .args(["cat-file", "-e", &canonical_path])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !tracked {
        return false;
    }

    Command::new("git")
        .args(["diff", "--quiet", canonical_oid, "--", relative_path])
        .current_dir(root)
        .status()
        .is_ok_and(|status| status.success())
}

// ─── Plugin implementation ───────────────────────────────────────────

impl UpgradePlugin for MigrateRfcMetadataPlugin {
    fn id(&self) -> &str {
        "migrate-rfc-metadata-v1"
    }

    fn description(&self) -> &str {
        "Migrates RFC metadata from YAML frontmatter to SQLite"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn is_needed(&self, context: &AgentContext) -> ExoResult<UpgradeStatus> {
        let files = find_rfc_files(&context.root);
        let canonical_oid = crate::rfc::canonical_rfc_commit_oid(&context.root)?;
        let unanchored = files
            .iter()
            .filter(|path| std::fs::read_to_string(path).is_ok_and(|content| !has_anchor(&content)))
            .count();

        // Also check if DB rows are missing
        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let db_missing = if db_path.exists() {
            let loader = crate::context::SqliteLoader::open(&db_path)?;
            let rows = loader.load_rfcs()?;
            files.len().saturating_sub(rows.len())
        } else {
            files.len()
        };

        let db_relationships_missing_from_disk = if db_path.exists() {
            let loader = crate::context::SqliteLoader::open(&db_path)?;
            let rows = loader
                .load_rfcs()?
                .into_iter()
                .map(|record| (record.text_id.clone(), record))
                .collect::<HashMap<_, _>>();
            files
                .iter()
                .filter_map(|path| {
                    let content = std::fs::read_to_string(path).ok()?;
                    let ulid = extract_anchor_ulid(&content)?;
                    let record = rows.get(&ulid)?;
                    let relationships = extract_rfc_relationships(&content);
                    let missing_superseded_by =
                        record.superseded_by.is_some() && !relationships.superseded_by_declared;
                    let missing_supersedes =
                        record.supersedes.is_some() && !relationships.supersedes_declared;
                    (missing_superseded_by || missing_supersedes).then_some(())
                })
                .count()
        } else {
            0
        };

        let db_lifecycle_missing_from_disk = if db_path.exists() {
            let rows = crate::context::SqliteLoader::open(&db_path)?
                .load_rfcs()?
                .into_iter()
                .map(|record| (record.text_id.clone(), record))
                .collect::<HashMap<_, _>>();
            files
                .iter()
                .filter_map(|path| {
                    let content = std::fs::read_to_string(path).ok()?;
                    let ulid = extract_anchor_ulid(&content)?;
                    let record = rows.get(&ulid)?;
                    let status = parse_status(path);
                    let portable_status = match status {
                        "withdrawn" => "Withdrawn",
                        "archived" => "Archived",
                        _ => return None,
                    };
                    let reason = match status {
                        "withdrawn" => record.withdrawal_reason.as_deref(),
                        "archived" => record.archived_reason.as_deref(),
                        _ => None,
                    };
                    let disk_reason = retired_rfc_reason_from_document(&content, status);
                    let db_missing_disk_reason = disk_reason
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty())
                        && reason.is_none()
                        && path
                            .strip_prefix(&context.root)
                            .ok()
                            .and_then(Path::to_str)
                            .is_some_and(|relative| {
                                document_matches_canonical(
                                    &context.root,
                                    canonical_oid.as_deref(),
                                    relative,
                                )
                            });
                    (!retired_rfc_lifecycle_metadata_is_portable(&content, status)
                        || db_missing_disk_reason
                        || backfill_rfc_lifecycle_metadata_content(
                            &content,
                            portable_status,
                            record.stage,
                            reason,
                        ) != content)
                        .then_some(())
                })
                .count()
        } else {
            0
        };

        let total_needed = unanchored
            .max(db_missing)
            .max(db_relationships_missing_from_disk)
            .max(db_lifecycle_missing_from_disk);

        if total_needed == 0 {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::warning(format!(
                "{total_needed} RFC(s) need metadata migration ({unanchored} unanchored, {db_missing} missing from DB, {db_relationships_missing_from_disk} DB relationship(s) missing from disk, {db_lifecycle_missing_from_disk} retired lifecycle record(s) missing from disk)"
            )))
        }
    }

    fn apply(&self, context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        let files = find_rfc_files(&context.root);
        if files.is_empty() {
            return Ok(UpgradeReport::no_changes(self.id()));
        }

        let db_path = crate::context::db_path(&context.root, context.project.as_ref());
        let db_existed = db_path.exists();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create SQLite database directory {}",
                    parent.display()
                )
            })?;
        }
        let existing_rfcs = if db_existed {
            crate::context::SqliteLoader::open(&db_path)?
                .load_rfcs()?
                .into_iter()
                .map(|record| (record.text_id.clone(), record))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        let writer = SqliteWriter::open(&db_path)?;
        let historical_stages = historical_retired_stages(&context.root);
        let canonical_oid = crate::rfc::canonical_rfc_commit_oid(&context.root)?;
        let mut changes = Vec::new();
        let mut seen_ulids: HashSet<String> = HashSet::new();
        let mut materialized_anchor = false;

        for path in &files {
            let file_content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    changes.push(format!("SKIP {}: read error: {e}", path.display()));
                    continue;
                }
            };

            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown");

            let rfc_number = match parse_rfc_number(filename) {
                Some(n) => n,
                None => {
                    changes.push(format!("SKIP {filename}: cannot parse RFC number"));
                    continue;
                }
            };

            let slug = parse_slug(filename);
            let already_anchored = has_anchor(&file_content);
            let stage = parse_stage(path);
            let status = parse_status(path);
            let rel_path = path
                .strip_prefix(&context.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let document_matched_head =
                document_matches_canonical(&context.root, canonical_oid.as_deref(), &rel_path);
            let frontmatter = (!already_anchored).then(|| parse_frontmatter(&file_content));
            let frontmatter_stage = frontmatter
                .as_ref()
                .and_then(|data| fm_string(data, "stage"))
                .and_then(|stage| stage.parse::<u8>().ok())
                .filter(|stage| *stage <= 4);

            // Parse metadata: from anchor (if already migrated) or from frontmatter
            let (
                ulid,
                title,
                mut feature,
                mut superseded_by,
                mut supersedes,
                mut withdrawal_reason,
                mut archived_reason,
                mut consolidated_into,
            );

            if already_anchored {
                // File already migrated — extract ULID from anchor, title from H1
                ulid = extract_anchor_ulid(&file_content).map_or_else(
                    || ulid::Ulid::new().to_string().to_lowercase(),
                    |id| id.to_ascii_lowercase(),
                );
                title =
                    extract_h1_title(&file_content).unwrap_or_else(|| format!("RFC {rfc_number}"));
                let existing = existing_rfcs.get(&ulid);
                let relationships = extract_rfc_relationships(&file_content);
                feature = existing.and_then(|record| record.feature.clone());
                superseded_by = relationships
                    .superseded_by
                    .or_else(|| existing.and_then(|record| record.superseded_by.clone()));
                supersedes = relationships
                    .supersedes
                    .or_else(|| existing.and_then(|record| record.supersedes.clone()));
                let materialized_content = insert_missing_relationship_metadata_lines(
                    &file_content,
                    superseded_by.as_deref(),
                    supersedes.as_deref(),
                );
                if materialized_content != file_content {
                    std::fs::write(path, materialized_content.as_bytes())
                        .with_context(|| format!("Failed to rewrite {}", path.display()))?;
                    changes.push(format!(
                        "MATERIALIZED {filename}: relationship metadata from SQLite"
                    ));
                }
                let disk_reason = document_matched_head
                    .then(|| retired_rfc_reason_from_document(&file_content, status))
                    .flatten();
                withdrawal_reason = (status == "withdrawn")
                    .then(|| disk_reason.clone())
                    .flatten()
                    .or_else(|| existing.and_then(|record| record.withdrawal_reason.clone()));
                archived_reason = (status == "archived")
                    .then_some(disk_reason)
                    .flatten()
                    .or_else(|| existing.and_then(|record| record.archived_reason.clone()));
                consolidated_into = existing.and_then(|record| record.consolidated_into.clone());
            } else {
                // Parse YAML frontmatter for metadata
                let fm_data = frontmatter.as_ref().expect("unanchored RFC frontmatter");
                let fm_title = fm_string(fm_data, "title");
                let fm_ulid = fm_string(fm_data, "ulid").and_then(|value| canonical_ulid(&value));
                feature = fm_string(fm_data, "feature");
                superseded_by = fm_string(fm_data, "superseded_by")
                    .or_else(|| fm_string(fm_data, "superseded-by"));
                supersedes = fm_string(fm_data, "supersedes");
                let relationships = extract_rfc_relationships(&file_content);
                superseded_by = superseded_by.or(relationships.superseded_by);
                supersedes = supersedes.or(relationships.supersedes);
                withdrawal_reason =
                    fm_string_any(fm_data, &["withdrawal_reason", "withdrawn_reason"]);
                archived_reason = fm_string(fm_data, "archived_reason");
                consolidated_into = fm_string(fm_data, "consolidated_into");

                // Resolve ULID
                ulid = if let Some(ref existing) = fm_ulid {
                    let is_duplicate_withdrawn = filename == DUPLICATE_ULID_WITHDRAWN;
                    if is_duplicate_withdrawn || seen_ulids.contains(existing) {
                        let fresh = ulid::Ulid::new().to_string().to_lowercase();
                        changes.push(format!(
                            "REISSUE {filename}: duplicate ULID, assigned {fresh}"
                        ));
                        fresh
                    } else {
                        existing.clone()
                    }
                } else {
                    ulid::Ulid::new().to_string().to_lowercase()
                };

                let existing = existing_rfcs.get(&ulid);
                feature = feature.or_else(|| existing.and_then(|record| record.feature.clone()));
                superseded_by = superseded_by
                    .or_else(|| existing.and_then(|record| record.superseded_by.clone()));
                supersedes =
                    supersedes.or_else(|| existing.and_then(|record| record.supersedes.clone()));
                withdrawal_reason = withdrawal_reason
                    .or_else(|| existing.and_then(|record| record.withdrawal_reason.clone()));
                archived_reason = archived_reason
                    .or_else(|| existing.and_then(|record| record.archived_reason.clone()));
                consolidated_into = consolidated_into
                    .or_else(|| existing.and_then(|record| record.consolidated_into.clone()));

                // Resolve title
                let body_content = insert_missing_relationship_metadata_lines(
                    &strip_frontmatter(&file_content),
                    superseded_by.as_deref(),
                    supersedes.as_deref(),
                );
                title = fm_title
                    .or_else(|| extract_h1_title(&body_content))
                    .unwrap_or_else(|| format!("RFC {rfc_number}"));

                // Build and write the new file content
                let anchor = format!("<!-- exo:{rfc_number} ulid:{ulid} -->");
                let has_canonical_h1 = {
                    let pattern = format!("# RFC {rfc_number}:");
                    body_content.contains(&pattern)
                };

                let new_content = if has_canonical_h1 {
                    format!("{anchor}\n\n{body_content}")
                } else {
                    format!("{anchor}\n\n# RFC {rfc_number}: {title}\n\n{body_content}")
                };

                std::fs::write(path, new_content.as_bytes())
                    .with_context(|| format!("Failed to rewrite {}", path.display()))?;
                materialized_anchor = true;
            }
            seen_ulids.insert(ulid.clone());

            let current_content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            if document_matched_head {
                match status {
                    "withdrawn" if withdrawal_reason.is_none() => {
                        withdrawal_reason =
                            retired_rfc_reason_from_document(&current_content, status);
                    }
                    "archived" if archived_reason.is_none() => {
                        archived_reason =
                            retired_rfc_reason_from_document(&current_content, status);
                    }
                    _ => {}
                }
            }
            let portable_stage = if matches!(status, "withdrawn" | "archived") {
                retired_rfc_stage_from_document(
                    &current_content,
                    historical_stage_for_path(&historical_stages, &rel_path).or(frontmatter_stage),
                    existing_rfcs
                        .get(&ulid)
                        .map_or(stage, |record| record.stage),
                )
            } else {
                stage
            };
            let lifecycle_reason = match status {
                "withdrawn" => withdrawal_reason.as_deref(),
                "archived" => archived_reason.as_deref(),
                _ => None,
            };
            let materialized_content = backfill_rfc_lifecycle_metadata_content(
                &current_content,
                match status {
                    "withdrawn" => "Withdrawn",
                    "archived" => "Archived",
                    _ => status,
                },
                portable_stage,
                lifecycle_reason,
            );
            if matches!(status, "withdrawn" | "archived") && materialized_content != current_content
            {
                std::fs::write(path, materialized_content.as_bytes())
                    .with_context(|| format!("Failed to rewrite {}", path.display()))?;
                changes.push(format!(
                    "MATERIALIZED {filename}: lifecycle metadata from SQLite"
                ));
            }

            // Upsert into SQLite
            writer.upsert_rfc(
                &ulid,
                rfc_number,
                &title,
                portable_stage,
                status,
                feature.as_deref(),
                &slug,
                &rel_path,
                superseded_by.as_deref(),
                supersedes.as_deref(),
                withdrawal_reason.as_deref(),
                archived_reason.as_deref(),
                consolidated_into.as_deref(),
            )
            .with_context(|| format!("Failed to upsert RFC {rfc_number} ({filename}), stage={stage}, status={status}"))?;

            changes.push(format!("MIGRATED {filename} → ulid:{ulid}"));
        }

        drop(writer);
        // Observe newly written anchors before a canonical reconciliation can
        // compare them with the pre-migration HEAD. Once the migrated document
        // tree reaches the canonical ref, ordinary reconciliation advances the
        // shared rows from that tree.
        crate::rfc::load_effective_rfc_view(&context.root, context.project.as_ref())?;
        if !materialized_anchor {
            crate::rfc::reconcile_rfcs_with_project(&context.root, context.project.as_ref())?;
        }

        if changes.is_empty() {
            Ok(UpgradeReport::no_changes(self.id()))
        } else {
            Ok(UpgradeReport::with_changes(self.id(), changes))
        }
    }

    fn verify(&self, context: &AgentContext) -> ExoResult<()> {
        let files = find_rfc_files(&context.root);
        let mut errors = Vec::new();

        for path in &files {
            let file_content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: read error: {e}", path.display()));
                    continue;
                }
            };

            if !has_anchor(&file_content) {
                let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
                errors.push(format!("{name}: missing anchor comment"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "RFC metadata migration verification failed:\n  {}",
                errors.join("\n  ")
            )
        }
    }
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{AgentContext, SQLITE_DB_PATH, SqliteLoader, SqliteWriter};

    #[test]
    fn untracked_document_does_not_match_head() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("README.md"), "canonical\n").unwrap();
        for args in [vec!["add", "README.md"], vec!["commit", "-qm", "initial"]] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let relative_path = "docs/rfcs/withdrawn/00001-local.md";
        let path = root.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "# RFC 1: Local\n").unwrap();

        let canonical_oid = crate::rfc::canonical_rfc_commit_oid(root).unwrap();
        assert!(!document_matches_canonical(
            root,
            canonical_oid.as_deref(),
            relative_path
        ));
    }

    #[test]
    fn feature_branch_document_does_not_match_canonical_ref() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let relative_path = "docs/rfcs/withdrawn/00001-retired.md";
        let path = root.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# RFC 1: Retired\n\n- **Reason**: Canonical.\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "canonical RFC"],
            vec!["update-ref", "refs/remotes/origin/HEAD", "HEAD"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(&path, "# RFC 1: Retired\n\n- **Reason**: Branch-only.\n").unwrap();
        assert!(
            Command::new("git")
                .args(["commit", "-qam", "branch RFC"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );

        let canonical_oid = crate::rfc::canonical_rfc_commit_oid(root).unwrap();
        assert!(!document_matches_canonical(
            root,
            canonical_oid.as_deref(),
            relative_path
        ));
    }

    #[test]
    fn anchored_rfc_preserves_db_only_metadata_when_another_rfc_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/stage-1");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        std::fs::write(
            rfc_dir.join("10184-project-workspace-worktree-unbundling-the-conflated-root.md"),
            format!(
                "<!-- exo:10184 ulid:{anchored_ulid} -->\n\n# RFC 10184: Project / Workspace / Worktree\n"
            ),
        )
        .unwrap();
        std::fs::write(
            rfc_dir.join("10185-missing-row.md"),
            "---\ntitle: Missing Row\nulid: 01kq1111111111111111111111\n---\n\n# RFC 10185: Missing Row\n",
        )
        .unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let writer = SqliteWriter::open(&db_path).unwrap();
        writer
            .upsert_rfc(
                anchored_ulid,
                10184,
                "Project / Workspace / Worktree",
                1,
                "active",
                Some("workspace-model"),
                "project-workspace-worktree-unbundling-the-conflated-root",
                "docs/rfcs/stage-1/10184-project-workspace-worktree-unbundling-the-conflated-root.md",
                None,
                Some("10177"),
                None,
                None,
                None,
            )
            .unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let loader = SqliteLoader::open(&db_path).unwrap();
        let rfc = loader.load_rfc_by_number(10184).unwrap().unwrap();
        assert_eq!(rfc.feature.as_deref(), Some("workspace-model"));
        assert_eq!(rfc.supersedes.as_deref(), Some("10177"));

        let content = std::fs::read_to_string(
            rfc_dir.join("10184-project-workspace-worktree-unbundling-the-conflated-root.md"),
        )
        .unwrap();
        assert!(content.contains("- **Supersedes**: RFC 10177"));
    }

    #[test]
    fn is_needed_detects_db_only_relationships_missing_from_disk() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/stage-1");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        std::fs::write(
            rfc_dir.join("10184-project-workspace-worktree-unbundling-the-conflated-root.md"),
            format!(
                "<!-- exo:10184 ulid:{anchored_ulid} -->\n\n# RFC 10184: Project / Workspace / Worktree\n"
            ),
        )
        .unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let writer = SqliteWriter::open(&db_path).unwrap();
        writer
            .upsert_rfc(
                anchored_ulid,
                10184,
                "Project / Workspace / Worktree",
                1,
                "active",
                Some("workspace-model"),
                "project-workspace-worktree-unbundling-the-conflated-root",
                "docs/rfcs/stage-1/10184-project-workspace-worktree-unbundling-the-conflated-root.md",
                None,
                Some("10177"),
                None,
                None,
                None,
            )
            .unwrap();

        let context = AgentContext::new_for_testing(root.to_path_buf());
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::Needed { .. }
        ));
    }

    #[test]
    fn is_needed_detects_db_reason_missing_from_portable_lifecycle_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        std::fs::write(
            rfc_dir.join("00001-retired.md"),
            format!(
                "<!-- exo:1 ulid:{anchored_ulid} -->\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Reason**:\n\nBody.\n"
            ),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "canonical retired RFC"],
            vec!["update-ref", "refs/remotes/origin/HEAD", "HEAD"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path)
            .unwrap()
            .upsert_rfc(
                anchored_ulid,
                1,
                "Retired",
                1,
                "withdrawn",
                None,
                "retired",
                "docs/rfcs/withdrawn/00001-retired.md",
                None,
                None,
                Some("Substantive database reason."),
                None,
                None,
            )
            .unwrap();

        let context = AgentContext::new_for_testing(root.to_path_buf());
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::Needed { .. }
        ));
    }

    #[test]
    fn migration_imports_portable_disk_reason_missing_from_database() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        std::fs::write(
            rfc_dir.join("00001-retired.md"),
            format!(
                "<!-- exo:1 ulid:{anchored_ulid} -->\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Reason**: Portable disk reason.\n\nBody.\n"
            ),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "canonical retired RFC"],
            vec!["update-ref", "refs/remotes/origin/HEAD", "HEAD"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path)
            .unwrap()
            .upsert_rfc(
                anchored_ulid,
                1,
                "Retired",
                1,
                "withdrawn",
                None,
                "retired",
                "docs/rfcs/withdrawn/00001-retired.md",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::Needed { .. }
        ));
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let row = SqliteLoader::open(&db_path)
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(
            row.withdrawal_reason.as_deref(),
            Some("Portable disk reason.")
        );
    }

    #[test]
    fn migration_keeps_dirty_workspace_reason_out_of_shared_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        let rfc_path = rfc_dir.join("00001-retired.md");
        std::fs::write(
            &rfc_path,
            format!(
                "<!-- exo:1 ulid:{anchored_ulid} -->\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Reason**: Canonical reason.\n\nBody.\n"
            ),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "canonical RFC"],
            vec!["update-ref", "refs/remotes/private-history/HEAD", "HEAD"],
            vec!["update-ref", "refs/remotes/upstream/HEAD", "HEAD"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(
            &rfc_path,
            format!(
                "<!-- exo:1 ulid:{anchored_ulid} -->\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Stage**: 1\n- **Reason**: Dirty workspace reason.\n\nBody.\n"
            ),
        )
        .unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path)
            .unwrap()
            .upsert_rfc(
                anchored_ulid,
                1,
                "Retired",
                1,
                "withdrawn",
                None,
                "retired",
                "docs/rfcs/withdrawn/00001-retired.md",
                None,
                None,
                Some("Canonical reason."),
                None,
                None,
            )
            .unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let row = SqliteLoader::open(&db_path)
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(row.withdrawal_reason.as_deref(), Some("Canonical reason."));
        let effective = crate::rfc::load_effective_rfc_by_number(root, None, 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            effective.record.withdrawal_reason.as_deref(),
            Some("Dirty workspace reason.")
        );
    }

    #[test]
    fn unanchored_migration_persists_body_only_retired_reason() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        std::fs::write(
            rfc_dir.join("00001-retired.md"),
            "---\ntitle: Retired\nstage: 2\nulid: 01kq09x2d9y6gc9eg27jatcgvf\n---\n\n# RFC 1: Retired\n\n- **Status**: Withdrawn\n- **Reason**: Body-only reason.\n\nBody.\n",
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "legacy retired RFC"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let row = SqliteLoader::open(root.join(SQLITE_DB_PATH))
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(row.withdrawal_reason.as_deref(), Some("Body-only reason."));
    }

    #[test]
    fn migration_materializes_retired_lifecycle_metadata_from_shared_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let active_dir = root.join("docs/rfcs/stage-3");
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&active_dir).unwrap();
        std::fs::create_dir_all(&rfc_dir).unwrap();

        let anchored_ulid = "01kq09x2d9y6gc9eg27jatcgvf";
        let active_path = active_dir.join("00001-retired.md");
        let intermediate_path = rfc_dir.join("00001-retired-before-rename.md");
        let rfc_path = rfc_dir.join("00001-retired.md");
        std::fs::write(
            &active_path,
            format!("<!-- exo:1 ulid:{anchored_ulid} -->\n\n# RFC 1: Retired\n\n- **Status**: Draft\n- **Reason**:\n\nBody.\n"),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "active RFC"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            Command::new("git")
                .args(["update-ref", "refs/remotes/private-history/HEAD", "HEAD"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "mv",
                    "docs/rfcs/stage-3/00001-retired.md",
                    "docs/rfcs/withdrawn/00001-retired-before-rename.md",
                ])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(intermediate_path.exists());
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "withdraw RFC"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "mv",
                    "docs/rfcs/withdrawn/00001-retired-before-rename.md",
                    "docs/rfcs/withdrawn/00001-retired.md",
                ])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "rename retired RFC"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["update-ref", "refs/remotes/origin/HEAD", "HEAD"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let writer = SqliteWriter::open(&db_path).unwrap();
        writer
            .upsert_rfc(
                anchored_ulid,
                1,
                "Retired",
                0,
                "withdrawn",
                None,
                "retired",
                "docs/rfcs/withdrawn/00001-retired.md",
                None,
                None,
                Some("The proposal was not implemented."),
                None,
                None,
            )
            .unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::Needed { .. }
        ));
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let content = std::fs::read_to_string(&rfc_path).unwrap();
        assert!(content.contains("- **Status**: Withdrawn"));
        assert!(!content.contains("- **Status**: Draft"));
        assert!(content.contains("- **Stage**: 3"));
        assert!(content.contains("- **Reason**: The proposal was not implemented."));
        assert!(retired_rfc_lifecycle_metadata_is_portable(
            &content,
            "withdrawn"
        ));

        let row = SqliteLoader::open(&db_path)
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(
            row.stage, 2,
            "shared metadata should continue to follow the committed canonical document"
        );
        assert_eq!(
            row.withdrawal_reason, None,
            "shared metadata should honor the canonical document's explicit empty reason"
        );
        let effective = crate::rfc::load_effective_rfc_by_number(root, None, 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            effective.record.stage, 3,
            "the issuing workspace should see the backfilled last-active stage"
        );
        assert_eq!(
            effective.record.withdrawal_reason.as_deref(),
            Some("The proposal was not implemented."),
            "the issuing workspace should see the materialized lifecycle reason"
        );
        assert!(effective.provenance.differs_from_canonical);
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::NotNeeded
        ));
    }

    #[test]
    fn migration_preserves_retired_stage_from_yaml_before_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/withdrawn");
        std::fs::create_dir_all(&rfc_dir).unwrap();
        let rfc_path = rfc_dir.join("00002-yaml-retired.md");
        std::fs::write(
            &rfc_path,
            "---\ntitle: YAML Retired\nstage: 3\nulid: 01kq09x2d9y6gc9eg27jatcgvg\nwithdrawal_reason: Replaced.\n---\n\n# RFC 2: YAML Retired\n\nBody.\n",
        )
        .unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        assert!(matches!(
            MigrateRfcMetadataPlugin.is_needed(&context).unwrap(),
            UpgradeStatus::Needed { .. }
        ));
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let content = std::fs::read_to_string(rfc_path).unwrap();
        assert!(content.contains("- **Status**: Withdrawn"));
        assert!(content.contains("- **Stage**: 3"));
        assert!(content.contains("- **Reason**: Replaced."));
    }

    #[test]
    fn historical_stage_lookup_normalizes_windows_paths() {
        let stages = HashMap::from([("docs/rfcs/withdrawn/00001-retired.md".to_string(), 3)]);

        assert_eq!(
            historical_stage_for_path(&stages, r"docs\rfcs\withdrawn\00001-retired.md"),
            Some(3)
        );
    }

    #[test]
    fn historical_stage_recovery_enables_rename_detection_explicitly() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let active_path = root.join("docs/rfcs/stage-3/00001-retired.md");
        let retired_path = root.join("docs/rfcs/withdrawn/00001-retired.md");
        std::fs::create_dir_all(active_path.parent().unwrap()).unwrap();
        std::fs::write(&active_path, "# RFC 1: Retired\n\nBody.\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["config", "diff.renames", "false"],
            vec!["add", "."],
            vec!["commit", "-qm", "active RFC"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir_all(retired_path.parent().unwrap()).unwrap();
        std::fs::rename(&active_path, &retired_path).unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-qm", "retire RFC"]] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        assert_eq!(
            historical_retired_stages(root)
                .get("docs/rfcs/withdrawn/00001-retired.md")
                .copied(),
            Some(3)
        );
    }

    #[test]
    fn anchored_rfc_migrates_relationship_metadata_from_disk() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/stage-4");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        std::fs::write(
            rfc_dir.join("00022-unified-project-state.md"),
            "<!-- exo:22 ulid:01oldstate -->\n\n# RFC 22: Unified Project State\n\n- **Superseded by**: RFC 10176\n",
        )
        .unwrap();
        std::fs::write(
            rfc_dir.join("10176-project-state-model.md"),
            "<!-- exo:10176 ulid:01newstate -->\n\n# RFC 10176: Project State Model\n\n- **Supersedes**: RFC 0022\n",
        )
        .unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path).unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let loader = SqliteLoader::open(&db_path).unwrap();
        let old_row = loader.load_rfc_by_number(22).unwrap().unwrap();
        let new_row = loader.load_rfc_by_number(10176).unwrap().unwrap();
        assert_eq!(old_row.superseded_by.as_deref(), Some("10176"));
        assert_eq!(new_row.supersedes.as_deref(), Some("0022"));
    }

    #[test]
    fn unanchored_rfc_migrates_relationship_metadata_from_body() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/stage-1");
        std::fs::create_dir_all(&rfc_dir).unwrap();

        std::fs::write(
            rfc_dir.join("00001-replacement.md"),
            "---\ntitle: Replacement\nulid: 01kq2222222222222222222222\n---\n\n# RFC 00001: Replacement\n\n- **Supersedes**: RFC 00002, RFC 00003\n",
        )
        .unwrap();

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path).unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let loader = SqliteLoader::open(&db_path).unwrap();
        let row = loader.load_rfc_by_number(1).unwrap().unwrap();
        assert_eq!(row.supersedes.as_deref(), Some("00002, 00003"));
    }

    #[test]
    fn unanchored_migration_keeps_the_migrated_workspace_anchor_addressable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rfc_dir = root.join("docs/rfcs/stage-1");
        std::fs::create_dir_all(&rfc_dir).unwrap();
        let rfc_path = rfc_dir.join("00001-proposal.md");
        std::fs::write(
            &rfc_path,
            "---\ntitle: Proposal\nulid: 01kq2222222222222222222222\n---\n\n# RFC 00001: Proposal\n",
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", "."],
            vec!["commit", "-qm", "legacy unanchored RFC"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let db_path = root.join(SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        SqliteWriter::open(&db_path).unwrap();

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        MigrateRfcMetadataPlugin.apply(&mut context).unwrap();

        let content = std::fs::read_to_string(&rfc_path).unwrap();
        assert!(content.starts_with("<!-- exo:1 ulid:01kq2222222222222222222222 -->"));
        let shared = SqliteLoader::open(&db_path)
            .unwrap()
            .load_rfc_by_number(1)
            .unwrap()
            .unwrap();
        assert_eq!(shared.text_id, "01kq2222222222222222222222");
        let effective = crate::rfc::load_effective_rfc_by_number(root, None, 1)
            .unwrap()
            .unwrap();
        assert_eq!(effective.record.text_id, "01kq2222222222222222222222");
        assert_eq!(
            effective.record.file_path,
            "docs/rfcs/stage-1/00001-proposal.md"
        );
    }

    #[test]
    fn materialized_relationship_metadata_preserves_anchor_position() {
        let content = "<!-- exo:1 ulid:01anchored -->\n\n# Noncanonical Heading\n\n| **Supersedes** | — |\n\nBody\n";

        let updated =
            insert_missing_relationship_metadata_lines(content, Some("00002"), Some("00003"));

        assert!(updated.starts_with("<!-- exo:1 ulid:01anchored -->"));
        assert!(updated.contains(
            "<!-- exo:1 ulid:01anchored -->\n\n- **Superseded by**: RFC 00002\n- **Supersedes**: RFC 00003\n\n# Noncanonical Heading"
        ));
        assert!(updated.contains("| **Supersedes** | — |"));
    }

    #[test]
    fn migration_ignores_rfc_evidence_markdown() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let evidence =
            root.join("docs/rfcs/evidence/0008-served-transport/2026-06-11-live-probes.md");
        std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
        let original = "---\ntitle: Live Probes\n---\n\n# Live probes\n\nEvidence notes.\n";
        std::fs::write(&evidence, original).unwrap();

        assert!(
            find_rfc_files(root).is_empty(),
            "supporting evidence markdown should not be discovered for RFC metadata migration"
        );

        let mut context = AgentContext::new_for_testing(root.to_path_buf());
        let report = MigrateRfcMetadataPlugin.apply(&mut context).unwrap();
        assert!(
            report.changes.is_empty(),
            "evidence-only workspace should not report RFC metadata migration changes: {report:#?}"
        );
        assert_eq!(
            std::fs::read_to_string(&evidence).unwrap(),
            original,
            "migration must not stamp anchors into evidence notes"
        );
    }
}
