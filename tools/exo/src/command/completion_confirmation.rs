use crate::api::protocol::{ErrorCode, WorkflowConfirmationDecision, WorkflowConfirmationInput};
use crate::command::traits::MutableCommandContext;
use crate::context::SqliteWriter;
use crate::context::sqlite_loader::CompletionClaimStatus;
use crate::failure::ExoFailure;
use crate::steering::{
    CompletionOutcomeDigestSummary, ProgressMode, SteeringBlock, SuggestedAction, WorkIntent,
};
use serde::Serialize;

const WORKFLOW_COMPLETION_CONFIRMATION_KIND: &str = "workflow_completion_confirmation";
const OUTCOME_REVIEW_CONFIRMATION_ALIAS: &str = "outcome_review";
const APPROVED_OUTCOME_SUBJECT: &str = "Outcome approved";

fn is_workflow_completion_confirmation_kind(kind: &str) -> bool {
    matches!(
        kind,
        WORKFLOW_COMPLETION_CONFIRMATION_KIND | OUTCOME_REVIEW_CONFIRMATION_ALIAS
    )
}

pub(super) fn workflow_completion_confirmation_matches(
    confirmation: &WorkflowConfirmationInput,
    entity_type: &str,
    entity_id: &str,
    outcome: &str,
) -> bool {
    is_workflow_completion_confirmation_kind(&confirmation.kind)
        && confirmation.entity_type == entity_type
        && confirmation.entity_id == entity_id
        && matches!(
            confirmation.decision,
            WorkflowConfirmationDecision::YesComplete
        )
        && confirmation.outcome == outcome
}

pub(super) fn record_workflow_completion_evidence(
    ctx: &MutableCommandContext,
    entity_type: &str,
    entity_id: &str,
    outcome: &str,
) -> anyhow::Result<bool> {
    let Some(confirmation) = ctx.workflow_confirmation.as_ref() else {
        return Ok(false);
    };
    if !workflow_completion_confirmation_matches(confirmation, entity_type, entity_id, outcome) {
        return Ok(false);
    }

    record_completion_approval_evidence(ctx, entity_type, entity_id, outcome)?;
    Ok(true)
}

