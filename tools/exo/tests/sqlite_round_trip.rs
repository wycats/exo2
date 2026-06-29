//! Round-trip tests: SqliteWriter → SqliteLoader
//!
//! Proves that data written via SqliteWriter can be read back correctly
//! via SqliteLoader, validating the full write→read cycle.

use anyhow::Result;
use exo::context::{SqliteLoader, SqliteWriter};
use tempfile::TempDir;

/// Create a writer and loader sharing the same temp database file.
fn create_pair() -> Result<(SqliteWriter, SqliteLoader, TempDir)> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("exo.db");

    let writer = SqliteWriter::open(&db_path)?;
    let loader = SqliteLoader::open(&db_path)?;

    Ok((writer, loader, tmp))
}

// ─────────────────────────────────────────────────────────────────────────
// Epochs, Phases, Goals
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn round_trip_epoch_phase_goal_hierarchy() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    // Write: epoch → phase → 3 goals
    let epoch_id = writer.add_epoch("Test Epoch", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "Phase 1", "regular", None, &[])?;
    writer.add_goal(
        &phase_id,
        "goal-a",
        "First Goal",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_id,
        "goal-b",
        "Second Goal",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_id,
        "goal-c",
        "Third Goal",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    // Read back via loader
    let state = loader.load_state()?;

    assert_eq!(state.epochs.len(), 1);
    let epoch = &state.epochs[0];
    assert_eq!(epoch.title, "Test Epoch");
    assert_eq!(epoch.id, epoch_id);

    assert_eq!(epoch.phases.len(), 1);
    let phase = &epoch.phases[0];
    assert_eq!(phase.title, "Phase 1");
    assert_eq!(phase.id, phase_id);
    assert_eq!(phase.status, "pending");

    assert_eq!(phase.goals.len(), 3);
    assert_eq!(phase.goals[0].id, "goal-a");
    assert_eq!(phase.goals[0].label, "First Goal");
    assert_eq!(phase.goals[0].status, "pending");
    assert_eq!(phase.goals[1].id, "goal-b");
    assert_eq!(phase.goals[2].id, "goal-c");

    Ok(())
}

#[test]
fn round_trip_goal_status_and_completion() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    writer.add_goal(
        &phase_id,
        "my-goal",
        "My Goal",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    // Update status and completion log
    writer.update_goal_status("my-goal", "in-progress")?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs[0].phases[0].goals[0].status, "in-progress");

    writer.update_goal_completion_log("my-goal", "All done!")?;
    writer.update_goal_status("my-goal", "completed")?;
    let state = loader.load_state()?;
    let goal = &state.epochs[0].phases[0].goals[0];
    assert_eq!(goal.status, "completed");
    assert_eq!(goal.completion_log.as_deref(), Some("All done!"));

    Ok(())
}

#[test]
fn round_trip_goal_reorder() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    writer.add_goal(
        &phase_id,
        "g1",
        "Goal 1",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_id,
        "g2",
        "Goal 2",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_id,
        "g3",
        "Goal 3",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    // Reorder g3 to top
    writer.reorder_goal("g3", "top")?;

    let state = loader.load_state()?;
    let goals = &state.epochs[0].phases[0].goals;
    assert_eq!(goals[0].id, "g3");
    assert_eq!(goals[1].id, "g1");
    assert_eq!(goals[2].id, "g2");

    // Reorder g1 to bottom
    writer.reorder_goal("g1", "bottom")?;

    let state = loader.load_state()?;
    let goals = &state.epochs[0].phases[0].goals;
    assert_eq!(goals[0].id, "g3");
    assert_eq!(goals[1].id, "g2");
    assert_eq!(goals[2].id, "g1");

    Ok(())
}

#[test]
fn round_trip_phase_status() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.update_phase_status(&phase_id, "in-progress")?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs[0].phases[0].status, "in-progress");

    writer.update_phase_status(&phase_id, "completed")?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs[0].phases[0].status, "completed");

    Ok(())
}

