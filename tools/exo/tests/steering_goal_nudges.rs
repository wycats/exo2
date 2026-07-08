//! Integration tests for goal-centric steering nudges.
//!
//! Per RFC 00177 (Goals and Tasks unified model), steering should:
//! 1. Suggest goal creation when an active phase has no goals
//! 2. Warn when goals exist but the phase lacks RFC linkage

use exo::context::{Goal, PhaseKind, PhaseRfc};
use exo::steering::{ProgressMode, WorkIntent, derive_world_steering};
use exo::world_state::{
    ActiveEpoch, ActivePhase, EpochBoundaryState, RfcPipelineEntry, SnapshotFileStatus, WorldState,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn make_default_snapshots() -> Vec<SnapshotFileStatus> {
    vec![SnapshotFileStatus {
        path: "docs/agent-context/current/implementation-plan.toml".to_string(),
        exists: true,
        read_only: false,
        writable: true,
        disk_read_only: false,
        direct_writable: false,
        status: "ok".to_string(),
        guidance: None,
    }]
}

fn make_world_state(
    active_phase: Option<ActivePhase>,
    tasks: Vec<(String, String, String)>,
    goals: Vec<Goal>,
) -> WorldState {
    WorldState {
        root: PathBuf::from("/tmp"),
        db_path: PathBuf::from("/tmp/.cache/exo.db"),
        workspace_root_key: None,
        active_phase,
        next_phase: None,
        epoch_state: EpochBoundaryState {
            active_epoch: Some(ActiveEpoch {
                id: "epoch-1".to_string(),
                title: "Epoch 1".to_string(),
                status: "active".to_string(),
            }),
            epoch_complete: false,
            has_epochs: true,
            all_epochs_complete: false,
        },
        git_dirty: false,
        git_changes: None,
        sidecar_sync: None,
        current_snapshots: make_default_snapshots(),
        tasks,
        goals,
        rfc_pipeline: HashMap::new(),
        unreviewed_epochs: vec![],
        session_boundary: exo::session_boundary::BoundaryDetection {
            boundary_type: exo::session_boundary::BoundaryType::Session,
            confidence: 0.5,
            rationale: "test default".to_string(),
            previous_session: None,
        },
    }
}

fn make_world_state_with_pipeline(
    active_phase: Option<ActivePhase>,
    tasks: Vec<(String, String, String)>,
    goals: Vec<Goal>,
    rfc_pipeline: HashMap<String, RfcPipelineEntry>,
) -> WorldState {
    let mut world = make_world_state(active_phase, tasks, goals);
    world.rfc_pipeline = rfc_pipeline;
    world
}

#[test]
fn nudge_suggests_goal_creation_when_planning_with_no_goals() {
    // Active phase with no tasks (goals), in Planning mode
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        vec![],
        vec![],
    );

    let steering = derive_world_steering(&world, None);

    // Should be in Planning mode (no tasks/steps)
    assert_eq!(steering.progress_mode, ProgressMode::Planning);

    // Should have a repair action suggesting goal creation
    let goal_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("exo goal add"));

    assert!(
        goal_nudge.is_some(),
        "Expected goal creation nudge, got: {:?}",
        steering.repair_actions
    );

    let nudge = goal_nudge.unwrap();
    assert_eq!(nudge.label, "Add a goal to the phase");
    assert!(nudge.rationale.contains("no goals defined"));
}

#[test]
fn nudge_rationale_includes_rfc_context_for_driving_rfc() {
    let mut rfc_pipeline = HashMap::new();
    rfc_pipeline.insert(
        "001".to_string(),
        RfcPipelineEntry {
            id: "001".to_string(),
            current_stage: 0,
            target_stage: Some(1),
            title: "Test RFC".to_string(),
            is_driving: true,
        },
    );

    let world = make_world_state_with_pipeline(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![PhaseRfc::driving("001", 1)],
            kind: PhaseKind::Regular,
        }),
        vec![],
        vec![],
        rfc_pipeline,
    );

    let steering = derive_world_steering(&world, None);
    let goal_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("exo goal add"))
        .expect("Expected goal creation nudge");

    assert!(
        goal_nudge
            .rationale
            .contains("Phase is advancing RFC 001 (Stage 0→1). User approval required.")
    );
}

