//! Unified diagnostics layer for routing errors (RFC 0132).
//!
//! This module bridges routing diagnostics to protocol-level steering and
//! CLI error formatting, providing consistent error handling across frontends.
//!
//! # Design
//!
//! ```text
//! RoutingDiagnostic ─────┬────► Steering (for tool responses)
//!                        │
//!                        └────► Formatted CLI output (for terminal)
//! ```

use crate::api::protocol::{NextCall, NextCallKind, Priority, Steering};
use crate::command::router::{DiagnosticCode, RoutingDiagnostic, Suggestion};
use crate::steering::{SuggestedAction, WorkIntent};
use serde_json::json;

// ============================================================================
// ANSI Color Constants (no external dependencies)
// ============================================================================

const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";

// ============================================================================
// Conversion Functions
// ============================================================================

/// Convert a routing diagnostic to protocol Steering.
///
/// This allows routing errors to provide actionable next steps to agents
/// via the protocol's steering mechanism.
pub fn routing_diagnostic_to_steering(diag: &RoutingDiagnostic) -> Steering {
    // Use the highest confidence suggestion for next_call
    let (next_call, confidence, priority) = if let Some(suggestion) = diag
        .suggestions
        .iter()
        .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
    {
        let next_call = NextCall {
            kind: NextCallKind::Help,
            params: json!({
                "command": suggestion.replacement,
                "context": diag.message,
            }),
        };
        (
            next_call,
            Some(suggestion.confidence),
            confidence_to_priority(suggestion.confidence),
        )
    } else {
        // No suggestions: guide to help
        let next_call = NextCall {
            kind: NextCallKind::Help,
            params: json!({
                "address": {"kind": "root"},
                "reason": diag.message,
            }),
        };
        (next_call, Some(0.7), Some(Priority::High))
    };

    Steering {
        next_call,
        priority,
        confidence,
        context_note: Some(format!(
            "Routing error ({}): {}",
            diagnostic_code_name(diag.code),
            diag.message
        )),
    }
}

/// Convert a routing suggestion to a `SuggestedAction`.
pub fn suggestion_to_action(suggestion: &Suggestion) -> SuggestedAction {
    SuggestedAction::legacy_exo_surface(
        suggestion.label.clone(),
        suggestion.replacement.clone(),
        format!("Fix routing error: {}", suggestion.label),
        WorkIntent::Execute,
        Some(suggestion.confidence),
    )
}

// ============================================================================
// CLI Error Formatting
// ============================================================================

/// Format a routing diagnostic for human-readable CLI output.
///
/// Uses ANSI escape codes for colors:
/// - Red for error messages
/// - Cyan for location information
/// - Green for suggestions
/// - Yellow for context
pub fn format_diagnostic_human(diag: &RoutingDiagnostic) -> String {
    let mut output = String::new();

    // Error header
    output.push_str(&format!(
        "{}{}error{}{}: {}{}\n",
        BOLD, RED, RESET, RED, diag.message, RESET
    ));

    // Location information
    if let Some(location) = &diag.location {
        output.push_str(&format!("  {CYAN}--> "));
        if let Some(token_idx) = location.token_index {
            output.push_str(&format!("token {token_idx}"));
        }
        if let Some((start, end)) = location.span {
            if location.token_index.is_some() {
                output.push_str(", ");
            }
            output.push_str(&format!("bytes {start}..{end}"));
        }
        output.push_str(&format!("{RESET}\n"));
    }

    // Context information
    if let Some(context) = &diag.context {
        if let Some(path) = &context.path {
            output.push_str(&format!("  {}Path: {}{}\n", YELLOW, path.join(" "), RESET));
        }
        if !context.available.is_empty() {
            output.push_str(&format!(
                "  {}Available: {}{}\n",
                YELLOW,
                context.available.join(", "),
                RESET
            ));
        }
        if let Some(expected) = &context.expected_type {
            output.push_str(&format!("  {YELLOW}Expected: {expected}{RESET}\n"));
        }
        if let Some(actual) = &context.actual_value {
            output.push_str(&format!("  {YELLOW}Got: {actual}{RESET}\n"));
        }
    }

    // Diagnostic code
    output.push_str(&format!(
        "  {}[{}]{}\n",
        CYAN,
        diagnostic_code_name(diag.code),
        RESET
    ));

    // Suggestions
    if !diag.suggestions.is_empty() {
        output.push('\n');
        output.push_str(&format!("{BOLD}Suggestions:{RESET}\n"));
        for (idx, suggestion) in diag.suggestions.iter().enumerate() {
            let confidence_pct = (suggestion.confidence * 100.0) as u32;
            output.push_str(&format!(
                "  {}{}. {}{} ({}{}%{})\n",
                GREEN,
                idx + 1,
                suggestion.label,
                RESET,
                CYAN,
                confidence_pct,
                RESET
            ));
            output.push_str(&format!(
                "     {}$ {}{}\n",
                GREEN, suggestion.replacement, RESET
            ));
        }
    }

    output
}

