//! Tests for `exo map` steering on minimal SQLite-backed workspaces.
//!
//! These cover the current empty-context behavior and ensure map rendering
//! tolerates legacy snapshot files in upgraded workspaces.

use exo::context::{AgentContext, ExoState};
use exo::map::build_map_json;
use exo::steering::{SteeringBlock, WorkIntent};
use std::fs::{self, File};
use tempfile::TempDir;

fn setup_test_context() -> (TempDir, AgentContext) {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    // Create minimal directory structure
    fs::create_dir_all(root.join("docs/agent-context/current")).unwrap();
    fs::create_dir_all(root.join("docs/agent-context")).unwrap();
    fs::create_dir_all(root.join("docs/rfcs")).unwrap();
    fs::create_dir_all(root.join(".cache")).unwrap();
    exosuit_storage::open_database(root.join(".cache/exo.db")).unwrap();

    let context = AgentContext {
        root,
        project: None,
        plan: ExoState {
            meta: None,
            epochs: Vec::new(),
        },
    };
    (temp_dir, context)
}

#[test]
fn test_map_returns_empty_context_steering_when_no_epochs_exist() {
    let (_temp, context) = setup_test_context();

    let result = build_map_json(&context, false, None, None);
    assert!(result.is_ok());

    let json = result.unwrap();
    let steering_block: SteeringBlock =
        serde_json::from_value(json).expect("Should deserialize to SteeringBlock");

    // Current empty-context steering prefers roadmap review.
    assert_eq!(steering_block.next_actions.len(), 1);
    assert_eq!(steering_block.next_actions[0].command, "exo plan review");
    assert_eq!(steering_block.primary_intent, WorkIntent::Orient);
    assert!(steering_block.repair_actions.is_empty());
}

#[test]
fn test_map_passes_through_when_no_critical_upgrades() {
    let (_temp, context) = setup_test_context();

    // No deprecated files - should proceed to normal steering
    let result = build_map_json(&context, false, None, None);
    assert!(result.is_ok());

    let json = result.unwrap();
    let steering_block: SteeringBlock =
        serde_json::from_value(json).expect("Should deserialize to SteeringBlock");

    // Should NOT be upgrade steering (exo update command)
    let is_upgrade_steering = steering_block.next_actions.len() == 1
        && steering_block.next_actions[0].command == "exo update";
    assert!(!is_upgrade_steering);
}

#[test]
fn test_map_next_returns_primary_empty_context_action() {
    let (_temp, context) = setup_test_context();

    // next=true returns the primary steering action, not the repair action.
    let result = build_map_json(&context, true, None, None);
    assert!(result.is_ok());

    let json = result.unwrap();
    let steering_block: SteeringBlock =
        serde_json::from_value(json).expect("Should deserialize to SteeringBlock");
    assert_eq!(steering_block.next_actions.len(), 1);
    assert_eq!(steering_block.next_actions[0].command, "exo plan review");
}

#[test]
fn test_empty_context_steering_rationale_includes_roadmap_details() {
    let (_temp, context) = setup_test_context();

    let result = build_map_json(&context, false, None, None);
    assert!(result.is_ok());

    let json = result.unwrap();
    let steering_block: SteeringBlock = serde_json::from_value(json).unwrap();

    let rationale = &steering_block.next_actions[0].rationale;

    assert!(rationale.contains("No epochs defined in the roadmap."));
    assert!(rationale.contains("Review the roadmap"));
}

#[test]
fn test_map_human_does_not_panic_with_legacy_snapshot_file() {
    let (_temp, context) = setup_test_context();

    // Create legacy snapshot file
    File::create(
        context
            .root
            .join("docs/agent-context/current/task-list.toml"),
    )
    .unwrap();

    // Just verify it doesn't panic and returns Ok
    let result = exo::map::show_map_human(&context, false, None, None);
    assert!(result.is_ok());
}
