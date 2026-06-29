//! Tests for v3 configuration schema (RFC 00215).

use exohook::config::{CheckV3, ConfigV3, ConfigVersion};
use exohook::{CheckCategory, ExecutionContext, FilesetScope, HookType};
use toml_edit::DocumentMut;

#[test]
fn parse_minimal_v3_config() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"
pre_push = "gate"

[workflow.coherence]
checks = ["fmt", "lint"]
scope = "staged"

[workflow.gate]
checks = ["test"]
scope = "committed_not_pushed"

[check.fmt]
command = "cargo fmt --"
filters = ["**/*.rs"]
category = "mutate"

[check.lint]
command = "cargo clippy -- -D warnings"

[check.test]
command = "cargo test"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    assert_eq!(parsed.version, 3);
    assert_eq!(parsed.hooks.pre_commit.as_deref(), Some("coherence"));
    assert_eq!(parsed.hooks.pre_push.as_deref(), Some("gate"));
    assert_eq!(parsed.check.len(), 3);

    let fmt = parsed.check.get("fmt").expect("missing fmt check");
    assert_eq!(fmt.command.as_deref(), Some("cargo fmt --"));
    assert_eq!(fmt.category, CheckCategory::Mutate);
    assert_eq!(fmt.filters, vec!["**/*.rs"]);

    parsed.validate().expect("validation failed");
}

#[test]
fn parse_full_v3_config() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"
pre_push = "gate"

[workflow.coherence]
checks = ["check", "typecheck", "lint", "fmt", "clippy"]
scope = "staged"

[workflow.gate]
checks = ["test"]
scope = "committed_not_pushed"

[check.check]
label = "Check"
command = "pnpm -r run check"

[check.typecheck]
label = "VS Code Typecheck"
command = "pnpm --filter exosuit-context run typecheck"

[check.lint]
label = "Lint"
command = "pnpm exec eslint --max-warnings 0"
filters = ["**/*.ts", "**/*.tsx"]

[check.fmt]
label = "Rust Fmt"
command = "cargo fmt --all --"
filters = ["**/*.rs"]
category = "mutate"

[check.clippy]
label = "Rust Clippy"
command = "cargo clippy --workspace -- -D warnings"
category = "mutate"
fix_command = "cargo clippy --workspace --fix --allow-dirty --allow-staged -- -D warnings"

[check.test]
label = "Test"
command = "pnpm -r run test:unit"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    assert_eq!(parsed.version, 3);
    assert_eq!(parsed.hooks.pre_commit.as_deref(), Some("coherence"));
    assert_eq!(parsed.check.len(), 6);

    let clippy = parsed.check.get("clippy").expect("missing clippy check");
    assert_eq!(clippy.category, CheckCategory::Mutate);
    assert!(clippy.fix_command.is_some());

    parsed.validate().expect("validation failed");
}

#[test]
fn parse_inline_checks() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = [
    { command = "cargo fmt --", filters = ["**/*.rs"], category = "mutate" },
    "lint"
]

[check.lint]
command = "cargo clippy -- -D warnings"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    assert_eq!(parsed.hooks.pre_commit.as_deref(), Some("coherence"));
    assert_eq!(parsed.workflow.len(), 1);
    parsed.validate().expect("validation failed");
}

#[test]
fn parse_workflow() {
    let config = r#"
version = 3

[hooks]
pre_commit = "quick"

[check.fmt]
command = "cargo fmt --"
category = "mutate"

[check.test]
command = "cargo test"

[workflow.full]
label = "Full Validation"
checks = ["fmt", "test"]
parallel = true
scope = "all"

[workflow.quick]
label = "Quick Check"
checks = ["fmt"]
scope = "uncommitted"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    assert_eq!(parsed.workflow.len(), 2);

    let full = parsed.workflow.get("full").expect("missing full workflow");
    assert_eq!(full.label.as_deref(), Some("Full Validation"));
    assert!(full.parallel);
    assert_eq!(full.scope.as_deref(), Some("all"));

    parsed.validate().expect("validation failed");
}

#[test]
fn validate_missing_command_and_tool() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["bad"]

