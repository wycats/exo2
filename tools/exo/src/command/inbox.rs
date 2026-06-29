//! Inbox namespace commands.
//!
//! - `inbox list`: List inbox items (Pure)
//! - `inbox add`: Add a new inbox item (Write)
//! - `inbox ack`: Acknowledge an inbox item (Write)
//! - `inbox resolve`: Resolve an inbox item (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::{AgentContext, SqliteWriter};
use crate::inbox::{
    CreateInboxItemInput, ENTITY_TYPES, InboxConfidence, InboxIntent, InboxItem, InboxItemStatus,
    InboxPriority, InboxSource,
};
use crate::phase_owner;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for inbox commands.
fn default_inbox_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "List inbox items".to_string(),
        command: "exo inbox list".to_string(),
        rationale: "View pending user intents to process.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the inbox namespace
// ============================================================================

/// Inbox namespace command specification.
///
/// This enum is the authoritative definition of the inbox namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `InboxCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "inbox", description = "Inbox management commands")]
pub enum InboxCommands {
    #[exo(effect = "pure", description = "List inbox items")]
    List {
        #[exo(long, optional, description = "Filter by status")]
        status: Option<String>,
        #[exo(
            long,
            optional,
            description = "Filter by entity type: goal, task, rfc, phase, epoch, project"
        )]
        entity_type: Option<String>,
        #[exo(long, optional, description = "Filter by entity ID")]
        entity_id: Option<String>,
        #[exo(
            long,
            optional,
            description = "Filter by source: user-feedback, system-observation, plan-mutation"
        )]
        source: Option<String>,
        #[exo(long, optional, description = "Maximum number of items")]
        limit: Option<i64>,
    },

    #[exo(effect = "write", description = "Add a new inbox item")]
    Add {
        #[exo(positional, description = "Subject line for the inbox item")]
        subject: String,
        #[exo(
            long,
            default = "project",
            description = "Entity type: goal, task, rfc, phase, epoch, project"
        )]
        entity_type: String,
        #[exo(
            long,
            optional,
            description = "Entity ID (required for non-project types)"
        )]
        entity_id: Option<String>,
        #[exo(
            long,
            default = "user-feedback",
            description = "Source: user-feedback, system-observation, plan-mutation"
        )]
        source: String,
        #[exo(
            long,
            default = "fyi",
            description = "Intent: claim, concern, inquiry, fyi"
        )]
        intent: String,
        #[exo(
            long,
            default = "next-touch",
            description = "Priority: immediate, next-touch, when-relevant"
        )]
        priority: String,
        #[exo(
            long,
            optional,
            description = "Confidence: high, low (only for claim intent)"
        )]
        confidence: Option<String>,
        #[exo(long, default = "", description = "Body text")]
        body: String,
        #[exo(
            long,
            optional,
            description = "Machine-readable action payload JSON for agent-applied recommendations"
        )]
        action_json: Option<String>,
    },

    #[exo(effect = "write", description = "Acknowledge an inbox item")]
    Ack {
        #[exo(positional, description = "The inbox item ID to acknowledge")]
        id: String,
    },

    #[exo(effect = "write", description = "Resolve an inbox item")]
    Resolve {
        #[exo(positional, description = "The inbox item ID to resolve")]
        id: String,
        #[exo(long, optional, description = "Resolution note")]
        resolution: Option<String>,
        #[exo(long, optional, description = "Promote to: goal or idea")]
        promote: Option<String>,
    },

    #[exo(effect = "write", description = "Archive an inbox item")]
    Archive {
        #[exo(positional, description = "The inbox item ID to archive")]
        id: String,
    },
}

impl InboxCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::List {
                status,
                entity_type,
                entity_id,
                source,
                limit,
            } => CommandBox::pure(InboxList::new(
                status,
                entity_type,
                entity_id,
                source,
                limit.and_then(|value| usize::try_from(value).ok()),
            )),
            Self::Add {
                subject,
                entity_type,
                entity_id,
                source,
                intent,
                priority,
                confidence,
                body,
                action_json,
            } => CommandBox::mutable(InboxAdd::new(
                subject,
                entity_type,
                entity_id,
                source,
                intent,
                priority,
                confidence,
                body,
                action_json,
            )),
            Self::Ack { id } => CommandBox::mutable(InboxAck::new(id)),
            Self::Resolve {
                id,
                resolution,
                promote,
            } => CommandBox::mutable(InboxResolve::new(id, resolution, promote)),
            Self::Archive { id } => CommandBox::mutable(InboxArchive::new(id)),
        })
    }
}

