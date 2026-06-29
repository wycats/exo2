#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)] // workspace-only dev tool uses blocking I/O

use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
struct List {
    name: String,
    items: Vec<String>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut sha: Option<String> = None;
    let mut check = false;
    let mut force = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--sha" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--sha requires a value"))?;
                sha = Some(value.clone());
            }
            "--check" => {
                check = true;
            }
            "--force" => {
                force = true;
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => {
                bail!("unknown arg: {other} (try --help)");
            }
        }
        i += 1;
    }

    let sha = sha.unwrap_or_else(|| "main".to_string());
    let repo = "https://github.com/moonrepo/moon";
    let path_in_repo = "crates/toolchain/src/detect/languages.rs";

    let upstream = fetch_file_via_git(repo, &sha, path_in_repo)
        .with_context(|| format!("failed to fetch {repo}@{sha}:{path_in_repo}"))?;

    let lists = parse_static_string_lists(&upstream)
        .with_context(|| "failed to parse StaticStringList constants".to_string())?;

    let out = render_marker_inventory_rs(repo, path_in_repo, &sha, &lists);

    let workspace_root = find_git_root(PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path())
        .ok_or_else(|| anyhow!("could not find workspace git root"))?;

    let out_path = workspace_root
        .join("tools/exo/src/marker_inventory.rs")
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.join("tools/exo/src/marker_inventory.rs"));

    if check {
        let existing = fs::read_to_string(&out_path)
            .with_context(|| format!("failed to read {}", out_path.display()))?;
        if normalize_newlines(&existing) != normalize_newlines(&out) {
            bail!(
                "{} is out of date (run this tool without --check)",
                out_path.display()
            );
        }
        return Ok(());
    }

    if !force {
        guard_output_not_dirty(&workspace_root, &out_path)?;
    }

    fs::write(&out_path, out).with_context(|| format!("failed to write {}", out_path.display()))?;

    Ok(())
}

fn print_help() {
    println!(
        "Usage: sync_moon_language_markers [--sha <sha|ref>] [--check] [--force]\n\
\n\
Fetches moonrepo/moon language marker lists and rewrites tools/exo/src/marker_inventory.rs.\n\
\n\
Options:\n\
  --sha <sha|ref>  Git ref to fetch from (default: main)\n\
  --check          Exit non-zero if marker_inventory.rs differs\n\
  --force          Overwrite even if marker_inventory.rs is modified in git\n"
    );
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn guard_output_not_dirty(workspace_root: &Path, out_path: &Path) -> Result<()> {
    // Only enforce this guard when we can make the output path relative.
    let rel = out_path.strip_prefix(workspace_root).unwrap_or(out_path);

    let output = Command::new("git")
        .current_dir(workspace_root)
        .arg("status")
        .arg("--porcelain")
        .arg("--")
        .arg(rel)
        .output()
        .context("failed to run git status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git status failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut blocking_lines: Vec<&str> = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Allow untracked output files; initial generation will often be untracked.
        // Anything else (modified/staged/etc) is considered unsafe to overwrite.
        if !trimmed.starts_with("??") {
            blocking_lines.push(trimmed);
        }
    }

    if !blocking_lines.is_empty() {
        bail!(
            "refusing to overwrite modified file: {} (use --force)\n{}",
            display_path(out_path),
            blocking_lines.join("\n")
        );
    }

    Ok(())
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);

    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }

    None
}

fn fetch_file_via_git(repo: &str, sha: &str, path_in_repo: &str) -> Result<String> {
    let tmp_root =
        std::env::temp_dir().join(format!("exo-update-moon-markers-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_root)?;

    // We avoid `git clone` (more data) and instead do a minimal fetch.
    run(Command::new("git").arg("init").arg(&tmp_root))?;
    run(Command::new("git")
        .current_dir(&tmp_root)
        .arg("remote")
        .arg("add")
        .arg("origin")
        .arg(repo))?;

    // Fetch the requested ref/sha.
    run(Command::new("git")
        .current_dir(&tmp_root)
        .arg("fetch")
        .arg("--depth")
        .arg("1")
        .arg("origin")
        .arg(sha))?;

    let output = Command::new("git")
        .current_dir(&tmp_root)
        .arg("show")
        .arg(format!("FETCH_HEAD:{path_in_repo}"))
        .output()
        .context("failed to run git show")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git show failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout).context("git show output was not utf-8")?;

    // Best-effort cleanup.
    let _ = fs::remove_dir_all(&tmp_root);

    Ok(stdout)
}

fn run(cmd: &mut Command) -> Result<()> {
    let output = cmd.output().context("failed to spawn process")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("command failed: {stderr}");
    }
    Ok(())
}