#[test]
fn no_goal_creation_nudge_when_executing_with_tasks() {
    // Active phase with tasks, in Executing mode - should NOT nudge about goals
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        vec![(
            "task-1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )],
        vec![],
    );

    let steering = derive_world_steering(&world, None);

    // Should be in Executing mode (has pending tasks)
    assert_eq!(steering.progress_mode, ProgressMode::Executing);

    // Should NOT have goal creation nudge (already have goals)
    let goal_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("exo goal add"));

    assert!(
        goal_nudge.is_none(),
        "Should not suggest goal creation when goals exist"
    );
}

#[test]
fn nudge_warns_when_goals_exist_without_rfc_linkage() {
    // Active phase with tasks but no RFCs
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![], // No RFC linkage
            kind: PhaseKind::Regular,
        }),
        vec![(
            "task-1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )],
        vec![],
    );

    let steering = derive_world_steering(&world, None);

    // Should have a repair action suggesting RFC linkage
    let rfc_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("--rfc"));

    assert!(
        rfc_nudge.is_some(),
        "Expected RFC linkage nudge, got: {:?}",
        steering.repair_actions
    );

    let nudge = rfc_nudge.unwrap();
    assert_eq!(nudge.label, "Link phase to RFC(s)");
    assert!(nudge.rationale.contains("no RFC linkage"));
}

#[test]
fn no_rfc_nudge_when_phase_has_rfc_linkage() {
    // Active phase WITH RFC linkage
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![PhaseRfc::related("0177")], // Has RFC linkage
            kind: PhaseKind::Regular,
        }),
        vec![(
            "task-1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )],
        vec![],
    );

    let steering = derive_world_steering(&world, None);

    // Should NOT have RFC linkage nudge
    let rfc_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("--rfc"));

    assert!(
        rfc_nudge.is_none(),
        "Should not suggest RFC linkage when already linked"
    );
}

#[test]
fn no_goal_nudges_when_no_active_phase() {
    // No active phase - should not produce goal-related nudges
    let world = make_world_state(None, vec![], vec![]);

    let steering = derive_world_steering(&world, None);

    // Should be in between-phases or between-epochs mode
    assert!(
        steering.progress_mode.is_between_state(),
        "Expected between state, got: {:?}",
        steering.progress_mode
    );

    // Should NOT have any goal-related nudges
    let goal_nudge = steering
        .repair_actions
        .iter()
        .find(|a| a.command.contains("exo goal add") || a.command.contains("--rfc"));

    assert!(
        goal_nudge.is_none(),
        "Should not produce goal nudges without active phase"
    );
}

#[test]
fn nudge_prompts_log_completion_when_tasks_complete_but_no_log() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        // Task ID format: "goal-id::task-id"
        vec![(
            "goal-1::task-1".to_string(),
            "Task 1".to_string(),
            "completed".to_string(),
        )],
        vec![Goal {
            id: "goal-1".to_string(),
            label: "Goal 1".to_string(),
            status: "in-progress".to_string(),
            kind: None,
            started_at: None,
            description: None,
            completion_log: None,
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: None,
            target_stage: None,
        }],
    );

    let steering = derive_world_steering(&world, None);

    let log_nudge = steering
        .next_actions
        .iter()
        .find(|a| a.command.contains("exo goal complete goal-1 --log"));

    assert!(
        log_nudge.is_some(),
        "Expected completion log nudge, got: {:?}",
        steering.next_actions
    );

    let nudge = log_nudge.unwrap();
    assert_eq!(nudge.intent, WorkIntent::Record);
    assert_eq!(nudge.confidence, Some(0.95));
}

