use serde::{Deserialize, Serialize};

/// A parsed workspace-relative file reference.
///
/// Generic infrastructure for turning file paths into structured references
/// with presentation tokens. App-specific parsing (RFC stage detection,
/// artifact identification) should live in the consumer, not here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileRef {
    Directory {
        path: String,
        name: String,
    },
    File {
        path: String,
        name: String,
        ext: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Webview,
    Tree,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresentationTokens {
    /// Stable kind string for TS consumers.
    pub kind: String,
    /// Workspace-relative (normalized) path, never absolute.
    pub path: String,
    /// Semantic codicon id.
    pub icon_id: String,
    pub primary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    pub tooltip: String,
    pub aria_label: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
    #[error("Invalid surface: {0}")]
    InvalidSurface(String),
}
