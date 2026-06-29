use crate::api::protocol::{NextCall, NextCallKind, Steering};
use blake3::Hasher;
use pathdiff::diff_paths;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct DocsLinksInput {
    #[serde(default)]
    pub targets: Targets,
    #[serde(default)]
    pub options: Options,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Targets {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub globs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Options {
    #[serde(default = "default_true")]
    pub strict: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { strict: true }
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DocsLinksResult {
    pub ok: bool,
    pub summary: Summary,
    pub diagnostics: Vec<Diagnostic>,
    pub changes: Vec<Change>,
    pub plan_ticket: String,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Summary {
    pub files_scanned: u32,
    pub exo_links_found: u32,
    pub exo_links_rewritten: u32,
    pub errors: u32,
    pub warnings: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub path: String,
    pub link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Change {
    pub path: String,
    pub replacements: Vec<Replacement>,
    pub before_blake3: String,
    pub after_blake3: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Replacement {
    pub byte_start: usize,
    pub byte_end: usize,
    pub replacement: String,
}

pub fn run_check(
    workspace_root: &Path,
    input: &DocsLinksInput,
) -> anyhow::Result<(DocsLinksResult, Option<Steering>)> {
    let project = crate::project::Project::resolve(workspace_root).ok();
    run_check_with_project(workspace_root, project.as_ref(), input)
}

pub fn run_check_with_project(
    workspace_root: &Path,
    project: Option<&crate::project::Project>,
    input: &DocsLinksInput,
) -> anyhow::Result<(DocsLinksResult, Option<Steering>)> {
    let (mut result, per_file_plans) = plan(workspace_root, project, input)?;

    // `check` is "ok" only when there is nothing to rewrite and no errors.
    result.ok = result.diagnostics.is_empty() && result.changes.is_empty();

    let steering = if !result.ok && !result.changes.is_empty() {
        Some(steer_fix(input))
    } else {
        None
    };

    // Avoid unused warning if we later add richer result; keep this structure.
    let _ = per_file_plans;

    Ok((result, steering))
}

pub fn run_fix(workspace_root: &Path, input: &DocsLinksInput) -> anyhow::Result<DocsLinksResult> {
    let project = crate::project::Project::resolve(workspace_root).ok();
    let (_pre, per_file_plans) = plan(workspace_root, project.as_ref(), input)?;

    for (abs_path, planned) in per_file_plans {
        if planned.replacements.is_empty() {
            continue;
        }

        crate::utils::edit_file_with_permissions(&abs_path, |content| {
            let mut updated = content.to_string();
            // Apply replacements from end to start to preserve byte offsets.
            for r in planned.replacements.iter().rev() {
                updated.replace_range(r.byte_start..r.byte_end, &r.replacement);
            }
            Ok(updated)
        })?;
    }

    // Re-run plan to compute post-fix status/hashes.
    let (mut post, _post_plans) = plan(workspace_root, project.as_ref(), input)?;
    post.ok = post.diagnostics.is_empty() && post.changes.is_empty();
    Ok(post)
}

#[derive(Debug, Clone)]
struct PlannedFile {
    replacements: Vec<Replacement>,
}

fn plan(
    workspace_root: &Path,
    project: Option<&crate::project::Project>,
    input: &DocsLinksInput,
) -> anyhow::Result<(DocsLinksResult, Vec<(PathBuf, PlannedFile)>)> {
    let files = gather_files(workspace_root, &input.targets)?;

    let re = Regex::new(r"\(exo:(?P<rest>//[^)\s]+|[^)\s]+)\)")
        .map_err(|e| anyhow::anyhow!("Invalid regex: {e}"))?;

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut changes: Vec<Change> = Vec::new();
    let mut per_file: Vec<(PathBuf, PlannedFile)> = Vec::new();

    let mut summary = Summary {
        files_scanned: files.len() as u32,
        ..Default::default()
    };

    for abs in files {
        let rel = abs.strip_prefix(workspace_root).unwrap_or(&abs);
        let path_display = rel.to_string_lossy().replace('\\', "/");

        let Ok(text) = fs::read_to_string(&abs) else {
            diagnostics.push(Diagnostic {
                severity: "error".to_string(),
                code: "read_failed".to_string(),
                message: "Failed to read file".to_string(),
                path: path_display,
                link: String::new(),
                suggested: None,
            });
            summary.errors += 1;
            continue;
        };

        let before_hash = blake3_hex(text.as_bytes());

        let mut replacements: Vec<Replacement> = Vec::new();

        for caps in re.captures_iter(&text) {
            let Some(m) = caps.get(0) else { continue };
            let Some(rest) = caps.name("rest") else {
                continue;
            };

            let mut uri = rest.as_str().to_string();
            if let Some(stripped) = uri.strip_prefix("//") {
                uri = stripped.to_string();
            }

            summary.exo_links_found += 1;

            let (uri_path, fragment) = split_fragment(&uri);

            // Common placeholder used in RFC prose/examples.
            // We intentionally do not treat this as an error.
            if uri_path == "..." {
                continue;
            }
            let resolved = resolve_exo_uri(workspace_root, project, &uri_path);

            match resolved {
                Ok(target_abs) => {
                    let from_dir = abs.parent().unwrap_or(workspace_root);
                    let Some(rel_to_target) = diff_paths(&target_abs, from_dir) else {
                        diagnostics.push(Diagnostic {
                            severity: "error".to_string(),
                            code: "pathdiff_failed".to_string(),
                            message: "Failed to compute relative path".to_string(),
                            path: path_display.clone(),
                            link: format!("exo:{uri}"),
                            suggested: None,
                        });
                        summary.errors += 1;
                        continue;
                    };

                    let mut rel_str = rel_to_target.to_string_lossy().replace('\\', "/");
                    if let Some(frag) = fragment {
                        rel_str.push('#');
                        rel_str.push_str(&frag);
                    }

                    // Replace the entire `(exo:...)` with `(relative...)`.
                    let replacement = format!("({rel_str})");
                    replacements.push(Replacement {
                        byte_start: m.start(),
                        byte_end: m.end(),
                        replacement,
                    });
                }
                Err(err_code) => {
                    let (severity, code) = if input.options.strict {
                        ("error", err_code)
                    } else {
                        ("warning", err_code)
                    };

                    diagnostics.push(Diagnostic {
                        severity: severity.to_string(),
                        code: code.to_string(),
                        message: "Unresolved exo link".to_string(),
                        path: path_display.clone(),
                        link: format!("exo:{uri}"),
                        suggested: None,
                    });

                    if input.options.strict {
                        summary.errors += 1;
                    } else {
                        summary.warnings += 1;
                    }
                }
            }
        }

        replacements.sort_by_key(|r| r.byte_start);

        // Compute after hash as if we applied replacements.
        let mut after_text = text.clone();
        for r in replacements.iter().rev() {
            after_text.replace_range(r.byte_start..r.byte_end, &r.replacement);
        }
        let after_hash = blake3_hex(after_text.as_bytes());

        if !replacements.is_empty() {
            summary.exo_links_rewritten += replacements.len() as u32;
            changes.push(Change {
                path: path_display,
                replacements: replacements.clone(),
                before_blake3: before_hash,
                after_blake3: after_hash,
            });
        }

        per_file.push((abs, PlannedFile { replacements }));
    }

    // Deterministic ticket for the change plan.
    let plan_ticket = blake3_ticket(&changes);

    Ok((
        DocsLinksResult {
            ok: false,
            summary,
            diagnostics,
            changes,
            plan_ticket,
        },
        per_file,
    ))
}

fn gather_files(workspace_root: &Path, targets: &Targets) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();

    // If explicit paths are provided, use them.
    if !targets.paths.is_empty() {
        for p in &targets.paths {
            let abs = workspace_root.join(p);
            if abs.is_dir() {
                for entry in WalkDir::new(&abs) {
                    let entry = entry?;
                    if entry.file_type().is_file() {
                        let path = entry.path();
                        if is_markdown(path) {
                            let rel = path
                                .strip_prefix(workspace_root)
                                .unwrap_or(path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            files.insert(rel, path.to_path_buf());
                        }
                    }
                }
            } else if abs.is_file() && is_markdown(&abs) {
                let rel = abs
                    .strip_prefix(workspace_root)
                    .unwrap_or(&abs)
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(rel, abs);
            }
        }

        return Ok(files.into_values().collect());
    }

    // Otherwise, respect a tiny subset of globs (enough for v1 defaults).
    // If globs are omitted, default to docs/**/*.md plus README.md.
    let mut globs = targets.globs.clone();
    if globs.is_empty() {
        globs.push("docs/**/*.md".to_string());
        globs.push("README.md".to_string());
    }

    for g in globs {
        match g.as_str() {
            "README.md" => {
                let abs = workspace_root.join("README.md");
                if abs.is_file() {
                    files.insert("README.md".to_string(), abs);
                }
            }
            "docs/**/*.md" => {
                let abs = workspace_root.join("docs");
                if abs.is_dir() {
                    for entry in WalkDir::new(&abs) {
                        let entry = entry?;
                        if entry.file_type().is_file() {
                            let path = entry.path();
                            if is_markdown(path) {
                                let rel = path
                                    .strip_prefix(workspace_root)
                                    .unwrap_or(path)
                                    .to_string_lossy()
                                    .replace('\\', "/");
                                files.insert(rel, path.to_path_buf());
                            }
                        }
                    }
                }
            }
            _ => {
                // Ignore unknown glob patterns for now.
            }
        }
    }

    Ok(files.into_values().collect())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

fn split_fragment(uri: &str) -> (String, Option<String>) {
    if let Some((left, frag)) = uri.split_once('#') {
        (left.to_string(), Some(frag.to_string()))
    } else {
        (uri.to_string(), None)
    }
}

fn resolve_exo_uri(
    workspace_root: &Path,
    project: Option<&crate::project::Project>,
    uri: &str,
) -> Result<PathBuf, &'static str> {
    let (kind, rest) = uri.split_once('/').ok_or("invalid_uri")?;

    match kind {
        "rfc" => resolve_rfc(workspace_root, rest),
        "context" => resolve_context(workspace_root, project, rest),
        "spec" => resolve_spec(workspace_root, rest),
        _ => Err("unknown_kind"),
    }
}

fn resolve_spec(workspace_root: &Path, rest: &str) -> Result<PathBuf, &'static str> {
    // Prefer docs/specs; fall back to docs/agent-context/specs.
    let mut rel = PathBuf::from("docs/specs");
    rel.push(rest);
    if rel.extension().is_none() {
        rel.set_extension("md");
    }
    let abs = workspace_root.join(&rel);
    if abs.is_file() {
        return Ok(abs);
    }

    let mut rel2 = PathBuf::from("docs/agent-context/specs");
    rel2.push(rest);
    if rel2.extension().is_none() {
        rel2.set_extension("md");
    }
    let abs2 = workspace_root.join(rel2);
    if abs2.is_file() {
        Ok(abs2)
    } else {
        Err("spec_not_found")
    }
}

fn resolve_context(
    workspace_root: &Path,
    project: Option<&crate::project::Project>,
    rest: &str,
) -> Result<PathBuf, &'static str> {
    let info = crate::command::context::context_path_info(workspace_root, project);

    let Some(path) = info.paths.get(rest) else {
        return Err("context_unknown");
    };

    let abs = absolute_context_path(workspace_root, path);
    if !abs.exists() {
        return Err("context_not_found");
    }

    if !abs.starts_with(workspace_root) {
        return Err("context_external_projection");
    }

    Ok(abs)
}

fn absolute_context_path(workspace_root: &Path, path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        workspace_root.join(p)
    }
}

fn resolve_rfc(workspace_root: &Path, rest: &str) -> Result<PathBuf, &'static str> {
    let n: u32 = rest.parse().map_err(|_| "rfc_invalid")?;
    let prefix = format!("{n:04}-");

    let rfcs_root = workspace_root.join("docs/rfcs");
    if !rfcs_root.is_dir() {
        return Err("rfc_not_found");
    }

    let mut matches = Vec::new();
    for entry in WalkDir::new(&rfcs_root) {
        let entry = entry.map_err(|_| "rfc_not_found")?;
        if entry.file_type().is_file() {
            let path = entry.path();
            let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
            let name_matches = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with(&prefix));

            if is_md && name_matches {
                matches.push(path.to_path_buf());
            }
        }
    }

    match matches.len() {
        0 => Err("rfc_not_found"),
        1 => Ok(matches.remove(0)),
        _ => Err("rfc_ambiguous"),
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn blake3_ticket(changes: &[Change]) -> String {
    // Serialize into a stable JSON representation.
    let payload = serde_json::to_vec(changes).unwrap_or_default();
    let mut hasher = Hasher::new();
    hasher.update(&payload);
    hasher.finalize().to_hex().to_string()
}

fn steer_fix(input: &DocsLinksInput) -> Steering {
    Steering {
        next_call: NextCall {
            kind: NextCallKind::Call,
            params: json!({
                "address": { "kind": "operation", "path": ["docs", "links", "fix"] },
                "input": serde_json::to_value(input).unwrap_or_else(|_| json!({}))
            }),
        },
        priority: None,
        confidence: None,
        context_note: None,
    }
}

pub fn parse_input(input: &JsonValue) -> anyhow::Result<DocsLinksInput> {
    if !input.is_object() {
        anyhow::bail!("input must be an object");
    }
    let parsed: DocsLinksInput = serde_json::from_value(input.clone())?;
    Ok(parsed)
}

pub fn result_to_json(result: DocsLinksResult) -> JsonValue {
    serde_json::to_value(result).unwrap_or_else(|_| json!({}))
}
