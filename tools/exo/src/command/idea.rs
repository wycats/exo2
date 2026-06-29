//! Idea namespace commands.
//!
//! - `idea add`: Add a new idea (Write)
//! - `idea show`: Show idea details (Pure)
//! - `idea list`: List ideas (Pure)
//! - `idea archive`: Archive an idea (Write)
//! - `idea to-rfc`: Convert an idea to an RFC (Write)

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::SqliteWriter;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for idea commands.
fn default_idea_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "List ideas".to_string(),
        command: "exo idea list".to_string(),
        rationale: "View all ideas in the backlog.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the idea namespace
// ============================================================================

/// Idea namespace command specification.
///
/// This enum is the authoritative definition of the idea namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `IdeaCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "idea", description = "Idea management commands")]
pub enum IdeaCommands {
    #[exo(effect = "write", description = "Add a new idea to the backlog")]
    Add {
        #[exo(positional, description = "The title of the idea")]
        title: String,
        #[exo(long, optional, description = "Optional description")]
        description: Option<String>,
        #[exo(long, optional, description = "Comma-separated tags")]
        tags: Option<String>,
    },

    #[exo(effect = "pure", description = "Show details of a specific idea")]
    Show {
        #[exo(positional, description = "The ID (or prefix) of the idea to show")]
        id: String,
    },

    #[exo(effect = "pure", description = "List all ideas in the backlog")]
    List {
        #[exo(long, optional, description = "Filter by comma-separated tags")]
        tags: Option<String>,
        #[exo(long, optional, description = "Maximum number of ideas")]
        limit: Option<i64>,
    },

    #[exo(effect = "write", description = "Archive an idea")]
    Archive {
        #[exo(positional, description = "The ID of the idea to archive")]
        id: String,
    },

    #[exo(
        effect = "write",
        operation = "to-rfc",
        description = "Convert an idea to an RFC"
    )]
    ToRfc {
        #[exo(positional, description = "The ID of the idea to convert")]
        id: String,
        #[exo(long, optional, description = "Feature category")]
        feature: Option<String>,
    },
}

impl IdeaCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        fn split_tags(tags: Option<String>) -> Vec<String> {
            tags.map(|raw| {
                raw.split(',')
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect()
            })
            .unwrap_or_default()
        }

        Ok(match self {
            Self::Add {
                title,
                description,
                tags,
            } => CommandBox::mutable(IdeaAdd::new(
                title,
                description.unwrap_or_default(),
                split_tags(tags),
            )),
            Self::Show { id } => CommandBox::pure(IdeaShow::new(id)),
            Self::List { tags, limit } => CommandBox::pure(IdeaList::new(
                tags,
                limit.and_then(|value| usize::try_from(value).ok()),
            )),
            Self::Archive { id } => CommandBox::mutable(IdeaArchive::new(id)),
            Self::ToRfc { id, feature } => CommandBox::mutable(IdeaToRfc::new(id, feature)),
        })
    }
}

// ============================================================================
// idea add
// ============================================================================

/// Add a new idea to the backlog.
#[derive(Debug, Clone)]
pub struct IdeaAdd {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

impl IdeaAdd {
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            tags,
        }
    }
}

#[derive(Debug, Serialize)]
struct IdeaAddOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for IdeaAdd {
    fn namespace(&self) -> &'static str {
        "idea"
    }

    fn operation(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add a new idea to the backlog"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_idea_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("IdeaAdd should be dispatched via execute_mut")
    }
}

impl MutableCommand for IdeaAdd {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let id = {
            let writer = SqliteWriter::open(ctx.db_path())?;
            let description = if self.description.is_empty() {
                None
            } else {
                Some(self.description.as_str())
            };
            writer.add_idea(&self.title, description, &self.tags)?
        };

        let output = IdeaAddOutput {
            kind: "idea.add",
            ok: true,
            id: id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Added idea: {id}\n→ Next: exo idea list or exo idea to-rfc {id}"),
            )),
        }
    }
}

// ============================================================================
// idea show
// ============================================================================

/// Show details of a specific idea.
#[derive(Debug, Clone)]
pub struct IdeaShow {
    pub id: String,
}

