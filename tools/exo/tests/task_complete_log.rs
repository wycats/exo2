//! Integration tests for `exo task complete`.

#[macro_use]
mod test_support;

use exo::api::protocol::{
    Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, WorkflowConfirmationDecision,
    WorkflowConfirmationInput,
};
use test_case::test_matrix;
use test_support::{
    exo_cmd, exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage,
};

const BANNED_COMPLETION_CONFIRMATION_TERMS: &[&str] = &[
    "completion claim",
    "intent claim",
    "inbox add",
    "acknowledged claim",
    "workflow_confirmation",
    "completion_input",
    "blocked_state",
    "entity_type",
    "entity_id",
    "--workflow-confirmation-json",
    "yes_complete",
];

fn assert_no_completion_process_vocabulary(text: &str) {
    for term in BANNED_COMPLETION_CONFIRMATION_TERMS {
        assert!(
            !text.contains(term),
            "blocked completion copy leaked process vocabulary '{term}': {text}"
        );
    }
}

fn assert_task_outcome_review_steering(text: &str) {
    assert!(
        text.contains("Present the proposed outcome for review"),
        "task completion steering changed away from outcome-review copy: {text}"
    );
    assert!(
        !text.contains("Ask the human"),
        "task completion steering leaked old confirmation copy: {text}"
    );
    assert!(
        !text.contains("child task"),
        "task completion steering leaked goal child-task copy: {text}"
    );
    assert!(
        !text.contains("goal complete"),
        "task completion steering leaked goal retry instructions: {text}"
    );
}

fn assert_no_completion_process_vocabulary_in_json(value: &serde_json::Value) {
    let mut user_visible_parts = Vec::new();
    for key in [
        "header",
        "question",
        "message",
        "readiness_rationale",
        "proposed_outcome",
    ] {
        if let Some(text) = value.get(key).and_then(serde_json::Value::as_str) {
            user_visible_parts.push(text.to_string());
        }
    }
    if let Some(options) = value.get("options").and_then(serde_json::Value::as_array) {
        for option in options {
            if let Some(label) = option.get("label").and_then(serde_json::Value::as_str) {
                user_visible_parts.push(label.to_string());
            }
            if let Some(description) = option
                .get("description")
                .and_then(serde_json::Value::as_str)
            {
                user_visible_parts.push(description.to_string());
            }
        }
    }

    assert_no_completion_process_vocabulary(&user_visible_parts.join("\n"));
}

fn add_agent_task_completion_confirmation(root: &std::path::Path, task_id: &str) {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "agent-task-confirmation".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["inbox".to_string(), "add".to_string()],
            },
            input: serde_json::json!({
                "subject": "Done",
                "entity_type": "task",
                "entity_id": task_id,
                "intent": "claim",
            }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: Some("agent://test".to_string()),
    };

    let response = test_support::run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, exo::api::protocol::Status::Ok);
    let agent_id = response
        .result
        .as_ref()
        .and_then(|result| result.get("agent_id"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(agent_id, Some("agent://test"));
}

fn add_agent_task_completion_confirmation_with_body(
    root: &std::path::Path,
    task_id: &str,
    subject: &str,
    body: &str,
) {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "agent-task-confirmation-with-body".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["inbox".to_string(), "add".to_string()],
            },
            input: serde_json::json!({
                "subject": subject,
                "body": body,
                "entity_type": "task",
                "entity_id": task_id,
                "intent": "claim",
            }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: Some("agent://test".to_string()),
    };

    let response = test_support::run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, exo::api::protocol::Status::Ok);
}

fn task_complete_request(
    id: &str,
    task_id: &str,
    outcome: &str,
    workflow_confirmation: Option<WorkflowConfirmationInput>,
    agent_id: Option<&str>,
) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: id.to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["task".to_string(), "complete".to_string()],
            },
            input: serde_json::json!({
                "id": task_id,
                "log": outcome,
            }),
        }),
        auth: None,
        workflow_confirmation,
        agent_id: agent_id.map(str::to_string),
    }
}