// ============================================================================
// inbox list
// ============================================================================

/// List inbox items.
#[derive(Debug, Clone)]
pub struct InboxList {
    /// Optional status filter.
    pub status: Option<String>,
    /// Optional entity type filter.
    pub entity_type: Option<String>,
    /// Optional entity id filter.
    pub entity_id: Option<String>,
    /// Optional source filter.
    pub source: Option<String>,
    /// Optional limit for number of items.
    pub limit: Option<usize>,
}

impl InboxList {
    pub const fn new(
        status: Option<String>,
        entity_type: Option<String>,
        entity_id: Option<String>,
        source: Option<String>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            status,
            entity_type,
            entity_id,
            source,
            limit,
        }
    }
}

#[derive(Debug, Serialize)]
struct InboxListOutput {
    kind: &'static str,
    ok: bool,
    items: Vec<InboxItem>,
}

impl Command for InboxList {
    fn namespace(&self) -> &'static str {
        "inbox"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List inbox items"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_inbox_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        use crate::context::SqliteLoader;

        if self.entity_id.is_some() && self.entity_type.is_none() {
            anyhow::bail!("--entity-id requires --entity-type");
        }

        if let Some(status) = &self.status {
            let _ = parse_status_filter(status)?;
        }

        if let Some(et) = &self.entity_type
            && !crate::inbox::ENTITY_TYPES.contains(&et.as_str())
        {
            anyhow::bail!(
                "Unknown entity type: {et}. Valid: {}",
                crate::inbox::ENTITY_TYPES.join(", ")
            );
        }

        if let Some(src) = &self.source {
            let _: crate::inbox::InboxSource =
                src.parse().map_err(|e: String| anyhow::anyhow!(e))?;
        }

        let items: Vec<InboxItem> = {
            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path)?;
            if self.status.is_some()
                || self.entity_type.is_some()
                || self.entity_id.is_some()
                || self.source.is_some()
                || self.limit.is_some()
            {
                loader.load_inbox_filtered(
                    self.status.as_deref(),
                    self.entity_type.as_deref(),
                    self.entity_id.as_deref(),
                    self.source.as_deref(),
                    self.limit,
                )?
            } else {
                loader.load_inbox()?
            }
        };

        let output = InboxListOutput {
            kind: "inbox.list",
            ok: true,
            items: items.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if items.is_empty() {
                    Ok(CommandOutput::new(
                        output,
                        "No inbox items found.".to_string(),
                    ))
                } else {
                    let mut message = format!("Inbox ({} item(s)):\n\n", items.len());
                    for item in &items {
                        message.push_str(&format!(
                            "  [{:?}] [{}] {} - {} ({:?})\n",
                            item.status,
                            item.priority.as_str(),
                            item.id,
                            item.subject,
                            item.intent
                        ));
                        if !item.body.is_empty() {
                            let preview = if item.body.len() > 60 {
                                format!("{}...", &item.body[..60].replace('\n', " "))
                            } else {
                                item.body.replace('\n', " ")
                            };
                            message.push_str(&format!("          {preview}\n"));
                        }
                    }
                    Ok(CommandOutput::new(output, message))
                }
            }
        }
    }
}

// ============================================================================
// inbox add
// ============================================================================

/// Add a new inbox item.
#[derive(Debug, Clone)]
pub struct InboxAdd {
    pub subject: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub source: String,
    pub intent: String,
    pub priority: String,
    pub confidence: Option<String>,
    pub body: String,
    pub action_json: Option<String>,
}

