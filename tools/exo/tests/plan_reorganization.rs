//! Integration tests for agent-visible plan reorganization commands.

#[macro_use]
mod test_support;

use exo::api::protocol::{
    Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, Status,
};
use exo::project::Project;
use exosuit_storage::OptionalExtension;
use std::path::Path;
use std::process::Command;
use test_support::{
    exo_active_epoch_id, exo_active_phase_id, exo_cmd, exo_init_with_storage, exo_plan_add_epoch,
    exo_plan_add_phase, exo_plan_update_status,
};

fn git_init(root: &Path) {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("run git init");

    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn epoch_ids(root: &Path) -> Vec<String> {
    let output = exo_cmd(root)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    json.get("result")
        .and_then(|r| r.get("epochs"))
        .and_then(|v| v.as_array())
        .expect("epochs array")
        .iter()
        .filter_map(|epoch| epoch.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect()
}

fn epoch_phase_ids(root: &Path, epoch_id: &str) -> Vec<String> {
    let output = exo_cmd(root)
        .args(["--format", "json", "epoch", "status", epoch_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    json.get("result")
        .and_then(|r| r.get("phases"))
        .and_then(|v| v.as_array())
        .expect("phases array")
        .iter()
        .filter_map(|phase| phase.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .collect()
}

fn goal_row(root: &Path, goal_id: &str) -> (String, String, i64) {
    let project = Project::resolve(root).expect("project resolves");
    let db = exosuit_storage::open_database(project.db_path()).expect("open db");
    db.connection()
        .query_row(
            "SELECT g.status, p.text_id, COUNT(t.id)
             FROM goals_data g
             JOIN phases_data p ON p.id = g.phase_id
             LEFT JOIN tasks_data t ON t.goal_id = g.id
             WHERE g.text_id = ?1
             GROUP BY g.id, p.text_id",
            [goal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("goal row")
}

fn workspace_pin(root: &Path) -> Option<String> {
    let project = Project::resolve(root).expect("project resolves");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    let db = exosuit_storage::open_database(project.db_path()).expect("open db");
    db.connection()
        .query_row(
            "SELECT p.text_id
             FROM workspace_active_phase_data wap
             JOIN phases_data p ON p.id = wap.phase_id
             WHERE wap.workspace_root = ?1",
            [workspace_root],
            |row| row.get(0),
        )
        .optional()
        .expect("workspace pin query")
}

fn machine_call(root: &Path, path: &[&str], input: serde_json::Value) -> ResponseEnvelope {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: format!("plan-reorganization-{}", path.join(".")),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: path.iter().map(|part| (*part).to_string()).collect(),
            },
            input,
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: Some("agent://plan-reorganization-test".to_string()),
    };

    test_support::run_machine_channel_in_process(root, &request)
}

#[test]
fn goal_move_preserves_status_and_nested_tasks() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let source_phase = exo_active_phase_id(root);
    let epoch = exo_active_epoch_id(root);
    let target_phase = exo_plan_add_phase(root, &epoch, "Target phase", None, None);

    exo_cmd(root)
        .args([
            "goal",
            "add",
            "Goal to move",
            "--id",
            "goal-to-move",
            "--phase",
            &source_phase,
        ])
        .assert()
        .success();
    exo_cmd(root)
        .args([
            "task",
            "add",
            "Nested task",
            "--id",
            "nested-task",
            "--goal",
            "goal-to-move",
        ])
        .assert()
        .success();
    exo_plan_update_status(root, "goal-to-move", "in-progress");

    exo_cmd(root)
        .args([
            "--format",
            "json",
            "goal",
            "move",
            "goal-to-move",
            "--phase",
            &target_phase,
            "--position",
            "top",
        ])
        .assert()
        .success();

    let (status, phase_id, task_count) = goal_row(root, "goal-to-move");
    assert_eq!(status, "in-progress");
    assert_eq!(phase_id, target_phase);
    assert_eq!(task_count, 1);
}

#[test]
fn machine_channel_goal_move_and_inbox_action_payload_work() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let source_phase = exo_active_phase_id(root);
    let epoch = exo_active_epoch_id(root);
    let target_phase = exo_plan_add_phase(root, &epoch, "Target phase", None, None);

    exo_cmd(root)
        .args([
            "goal",
            "add",
            "Goal to move",
            "--id",
            "machine-goal-to-move",
            "--phase",
            &source_phase,
        ])
        .assert()
        .success();
    exo_plan_update_status(root, "machine-goal-to-move", "in-progress");

    let move_response = machine_call(
        root,
        &["goal", "move"],
        serde_json::json!({
            "id": "machine-goal-to-move",
            "phase": target_phase,
            "position": "top",
        }),
    );
    assert_eq!(move_response.status, Status::Ok, "{move_response:?}");
    assert_eq!(
        move_response
            .result
            .as_ref()
            .and_then(|result| result.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("goal.move")
    );

    let (status, phase_id, _) = goal_row(root, "machine-goal-to-move");
    assert_eq!(status, "in-progress");
    assert_eq!(phase_id, target_phase);

    let action = serde_json::json!({
        "type": "goal.move",
        "goal_id": "machine-goal-to-move",
        "phase_id": target_phase,
    });
    let inbox_response = machine_call(
        root,
        &["inbox", "add"],
        serde_json::json!({
            "subject": "Move goal request",
            "entity_type": "goal",
            "entity_id": "machine-goal-to-move",
            "intent": "fyi",
            "priority": "immediate",
            "action_json": action.to_string(),
        }),
    );
    assert_eq!(inbox_response.status, Status::Ok, "{inbox_response:?}");
    let result = inbox_response.result.as_ref().expect("inbox result");
    assert_eq!(
        result.pointer("/action/type").and_then(|v| v.as_str()),
        Some("goal.move")
    );
}

#[test]
fn phase_move_preserves_workspace_pin_and_epoch_order() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let active_phase = exo_active_phase_id(root);
    let target_epoch = exo_plan_add_epoch(root, "Target epoch");
    let existing_target_phase = exo_plan_add_phase(root, &target_epoch, "Existing", None, None);

    exo_cmd(root)
        .args([
            "--format",
            "json",
            "phase",
            "move",
            &active_phase,
            "--epoch",
            &target_epoch,
            "--position",
            "top",
        ])
        .assert()
        .success();

    assert_eq!(workspace_pin(root).as_deref(), Some(active_phase.as_str()));
    assert_eq!(
        epoch_phase_ids(root, &target_epoch),
        vec![active_phase, existing_target_phase]
    );
}

#[test]
fn epoch_update_and_reorder_are_visible_in_epoch_list() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap_epoch = exo_active_epoch_id(root);
    let first = exo_plan_add_epoch(root, "First");
    let second = exo_plan_add_epoch(root, "Second");

    exo_cmd(root)
        .args(["epoch", "update", &second, "--title", "Renamed Second"])
        .assert()
        .success();
    exo_cmd(root)
        .args(["epoch", "reorder", &second, &format!("before:{first}")])
        .assert()
        .success();

    assert_eq!(epoch_ids(root), vec![bootstrap_epoch, second, first]);
}

#[test]
fn inbox_action_payload_round_trips_through_json() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let action = r#"{"type":"goal.move","goal_id":"goal-a","phase_id":"phase-b"}"#;
    let output = exo_cmd(root)
        .args([
            "--format",
            "json",
            "inbox",
            "add",
            "Move goal request",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-a",
            "--intent",
            "fyi",
            "--priority",
            "immediate",
            "--action-json",
            action,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let item = json.get("result").expect("inbox item");
    assert_eq!(
        item.pointer("/action/type").and_then(|v| v.as_str()),
        Some("goal.move")
    );
    assert_eq!(
        item.pointer("/action/phase_id").and_then(|v| v.as_str()),
        Some("phase-b")
    );
}
