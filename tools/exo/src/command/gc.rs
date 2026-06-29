//! Garbage collection commands.
//!
//! - `gc inbox`: Remove old archived inbox items

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::context::SqliteWriter;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use serde::Serialize;

/// Default steering for gc commands.
fn default_gc_steering() -> Vec<SuggestedAction> {
    vec![SuggestedAction {
        label: "List inbox items".to_string(),
        command: "exo inbox list --all".to_string(),
        rationale: "View all inbox items including archived.".to_string(),
        intent: WorkIntent::Orient,
        confidence: Some(0.5),
    }]
}

// ============================================================================
// ExoSpec definition — single source of truth for the gc namespace
// ============================================================================

/// Gc namespace command specification.
///
/// This enum is the authoritative definition of the gc namespace's commands,
/// arguments, and effects. The `#[derive(ExoSpec)]` macro generates:
/// - `HasExoSpec::spec()` → `NamespaceSpec` with all operations and args
/// - `GcCommands::from_invocation()` → typed construction from `Invocation`
#[derive(Debug, Clone, Copy, exospec::ExoSpec)]
#[exo(namespace = "gc", description = "Garbage collection commands")]
pub enum GcCommands {
    #[exo(effect = "write", description = "Remove old archived inbox items")]
    Inbox {
        #[exo(long, optional, default = "30", description = "Number of days to keep")]
        days: Option<i64>,
    },
}

impl GcCommands {
    /// Convert the parsed `ExoSpec` enum variant into a dispatchable `CommandBox`.
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Inbox { days } => {
                let days = days
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or(30);
                CommandBox::mutable(GcInbox::new(days))
            }
        })
    }
}

// ============================================================================
// gc inbox
// ============================================================================

/// Remove old archived inbox items.
///
/// Removes inbox items with status "archived" that are older than the
/// specified number of days.
#[derive(Debug, Clone, Copy)]
pub struct GcInbox {
    /// Days to keep archived items (default: 30).
    pub days: u64,
}

impl GcInbox {
    pub const fn new(days: u64) -> Self {
        Self { days }
    }
}

#[derive(Serialize)]
struct GcInboxOutput {
    kind: &'static str,
    ok: bool,
    removed: usize,
    days: u64,
}

impl Command for GcInbox {
    fn namespace(&self) -> &'static str {
        "gc"
    }

    fn operation(&self) -> &'static str {
        "inbox"
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        default_gc_steering()
    }

    fn execute(&self, _ctx: &CommandContext) -> ExoResult<CommandOutput> {
        unreachable!("GcInbox should be dispatched via execute_mut")
    }
}

impl MutableCommand for GcInbox {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let removed = {
            let writer = SqliteWriter::open(ctx.db_path())?;
            writer.gc_old_archived_inbox(self.days as u32)? as usize
        };

        let output = GcInboxOutput {
            kind: "gc.inbox",
            ok: true,
            removed,
            days: self.days,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = if removed == 0 {
                    format!(
                        "No archived inbox items older than {} day(s) found.",
                        self.days
                    )
                } else {
                    format!(
                        "Removed {} archived inbox item(s) older than {} day(s).",
                        removed, self.days
                    )
                };
                Ok(CommandOutput::new(output, message))
            }
        }
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
    fn test_gc_inbox_metadata() {
        let cmd = GcInbox::new(30);
        assert_eq!(cmd.namespace(), "gc");
        assert_eq!(cmd.operation(), "inbox");
        assert_eq!(cmd.effect(), Effect::Write);
        assert_eq!(cmd.days, 30);
    }

    #[test]
    fn test_gc_inbox_custom_days() {
        let cmd = GcInbox::new(7);
        assert_eq!(cmd.days, 7);
    }
}