impl IdeaShow {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct IdeaShowOutput {
    kind: &'static str,
    ok: bool,
    idea: IdeaShowEntry,
}

#[derive(Debug, Clone, Serialize)]
struct IdeaShowEntry {
    id: String,
    title: String,
    description: String,
    status: String,
    created_at: String,
    source: String,
    tags: Vec<String>,
    related_tasks: Vec<String>,
}

impl Command for IdeaShow {
    fn namespace(&self) -> &'static str {
        "idea"
    }

    fn operation(&self) -> &'static str {
        "show"
    }

    fn description(&self) -> &'static str {
        "Show details of a specific idea"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_idea_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        use crate::context::SqliteLoader;

        let db_path = ctx.db_path();
        let loader = SqliteLoader::open(&db_path)?;

        // Try exact match first, then prefix match
        let idea = loader.load_idea_by_id(&self.id)?;
        let idea = match idea {
            Some(i) => i,
            None => {
                // Try prefix match
                let all = loader.load_ideas()?;
                let matches: Vec<_> = all
                    .into_iter()
                    .filter(|i| i.id.starts_with(&self.id))
                    .collect();
                match matches.len() {
                    0 => anyhow::bail!("No idea found matching '{}'", self.id),
                    1 => matches
                        .into_iter()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("No idea found matching '{}'", self.id))?,
                    n => anyhow::bail!(
                        "Ambiguous prefix '{}' matches {} ideas. Use a longer prefix.",
                        self.id,
                        n
                    ),
                }
            }
        };

        let entry = IdeaShowEntry {
            id: idea.id.clone(),
            title: idea.title,
            description: idea.description,
            status: idea.status,
            created_at: idea.created_at,
            source: idea.source,
            tags: idea.tags,
            related_tasks: idea.related_tasks,
        };

        let output = IdeaShowOutput {
            kind: "idea.show",
            ok: true,
            idea: entry.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut msg = format!("# {}\n", entry.title);
                msg.push_str(&format!("ID: {}\n", entry.id));
                msg.push_str(&format!("Status: {}\n", entry.status));
                if !entry.tags.is_empty() {
                    msg.push_str(&format!("Tags: {}\n", entry.tags.join(", ")));
                }
                if !entry.description.is_empty() {
                    msg.push('\n');
                    msg.push_str(&entry.description);
                    msg.push('\n');
                }
                Ok(CommandOutput::new(output, msg))
            }
        }
    }
}

// ============================================================================
// idea list
// ============================================================================

/// List all ideas in the backlog.
#[derive(Debug, Clone, Default)]
pub struct IdeaList {
    pub tags: Option<String>,
    pub limit: Option<usize>,
}

impl IdeaList {
    pub const fn new(tags: Option<String>, limit: Option<usize>) -> Self {
        Self { tags, limit }
    }
}

#[derive(Debug, Clone, Serialize)]
struct IdeaListEntry {
    id: String,
    title: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct IdeaListOutput {
    kind: &'static str,
    ok: bool,
    ideas: Vec<IdeaListEntry>,
}

impl Command for IdeaList {
    fn namespace(&self) -> &'static str {
        "idea"
    }

    fn operation(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "List all ideas in the backlog"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_idea_steering()
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        use crate::context::SqliteLoader;

        let ideas = {
            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path)?;
            loader.load_ideas()?
        };

        let tag_filter: Option<Vec<String>> = self.tags.as_ref().map(|raw| {
            raw.split(',')
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty())
                .collect()
        });

        let mut entries: Vec<IdeaListEntry> = ideas
            .into_iter()
            .filter(|idea| {
                if let Some(tags) = &tag_filter {
                    tags.iter().any(|tag| idea.tags.contains(tag))
                } else {
                    true
                }
            })
            .map(|i| IdeaListEntry {
                id: i.id,
                title: i.title,
                status: i.status,
            })
            .collect();

        if let Some(limit) = self.limit {
            entries.truncate(limit);
        }

        let output = IdeaListOutput {
            kind: "idea.list",
            ok: true,
            ideas: entries.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                if entries.is_empty() {
                    Ok(CommandOutput::new(output, "No ideas found.".to_string()))
                } else {
                    let mut msg = String::new();
                    for idea in &entries {
                        msg.push_str(&format!("{} - {} ({})\n", idea.id, idea.title, idea.status));
                    }
                    Ok(CommandOutput::new(output, msg.trim_end().to_string()))
                }
            }
        }
    }
}