pub(super) fn record_completion_approval_evidence(
    ctx: &MutableCommandContext,
    entity_type: &str,
    entity_id: &str,
    outcome: &str,
) -> anyhow::Result<()> {
    let writer = SqliteWriter::open(ctx.db_path())?;
    let evidence_id = writer.add_inbox_item(
        entity_type,
        Some(entity_id),
        "user-feedback",
        "claim",
        "next-touch",
        None,
        None,
        APPROVED_OUTCOME_SUBJECT,
        outcome,
        None,
    )?;
    writer.update_inbox_status(&evidence_id, "acknowledged", None)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct WorkflowCompletionConfirmation {
    pub kind: &'static str,
    pub evidence_recorded: bool,
    pub entity_type: &'static str,
    pub entity_id: String,
    pub completion_input: WorkflowCompletionInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_digest: Option<CompletionOutcomeDigestSummary>,
    pub header: String,
    pub question: String,
    pub message: String,
    pub readiness_rationale: String,
    pub proposed_outcome: String,
    pub options: Vec<WorkflowConfirmationOption>,
    pub branch_instructions: WorkflowConfirmationBranches,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct WorkflowCompletionInput {
    pub kind: &'static str,
    pub entity_type: &'static str,
    pub entity_id: String,
    pub decision: &'static str,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct WorkflowConfirmationOption {
    pub label: &'static str,
    pub value: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct WorkflowConfirmationBranches {
    pub yes_complete: &'static str,
    pub revise_outcome: &'static str,
    pub not_complete_yet: &'static str,
    pub discuss: &'static str,
}

pub(super) fn goal_workflow_confirmation(
    goal_id: &str,
    _goal_label: &str,
    proposed_outcome: &str,
    child_task_count: usize,
    workflow_evidence_recorded: bool,
    completion_digest: Option<CompletionOutcomeDigestSummary>,
) -> WorkflowCompletionConfirmation {
    let readiness_rationale = if child_task_count == 1 {
        "The child task is complete. The goal outcome is ready for review.".to_string()
    } else if child_task_count == 0 {
        "The goal outcome is ready for review.".to_string()
    } else {
        format!(
            "All {child_task_count} child tasks are complete. The goal outcome is ready for review."
        )
    };
    let message = format!("{readiness_rationale}\n\nProposed outcome: {proposed_outcome}");

    WorkflowCompletionConfirmation {
        kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
        evidence_recorded: workflow_evidence_recorded,
        entity_type: "goal",
        entity_id: goal_id.to_string(),
        completion_input: WorkflowCompletionInput {
            kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
            entity_type: "goal",
            entity_id: goal_id.to_string(),
            decision: "yes_complete",
            outcome: proposed_outcome.to_string(),
        },
        completion_digest,
        header: "Outcome ready for review".to_string(),
        question: "Approve this outcome?".to_string(),
        message,
        readiness_rationale,
        proposed_outcome: proposed_outcome.to_string(),
        options: vec![
            WorkflowConfirmationOption {
                label: "Approve outcome",
                value: "yes_complete",
                description: "Record this outcome and close the goal.",
            },
            WorkflowConfirmationOption {
                label: "Revise outcome",
                value: "revise_outcome",
                description: "Use a revised outcome summary before completing the goal.",
            },
            WorkflowConfirmationOption {
                label: "Keep working",
                value: "not_complete_yet",
                description: "Leave the goal pending and continue work.",
            },
            WorkflowConfirmationOption {
                label: "Discuss first",
                value: "discuss",
                description: "Discuss what needs to change before updating completion state.",
            },
        ],
        branch_instructions: WorkflowConfirmationBranches {
            yes_complete: if workflow_evidence_recorded {
                "The outcome is approved and completion evidence has been recorded. Continue closing the goal."
            } else {
                "Record the approved outcome and close the goal."
            },
            revise_outcome: "Use the revised outcome summary before completing the goal.",
            not_complete_yet: "Leave the goal pending. If there is remaining work, add or update tasks before trying again.",
            discuss: "Discuss what needs to change before updating completion state.",
        },
    }
}

pub(super) fn task_workflow_confirmation(
    task_id: &str,
    proposed_outcome: &str,
    workflow_evidence_recorded: bool,
    completion_digest: Option<CompletionOutcomeDigestSummary>,
) -> WorkflowCompletionConfirmation {
    let readiness_rationale = "The task outcome is ready for review.".to_string();
    let message = format!("{readiness_rationale}\n\nProposed outcome: {proposed_outcome}");

    WorkflowCompletionConfirmation {
        kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
        evidence_recorded: workflow_evidence_recorded,
        entity_type: "task",
        entity_id: task_id.to_string(),
        completion_input: WorkflowCompletionInput {
            kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
            entity_type: "task",
            entity_id: task_id.to_string(),
            decision: "yes_complete",
            outcome: proposed_outcome.to_string(),
        },
        completion_digest,
        header: "Outcome ready for review".to_string(),
        question: "Approve this outcome?".to_string(),
        message,
        readiness_rationale,
        proposed_outcome: proposed_outcome.to_string(),
        options: vec![
            WorkflowConfirmationOption {
                label: "Approve outcome",
                value: "yes_complete",
                description: "Record this outcome and close the task.",
            },
            WorkflowConfirmationOption {
                label: "Revise outcome",
                value: "revise_outcome",
                description: "Use a revised outcome summary before completing the task.",
            },
            WorkflowConfirmationOption {
                label: "Keep working",
                value: "not_complete_yet",
                description: "Leave the task pending and continue work.",
            },
            WorkflowConfirmationOption {
                label: "Discuss first",
                value: "discuss",
                description: "Discuss what needs to change before updating completion state.",
            },
        ],
        branch_instructions: WorkflowConfirmationBranches {
            yes_complete: if workflow_evidence_recorded {
                "The outcome is approved and completion evidence has been recorded. Continue closing the task."
            } else {
                "Record the approved outcome and close the task."
            },
            revise_outcome: "Use the revised outcome summary before completing the task.",
            not_complete_yet: "Leave the task pending. If there is remaining work, add or update tasks before trying again.",
            discuss: "Discuss what needs to change before updating completion state.",
        },
    }
}

pub(super) fn completion_confirmation_failure_with_workflow(
    entity_type: &str,
    entity_id: &str,
    status: CompletionClaimStatus,
    workflow_confirmation: Option<WorkflowCompletionConfirmation>,
) -> Option<ExoFailure> {
    let entity = CompletionEntity::new(entity_type, entity_id);
    let has_review_prompt = workflow_confirmation.is_some();
    let steering_mode = if has_review_prompt {
        CompletionSteeringMode::ReviewOutcome
    } else {
        CompletionSteeringMode::ConfirmCompletion
    };

    match status {
        CompletionClaimStatus::HumanClaim | CompletionClaimStatus::AgentClaimAcknowledged => None,
        CompletionClaimStatus::NoClaim => {
            if has_review_prompt {
                Some(blocked_completion_failure(
                    entity,
                    "Outcome ready for review.",
                    "Approve, revise, continue, or discuss the outcome before recording completion.",
                    "Present outcome for review",
                    "Show the proposed outcome and wait for an approval decision.",
                    steering_mode,
                    workflow_confirmation,
                ))
            } else {
                Some(blocked_completion_failure(
                    entity,
                    "Outcome approval needed.",
                    "Approve the outcome before recording completion.",
                    "Present outcome for review",
                    "Show the proposed outcome and wait for an approval decision.",
                    steering_mode,
                    workflow_confirmation,
                ))
            }
        }
        CompletionClaimStatus::AgentClaimPending => {
            if has_review_prompt {
                Some(blocked_completion_failure(
                    entity,
                    "Outcome ready for review.",
                    "Approve, revise, continue, or discuss the outcome before recording completion.",
                    "Present outcome for review",
                    "Show the proposed outcome and wait for an approval decision.",
                    steering_mode,
                    workflow_confirmation,
                ))
            } else {
                Some(blocked_completion_failure(
                    entity,
                    "Outcome approval needed.",
                    "Approve the outcome before recording completion.",
                    "Present outcome for review",
                    "Show the proposed outcome and wait for an approval decision.",
                    steering_mode,
                    workflow_confirmation,
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CompletionSteeringMode {
    ConfirmCompletion,
    ReviewOutcome,
}

#[derive(Debug, Clone, Copy)]
struct CompletionEntity<'a> {
    entity_type: &'a str,
    entity_id: &'a str,
}

impl<'a> CompletionEntity<'a> {
    const fn new(entity_type: &'a str, entity_id: &'a str) -> Self {
        Self {
            entity_type,
            entity_id,
        }
    }

    fn label(self) -> String {
        format!("{} '{}'", self.entity_type, self.entity_id)
    }
}

fn blocked_completion_failure(
    entity: CompletionEntity<'_>,
    state: &str,
    repair: &str,
    action_label: &str,
    action_rationale: &str,
    steering_mode: CompletionSteeringMode,
    workflow_confirmation: Option<WorkflowCompletionConfirmation>,
) -> ExoFailure {
    let message = match steering_mode {
        CompletionSteeringMode::ReviewOutcome => {
            format!(
                "Outcome review needed for {}: {state} {repair}",
                entity.label()
            )
        }
        CompletionSteeringMode::ConfirmCompletion => {
            format!("Cannot complete {}: {state} {repair}", entity.label())
        }
    };

    let mut details = serde_json::json!({
        "entity_type": entity.entity_type,
        "entity_id": entity.entity_id,
        "blocked_state": state,
        "repair": repair,
    });

    if let Some(workflow_confirmation) = workflow_confirmation
        && let Some(details) = details.as_object_mut()
    {
        details.insert(
            "workflow_confirmation".to_string(),
            serde_json::to_value(workflow_confirmation).unwrap_or(serde_json::Value::Null),
        );
    }

    ExoFailure::new(
        ErrorCode::PreconditionFailed,
        message,
        completion_confirmation_steering(steering_mode, repair, action_label, action_rationale),
    )
    .with_details(details)
}

fn completion_confirmation_steering(
    mode: CompletionSteeringMode,
    repair: &str,
    action_label: &str,
    action_rationale: &str,
) -> SteeringBlock {
    let (situation, command) = match mode {
        CompletionSteeringMode::ConfirmCompletion => (
            "The outcome needs approval before it is recorded.",
            "Present the proposed outcome for review.",
        ),
        CompletionSteeringMode::ReviewOutcome => (
            "The outcome is ready for review before it is recorded.",
            "Present the proposed outcome for review.",
        ),
    };

    SteeringBlock {
        primary_intent: WorkIntent::Record,
        progress_mode: ProgressMode::Executing,
        situation: situation.to_string(),
        next_actions: vec![SuggestedAction::human_action(
            action_label,
            command,
            action_rationale,
            WorkIntent::Record,
            Some(0.95),
        )],
        repair_actions: vec![SuggestedAction::human_action(
            "State the completed outcome",
            "Describe what is complete and ask for approval.",
            repair,
            WorkIntent::Record,
            Some(0.9),
        )],
        perception_summaries: vec![],
        completion_digests: vec![],
        rfc_context: vec![],
        session_boundary: None,
        entity_context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_completion_failure_uses_confirmation_steering() {
        let failure = completion_confirmation_failure_with_workflow(
            "task",
            "goal::task",
            CompletionClaimStatus::NoClaim,
            None,
        )
        .expect("task completion should be blocked");

        assert_eq!(
            failure.steering.situation,
            "The outcome needs approval before it is recorded."
        );
        assert_eq!(
            failure.steering.next_actions[0].command,
            "Present the proposed outcome for review."
        );

        let steering = serde_json::to_string(&failure.steering).expect("serialize steering");
        assert!(steering.contains("approval"));
        assert!(steering.contains("Present the proposed outcome for review."));
    }
}