/// Format a routing diagnostic without colors (for logs/tests).
pub fn format_diagnostic_plain(diag: &RoutingDiagnostic) -> String {
    let mut output = String::new();

    output.push_str(&format!("error: {}\n", diag.message));

    if let Some(location) = &diag.location {
        output.push_str("  --> ");
        if let Some(token_idx) = location.token_index {
            output.push_str(&format!("token {token_idx}"));
        }
        if let Some((start, end)) = location.span {
            if location.token_index.is_some() {
                output.push_str(", ");
            }
            output.push_str(&format!("bytes {start}..{end}"));
        }
        output.push('\n');
    }

    if let Some(context) = &diag.context
        && !context.available.is_empty()
    {
        output.push_str(&format!("  Available: {}\n", context.available.join(", ")));
    }

    output.push_str(&format!("  [{}]\n", diagnostic_code_name(diag.code)));

    if !diag.suggestions.is_empty() {
        output.push_str("\nSuggestions:\n");
        for (idx, suggestion) in diag.suggestions.iter().enumerate() {
            output.push_str(&format!("  {}. {}\n", idx + 1, suggestion.label));
            output.push_str(&format!("     $ {}\n", suggestion.replacement));
        }
    }

    output
}

// ============================================================================
// Integration Trait
// ============================================================================

/// Trait for types that can be converted to diagnostic steering.
pub trait IntoDiagnosticSteering {
    /// Convert to a protocol Steering suggestion.
    fn into_steering(&self) -> Steering;

    /// Convert to a CLI-formatted string with colors.
    fn format_cli(&self) -> String;

    /// Convert to a CLI-formatted string without colors.
    fn format_plain(&self) -> String;
}

impl IntoDiagnosticSteering for RoutingDiagnostic {
    fn into_steering(&self) -> Steering {
        routing_diagnostic_to_steering(self)
    }

    fn format_cli(&self) -> String {
        format_diagnostic_human(self)
    }

