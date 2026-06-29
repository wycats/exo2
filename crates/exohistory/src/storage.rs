//! Storage discovery and session loading.

use anyhow::{Context, Result, anyhow};
use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::session::{ChatSession, SessionRef};

/// Locates and loads chat sessions from `VSCode` workspace storage.
pub struct ChatStorageLocator {
    storage_root: PathBuf,
}

impl ChatStorageLocator {
    /// Create a new locator with optional custom storage path.
    ///
    /// If no path is provided, uses the default `VSCode` workspace storage location.
    pub fn new(custom_path: Option<PathBuf>) -> Result<Self> {
        let storage_root = if let Some(path) = custom_path {
            path
        } else {
            default_storage_path()?
        };

        if !storage_root.exists() {
            anyhow::bail!("Storage path does not exist: {}", storage_root.display());
        }

        Ok(Self { storage_root })
    }

    /// Discover all chat sessions in storage.
    ///
    /// Optionally filter by workspace path (partial match).
    #[allow(clippy::unnecessary_wraps)] // Result kept for API consistency
    pub fn discover_sessions(&self, workspace_filter: Option<&str>) -> Result<Vec<SessionRef>> {
        let mut sessions = Vec::new();

        // Walk through workspace storage directories
        for entry in WalkDir::new(&self.storage_root)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let workspace_id = entry.file_name().to_str().unwrap_or("").to_string();

            // Check for chatSessions directory
            let chat_sessions_dir = entry.path().join("chatSessions");
            if !chat_sessions_dir.exists() {
                continue;
            }

            // Get workspace path from workspace.json if available
            let workspace_path = read_workspace_path(entry.path());

            // Apply workspace filter if provided (matches workspace name, not full path)
            if let Some(filter) = workspace_filter {
                let matches = workspace_path.as_ref().is_some_and(|p| {
                    // Match on the workspace name (final path component), not full path
                    p.file_name()
                        .is_some_and(|name| name.to_string_lossy().contains(filter))
                });
                if !matches {
                    continue;
                }
            }

            // Load session files
            for session_entry in WalkDir::new(&chat_sessions_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter()
                .filter_map(Result::ok)
            {
                let path = session_entry.path();
                let ext = path.extension().and_then(|e| e.to_str());
                if matches!(ext, Some("json" | "jsonl"))
                    && let Ok(session_ref) =
                        self.create_session_ref(path, &workspace_id, workspace_path.clone())
                {
                    sessions.push(session_ref);
                }
            }
        }

        // Sort by last message date (most recent first)
        sessions.sort_by_key(|session| Reverse(session.last_message_at));

        Ok(sessions)
    }

    /// Discover sessions matching an exact workspace URI.
    /// The URI should match the "folder" or "workspace" value in workspace.json.
    #[allow(clippy::unnecessary_wraps)]
    pub fn discover_sessions_by_uri(&self, target_uri: &str) -> Result<Vec<SessionRef>> {
        let mut sessions = Vec::new();

        // Walk through workspace storage directories
        for entry in WalkDir::new(&self.storage_root)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let workspace_id = entry.file_name().to_str().unwrap_or("").to_string();

            // Check for chatSessions directory
            let chat_sessions_dir = entry.path().join("chatSessions");
            if !chat_sessions_dir.exists() {
                continue;
            }

            // Get raw workspace URI from workspace.json
            let raw_uri = read_workspace_uri(entry.path());

            // Check if the URI matches (exact match)
            if raw_uri.as_ref().is_none_or(|u| u != target_uri) {
                continue;
            }

            // Get workspace path for display
            let workspace_path = raw_uri.as_ref().and_then(|u| parse_any_uri(u).ok());

            // Load session files (including .jsonl format)
            for session_entry in WalkDir::new(&chat_sessions_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter()
                .filter_map(Result::ok)
            {
                let path = session_entry.path();
                let ext = path.extension().and_then(|e| e.to_str());
                if matches!(ext, Some("json" | "jsonl"))
                    && let Ok(session_ref) =
                        self.create_session_ref(path, &workspace_id, workspace_path.clone())
                {
                    sessions.push(session_ref);
                }
            }
        }

        // Sort by last message date (most recent first)
        sessions.sort_by_key(|session| Reverse(session.last_message_at));

        Ok(sessions)
    }

