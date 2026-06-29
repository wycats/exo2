//! Integration tests for `exo goal complete`.

#[macro_use]
mod test_support;

use exo::api::protocol::{
    Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, WorkflowConfirmationDecision,
    WorkflowConfirmationInput,
};
use predicates::str::contains;
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

fn add_agent_goal_completion_confirmation(root: &std::path::Path, goal_id: &str) {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "agent-goal-confirmation".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["inbox".to_string(), "add".to_string()],
            },
            input: serde_json::json!({
                "subject": "Done",
                "entity_type": "goal",
                "entity_id": goal_id,
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

fn add_agent_goal_completion_confirmation_with_body(
    root: &std::path::Path,
    goal_id: &str,
    subject: &str,
    body: &str,
) {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "agent-goal-confirmation-with-body".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["inbox".to_string(), "add".to_string()],
            },
            input: serde_json::json!({
                "subject": subject,
                "body": body,
                "entity_type": "goal",
                "entity_id": goal_id,
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

fn goal_complete_request(
    id: &str,
    goal_id: &str,
    outcome: &str,
    workflow_confirmation: Option<WorkflowConfirmationInput>,
    agent_id: Option<&str>,
) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: id.to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["goal".to_string(), "complete".to_string()],
            },
            input: serde_json::json!({
                "id": goal_id,
                "log": outcome,
            }),
        }),
        auth: None,
        workflow_confirmation,
        agent_id: agent_id.map(str::to_string),
    }
}

#[test_matrix(["sqlite"])]
fn goal_complete_marks_goal_completed_and_stores_log(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add a goal first
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Test goal for completion",
            "--id",
            "goal-complete-test",
        ])
        .assert()
        .success();

    // Claim the goal for completion
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-complete-test",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete the goal with a log message
    let output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "goal-complete-test",
            "--log",
            "Implemented all required functionality",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("kind"))
            .and_then(|v| v.as_str()),
        Some("goal.complete")
    );
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("ok"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("goal_id"))
            .and_then(|v| v.as_str()),
        Some("goal-complete-test")
    );
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("message"))
            .and_then(|v| v.as_str()),
        Some("Implemented all required functionality")
    );
    assert_eq!(
        json.get("result")
            .and_then(|r| r.get("outcome"))
            .and_then(|v| v.as_str()),
        Some("Implemented all required functionality")
    );

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let goals = list_json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-complete-test"))
        .expect("goal-complete-test entry");

    assert_eq!(
        goal.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Implemented all required functionality")
    );
}

#[test_matrix(["sqlite"])]
fn goal_complete_human_output_treats_log_as_completed_outcome(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Outcome copy goal",
            "--id",
            "outcome-copy-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Outcome confirmed",
            "--entity-type",
            "goal",
            "--entity-id",
            "outcome-copy-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "outcome-copy-goal",
            "--log",
            "Outcome shipped to users",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("stdout utf8");
    assert!(stdout.contains("Completed goal: outcome-copy-goal"));
    assert!(stdout.contains("Outcome: Outcome shipped to users"));
    assert!(!stdout.contains("Log:"));
    assert_no_completion_process_vocabulary(&stdout);
}

#[test_matrix(["sqlite"])]
fn goal_complete_fails_with_pending_tasks(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal with pending tasks",
            "--id",
            "goal-with-pending",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Pending task",
            "--id",
            "task-1",
            "--goal",
            "goal-with-pending",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-with-pending",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "goal-with-pending",
            "--log",
            "Tried to complete",
        ])
        .assert()
        .failure()
        .stderr(contains("Complete tasks first"))
        .stderr(contains("exo goal abandon goal-with-pending"));
}

