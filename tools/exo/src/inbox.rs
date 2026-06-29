//! Inbox system for perception events (RFC 10181: Shared Perception).
//!
//! The inbox is a **steering channel** — a unified mechanism for user feedback,
//! system observations, and plan mutation events to enter the agent's perception
//! at the right time and in the right amount.
//!
//! Each inbox item is a **perception event** with orthogonal fields:
//! - `entity_type` + `entity_id`: what entity this is about
//! - `source`: who created it (user, system, plan mutation)
//! - `intent`: what the sender is communicating (claim, concern, inquiry, fyi)
//! - `priority`: when to surface it (immediate, next-touch, when-relevant)
//! - `confidence`: strength of a claim (high, low, null for non-claims)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// The inbox file containing all perception events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboxFile {
    /// All inbox items (pending, acknowledged, resolved, archived).
    #[serde(default, rename = "intent")]
    pub items: Vec<InboxItem>,
}

/// A single perception event in the inbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    /// Unique identifier.
    pub id: String,

    /// When this event was created.
    pub created: String,

    /// Current status in the resolution lifecycle.
    #[serde(default)]
    pub status: InboxItemStatus,

    /// What entity this is about (goal, task, rfc, phase, epoch, project).
    #[serde(default = "default_entity_type")]
    pub entity_type: String,

    /// ID of the entity (NULL for project-level items).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,

    /// Who created this event.
    #[serde(default)]
    pub source: InboxSource,

    /// What the sender is communicating.
    #[serde(default)]
    pub intent: InboxIntent,

    /// When to surface this event.
    #[serde(default)]
    pub priority: InboxPriority,

    /// Strength of a claim (null for non-claims).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<InboxConfidence>,

    /// Which agent created this (null = user/sidebar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Brief summary (like email subject).
    pub subject: String,

    /// Full content.
    #[serde(default)]
    pub body: String,

    /// Optional machine-readable action payload for agent-applied recommendations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,

    /// When this event was last updated.
    #[serde(default)]
    pub updated: Option<String>,

    /// Resolution note (when status = resolved).
    #[serde(default)]
    pub resolution: Option<String>,
}

fn default_entity_type() -> String {
    "project".to_string()
}

/// Status of an inbox item in its lifecycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InboxItemStatus {
    /// Awaiting agent attention.
    #[default]
    Pending,
    /// Agent has seen but not yet acted on.
    Acknowledged,
    /// Agent has addressed the intent.
    Resolved,
    /// Moved to archive (no longer surfaced).
    Archived,
}

impl InboxItemStatus {
    /// Returns true if this status means the item is still active.
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::Acknowledged)
    }

    /// Returns the string representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::Archived => "archived",
        }
    }
}

impl std::str::FromStr for InboxItemStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "acknowledged" => Ok(Self::Acknowledged),
            "resolved" => Ok(Self::Resolved),
            "archived" => Ok(Self::Archived),
            _ => Err(format!("Unknown inbox status: {s}")),
        }
    }
}

/// Who created this perception event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxSource {
    /// The user typed something via a feedback button or CLI.
    #[default]
    UserFeedback,
    /// The daemon detected something (future: hooks).
    SystemObservation,
    /// A plan command was executed (goal added, task completed, etc.).
    PlanMutation,
}

impl InboxSource {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UserFeedback => "user-feedback",
            Self::SystemObservation => "system-observation",
            Self::PlanMutation => "plan-mutation",
        }
    }
}

impl std::str::FromStr for InboxSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user-feedback" => Ok(Self::UserFeedback),
            "system-observation" => Ok(Self::SystemObservation),
            "plan-mutation" => Ok(Self::PlanMutation),
            _ => Err(format!("Unknown inbox source: {s}")),
        }
    }
}

/// What the sender is communicating (speech act).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InboxIntent {
    /// "I believe something about this entity's state."
    Claim,
    /// "Something about this worries me."
    Concern,
    /// "What's the status of this?"
    Inquiry,
    /// "Just be aware of this."
    #[default]
    Fyi,
}

impl InboxIntent {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::Concern => "concern",
            Self::Inquiry => "inquiry",
            Self::Fyi => "fyi",
        }
    }
}

impl std::str::FromStr for InboxIntent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claim" => Ok(Self::Claim),
            "concern" => Ok(Self::Concern),
            "inquiry" => Ok(Self::Inquiry),
            "fyi" => Ok(Self::Fyi),
            _ => Err(format!("Unknown inbox intent: {s}")),
        }
    }
}