    /// Find sessions matching an ID (partial match).
    pub fn find_session(&self, id: &str) -> Result<Vec<SessionRef>> {
        let all_sessions = self.discover_sessions(None)?;
        let matching: Vec<_> = all_sessions
            .into_iter()
            .filter(|s| s.id.contains(id))
            .collect();

        Ok(matching)
    }

    /// Load full session data from a session reference.
    #[allow(clippy::unused_self)] // Takes &self for API consistency
    pub fn load_session(&self, session_ref: &SessionRef) -> Result<ChatSession> {
        let content = fs::read_to_string(&session_ref.path)
            .with_context(|| format!("Failed to read session: {}", session_ref.path.display()))?;

        // Check if this is a JSONL file (based on extension or content)
        let is_jsonl = session_ref
            .path
            .extension()
            .is_some_and(|ext| ext == "jsonl");

        if is_jsonl {
            return self.load_jsonl_session(&content, session_ref);
        }

        let session: ChatSession = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse session: {}", session_ref.path.display()))?;

        Ok(session)
    }

    /// Load a JSONL-format session by reconstructing from incremental patches.
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)]
    fn load_jsonl_session(&self, content: &str, session_ref: &SessionRef) -> Result<ChatSession> {
        use crate::session::ChatRequest;

        let mut session_id = String::new();
        let mut requests: Vec<ChatRequest> = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let wrapper: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let kind = wrapper
                .get("kind")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(999);

            match kind {
                0 => {
                    // Session header - extract session ID and initial requests
                    if let Some(v) = wrapper.get("v") {
                        session_id = v
                            .get("sessionId")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Load any requests present in the header snapshot
                        if let Some(req_array) = v.get("requests").and_then(|r| r.as_array()) {
                            for req_val in req_array {
                                if let Ok(req) = parse_request_from_value(req_val) {
                                    requests.push(req);
                                }
                            }
                        }
                    }
                }
                2 => {
                    // Array/object assignment - look for request additions
                    let key_path = wrapper.get("k").and_then(|k| k.as_array());
                    let value = wrapper.get("v");

                    if let (Some(keys), Some(val)) = (key_path, value) {
                        // Check if this is adding to "requests" array
                        if keys.first().and_then(|k| k.as_str()) == Some("requests")
                            && keys.len() == 1
                        {
                            // This is setting the entire requests array
                            if let Some(req_array) = val.as_array() {
                                for req_val in req_array {
                                    if let Ok(req) = parse_request_from_value(req_val) {
                                        requests.push(req);
                                    }
                                }
                            }
                        } else if keys.first().and_then(|k| k.as_str()) == Some("requests")
                            && keys.len() == 2
                        {
                            // This might be ["requests", N, ...] - setting a specific request
                            if let Some(idx) = keys.get(1).and_then(serde_json::Value::as_u64) {
                                let idx = idx as usize;
                                // Ensure we have space
                                while requests.len() <= idx {
                                    requests.push(ChatRequest::default());
                                }
                                // Try to parse as a full request
                                if let Ok(req) = parse_request_from_value(val) {
                                    requests[idx] = req;
                                }
                            }
                        } else if keys.first().and_then(|k| k.as_str()) == Some("requests")
                            && keys.len() == 3
                        {
                            // ["requests", N, "response"] - updating response for request N
                            if let (Some(idx), Some(field)) = (
                                keys.get(1).and_then(serde_json::Value::as_u64),
                                keys.get(2).and_then(|k| k.as_str()),
                            ) {
                                let idx = idx as usize;
                                while requests.len() <= idx {
                                    requests.push(ChatRequest::default());
                                }
                                if field == "response"
                                    && let Some(resp_array) = val.as_array()
                                {
                                    requests[idx].response = parse_response_parts(resp_array);
                                }
                            }
                        }
                    }
                }
                _ => {
                    // kind 1 is property updates, kind 12 is something else
                    // For now, we skip these as they're typically metadata updates
                }
            }
        }

        if session_id.is_empty() {
            session_id = session_ref
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
        }

        Ok(ChatSession {
            version: 3,
            requester_username: None,
            responder_username: None,
            initial_location: None,
            requests,
            session_id,
            creation_date: None,
            last_message_date: None,
            is_imported: None,
        })
    }

    #[allow(clippy::unused_self)] // Takes &self for consistency with other methods
    fn create_session_ref(
        &self,
        path: &Path,
        workspace_id: &str,
        workspace_path: Option<PathBuf>,
    ) -> Result<SessionRef> {
        // Quick parse to get metadata without loading full session
        // For large files, we only extract the session ID from the filename
        // and skip heavy metadata parsing
        let file_size = fs::metadata(path).map_or(0, |m| m.len());

        // For files > 10MB, use fast path (just filename-based ID, no metadata)
        if file_size > 10 * 1024 * 1024 {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            return Ok(SessionRef {
                id: session_id,
                path: path.to_path_buf(),
                workspace_id: workspace_id.to_string(),
                workspace_path,
                created_at: None,
                last_message_at: None,
                request_count: 0, // Unknown for large files
            });
        }

        let is_jsonl = path.extension().is_some_and(|ext| ext == "jsonl");
        let content = fs::read_to_string(path)?;

        // For JSONL files, extract the session data from the first line (kind 0)
        let value: serde_json::Value = if is_jsonl {
            // JSONL: first line should be {"kind": 0, "v": {...}}
            let first_line = content.lines().find(|l| !l.trim().is_empty());
            let wrapper: serde_json::Value = first_line
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or(serde_json::Value::Null);

            if wrapper.get("kind").and_then(serde_json::Value::as_u64) == Some(0) {
                wrapper.get("v").cloned().unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        } else {
            serde_json::from_str(&content)?
        };

        let session_id = value
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();

        let creation_date = value
            .get("creationDate")
            .and_then(serde_json::Value::as_i64)
            .and_then(chrono::DateTime::from_timestamp_millis);

        // For JSONL files, use file mtime as last_message_date since header is stale
        let last_message_date = if is_jsonl {
            fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| {
                    use chrono::{DateTime, Utc};
                    let duration = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                    DateTime::<Utc>::from_timestamp(
                        duration.as_secs() as i64,
                        duration.subsec_nanos(),
                    )
                })
        } else {
            value
                .get("lastMessageDate")
                .and_then(serde_json::Value::as_i64)
                .and_then(chrono::DateTime::from_timestamp_millis)
        };

        let request_count = value
            .get("requests")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);

        Ok(SessionRef {
            id: session_id,
            path: path.to_path_buf(),
            workspace_id: workspace_id.to_string(),
            workspace_path,
            created_at: creation_date,
            last_message_at: last_message_date,
            request_count,
        })
    }
}

