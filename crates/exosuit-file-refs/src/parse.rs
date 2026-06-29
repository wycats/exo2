use crate::model::FileRef;
use std::path::{Path, PathBuf};

pub fn normalize_slashes(s: &str) -> String {
    s.replace('\\', "/")
}

fn is_windows_path_like(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || bytes.starts_with(b"//")
}

pub fn workspace_relative_path(workspace_root: &str, input_path: &str) -> String {
    let root_norm = normalize_slashes(workspace_root);
    let root_norm = root_norm.trim_end_matches('/');
    let input_norm = normalize_slashes(input_path);

    if !root_norm.is_empty() {
        let root_prefix = format!("{root_norm}/");
        if is_windows_path_like(root_norm) || is_windows_path_like(&input_norm) {
            if input_norm.eq_ignore_ascii_case(root_norm) {
                return String::new();
            }
            if let Some(prefix) = input_norm.get(..root_prefix.len()) {
                if prefix.eq_ignore_ascii_case(&root_prefix) {
                    if let Some(rel) = input_norm.get(root_prefix.len()..) {
                        return rel.to_string();
                    }
                }
            }
        } else {
            if input_norm == root_norm {
                return String::new();
            }
            if let Some(rel) = input_norm.strip_prefix(&root_prefix) {
                return rel.to_string();
            }
        }
    }

    let root = Path::new(workspace_root);
    let input = Path::new(input_path);

    let rel: PathBuf = if input.is_absolute() {
        input.strip_prefix(root).unwrap_or(input).to_path_buf()
    } else {
        input.to_path_buf()
    };

    normalize_slashes(&rel.to_string_lossy())
}

fn basename(path: &str) -> String {
    normalize_slashes(path)
        .split('/')
        .next_back()
        .unwrap_or(path)
        .to_string()
}

/// Parse a file path into a generic FileRef (Directory or File).
///
/// App-specific classification (RFC detection, artifact identification)
/// should be done by the consumer after calling this function.
pub fn parse_file_ref(workspace_root: &str, input_path: &str) -> FileRef {
    let rel_path = workspace_relative_path(workspace_root, input_path);

    if rel_path.ends_with('/') {
        return FileRef::Directory {
            name: basename(rel_path.trim_end_matches('/')),
            path: rel_path,
        };
    }

    let name = basename(&rel_path);
    let ext = name.rsplit('.').next().unwrap_or("").to_string();

    FileRef::File {
        path: rel_path,
        name,
        ext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_absolute_paths_under_root() {
        let r = parse_file_ref("/repo", "/repo/docs/rfcs/stage-0/0059-hello.md");
        match r {
            FileRef::File { path, .. } => assert_eq!(path, "docs/rfcs/stage-0/0059-hello.md"),
            _ => panic!("expected file"),
        }
    }

    #[test]
    fn normalizes_windows_paths_under_root_case_insensitively() {
        let r = parse_file_ref("C:\\Repo", "c:\\repo\\docs\\rfcs\\stage-0\\0059-hello.md");
        match r {
            FileRef::File { path, .. } => assert_eq!(path, "docs/rfcs/stage-0/0059-hello.md"),
            _ => panic!("expected file"),
        }
    }

    #[test]
    fn parses_directories() {
        let r = parse_file_ref("/repo", "docs/rfcs/stage-1/");
        assert!(matches!(r, FileRef::Directory { .. }));
    }

    #[test]
    fn extracts_extension() {
        let r = parse_file_ref("/repo", "docs/agent-context/epochs.sql");
        match r {
            FileRef::File { ext, .. } => assert_eq!(ext, "sql"),
            _ => panic!("expected file"),
        }
    }
}
