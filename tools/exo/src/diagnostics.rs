use serde::{Deserialize, Serialize};

use crate::shell_ops::{ShellOperatorHit, ShellOperatorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    ShellOperator,
    UnknownFlag,
    InvalidFlag,
    MissingValue,
    InvalidValue,
    TooManyPositionals,
    MissingRequired,
    NonRepeatable,
    AmbiguousSubcommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Index into argv associated with this diagnostic.
    pub arg_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// Concrete suggestions for fixing this diagnostic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<Suggestion>,
}

/// A concrete suggestion for fixing a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Human-readable description of what this suggestion does.
    pub label: String,
    /// The replacement text or command to apply.
    pub replacement: String,
}

impl Diagnostic {
    /// Create a new diagnostic without suggestions.
    #[must_use]
    pub fn new(code: DiagnosticCode, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            code,
            message: message.into(),
            span,
            suggestions: Vec::new(),
        }
    }

    #[must_use]
    pub fn strip_spans(&self) -> Self {
        Self {
            code: self.code,
            message: self.message.clone(),
            span: None,
            suggestions: self.suggestions.clone(),
        }
    }

    /// Create a new diagnostic with a suggestion.
    #[must_use]
    pub fn with_suggestion(
        mut self,
        label: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        self.suggestions.push(Suggestion {
            label: label.into(),
            replacement: replacement.into(),
        });
        self
    }
}

pub fn diagnostics_for_shell_operators(hits: &[ShellOperatorHit]) -> Vec<Diagnostic> {
    let mut out = hits
        .iter()
        .map(|hit| Diagnostic {
            code: DiagnosticCode::ShellOperator,
            message: format!(
                "Unsupported shell operator token '{}' ({})",
                hit.token,
                shell_operator_label(hit.kind)
            ),
            span: Some(Span {
                arg_index: hit.arg_index,
            }),
            suggestions: vec![Suggestion {
                label: "Remove shell operator and use CLI flags instead".to_string(),
                replacement: suggestion_for_shell_operator(hit.kind),
            }],
        })
        .collect::<Vec<_>>();

    sort_diagnostics(&mut out);

    out
}

fn suggestion_for_shell_operator(kind: ShellOperatorKind) -> String {
    match kind {
        ShellOperatorKind::Pipe => {
            "Use --limit or --filter flags instead of piping to head/grep".to_string()
        }
        ShellOperatorKind::RedirectOut | ShellOperatorKind::RedirectOutAppend => {
            "Use --output <file> flag instead of shell redirection".to_string()
        }
        ShellOperatorKind::RedirectIn => {
            "Use --input <file> flag instead of shell redirection".to_string()
        }
        ShellOperatorKind::HereDoc => "Use --input <file> or pass content via stdin".to_string(),
        ShellOperatorKind::AndAnd | ShellOperatorKind::OrOr | ShellOperatorKind::Semicolon => {
            "Run commands separately; each exo command is atomic".to_string()
        }
        ShellOperatorKind::SubstitutionDollarParen | ShellOperatorKind::SubstitutionBackticks => {
            "Use explicit values instead of shell substitution".to_string()
        }
    }
}

/// Sort diagnostics deterministically.
///
/// Ordering key: `(code, span.arg_index, message)`.
pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        (
            a.code,
            a.span.as_ref().map_or(usize::MAX, |s| s.arg_index),
            &a.message,
        )
            .cmp(&(
                b.code,
                b.span.as_ref().map_or(usize::MAX, |s| s.arg_index),
                &b.message,
            ))
    });
}

const fn shell_operator_label(kind: ShellOperatorKind) -> &'static str {
    match kind {
        ShellOperatorKind::Pipe => "pipe",
        ShellOperatorKind::RedirectOut => "redirect_out",
        ShellOperatorKind::RedirectOutAppend => "redirect_out_append",
        ShellOperatorKind::RedirectIn => "redirect_in",
        ShellOperatorKind::HereDoc => "heredoc",
        ShellOperatorKind::AndAnd => "and_and",
        ShellOperatorKind::OrOr => "or_or",
        ShellOperatorKind::Semicolon => "semicolon",
        ShellOperatorKind::SubstitutionDollarParen => "substitution_dollar_paren",
        ShellOperatorKind::SubstitutionBackticks => "substitution_backticks",
    }
}