#[test]
fn workspace_active_phase_round_trip() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_workspace_active_phase("/tmp/exo-workspace", &phase_id)?;

    assert_eq!(
        loader.load_workspace_active_phase("/tmp/exo-workspace")?,
        Some(phase_id)
    );

    Ok(())
}

#[test]
fn workspace_active_phase_upsert_replaces_phase() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_a = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    let phase_b = writer.add_phase(&epoch_id, "P2", "regular", None, &[])?;

    writer.set_workspace_active_phase("/tmp/exo-workspace", &phase_a)?;
    writer.set_workspace_active_phase("/tmp/exo-workspace", &phase_b)?;

    assert_eq!(
        loader.load_workspace_active_phase("/tmp/exo-workspace")?,
        Some(phase_b)
    );

    Ok(())
}

#[test]
fn workspace_active_phase_clear_removes_pin() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_workspace_active_phase("/tmp/exo-workspace", &phase_id)?;
    writer.clear_workspace_active_phase("/tmp/exo-workspace")?;

    assert_eq!(
        loader.load_workspace_active_phase("/tmp/exo-workspace")?,
        None
    );

    Ok(())
}

#[test]
fn workspace_active_phase_cascades_when_phase_deleted() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_workspace_active_phase("/tmp/exo-workspace", &phase_id)?;
    writer.remove_phase(&phase_id)?;

    assert_eq!(
        loader.load_workspace_active_phase("/tmp/exo-workspace")?,
        None
    );

    Ok(())
}

#[test]
fn phase_owner_round_trip() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_phase_owner(
        &phase_id,
        "workspace",
        "workspace:project:abc123",
        Some("workspace:project:abc123"),
        Some("/tmp/exo-workspace"),
    )?;

    let owner = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist");
    assert_eq!(owner.owner_kind, "workspace");
    assert_eq!(owner.owner_id, "workspace:project:abc123");
    assert_eq!(
        owner.claimed_by_workspace_root.as_deref(),
        Some("/tmp/exo-workspace")
    );
    let owners = loader.load_phase_owners()?;
    assert_eq!(
        owners.get(&phase_id).map(|owner| owner.owner_id.as_str()),
        Some("workspace:project:abc123")
    );

    Ok(())
}

#[test]
fn phase_owner_upsert_and_clear() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_phase_owner(
        &phase_id,
        "workspace",
        "workspace:project:abc123",
        Some("workspace:project:abc123"),
        Some("/tmp/exo-workspace"),
    )?;
    let first_claimed_at = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist")
        .claimed_at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    writer.set_phase_owner(
        &phase_id,
        "branch",
        "feature/isolation",
        Some("workspace:project:def456"),
        Some("/tmp/exo-workspace-2"),
    )?;

    let owner = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist");
    assert_eq!(owner.owner_kind, "branch");
    assert_eq!(owner.owner_id, "feature/isolation");
    assert_ne!(owner.claimed_at, first_claimed_at);

    writer.clear_phase_owner(&phase_id)?;
    assert!(loader.load_phase_owner(&phase_id)?.is_none());

    Ok(())
}

