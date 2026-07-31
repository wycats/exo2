#![allow(clippy::redundant_pub_crate)]

use super::{WorkbenchHostInner, WorkbenchSession, WorkbenchState, snapshot};
use crate::api::protocol::{
    CallParams, Effect, ErrorBody, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope,
    ResponseEnvelope, Status,
};
use crate::command::completion_confirmation;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use std::path::Path;

pub(crate) const PLANNING_PROTOCOL_VERSION: u32 = 2;
pub(crate) const PLANNING_CONTEXT_FIELD: &str = "_exo_workbench_planning";
pub(super) const MAX_COMPLETION_REVIEWS_IN_FLIGHT: usize = 32;
pub(crate) const PLANNING_CAPABILITIES: [&str; 7] = [
    "workbench.task.add",
    "workbench.task.update",
    "workbench.task.reorder",
    "workbench.task.start",
    "workbench.task.log",
    "workbench.task.complete.review",
    "workbench.task.complete.approve",
];

const MAX_TITLE_BYTES: usize = 512;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub(super) const MAX_COMPLETION_REVIEWS_PER_SESSION: usize = 32;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrowserPlanningRequest {
    pub protocol_version: u32,
    pub id: String,
    pub session_key: String,
    pub expected_daemon_instance_id: String,
    pub expected_revision: u64,
    pub expected_phase_id: String,
    pub operation: BrowserPlanningOperation,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::enum_variant_names)]
pub(super) enum BrowserPlanningOperation {
    TaskAdd {
        goal_id: String,
        title: String,
    },
    TaskUpdate {
        task_id: String,
        title: String,
    },
    TaskReorder {
        task_id: String,
        position: usize,
    },
    TaskStart {
        task_id: String,
    },
    TaskLog {
        task_id: String,
        message: String,
    },
    TaskCompleteReview {
        task_id: String,
        outcome: String,
    },
    TaskCompleteApprove {
        review_id: String,
        task_id: String,
        outcome: String,
    },
}

