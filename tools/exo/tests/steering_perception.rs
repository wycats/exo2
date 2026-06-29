//! Integration tests for the command-level perception pipeline.
//!
//! These tests verify that inbox items surface as perception summaries
//! in steering output from task/goal commands and `exo status`.

#[macro_use]
mod test_support;

use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage};

/// Helper: parse JSON from command stdout.
fn parse_stdout_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("valid JSON from command stdout")
}

fn steering_commands(steering: &serde_json::Value) -> Vec<&str> {
    ["next_actions", "repair_actions"]
        .into_iter()
        .flat_map(|key| {
            steering
                .get(key)
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
        })
        .filter_map(|action| action.get("command").and_then(|value| value.as_str()))
        .collect()
}

// ============================================================================
// Steering regressions and perception summaries
// ============================================================================

#[test]
fn status_steering_task_add_uses_supported_syntax() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");
    exo_phase_start_with_storage(root, "sqlite");

    let output = exo_cmd_with_storage(root, "sqlite")
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = parse_stdout_json(&output);
    let steering = json
        .get("result")
        .and_then(|result| result.get("steering"))
        .expect("expected steering in status output");
    let commands = steering_commands(steering);

    assert!(
        commands.contains(&"exo task add <title> --id <id>"),
        "status steering should suggest the supported task-add shape: {commands:?}"
    );
    assert!(
        !commands.iter().any(|command| command.contains("--label")),
        "status steering should not suggest the removed --label flag: {commands:?}"
    );
}

#[test]
fn perception_summary_surfaces_on_goal_complete() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");
    exo_phase_start_with_storage(root, "sqlite");

    // Add a goal
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "goal",
            "add",
            "Implement perception pipeline",
            "--id",
            "perc-goal",
        ])
        .assert()
        .success();

    // Add a concern inbox item targeting this goal with immediate priority
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "I have a concern about the approach",
            "--entity-type",
            "goal",
            "--entity-id",
            "perc-goal",
            "--intent",
            "concern",
            "--priority",
            "immediate",
        ])
        .assert()
        .success();

    // Add a completion claim so goal complete succeeds
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "perc-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete the goal with --format json
    let output = exo_cmd_with_storage(root, "sqlite")
        .args([
            "--format",
            "json",
            "goal",
            "complete",
            "perc-goal",
            "--log",
            "Done with perception work",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = parse_stdout_json(&output);

    // The steering block is inside result.steering
    let steering = json
        .get("result")
        .and_then(|r| r.get("steering"))
        .expect("expected steering in goal complete output");

    let summaries = steering
        .get("perception_summaries")
        .and_then(|v| v.as_array())
        .expect("expected perception_summaries array");

    assert!(
        !summaries.is_empty(),
        "perception_summaries should not be empty when inbox items exist"
    );

    // The concern and claim are grouped by entity (goal, perc-goal).
    // The claim has higher intent rank so it's the representative, but both
    // items are counted. Verify the entity-scoped summary exists.
    let goal_summary = summaries.iter().find(|s| {
        s.get("entity_type")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "goal")
            && s.get("entity_id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| id == "perc-goal")
    });
    assert!(
        goal_summary.is_some(),
        "expected a perception summary for goal perc-goal, got: {summaries:?}"
    );

    let summary = goal_summary.unwrap();
    let count = summary.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        count >= 2,
        "expected count >= 2 (concern + claim), got: {count}"
    );
}

// ============================================================================
// Test 1b: Completion digest preserves multiple active claim outcomes
// ============================================================================