#[test]
fn phase_owner_conditional_claim_does_not_overwrite_changed_owner() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_phase_owner(
        &phase_id,
        "branch",
        "feature/a",
        Some("workspace:project:a"),
        Some("/tmp/exo-workspace-a"),
    )?;

    let claimed = writer.claim_phase_owner_if_current(
        &phase_id,
        "branch",
        "feature/b",
        Some("workspace:project:b"),
        Some("/tmp/exo-workspace-b"),
        None,
    )?;
    assert!(!claimed, "unowned claim must not overwrite existing owner");
    let owner = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist");
    assert_eq!(owner.owner_id, "feature/a");

    let claimed = writer.claim_phase_owner_if_current(
        &phase_id,
        "branch",
        "feature/b",
        Some("workspace:project:b"),
        Some("/tmp/exo-workspace-b"),
        Some(("branch", "feature/a")),
    )?;
    assert!(claimed, "matching expected owner should allow takeover");
    let owner = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist");
    assert_eq!(owner.owner_id, "feature/b");

    let cleared = writer.clear_phase_owner_if_current(&phase_id, "branch", "feature/a")?;
    assert!(
        !cleared,
        "conditional clear must not remove a changed owner"
    );
    let owner = loader
        .load_phase_owner(&phase_id)?
        .expect("phase owner should exist");
    assert_eq!(owner.owner_id, "feature/b");

    let cleared = writer.clear_phase_owner_if_current(&phase_id, "branch", "feature/b")?;
    assert!(cleared, "matching current owner should be cleared");
    assert!(loader.load_phase_owner(&phase_id)?.is_none());

    Ok(())
}

#[test]
fn phase_owner_cascades_when_phase_deleted() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;

    writer.set_phase_owner(
        &phase_id,
        "branch",
        "feature/isolation",
        Some("workspace:project:abc123"),
        Some("/tmp/exo-workspace"),
    )?;
    writer.remove_phase(&phase_id)?;

    assert!(loader.load_phase_owner(&phase_id)?.is_none());

    Ok(())
}

#[test]
fn workspace_active_phase_scopes_details_tasks_entities_and_counts() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_a = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    let phase_b = writer.add_phase(&epoch_id, "P2", "regular", None, &[])?;
    writer.replace_phase_rfcs(&phase_a, &["101".to_string()])?;
    writer.replace_phase_rfcs(&phase_b, &["202".to_string()])?;
    writer.update_phase_status(&phase_a, "in-progress")?;
    writer.update_phase_status(&phase_b, "in-progress")?;
    writer.add_goal(
        &phase_a,
        "goal-a",
        "Goal A",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_b,
        "goal-b",
        "Goal B",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_task("goal-a", "task-a", "Task A", None)?;
    writer.add_task("goal-b", "task-b", "Task B", None)?;
    writer.set_workspace_active_phase("/workspace-b", &phase_b)?;

    let details = loader
        .load_active_phase_details_for_workspace(Some("/workspace-b"))?
        .expect("details for pinned phase");
    assert_eq!(details.phase_id, phase_b);
    assert_eq!(details.goals.len(), 1);
    assert_eq!(details.goals[0].id, "goal-b");

    assert_eq!(
        loader.list_active_phase_tasks_for_workspace(Some("/workspace-b"))?,
        vec![(
            "goal-b::task-b".to_string(),
            "Task B".to_string(),
            "pending".to_string()
        )]
    );

    let counts = loader.count_tasks_per_goal_for_workspace(Some("/workspace-b"))?;
    assert_eq!(counts.get("goal-b"), Some(&1));
    assert_eq!(counts.get("goal-a"), None);

    let entities = loader.collect_active_phase_entity_ids_for_workspace(Some("/workspace-b"))?;
    assert!(entities.contains(&("goal".to_string(), "goal-b".to_string())));
    assert!(entities.contains(&("task".to_string(), "task-b".to_string())));
    assert!(entities.contains(&("rfc".to_string(), "202".to_string())));
    assert!(!entities.contains(&("goal".to_string(), "goal-a".to_string())));

    Ok(())
}