/// Get the default `VSCode` workspace storage path.
fn default_storage_path() -> Result<PathBuf> {
    let home = dirs_path()?;

    // Try common locations in order
    let candidates = [
        // Linux
        home.join(".config/Code/User/workspaceStorage"),
        // macOS
        home.join("Library/Application Support/Code/User/workspaceStorage"),
        // Windows (via WSL or native)
        home.join("AppData/Roaming/Code/User/workspaceStorage"),
        // Insiders variants
        home.join(".config/Code - Insiders/User/workspaceStorage"),
        home.join("Library/Application Support/Code - Insiders/User/workspaceStorage"),
    ];

    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    anyhow::bail!(
        "Could not find VSCode workspace storage. Tried:\n{}",
        candidates
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn dirs_path() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("USERPROFILE").map(PathBuf::from))
        .map_err(|_| anyhow!("Could not determine home directory"))
}

/// Read workspace path from workspace.json in the storage directory.
fn read_workspace_path(storage_dir: &Path) -> Option<PathBuf> {
    let workspace_json = storage_dir.join("workspace.json");
    let content = fs::read_to_string(workspace_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try folder first (single-folder workspace)
    if let Some(folder) = value.get("folder").and_then(|v| v.as_str())
        && let Ok(uri) = parse_file_uri(folder)
    {
        return Some(uri);
    }

    // Try workspace file path
    if let Some(workspace_file) = value.get("workspace").and_then(|v| v.as_str())
        && let Ok(uri) = parse_file_uri(workspace_file)
    {
        return Some(uri);
    }

    None
}

/// Read the raw workspace URI from workspace.json (without parsing to `PathBuf`).
fn read_workspace_uri(storage_dir: &Path) -> Option<String> {
    let workspace_json = storage_dir.join("workspace.json");
    let content = fs::read_to_string(workspace_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try folder first (single-folder workspace)
    if let Some(folder) = value.get("folder").and_then(|v| v.as_str()) {
        return Some(folder.to_string());
    }

    // Try workspace file path
    if let Some(workspace_file) = value.get("workspace").and_then(|v| v.as_str()) {
        return Some(workspace_file.to_string());
    }

    None
}

/// Parse any supported URI (file:// or vscode-remote://) into a `PathBuf`.
/// Alias for `parse_file_uri` for clarity.
fn parse_any_uri(uri: &str) -> Result<PathBuf> {
    parse_file_uri(uri)
}

/// Parse a file:// or vscode-remote:// URI into a `PathBuf`.
fn parse_file_uri(uri: &str) -> Result<PathBuf> {
    if let Some(path) = uri.strip_prefix("file://") {
        // URL decode
        let decoded = urlencoding_decode(path);
        Ok(PathBuf::from(decoded))
    } else if let Some(rest) = uri.strip_prefix("vscode-remote://") {
        // vscode-remote://attached-container%2B.../path
        // Find the first / after the authority to get the path
        if let Some(path_start) = rest.find('/') {
            let path = &rest[path_start..];
            let decoded = urlencoding_decode(path);
            Ok(PathBuf::from(decoded))
        } else {
            anyhow::bail!("No path in vscode-remote URI: {uri}")
        }
    } else {
        anyhow::bail!("Not a file or vscode-remote URI: {uri}")
    }
}

/// Simple URL decoding (handles %20 -> space, etc.)
/// Works at byte level to properly handle multi-byte UTF-8 sequences.
fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Percent-encodings are ASCII hex digits
            let hex_str = &s[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex_str, 16) {
                decoded.push(byte);
                i += 3;
                continue;
            }
        }

        decoded.push(bytes[i]);
        i += 1;
    }

    // Return valid UTF-8; malformed sequences are replaced
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Parse a `ChatRequest` from a JSONL value.
#[allow(clippy::unnecessary_wraps)]
fn parse_request_from_value(val: &serde_json::Value) -> Result<crate::session::ChatRequest> {
    use crate::session::{ChatMessage, ChatRequest};

    let message = val.get("message").map(|m| ChatMessage {
        text: m.get("text").and_then(|t| t.as_str()).map(String::from),
        parts: None,
    });

    let response = val
        .get("response")
        .and_then(|r| r.as_array())
        .map(|arr| parse_response_parts(arr))
        .unwrap_or_default();

    Ok(ChatRequest {
        request_id: val
            .get("requestId")
            .and_then(|r| r.as_str())
            .map(String::from),
        message,
        variable_data: None,
        response,
    })
}

/// Parse response parts from a JSONL response array.
fn parse_response_parts(resp_array: &[serde_json::Value]) -> Vec<crate::session::ResponsePart> {
    use crate::session::ResponsePart;

    resp_array
        .iter()
        .filter_map(|part| {
            // ResponsePart is a struct, not an enum
            // Just deserialize directly
            serde_json::from_value::<ResponsePart>(part.clone()).ok()
        })
        .collect()
}
