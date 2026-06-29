use anyhow::Context;
use std::fs;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Copy)]
enum DesiredPermissions {
    #[cfg(unix)]
    UnixMode(u32),
    #[cfg(not(unix))]
    Readonly(bool),
}

pub fn write_stdout(content: &str) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    match out.write_all(content.as_bytes()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e).context("Failed writing to stdout"),
    }
}

pub fn ensure_readonly(path: &Path) -> anyhow::Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    let mut permissions = metadata.permissions();

    #[cfg(unix)]
    {
        let mode = permissions.mode();
        // Strip all write bits (user/group/other).
        let readonly_mode = mode & !0o222;
        if readonly_mode != mode {
            permissions.set_mode(readonly_mode);
            fs::set_permissions(path, permissions).with_context(|| {
                format!("Failed to set read-only permissions on {}", path.display())
            })?;
        }
    }

    #[cfg(not(unix))]
    {
        if !permissions.readonly() {
            permissions.set_readonly(true);
            fs::set_permissions(path, permissions).with_context(|| {
                format!("Failed to set read-only permissions on {}", path.display())
            })?;
        }
    }

    Ok(())
}

pub fn ensure_writable(path: &Path) -> anyhow::Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    let mut permissions = metadata.permissions();

    #[cfg(unix)]
    {
        let mode = permissions.mode();

        // Add user-write bit (minimally) so the file can be edited/removed without
        // fighting permissions.
        let writable_mode = mode | 0o200;
        if writable_mode != mode {
            permissions.set_mode(writable_mode);
            fs::set_permissions(path, permissions).with_context(|| {
                format!("Failed to set writable permissions on {}", path.display())
            })?;
        }
    }

    #[cfg(not(unix))]
    {
        if permissions.readonly() {
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions).with_context(|| {
                format!("Failed to set writable permissions on {}", path.display())
            })?;
        }
    }

    Ok(())
}

pub fn edit_cli_managed_file<F>(path: &Path, edit_op: F) -> anyhow::Result<()>
where
    F: FnOnce(&str) -> anyhow::Result<String>,
{
    edit_file_with_permissions(path, edit_op)?;

    // RFC 0111: Agents are collaborators, not adversaries.
    // We no longer enforce readonly on managed files. Correctness is ensured
    // via file-scoped instructions and LM tool guidance, not filesystem locks.

    Ok(())
}