#[test_matrix(["sqlite"])]
fn goal_complete_without_confirmation_returns_plain_precondition_failure(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal needing confirmation",
            "--id",
            "goal-needs-confirmation",
        ])
        .assert()
        .success();

    let json_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "goal-needs-confirmation",
            "--log",
            "Done",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json_output).expect("valid json");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "precondition_failed");
    assert_eq!(json["error"]["details"]["entity_type"], "goal");
    assert_eq!(
        json["error"]["details"]["entity_id"],
        "goal-needs-confirmation"
    );
    assert_eq!(
        json["error"]["details"]["blocked_state"],
        "Outcome ready for review."
    );
    let workflow = &json["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(workflow["entity_type"], "goal");
    assert_eq!(workflow["entity_id"], "goal-needs-confirmation");
    assert_eq!(
        workflow["completion_input"]["kind"],
        "workflow_completion_confirmation"
    );
    assert_eq!(workflow["completion_input"]["entity_type"], "goal");
    assert_eq!(
        workflow["completion_input"]["entity_id"],
        "goal-needs-confirmation"
    );
    assert_eq!(workflow["completion_input"]["decision"], "yes_complete");
    assert_eq!(workflow["completion_input"]["outcome"], "Done");
    assert_eq!(workflow["header"], "Outcome ready for review");
    assert_eq!(workflow["question"], "Approve this outcome?");
    assert_eq!(workflow["proposed_outcome"], "Done");
    assert_eq!(
        workflow["readiness_rationale"],
        "The goal outcome is ready for review."
    );
    let option_labels: Vec<_> = workflow["options"]
        .as_array()
        .expect("workflow options")
        .iter()
        .map(|option| option["label"].as_str().expect("option label"))
        .collect();
    assert_eq!(
        option_labels,
        vec![
            "Approve outcome",
            "Revise outcome",
            "Keep working",
            "Discuss first",
        ]
    );
    assert_eq!(
        workflow["branch_instructions"]["revise_outcome"],
        "Use the revised outcome summary before completing the goal."
    );
    assert_eq!(
        workflow["branch_instructions"]["yes_complete"],
        "Record the approved outcome and close the goal."
    );
    assert_no_completion_process_vocabulary_in_json(workflow);

    let human_output = exo_cmd(root)
        .args([
            "goal",
            "complete",
            "goal-needs-confirmation",
            "--log",
            "Done",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(human_output).expect("stderr utf8");
    assert!(stderr.contains("Outcome review needed"));
    assert!(stderr.contains("Outcome ready for review."));
    assert!(stderr.contains("Approve this outcome?"));
    assert_no_completion_process_vocabulary(&stderr);
}

#[test_matrix(["sqlite"])]
fn goal_complete_with_agent_confirmation_pending_returns_plain_precondition_failure(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal waiting on human",
            "--id",
            "goal-waiting-on-human",
        ])
        .assert()
        .success();

    add_agent_goal_completion_confirmation(root, "goal-waiting-on-human");

    let json_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "goal-waiting-on-human",
            "--log",
            "Done",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&json_output).expect("valid json");
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "precondition_failed");
    assert_eq!(json["error"]["details"]["entity_type"], "goal");
    assert_eq!(
        json["error"]["details"]["entity_id"],
        "goal-waiting-on-human"
    );
    assert_eq!(
        json["error"]["details"]["blocked_state"],
        "Outcome ready for review."
    );
    let workflow = &json["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(workflow["proposed_outcome"], "Done");
    assert_eq!(workflow["completion_input"]["decision"], "yes_complete");
    assert_eq!(workflow["completion_input"]["outcome"], "Done");
    assert_no_completion_process_vocabulary_in_json(workflow);

    let human_output = exo_cmd(root)
        .args(["goal", "complete", "goal-waiting-on-human", "--log", "Done"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(human_output).expect("stderr utf8");
    assert!(stderr.contains("Outcome review needed"));
    assert!(stderr.contains("Outcome ready for review."));
    assert!(stderr.contains("Approve this outcome?"));
    assert_no_completion_process_vocabulary(&stderr);
}

#[test_matrix(["sqlite"])]
fn blocked_goal_completion_workflow_confirmation_includes_completion_digest(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal with digest in review prompt",
            "--id",
            "goal-digest-review",
        ])
        .assert()
        .success();

    add_agent_goal_completion_confirmation_with_body(
        root,
        "goal-digest-review",
        "Digest subject from agent",
        "Digest body with implementation details",
    );

    let request = goal_complete_request(
        "workflow-digest-attempt",
        "goal-digest-review",
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
    assert_eq!(digest["entity_type"], "goal");
    assert_eq!(digest["entity_id"], "goal-digest-review");
    assert_eq!(digest["count"], 1);
    let claims = digest["claims"].as_array().expect("digest claims");
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0]["subject"], "Digest subject from agent");
    assert_eq!(claims[0]["body"], "Digest body with implementation details");
    assert_no_completion_process_vocabulary_in_json(workflow);
}