/// When to surface this perception event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxPriority {
    /// Surface in the next steering response.
    Immediate,
    /// Surface when the agent next interacts with the scoped entity.
    #[default]
    NextTouch,
    /// Surface when contextually appropriate.
    WhenRelevant,
}

impl InboxPriority {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::NextTouch => "next-touch",
            Self::WhenRelevant => "when-relevant",
        }
    }
}

impl std::str::FromStr for InboxPriority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "immediate" => Ok(Self::Immediate),
            "next-touch" => Ok(Self::NextTouch),
            "when-relevant" => Ok(Self::WhenRelevant),
            _ => Err(format!("Unknown inbox priority: {s}")),
        }
    }
}

/// Strength of a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InboxConfidence {
    High,
    Low,
}

impl InboxConfidence {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }
}

impl std::str::FromStr for InboxConfidence {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "high" => Ok(Self::High),
            "low" => Ok(Self::Low),
            _ => Err(format!("Unknown inbox confidence: {s}")),
        }
    }
}

/// Valid entity types for perception events.
pub const ENTITY_TYPES: &[&str] = &["goal", "task", "rfc", "phase", "epoch", "project"];

/// Context for computing inbox item relevance scores.
#[derive(Debug)]
pub struct RelevanceContext {
    pub active_phase_id: Option<String>,
    /// Entity IDs `(entity_type, entity_id)` belonging to the active phase.
    pub active_entity_ids: HashSet<(String, String)>,
}

impl InboxItem {
    /// Returns true if this item is pending (needs attention).
    pub const fn is_pending(&self) -> bool {
        matches!(self.status, InboxItemStatus::Pending)
    }

    /// Returns true if this item is active (pending or acknowledged).
    pub const fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Compute relevance score (0.0 - 1.0) for this item given current context.
    pub fn relevance(&self, context: &RelevanceContext) -> f32 {
        let base: f32 = match self.priority {
            InboxPriority::Immediate => 1.0,
            InboxPriority::NextTouch => 0.7,
            InboxPriority::WhenRelevant => 0.3,
        };

        // Entity-scoped items get a boost when the agent is touching that entity's phase
        let entity_matches = match (self.entity_type.as_str(), &self.entity_id) {
            ("project", _) => true, // project-level always matches
            ("phase", Some(id)) => context.active_phase_id.as_deref() == Some(id.as_str()),
            ("goal", Some(id)) => context
                .active_entity_ids
                .contains(&("goal".to_string(), id.clone())),
            ("task", Some(id)) => context
                .active_entity_ids
                .contains(&("task".to_string(), id.clone())),
            ("rfc", Some(id)) => context
                .active_entity_ids
                .contains(&("rfc".to_string(), id.clone())),
            ("epoch", _) => false,
            _ => false,
        };

        if entity_matches && self.entity_type != "project" {
            (base + 0.3).min(1.0)
        } else if entity_matches {
            base
        } else if matches!(self.priority, InboxPriority::WhenRelevant) {
            0.0
        } else {
            base
        }
    }
}

/// Input for creating a new inbox item.
#[derive(Debug)]
pub struct CreateInboxItemInput {
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub source: InboxSource,
    pub intent: InboxIntent,
    pub priority: InboxPriority,
    pub confidence: Option<InboxConfidence>,
    pub agent_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub action: Option<Value>,
}

/// Surfaced intent for steering output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfacedIntent {
    pub id: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    pub source: InboxSource,
    pub intent: InboxIntent,
    pub priority: InboxPriority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub subject: String,
    pub relevance: f32,
}

impl From<&InboxItem> for SurfacedIntent {
    fn from(item: &InboxItem) -> Self {
        Self {
            id: item.id.clone(),
            entity_type: item.entity_type.clone(),
            entity_id: item.entity_id.clone(),
            source: item.source,
            intent: item.intent,
            priority: item.priority,
            agent_id: item.agent_id.clone(),
            subject: item.subject.clone(),
            relevance: 0.0,
        }
    }
}