impl BrowserPlanningOperation {
    pub(super) const fn capability(&self) -> &'static str {
        match self {
            Self::TaskAdd { .. } => "workbench.task.add",
            Self::TaskUpdate { .. } => "workbench.task.update",
            Self::TaskReorder { .. } => "workbench.task.reorder",
            Self::TaskStart { .. } => "workbench.task.start",
            Self::TaskLog { .. } => "workbench.task.log",
            Self::TaskCompleteReview { .. } => "workbench.task.complete.review",
            Self::TaskCompleteApprove { .. } => "workbench.task.complete.approve",
        }
    }

    pub(super) const fn operation_name(&self) -> &'static str {
        match self {
            Self::TaskAdd { .. } => "task_add",
            Self::TaskUpdate { .. } => "task_update",
            Self::TaskReorder { .. } => "task_reorder",
            Self::TaskStart { .. } => "task_start",
            Self::TaskLog { .. } => "task_log",
            Self::TaskCompleteReview { .. } => "task_complete_review",
            Self::TaskCompleteApprove { .. } => "task_complete_approve",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WorkbenchPlanningContext {
    pub schema_version: u8,
    pub session_id: String,
    pub expected_daemon_instance_id: String,
    pub expected_revision: u64,
    pub expected_phase_id: String,
    pub operation: WorkbenchPlanningContextOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum WorkbenchPlanningContextOperation {
    TaskAdd { goal_id: String },
    TaskUpdate { task_id: String },
    TaskReorder { task_id: String, position: usize },
    TaskStart { task_id: String },
    TaskLog { task_id: String },
    TaskCompleteReview { task_id: String },
    TaskCompleteApprove { task_id: String, review_id: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct WorkbenchTaskCompletionReview {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub review_id: String,
    pub task_id: String,
    pub readiness_rationale: String,
    pub proposed_outcome: String,
    pub approval_evidence_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletionReviewFingerprint {
    pub expected_daemon_instance_id: String,
    pub expected_revision: u64,
    pub expected_phase_id: String,
    pub task_id: String,
    pub proposed_outcome: String,
}

#[derive(Debug, Clone)]
pub(super) struct CompletionReviewRequestRecord {
    pub fingerprint: CompletionReviewFingerprint,
    pub review_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct CompletionReviewRecord {
    pub session_id: String,
    pub expected_daemon_instance_id: String,
    pub expected_revision: u64,
    pub expected_phase_id: String,
    pub task_id: String,
    pub proposed_outcome: String,
    pub result: WorkbenchTaskCompletionReview,
    pub approval_request_id: Option<String>,
    pub consumed: bool,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CompletionReviewRequestKey {
    pub session_id: String,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedCompletionApproval {
    pub task_id: String,
    pub proposed_outcome: String,
    pub context: WorkbenchPlanningContext,
}

fn evict_completion_review_if_needed(state: &mut WorkbenchState, session_id: &str) {
    while state
        .completion_reviews
        .values()
        .filter(|review| review.session_id == session_id)
        .count()
        >= MAX_COMPLETION_REVIEWS_PER_SESSION
    {
        let Some(review_id) = state
            .completion_reviews
            .iter()
            .filter(|(_, review)| review.session_id == session_id)
            .min_by_key(|(_, review)| (if review.consumed { 0 } else { 1 }, review.sequence))
            .map(|(review_id, _)| review_id.clone())
        else {
            break;
        };
        state.completion_reviews.remove(&review_id);
        state
            .completion_review_requests
            .retain(|_, request| request.review_id != review_id);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkbenchPlanningError {
    kind: &'static str,
    message: &'static str,
    code: ErrorCode,
    retry_with_same_request_id: bool,
}

impl WorkbenchPlanningError {
    pub(crate) const fn invalid_request() -> Self {
        Self::new(
            "workbench.invalid_request",
            "The workbench planning request is invalid",
            ErrorCode::InvalidInput,
            false,
        )
    }

    pub(crate) const fn invalid_input() -> Self {
        Self::new(
            "workbench.invalid_input",
            "The workbench planning input is invalid",
            ErrorCode::InvalidInput,
            false,
        )
    }

    pub(crate) const fn stale_snapshot() -> Self {
        Self::new(
            "workbench.stale_snapshot",
            "The workbench snapshot is stale",
            ErrorCode::PreconditionFailed,
            false,
        )
    }

    pub(crate) const fn phase_mismatch() -> Self {
        Self::new(
            "workbench.phase_mismatch",
            "The workspace no longer focuses the expected phase",
            ErrorCode::PreconditionFailed,
            false,
        )
    }

    pub(crate) const fn entity_outside_phase() -> Self {
        Self::new(
            "workbench.entity_outside_phase",
            "The selected planning entity is outside the focused phase",
            ErrorCode::PreconditionFailed,
            false,
        )
    }

    pub(crate) const fn invalid_transition() -> Self {
        Self::new(
            "workbench.invalid_transition",
            "The task state does not allow this operation",
            ErrorCode::PreconditionFailed,
            false,
        )
    }

    pub(crate) const fn review_invalid() -> Self {
        Self::new(
            "workbench.review_invalid",
            "The task completion review is no longer valid",
            ErrorCode::PreconditionFailed,
            false,
        )
    }

    pub(crate) const fn internal() -> Self {
        Self::new(
            "workbench.command_failed",
            "The workbench planning request could not be completed",
            ErrorCode::Internal,
            true,
        )
    }

    pub(crate) const fn busy() -> Self {
        Self::new(
            "workbench.busy",
            "The workbench planning service is busy",
            ErrorCode::PreconditionFailed,
            true,
        )
    }

    const fn new(
        kind: &'static str,
        message: &'static str,
        code: ErrorCode,
        retry_with_same_request_id: bool,
    ) -> Self {
        Self {
            kind,
            message,
            code,
            retry_with_same_request_id,
        }
    }

    pub(crate) fn response(&self, id: String) -> ResponseEnvelope {
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id,
            status: Status::Error,
            result: None,
            error: Some(ErrorBody {
                code: self.code,
                message: self.message.to_string(),
                details: Some(json!({
                    "kind": self.kind,
                    "retry_with_same_request_id": self.retry_with_same_request_id,
                })),
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        }
    }
}

pub(super) fn normalize_title(value: &str) -> Result<String, WorkbenchPlanningError> {
    let title = value.trim();
    if title.is_empty()
        || title.len() > MAX_TITLE_BYTES
        || title.contains('\n')
        || title.contains('\r')
    {
        return Err(WorkbenchPlanningError::invalid_input());
    }
    Ok(title.to_string())
}

pub(super) fn validate_message(value: &str) -> Result<(), WorkbenchPlanningError> {
    if value.trim().is_empty() || value.len() > MAX_MESSAGE_BYTES {
        return Err(WorkbenchPlanningError::invalid_input());
    }
    Ok(())
}

pub(crate) fn attach_context(
    request: &mut RequestEnvelope,
    context: &WorkbenchPlanningContext,
) -> Result<(), WorkbenchPlanningError> {
    let Op::Call(CallParams { input, .. }) = &mut request.op else {
        return Err(WorkbenchPlanningError::invalid_request());
    };
    let Some(input) = input.as_object_mut() else {
        return Err(WorkbenchPlanningError::invalid_request());
    };
    input.insert(
        PLANNING_CONTEXT_FIELD.to_string(),
        serde_json::to_value(context).map_err(|_| WorkbenchPlanningError::internal())?,
    );
    Ok(())
}

pub(crate) fn context_from_request(
    request: &RequestEnvelope,
) -> Result<Option<WorkbenchPlanningContext>, WorkbenchPlanningError> {
    let Op::Call(CallParams { input, .. }) = &request.op else {
        return Ok(None);
    };
    let Some(input) = input.as_object() else {
        return Ok(None);
    };
    input
        .get(PLANNING_CONTEXT_FIELD)
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| WorkbenchPlanningError::invalid_request())
        })
        .transpose()
}

pub(crate) fn request_without_context(request: &RequestEnvelope) -> RequestEnvelope {
    let mut request = request.clone();
    if let Op::Call(CallParams { input, .. }) = &mut request.op
        && let Some(input) = input.as_object_mut()
    {
        input.remove(PLANNING_CONTEXT_FIELD);
    }
    request
}

pub(super) fn mutation_request(
    id: String,
    workspace_root: std::path::PathBuf,
    session: &WorkbenchSession,
    expected_daemon_instance_id: String,
    expected_revision: u64,
    expected_phase_id: String,
    operation: &BrowserPlanningOperation,
) -> Result<RequestEnvelope, WorkbenchPlanningError> {
    let (path, input, context_operation) = match operation {
        BrowserPlanningOperation::TaskAdd { goal_id, title } => {
            let title = normalize_title(title)?;
            (
                vec!["task".to_string(), "add".to_string()],
                json!({ "label": title, "goal": goal_id }),
                WorkbenchPlanningContextOperation::TaskAdd {
                    goal_id: goal_id.clone(),
                },
            )
        }
        BrowserPlanningOperation::TaskUpdate { task_id, title } => {
            let title = normalize_title(title)?;
            (
                vec!["task".to_string(), "update".to_string()],
                json!({ "id": task_id, "title": title }),
                WorkbenchPlanningContextOperation::TaskUpdate {
                    task_id: task_id.clone(),
                },
            )
        }
        BrowserPlanningOperation::TaskReorder { task_id, position } => (
            vec!["task".to_string(), "reorder".to_string()],
            json!({ "id": task_id, "position": position.to_string() }),
            WorkbenchPlanningContextOperation::TaskReorder {
                task_id: task_id.clone(),
                position: *position,
            },
        ),
        BrowserPlanningOperation::TaskStart { task_id } => (
            vec!["task".to_string(), "start".to_string()],
            json!({ "id": task_id }),
            WorkbenchPlanningContextOperation::TaskStart {
                task_id: task_id.clone(),
            },
        ),
        BrowserPlanningOperation::TaskLog { task_id, message } => {
            validate_message(message)?;
            (
                vec!["task".to_string(), "log".to_string()],
                json!({ "id": task_id, "message": message }),
                WorkbenchPlanningContextOperation::TaskLog {
                    task_id: task_id.clone(),
                },
            )
        }
        BrowserPlanningOperation::TaskCompleteReview { .. }
        | BrowserPlanningOperation::TaskCompleteApprove { .. } => {
            return Err(WorkbenchPlanningError::invalid_request());
        }
    };
    let context = WorkbenchPlanningContext {
        schema_version: 1,
        session_id: session.id.clone(),
        expected_daemon_instance_id,
        expected_revision,
        expected_phase_id,
        operation: context_operation,
    };
    let mut request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        op: Op::Call(CallParams {
            address: crate::api::protocol::Address::Operation { path },
            input,
        }),
        workspace_root: Some(workspace_root),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };
    attach_context(&mut request, &context)?;
    Ok(request)
}

pub(super) fn completion_approval_request(
    id: String,
    workspace_root: std::path::PathBuf,
    session: &WorkbenchSession,
    expected_daemon_instance_id: String,
    expected_revision: u64,
    expected_phase_id: String,
    review_id: String,
    task_id: String,
    proposed_outcome: String,
) -> Result<RequestEnvelope, WorkbenchPlanningError> {
    if task_id.trim().is_empty() || review_id.trim().is_empty() {
        return Err(WorkbenchPlanningError::invalid_input());
    }
    validate_message(&proposed_outcome)?;
    let context = WorkbenchPlanningContext {
        schema_version: 1,
        session_id: session.id.clone(),
        expected_daemon_instance_id,
        expected_revision,
        expected_phase_id,
        operation: WorkbenchPlanningContextOperation::TaskCompleteApprove {
            task_id: task_id.clone(),
            review_id,
        },
    };
    let mut request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        op: Op::Call(CallParams {
            address: crate::api::protocol::Address::Operation {
                path: vec!["task".to_string(), "complete".to_string()],
            },
            input: json!({
                "id": task_id,
                "log": proposed_outcome,
            }),
        }),
        workspace_root: Some(workspace_root),
        auth: None,
        workflow_confirmation: Some(crate::api::protocol::WorkflowConfirmationInput {
            kind: "workflow_completion_confirmation".to_string(),
            entity_type: "task".to_string(),
            entity_id: task_id,
            decision: crate::api::protocol::WorkflowConfirmationDecision::YesComplete,
            outcome: proposed_outcome,
        }),
        agent_id: None,
    };
    attach_context(&mut request, &context)?;
    Ok(request)
}

impl WorkbenchHostInner {
    pub(super) fn completion_review(
        &self,
        session: &WorkbenchSession,
        request_id: &str,
        expected_daemon_instance_id: &str,
        expected_revision: u64,
        expected_phase_id: &str,
        task_id: &str,
        proposed_outcome: &str,
    ) -> Result<WorkbenchTaskCompletionReview, WorkbenchPlanningError> {
        validate_message(proposed_outcome)?;
        let fingerprint = CompletionReviewFingerprint {
            expected_daemon_instance_id: expected_daemon_instance_id.to_string(),
            expected_revision,
            expected_phase_id: expected_phase_id.to_string(),
            task_id: task_id.to_string(),
            proposed_outcome: proposed_outcome.to_string(),
        };
        let request_key = CompletionReviewRequestKey {
            session_id: session.id.clone(),
            request_id: request_id.to_string(),
        };

        if let Some(result) = self.cached_completion_review(&request_key, &fingerprint)? {
            return Ok(result);
        }

        let _gate = self
            .project_state_gate
            .lock()
            .map_err(|_| WorkbenchPlanningError::internal())?;
        if let Some(result) = self.cached_completion_review(&request_key, &fingerprint)? {
            return Ok(result);
        }

        let context = WorkbenchPlanningContext {
            schema_version: 1,
            session_id: session.id.clone(),
            expected_daemon_instance_id: expected_daemon_instance_id.to_string(),
            expected_revision,
            expected_phase_id: expected_phase_id.to_string(),
            operation: WorkbenchPlanningContextOperation::TaskCompleteReview {
                task_id: task_id.to_string(),
            },
        };
        self.validate_planning_context(&session.workspace_root, &context, true)?;
        let review = completion_confirmation::review_task_completion(
            &session.workspace_root,
            Some(&self.project),
            task_id,
            proposed_outcome,
        )
        .map_err(|_| WorkbenchPlanningError::internal())?;
        let review_id = super::random_token().map_err(|_| WorkbenchPlanningError::internal())?;
        let result = WorkbenchTaskCompletionReview {
            kind: "workbench.task_completion_review",
            ok: true,
            schema_version: 1,
            review_id: review_id.clone(),
            task_id: review.task_id.clone(),
            readiness_rationale: review.readiness_rationale,
            proposed_outcome: review.proposed_outcome.clone(),
            approval_evidence_present: review.approval_evidence_present,
        };

        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkbenchPlanningError::internal())?;
        evict_completion_review_if_needed(&mut state, &session.id);
        state.completion_review_sequence = state.completion_review_sequence.saturating_add(1);
        let sequence = state.completion_review_sequence;
        state.completion_review_requests.insert(
            request_key,
            CompletionReviewRequestRecord {
                fingerprint,
                review_id: review_id.clone(),
            },
        );
        state.completion_reviews.insert(
            review_id,
            CompletionReviewRecord {
                session_id: session.id.clone(),
                expected_daemon_instance_id: expected_daemon_instance_id.to_string(),
                expected_revision,
                expected_phase_id: expected_phase_id.to_string(),
                task_id: review.task_id,
                proposed_outcome: review.proposed_outcome,
                result: result.clone(),
                approval_request_id: None,
                consumed: false,
                sequence,
            },
        );
        drop(state);
        Ok(result)
    }

    fn cached_completion_review(
        &self,
        request_key: &CompletionReviewRequestKey,
        fingerprint: &CompletionReviewFingerprint,
    ) -> Result<Option<WorkbenchTaskCompletionReview>, WorkbenchPlanningError> {
        let state = self
            .state
            .lock()
            .map_err(|_| WorkbenchPlanningError::internal())?;
        let Some(request) = state.completion_review_requests.get(request_key) else {
            return Ok(None);
        };
        if request.fingerprint != *fingerprint {
            return Err(WorkbenchPlanningError::invalid_input());
        }
        let result = state
            .completion_reviews
            .get(&request.review_id)
            .filter(|review| review.session_id == request_key.session_id)
            .map(|review| review.result.clone())
            .ok_or_else(WorkbenchPlanningError::review_invalid)?;
        drop(state);
        Ok(Some(result))
    }

    pub(super) fn prepare_completion_approval(
        &self,
        session: &WorkbenchSession,
        request_id: &str,
        expected_daemon_instance_id: &str,
        expected_revision: u64,
        expected_phase_id: &str,
        review_id: &str,
        task_id: &str,
        proposed_outcome: &str,
    ) -> Result<PreparedCompletionApproval, WorkbenchPlanningError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkbenchPlanningError::internal())?;
        let review = state
            .completion_reviews
            .get_mut(review_id)
            .filter(|review| review.session_id == session.id)
            .ok_or_else(WorkbenchPlanningError::review_invalid)?;
        if review.expected_daemon_instance_id != expected_daemon_instance_id
            || review.expected_revision != expected_revision
            || review.expected_phase_id != expected_phase_id
            || review.task_id != task_id
            || review.proposed_outcome != proposed_outcome
        {
            return Err(WorkbenchPlanningError::review_invalid());
        }
        if review
            .approval_request_id
            .as_deref()
            .is_some_and(|bound| bound != request_id)
        {
            return Err(WorkbenchPlanningError::review_invalid());
        }
        review.approval_request_id = Some(request_id.to_string());

        let approval = PreparedCompletionApproval {
            task_id: review.task_id.clone(),
            proposed_outcome: review.proposed_outcome.clone(),
            context: WorkbenchPlanningContext {
                schema_version: 1,
                session_id: session.id.clone(),
                expected_daemon_instance_id: expected_daemon_instance_id.to_string(),
                expected_revision,
                expected_phase_id: expected_phase_id.to_string(),
                operation: WorkbenchPlanningContextOperation::TaskCompleteApprove {
                    task_id: review.task_id.clone(),
                    review_id: review_id.to_string(),
                },
            },
        };
        drop(state);
        Ok(approval)
    }

    pub(super) fn mark_completion_review_consumed(
        &self,
        session_id: &str,
        review_id: &str,
        request_id: &str,
    ) {
        if let Ok(mut state) = self.state.lock()
            && let Some(review) = state.completion_reviews.get_mut(review_id)
            && review.session_id == session_id
            && review.approval_request_id.as_deref() == Some(request_id)
        {
            review.consumed = true;
        }
    }

    pub(crate) fn validate_planning_context(
        &self,
        workspace_root: &Path,
        context: &WorkbenchPlanningContext,
        review: bool,
    ) -> Result<(), WorkbenchPlanningError> {
        if context.schema_version != 1
            || context.expected_daemon_instance_id != self.instance_id.as_ref()
            || self.current_revision() != context.expected_revision
        {
            return Err(WorkbenchPlanningError::stale_snapshot());
        }

        let registered = {
            let state = self
                .state
                .lock()
                .map_err(|_| WorkbenchPlanningError::internal())?;
            let session = state
                .sessions
                .get(&context.session_id)
                .filter(|session| session.workspace_root == workspace_root)
                .ok_or_else(WorkbenchPlanningError::review_invalid)?;
            state
                .workspaces_by_key
                .get(&session.workspace_key)
                .cloned()
                .ok_or_else(WorkbenchPlanningError::review_invalid)?
        };
        let snapshot = snapshot::build(
            &self.project,
            &registered,
            context.expected_revision,
            &self.instance_id,
        )
        .map_err(|_| WorkbenchPlanningError::internal())?;
        let phase = snapshot
            .phase
            .as_ref()
            .filter(|phase| phase.id == context.expected_phase_id)
            .ok_or_else(WorkbenchPlanningError::phase_mismatch)?;
        let focus_is_coherent = snapshot.focused_lane.as_ref().is_some_and(|lane| {
            lane.summary.phase_id == context.expected_phase_id
                && lane.summary.focused_here
                && lane.summary.phase_status == "in-progress"
        }) && !snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lane.phase_focus_mismatch");
        if !focus_is_coherent || phase.status != "in-progress" {
            return Err(WorkbenchPlanningError::phase_mismatch());
        }

        match &context.operation {
            WorkbenchPlanningContextOperation::TaskAdd { goal_id } => {
                let goal = phase
                    .goals
                    .iter()
                    .find(|goal| goal.id == *goal_id)
                    .ok_or_else(WorkbenchPlanningError::entity_outside_phase)?;
                if !matches!(goal.status.as_str(), "pending" | "in-progress" | "active") {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
            }
            WorkbenchPlanningContextOperation::TaskUpdate { task_id } => {
                let (_, task) = find_task(phase, task_id)?;
                if !matches!(task.status.as_str(), "pending" | "in-progress") {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
            }
            WorkbenchPlanningContextOperation::TaskReorder { task_id, position } => {
                let (goal, task) = find_task(phase, task_id)?;
                if !matches!(task.status.as_str(), "pending" | "in-progress")
                    || *position >= goal.tasks.len()
                {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
            }
            WorkbenchPlanningContextOperation::TaskStart { task_id } => {
                let (_, task) = find_task(phase, task_id)?;
                if task.status != "pending" {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
            }
            WorkbenchPlanningContextOperation::TaskLog { task_id } => {
                let (_, task) = find_task(phase, task_id)?;
                if task.status != "in-progress" {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
            }
            WorkbenchPlanningContextOperation::TaskCompleteReview { task_id } => {
                let (_, task) = find_task(phase, task_id)?;
                if task.status != "in-progress" {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
                if !review {
                    return Err(WorkbenchPlanningError::invalid_request());
                }
            }
            WorkbenchPlanningContextOperation::TaskCompleteApprove { task_id, .. } => {
                let (_, task) = find_task(phase, task_id)?;
                if task.status != "in-progress" {
                    return Err(WorkbenchPlanningError::invalid_transition());
                }
                if review {
                    return Err(WorkbenchPlanningError::invalid_request());
                }
            }
        }
        Ok(())
    }
}

fn find_task<'a>(
    phase: &'a super::WorkbenchPhase,
    task_id: &str,
) -> Result<(&'a super::WorkbenchGoal, &'a super::WorkbenchTask), WorkbenchPlanningError> {
    phase
        .goals
        .iter()
        .find_map(|goal| {
            goal.tasks
                .iter()
                .find(|task| task.id == task_id)
                .map(|task| (goal, task))
        })
        .ok_or_else(WorkbenchPlanningError::entity_outside_phase)
}

pub(crate) fn planning_error_kind(response: &ResponseEnvelope) -> Option<&str> {
    response
        .error
        .as_ref()?
        .details
        .as_ref()?
        .get("kind")?
        .as_str()
}

pub(crate) fn safe_planning_error(response: &ResponseEnvelope) -> WorkbenchPlanningError {
    match planning_error_kind(response) {
        Some("workbench.stale_snapshot") => WorkbenchPlanningError::stale_snapshot(),
        Some("workbench.phase_mismatch") => WorkbenchPlanningError::phase_mismatch(),
        Some("workbench.entity_outside_phase") => WorkbenchPlanningError::entity_outside_phase(),
        Some("workbench.invalid_transition") => WorkbenchPlanningError::invalid_transition(),
        Some("workbench.invalid_input") => WorkbenchPlanningError::invalid_input(),
        Some("workbench.review_invalid") => WorkbenchPlanningError::review_invalid(),
        Some("daemon.busy" | "workbench.busy") => WorkbenchPlanningError::busy(),
        Some("daemon.request_id_conflict") => WorkbenchPlanningError::invalid_input(),
        _ if response.error.as_ref().is_some_and(|error| {
            matches!(
                error.code,
                ErrorCode::InvalidInput | ErrorCode::PreconditionFailed
            )
        }) =>
        {
            WorkbenchPlanningError::invalid_transition()
        }
        _ => WorkbenchPlanningError::internal(),
    }
}

pub(crate) fn safe_mutation_result(
    response: &ResponseEnvelope,
    operation: &str,
) -> Result<JsonValue, WorkbenchPlanningError> {
    let task_id = response
        .result
        .as_ref()
        .and_then(|result| result.get("task_id"))
        .and_then(JsonValue::as_str)
        .ok_or_else(WorkbenchPlanningError::internal)?;
    Ok(json!({
        "kind": "workbench.task_mutation",
        "ok": true,
        "schema_version": 1,
        "operation": operation,
        "task_id": task_id,
    }))
}

pub(crate) fn review_response(
    id: String,
    review: &WorkbenchTaskCompletionReview,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Ok,
        result: serde_json::to_value(review).ok(),
        error: None,
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: Some(Effect::Pure),
        trace: None,
    }
}