pub fn edit_file_with_permissions<F>(path: &Path, edit_op: F) -> anyhow::Result<()>
where
    F: FnOnce(&str) -> anyhow::Result<String>,
{
    let mut existed = true;

    // 1. Read (missing files are treated as empty and created on write)
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            existed = false;
            String::new()
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to read {}", path.display()));
        }
    };

    // 2. Edit (in memory)
    let new_content = edit_op(&content)?;

    // 3. Check if content changed
    if content == new_content {
        return Ok(());
    }

    // F023 Diagnostic: Log file modifications for debugging "edits don't persist" issue
    if std::env::var("EXO_TRACE_FILE_EDITS").is_ok() {
        eprintln!(
            "[EXO_TRACE] edit_file_with_permissions: writing to {}",
            path.display()
        );
        // Show a sample of what changed (first difference)
        for (i, (old_line, new_line)) in content.lines().zip(new_content.lines()).enumerate() {
            if old_line != new_line {
                eprintln!(
                    "[EXO_TRACE]   line {}: {:?} -> {:?}",
                    i + 1,
                    old_line,
                    new_line
                );
                break;
            }
        }
    }

    // 4. Ensure parent directory exists for new files.
    if !existed {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        atomic_write(path, new_content.as_bytes(), None)
            .context(format!("Failed to write {}", path.display()))?;
        return Ok(());
    }

    // 5. Handle permissions (existing files)
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();

    #[cfg(unix)]
    let original_permissions = {
        let original_mode = permissions.mode();
        let was_readonly = original_mode & 0o200 == 0;

        if was_readonly {
            permissions.set_mode(original_mode | 0o200);
            fs::set_permissions(path, permissions).context("Failed to set write permissions")?;
        }
        (DesiredPermissions::UnixMode(original_mode), was_readonly)
    };

    #[cfg(not(unix))]
    let original_permissions = {
        let original_readonly = permissions.readonly();
        if original_readonly {
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions).context("Failed to set write permissions")?;
        }
        (
            DesiredPermissions::Readonly(original_readonly),
            original_readonly,
        )
    };

    // 6. Write (atomically) so we never leave a truncated/empty file behind.
    let write_result = atomic_write(path, new_content.as_bytes(), Some(original_permissions.0))
        .context(format!("Failed to write {}", path.display()));

    // 7. Restore permissions
    if original_permissions.1 {
        let mut permissions = fs::metadata(path)?.permissions();
        apply_permissions(&mut permissions, original_permissions.0);
        let _ = fs::set_permissions(path, permissions);
    }

    write_result?;
    Ok(())
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    desired_permissions: Option<DesiredPermissions>,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("Path had no parent directory: {}", path.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Path had non-utf8 filename: {}", path.display()))?;

    // Write into the same directory so persistence can replace atomically.
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}.exo-tmp."))
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;
    let tmp_path = tmp.path().to_path_buf();

    tmp.write_all(bytes)
        .with_context(|| format!("Failed writing temp file {}", tmp_path.display()))?;
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("Failed syncing temp file {}", tmp_path.display()))?;

    // Best-effort: preserve the original permissions on the replaced file.
    if let Some(desired) = desired_permissions {
        let mut perms = tmp
            .as_file()
            .metadata()
            .with_context(|| format!("Failed to stat temp file {}", tmp_path.display()))?
            .permissions();
        apply_permissions(&mut perms, desired);
        let _ = fs::set_permissions(&tmp_path, perms);
    }

    let persist_result = tmp.persist(path).map(drop).map_err(|error| {
        anyhow::Error::new(error.error).context(format!(
            "Failed to persist temp file {} into place at {}",
            tmp_path.display(),
            path.display()
        ))
    });

    if persist_result.is_ok()
        && let Some(desired) = desired_permissions
        && let Ok(metadata) = fs::metadata(path)
    {
        let mut perms = metadata.permissions();
        apply_permissions(&mut perms, desired);
        let _ = fs::set_permissions(path, perms);
    }

    persist_result
}

fn apply_permissions(permissions: &mut fs::Permissions, desired: DesiredPermissions) {
    match desired {
        #[cfg(unix)]
        DesiredPermissions::UnixMode(mode) => permissions.set_mode(mode),
        #[cfg(not(unix))]
        DesiredPermissions::Readonly(readonly) => permissions.set_readonly(readonly),
    }
}

pub fn read_text_input(root: &Path, path: &str) -> anyhow::Result<String> {
    let mut contents = if path == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        let provided = Path::new(path);
        let resolved = if provided.is_absolute() {
            provided.to_path_buf()
        } else {
            root.join(provided)
        };
        fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?
    };

    // Allow trailing newline(s) when reading from files/stdin.
    while contents.ends_with(['\n', '\r']) {
        contents.pop();
    }

    Ok(contents)
}

/// Convert a human-readable string into a kebab-case slug suitable for use as an ID.
///
/// Examples: "My Cool Task" -> "my-cool-task", "RFC: The Plan!" -> "rfc-the-plan"
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;

    for ch in text.chars() {
        let ch = ch.to_ascii_lowercase();

        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !out.is_empty() && !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }

    // Trim trailing dashes.
    while out.ends_with('-') {
        out.pop();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        fs::write(&path, b"{\"old\":true}\n").unwrap();

        atomic_write(&path, b"{\"new\":true}\n", None).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"new\":true}\n");
    }
}