impl InboxAdd {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject: impl Into<String>,
        entity_type: impl Into<String>,
        entity_id: Option<String>,
        source: impl Into<String>,
        intent: impl Into<String>,
        priority: impl Into<String>,
        confidence: Option<String>,
        body: impl Into<String>,
        action_json: Option<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            entity_type: entity_type.into(),
            entity_id,
            source: source.into(),
            intent: intent.into(),
            priority: priority.into(),
            confidence,
            body: body.into(),
            action_json,
        }
    }

    fn parse_entity_type(&self) -> ExoResult<String> {
        let et = self.entity_type.to_lowercase();
        if ENTITY_TYPES.contains(&et.as_str()) {
            Ok(et)
        } else {
            anyhow::bail!(
                "Unknown entity type: {et}. Valid: {}",
                ENTITY_TYPES.join(", ")
            )
        }
    }

    fn parse_source(&self) -> ExoResult<InboxSource> {
        self.source.parse().map_err(|e: String| anyhow::anyhow!(e))
    }

    fn parse_intent(&self) -> ExoResult<InboxIntent> {
        self.intent.parse().map_err(|e: String| anyhow::anyhow!(e))
    }

    fn parse_priority(&self) -> ExoResult<InboxPriority> {
        self.priority
            .parse()
            .map_err(|e: String| anyhow::anyhow!(e))
    }

    fn parse_confidence(&self) -> ExoResult<Option<InboxConfidence>> {
        match self.confidence.as_deref() {
            Some(s) => s.parse().map(Some).map_err(|e: String| anyhow::anyhow!(e)),
            None => Ok(None),
        }
    }

    fn parse_action(&self) -> ExoResult<Option<serde_json::Value>> {
        self.action_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|err| anyhow::anyhow!("Invalid action JSON: {err}"))
    }
}

#[derive(Debug, Serialize)]
struct InboxAddOutput {
    kind: &'static str,
    ok: bool,
    #[serde(flatten)]
    item: InboxItem,
}

impl Command for InboxAdd {
    fn namespace(&self) -> &'static str {
        "inbox"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new inbox item"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_inbox_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("InboxAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for InboxAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let input = CreateInboxItemInput {
            entity_type: self.parse_entity_type()?,
            entity_id: self.entity_id.clone(),
            source: self.parse_source()?,
            intent: self.parse_intent()?,
            priority: self.parse_priority()?,
            confidence: self.parse_confidence()?,
            agent_id: ctx.agent_id.clone(),
            subject: self.subject.clone(),
            body: self.body.clone(),
            action: self.parse_action()?,
        };

        // Validate entity_id consistency
        if input.entity_type == "project" && input.entity_id.is_some() {
            anyhow::bail!("Project-level items must not have an entity_id");
        }
        if input.entity_type != "project" && input.entity_id.is_none() {
            anyhow::bail!("Non-project entity types require --entity-id");
        }

        let item = {
            let writer = SqliteWriter::open(ctx.db_path())?;

            let id = writer.add_inbox_item(
                &input.entity_type,
                input.entity_id.as_deref(),
                input.source.as_str(),
                input.intent.as_str(),
                input.priority.as_str(),
                input.confidence.map(|c| c.as_str()),
                input.agent_id.as_deref(),
                &input.subject,
                &input.body,
                input
                    .action
                    .as_ref()
                    .map(serde_json::Value::to_string)
                    .as_deref(),
            )?;
            InboxItem {
                id,
                created: chrono::Utc::now().to_rfc3339(),
                status: InboxItemStatus::Pending,
                entity_type: input.entity_type,
                entity_id: input.entity_id,
                source: input.source,
                intent: input.intent,
                priority: input.priority,
                confidence: input.confidence,
                agent_id: input.agent_id,
                subject: input.subject,
                body: input.body,
                action: input.action,
                updated: None,
                resolution: None,
            }
        };

        let output = InboxAddOutput {
            kind: "inbox.add",
            ok: true,
            item: item.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Created inbox item: {}", item.id),
            )),
        }
    }
}

// ============================================================================
// inbox ack
// ============================================================================

/// Acknowledge an inbox item.
#[derive(Debug, Clone)]
pub struct InboxAck {
    pub id: String,
}

impl InboxAck {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct InboxAckOutput {
    kind: &'static str,
    ok: bool,
    status: &'static str,
    id: String,
}

impl Command for InboxAck {
    fn namespace(&self) -> &'static str {
        "inbox"
    }

    fn operation(&self) -> &'static str {
        "ack"
    }

    fn description(&self) -> &'static str {
        "Acknowledge an inbox item"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_inbox_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("InboxAck should be dispatched via execute_mut")
    }
}

impl MutableCommand for InboxAck {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_inbox_status(&self.id, "acknowledged", None)?;

        let output = InboxAckOutput {
            kind: "inbox.ack",
            ok: true,
            status: "acknowledged",
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Acknowledged inbox item: {}", self.id),
            )),
        }
    }
}

// ============================================================================
// inbox resolve
// ============================================================================