#[test_matrix(["sqlite"])]
fn task_complete_defaults_log_message(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add a goal first (which becomes a phase task in plan.toml)
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Test goal for completion",
            "--id",
            "test-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "test-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete without --log should use the default log
    exo_cmd_with_storage(root, backend)
        .args(["task", "complete", "test-goal"])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("test-goal"))
        .expect("test-goal entry");
    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Completed")
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_with_log_succeeds_on_phase_task(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add a goal (phase task in plan.toml)
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal with log", "--id", "log-goal"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "task",
            "--entity-id",
            "log-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete with --log should succeed
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "log-goal",
            "--log",
            "Completed implementation and tested",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    let goals = json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("log-goal"))
        .expect("log-goal entry");

    assert_eq!(
        goal.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Completed implementation and tested")
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_rejects_unknown_id_without_recording_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "missing-task",
            "--log",
            "Approved missing outcome",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Task not found: missing-task"));

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "goal",
            "--entity-id",
            "missing-task",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox_json: serde_json::Value = serde_json::from_slice(&inbox_output).expect("valid json");
    let items = inbox_json["result"]["items"]
        .as_array()
        .expect("inbox items");
    assert!(
        items.is_empty(),
        "failed completion recorded approval evidence"
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_goal_fallback_requires_goal_completion_checks(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Pending goal", "--id", "pending-goal"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Pending child",
            "--id",
            "pending-child",
            "--goal",
            "pending-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "pending-goal",
            "--log",
            "Approved goal outcome",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("1 task(s) still pending"));

    let goal_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let goal_json: serde_json::Value = serde_json::from_slice(&goal_output).expect("valid json");
    let goal = goal_json["result"]["goals"]
        .as_array()
        .expect("goals")
        .iter()
        .find(|goal| goal["id"].as_str() == Some("pending-goal"))
        .expect("pending goal");
    assert_ne!(goal["status"].as_str(), Some("completed"));

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "goal",
            "--entity-id",
            "pending-goal",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox_json: serde_json::Value = serde_json::from_slice(&inbox_output).expect("valid json");
    let items = inbox_json["result"]["items"]
        .as_array()
        .expect("inbox items");
    assert!(
        items.is_empty(),
        "rejected goal fallback recorded approval evidence"
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_without_confirmation_returns_plain_precondition_failure(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-task"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Task needing confirmation",
            "--id",
            "needs-confirmation",
            "--goal",
            "goal-for-task",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-task::needs-confirmation";
    let task_id = "needs-confirmation";
    let json_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format", "json", "task", "complete", task_ref, "--log", "Done",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json_output).expect("valid json");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "precondition_failed");
    assert_eq!(json["error"]["details"]["entity_type"], "task");
    assert_eq!(json["error"]["details"]["entity_id"], task_id);
    assert_eq!(
        json["error"]["details"]["blocked_state"],
        "Outcome ready for review."
    );
    assert_eq!(
        json["error"]["details"]["repair"],
        "Approve, revise, continue, or discuss the outcome before recording completion."
    );
    let workflow = &json["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(workflow["entity_type"], "task");
    assert_eq!(workflow["entity_id"], task_id);
    assert_eq!(
        workflow["completion_input"]["kind"],
        "workflow_completion_confirmation"
    );
    assert_eq!(workflow["completion_input"]["entity_type"], "task");
    assert_eq!(workflow["completion_input"]["entity_id"], task_id);
    assert_eq!(workflow["completion_input"]["decision"], "yes_complete");
    assert_eq!(workflow["completion_input"]["outcome"], "Done");
    assert_eq!(workflow["header"], "Outcome ready for review");
    assert_eq!(workflow["question"], "Approve this outcome?");
    assert_eq!(workflow["proposed_outcome"], "Done");
    assert_eq!(
        workflow["readiness_rationale"],
        "The task outcome is ready for review."
    );
    assert_eq!(
        workflow["branch_instructions"]["revise_outcome"],
        "Use the revised outcome summary before completing the task."
    );
    assert_eq!(
        workflow["branch_instructions"]["yes_complete"],
        "Record the approved outcome and close the task."
    );
    assert_no_completion_process_vocabulary_in_json(workflow);

    let human_output = exo_cmd(root)
        .args(["task", "complete", task_ref, "--log", "Done"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(human_output).expect("stderr utf8");
    assert!(stderr.contains("Outcome review needed"));
    assert!(stderr.contains("Outcome ready for review."));
    assert!(stderr.contains("Approve this outcome?"));
    assert_task_outcome_review_steering(&stderr);
    assert_no_completion_process_vocabulary(&stderr);
}

#[test_matrix(["sqlite"])]
fn task_complete_cli_workflow_confirmation_json_recovers_completion(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-cli-workflow"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "CLI workflow-confirmed task",
            "--id",
            "cli-workflow-confirmed-task",
            "--goal",
            "goal-for-cli-workflow",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-cli-workflow::cli-workflow-confirmed-task";
    let first_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "complete",
            task_ref,
            "--log",
            "CLI workflow task outcome shipped",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let first_json: serde_json::Value =
        serde_json::from_slice(&first_output).expect("valid first json");
    let workflow = &first_json["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    let workflow_confirmation =
        serde_json::to_string(&workflow["completion_input"]).expect("serialize confirmation");

    exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "--workflow-confirmation-json",
            &workflow_confirmation,
            "task",
            "complete",
            task_ref,
            "--log",
            "CLI workflow task outcome shipped",
        ])
        .assert()
        .success();

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let tasks = list_json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    let task = tasks
        .iter()
        .find(|task| task.get("id").and_then(|v| v.as_str()) == Some(task_ref))
        .expect("completed task");
    assert_eq!(
        task.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_cli_accepts_mcp_shaped_workflow_confirmation_json(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-mcp-workflow"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "MCP-shaped workflow-confirmed task",
            "--id",
            "mcp-workflow-confirmed-task",
            "--goal",
            "goal-for-mcp-workflow",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-mcp-workflow::mcp-workflow-confirmed-task";
    let task_id = "mcp-workflow-confirmed-task";
    let workflow_confirmation = serde_json::json!({
        "workflowConfirmation": {
            "kind": "workflow_completion_confirmation",
            "entityType": "task",
            "entityId": task_id,
            "decision": "yes_complete",
            "outcome": "MCP-shaped workflow task outcome shipped",
        }
    })
    .to_string();

    exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "--workflow-confirmation-json",
            &workflow_confirmation,
            "task",
            "complete",
            task_ref,
            "--log",
            "MCP-shaped workflow task outcome shipped",
        ])
        .assert()
        .success();

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let tasks = list_json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    let task = tasks
        .iter()
        .find(|task| task.get("id").and_then(|v| v.as_str()) == Some(task_ref))
        .expect("completed task");
    assert_eq!(
        task.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test_matrix(["sqlite"])]
fn task_complete_with_agent_confirmation_pending_returns_plain_precondition_failure(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-pending-task"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Task waiting on human",
            "--id",
            "waiting-on-human",
            "--goal",
            "goal-for-pending-task",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-pending-task::waiting-on-human";
    let task_id = "waiting-on-human";
    add_agent_task_completion_confirmation(root, task_id);

    let json_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format", "json", "task", "complete", task_ref, "--log", "Done",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json_output).expect("valid json");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "precondition_failed");
    assert_eq!(json["error"]["details"]["entity_type"], "task");
    assert_eq!(json["error"]["details"]["entity_id"], task_id);
    assert_eq!(
        json["error"]["details"]["blocked_state"],
        "Outcome ready for review."
    );
    assert_eq!(
        json["error"]["details"]["repair"],
        "Approve, revise, continue, or discuss the outcome before recording completion."
    );
    let workflow = &json["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(workflow["entity_type"], "task");
    assert_eq!(workflow["proposed_outcome"], "Done");
    assert_eq!(workflow["completion_input"]["decision"], "yes_complete");
    assert_eq!(workflow["completion_input"]["outcome"], "Done");
    assert_no_completion_process_vocabulary_in_json(workflow);

    let human_output = exo_cmd(root)
        .args(["task", "complete", task_ref, "--log", "Done"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(human_output).expect("stderr utf8");
    assert!(stderr.contains("Outcome review needed"));
    assert!(stderr.contains("Outcome ready for review."));
    assert!(stderr.contains("Approve this outcome?"));
    assert_task_outcome_review_steering(&stderr);
    assert_no_completion_process_vocabulary(&stderr);
}

#[test_matrix(["sqlite"])]
fn blocked_task_completion_workflow_confirmation_includes_completion_digest(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-task-digest"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Task with digest in review prompt",
            "--id",
            "task-digest-review",
            "--goal",
            "goal-for-task-digest",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-task-digest::task-digest-review";
    let task_id = "task-digest-review";
    add_agent_task_completion_confirmation_with_body(
        root,
        task_id,
        "Digest subject from agent",
        "Digest body with implementation details",
    );

    let request = task_complete_request(
        "task-workflow-digest-attempt",
        task_ref,
        "Digest outcome ready",
        None,
        Some("agent://workflow-test"),
    );
    let response = test_support::run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, exo::api::protocol::Status::Error);
    let details = response
        .error
        .as_ref()
        .and_then(|error| error.details.as_ref())
        .expect("error details");
    let workflow = details
        .get("workflow_confirmation")
        .or_else(|| {
            details
                .get("details")
                .and_then(|details| details.get("workflow_confirmation"))
        })
        .expect("workflow confirmation payload");

    let digest = workflow
        .get("completion_digest")
        .expect("workflow completion digest");
    assert_eq!(digest["entity_type"], "task");
    assert_eq!(digest["entity_id"], task_id);
    assert_eq!(digest["count"], 1);
    let claims = digest["claims"].as_array().expect("digest claims");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0]["subject"], "Digest subject from agent");
    assert_eq!(claims[0]["body"], "Digest body with implementation details");
    assert_no_completion_process_vocabulary_in_json(workflow);
}

#[test_matrix(["sqlite"])]
fn workflow_yes_confirmation_completes_task_and_records_internal_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-workflow-task"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Workflow-confirmed task",
            "--id",
            "workflow-confirmed-task",
            "--goal",
            "goal-for-workflow-task",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-workflow-task::workflow-confirmed-task";
    let task_id = "workflow-confirmed-task";
    let first = task_complete_request(
        "task-workflow-first-attempt",
        task_ref,
        "Workflow task outcome shipped",
        None,
        Some("agent://workflow-test"),
    );
    let first_response = test_support::run_machine_channel_in_process(root, &first);
    assert_eq!(first_response.status, exo::api::protocol::Status::Error);
    let first_details = first_response
        .error
        .as_ref()
        .and_then(|error| error.details.as_ref())
        .expect("error details");
    let workflow = first_details
        .get("workflow_confirmation")
        .or_else(|| {
            first_details
                .get("details")
                .and_then(|details| details.get("workflow_confirmation"))
        })
        .expect("workflow confirmation payload");
    assert_eq!(
        workflow["proposed_outcome"],
        "Workflow task outcome shipped"
    );

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "task".to_string(),
        entity_id: task_id.to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow task outcome shipped".to_string(),
    };
    let second = task_complete_request(
        "task-workflow-confirmed-attempt",
        task_ref,
        "Workflow task outcome shipped",
        Some(confirmation),
        Some("agent://workflow-test"),
    );
    let second_response = test_support::run_machine_channel_in_process(root, &second);
    assert_eq!(second_response.status, exo::api::protocol::Status::Ok);
    assert_eq!(
        second_response
            .result
            .as_ref()
            .and_then(|result| result.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("task.complete")
    );

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let tasks = list_json
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .expect("tasks array");
    let task = tasks
        .iter()
        .find(|task| task.get("id").and_then(|v| v.as_str()) == Some(task_ref))
        .expect("workflow-confirmed-task entry");
    assert_eq!(
        task.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "task",
            "--entity-id",
            task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox_json: serde_json::Value = serde_json::from_slice(&inbox_output).expect("valid json");
    let items = inbox_json
        .get("result")
        .and_then(|r| r.get("items"))
        .and_then(|v| v.as_array())
        .expect("inbox items");
    let evidence = items
        .iter()
        .find(|item| item.get("subject").and_then(|v| v.as_str()) == Some("Outcome approved"))
        .expect("outcome approval evidence");
    assert_eq!(
        evidence.get("status").and_then(|v| v.as_str()),
        Some("acknowledged")
    );
    assert!(
        evidence
            .get("agent_id")
            .is_none_or(serde_json::Value::is_null),
        "outcome approval evidence is user evidence, not agent-authored evidence"
    );
}

#[test_matrix(["sqlite"])]
fn workflow_yes_confirmation_completes_task_without_agent_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-workflow-no-agent"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Workflow task without agent id",
            "--id",
            "workflow-task-no-agent",
            "--goal",
            "goal-for-workflow-no-agent",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-workflow-no-agent::workflow-task-no-agent";
    let task_id = "workflow-task-no-agent";
    let first = task_complete_request(
        "task-workflow-no-agent-first-attempt",
        task_ref,
        "Workflow no-agent task outcome shipped",
        None,
        None,
    );
    let first_response = test_support::run_machine_channel_in_process(root, &first);
    assert_eq!(first_response.status, exo::api::protocol::Status::Error);

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "task".to_string(),
        entity_id: task_id.to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow no-agent task outcome shipped".to_string(),
    };
    let second = task_complete_request(
        "task-workflow-no-agent-confirmed-attempt",
        task_ref,
        "Workflow no-agent task outcome shipped",
        Some(confirmation),
        None,
    );
    let second_response = test_support::run_machine_channel_in_process(root, &second);
    assert_eq!(second_response.status, exo::api::protocol::Status::Ok);
    assert_eq!(
        second_response
            .result
            .as_ref()
            .and_then(|result| result.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("task.complete")
    );
}

#[test_matrix(["sqlite"])]
fn workflow_outcome_review_alias_completes_task_and_records_internal_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-workflow-alias"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Alias workflow task",
            "--id",
            "workflow-alias-task",
            "--goal",
            "goal-for-workflow-alias",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-workflow-alias::workflow-alias-task";
    let task_id = "workflow-alias-task";
    let first = task_complete_request(
        "task-workflow-alias-first-attempt",
        task_ref,
        "Workflow alias task outcome shipped",
        None,
        Some("agent://workflow-test"),
    );
    let first_response = test_support::run_machine_channel_in_process(root, &first);
    assert_eq!(first_response.status, exo::api::protocol::Status::Error);
    let first_details = first_response
        .error
        .as_ref()
        .and_then(|error| error.details.as_ref())
        .expect("error details");
    let workflow = first_details
        .get("workflow_confirmation")
        .or_else(|| {
            first_details
                .get("details")
                .and_then(|details| details.get("workflow_confirmation"))
        })
        .expect("workflow confirmation payload");
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(
        workflow["completion_input"]["kind"],
        "workflow_completion_confirmation"
    );

    let confirmation = WorkflowConfirmationInput {
        kind: "outcome_review".to_string(),
        entity_type: "task".to_string(),
        entity_id: task_id.to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow alias task outcome shipped".to_string(),
    };
    let second = task_complete_request(
        "task-workflow-alias-confirmed-attempt",
        task_ref,
        "Workflow alias task outcome shipped",
        Some(confirmation),
        Some("agent://workflow-test"),
    );
    let second_response = test_support::run_machine_channel_in_process(root, &second);
    assert_eq!(second_response.status, exo::api::protocol::Status::Ok);
    assert_eq!(
        second_response
            .result
            .as_ref()
            .and_then(|result| result.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("task.complete")
    );

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "task",
            "--entity-id",
            task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox_json: serde_json::Value = serde_json::from_slice(&inbox_output).expect("valid json");
    let items = inbox_json
        .get("result")
        .and_then(|r| r.get("items"))
        .and_then(|v| v.as_array())
        .expect("inbox items");
    let evidence = items
        .iter()
        .find(|item| item.get("subject").and_then(|v| v.as_str()) == Some("Outcome approved"))
        .expect("outcome approval evidence");
    assert!(
        evidence
            .get("agent_id")
            .is_none_or(serde_json::Value::is_null),
        "outcome approval evidence is user evidence, not agent-authored evidence"
    );
}

#[test_matrix(["sqlite"])]
fn workflow_yes_confirmation_for_goal_fallback_records_goal_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal fallback", "--id", "goal-fallback"])
        .assert()
        .success();

    let first = task_complete_request(
        "goal-fallback-first-attempt",
        "goal-fallback",
        "Goal fallback outcome shipped",
        None,
        Some("agent://workflow-test"),
    );
    let first_response = test_support::run_machine_channel_in_process(root, &first);
    assert_eq!(first_response.status, exo::api::protocol::Status::Error);

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "goal".to_string(),
        entity_id: "goal-fallback".to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Goal fallback outcome shipped".to_string(),
    };
    let second = task_complete_request(
        "goal-fallback-confirmed-attempt",
        "goal-fallback",
        "Goal fallback outcome shipped",
        Some(confirmation),
        Some("agent://workflow-test"),
    );
    let second_response = test_support::run_machine_channel_in_process(root, &second);
    assert_eq!(second_response.status, exo::api::protocol::Status::Ok);

    let goal_inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-fallback",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let goal_inbox_json: serde_json::Value =
        serde_json::from_slice(&goal_inbox_output).expect("valid json");
    let goal_items = goal_inbox_json
        .get("result")
        .and_then(|r| r.get("items"))
        .and_then(|v| v.as_array())
        .expect("goal inbox items");
    assert!(
        goal_items.iter().any(|item| {
            item.get("subject").and_then(|v| v.as_str()) == Some("Outcome approved")
        })
    );

    let task_inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "task",
            "--entity-id",
            "goal-fallback",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task_inbox_json: serde_json::Value =
        serde_json::from_slice(&task_inbox_output).expect("valid json");
    let task_items = task_inbox_json
        .get("result")
        .and_then(|r| r.get("items"))
        .and_then(|v| v.as_array())
        .expect("task inbox items");
    assert!(task_items.is_empty());
}

#[test_matrix(["sqlite"])]
fn workflow_yes_confirmation_mismatch_still_blocks_task_completion(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal", "--id", "goal-for-task-mismatch"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Mismatch workflow task",
            "--id",
            "workflow-mismatch-task",
            "--goal",
            "goal-for-task-mismatch",
        ])
        .assert()
        .success();

    let task_ref = "goal-for-task-mismatch::workflow-mismatch-task";
    let task_id = "workflow-mismatch-task";
    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "task".to_string(),
        entity_id: task_id.to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Different outcome".to_string(),
    };
    let request = task_complete_request(
        "task-workflow-mismatch-attempt",
        task_ref,
        "Original outcome",
        Some(confirmation),
        Some("agent://workflow-test"),
    );
    let response = test_support::run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, exo::api::protocol::Status::Error);
    assert_eq!(
        response.error.as_ref().map(|error| error.code),
        Some(exo::api::protocol::ErrorCode::PreconditionFailed)
    );
}