/// Build a `RelevanceContext` from the active phase's entities.
fn build_relevance_context(
    loader: &crate::context::SqliteLoader,
    active_phase_id: Option<&str>,
    workspace_root: Option<&str>,
) -> anyhow::Result<RelevanceContext> {
    let active_entity_ids = if active_phase_id.is_some() {
        loader.collect_active_phase_entity_ids_for_workspace(workspace_root)?
    } else {
        HashSet::new()
    };

    Ok(RelevanceContext {
        active_phase_id: active_phase_id.map(String::from),
        active_entity_ids,
    })
}

/// Get inbox items that should be surfaced in steering output.
pub fn get_surfaced_intents(
    root: &Path,
    active_phase_id: Option<&str>,
    suppress_agent_id: Option<&str>,
) -> anyhow::Result<Vec<SurfacedIntent>> {
    let agent_ctx = crate::context::AgentContext::load(root.to_path_buf()).ok();
    let workspace_root = agent_ctx
        .as_ref()
        .and_then(crate::context::AgentContext::workspace_root_key);
    let db_path = crate::context::db_path(
        root,
        agent_ctx.as_ref().and_then(|ctx| ctx.project.as_ref()),
    );
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let loader = crate::context::SqliteLoader::open(&db_path)?;
    let inbox = InboxFile {
        items: loader.load_inbox()?,
    };

    let context = build_relevance_context(&loader, active_phase_id, workspace_root.as_deref())?;

    let mut surfaced: Vec<SurfacedIntent> = inbox
        .items
        .iter()
        .filter(|i| i.is_active())
        .filter(|i| match (i.agent_id.as_deref(), suppress_agent_id) {
            (Some(item_agent_id), Some(suppress_agent_id)) => item_agent_id != suppress_agent_id,
            _ => true,
        })
        .map(|i| {
            let mut s = SurfacedIntent::from(i);
            s.relevance = i.relevance(&context);
            s
        })
        .filter(|s| s.relevance > 0.0)
        .collect();

    surfaced.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(surfaced)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SQLITE_DB_PATH;
    use crate::context::SqliteWriter;

    fn write_items_to_db(root: &std::path::Path, items: &[InboxItem]) {
        let db_path = root.join(SQLITE_DB_PATH);
        let parent = db_path.parent().expect("db parent");
        std::fs::create_dir_all(parent).expect("create cache dir");
        let writer = SqliteWriter::open(&db_path).expect("open writer");

        for item in items {
            let text_id = writer
                .add_inbox_item(
                    &item.entity_type,
                    item.entity_id.as_deref(),
                    item.source.as_str(),
                    item.intent.as_str(),
                    item.priority.as_str(),
                    item.confidence.map(|c| c.as_str()),
                    item.agent_id.as_deref(),
                    &item.subject,
                    &item.body,
                    item.action
                        .as_ref()
                        .map(serde_json::Value::to_string)
                        .as_deref(),
                )
                .expect("insert inbox item");

            if item.status != InboxItemStatus::Pending || item.resolution.is_some() {
                writer
                    .update_inbox_status(&text_id, item.status.as_str(), item.resolution.as_deref())
                    .expect("update inbox status");
            }
        }
    }

    fn make_context(phase_id: Option<&str>) -> RelevanceContext {
        RelevanceContext {
            active_phase_id: phase_id.map(String::from),
            active_entity_ids: HashSet::new(),
        }
    }

    fn make_context_with_entities(
        phase_id: Option<&str>,
        entities: &[(&str, &str)],
    ) -> RelevanceContext {
        RelevanceContext {
            active_phase_id: phase_id.map(String::from),
            active_entity_ids: entities
                .iter()
                .map(|(t, id)| (t.to_string(), id.to_string()))
                .collect(),
        }
    }

    fn make_item(entity_type: &str, entity_id: Option<&str>, priority: InboxPriority) -> InboxItem {
        InboxItem {
            id: "test".into(),
            created: "2026-01-01T00:00:00Z".into(),
            status: InboxItemStatus::Pending,
            entity_type: entity_type.into(),
            entity_id: entity_id.map(String::from),
            source: InboxSource::UserFeedback,
            intent: InboxIntent::Fyi,
            priority,
            confidence: None,
            agent_id: None,
            subject: "Test".into(),
            body: String::new(),
            action: None,
            updated: None,
            resolution: None,
        }
    }

    #[test]
    fn test_inbox_item_status_is_active() {
        assert!(InboxItemStatus::Pending.is_active());
        assert!(InboxItemStatus::Acknowledged.is_active());
        assert!(!InboxItemStatus::Resolved.is_active());
        assert!(!InboxItemStatus::Archived.is_active());
    }

    #[test]
    fn test_relevance_project_level() {
        let ctx = make_context(None);
        let item = make_item("project", None, InboxPriority::Immediate);
        assert_eq!(item.relevance(&ctx), 1.0);

        let item_next = make_item("project", None, InboxPriority::NextTouch);
        assert!((item_next.relevance(&ctx) - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_relevance_phase_match() {
        let item = make_item("phase", Some("p1"), InboxPriority::NextTouch);
        // Phase matches → 0.7 + 0.3 = 1.0
        assert!((item.relevance(&make_context(Some("p1"))) - 1.0).abs() < 0.01);
        // Phase doesn't match, next-touch → still 0.7
        assert!((item.relevance(&make_context(Some("p2"))) - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_relevance_when_relevant_no_match() {
        let item = make_item("phase", Some("p1"), InboxPriority::WhenRelevant);
        assert_eq!(item.relevance(&make_context(Some("p2"))), 0.0);
        assert!((item.relevance(&make_context(Some("p1"))) - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_relevance_goal_in_active_phase() {
        let ctx = make_context_with_entities(Some("p1"), &[("goal", "my-goal")]);
        let item = make_item("goal", Some("my-goal"), InboxPriority::NextTouch);
        // Goal in active phase → 0.7 + 0.3 = 1.0
        assert!((item.relevance(&ctx) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_relevance_goal_not_in_phase() {
        let ctx = make_context_with_entities(Some("p1"), &[("goal", "other-goal")]);
        let item = make_item("goal", Some("my-goal"), InboxPriority::WhenRelevant);
        // Goal not in active phase, when-relevant → 0.0
        assert_eq!(item.relevance(&ctx), 0.0);
    }

    #[test]
    fn test_relevance_task_in_active_phase() {
        let ctx = make_context_with_entities(Some("p1"), &[("task", "my-task")]);
        let item = make_item("task", Some("my-task"), InboxPriority::WhenRelevant);
        // Task in active phase → 0.3 + 0.3 = 0.6
        assert!((item.relevance(&ctx) - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_relevance_rfc_in_active_phase() {
        let ctx = make_context_with_entities(Some("p1"), &[("rfc", "00181")]);
        let item = make_item("rfc", Some("00181"), InboxPriority::NextTouch);
        // RFC in active phase → 0.7 + 0.3 = 1.0
        assert!((item.relevance(&ctx) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_empty_inbox() {
        let inbox = InboxFile::default();
        assert!(inbox.items.is_empty());
    }

    #[test]
    fn test_inbox_source_roundtrip() {
        for s in ["user-feedback", "system-observation", "plan-mutation"] {
            let parsed: InboxSource = s.parse().unwrap();
            assert_eq!(parsed.as_str(), s);
        }
    }

    #[test]
    fn test_inbox_intent_roundtrip() {
        for s in ["claim", "concern", "inquiry", "fyi"] {
            let parsed: InboxIntent = s.parse().unwrap();
            assert_eq!(parsed.as_str(), s);
        }
    }

    #[test]
    fn test_inbox_priority_roundtrip() {
        for s in ["immediate", "next-touch", "when-relevant"] {
            let parsed: InboxPriority = s.parse().unwrap();
            assert_eq!(parsed.as_str(), s);
        }
    }

    #[test]
    fn test_inbox_confidence_roundtrip() {
        for s in ["high", "low"] {
            let parsed: InboxConfidence = s.parse().unwrap();
            assert_eq!(parsed.as_str(), s);
        }
    }

    #[test]
    fn test_get_surfaced_intents_suppresses_matching_agent_id_only() {
        let temp = tempfile::tempdir().expect("tempdir");

        let mut from_agent = make_item("project", None, InboxPriority::Immediate);
        from_agent.id = "agent-item".into();
        from_agent.agent_id = Some("agent-1".into());
        from_agent.subject = "From agent".into();

        let mut from_user = make_item("project", None, InboxPriority::Immediate);
        from_user.id = "user-item".into();
        from_user.subject = "From user".into();

        write_items_to_db(temp.path(), &[from_agent, from_user]);

        let surfaced =
            get_surfaced_intents(temp.path(), None, Some("agent-1")).expect("surface intents");

        assert_eq!(surfaced.len(), 1);
        assert_eq!(surfaced[0].subject, "From user");
        assert_eq!(surfaced[0].agent_id, None);
    }
}
