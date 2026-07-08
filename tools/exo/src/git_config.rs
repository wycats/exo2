use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_INCLUDE_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitConfigEntry {
    section: String,
    subsection: Option<String>,
    key: String,
    value: String,
}

impl GitConfigEntry {
    fn matches(&self, section: &str, subsection: Option<&str>, key: &str) -> bool {
        self.section.eq_ignore_ascii_case(section)
            && self.subsection.as_deref() == subsection
            && self.key.eq_ignore_ascii_case(key)
    }
}

#[cfg(test)]
pub(crate) fn read_git_config(path: &Path) -> io::Result<Vec<GitConfigEntry>> {
    parse_git_config(path).map(|(entries, _)| entries)
}

pub(crate) fn read_repo_git_config(
    repo_root: &Path,
    path: &Path,
) -> io::Result<Vec<GitConfigEntry>> {
    let (entries, has_conditional_include) = parse_git_config(path)?;
    if has_conditional_include {
        read_effective_local_git_config(repo_root)
    } else {
        Ok(entries)
    }
}

fn parse_git_config(path: &Path) -> io::Result<(Vec<GitConfigEntry>, bool)> {
    let mut entries = Vec::new();
    let mut active_includes = BTreeSet::new();
    let mut has_conditional_include = false;
    read_git_config_into(
        path,
        &mut active_includes,
        0,
        &mut entries,
        &mut has_conditional_include,
    )?;
    Ok((entries, has_conditional_include))
}

pub(crate) fn last_value<'a>(
    entries: &'a [GitConfigEntry],
    section: &str,
    subsection: Option<&str>,
    key: &str,
) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .find(|entry| entry.matches(section, subsection, key))
        .map(|entry| entry.value.as_str())
}

pub(crate) fn subsections_with_key(
    entries: &[GitConfigEntry],
    section: &str,
    key: &str,
) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|entry| {
            entry.section.eq_ignore_ascii_case(section) && entry.key.eq_ignore_ascii_case(key)
        })
        .filter_map(|entry| entry.subsection.clone())
        .collect()
}

fn read_git_config_into(
    path: &Path,
    active_includes: &mut BTreeSet<PathBuf>,
    depth: usize,
    entries: &mut Vec<GitConfigEntry>,
    has_conditional_include: &mut bool,
) -> io::Result<()> {
    if depth >= MAX_INCLUDE_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "git config include depth exceeded while reading {}",
                path.display()
            ),
        ));
    }

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !active_includes.insert(canonical.clone()) {
        return Ok(());
    }

    let result = read_git_config_contents(
        path,
        active_includes,
        depth,
        entries,
        has_conditional_include,
    );
    active_includes.remove(&canonical);
    result
}

fn read_git_config_contents(
    path: &Path,
    active_includes: &mut BTreeSet<PathBuf>,
    depth: usize,
    entries: &mut Vec<GitConfigEntry>,
    has_conditional_include: &mut bool,
) -> io::Result<()> {
    let config = std::fs::read_to_string(path)?;
    let mut current_section: Option<(String, Option<String>)> = None;

    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            current_section = parse_section_header(line);
            continue;
        }

        let Some((key, value)) = parse_key_value(line) else {
            continue;
        };
        let Some((section, subsection)) = current_section.as_ref() else {
            continue;
        };

        if section.eq_ignore_ascii_case("include")
            && subsection.is_none()
            && key.eq_ignore_ascii_case("path")
        {
            let include_path = resolve_include_path(path, value);
            read_git_config_into(
                &include_path,
                active_includes,
                depth + 1,
                entries,
                has_conditional_include,
            )?;
            continue;
        }
        if section.eq_ignore_ascii_case("includeif")
            && subsection.is_some()
            && key.eq_ignore_ascii_case("path")
        {
            *has_conditional_include = true;
            continue;
        }

        entries.push(GitConfigEntry {
            section: section.clone(),
            subsection: subsection.clone(),
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    Ok(())
}

fn read_effective_local_git_config(repo_root: &Path) -> io::Result<Vec<GitConfigEntry>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", "--includes", "--null", "--list"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git config --local --includes failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(parse_effective_entry)
        .collect()
}

fn parse_effective_entry(record: &[u8]) -> io::Result<GitConfigEntry> {
    let record = std::str::from_utf8(record)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    record.and_then(|record| {
        let (name, value) = record.split_once('\n').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git config entry did not contain a value separator",
            )
        })?;
        let (section, remainder) = name.split_once('.').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git config entry did not contain a section separator",
            )
        })?;
        let (subsection, key) = remainder
            .rsplit_once('.')
            .map_or((None, remainder), |(subsection, key)| {
                (Some(subsection.to_string()), key)
            });
        Ok(GitConfigEntry {
            section: section.to_string(),
            subsection,
            key: key.to_string(),
            value: value.to_string(),
        })
    })
}

fn parse_section_header(line: &str) -> Option<(String, Option<String>)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let subsection_start = inner.find(char::is_whitespace);
    let Some(subsection_start) = subsection_start else {
        return Some((inner.to_string(), None));
    };

    let section = inner[..subsection_start].trim();
    let subsection = inner[subsection_start..].trim();
    let subsection = subsection.strip_prefix('"')?.strip_suffix('"')?;
    Some((section.to_string(), Some(subsection.to_string())))
}

fn parse_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

fn resolve_include_path(config_path: &Path, value: &str) -> PathBuf {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    let include_path = PathBuf::from(value);
    if include_path.is_absolute() {
        include_path
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(include_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_value_wins_across_repeated_keys_and_sections() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let config = temp.path().join("config");
        std::fs::write(
            &config,
            "[merge \"exo\"]\n\tdriver = false\n\tdriver = stale\n[merge \"exo\"]\n\tdriver = true\n",
        )
        .expect("write config");

        let entries = read_git_config(&config).expect("read config");
        assert_eq!(
            last_value(&entries, "merge", Some("exo"), "driver"),
            Some("true")
        );
    }

    #[test]
    fn include_contents_are_evaluated_at_the_include_position() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let config = temp.path().join("config");
        let include = temp.path().join("branch.inc");
        std::fs::write(&include, "[branch \"main\"]\n\tremote = included\n")
            .expect("write include");
        std::fs::write(
            &config,
            "[branch \"main\"]\n\tremote = before\n[include]\n\tpath = branch.inc\n[branch \"main\"]\n\tremote = after\n",
        )
        .expect("write config");

        let entries = read_git_config(&config).expect("read config");
        assert_eq!(
            last_value(&entries, "branch", Some("main"), "remote"),
            Some("after")
        );
    }
}