// ============================================================================
// idea archive
// ============================================================================

/// Archive an idea.
#[derive(Debug, Clone)]
pub struct IdeaArchive {
    pub id: String,
}

impl IdeaArchive {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Serialize)]
struct IdeaArchiveOutput {
    kind: &'static str,
    ok: bool,
    id: String,
}

impl Command for IdeaArchive {
    fn namespace(&self) -> &'static str {
        "idea"
    }

    fn operation(&self) -> &'static str {
        "archive"
    }

    fn description(&self) -> &'static str {
        "Archive an idea"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_idea_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("IdeaArchive should be dispatched via execute_mut")
    }
}

impl MutableCommand for IdeaArchive {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let writer = SqliteWriter::open(ctx.db_path())?;
        writer.archive_idea(&self.id)?;

        let output = IdeaArchiveOutput {
            kind: "idea.archive",
            ok: true,
            id: self.id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Archived idea: {}", self.id),
            )),
        }
    }
}

// ============================================================================
// idea to-rfc
// ============================================================================

/// Convert an idea to an RFC.
#[derive(Debug, Clone)]
pub struct IdeaToRfc {
    pub id: String,
    pub feature: Option<String>,
}

impl IdeaToRfc {
    pub fn new(id: impl Into<String>, feature: Option<String>) -> Self {
        Self {
            id: id.into(),
            feature,
        }
    }
}

#[derive(Debug, Serialize)]
struct IdeaToRfcOutput {
    kind: &'static str,
    ok: bool,
    idea_id: String,
    rfc_id: String,
}

impl Command for IdeaToRfc {
    fn namespace(&self) -> &'static str {
        "idea"
    }

    fn operation(&self) -> &'static str {
        "to-rfc"
    }

    fn description(&self) -> &'static str {
        "Convert an idea to an RFC"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![
            SuggestedAction {
                label: "List ideas".to_string(),
                command: "exo idea list".to_string(),
                rationale: "Find the idea ID to convert.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.8),
            },
            SuggestedAction {
                label: "List RFCs".to_string(),
                command: "exo rfc list".to_string(),
                rationale: "View existing RFCs after conversion.".to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.6),
            },
        ]
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("IdeaToRfc should be dispatched via execute_mut")
    }
}

impl MutableCommand for IdeaToRfc {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let rfc_id = {
            use crate::context::SqliteLoader;

            let db_path = ctx.db_path();
            let loader = SqliteLoader::open(&db_path)?;
            let idea = loader.load_idea_by_id(&self.id)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "Idea not found: {}. Use `exo idea list` to see available ideas.",
                    self.id
                )
            })?;

            if idea.status == "archived" {
                anyhow::bail!("Cannot convert archived idea '{}' to RFC", self.id);
            }

            // Create the RFC
            let rfc_path = crate::rfc::create(
                ctx.root,
                &idea.title,
                None,
                self.feature.as_deref().unwrap_or("General"),
                0,
                Some(&idea.description),
            )?;

            let rfc_id = rfc_path
                .file_name()
                .and_then(|f| f.to_str())
                .and_then(|f| f.split('-').next())
                .unwrap_or("unknown")
                .to_string();

            // Archive the idea and add RFC reference
            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.archive_idea(&self.id)?;
            writer.add_idea_task_ref(&self.id, &format!("rfc:{rfc_id}"))?;

            rfc_id
        };

        let output = IdeaToRfcOutput {
            kind: "idea.to-rfc",
            ok: true,
            idea_id: self.id.clone(),
            rfc_id: rfc_id.clone(),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "Converted idea '{}' to RFC {}.\nIdea has been archived.",
                    self.id, rfc_id
                ),
            )),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idea_add_metadata() {
        let cmd = IdeaAdd::new("Test idea", "Description of test", vec!["test".to_string()]);
        assert_eq!(cmd.namespace(), "idea");
        assert_eq!(cmd.operation(), "add");
        assert_eq!(cmd.effect(), Effect::Write);
    }

    #[test]
    fn test_idea_list_metadata() {
        let cmd = IdeaList::new(None, None);
        assert_eq!(cmd.namespace(), "idea");
        assert_eq!(cmd.operation(), "list");
        assert_eq!(cmd.effect(), Effect::Pure);
    }
}
