#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let root = exo_workspace::workspace_root()?;
    check_command_spec(&root)?;
    check_lm_tools(&root)?;
    println!("Generated artifacts are up to date.");
    Ok(())
}

fn check_command_spec(root: &Path) -> Result<()> {
    let temp_file = temp_artifact_path("command-spec.json");
    let args: Vec<OsString> = vec![
        "run".into(),
        "-p".into(),
        "exo".into(),
        "--quiet".into(),
        "--bin".into(),
        "exo".into(),
        "--".into(),
        "--direct".into(),
        "json".into(),
        "artifact".into(),
        "--output".into(),
        temp_file.as_os_str().into(),
    ];
    exo_workspace::run_command("cargo", &args, root)?;

    let target = root.join("packages/exosuit-vscode/src/command-spec.json");
    let generated = std::fs::read_to_string(&temp_file)
        .with_context(|| format!("failed to read {}", temp_file.display()))?;
    let committed = std::fs::read_to_string(&target)
        .with_context(|| format!("failed to read {}", target.display()))?;
    let _ = std::fs::remove_file(&temp_file);

    if normalize_newlines(&generated) != normalize_newlines(&committed) {
        bail!(
            "{} is out of date; run `cargo run -p exo --bin exo -- --direct json artifact`",
            target.display()
        );
    }

    Ok(())
}

fn check_lm_tools(root: &Path) -> Result<()> {
    exo_workspace::run_command("node", &["scripts/sync-lm-tools.ts", "--check"], root)
}

fn temp_artifact_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("exo-{name}-{}-{nanos}", std::process::id()))
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}
