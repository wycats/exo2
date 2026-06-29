use crate::api::protocol::{ErrorBody, ErrorCode, Status};
use crate::command_reference::ExoCommandReference;
use crate::steering::{SteeringBlock, SuggestedAction, WorkIntent};
use serde_json::Value as JsonValue;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ExoFailure {
    pub status: Status,
    pub error: ErrorBody,
    pub steering: SteeringBlock,
}

impl ExoFailure {
    pub fn new(code: ErrorCode, message: impl Into<String>, steering: SteeringBlock) -> Self {
        Self {
            status: Status::Error,
            error: ErrorBody {
                code,
                message: message.into(),
                details: None,
            },
            steering,
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: JsonValue) -> Self {
        self.error.details = Some(details);
        self
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn orienting_steering(next_actions: Vec<SuggestedAction>) -> SteeringBlock {
        SteeringBlock {
            primary_intent: WorkIntent::Orient,
            progress_mode: crate::steering::ProgressMode::BetweenPhases,
            situation: "An error occurred. Follow the suggested actions to recover.".to_string(),
            next_actions,
            repair_actions: vec![],
            perception_summaries: vec![],
            completion_digests: vec![],
            rfc_context: vec![],
            session_boundary: None,
            entity_context: None,
        }
    }

    pub fn plan_anchor_not_found(anchor_id: &str, context: &str) -> Self {
        Self::new(
            ErrorCode::NotFound,
            format!("{context} '{anchor_id}' not found (required by --after)"),
            Self::orienting_steering(vec![
                SuggestedAction::exo(
                    "Show active phase/epoch",
                    ExoCommandReference::new(&["phase", "status"]),
                    "Use phase status to orient in the current plan and find the active epoch.",
                    WorkIntent::Orient,
                    Some(0.8),
                ),
                SuggestedAction::exo(
                    "List epochs and IDs",
                    ExoCommandReference::new(&["plan", "review"]),
                    "Review the plan to see epoch IDs for use with --after.",
                    WorkIntent::Orient,
                    Some(0.8),
                ),
            ]),
        )
        .with_details(serde_json::json!({
            "anchor": anchor_id,
            "context": context,
            "flag": "--after"
        }))
    }
}

impl fmt::Display for ExoFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error.message)
    }
}

impl std::error::Error for ExoFailure {}
