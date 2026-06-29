#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

fn main() -> Result<()> {
    println!("Verifying TOML files...");

    let provided: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect();

    let files = if provided.is_empty() {
        let mut files = Vec::new();
        collect_toml_files(Path::new("."), &mut files)?;
        files
    } else {
        provided
    };

    if files.is_empty() {
        println!("No TOML files found.");
        return Ok(());
    }

    let mut invalid = Vec::new();
    for file in &files {
        if !file.exists() {
            continue;
        }
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        if let Err(error) = toml::from_str::<toml::Table>(&content) {
            invalid.push((file.clone(), error.to_string()));
        }
    }

    if invalid.is_empty() {
        println!("Verified {} TOML files.", files.len());
        return Ok(());
    }

    for (file, error) in &invalid {
        eprintln!("{} is invalid:", file.display());
        eprintln!("{error}");
    }
    anyhow::bail!("TOML verification failed");
}

fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if matches!(file_name.as_ref(), ".git" | "node_modules" | "target") {
                continue;
            }
            collect_toml_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            out.push(path);
        }
    }
    Ok(())
}