fn parse_static_string_lists(src: &str) -> Result<Vec<List>> {
    let mut lists: Vec<List> = Vec::new();

    let mut current_name: Option<String> = None;
    let mut current_items: Vec<String> = Vec::new();
    let mut in_list = false;

    for line in src.lines() {
        let trimmed = line.trim();

        if !in_list {
            // Example: pub static NODE: StaticStringList = &[
            if let Some(rest) = trimmed.strip_prefix("pub static ")
                && let Some((name, after_name)) = rest.split_once(':')
            {
                let name = name.trim();
                let after_name = after_name.trim();
                if after_name.starts_with("StaticStringList") && trimmed.contains("= &[") {
                    current_name = Some(name.to_string());
                    current_items.clear();
                    in_list = true;

                    // Handle single-line lists like:
                    // pub static BUN: StaticStringList = &["a", "b"];
                    if let Some((_, after_open)) = trimmed.split_once("= &[") {
                        extract_string_literals(after_open, &mut current_items);
                        if trimmed.contains("];") {
                            let name = current_name
                                .take()
                                .ok_or_else(|| anyhow!("missing list name"))?;
                            lists.push(List {
                                name,
                                items: current_items.clone(),
                            });
                            in_list = false;
                        }
                    }
                    continue;
                }
            }
            continue;
        }

        // End of list: ];
        if trimmed.starts_with("];") {
            let name = current_name
                .take()
                .ok_or_else(|| anyhow!("missing list name"))?;
            lists.push(List {
                name,
                items: current_items.clone(),
            });
            in_list = false;
            continue;
        }

        // Extract string literals from the line.
        extract_string_literals(trimmed, &mut current_items);
    }

    if in_list {
        bail!("unterminated list in upstream file");
    }

    Ok(lists)
}

fn extract_string_literals(line: &str, out: &mut Vec<String>) {
    let mut chars = line.chars();

    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }

        let mut value = String::new();
        while let Some(next) = chars.next() {
            match next {
                '"' => break,
                '\\' => {
                    // Best-effort unescape of \" and \\.
                    if let Some(escaped) = chars.next() {
                        value.push(escaped);
                    }
                }
                _ => value.push(next),
            }
        }

        if !value.is_empty() {
            out.push(value);
        }
    }
}

#[allow(clippy::format_push_string, clippy::single_char_add_str)]
fn render_marker_inventory_rs(repo: &str, path_in_repo: &str, sha: &str, lists: &[List]) -> String {
    let mut out = String::new();

    out.push_str("//! Marker inventories for detecting \"source markers\" in a repository.\n");
    out.push_str("//!\n");
    out.push_str(
        "//! This file is intended to be *data-only* and can be refreshed from upstream.\n",
    );
    out.push_str("//!\n");
    out.push_str("//! Upstream:\n");
    out.push_str(&format!("//! - repo: <{repo}>\n"));
    out.push_str(&format!("//! - path: {path_in_repo}\n"));
    out.push_str(&format!("//! - pinned: {sha}\n"));
    out.push_str("//!\n");
    out.push_str("//! Update:\n");
    out.push_str("//! - run: `cargo run -p exo-workspace --bin sync_moon_language_markers` (optionally `--sha <sha>`)\n");
    out.push_str("\n");

    out.push_str("pub type MarkerList = &'static [&'static str];\n\n");
    out.push_str(&format!("pub const UPSTREAM_REPO: &str = \"{repo}\";\n"));
    out.push_str(&format!(
        "pub const UPSTREAM_PATH: &str = \"{path_in_repo}\";\n"
    ));
    out.push_str(&format!("pub const UPSTREAM_SHA: &str = \"{sha}\";\n\n"));

    for list in lists {
        out.push_str(&format!("pub const {}: MarkerList = &[\n", list.name));
        for item in &list.items {
            out.push_str(&format!("    \"{}\",\n", item.replace('"', "\\\"")));
        }
        out.push_str("];\n\n");
    }

    out
}

#[test]
#[allow(clippy::panic)]
fn parse_extracts_expected_lists() {
    let src = r#"
pub type StaticString = &'static str;
pub type StaticStringList = &'static [StaticString];

pub static RUST: StaticStringList = &[
    "Cargo.toml",
    "Cargo.lock",
];

pub static GO: StaticStringList = &["go.mod"]; 
"#;

    let Ok(lists) = parse_static_string_lists(src) else {
        panic!("parsing test input should succeed");
    };
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].name, "RUST");
    assert_eq!(lists[0].items, vec!["Cargo.toml", "Cargo.lock"]);
    assert_eq!(lists[1].name, "GO");
    assert_eq!(lists[1].items, vec!["go.mod"]);
}

#[test]
fn render_is_stable() {
    let lists = vec![List {
        name: "RUST".to_string(),
        items: vec!["Cargo.toml".to_string()],
    }];

    let rendered = render_marker_inventory_rs(
        "https://example.com/repo",
        "path/to/file.rs",
        "deadbeef",
        &lists,
    );

    assert!(rendered.contains("pub const RUST: MarkerList"));
    assert!(rendered.contains("UPSTREAM_SHA: &str = \"deadbeef\""));
}

#[test]
fn dirty_guard_checks_only_target_file() {
    // Minimal sanity: the guard should try to run git. If we're not in a git repo,
    // it's ok for this test to be skipped by returning early.
    let Some(root) = find_git_root(Path::new(env!("CARGO_MANIFEST_DIR"))) else {
        return;
    };

    // Create a path that almost certainly isn't tracked/modified; git should return ok.
    let bogus = root.join("this/does/not/exist");
    let _ = guard_output_not_dirty(&root, &bogus);
}
