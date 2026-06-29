use crate::api::protocol::{ErrorCode, NextCall, NextCallKind, Priority, Steering};
use serde_json::{Value as JsonValue, json};

const RFC_RECONCILE_CONTEXT: &str = "Failed to reconcile RFC metadata from disk into SQLite";
const RFC_MISSING_ANCHOR: &str = "RFC file missing anchor comment";

#[derive(Debug, Clone)]
pub struct PreloadGuidance {
    pub classification: &'static str,
    pub subsystem: &'static str,
    pub cause: &'static str,
    pub headline: &'static str,
    pub explanation: &'static str,
    pub next_command: &'static str,
    pub retry_command: Option<String>,
    pub diagnostic_command: Option<&'static str>,
    pub affected_path: Option<String>,
    pub error_code: ErrorCode,
}

impl PreloadGuidance {
    #[must_use]
    pub fn message(&self) -> String {
        let mut message = format!("{}\n\n{}", self.headline, self.explanation);

        message.push_str(&format!("\n\n[Next]\n- {}", self.next_command));

        if let Some(retry_command) = &self.retry_command {
            message.push_str(&format!("\n\n[Then]\n- rerun {retry_command}"));
        }

        if let Some(diagnostic_command) = self.diagnostic_command {
            message.push_str(&format!("\n\n[Diagnostic]\n- {diagnostic_command}"));
        }

        if let Some(path) = &self.affected_path {
            message.push_str(&format!("\n\nFirst failing RFC: {path}"));
        }

        message
    }

    #[must_use]
    pub fn details(&self) -> JsonValue {
        let mut details = json!({
            "classification": self.classification,
            "subsystem": self.subsystem,
            "cause": self.cause,
            "next_command": self.next_command,
        });

        if let Some(retry_command) = &self.retry_command {
            details["retry_command"] = json!(retry_command);
        }
        if let Some(diagnostic_command) = self.diagnostic_command {
            details["diagnostic_command"] = json!(diagnostic_command);
        }
        if let Some(path) = &self.affected_path {
            details["affected_path"] = json!(path);
        }

        details
    }

    #[must_use]
    pub fn to_steering(&self) -> Steering {
        let context_note = if let Some(retry_command) = &self.retry_command {
            format!(
                "{} Run `{}`, then rerun {retry_command}.",
                self.headline, self.next_command
            )
        } else {
            format!("{} Run `{}`.", self.headline, self.next_command)
        };

        Steering {
            next_call: NextCall {
                kind: NextCallKind::Call,
                params: json!({
                    "address": {
                        "kind": "operation",
                        "path": ["update"]
                    },
                    "input": {}
                }),
            },
            priority: Some(Priority::Blocking),
            confidence: Some(1.0),
            context_note: Some(context_note),
        }
    }
}

#[must_use]
pub fn classify_context_load_error(
    error: &anyhow::Error,
    original_command: &str,
) -> Option<PreloadGuidance> {
    let messages = error.chain().map(ToString::to_string).collect::<Vec<_>>();
    let has_rfc_reconcile_context = messages
        .iter()
        .any(|message| message.contains(RFC_RECONCILE_CONTEXT));
    let missing_anchor_message = messages
        .iter()
        .find(|message| message.contains(RFC_MISSING_ANCHOR));

    if has_rfc_reconcile_context && missing_anchor_message.is_some() {
        return Some(PreloadGuidance {
            classification: "migration_blocked:rfc_metadata_anchor",
            subsystem: "rfc metadata reconciliation",
            cause: "legacy RFC file missing anchor comment",
            headline: "Workspace context is blocked by a legacy RFC metadata migration.",
            explanation: "Exosuit found an RFC file that still needs the required anchor comment before full workspace context can load.",
            next_command: "exo update",
            retry_command: non_empty_command(original_command),
            diagnostic_command: Some("exo rfc status"),
            affected_path: missing_anchor_message.and_then(|message| extract_after_colon(message)),
            error_code: ErrorCode::PreconditionFailed,
        });
    }

    None
}

fn non_empty_command(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        None
    } else {
        Some(command.to_string())
    }
}

fn extract_after_colon(message: &str) -> Option<String> {
    message
        .split_once(':')
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
