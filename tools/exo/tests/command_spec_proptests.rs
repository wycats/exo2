#![allow(missing_docs)]

use proptest::prelude::*;

use exo::shell_ops::ShellOperatorKind;

fn token_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Literal shell operators (must be rejected; never interpreted)
        Just("|".to_string()),
        Just(">".to_string()),
        Just(">>".to_string()),
        Just("<".to_string()),
        Just("<<".to_string()),
        Just("&&".to_string()),
        Just("||".to_string()),
        Just(";".to_string()),
        Just("$(".to_string()),
        Just("`".to_string()),
        // Normal words
        "[a-zA-Z0-9_./-]{1,16}".prop_map(|s| s),
        // Some quoted-ish tokens that should be treated as opaque strings
        r"[a-zA-Z0-9_./-]{0,8}\|[a-zA-Z0-9_./-]{0,8}".prop_map(|s| s),
    ]
}

fn argv_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(token_strategy(), 0..25)
}

proptest! {
    #[test]
    fn analyze_argv_is_panic_free(argv in argv_strategy()) {
        let _ = exo::router::analyze_argv(&argv);
    }

    #[test]
    fn analyze_argv_is_deterministic(argv in argv_strategy()) {
        let a = exo::router::analyze_argv(&argv);
        let b = exo::router::analyze_argv(&argv);

        prop_assert_eq!(
            a.diagnostics
                .iter()
                .map(exo::diagnostics::Diagnostic::strip_spans)
                .collect::<Vec<_>>(),
            b.diagnostics
                .iter()
                .map(exo::diagnostics::Diagnostic::strip_spans)
                .collect::<Vec<_>>()
        );

        // Steering is allowed to be present/absent deterministically.
        prop_assert_eq!(
            a.steering.is_some(),
            b.steering.is_some()
        );
    }

    #[test]
    fn literal_pipe_token_triggers_diagnostic_and_steering(argv in argv_strategy()) {
        let mut argv = argv;
        argv.push("|".to_string());

        let analysis = exo::router::analyze_argv(&argv);

        prop_assert!(analysis.diagnostics.iter().any(|d| d.message.contains("Unsupported shell operator token '|'")));
        prop_assert!(analysis.steering.is_some());
    }

    #[test]
    fn non_literal_pipe_does_not_trigger_shell_operator_hit(argv in argv_strategy()) {
        // Ensure we only treat literal operators as operator signals.
        // If '|' appears within a token, we should not treat it as a shell operator.
        let mut argv = argv;
        argv.push("foo|bar".to_string());

        let hits = exo::shell_ops::detect_shell_operators(&argv);
        prop_assert!(!hits.iter().any(|h| h.kind == ShellOperatorKind::Pipe && h.token != "|"));
    }
}