#[test]
fn steering_no_nudge_for_abandoned_goals() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        vec![
            (
                "goal-1::task-1".to_string(),
                "Task 1".to_string(),
                "completed".to_string(),
            ),
            (
                "goal-1::task-2".to_string(),
                "Task 2".to_string(),
                "completed".to_string(),
            ),
        ],
        vec![Goal {
            id: "goal-1".to_string(),
            label: "Goal 1".to_string(),
            status: "abandoned".to_string(),
            kind: None,
            started_at: None,
            description: None,
            completion_log: None,
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: None,
            target_stage: None,
        }],
    );

    let steering = derive_world_steering(&world, None);

    let log_nudge = steering
        .next_actions
        .iter()
        .find(|a| a.command.contains("exo goal complete goal-1 --log"));

    assert!(
        log_nudge.is_none(),
        "Should not nudge completion for abandoned goals"
    );
}

#[test]
fn no_log_nudge_when_completion_log_already_exists() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        // Task ID format: "goal-id::task-id"
        vec![(
            "goal-1::task-1".to_string(),
            "Task 1".to_string(),
            "completed".to_string(),
        )],
        vec![Goal {
            id: "goal-1".to_string(),
            label: "Goal 1".to_string(),
            status: "in-progress".to_string(),
            kind: None,
            started_at: None,
            description: None,
            completion_log: Some("Done".to_string()),
            ulid: None,
            slug: None,
            aliases: vec![],
            rfc: None,
            target_stage: None,
        }],
    );

    let steering = derive_world_steering(&world, None);

    let log_nudge = steering
        .next_actions
        .iter()
        .find(|a| a.command.contains("exo goal complete goal-1 --log"));

    assert!(
        log_nudge.is_none(),
        "Should not suggest completion log when one already exists"
    );
}

#[test]
fn nudge_fires_per_goal_not_waiting_for_all_phase_tasks() {
    // Goal 1 has all tasks complete, Goal 2 has pending tasks
    // Nudge should fire for Goal 1 only
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Test Phase".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        vec![
            // Goal 1's tasks - all completed
            (
                "goal-1::task-1".to_string(),
                "Task 1".to_string(),
                "completed".to_string(),
            ),
            (
                "goal-1::task-2".to_string(),
                "Task 2".to_string(),
                "completed".to_string(),
            ),
            // Goal 2's tasks - still pending
            (
                "goal-2::task-1".to_string(),
                "Task A".to_string(),
                "pending".to_string(),
            ),
        ],
        vec![
            Goal {
                id: "goal-1".to_string(),
                label: "Goal 1".to_string(),
                status: "in-progress".to_string(),
                kind: None,
                started_at: None,
                description: None,
                completion_log: None,
                ulid: None,
                slug: None,
                aliases: vec![],
                rfc: None,
                target_stage: None,
            },
            Goal {
                id: "goal-2".to_string(),
                label: "Goal 2".to_string(),
                status: "in-progress".to_string(),
                kind: None,
                started_at: None,
                description: None,
                completion_log: None,
                ulid: None,
                slug: None,
                aliases: vec![],
                rfc: None,
                target_stage: None,
            },
        ],
    );

    let steering = derive_world_steering(&world, None);

    // Should nudge for goal-1 (all tasks complete)
    let goal1_nudge = steering
        .next_actions
        .iter()
        .find(|a| a.command.contains("exo goal complete goal-1 --log"));
    assert!(
        goal1_nudge.is_some(),
        "Expected nudge for goal-1 (all tasks complete), got: {:?}",
        steering.next_actions
    );

    // Should NOT nudge for goal-2 (has pending tasks)
    let goal2_nudge = steering
        .next_actions
        .iter()
        .find(|a| a.command.contains("exo goal complete goal-2 --log"));
    assert!(
        goal2_nudge.is_none(),
        "Should not nudge for goal-2 (has pending tasks)"
    );
}