[check.bad]
label = "Missing command"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let err = parsed.validate().expect_err("should fail validation");
    assert!(
        err.to_string()
            .contains("must specify either 'command' or 'tool'")
    );
}

#[test]
fn validate_both_command_and_tool() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["bad"]

[check.bad]
command = "echo hello"
tool = "exo.something"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let err = parsed.validate().expect_err("should fail validation");
    assert!(
        err.to_string()
            .contains("cannot specify both 'command' and 'tool'")
    );
}

#[test]
fn validate_unknown_check_reference() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["nonexistent"]

[check.fmt]
command = "cargo fmt --"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let err = parsed.validate().expect_err("should fail validation");
    assert!(
        err.to_string()
            .contains("workflow.coherence.checks[0]: unknown check 'nonexistent'")
    );
}

#[test]
fn validate_fix_command_requires_mutate() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["bad"]

[check.bad]
command = "cargo clippy"
fix_command = "cargo clippy --fix"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let err = parsed.validate().expect_err("should fail validation");
    assert!(
        err.to_string()
            .contains("'fix_command' requires 'category = \"mutate\"'")
    );
}

#[test]
fn parse_explicit_observe_category() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["lint"]

[check.lint]
command = "cargo clippy -- -D warnings"
category = "observe"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let lint = parsed.check.get("lint").expect("missing lint check");
    assert_eq!(lint.category, CheckCategory::Observe);
}

#[test]
fn default_category_is_observe() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["lint"]

[check.lint]
command = "cargo clippy -- -D warnings"
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    let lint = parsed.check.get("lint").expect("missing lint check");
    assert_eq!(lint.category, CheckCategory::Observe);
}

#[test]
fn effective_skip_if_empty() {
    // With filters, default is true
    let check_with_filters = CheckV3 {
        filters: vec!["**/*.rs".to_string()],
        ..Default::default()
    };
    assert!(check_with_filters.effective_skip_if_empty());

    // Without filters, default is false
    let check_without_filters = CheckV3::default();
    assert!(!check_without_filters.effective_skip_if_empty());

    // Explicit override
    let check_explicit = CheckV3 {
        skip_if_empty: Some(false),
        filters: vec!["**/*.rs".to_string()],
        ..Default::default()
    };
    assert!(!check_explicit.effective_skip_if_empty());
}

#[test]
fn config_version_detection() {
    let v1: DocumentMut = "".parse().unwrap();
    assert_eq!(ConfigVersion::from_doc(&v1), ConfigVersion::V1);

    let v1_explicit: DocumentMut = "version = 1".parse().unwrap();
    assert_eq!(ConfigVersion::from_doc(&v1_explicit), ConfigVersion::V1);

    let v2: DocumentMut = "version = 2".parse().unwrap();
    assert_eq!(ConfigVersion::from_doc(&v2), ConfigVersion::V2);

    let v3: DocumentMut = "version = 3".parse().unwrap();
    assert_eq!(ConfigVersion::from_doc(&v3), ConfigVersion::V3);

    let unknown: DocumentMut = "version = 99".parse().unwrap();
    assert!(matches!(
        ConfigVersion::from_doc(&unknown),
        ConfigVersion::Unknown(99)
    ));
}

#[test]
fn deprecation_warnings() {
    assert!(ConfigVersion::V1.is_deprecated());
    assert!(ConfigVersion::V2.is_deprecated());
    assert!(!ConfigVersion::V3.is_deprecated());

    assert!(ConfigVersion::V1.deprecation_warning().is_some());
    assert!(ConfigVersion::V2.deprecation_warning().is_some());
    assert!(ConfigVersion::V3.deprecation_warning().is_none());
}

#[test]
fn tool_field_validation() {
    let config = r#"
version = 3

[hooks]
pre_commit = "coherence"

[workflow.coherence]
checks = ["verify-links"]

[check.verify-links]
tool = "exo.docs.links.check"
filters = ["**/*.md"]
"#;

    let parsed = ConfigV3::parse(config).expect("failed to parse");
    parsed.validate().expect("validation failed");

    let check = parsed.check.get("verify-links").expect("missing check");
    assert_eq!(check.tool.as_deref(), Some("exo.docs.links.check"));
    assert!(check.command.is_none());
}

