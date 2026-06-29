use serde::{Deserialize, Serialize};

use crate::api::protocol::Steering;
use crate::diagnostics::{Diagnostic, diagnostics_for_shell_operators};
use crate::shell_ops::detect_shell_operators;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steering: Option<Steering>,
}

pub use crate::argv_compiler::Compilation;

/// Analyze argv for unsupported shell syntax/operators.
///
/// This does not interpret shell operators; it only detects and rejects them.
pub fn analyze_argv(argv: &[String]) -> Analysis {
    let hits = detect_shell_operators(argv);

    if hits.is_empty() {
        return Analysis {
            diagnostics: Vec::new(),
            steering: None,
        };
    }

    let diagnostics = diagnostics_for_shell_operators(&hits);

    // Steering is a suggestion hook only (no rewrite). Keep it minimal and stable.
    // RFC 0132: shell-like idioms should suggest invoking help and using structured output.
    let steering = crate::steering::steering_for_shell_operators(&hits);

    Analysis {
        diagnostics,
        steering,
    }
}

/// Compile argv into an `Invocation` using a `CommandSpec`.
///
/// Step 2 (RFC 0132 verification): this is a minimal surface that
/// (1) rejects literal shell operators and (2) returns an Invocation rooted
/// at the spec root. Deterministic routing and argument parsing are added in
/// later steps.
pub fn compile_argv(
    spec: &crate::command::command_spec::CommandSpec,
    argv: &[String],
) -> Compilation {
    crate::argv_compiler::compile(spec, argv)
}
