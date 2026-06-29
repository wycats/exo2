use crate::api::protocol::{Reminder, ReminderSeverity};
use crate::rfc;
use anyhow::Context;
use gray_matter::{Matter, engine::YAML};
use serde_json::json;
use std::path::Path;
use std::process::Command;

pub fn run_global_verifiers(root: &Path) -> Vec<Reminder> {
    // Some verifiers (e.g. empty RFC detection) do not require git.
    // If we're not in a git repository, treat the untracked list as empty but
    // still run the rest of the checks.
    let untracked = git_untracked_paths(root).unwrap_or_default();

    let mut reminders = Vec::new();
    for v in all_verifiers() {
        let found = v.check(root, &untracked);
        let fixed = v.autofix(root, &untracked);

        if fixed.is_empty() {
            reminders.extend(found);
        } else {
            reminders.extend(fixed);
        }
    }
    reminders
}

trait Verifier {
    /// Pure analysis: report problems without mutating the workspace.
    fn check(&self, root: &Path, untracked: &[String]) -> Vec<Reminder>;

    /// Best-effort repair: fix problems if possible and report what was changed.
    fn autofix(&self, root: &Path, untracked: &[String]) -> Vec<Reminder>;
}

fn all_verifiers() -> Vec<Box<dyn Verifier>> {
    vec![
        Box::new(RfcManualCreationRepairVerifier),
        Box::new(RfcEmptyFileVerifier),
        Box::new(RfcFilenameIdentityVerifier),
    ]
}

struct RfcManualCreationRepairVerifier;

struct RfcEmptyFileVerifier;

struct RfcFilenameIdentityVerifier;

impl Verifier for RfcManualCreationRepairVerifier {
    fn check(&self, root: &Path, untracked: &[String]) -> Vec<Reminder> {
        detect_manual_rfcs(root, untracked)
    }

    fn autofix(&self, root: &Path, untracked: &[String]) -> Vec<Reminder> {
        repair_manual_rfcs(root, untracked)
    }
}

impl Verifier for RfcEmptyFileVerifier {
    fn check(&self, root: &Path, _untracked: &[String]) -> Vec<Reminder> {
        detect_empty_rfcs(root)
    }

    fn autofix(&self, _root: &Path, _untracked: &[String]) -> Vec<Reminder> {
        // Intentionally no autofix: replacing empty RFCs automatically could
        // destroy evidence needed to debug how they became empty.
        Vec::new()
    }
}

impl Verifier for RfcFilenameIdentityVerifier {
    fn check(&self, root: &Path, _untracked: &[String]) -> Vec<Reminder> {
        detect_rfc_identity_repairs(root)
    }

    fn autofix(&self, _root: &Path, _untracked: &[String]) -> Vec<Reminder> {
        // Intentionally no autofix: filename identity repair should be explicit
        // (`exo rfc repair <id>`) so users can review the path changes.
        Vec::new()
    }
}

fn detect_empty_rfcs(root: &Path) -> Vec<Reminder> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        return Vec::new();
    }

    let mut reminders = Vec::new();

    for entry in walkdir::WalkDir::new(&rfc_root)
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
        if !rfc::is_rfc_document_path(&rfc_root, path) {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if filename == "0000-template.md" || filename == "README.md" {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        if !content.trim().is_empty() {
            continue;
        }

        let rel = path.strip_prefix(root).ok().map_or_else(
            || path.to_string_lossy().to_string(),
            |p| p.to_string_lossy().to_string(),
        );

        reminders.push(Reminder {
            kind: "rfc.empty_file".to_string(),
            severity: ReminderSeverity::Error,
            message: "RFC file is empty (0 bytes/blank). This usually indicates an interrupted write or an incomplete RFC create/edit step.".to_string(),
            details: Some(json!({
                "path": rel,
                "recommended_action": "Restore the file contents (e.g. from your editor history) and rerun exo verify"
            })),
        });
    }

    reminders
}

fn detect_rfc_identity_repairs(root: &Path) -> Vec<Reminder> {
    rfc::detect_rfc_repair_candidates(root).map_or_else(
        |_| Vec::new(),
        |candidates| {
            candidates
                .into_iter()
                .map(|candidate| Reminder {
                    kind: "rfc.identity_repair_needed".to_string(),
                    severity: ReminderSeverity::Warning,
                    message: format!(
                        "RFC {} identity metadata needs repair. Run: exo rfc repair {}",
                        candidate.id, candidate.id
                    ),
                    details: Some(json!({
                        "id": candidate.id,
                        "current_path": candidate.current_path,
                        "expected_path": candidate.expected_path,
                        "title": candidate.title,
                        "reasons": candidate.reasons,
                        "recommended_action": format!("exo rfc repair {}", candidate.id),
                    })),
                })
                .collect()
        },
    )
}

