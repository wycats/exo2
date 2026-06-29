use crate::model::{FileRef, PresentationTokens, Surface};
use crate::parse::parse_file_ref;

fn icon_id(file_ref: &FileRef) -> &'static str {
    match file_ref {
        FileRef::Directory { .. } => "folder",
        FileRef::File { .. } => "file",
    }
}

pub fn present_file_ref(file_ref: &FileRef, _surface: Surface) -> PresentationTokens {
    match file_ref {
        FileRef::Directory { path, name } => PresentationTokens {
            kind: "directory".to_string(),
            path: path.clone(),
            icon_id: icon_id(file_ref).to_string(),
            primary: name.clone(),
            secondary: None,
            badge: None,
            tooltip: path.clone(),
            aria_label: format!("Directory {name}"),
        },
        FileRef::File { path, name, .. } => PresentationTokens {
            kind: "file".to_string(),
            path: path.clone(),
            icon_id: icon_id(file_ref).to_string(),
            primary: name.clone(),
            secondary: None,
            badge: None,
            tooltip: path.clone(),
            aria_label: format!("File {name}"),
        },
    }
}

pub fn present_paths(
    workspace_root: &str,
    surface: Surface,
    input_paths: impl IntoIterator<Item = String>,
) -> Vec<PresentationTokens> {
    input_paths
        .into_iter()
        .map(|p| {
            let file_ref = parse_file_ref(workspace_root, &p);
            present_file_ref(&file_ref, surface)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presents_generic_file_tokens() {
        let tokens = present_paths(
            "/repo",
            Surface::Webview,
            vec!["docs/rfcs/stage-2/12345-some-title.md".to_string()],
        );
        assert_eq!(tokens[0].kind, "file");
        assert_eq!(tokens[0].primary, "12345-some-title.md");
        assert_eq!(tokens[0].icon_id, "file");
        assert!(tokens[0].badge.is_none());
    }
}
