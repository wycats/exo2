//! Workspace-local binary re-exec for exo-family tools.
//!
//! When an exo binary starts, it checks `exosuit.toml` for a `[dev] binary_dir`
//! setting. If a different binary exists at that path, it re-execs to it.
//! This ensures the system-installed binary always delegates to the workspace-local
//! build, eliminating stale binary drift.
//!
//! See RFC 10179 for the full design.
//!
//! # Usage
//!
//! Call `maybe_reexec()` as the very first line of `main()`:
//!
//! ```ignore
//! exo_reexec::maybe_reexec();
//! // ... rest of main
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Minimal config struct — only reads `[dev]` from `exosuit.toml`.
#[derive(Debug, Deserialize, Default)]
struct ExosuitDevConfig {
    dev: Option<DevSection>,
}

#[derive(Debug, Deserialize, Default)]
struct DevSection {
    binary_dir: Option<String>,
}

/// Walk up from `start` looking for `exosuit.toml`.
///
/// Returns the directory containing `exosuit.toml`, or `None` if not found.
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join("exosuit.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Check for a workspace-local binary and re-exec to it if found.
///
/// This should be called as the very first thing in `main()`, before any
/// argument parsing. If a workspace-local binary is found, this function
/// does not return — it replaces the current process via `execv`.
///
/// If no workspace-local binary is found (or if re-exec is suppressed),
/// this function returns normally and the current binary continues.
pub fn maybe_reexec() {
    // Re-exec is Unix-only (uses execv to replace the process).
    // Stage 4 criteria: Windows support via Command::spawn + exit.
    #[cfg(not(unix))]
    return;

    #[cfg(unix)]
    maybe_reexec_unix();
}

#[cfg(unix)]
fn maybe_reexec_unix() {
    // Loop prevention layer 1: env var gate
    if std::env::var("EXO_NO_REEXEC").is_ok() {
        return;
    }

    let Some(cwd) = std::env::current_dir().ok() else {
        return;
    };

    let Some(root) = find_workspace_root(&cwd) else {
        return;
    };

    // Read [dev] section from exosuit.toml
    let config_path = root.join("exosuit.toml");
    #[allow(clippy::disallowed_methods)] // Pre-main sync utility, no async runtime
    let Ok(content) = std::fs::read_to_string(&config_path) else {
        return;
    };
    let Ok(config) = toml::from_str::<ExosuitDevConfig>(&content) else {
        return;
    };

    let Some(binary_dir) = config.dev.as_ref().and_then(|d| d.binary_dir.as_ref()) else {
        return;
    };

    // Reject absolute paths (simplicity constraint, not security boundary)
    if Path::new(binary_dir).is_absolute() {
        return;
    }

    // Get the current binary's file name
    let Some(binary_name) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_os_string()))
    else {
        return;
    };

    let candidate = root.join(binary_dir).join(&binary_name);

    if !candidate.exists() || !candidate.is_file() {
        return;
    }

    // Verify candidate is within workspace root
    let Ok(canonical_candidate) = candidate.canonicalize() else {
        return;
    };
    let Ok(canonical_root) = root.canonicalize() else {
        return;
    };
    if !canonical_candidate.starts_with(&canonical_root) {
        return;
    }

    // Loop prevention layer 2: don't re-exec to ourselves
    if let Ok(current) = std::env::current_exe().and_then(|p| p.canonicalize()) {
        if current == canonical_candidate {
            return;
        }
    }

    // Re-exec — replaces the current process
    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[allow(clippy::disallowed_methods)] // Pre-main sync utility, no async runtime
    let err = std::process::Command::new(&candidate)
        .args(&args)
        .env("EXO_NO_REEXEC", "1")
        .exec();
    // exec() only returns on error
    eprintln!("exo: re-exec to {} failed: {err}", candidate.display());
}