fn detect_manual_rfcs(root: &Path, untracked: &[String]) -> Vec<Reminder> {
    let mut reminders = Vec::new();

    for rel in untracked {
        if !is_rfc_path(rel) {
            continue;
        }

        // Skip known non-RFC files.
        if rel.ends_with("/README.md") || rel.ends_with("/0007-template.md") {
            continue;
        }

        // If the file is already stamped as tool-created, it's not a manual RFC.
        // This commonly happens when `exo rfc create` is used but the file is still untracked.
        let full = root.join(rel);
        // If the path doesn't exist under `root`, it's likely being reported relative to a
        // parent git repository root. In that case, skip: this verifier only reasons about
        // files within the current workspace root.
        if !full.exists() {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&full)
            && looks_tool_created_rfc(&content)
        {
            continue;
        }

        reminders.push(Reminder {
            kind: "rfc.manual_file_detected".to_string(),
            severity: ReminderSeverity::Error,
            message: "Manual RFC file detected (this should not happen). Use `exo rfc create` instead of creating files under docs/rfcs/ manually.".to_string(),
            details: Some(json!({
                "path": rel,
                "recommended_action": "exo rfc create"
            })),
        });
    }

    reminders
}

fn git_untracked_paths(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .context("Failed to run git status --porcelain")?;

    if !output.status.success() {
        anyhow::bail!("git status --porcelain failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out = Vec::new();

    for line in stdout.lines() {
        // Untracked files are `?? path`
        if let Some(path) = line.strip_prefix("?? ") {
            out.push(path.trim().to_string());
        }
    }

    Ok(out)
}

fn repair_manual_rfcs(root: &Path, untracked: &[String]) -> Vec<Reminder> {
    let mut reminders = Vec::new();

    for rel in untracked {
        if !is_rfc_path(rel) {
            continue;
        }

        // Skip known non-RFC files.
        if rel.ends_with("/README.md") || rel.ends_with("/0007-template.md") {
            continue;
        }

        let full = root.join(rel);
        if !full.exists() {
            continue;
        }

        // Skip files modified within the last 5 seconds to avoid racing with
        // concurrent file creation (e.g., VS Code's create_file tool).
        // This prevents corruption when the verifier reads a partially-written file.
        if let Ok(metadata) = std::fs::metadata(&full)
            && let Ok(modified) = metadata.modified()
            && let Ok(elapsed) = modified.elapsed()
            && elapsed.as_secs() < 5
        {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&full) else {
            continue;
        };

        if looks_tool_created_rfc(&content) {
            continue;
        }

        let filename = full
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("????.md");

        let stage = infer_stage_from_rel_path(rel).unwrap_or(0);
        let parsed = rfc::parse_rfc(filename, &content, Some(stage));

        let (title, feature, stage) = (parsed.title, parsed.feature, parsed.stage);

        let body = extract_body_without_frontmatter(&content);
        let body = strip_leading_h1(&body);

        let old_id = rfc_number_from_filename(filename);
        let (create_id, reason) = match old_id.as_deref() {
            Some(number) => match rfc_number_available(root, number, &full) {
                Ok(true) => (Some(number), "preserved_visible_filename_number"),
                Ok(false) => (None, "visible_filename_number_already_exists"),
                Err(_) => (None, "could_not_check_visible_filename_number"),
            },
            None => (None, "manual_filename_has_no_numeric_prefix"),
        };

        let (new_path, reason) =
            match rfc::create(root, &title, create_id, &feature, stage, Some(&body)) {
                Ok(path) => (path, reason),
                Err(_) if create_id.is_some() => {
                    let Ok(path) = rfc::create(root, &title, None, &feature, stage, Some(&body))
                    else {
                        continue;
                    };
                    (path, "visible_filename_number_could_not_be_reused")
                }
                Err(_) => continue,
            };

        let new_id = new_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(rfc_number_from_filename);

        // Remove the original manual file unless the repaired RFC reused the
        // same filename. In the same-path case `rfc::create` already replaced
        // the file with anchored Exo-owned content.
        if new_path != full && std::fs::remove_file(&full).is_err() {
            // If we can't remove it, still report what happened.
        }

        let new_rel = new_path.strip_prefix(root).ok().map_or_else(
            || new_path.to_string_lossy().to_string(),
            |p| p.to_string_lossy().to_string(),
        );

        let old_id_for_message = old_id.as_deref().unwrap_or("unknown");
        let new_id_for_message = new_id.as_deref().unwrap_or("unknown");
        let message = if old_id == new_id {
            format!(
                "Manual RFC file detected and repaired as RFC {new_id_for_message}. Use `exo rfc create` instead of creating files under docs/rfcs/ manually."
            )
        } else {
            format!(
                "Manual RFC file detected and repaired from RFC {old_id_for_message} to RFC {new_id_for_message}. Use `exo rfc create` instead of creating files under docs/rfcs/ manually."
            )
        };
        let action = if new_path == full {
            "recreated_with_exo_rfc_create_at_same_path"
        } else {
            "recreated_with_exo_rfc_create_and_removed_original"
        };

        reminders.push(Reminder {
            kind: "rfc.manual_file_repaired".to_string(),
            severity: ReminderSeverity::Error,
            message,
            details: Some(json!({
                "old_path": rel,
                "new_path": new_rel,
                "old_id": old_id,
                "new_id": new_id,
                "reason": reason,
                "action": action
            })),
        });
    }

    reminders
}

fn rfc_number_from_filename(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let number = stem
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(number)
}

fn rfc_number_available(root: &Path, number: &str, ignored_path: &Path) -> anyhow::Result<bool> {
    let rfc_root = root.join("docs/rfcs");
    if !rfc_root.exists() {
        return Ok(true);
    }

    let ignored_path = ignored_path
        .canonicalize()
        .unwrap_or_else(|_| ignored_path.to_path_buf());
    let wanted = number.parse::<i64>().ok();

    for entry in walkdir::WalkDir::new(&rfc_root)
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
        if !rfc::is_rfc_document_path(&rfc_root, path) {
            continue;
        }

        let comparable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if comparable_path == ignored_path {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(existing) = rfc_number_from_filename(filename) else {
            continue;
        };

        let same_number = match (wanted, existing.parse::<i64>().ok()) {
            (Some(wanted), Some(existing)) => wanted == existing,
            _ => existing == number,
        };
        if same_number {
            return Ok(false);
        }
    }

    Ok(true)
}

fn is_rfc_path(path: &str) -> bool {
    path.starts_with("docs/rfcs/stage-")
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn infer_stage_from_rel_path(rel: &str) -> Option<u8> {
    // docs/rfcs/stage-1/...
    let parts: Vec<&str> = rel.split('/').collect();
    let stage_part = parts.get(2)?; // "stage-1"
    stage_part
        .strip_prefix("stage-")
        .and_then(|s| s.parse::<u8>().ok())
}

fn looks_tool_created_rfc(content: &str) -> bool {
    // Anchor-based RFCs are the current managed format. An anchored file is
    // already owned by `exo rfc create`/`exo rfc promote`, even when git still
    // reports it as untracked. Treating it as manual would destroy the RFC's
    // identity by recreating it with a new number.
    if rfc::has_anchor(content) {
        return true;
    }

    // Fast-path heuristic: if the RFC has a YAML frontmatter block and it
    // contains `tool: exo rfc create`, treat it as tool-created even if YAML
    // parsing fails (we prefer false-negatives over noisy false-positives).
    {
        let mut lines = content.lines();
        if matches!(lines.next(), Some(first) if first.trim() == "---") {
            for line in lines {
                let trimmed = line.trim();
                if trimmed == "---" {
                    break;
                }
                if trimmed == "tool: exo rfc create" {
                    return true;
                }
            }
        }
    }

    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content);
    let Some(data) = parsed.data else {
        return false;
    };

    let Ok(v) = data.deserialize::<serde_json::Value>() else {
        return false;
    };

    v.get("exo")
        .and_then(|exo| exo.get("tool"))
        .and_then(|t| t.as_str())
        .is_some_and(|t| t == "exo rfc create")
}

fn extract_body_without_frontmatter(content: &str) -> String {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content);
    parsed.content
}

fn strip_leading_h1(body: &str) -> String {
    // If the body starts with an H1, drop it to avoid duplicate headers.
    let mut lines = body.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };

    let trimmed_first = first.trim_start();
    if !trimmed_first.starts_with("# ") {
        return body.to_string();
    }

    // Drop following single blank line if present.
    let mut rest: Vec<&str> = lines.collect();
    if rest.first().is_some_and(|l| l.trim().is_empty()) {
        rest.remove(0);
    }

    rest.join("\n")
}

#[cfg(test)]
mod tests {
    use super::looks_tool_created_rfc;

    #[test]
    fn recognizes_anchor_stamped_rfc_as_tool_created() {
        let content =
            "<!-- exo:10185 ulid:01kt2swhyfq7a1astajrfk71e7 -->\n\n# RFC 10185: Example\n";

        assert!(looks_tool_created_rfc(content));
    }
}