#[test]
fn hook_type_inferred_scope() {
    assert_eq!(HookType::PreCommit.inferred_scope(), FilesetScope::Staged);
    assert_eq!(
        HookType::PrePush.inferred_scope(),
        FilesetScope::CommittedNotPushed
    );
    assert_eq!(HookType::CommitMsg.inferred_scope(), FilesetScope::Staged);
    assert_eq!(
        HookType::PreMergeCommit.inferred_scope(),
        FilesetScope::Staged
    );
    assert_eq!(HookType::Manual.inferred_scope(), FilesetScope::Uncommitted);
}

#[test]
fn hook_type_from_hook_name() {
    assert_eq!(
        HookType::from_hook_name("pre-commit"),
        Some(HookType::PreCommit)
    );
    assert_eq!(
        HookType::from_hook_name("pre_commit"),
        Some(HookType::PreCommit)
    );
    assert_eq!(
        HookType::from_hook_name("pre-push"),
        Some(HookType::PrePush)
    );
    assert_eq!(
        HookType::from_hook_name("pre_push"),
        Some(HookType::PrePush)
    );
    assert_eq!(
        HookType::from_hook_name("commit-msg"),
        Some(HookType::CommitMsg)
    );
    assert_eq!(
        HookType::from_hook_name("commit_msg"),
        Some(HookType::CommitMsg)
    );
    assert_eq!(
        HookType::from_hook_name("pre-merge-commit"),
        Some(HookType::PreMergeCommit)
    );
    assert_eq!(
        HookType::from_hook_name("pre_merge_commit"),
        Some(HookType::PreMergeCommit)
    );
    assert_eq!(HookType::from_hook_name("unknown"), None);
}

#[test]
fn execution_context_should_fix() {
    let check_mutate = CheckV3 {
        category: CheckCategory::Mutate,
        ..Default::default()
    };
    let check_observe = CheckV3 {
        category: CheckCategory::Observe,
        ..Default::default()
    };

    let cases = vec![
        (HookType::PreCommit, true, false, false, true),
        (HookType::PreCommit, false, false, false, false),
        (HookType::PrePush, true, false, false, false),
        (HookType::CommitMsg, true, false, false, false),
        (HookType::PreMergeCommit, true, false, false, false),
        (HookType::Manual, true, false, false, true),
        (HookType::Manual, false, false, false, true),
        (HookType::PrePush, false, true, false, true),
        (HookType::PreCommit, true, true, false, true),
        (HookType::PreCommit, true, true, true, false),
    ];

    for (hook_type, interactive, force_fix, force_no_fix, expected) in cases {
        let mut ctx = ExecutionContext::new(hook_type, interactive);
        ctx.force_fix = force_fix;
        ctx.force_no_fix = force_no_fix;
        assert_eq!(ctx.should_fix(&check_mutate), expected);
    }

    let mut ctx = ExecutionContext::new(HookType::Manual, true);
    ctx.force_fix = true;
    assert!(!ctx.should_fix(&check_observe));
}

#[test]
fn execution_context_should_restage() {
    let check_mutate = CheckV3 {
        category: CheckCategory::Mutate,
        ..Default::default()
    };

    let ctx_pre_commit = ExecutionContext::new(HookType::PreCommit, true);
    assert!(ctx_pre_commit.should_restage(&check_mutate));

    let ctx_pre_commit_noninteractive = ExecutionContext::new(HookType::PreCommit, false);
    assert!(!ctx_pre_commit_noninteractive.should_restage(&check_mutate));

    let ctx_manual = ExecutionContext::new(HookType::Manual, true);
    assert!(!ctx_manual.should_restage(&check_mutate));

    let check_observe = CheckV3 {
        category: CheckCategory::Observe,
        ..Default::default()
    };
    let ctx_pre_commit_fix_disabled = ExecutionContext::new(HookType::PreCommit, true);
    assert!(!ctx_pre_commit_fix_disabled.should_restage(&check_observe));
}
