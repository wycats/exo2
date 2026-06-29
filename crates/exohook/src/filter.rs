use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

pub fn build_globset(filters: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in filters {
        let glob =
            Glob::new(pattern).with_context(|| format!("invalid filter glob '{pattern}'"))?;
        builder.add(glob);
    }
    builder.build().context("failed to build filter globset")
}

pub fn filter_files(files: &[String], filters: &[String]) -> Result<Vec<String>> {
    if filters.is_empty() {
        return Ok(files.to_vec());
    }

    let set = build_globset(filters)?;
    Ok(files
        .iter()
        .filter(|f| set.is_match(f.as_str()))
        .cloned()
        .collect())
}
