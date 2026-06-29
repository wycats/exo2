//! Shell operator detection and classification utilities.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellOperatorKind {
    Pipe,
    RedirectOut,
    RedirectOutAppend,
    RedirectIn,
    HereDoc,
    AndAnd,
    OrOr,
    Semicolon,
    SubstitutionDollarParen,
    SubstitutionBackticks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellOperatorHit {
    pub kind: ShellOperatorKind,
    /// Index into argv where the operator token was observed.
    pub arg_index: usize,
    /// The raw token that triggered the hit.
    pub token: String,
}

pub fn detect_shell_operators(argv: &[String]) -> Vec<ShellOperatorHit> {
    let mut hits = Vec::new();

    for (arg_index, token) in argv.iter().enumerate() {
        if let Some(kind) = classify_shell_operator_token(token) {
            hits.push(ShellOperatorHit {
                kind,
                arg_index,
                token: token.clone(),
            });
        }
    }

    // Deterministic ordering: (arg_index, kind, token)
    hits.sort_by(|a, b| (a.arg_index, a.kind, &a.token).cmp(&(b.arg_index, b.kind, &b.token)));

    hits
}

pub fn classify_shell_operator_token(token: &str) -> Option<ShellOperatorKind> {
    // Non-negotiable: we only treat literal shell operators as signals.
    // We do NOT interpret pipelines/redirects/substitutions.
    match token {
        "|" => Some(ShellOperatorKind::Pipe),
        ">" => Some(ShellOperatorKind::RedirectOut),
        ">>" => Some(ShellOperatorKind::RedirectOutAppend),
        "<" => Some(ShellOperatorKind::RedirectIn),
        "<<" => Some(ShellOperatorKind::HereDoc),
        "&&" => Some(ShellOperatorKind::AndAnd),
        "||" => Some(ShellOperatorKind::OrOr),
        ";" => Some(ShellOperatorKind::Semicolon),
        "$(" => Some(ShellOperatorKind::SubstitutionDollarParen),
        "`" => Some(ShellOperatorKind::SubstitutionBackticks),
        _ => None,
    }
}