#[test_matrix(["sqlite"])]
fn workflow_yes_confirmation_completes_goal_and_records_internal_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Workflow-confirmed goal",
            "--id",
            "workflow-confirmed-goal",
        ])
        .assert()
        .success();

    let first = goal_complete_request(
        "workflow-first-attempt",
        "workflow-confirmed-goal",
        "Workflow outcome shipped",
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
    assert_eq!(workflow["proposed_outcome"], "Workflow outcome shipped");

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "goal".to_string(),
        entity_id: "workflow-confirmed-goal".to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow outcome shipped".to_string(),
    };
    let second = goal_complete_request(
        "workflow-confirmed-attempt",
        "workflow-confirmed-goal",
        "Workflow outcome shipped",
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
        Some("goal.complete")
    );

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let goals = list_json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("workflow-confirmed-goal"))
        .expect("workflow-confirmed-goal entry");
    assert_eq!(
        goal.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "goal",
            "--entity-id",
            "workflow-confirmed-goal",
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
fn workflow_yes_confirmation_completes_goal_without_agent_id(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Workflow-confirmed goal without agent id",
            "--id",
            "workflow-confirmed-goal-no-agent",
        ])
        .assert()
        .success();

    let first = goal_complete_request(
        "workflow-no-agent-first-attempt",
        "workflow-confirmed-goal-no-agent",
        "Workflow no-agent outcome shipped",
        None,
        None,
    );
    let first_response = test_support::run_machine_channel_in_process(root, &first);
    assert_eq!(first_response.status, exo::api::protocol::Status::Error);

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "goal".to_string(),
        entity_id: "workflow-confirmed-goal-no-agent".to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow no-agent outcome shipped".to_string(),
    };
    let second = goal_complete_request(
        "workflow-no-agent-confirmed-attempt",
        "workflow-confirmed-goal-no-agent",
        "Workflow no-agent outcome shipped",
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
        Some("goal.complete")
    );
}

#[test_matrix(["sqlite"])]
fn workflow_outcome_review_alias_completes_goal_and_records_internal_evidence(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Alias workflow goal",
            "--id",
            "workflow-alias-goal",
        ])
        .assert()
        .success();

    let first = goal_complete_request(
        "workflow-alias-first-attempt",
        "workflow-alias-goal",
        "Workflow alias outcome shipped",
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
        entity_type: "goal".to_string(),
        entity_id: "workflow-alias-goal".to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Workflow alias outcome shipped".to_string(),
    };
    let second = goal_complete_request(
        "workflow-alias-confirmed-attempt",
        "workflow-alias-goal",
        "Workflow alias outcome shipped",
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
        Some("goal.complete")
    );

    let inbox_output = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "goal",
            "--entity-id",
            "workflow-alias-goal",
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
fn workflow_yes_confirmation_mismatch_still_blocks_completion(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Mismatch workflow goal",
            "--id",
            "workflow-mismatch-goal",
        ])
        .assert()
        .success();

    let confirmation = WorkflowConfirmationInput {
        kind: "workflow_completion_confirmation".to_string(),
        entity_type: "goal".to_string(),
        entity_id: "workflow-mismatch-goal".to_string(),
        decision: WorkflowConfirmationDecision::YesComplete,
        outcome: "Different outcome".to_string(),
    };
    let request = goal_complete_request(
        "workflow-mismatch-attempt",
        "workflow-mismatch-goal",
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

#[test_matrix(["sqlite"])]
fn goal_complete_succeeds_when_all_tasks_done(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal with completed tasks",
            "--id",
            "goal-all-done",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Finish work",
            "--id",
            "task-1",
            "--goal",
            "goal-all-done",
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
            "goal-all-done::task-1",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "complete",
            "goal-all-done::task-1",
            "--log",
            "Completed task",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-all-done",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "goal-all-done",
            "--log",
            "All tasks done",
        ])
        .assert()
        .success();
}