#[test]
fn completion_digest_preserves_multiple_active_claim_subjects_and_bodies() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");
    exo_phase_start_with_storage(root, "sqlite");

    exo_cmd_with_storage(root, "sqlite")
        .args(["goal", "add", "Digest goal", "--id", "digest-goal"])
        .assert()
        .success();

    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "First outcome subject",
            "--body",
            "First outcome body",
            "--entity-type",
            "goal",
            "--entity-id",
            "digest-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "Second outcome subject",
            "--body",
            "Second outcome body",
            "--entity-type",
            "goal",
            "--entity-id",
            "digest-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    let output = exo_cmd_with_storage(root, "sqlite")
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = parse_stdout_json(&output);
    let steering = json
        .get("result")
        .and_then(|r| r.get("steering"))
        .expect("expected steering in status output");

    let digest = steering
        .get("completion_digests")
        .and_then(|v| v.as_array())
        .expect("completion digests")
        .iter()
        .find(|digest| {
            digest.get("entity_type").and_then(|v| v.as_str()) == Some("goal")
                && digest.get("entity_id").and_then(|v| v.as_str()) == Some("digest-goal")
        })
        .expect("digest for goal");

    let claims = digest
        .get("claims")
        .and_then(|v| v.as_array())
        .expect("digest claims");
    assert_eq!(claims.len(), 2);
    let subjects = claims
        .iter()
        .map(|claim| claim.get("subject").and_then(|v| v.as_str()).unwrap_or(""))
        .collect::<Vec<_>>();
    let bodies = claims
        .iter()
        .map(|claim| claim.get("body").and_then(|v| v.as_str()).unwrap_or(""))
        .collect::<Vec<_>>();

    assert!(subjects.contains(&"First outcome subject"));
    assert!(subjects.contains(&"Second outcome subject"));
    assert!(bodies.contains(&"First outcome body"));
    assert!(bodies.contains(&"Second outcome body"));

    let summaries = steering
        .get("perception_summaries")
        .and_then(|v| v.as_array())
        .expect("perception summaries");
    let summary = summaries
        .iter()
        .find(|summary| {
            summary.get("entity_type").and_then(|v| v.as_str()) == Some("goal")
                && summary.get("entity_id").and_then(|v| v.as_str()) == Some("digest-goal")
        })
        .expect("perception summary for goal");
    let summary_subjects = summary
        .get("subjects")
        .and_then(|v| v.as_array())
        .expect("summary subjects")
        .iter()
        .map(|subject| subject.as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    assert!(summary_subjects.contains(&"First outcome subject"));
    assert!(summary_subjects.contains(&"Second outcome subject"));
}

// ============================================================================
// Test 2: Repair action for concern on completed goal via exo status
// ============================================================================

#[test]
fn repair_action_for_concern_on_completed_goal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");
    exo_phase_start_with_storage(root, "sqlite");

    // Add and complete a goal
    exo_cmd_with_storage(root, "sqlite")
        .args(["goal", "add", "Completed goal", "--id", "done-goal"])
        .assert()
        .success();

    // Add a completion claim
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "Done",
            "--entity-type",
            "goal",
            "--entity-id",
            "done-goal",
            "--intent",
            "claim",
        ])
        .assert()
        .success();

    // Complete the goal
    exo_cmd_with_storage(root, "sqlite")
        .args(["goal", "complete", "done-goal", "--log", "Finished"])
        .assert()
        .success();

    // Now add a concern on the completed goal
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "I'm not sure about this implementation",
            "--entity-type",
            "goal",
            "--entity-id",
            "done-goal",
            "--intent",
            "concern",
            "--priority",
            "immediate",
        ])
        .assert()
        .success();

    // Run exo status --format json
    let output = exo_cmd_with_storage(root, "sqlite")
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = parse_stdout_json(&output);

    let steering = json
        .get("result")
        .and_then(|r| r.get("steering"))
        .expect("expected steering in status output");

    let repairs = steering
        .get("repair_actions")
        .and_then(|v| v.as_array())
        .expect("expected repair_actions array");

    let has_concern_repair = repairs.iter().any(|r| {
        r.get("label")
            .and_then(|v| v.as_str())
            .is_some_and(|label| label.contains("Review concern on completed goal"))
    });
    assert!(
        has_concern_repair,
        "expected a repair action for concern on completed goal, got: {repairs:?}"
    );
}

// ============================================================================
// Test 3: World steering perception via exo status
// ============================================================================

#[test]
fn world_steering_perception_via_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");
    exo_phase_start_with_storage(root, "sqlite");

    // Add a goal so the phase has work
    exo_cmd_with_storage(root, "sqlite")
        .args(["goal", "add", "Some goal", "--id", "status-goal"])
        .assert()
        .success();

    // Add a project-level inbox item with immediate priority
    exo_cmd_with_storage(root, "sqlite")
        .args([
            "inbox",
            "add",
            "General feedback on project direction",
            "--entity-type",
            "project",
            "--intent",
            "fyi",
            "--priority",
            "immediate",
        ])
        .assert()
        .success();

    // Run exo status --format json
    let output = exo_cmd_with_storage(root, "sqlite")
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = parse_stdout_json(&output);

    let steering = json
        .get("result")
        .and_then(|r| r.get("steering"))
        .expect("expected steering in status output");

    let summaries = steering
        .get("perception_summaries")
        .and_then(|v| v.as_array())
        .expect("expected perception_summaries array");

    assert!(
        !summaries.is_empty(),
        "perception_summaries should contain the project-level inbox item"
    );

    // The summary should reference our feedback
    let has_feedback = summaries.iter().any(|s| {
        s.get("sample_subject")
            .and_then(|v| v.as_str())
            .is_some_and(|subj| subj.contains("General feedback"))
    });
    assert!(
        has_feedback,
        "expected a perception summary with the feedback subject, got: {summaries:?}"
    );
}
