//! Fileset computation from git state.

use std::path::Path;
use std::process::Command;

/// The scope of files to validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesetScope {
    /// Files staged for commit (git diff --cached --name-only).
    Staged,
    /// Files with uncommitted changes (git diff --name-only + staged).
    Uncommitted,
    /// Commits not yet pushed (git diff origin/main...HEAD --name-only).
    CommittedNotPushed,
    /// All files at HEAD.
    Head,
}

/// Compute the file list for a given scope.
pub fn compute_fileset(repo_root: &Path, scope: FilesetScope) -> Result<Vec<String>, String> {
    match scope {
        FilesetScope::Staged => run_git(
            repo_root,
            &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        ),
        FilesetScope::Uncommitted => {
            // Staged + unstaged changes.
            let staged = run_git(
                repo_root,
                &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
            )?;
            let unstaged = run_git(repo_root, &["diff", "--name-only", "--diff-filter=ACMR"])?;
            let mut files: Vec<_> = staged.into_iter().chain(unstaged).collect();
            files.sort();
            files.dedup();
            Ok(files)
        }
        FilesetScope::CommittedNotPushed => {
            // Try origin/main, fall back to origin/master, then just list files.
            run_git(
                repo_root,
                &[
                    "diff",
                    "--name-only",
                    "--diff-filter=ACMR",
                    "origin/main...HEAD",
                ],
            )
            .or_else(|_| {
                run_git(
                    repo_root,
                    &[
                        "diff",
                        "--name-only",
                        "--diff-filter=ACMR",
                        "origin/master...HEAD",
                    ],
                )
            })
            .or_else(|_| run_git(repo_root, &["ls-files"]))
        }
        FilesetScope::Head => run_git(repo_root, &["ls-files"]),
    }
}

/// Substitute a `{{files}}` placeholder in a shell command string.
pub fn substitute_files(command: &str, files: &[String]) -> String {
    if command.contains("{{files}}") {
        let file_list = files
            .iter()
            .map(|f| shell_escape_if_needed(f))
            .collect::<Vec<_>>()
            .join(" ");
        command.replace("{{files}}", &file_list)
    } else {
        command.to_string()
    }
}

/// Substitute `{{files}}` in an argv vector by expanding to file paths.
pub fn substitute_files_in_argv(argv: &[String], files: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for arg in argv {
        if arg == "{{files}}" {
            out.extend(files.iter().cloned());
        } else {
            out.push(arg.clone());
        }
    }
    out
}

/// Rebase repo-root-relative file paths to be relative to a per-check working directory.
///
/// This intentionally uses `/`-separated paths (as produced by git) rather than
/// platform-specific path separators, so that the resulting paths round-trip
/// cleanly through hooks on all platforms.
pub fn rebase_files_for_cwd(files: &[String], cwd: Option<&str>) -> Vec<String> {
    let Some(cwd) = cwd else {
        return files.to_vec();
    };
    let cwd = cwd.trim().trim_matches('/');
    if cwd.is_empty() {
        return files.to_vec();
    }

    files.iter().map(|f| rebase_path_for_cwd(cwd, f)).collect()
}

fn rebase_path_for_cwd(cwd: &str, path: &str) -> String {
    let cwd_parts: Vec<&str> = cwd
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let path_parts: Vec<&str> = path
        .trim()
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();

    let mut common = 0;
    while common < cwd_parts.len()
        && common < path_parts.len()
        && cwd_parts[common] == path_parts[common]
    {
        common += 1;
    }

    let mut out: Vec<&str> = Vec::new();
    let ups = cwd_parts.len().saturating_sub(common);
    out.extend(std::iter::repeat_n("..", ups));
    out.extend_from_slice(&path_parts[common..]);

    if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/")
    }
}

fn shell_escape_if_needed(s: &str) -> String {
    #[cfg(windows)]
    {
        return cmd_escape_if_needed(s);
    }

    #[cfg(not(windows))]
    {
        posix_escape_if_needed(s)
    }
}

#[cfg(not(windows))]
fn posix_escape_if_needed(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));

    if safe {
        return s.to_string();
    }

    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

#[cfg(windows)]
fn cmd_escape_if_needed(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".to_string();
    }

    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-' | ':'));

    if safe {
        return s.to_string();
    }

    let mut escaped = String::new();
    for ch in s.chars() {
        match ch {
            '^' | '&' | '|' | '<' | '>' | '%' | '!' | '(' | ')' => {
                escaped.push('^');
                escaped.push(ch);
            }
            '"' => escaped.push_str("\\\""),
            _ => escaped.push(ch),
        }
    }
    format!("\"{escaped}\"")
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect())
}

#[cfg(test)]
mod rebase_tests {
    use super::*;

    #[test]
    fn rebase_files_no_cwd_is_identity() {
        let files = vec!["a/b.ts".to_string(), "Cargo.toml".to_string()];
        assert_eq!(rebase_files_for_cwd(&files, None), files);
    }

    #[test]
    fn rebase_files_under_cwd_strip_prefix() {
        let files = vec!["packages/exosuit-vscode/src/a.ts".to_string()];
        let rebased = rebase_files_for_cwd(&files, Some("packages/exosuit-vscode"));
        assert_eq!(rebased, vec!["src/a.ts".to_string()]);
    }

    #[test]
    fn rebase_files_outside_cwd_gets_dotdots() {
        let files = vec!["Cargo.toml".to_string()];
        let rebased = rebase_files_for_cwd(&files, Some("packages/exosuit-vscode"));
        assert_eq!(rebased, vec!["../../Cargo.toml".to_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_files_basic() {
        let cmd = "eslint {{files}}";
        let files = vec!["src/a.ts".to_string(), "src/b.ts".to_string()];
        let result = substitute_files(cmd, &files);
        assert_eq!(result, "eslint src/a.ts src/b.ts");
    }

    #[test]
    fn test_substitute_files_no_placeholder() {
        let cmd = "cargo check";
        let files = vec!["src/lib.rs".to_string()];
        let result = substitute_files(cmd, &files);
        assert_eq!(result, "cargo check");
    }

    #[test]
    fn test_substitute_files_empty_list() {
        let cmd = "eslint {{files}}";
        let files: Vec<String> = vec![];
        let result = substitute_files(cmd, &files);
        assert_eq!(result, "eslint ");
    }

    #[test]
    fn test_substitute_files_quotes_spaces() {
        let cmd = "eslint {{files}}";
        let files = vec!["src/with space.ts".to_string()];
        let result = substitute_files(cmd, &files);
        #[cfg(windows)]
        assert_eq!(result, "eslint \"src/with space.ts\"");
        #[cfg(not(windows))]
        assert_eq!(result, "eslint 'src/with space.ts'");
    }

    #[test]
    fn test_substitute_files_in_argv() {
        let argv = vec![
            "eslint".to_string(),
            "--max-warnings=0".to_string(),
            "{{files}}".to_string(),
        ];
        let files = vec!["src/a.ts".to_string(), "src/b.ts".to_string()];
        let result = substitute_files_in_argv(&argv, &files);
        assert_eq!(
            result,
            vec![
                "eslint".to_string(),
                "--max-warnings=0".to_string(),
                "src/a.ts".to_string(),
                "src/b.ts".to_string(),
            ]
        );
    }
}