#[test]
fn phase_details_inbox_only_surfaces_pending_non_claim_items() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    writer.update_phase_status(&phase_id, "in-progress")?;
    writer.add_goal(
        &phase_id,
        "goal-a",
        "Goal A",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    let pending_claim_id = writer.add_inbox_item(
        "goal",
        Some("goal-a"),
        "user-feedback",
        "claim",
        "next-touch",
        None,
        None,
        "Pending claim",
        "Needs attention",
        None,
    )?;
    let pending_concern_id = writer.add_inbox_item(
        "goal",
        Some("goal-a"),
        "user-feedback",
        "concern",
        "next-touch",
        None,
        None,
        "Pending concern",
        "Needs attention",
        None,
    )?;
    let acknowledged_id = writer.add_inbox_item(
        "goal",
        Some("goal-a"),
        "user-feedback",
        "claim",
        "next-touch",
        None,
        Some("agent://test"),
        "Acknowledged claim",
        "Already reviewed",
        None,
    )?;
    writer.update_inbox_status(&acknowledged_id, "acknowledged", None)?;

    let details = loader
        .load_active_phase_details_for_workspace(None)?
        .expect("active phase details");

    assert_eq!(details.inbox_items.len(), 1);
    assert_eq!(details.inbox_items[0].id, pending_concern_id);
    assert_eq!(details.inbox_items[0].status, "pending");
    assert_eq!(details.inbox_items[0].intent.as_str(), "concern");

    let digest = details
        .completion_digests
        .iter()
        .find(|digest| digest.entity_type == "goal" && digest.entity_id == "goal-a")
        .expect("completion digest still includes acknowledged claim");
    assert!(
        digest
            .claims
            .iter()
            .any(|claim| claim.id == pending_claim_id)
    );
    assert!(
        digest
            .claims
            .iter()
            .any(|claim| claim.id == acknowledged_id)
    );

    Ok(())
}

#[test]
fn workspace_active_phase_fallback_requires_single_global_active_phase() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;
    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_a = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    let phase_b = writer.add_phase(&epoch_id, "P2", "regular", None, &[])?;

    writer.update_phase_status(&phase_a, "in-progress")?;
    assert_eq!(
        loader
            .load_active_phase_details_for_workspace(None)?
            .expect("single global active")
            .phase_id,
        phase_a
    );

    writer.update_phase_status(&phase_b, "in-progress")?;
    assert!(
        loader
            .load_active_phase_details_for_workspace(None)?
            .is_none()
    );

    Ok(())
}

#[test]
fn round_trip_multiple_phases_ordered() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let p1 = writer.add_phase(&epoch_id, "Phase 1", "regular", None, &[])?;
    let p2 = writer.add_phase(&epoch_id, "Phase 2", "regular", None, &[])?;
    let p3 = writer.add_phase(&epoch_id, "Phase 3", "chore", None, &[])?;

    let state = loader.load_state()?;
    let phases = &state.epochs[0].phases;
    assert_eq!(phases.len(), 3);
    assert_eq!(phases[0].id, p1);
    assert_eq!(phases[1].id, p2);
    assert_eq!(phases[2].id, p3);
    assert_eq!(phases[0].title, "Phase 1");
    assert_eq!(phases[1].title, "Phase 2");
    assert_eq!(phases[2].title, "Phase 3");

    Ok(())
}

