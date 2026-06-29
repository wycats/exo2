use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Debug)]
pub struct Task {
    pub cmd: String,
    pub desc: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
}

fn default_cwd() -> String {
    "root".to_string()
}

#[derive(Deserialize, Debug)]
pub struct ExosuitConfig {
    pub tasks: HashMap<String, Task>,
}

pub fn load_config(root: &Path) -> Result<ExosuitConfig> {
    let candidates = [
        root.join("exosuit.toml"),
        root.join(".config/exo/exosuit.toml"),
    ];

    let Some((config_path, content)) = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok().map(|c| (p, c)))
    else {
        // Fallback: if no exosuit.toml, return empty config instead of error.
        // This allows `exo run --list` to succeed (returning empty) or fail gracefully,
        // and allows other tools to not crash.
        return Ok(ExosuitConfig {
            tasks: HashMap::new(),
        });
    };

    let config: ExosuitConfig = toml::from_str(&content).with_context(|| {
        format!(
            "Failed to parse exosuit config file: {}",
            config_path.display()
        )
    })?;

    Ok(config)
}

pub fn list_tasks(root: &Path) -> Result<()> {
    let config = load_config(root)?;

    if config.tasks.is_empty() {
        println!("No tasks defined in exosuit.toml");
        return Ok(());
    }

    println!("Available tasks:");
    // Sort tasks for consistent output
    let mut tasks: Vec<_> = config.tasks.iter().collect();
    tasks.sort_by_key(|(name, _)| *name);

    for (name, task) in tasks {
        println!("  - {}: {}", name, task.desc);
    }

    Ok(())
}

pub fn run_task(root: &Path, task_name: &str) -> Result<()> {
    let config = load_config(root)?;

    let task = config
        .tasks
        .get(task_name)
        .with_context(|| format!("Task '{task_name}' not found"))?;

    println!("Running task: {task_name}");
    println!("Command: {}", task.cmd);

    let cwd = if task.cwd == "root" {
        root.to_path_buf()
    } else {
        root.join(&task.cwd)
    };

    let status = Command::new("sh")
        .arg("-c")
        .arg(&task.cmd)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("Failed to execute command: {}", task.cmd))?;

    if !status.success() {
        anyhow::bail!("Task failed with status: {status}");
    }

    Ok(())
}
