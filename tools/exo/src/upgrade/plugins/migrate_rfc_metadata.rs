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
    extract_anchor_ulid, extract_h1_title, extract_rfc_relationships, has_anchor, parse_rfc_number,
    parse_slug, parse_stage, parse_status, strip_frontmatter,
};
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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

        let total_needed = unanchored
            .max(db_missing)
            .max(db_relationships_missing_from_disk);

        if total_needed == 0 {
            Ok(UpgradeStatus::NotNeeded)
        } else {
            Ok(UpgradeStatus::warning(format!(
                "{total_needed} RFC(s) need metadata migration ({unanchored} unanchored, {db_missing} missing from DB, {db_relationships_missing_from_disk} DB relationship(s) missing from disk)"
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
        let mut changes = Vec::new();
        let mut seen_ulids: HashSet<String> = HashSet::new();

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
                .to_string();

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
                withdrawal_reason = existing.and_then(|record| record.withdrawal_reason.clone());
                archived_reason = existing.and_then(|record| record.archived_reason.clone());
                consolidated_into = existing.and_then(|record| record.consolidated_into.clone());
            } else {
                // Parse YAML frontmatter for metadata
                let fm_data = parse_frontmatter(&file_content);
                let fm_title = fm_string(&fm_data, "title");
                let fm_ulid = fm_string(&fm_data, "ulid").and_then(|value| canonical_ulid(&value));
                feature = fm_string(&fm_data, "feature");
                superseded_by = fm_string(&fm_data, "superseded_by")
                    .or_else(|| fm_string(&fm_data, "superseded-by"));
                supersedes = fm_string(&fm_data, "supersedes");
                let relationships = extract_rfc_relationships(&file_content);
                superseded_by = superseded_by.or(relationships.superseded_by);
                supersedes = supersedes.or(relationships.supersedes);
                withdrawal_reason =
                    fm_string_any(&fm_data, &["withdrawal_reason", "withdrawn_reason"]);
                archived_reason = fm_string(&fm_data, "archived_reason");
                consolidated_into = fm_string(&fm_data, "consolidated_into");

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
            }
            seen_ulids.insert(ulid.clone());

            // Upsert into SQLite
            writer.upsert_rfc(
                &ulid,
                rfc_number,
                &title,
                stage,
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