#[test]
fn round_trip_remove_entities() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    writer.add_goal(
        &phase_id,
        "g1",
        "Goal 1",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;
    writer.add_goal(
        &phase_id,
        "g2",
        "Goal 2",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    // Remove goal
    writer.remove_goal("g1")?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs[0].phases[0].goals.len(), 1);
    assert_eq!(state.epochs[0].phases[0].goals[0].id, "g2");

    // Remove phase (cascades goals)
    writer.remove_phase(&phase_id)?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs[0].phases.len(), 0);

    // Remove epoch
    writer.remove_epoch(&epoch_id)?;
    let state = loader.load_state()?;
    assert_eq!(state.epochs.len(), 0);

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Tasks
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn round_trip_tasks() -> Result<()> {
    let (writer, _loader, _tmp) = create_pair()?;
    let conn = writer.database().connection();

    let epoch_id = writer.add_epoch("E1", None, &[])?;
    let phase_id = writer.add_phase(&epoch_id, "P1", "regular", None, &[])?;
    writer.add_goal(
        &phase_id,
        "g1",
        "Goal 1",
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
    )?;

    // Add tasks
    writer.add_task("g1", "t1", "Task 1", None)?;
    writer.add_task("g1", "t2", "Task 2", None)?;

    // Verify via SQL (tasks aren't loaded by load_state yet)
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks_data WHERE goal_id = (SELECT id FROM goals_data WHERE text_id = 'g1')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 2);

    // Complete a task
    writer.complete_task("t1", "Done with task 1")?;
    let status: String = conn.query_row(
        "SELECT status FROM tasks_data WHERE text_id = 't1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(status, "completed");

    let log: String = conn.query_row(
        "SELECT completion_log FROM tasks_data WHERE text_id = 't1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(log, "Done with task 1");

    // Add log and verification
    writer.add_task_log("t2", "note", "Started working on this")?;
    writer.add_task_verification("t2", "test", Some("cargo test"), "pass", None)?;

    let log_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM task_logs tl JOIN tasks_data td ON tl.task_id = td.id WHERE td.text_id = 't2'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(log_count, 1);

    let ver_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM task_verifications tv JOIN tasks_data td ON tv.task_id = td.id WHERE td.text_id = 't2'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(ver_count, 1);

    // Remove task
    writer.remove_task("t1")?;
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks_data WHERE goal_id = (SELECT id FROM goals_data WHERE text_id = 'g1')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(remaining, 1);

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Ideas
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn round_trip_ideas() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let id1 = writer.add_idea(
        "Idea One",
        Some("Description"),
        &["cli".into(), "ux".into()],
    )?;
    let id2 = writer.add_idea("Idea Two", None, &[])?;

    let ideas = loader.load_ideas()?;
    // Ideas are ordered by created_at DESC, so newest first
    assert_eq!(ideas.len(), 2);

    // Find by id (order may vary by timestamp precision)
    let idea1 = ideas
        .iter()
        .find(|i| i.id == id1)
        .expect("idea1 should exist");
    let idea2 = ideas
        .iter()
        .find(|i| i.id == id2)
        .expect("idea2 should exist");

    assert_eq!(idea1.title, "Idea One");
    assert_eq!(idea1.description, "Description");
    assert_eq!(idea1.tags, vec!["cli", "ux"]);

    assert_eq!(idea2.title, "Idea Two");
    assert!(idea2.tags.is_empty());

    // Archive
    writer.archive_idea(&id1)?;
    let ideas = loader.load_ideas()?;
    let idea1 = ideas
        .iter()
        .find(|i| i.id == id1)
        .expect("idea1 should exist");
    assert_eq!(idea1.status, "archived");

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Inbox
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn round_trip_inbox() -> Result<()> {
    let (writer, loader, _tmp) = create_pair()?;

    let id = writer.add_inbox_item(
        "project",
        None,
        "user-feedback",
        "fyi",
        "next-touch",
        None,
        None,
        "Please review this",
        "",
        None,
    )?;
    assert!(id.starts_with("intent-"));

    let items = loader.load_inbox()?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id);
    assert_eq!(items[0].subject, "Please review this");
    assert_eq!(items[0].intent.as_str(), "fyi");
    assert_eq!(items[0].priority.as_str(), "next-touch");
    assert_eq!(items[0].status.as_str(), "pending");

    // Acknowledge
    writer.update_inbox_status(&id, "acknowledged", None)?;
    let items = loader.load_inbox()?;
    assert_eq!(items[0].status.as_str(), "acknowledged");

    // Resolve
    writer.update_inbox_status(&id, "resolved", Some("Done"))?;
    let items = loader.load_inbox()?;
    assert_eq!(items[0].status.as_str(), "resolved");
    assert_eq!(items[0].resolution.as_deref(), Some("Done"));

    // Archive resolved
    let archived = writer.archive_resolved_inbox()?;
    assert_eq!(archived, 1);
    let items = loader.load_inbox()?;
    assert_eq!(items[0].status.as_str(), "archived");

    Ok(())
}