/// Resolve an inbox item.
///
/// Optionally promotes the inbox item to a goal or idea before resolving.
#[derive(Debug, Clone)]
pub struct InboxResolve {
    pub id: String,
    pub resolution: Option<String>,
    /// Promote to "goal" or "idea" before resolving.
    pub promote_as: Option<String>,
}

impl InboxResolve {
    pub fn new(
        id: impl Into<String>,
        resolution: Option<String>,
        promote_as: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            resolution,
            promote_as,
        }
    }
}

#[derive(Debug, Serialize)]
struct InboxResolveOutput {
    kind: &'static str,
    ok: bool,
    status: &'static str,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    promoted_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    promoted_id: Option<String>,
}

impl Command for InboxResolve {
    fn namespace(&self) -> &'static str {
        "inbox"
    }

    fn operation(&self) -> &'static str {
        "resolve"
    }

    fn description(&self) -> &'static str {
        "Resolve an inbox item"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_inbox_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("InboxResolve should be dispatched via execute_mut")
    }
}

impl MutableCommand for InboxResolve {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        // First, get the inbox item to extract its subject for promotion
        let items: Vec<InboxItem> = {
            use crate::context::SqliteLoader;
            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path)?;
            loader.load_inbox()?
        };
        let item = items.iter().find(|i| i.id == self.id).ok_or_else(|| {
            anyhow::anyhow!(
                "Inbox item '{}' not found. Use `exo inbox list` to see available items.",
                self.id
            )
        })?;

        let subject = item.subject.clone();
        let mut promoted_to: Option<String> = None;
        let mut promoted_id: Option<String> = None;

        // Handle promotion if requested
        if let Some(ref promote) = self.promote_as {
            match promote.as_str() {
                "goal" => {
                    // Promote to goal in active phase
                    let agent_ctx = AgentContext::load(ctx.root.to_path_buf())?;
                    let phase_id =
                        agent_ctx.find_workspace_active_phase_id()?.ok_or_else(|| {
                            anyhow::anyhow!(
                                "No active phase. Start a phase first to promote to goal."
                            )
                        })?;
                    phase_owner::ensure_phase_write_allowed(
                        ctx.root,
                        ctx.project,
                        &ctx.db_path(),
                        &phase_id,
                    )?;

                    // Generate goal ID from subject (same logic as goal add)
                    let goal_id = {
                        let slug = crate::utils::slugify(&subject);
                        if slug.is_empty() {
                            "untitled".to_string()
                        } else {
                            slug
                        }
                    };

                    let writer = SqliteWriter::open(ctx.db_path())?;
                    writer.add_goal(
                        &phase_id,
                        &goal_id,
                        &subject,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        &[],
                    )?;
                    promoted_to = Some("goal".to_string());
                    promoted_id = Some(goal_id);
                }
                "idea" => {
                    // Promote to idea
                    let idea_id = {
                        let writer = SqliteWriter::open(ctx.db_path())?;
                        writer.add_idea(&subject, None, &[])?
                    };
                    promoted_to = Some("idea".to_string());
                    promoted_id = Some(idea_id);
                }
                other => {
                    anyhow::bail!("Invalid --promote value '{other}'. Use 'goal' or 'idea'.");
                }
            }
        }

        // Now resolve the inbox item
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_inbox_status(&self.id, "resolved", self.resolution.as_deref())?;

        let output = InboxResolveOutput {
            kind: "inbox.resolve",
            ok: true,
            status: "resolved",
            id: self.id.clone(),
            promoted_to: promoted_to.clone(),
            promoted_id: promoted_id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let msg = if let (Some(to), Some(id)) = (&promoted_to, &promoted_id) {
                    format!(
                        "Resolved inbox item: {}\n→ Promoted to {}: {}",
                        self.id, to, id
                    )
                } else {
                    format!("Resolved inbox item: {}", self.id)
                };
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// inbox archive
// ============================================================================

/// Archive inbox items.
///
/// Archives a specific item by ID, or all resolved items.
#[derive(Debug, Clone)]
pub struct InboxArchive {
    /// The inbox item ID to archive.
    pub id: String,
}

impl InboxArchive {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Serialize)]
struct InboxArchiveOutput {
    kind: &'static str,
    ok: bool,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
}

impl Command for InboxArchive {
    fn namespace(&self) -> &'static str {
        "inbox"
    }

    fn operation(&self) -> &'static str {
        "archive"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_inbox_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("InboxArchive should be dispatched via execute_mut")
    }
}

