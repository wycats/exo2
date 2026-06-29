use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct ExosuitConfig {
    #[serde(default)]
    pub tdd: TddConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct TddConfig {
    #[serde(default)]
    pub runners: Vec<RunnerConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RunnerConfig {
    pub glob: String,
    pub command: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
}

fn default_cwd() -> String {
    "root".to_string()
}

impl ExosuitConfig {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("exosuit.toml");
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path).context("Failed to read exosuit.toml")?;

        // Use toml crate for deserialization (toml_edit is for editing)
        // But wait, Cargo.toml has 'toml = "0.9.8"'.
        // Let's check if we can use toml::from_str.
        let config: Self = toml::from_str(&content).context("Failed to parse exosuit.toml")?;

        Ok(config)
    }
}