#[test_matrix(["sqlite"])]
fn goal_complete_succeeds_with_no_tasks(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal without tasks", "--id", "goal-no-tasks"])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-no-tasks",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "goal-no-tasks",
            "--log",
            "Completed without tasks",
        ])
        .assert()
        .success();
}

#[test_matrix(["sqlite"])]
fn goal_complete_defaults_log_when_missing(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add a goal
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal without log test",
            "--id",
            "goal-no-log",
        ])
        .assert()
        .success();

    // Claim the goal for completion
    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "goal-no-log",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete without --log should use the default log
    exo_cmd_with_storage(root, backend)
        .args(["goal", "complete", "goal-no-log"])
        .assert()
        .success();

    let list_output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "goal", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let list_json: serde_json::Value = serde_json::from_slice(&list_output).expect("valid json");
    let goals = list_json
        .get("result")
        .and_then(|r| r.get("goals"))
        .and_then(|v| v.as_array())
        .expect("goals array");
    let goal = goals
        .iter()
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("goal-no-log"))
        .expect("goal-no-log entry");

    assert_eq!(
        goal.get("completion_log").and_then(|v| v.as_str()),
        Some("Completed")
    );
}

#[test]
fn goal_close_is_not_a_live_completion_surface() {
    let command_source = include_str!("../src/command/goal.rs");
    assert!(!command_source.contains("GoalClose"));
    assert!(!command_source.contains("goal.close"));
    assert!(!command_source.contains("goal close"));
    assert!(!command_source.contains("--outcome"));

    let command_spec = include_str!("../../../packages/exosuit-vscode/src/command-spec.json");
    assert!(!command_spec.contains("goal.close"));
    assert!(!command_spec.contains("goal close"));
    assert!(!command_spec.contains("--outcome"));

    let package_json = include_str!("../../../packages/exosuit-vscode/package.json");
    assert!(!package_json.contains("exo-goal-close"));
    assert!(!package_json.contains("goal close"));
    assert!(!package_json.contains("goal.close"));
}

#[test_matrix(["sqlite"])]
fn goal_complete_fails_for_nonexistent_goal(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Attempt to complete a non-existent goal
    exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "nonexistent-goal",
            "--log",
            "This should fail",
        ])
        .assert()
        .failure();
}

#[test_matrix(["sqlite"])]
fn goal_complete_fails_for_already_completed_goal(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add and complete a goal
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Already completed goal",
            "--id",
            "already-done",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "already-done",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "already-done",
            "--log",
            "First completion",
        ])
        .assert()
        .success();

    // Attempt to complete again should fail
    exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "already-done",
            "--log",
            "Second completion attempt",
        ])
        .assert()
        .failure();
}

#[test_matrix(["sqlite"])]
fn goal_list_shows_completed_status(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    // Add and complete a goal
    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "add",
            "Goal for list test",
            "--id",
            "list-test-goal",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "list-test-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args([
            "goal",
            "complete",
            "list-test-goal",
            "--log",
            "Completed for list test",
        ])
        .assert()
        .success();

    // List should show completed status
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
        .find(|g| g.get("id").and_then(|v| v.as_str()) == Some("list-test-goal"))
        .expect("goal should exist");

    assert!(
        goal.get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("completed"))
            .unwrap_or(false),
        "Status should contain 'completed'"
    );
}