impl MutableCommand for InboxArchive {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.update_inbox_status(&self.id, "archived", None)?;

        let output = InboxArchiveOutput {
            kind: "inbox.archive",
            ok: true,
            status: "archived",
            id: Some(self.id.clone()),
            count: None,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Archived inbox item: {}", self.id),
            )),
        }
    }
}

fn parse_status_filter(status: &str) -> ExoResult<InboxItemStatus> {
    match status.to_lowercase().as_str() {
        "pending" => Ok(InboxItemStatus::Pending),
        "acknowledged" => Ok(InboxItemStatus::Acknowledged),
        "resolved" => Ok(InboxItemStatus::Resolved),
        "archived" => Ok(InboxItemStatus::Archived),
        other => anyhow::bail!("Unknown status: {other}"),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::Effect;

    #[test]
    fn test_inbox_list_metadata() {
        let cmd = InboxList::new(None, None, None, None, None);
        assert_eq!(cmd.namespace(), "inbox");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn test_inbox_list_all_metadata() {
        let cmd = InboxList::new(
            Some("pending".to_string()),
            Some("goal".to_string()),
            Some("goal-1".to_string()),
            Some("user-feedback".to_string()),
            Some(10),
        );
        assert_eq!(cmd.namespace(), "inbox");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.status.as_deref(), Some("pending"));
        assert_eq!(cmd.entity_type.as_deref(), Some("goal"));
        assert_eq!(cmd.entity_id.as_deref(), Some("goal-1"));
        assert_eq!(cmd.source.as_deref(), Some("user-feedback"));
    }

    #[test]
    fn test_inbox_add_metadata() {
        let cmd = InboxAdd::new(
            "Fix bug",
            "project",
            None,
            "user-feedback",
            "concern",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.namespace(), "inbox");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_inbox_add_intent_parsing() {
        let cmd = InboxAdd::new(
            "",
            "project",
            None,
            "user-feedback",
            "claim",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_intent().unwrap(), InboxIntent::Claim);

        let cmd = InboxAdd::new(
            "",
            "project",
            None,
            "user-feedback",
            "concern",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_intent().unwrap(), InboxIntent::Concern);

        let cmd = InboxAdd::new(
            "",
            "project",
            None,
            "user-feedback",
            "inquiry",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_intent().unwrap(), InboxIntent::Inquiry);

        let cmd = InboxAdd::new(
            "",
            "project",
            None,
            "user-feedback",
            "fyi",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_intent().unwrap(), InboxIntent::Fyi);
    }

    #[test]
    fn test_inbox_add_entity_type_parsing() {
        let cmd = InboxAdd::new(
            "",
            "goal",
            Some("g1".into()),
            "user-feedback",
            "fyi",
            "next-touch",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_entity_type().unwrap(), "goal");

        let cmd = InboxAdd::new(
            "",
            "invalid",
            None,
            "user-feedback",
            "fyi",
            "next-touch",
            None,
            "",
            None,
        );
        assert!(cmd.parse_entity_type().is_err());
    }

    #[test]
    fn test_inbox_add_priority_parsing() {
        let cmd = InboxAdd::new(
            "",
            "project",
            None,
            "user-feedback",
            "fyi",
            "immediate",
            None,
            "",
            None,
        );
        assert_eq!(cmd.parse_priority().unwrap(), InboxPriority::Immediate);
    }

    #[test]
    fn test_inbox_ack_metadata() {
        let cmd = InboxAck::new("inbox-item-id");
        assert_eq!(cmd.namespace(), "inbox");
        assert_eq!(cmd.operation(), "ack");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "inbox-item-id");
    }

    #[test]
    fn test_inbox_resolve_metadata() {
        let cmd = InboxResolve::new("inbox-item-id", Some("Resolution note".to_string()), None);
        assert_eq!(cmd.namespace(), "inbox");
        assert_eq!(cmd.operation(), "resolve");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.id, "inbox-item-id");
        assert_eq!(cmd.resolution.as_deref(), Some("Resolution note"));
        assert!(cmd.promote_as.is_none());
    }

    #[test]
    fn test_inbox_resolve_with_promote_as() {
        let cmd = InboxResolve::new("inbox-item-id", None, Some("goal".to_string()));
        assert_eq!(cmd.id, "inbox-item-id");
        assert!(cmd.resolution.is_none());
        assert_eq!(cmd.promote_as.as_deref(), Some("goal"));
    }
}
