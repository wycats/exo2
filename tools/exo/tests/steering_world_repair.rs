//! Integration tests for steering world repair derivation.

use exo::command::sidecar::SidecarRepoSyncStatus;
use exo::context::Goal;
use exo::context::PhaseKind;
use exo::steering::{ProgressMode, derive_progress_mode, derive_world_steering};
use exo::world_state::{
    ActiveEpoch, ActivePhase, EpochBoundaryState, NextPhase, SnapshotFileStatus, WorldState,
};
use std::collections::HashMap;
use std::path::PathBuf;
#[test]
fn world_steering_includes_repair_actions_for_dirty_and_missing_snapshots() {
    let world = WorldState {
        root: PathBuf::from("/tmp"),
        active_phase: Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Phase 1".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
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
        git_dirty: true,
        git_changes: None,
        sidecar_sync: None,
        session_boundary: exo::session_boundary::BoundaryDetection {
            boundary_type: exo::session_boundary::BoundaryType::Session,
            confidence: 0.5,
            rationale: "test default".to_string(),
            previous_session: None,
        },
        current_snapshots: vec![
            SnapshotFileStatus {
                path: "docs/agent-context/current/implementation-plan.toml".to_string(),
                exists: false,
                read_only: false,
                writable: false,
                disk_read_only: false,
                direct_writable: false,
                status: "missing".to_string(),
                guidance: None,
            },
            SnapshotFileStatus {
                path: "docs/agent-context/current/task-list.toml".to_string(),
                exists: false,
                read_only: false,
                writable: false,
                disk_read_only: false,
                direct_writable: false,
                status: "missing".to_string(),
                guidance: None,
            },
            SnapshotFileStatus {
                path: "docs/agent-context/current/walkthrough.toml".to_string(),
                exists: false,
                read_only: false,
                writable: false,
                disk_read_only: false,
                direct_writable: false,
                status: "missing".to_string(),
                guidance: None,
            },
        ],
        tasks: vec![],
        goals: vec![],
        rfc_pipeline: HashMap::new(),
        unreviewed_epochs: vec![],
    };

    let steering = derive_world_steering(&world, None);

    assert!(
        steering
            .repair_actions
            .iter()
            .any(|a| a.command == "git status --porcelain")
    );

    // "exo update" repair was removed — snapshot-based repair triggers were
    // part of the old TOML stack. SQLite state doesn't use file snapshots.

    assert!(
        !steering
            .repair_actions
            .iter()
            .any(|a| a.command == "exo task init")
    );

    assert!(
        !steering
            .repair_actions
            .iter()
            .any(|a| a.command == "exo walkthrough init")
    );
}

