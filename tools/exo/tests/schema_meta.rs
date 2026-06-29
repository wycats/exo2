//! Tests for schema version metadata (Meta struct).
//!
//! Verifies that all canonical TOML file types can be parsed
//! with and without the `[meta]` section for backward compatibility.

use exo::context::{ExoState, Meta};

#[test]
fn test_meta_default_values() {
    let meta = Meta::default();
    assert_eq!(meta.schema_version, "1.0.0");
    // exo_version is set from CARGO_PKG_VERSION at compile time
    assert!(!meta.exo_version.is_empty());
}

#[test]
fn test_meta_new_constructor() {
    let meta = Meta::new("2.0.0", "0.5.0");
    assert_eq!(meta.schema_version, "2.0.0");
    assert_eq!(meta.exo_version, "0.5.0");
}

#[test]
fn test_meta_current_constructor() {
    let meta = Meta::current();
    assert_eq!(meta.schema_version, "1.0.0");
    assert!(!meta.exo_version.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// ExoState (plan.toml) parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_parse_plan_with_meta() {
    let toml = r#"
[meta]
schema_version = "1.0.0"
exo_version = "0.3.1"

[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "pending"
"#;

    let state: ExoState = toml::from_str(toml).expect("Failed to parse plan with meta");
    assert!(state.meta.is_some());

    let meta = state.meta.unwrap();
    assert_eq!(meta.schema_version, "1.0.0");
    assert_eq!(meta.exo_version, "0.3.1");
    assert_eq!(state.epochs.len(), 1);
}

#[test]
fn test_parse_plan_without_meta_backward_compat() {
    let toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "pending"
"#;

    let state: ExoState = toml::from_str(toml).expect("Failed to parse plan without meta");
    assert!(state.meta.is_none());
    assert_eq!(state.epochs.len(), 1);
}

#[test]
fn test_serialize_plan_with_meta() {
    let mut state = ExoState::default();
    state.meta = Some(Meta::new("1.0.0", "0.3.0"));

    let serialized = toml::to_string_pretty(&state).expect("Failed to serialize");
    assert!(serialized.contains("[meta]"));
    assert!(serialized.contains("schema_version = \"1.0.0\""));
    assert!(serialized.contains("exo_version = \"0.3.0\""));
}

#[test]
fn test_serialize_plan_without_meta_no_section() {
    let state = ExoState::default(); // meta is None

    let serialized = toml::to_string_pretty(&state).expect("Failed to serialize");
    // Should NOT contain [meta] section when meta is None
    assert!(!serialized.contains("[meta]"));
}

// ─────────────────────────────────────────────────────────────────────────────
// IdeasFile (ideas.toml) parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_parse_ideas_with_meta() {
    use exo::idea::IdeasFile;

    let toml = r#"
[meta]
schema_version = "1.0.0"
exo_version = "0.3.1"

[[ideas]]
id = "idea-1"
title = "Test Idea"
description = "A test idea"
status = "new"
created_at = "2025-01-01T00:00:00Z"
source = "user"
tags = []
related_tasks = []
"#;

    let file: IdeasFile = toml::from_str(toml).expect("Failed to parse ideas with meta");
    assert!(file.meta.is_some());
    assert_eq!(file.ideas.len(), 1);
}

#[test]
fn test_parse_ideas_without_meta_backward_compat() {
    use exo::idea::IdeasFile;

    let toml = r#"
[[ideas]]
id = "idea-1"
title = "Test Idea"
description = "A test idea"
status = "new"
created_at = "2025-01-01T00:00:00Z"
source = "user"
tags = []
related_tasks = []
"#;

    let file: IdeasFile = toml::from_str(toml).expect("Failed to parse ideas without meta");
    assert!(file.meta.is_none());
    assert_eq!(file.ideas.len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Meta equality tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_meta_equality() {
    let meta1 = Meta::new("1.0.0", "0.3.0");
    let meta2 = Meta::new("1.0.0", "0.3.0");
    let meta3 = Meta::new("1.0.0", "0.4.0");

    assert_eq!(meta1, meta2);
    assert_ne!(meta1, meta3);
}

#[test]
fn test_meta_clone() {
    let meta = Meta::new("1.0.0", "0.3.0");
    let cloned = meta.clone();
    assert_eq!(meta, cloned);
}
