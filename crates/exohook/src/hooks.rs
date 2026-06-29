use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::read_hooks_doc;

pub(crate) fn git_repo_root() -> Result<PathBuf> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;
    if !out.status.success() {
        return Err(anyhow!("not a git repository"));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let root = s.trim();
    if root.is_empty() {
        return Err(anyhow!("git rev-parse returned empty root"));
    }
    Ok(PathBuf::from(root))
}

pub(crate) fn hooks_install() -> Result<()> {
    let repo_root = git_repo_root()?;

    // Best-effort: read projections from hooks.toml if present.
    let config_path = repo_root.join(".config/exo/hooks.toml");
    let mut pre_commit_lane = "coherence".to_string();
    let mut pre_push_lane = "gate".to_string();
    if config_path.exists() {
        let doc = read_hooks_doc(&config_path)?;
        if let Some(hooks) = doc
            .get("projections")
            .and_then(toml_edit::Item::as_table)
            .and_then(|p| p.get("git_hooks"))
            .and_then(toml_edit::Item::as_table)
        {
            if let Some(s) = hooks.get("pre_commit").and_then(toml_edit::Item::as_str) {
                pre_commit_lane = s.to_string();
            }
            if let Some(s) = hooks.get("pre_push").and_then(toml_edit::Item::as_str) {
                pre_push_lane = s.to_string();
            }
        }
    }

    let hooks_dir = repo_root.join(".git/hooks");
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("failed to create {}", hooks_dir.display()))?;

    write_hook_shim(
        &hooks_dir.join("pre-commit"),
        "pre-commit",
        &pre_commit_lane,
    )?;
    write_hook_shim(&hooks_dir.join("pre-push"), "pre-push", &pre_push_lane)?;

    println!("Installed git hooks into {}", hooks_dir.display());
    Ok(())
}

pub(crate) fn write_hook_shim(path: &Path, hook_name: &str, lane: &str) -> Result<()> {
    let script = format!(
        "#!/usr/bin/env bash\n\
set -euo pipefail\n\
root=\"$(git rev-parse --show-toplevel 2>/dev/null)\"\n\
if [[ -z \"$root\" ]]; then\n\
  echo \"exohook: {hook_name}: not a git repository\" >&2\n\
  exit 1\n\
fi\n\
cd \"$root\"\n\
lane=\"{lane}\"\n\
if [[ -x \"$root/target/release/exohook\" ]]; then\n\
  exec \"$root/target/release/exohook\" validate \"$lane\"\n\
elif [[ -x \"$root/target/debug/exohook\" ]]; then\n\
  exec \"$root/target/debug/exohook\" validate \"$lane\"\n\
elif command -v exohook >/dev/null 2>&1; then\n\
  exec exohook validate \"$lane\"\n\
elif command -v cargo >/dev/null 2>&1; then\n\
  exec cargo run -q -p exohook -- validate \"$lane\"\n\
else\n\
  echo \"exohook: {hook_name}: could not find exohook (or cargo)\" >&2\n\
  exit 1\n\
fi\n"
    );

    fs::write(path, script).with_context(|| format!("failed to write {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}
