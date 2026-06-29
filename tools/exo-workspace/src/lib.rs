#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::disallowed_methods)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("failed to resolve workspace root from CARGO_MANIFEST_DIR")
}

pub fn run_command(
    program: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
) -> Result<()> {
    let program = program.as_ref();
    let mut command = command_for(program);
    command.args(args).current_dir(cwd);
    let status = command
        .status()
        .with_context(|| format!("failed to run {}", display_command(program, args)))?;
    if !status.success() {
        bail!(
            "command failed with status {status}: {}",
            display_command(program, args)
        );
    }
    Ok(())
}

pub fn run_command_env(
    program: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    envs: &[(&str, &str)],
) -> Result<()> {
    let program = program.as_ref();
    let mut command = command_for(program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied());
    let status = command
        .status()
        .with_context(|| format!("failed to run {}", display_command(program, args)))?;
    if !status.success() {
        bail!(
            "command failed with status {status}: {}",
            display_command(program, args)
        );
    }
    Ok(())
}

pub fn run_command_env_os(
    program: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    envs: &[(OsString, OsString)],
) -> Result<()> {
    let program = program.as_ref();
    let mut command = command_for(program);
    command.args(args).current_dir(cwd).envs(
        envs.iter()
            .map(|(key, value)| (key.as_os_str(), value.as_os_str())),
    );
    let status = command
        .status()
        .with_context(|| format!("failed to run {}", display_command(program, args)))?;
    if !status.success() {
        bail!(
            "command failed with status {status}: {}",
            display_command(program, args)
        );
    }
    Ok(())
}

pub fn output_command(
    program: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
) -> Result<std::process::Output> {
    let program = program.as_ref();
    let mut command = command_for(program);
    command.args(args).current_dir(cwd);
    let output = command
        .output()
        .with_context(|| format!("failed to run {}", display_command(program, args)))?;
    if !output.status.success() {
        bail!(
            "command failed with status {}: {}\n{}",
            output.status,
            display_command(program, args),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

pub fn display_command(program: &OsStr, args: &[impl AsRef<OsStr>]) -> String {
    let mut parts = vec![program.to_string_lossy().to_string()];
    parts.extend(
        args.iter()
            .map(|arg| arg.as_ref().to_string_lossy().to_string()),
    );
    parts.join(" ")
}

fn command_for(program: &OsStr) -> Command {
    #[cfg(windows)]
    {
        let resolved = resolve_windows_program(program);
        let program = resolved.as_deref().unwrap_or_else(|| Path::new(program));
        if is_windows_script(program) {
            let mut command = Command::new("cmd.exe");
            command.arg("/C").arg(program);
            return command;
        }
        Command::new(program)
    }

    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}

#[cfg(windows)]
fn resolve_windows_program(program: &OsStr) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 || path.extension().is_some() {
        return path.is_file().then(|| path.to_path_buf());
    }

    resolve_windows_program_from(
        path,
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
        |candidate| candidate.is_file(),
    )
}

#[cfg(windows)]
fn resolve_windows_program_from(
    program: &Path,
    paths: Option<&OsStr>,
    path_exts: Option<&OsStr>,
    mut is_file: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    let path_exts = path_exts
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|exts| !exts.is_empty())
        .unwrap_or_else(|| {
            vec![
                ".COM".to_string(),
                ".EXE".to_string(),
                ".BAT".to_string(),
                ".CMD".to_string(),
            ]
        });

    let paths = paths?;
    for dir in std::env::split_paths(paths) {
        for ext in &path_exts {
            let candidate = dir.join(format!("{}{}", program.to_string_lossy(), ext));
            if is_file(&candidate) {
                return Some(candidate);
            }
        }

        let direct = dir.join(program);
        if is_file(&direct) {
            return Some(direct);
        }
    }

    None
}

#[cfg(windows)]
fn is_windows_script(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

pub fn cargo_bin_args(bin: &str) -> Vec<OsString> {
    vec![
        "run".into(),
        "-p".into(),
        "exo-workspace".into(),
        "--bin".into(),
        bin.into(),
        "--".into(),
    ]
}

pub fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::copy(src, dest)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dest.display()))?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn windows_program_resolution_prefers_pathext_candidate_over_extensionless_match() {
        let path = OsString::from(r"C:\Tools");
        let path_ext = OsString::from(".COM;.EXE;.BAT;.CMD");
        let direct = PathBuf::from(r"C:\Tools\pnpm");
        let cmd = PathBuf::from(r"C:\Tools\pnpm.CMD");

        let resolved = resolve_windows_program_from(
            Path::new("pnpm"),
            Some(path.as_os_str()),
            Some(path_ext.as_os_str()),
            |candidate| candidate == direct || candidate == cmd,
        );

        assert_eq!(resolved, Some(cmd));
    }
}