#[test]
fn world_steering_treats_loose_sidecar_dirtiness_as_repair_debt() {
    let mut world = make_world_state(None, make_epoch_state(true, false, false), vec![], vec![]);
    world.sidecar_sync = Some(SidecarRepoSyncStatus {
        kind: "sidecar.repo.sync_status",
        ok: true,
        sidecar_root: PathBuf::from("/tmp/sidecar"),
        branch: Some("main".to_string()),
        clean: true,
        repo_clean: false,
        syncable: false,
        has_remote: true,
        remote: Some("origin".to_string()),
        ahead: Some(0),
        behind: Some(0),
        issue_kind: Some("dirty"),
        issue: Some("sidecar repo has uncommitted changes".to_string()),
        project_files: Vec::new(),
        foreign_checkpoint_debt: Vec::new(),
    });

    let steering = derive_world_steering(&world, None);

    assert!(
        steering
            .repair_actions
            .iter()
            .any(|action| action.command == "exo sidecar repo status"),
        "{steering:?}"
    );
    assert!(
        steering
            .repair_actions
            .iter()
            .all(|action| action.command != "exo sidecar repo push"),
        "{steering:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi-level steering state machine tests (RFC 0107)
// ─────────────────────────────────────────────────────────────────────────────

fn make_epoch_state(
    has_epochs: bool,
    all_epochs_complete: bool,
    epoch_complete: bool,
) -> EpochBoundaryState {
    EpochBoundaryState {
        active_epoch: if has_epochs {
            Some(ActiveEpoch {
                id: "epoch-1".to_string(),
                title: "Epoch 1".to_string(),
                status: if epoch_complete {
                    "completed".to_string()
                } else {
                    "active".to_string()
                },
            })
        } else {
            None
        },
        epoch_complete,
        has_epochs,
        all_epochs_complete,
    }
}

fn make_world_state(
    active_phase: Option<ActivePhase>,
    epoch_state: EpochBoundaryState,
    tasks: Vec<(String, String, String)>,
    goals: Vec<Goal>,
) -> WorldState {
    WorldState {
        root: PathBuf::from("/tmp"),
        active_phase,
        next_phase: Some(NextPhase {
            id: "next-phase".to_string(),
            title: "Next Phase".to_string(),
            epoch_title: "Epoch 1".to_string(),
        }),
        epoch_state,
        git_dirty: false,
        git_changes: None,
        sidecar_sync: None,
        current_snapshots: vec![],
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

fn goal_with_status(id: &str, status: &str) -> Goal {
    Goal {
        id: id.to_string(),
        label: id.to_string(),
        status: status.to_string(),
        kind: None,
        started_at: None,
        description: None,
        completion_log: None,
        ulid: None,
        slug: None,
        aliases: Vec::new(),
        rfc: None,
        target_stage: None,
    }
}

#[test]
fn progress_mode_between_epochs_when_all_epochs_complete() {
    let world = make_world_state(None, make_epoch_state(true, true, true), vec![], vec![]);

    assert_eq!(derive_progress_mode(&world), ProgressMode::BetweenEpochs);
}

#[test]
fn progress_mode_between_epochs_when_no_epochs_defined() {
    let world = make_world_state(None, make_epoch_state(false, true, false), vec![], vec![]);

    assert_eq!(derive_progress_mode(&world), ProgressMode::BetweenEpochs);
}

#[test]
fn progress_mode_between_phases_when_in_active_epoch() {
    let world = make_world_state(None, make_epoch_state(true, false, false), vec![], vec![]);

    assert_eq!(derive_progress_mode(&world), ProgressMode::BetweenPhases);
}

#[test]
fn progress_mode_planning_when_active_phase_no_tasks() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Phase 1".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        make_epoch_state(true, false, false),
        vec![], // No tasks
        vec![], // No goals
    );

    assert_eq!(derive_progress_mode(&world), ProgressMode::Planning);
}

#[test]
fn progress_mode_executing_when_pending_tasks() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Phase 1".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        make_epoch_state(true, false, false),
        vec![(
            "task-1".to_string(),
            "Task 1".to_string(),
            "pending".to_string(),
        )],
        vec![],
    );

    assert_eq!(derive_progress_mode(&world), ProgressMode::Executing);
}

/// RFC 00187: When all work is complete with an active phase, mode is Executing
/// (not a separate Transitioning mode). The "ready to ship" state is shown via
/// context in BetweenPhases after `exo phase finish`.
#[test]
fn progress_mode_executing_when_all_complete() {
    let world = make_world_state(
        Some(ActivePhase {
            id: "phase-1".to_string(),
            title: "Phase 1".to_string(),
            epoch_id: "epoch-1".to_string(),
            epoch_title: "Epoch 1".to_string(),
            rfcs: vec![],
            kind: PhaseKind::Regular,
        }),
        make_epoch_state(true, false, false),
        vec![(
            "task-1".to_string(),
            "Task 1".to_string(),
            "completed".to_string(),
        )],
        vec![goal_with_status("goal-1", "green")],
    );

    // RFC 00187: All work done = still Executing, not Transitioning
    assert_eq!(derive_progress_mode(&world), ProgressMode::Executing);
}

#[test]
fn steering_between_epochs_suggests_roadmap_review() {
    let world = make_world_state(None, make_epoch_state(true, true, true), vec![], vec![]);

    let steering = derive_world_steering(&world, None);

    assert_eq!(steering.progress_mode, ProgressMode::BetweenEpochs);
    assert!(
        steering
            .next_actions
            .iter()
            .any(|a| a.command.contains("exo plan review")),
        "Should suggest reviewing roadmap when between epochs"
    );
}

#[test]
fn steering_between_phases_suggests_next_phase() {
    let world = make_world_state(None, make_epoch_state(true, false, false), vec![], vec![]);

    let steering = derive_world_steering(&world, None);

    assert_eq!(steering.progress_mode, ProgressMode::BetweenPhases);
    assert!(
        steering
            .next_actions
            .iter()
            .any(|a| a.command.contains("exo phase start")),
        "Should suggest starting next phase when between phases"
    );
}
