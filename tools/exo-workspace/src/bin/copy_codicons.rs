#![allow(missing_docs)]
#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use anyhow::Result;

fn main() -> Result<()> {
    let package_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    let codicons_dir = package_dir.join("node_modules/@vscode/codicons/dist");
    let media_dir = package_dir.join("media");

    exo_workspace::copy_file(
        &codicons_dir.join("codicon.css"),
        &media_dir.join("codicon.css"),
    )?;
    exo_workspace::copy_file(
        &codicons_dir.join("codicon.ttf"),
        &media_dir.join("codicon.ttf"),
    )?;

    println!("Copied codicons into {}", media_dir.display());
    Ok(())
}