    fn format_plain(&self) -> String {
        format_diagnostic_plain(self)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get a human-readable name for a diagnostic code.
const fn diagnostic_code_name(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::UnknownNamespace => "unknown-namespace",
        DiagnosticCode::UnknownOperation => "unknown-operation",
        DiagnosticCode::UnknownFlag => "unknown-flag",
        DiagnosticCode::MissingRequiredArg => "missing-required-arg",
        DiagnosticCode::InvalidArgType => "invalid-arg-type",
        DiagnosticCode::AmbiguousCommand => "ambiguous-command",
        DiagnosticCode::UnsupportedShellFeature => "unsupported-shell-feature",
        DiagnosticCode::TooManyPositionals => "too-many-positionals",
    }
}

/// Determine priority from suggestion confidence.
fn confidence_to_priority(confidence: f32) -> Option<Priority> {
    if confidence >= 0.9 {
        Some(Priority::High)
    } else if confidence >= 0.7 {
        Some(Priority::Normal)
    } else {
        Some(Priority::Low)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::router::DiagnosticContext;

    #[test]
    fn test_routing_diagnostic_to_steering() {
        let diag = RoutingDiagnostic::new(
            DiagnosticCode::UnknownNamespace,
            "Namespace 'foo' not found",
        )
        .with_suggestion(Suggestion::new("Read the plan", "exo plan read", 0.85));

        let steering = routing_diagnostic_to_steering(&diag);

        assert_eq!(steering.next_call.kind, NextCallKind::Help);
        assert!(steering.confidence.unwrap() > 0.8);
        assert!(steering.context_note.is_some());
        assert!(
            steering
                .context_note
                .as_ref()
                .unwrap()
                .contains("unknown-namespace")
        );
    }

    #[test]
    fn test_routing_diagnostic_to_steering_no_suggestions() {
        let diag = RoutingDiagnostic::new(
            DiagnosticCode::MissingRequiredArg,
            "Missing required argument 'name'",
        );

        let steering = routing_diagnostic_to_steering(&diag);

        assert_eq!(steering.next_call.kind, NextCallKind::Help);
        assert_eq!(steering.priority, Some(Priority::High));
        assert!(steering.confidence.unwrap() >= 0.7);
    }

    #[test]
    fn test_suggestion_to_action() {
        let suggestion = Suggestion::new("Use 'plan read' instead", "exo plan read", 0.9);

        let action = suggestion_to_action(&suggestion);

        assert_eq!(action.label, "Use 'plan read' instead");
        assert_eq!(action.command, "exo plan read");
        assert_eq!(action.intent, WorkIntent::Execute);
        assert_eq!(action.confidence, Some(0.9));
    }

    #[test]
    fn test_format_diagnostic_plain_basic() {
        let diag = RoutingDiagnostic::new(
            DiagnosticCode::UnknownOperation,
            "Operation 'foo' not found in namespace 'phase'",
        );

        let formatted = format_diagnostic_plain(&diag);

        assert!(formatted.contains("error:"));
        assert!(formatted.contains("Operation 'foo' not found"));
        assert!(formatted.contains("[unknown-operation]"));
    }

    #[test]
    fn test_format_diagnostic_plain_with_context() {
        let diag = RoutingDiagnostic::new(
            DiagnosticCode::UnknownOperation,
            "Operation 'lis' not found",
        )
        .with_context(DiagnosticContext {
            path: Some(vec!["phase".to_string()]),
            available: vec!["list".to_string(), "start".to_string()],
            expected_type: None,
            actual_value: None,
        });

        let formatted = format_diagnostic_plain(&diag);

        assert!(formatted.contains("Available: list, start"));
    }

    #[test]
    fn test_format_diagnostic_plain_with_suggestions() {
        let diag = RoutingDiagnostic::new(
            DiagnosticCode::UnknownOperation,
            "Operation 'lis' not found",
        )
        .with_suggestion(Suggestion::new("Read the plan", "exo plan read", 0.92));

        let formatted = format_diagnostic_plain(&diag);

        assert!(formatted.contains("Suggestions:"));
        assert!(formatted.contains("Read the plan"));
        assert!(formatted.contains("$ exo plan read"));
    }

    #[test]
    fn test_into_diagnostic_steering_trait() {
        let diag = RoutingDiagnostic::new(DiagnosticCode::UnknownNamespace, "Test error");

        let steering = diag.into_steering();
        let formatted = diag.format_plain();

        assert_eq!(steering.next_call.kind, NextCallKind::Help);
        assert!(formatted.contains("error:"));
        assert!(formatted.contains("Test error"));
    }

    #[test]
    fn test_diagnostic_code_names() {
        assert_eq!(
            diagnostic_code_name(DiagnosticCode::UnknownNamespace),
            "unknown-namespace"
        );
        assert_eq!(
            diagnostic_code_name(DiagnosticCode::UnknownOperation),
            "unknown-operation"
        );
        assert_eq!(
            diagnostic_code_name(DiagnosticCode::UnknownFlag),
            "unknown-flag"
        );
        assert_eq!(
            diagnostic_code_name(DiagnosticCode::MissingRequiredArg),
            "missing-required-arg"
        );
    }

    #[test]
    fn test_confidence_to_priority() {
        assert_eq!(confidence_to_priority(0.95), Some(Priority::High));
        assert_eq!(confidence_to_priority(0.9), Some(Priority::High));
        assert_eq!(confidence_to_priority(0.85), Some(Priority::Normal));
        assert_eq!(confidence_to_priority(0.7), Some(Priority::Normal));
        assert_eq!(confidence_to_priority(0.65), Some(Priority::Low));
    }
}
