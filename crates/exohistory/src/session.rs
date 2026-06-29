//! Data structures for `VSCode` Copilot chat sessions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Reference to a session file (lightweight, for indexing)
#[derive(Debug, Clone, Serialize)]
pub struct SessionRef {
    pub id: String,
    pub path: PathBuf,
    pub workspace_id: String,
    pub workspace_path: Option<PathBuf>,
    pub created_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub request_count: usize,
}

/// Full session data (loaded on demand)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub version: i32,
    #[serde(rename = "requesterUsername")]
    pub requester_username: Option<String>,
    #[serde(rename = "responderUsername")]
    pub responder_username: Option<String>,
    #[serde(rename = "initialLocation")]
    pub initial_location: Option<String>,
    pub requests: Vec<ChatRequest>,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "creationDate")]
    pub creation_date: Option<i64>,
    #[serde(rename = "lastMessageDate")]
    pub last_message_date: Option<i64>,
    #[serde(rename = "isImported")]
    pub is_imported: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
    pub message: Option<ChatMessage>,
    #[serde(rename = "variableData")]
    pub variable_data: Option<VariableData>,
    #[serde(default)]
    pub response: Vec<ResponsePart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub text: Option<String>,
    pub parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    pub text: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableData {
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePart {
    pub kind: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(rename = "toolName")]
    pub tool_name: Option<String>,
    #[serde(rename = "toolId")]
    pub tool_id: Option<String>,
    #[serde(rename = "invocationMessage", default)]
    pub invocation_message: Option<serde_json::Value>,
    #[serde(rename = "pastTenseMessage", default)]
    pub past_tense_message: Option<serde_json::Value>,
    #[serde(rename = "resultDetails", default)]
    pub result_details: Option<serde_json::Value>,
    #[serde(rename = "isComplete", default)]
    pub is_complete: Option<bool>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Search result within a session
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub session_id: String,
    pub request_index: usize,
    pub match_type: MatchType,
    pub context: String,
    pub matched_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum MatchType {
    Prompt,
    Response,
    ToolInvocation,
}

impl ChatSession {
    /// Search for pattern matches in the session
    pub fn search(
        &self,
        pattern: &regex::Regex,
        prompts_only: bool,
        responses_only: bool,
    ) -> Vec<SearchMatch> {
        let mut matches = Vec::new();

        for (idx, request) in self.requests.iter().enumerate() {
            // Search in prompts
            if !responses_only
                && let Some(ref msg) = request.message
                && let Some(ref text) = msg.text
                && pattern.is_match(text)
            {
                matches.push(SearchMatch {
                    session_id: self.session_id.clone(),
                    request_index: idx,
                    match_type: MatchType::Prompt,
                    context: truncate_context(text, 200),
                    matched_text: extract_match(pattern, text),
                });
            }

            // Search in responses
            if !prompts_only {
                for part in &request.response {
                    if let Some(ref value) = part.value {
                        let text = value_to_string(value);
                        if pattern.is_match(&text) {
                            let match_type = if part.tool_name.is_some() {
                                MatchType::ToolInvocation
                            } else {
                                MatchType::Response
                            };
                            matches.push(SearchMatch {
                                session_id: self.session_id.clone(),
                                request_index: idx,
                                match_type,
                                context: truncate_context(&text, 200),
                                matched_text: extract_match(pattern, &text),
                            });
                        }
                    }
                }
            }
        }

        matches
    }

    /// Extract all tool invocations from the session
    pub fn extract_tool_invocations(&self) -> Vec<ToolInvocation> {
        let mut invocations = Vec::new();

        for (req_idx, request) in self.requests.iter().enumerate() {
            for part in &request.response {
                // Try tool_name first, then fall back to tool_id
                let tool_name = part.tool_name.clone().or_else(|| part.tool_id.clone());

                if let Some(tool_name) = tool_name {
                    let success =
                        part.is_complete.unwrap_or(false) || part.past_tense_message.is_some();
                    let message = extract_string_from_value(part.invocation_message.as_ref())
                        .unwrap_or_else(|| "Unknown".to_string());

                    invocations.push(ToolInvocation {
                        session_id: self.session_id.clone(),
                        request_index: req_idx,
                        tool_name,
                        message,
                        success,
                    });
                }
            }
        }

        invocations
    }

    /// Extract all code blocks from response content.
    pub fn extract_code_blocks(&self) -> Vec<CodeBlock> {
        use std::sync::LazyLock;

        // This is a constant regex that is known to be valid at compile time.
        // Using LazyLock to initialize once, and map_or_else to handle the
        // impossible error case without triggering clippy::panic.
        static CODE_BLOCK_RE: LazyLock<Option<regex::Regex>> =
            LazyLock::new(|| regex::Regex::new(r"```(\w*)\n([\s\S]*?)```").ok());

        let mut blocks = Vec::new();
        let Some(ref code_block_re) = *CODE_BLOCK_RE else {
            return blocks;
        };

        for (req_idx, request) in self.requests.iter().enumerate() {
            for part in &request.response {
                if let Some(serde_json::Value::String(ref text)) = part.value {
                    for cap in code_block_re.captures_iter(text) {
                        let language = cap.get(1).map_or("", |m| m.as_str());
                        let code = cap.get(2).map_or("", |m| m.as_str());

                        if !code.trim().is_empty() {
                            blocks.push(CodeBlock {
                                session_id: self.session_id.clone(),
                                request_index: req_idx,
                                language: if language.is_empty() {
                                    None
                                } else {
                                    Some(language.to_string())
                                },
                                code: code.to_string(),
                                line_count: code.lines().count(),
                            });
                        }
                    }
                }
            }
        }

        blocks
    }
}

/// A code block extracted from a response.
#[derive(Debug, Clone, Serialize)]
pub struct CodeBlock {
    pub session_id: String,
    pub request_index: usize,
    pub language: Option<String>,
    pub code: String,
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolInvocation {
    pub session_id: String,
    pub request_index: usize,
    pub tool_name: String,
    pub message: String,
    pub success: bool,
}

fn truncate_context(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid char boundary at or before max_len
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

fn extract_match(pattern: &regex::Regex, text: &str) -> String {
    pattern
        .find(text)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Extract a string from a `serde_json::Value` that might be a string or object
fn extract_string_from_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(obj) => {
            // Try common string fields
            obj.get("value")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    obj.get("message")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
        }
        other => Some(other.to_string()),
    }
}

fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
