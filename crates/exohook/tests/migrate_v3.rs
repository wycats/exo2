use exohook::CheckCategory;
use exohook::config::{CheckRefV3, ConfigV3, migrate_v2_to_v3};
use toml_edit::DocumentMut;

#[test]
fn migrate_simple_v2_to_v3() {
    let v2 = r#"
version = 2

[defaults]
timeout_seconds = 120

[lane.coherence]
checks = ["fmt", "lint"]
parallel = true

[lane.gate]
checks = ["test"]

[check.fmt]
run = "cargo fmt --"
input_mode = "paths"
autofix = true

[check.lint]
run = "cargo clippy -- -D warnings"
input_mode = "none"

[check.test]
run = "cargo test"
"#;

    let doc: DocumentMut = v2.parse().unwrap();
    let migrated = migrate_v2_to_v3(&doc).expect("migration failed");
    let config = ConfigV3::parse(&migrated).expect("failed to parse v3");
    config.validate().expect("v3 validation failed");

    assert_eq!(config.version, 3);
    assert_eq!(config.hooks.pre_commit.as_deref(), Some("coherence"));
    assert_eq!(config.hooks.pre_push.as_deref(), Some("gate"));

    let coherence = config
        .workflow
        .get("coherence")
        .expect("missing coherence workflow");
    assert_eq!(coherence.checks.len(), 2);
    assert!(matches!(
        coherence.checks[0],
        CheckRefV3::Ref(ref name) if name == "fmt"
    ));
    assert!(matches!(
        coherence.checks[1],
        CheckRefV3::Ref(ref name) if name == "lint"
    ));

    let gate = config.workflow.get("gate").expect("missing gate workflow");
    assert_eq!(gate.checks.len(), 1);
    assert!(matches!(
        gate.checks[0],
        CheckRefV3::Ref(ref name) if name == "test"
    ));

    let fmt = config.check.get("fmt").expect("missing fmt");
    assert_eq!(fmt.command.as_deref(), Some("cargo fmt --"));
    assert_eq!(fmt.category, CheckCategory::Mutate);
    assert_eq!(fmt.filters, vec!["**/*".to_string()]);

    let lint = config.check.get("lint").expect("missing lint");
    assert_eq!(lint.command.as_deref(), Some("cargo clippy -- -D warnings"));
    assert_eq!(lint.category, CheckCategory::Observe);

    let test = config.check.get("test").expect("missing test");
    assert_eq!(test.command.as_deref(), Some("cargo test"));

    assert_eq!(config.defaults.timeout_seconds, Some(120));
    assert_eq!(config.defaults.parallel, Some(true));
}

#[test]
fn migrate_run_fix_to_fix_command() {
    let v2 = r#"
version = 2

[lane.coherence]
checks = ["clippy"]

[check.clippy]
run = "cargo clippy -- -D warnings"
run_fix = "cargo clippy --fix -- -D warnings"
"#;

    let doc: DocumentMut = v2.parse().unwrap();
    let migrated = migrate_v2_to_v3(&doc).expect("migration failed");
    let config = ConfigV3::parse(&migrated).expect("failed to parse v3");

    let clippy = config.check.get("clippy").expect("missing clippy");
    assert_eq!(clippy.category, CheckCategory::Mutate);
    assert_eq!(
        clippy.fix_command.as_deref(),
        Some("cargo clippy --fix -- -D warnings")
    );
}
