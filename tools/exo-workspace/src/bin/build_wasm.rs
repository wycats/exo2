#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const WASM_BINDGEN_VERSION: &str = "0.2.106";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

fn main() -> Result<()> {
    let root = exo_workspace::workspace_root()?;
    ensure_wasm_target(&root)?;
    let wasm_bindgen = ensure_wasm_bindgen(&root)?;

    for package in ["exosuit-reactivity", "exosuit-file-refs", "exosuit-ulid"] {
        exo_workspace::run_command(
            "cargo",
            &[
                "build",
                "-p",
                package,
                "--target",
                WASM_TARGET,
                "--features",
                "wasm",
                "--release",
            ],
            &root,
        )?;
    }

    let out_dir = root.join("packages/exosuit-vscode/src/wasm");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;

    for artifact in ["exosuit_reactivity", "exosuit_file_refs", "exosuit_ulid"] {
        let wasm_path = root
            .join("target")
            .join(WASM_TARGET)
            .join("release")
            .join(format!("{artifact}.wasm"));
        run_wasm_bindgen(&root, &wasm_path, &out_dir, &wasm_bindgen)?;
    }

    println!("WASM bindings generated in {}", out_dir.display());
    Ok(())
}

fn ensure_wasm_target(root: &Path) -> Result<()> {
    let output = exo_workspace::output_command("rustup", &["target", "list", "--installed"], root)?;
    let installed = String::from_utf8(output.stdout).context("rustup output was not utf-8")?;
    if installed.lines().any(|line| line.trim() == WASM_TARGET) {
        return Ok(());
    }

    exo_workspace::run_command("rustup", &["target", "add", WASM_TARGET], root)
}

fn ensure_wasm_bindgen(root: &Path) -> Result<PathBuf> {
    if let Some(path) = find_matching_wasm_bindgen(root)? {
        return Ok(path);
    }

    exo_workspace::run_command(
        "cargo",
        &[
            "install",
            "wasm-bindgen-cli",
            "--version",
            WASM_BINDGEN_VERSION,
            "--locked",
            "--no-default-features",
        ],
        root,
    )?;

    let installed_path =
        cargo_wasm_bindgen_path().context("failed to resolve Cargo bin directory")?;
    if wasm_bindgen_matches(root, &installed_path)? {
        Ok(installed_path)
    } else {
        bail!("wasm-bindgen {WASM_BINDGEN_VERSION} was not found after installation")
    }
}

fn find_matching_wasm_bindgen(root: &Path) -> Result<Option<PathBuf>> {
    for path in find_wasm_bindgen_candidates() {
        if wasm_bindgen_matches(root, &path)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn wasm_bindgen_matches(root: &Path, path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let output = exo_workspace::output_command(path, &["--version"], root)?;
    let version = String::from_utf8(output.stdout).context("wasm-bindgen output was not utf-8")?;
    Ok(version.trim() == format!("wasm-bindgen {WASM_BINDGEN_VERSION}"))
}

fn run_wasm_bindgen(
    root: &Path,
    wasm_path: &Path,
    out_dir: &Path,
    wasm_bindgen: &Path,
) -> Result<()> {
    let args: Vec<OsString> = vec![
        wasm_path.as_os_str().into(),
        "--out-dir".into(),
        out_dir.as_os_str().into(),
        "--target".into(),
        "web".into(),
        "--typescript".into(),
    ];
    exo_workspace::run_command(wasm_bindgen, &args, root)
}

fn find_wasm_bindgen_candidates() -> Vec<PathBuf> {
    let binary = if cfg!(windows) {
        "wasm-bindgen.exe"
    } else {
        "wasm-bindgen"
    };

    let mut candidates = Vec::new();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(binary);
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }

    if let Some(path) = cargo_wasm_bindgen_path() {
        if path.is_file() && !candidates.iter().any(|candidate| candidate == &path) {
            candidates.push(path);
        }
    }

    candidates
}

fn cargo_wasm_bindgen_path() -> Option<PathBuf> {
    let binary = if cfg!(windows) {
        "wasm-bindgen.exe"
    } else {
        "wasm-bindgen"
    };

    cargo_home().map(|home| home.join("bin").join(binary))
}

fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".cargo"))
        })
}
