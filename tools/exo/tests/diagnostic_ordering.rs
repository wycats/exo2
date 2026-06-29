#![allow(missing_docs)]

use exo::diagnostics::{Diagnostic, DiagnosticCode, Span, sort_diagnostics};

#[test]
fn diagnostics_sort_deterministically() {
    let mut diagnostics = vec![
        Diagnostic {
            code: DiagnosticCode::UnknownFlag,
            message: "b".to_string(),
            span: Some(Span { arg_index: 2 }),
            suggestions: Vec::new(),
        },
        Diagnostic {
            code: DiagnosticCode::UnknownFlag,
            message: "a".to_string(),
            span: Some(Span { arg_index: 2 }),
            suggestions: Vec::new(),
        },
        Diagnostic {
            code: DiagnosticCode::UnknownFlag,
            message: "x".to_string(),
            span: None,
            suggestions: Vec::new(),
        },
        Diagnostic {
            code: DiagnosticCode::ShellOperator,
            message: "zz".to_string(),
            span: Some(Span { arg_index: 1 }),
            suggestions: Vec::new(),
        },
        Diagnostic {
            code: DiagnosticCode::ShellOperator,
            message: "aa".to_string(),
            span: Some(Span { arg_index: 1 }),
            suggestions: Vec::new(),
        },
        Diagnostic {
            code: DiagnosticCode::ShellOperator,
            message: "bb".to_string(),
            span: Some(Span { arg_index: 0 }),
            suggestions: Vec::new(),
        },
    ];

    sort_diagnostics(&mut diagnostics);

    let simplified = diagnostics
        .into_iter()
        .map(|d| (d.code, d.span.map(|s| s.arg_index), d.message))
        .collect::<Vec<_>>();

    assert_eq!(
        simplified,
        vec![
            (DiagnosticCode::ShellOperator, Some(0), "bb".to_string()),
            (DiagnosticCode::ShellOperator, Some(1), "aa".to_string()),
            (DiagnosticCode::ShellOperator, Some(1), "zz".to_string()),
            (DiagnosticCode::UnknownFlag, Some(2), "a".to_string()),
            (DiagnosticCode::UnknownFlag, Some(2), "b".to_string()),
            (DiagnosticCode::UnknownFlag, None, "x".to_string()),
        ]
    );
}
