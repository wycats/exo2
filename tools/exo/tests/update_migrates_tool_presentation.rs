//! Integration tests for update migration of tool-presentation config.

#[macro_use]
mod test_support;

use exo::command::update::run_update;
use exo::context::{AgentContext, ExoState};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use test_case::test_matrix;
use test_support::fs;

fn dummy_context(root: &std::path::Path) -> AgentContext {
    let cache_dir = root.join(".cache");
    assert!(fs::create_dir_all(&cache_dir).is_ok());
    assert!(exosuit_storage::open_database(cache_dir.join("exo.db")).is_ok());

    AgentContext {
        root: root.to_path_buf(),
        project: None,
        plan: ExoState {
            meta: None,
            epochs: Vec::new(),
        },
    }
}

#[test_matrix(["sqlite"])]
fn update_migrates_legacy_tool_presentation_into_config_dir(_backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());

    let legacy_path = root.join("docs/agent-context/tool-presentation.toml");
    let legacy_contents = r#"# Legacy tool presentation config

[[tools]]
name = "example"
"#;

    assert!(fs::write(&legacy_path, legacy_contents).is_ok());

    let mut ctx = dummy_context(root);
    assert!(run_update(&mut ctx).is_ok());

    let preferred_path = root.join(".config/exo/tool-presentation.toml");

    assert!(!legacy_path.exists());
    assert!(preferred_path.exists());

    let migrated = ok_or_return!(
        fs::read_to_string(&preferred_path),
        "failed to read migrated config"
    );
    assert_eq!(migrated, legacy_contents);

    let permissions = ok_or_return!(
        fs::metadata(&preferred_path).map(|m| m.permissions()),
        "failed to read metadata for migrated config"
    );

    #[cfg(unix)]
    assert_ne!(
        permissions.mode() & 0o200,
        0,
        "expected migrated config to be writable"
    );
    #[cfg(windows)]
    assert!(
        !permissions.readonly(),
        "expected migrated config to be writable"
    );
}

#[test_matrix(["sqlite"])]
fn update_does_not_override_preferred_tool_presentation(_backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();

    assert!(fs::create_dir_all(root.join("docs/agent-context")).is_ok());
    assert!(fs::create_dir_all(root.join(".config/exo")).is_ok());

    let legacy_path = root.join("docs/agent-context/tool-presentation.toml");
    let preferred_path = root.join(".config/exo/tool-presentation.toml");

    assert!(fs::write(&legacy_path, "legacy").is_ok());
    assert!(fs::write(&preferred_path, "preferred").is_ok());

    let mut ctx = dummy_context(root);
    assert!(run_update(&mut ctx).is_ok());

    assert!(legacy_path.exists());
    assert!(preferred_path.exists());

    let preferred = ok_or_return!(
        fs::read_to_string(&preferred_path),
        "failed to read preferred config"
    );
    assert_eq!(preferred, "preferred");
}
