/// Display tier system for generating human-readable invocation messages.
///
/// Each command is assigned a display tier based on what identity information
/// is available at preview time (before execution). The tier determines the
/// template used to generate the `invocation_message` in `PreviewDisplay`.
///
/// See `docs/design/display-titles.md` for the full specification.
use serde_json::Value as JsonValue;

use super::protocol::ConfirmationInfo;

/// The display tier for a command, determining what identity information
/// appears in the invocation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTier {
    /// Entity has a slug (e.g., task, goal, rfc).
    /// Format: `"{Verb} {noun} '{slug}'"` — title added server-side post-execution.
    SlugPlusTitle,

    /// No slug, but title exists in data (e.g., epoch, phase).
    /// Format: `"{Verb} {noun}"` — title added server-side post-execution.
    TitleOnly,

    /// No slug, but a key option carries the content (e.g., rfc create --title).
    /// Format: `"{Verb} {noun} \"{option_value}\""`.
    OptionPreview {
        /// The JSON key to extract the option value from `CallParams.input`.
        option_key: &'static str,
    },

    /// Nothing useful available at call time.
    /// Format: `"{Verb} {noun}"`.
    Generic,
}

/// Map a command to its display tier based on namespace and operation.
///
/// This is the single source of truth for tier assignments. The mapping is
/// derived from the command display table in `docs/design/display-titles.md`.
pub fn tier_for_command(namespace: &str, operation: &str) -> DisplayTier {
    match (namespace, operation) {
        // Slug + title tier: entities with slugs as primary identifiers
        ("task", _) => slug_or_list(operation),
        ("goal", _) => slug_or_list(operation),
        ("rfc", "create") => DisplayTier::OptionPreview {
            option_key: "title",
        },
        ("rfc", _) => slug_or_list(operation),
        ("axiom", _) => slug_or_list_with_key(operation, "id"),

        // Title only tier: entities with titles but no slugs
        ("epoch", _) => DisplayTier::TitleOnly,
        ("phase", _) => DisplayTier::TitleOnly,

        // Option preview tier: creative actions where an option carries the content
        ("idea", "add") => DisplayTier::OptionPreview {
            option_key: "title",
        },
        ("inbox", "add") => DisplayTier::OptionPreview {
            option_key: "subject",
        },

        // Generic tier: everything else
        _ => DisplayTier::Generic,
    }
}

/// For slug-bearing namespaces, `list` is generic (no slug), everything else has a slug.
fn slug_or_list(operation: &str) -> DisplayTier {
    if operation == "list" {
        DisplayTier::Generic
    } else {
        DisplayTier::SlugPlusTitle
    }
}

/// Like `slug_or_list` but for namespaces where the slug comes from a named option (e.g., axiom --id).
fn slug_or_list_with_key(operation: &str, _key: &str) -> DisplayTier {
    if operation == "list" {
        DisplayTier::Generic
    } else {
        DisplayTier::SlugPlusTitle
    }
}

/// Generate a tier-based invocation message.
///
/// This replaces the mechanical `generate_invocation_message` with tier-aware
/// templates that extract identity information from the command args.
pub fn generate_preview_message(namespace: &str, operation: &str, input: &JsonValue) -> String {
    let tier = tier_for_command(namespace, operation);
    let verb = verb_for_operation(operation);
    let noun = noun_for_namespace(namespace, operation);

    match tier {
        DisplayTier::SlugPlusTitle => {
            // Extract slug from positional arg (usually "id")
            let slug = extract_slug(namespace, operation, input);
            match slug {
                Some(s) => format!("{verb} {noun} '{s}'"),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::TitleOnly => {
            // Title only — no slug available at preview time.
            // Server-side post-execution will enrich with title.
            format!("{verb} {noun}")
        }
        DisplayTier::OptionPreview { option_key } => {
            // Extract the option value (e.g., --title, --subject, --name)
            let option_value = input
                .get(option_key)
                .and_then(|v| v.as_str())
                .map(str::to_string);

            match option_value {
                Some(val) => format!("{verb} {noun} \"{val}\""),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::Generic => {
            format!("{verb} {noun}")
        }
    }
}

/// Generate a past-tense invocation message for post-completion display.
///
/// This is used for `pastTenseMessage` in the VS Code chat UI. After a tool
/// completes, the collapsed title transitions from the progressive-tense
/// `invocationMessage` to this past-tense version.
///
/// Example: "Completing task 'fix-bug'" → "Completed task 'fix-bug'"
///
/// Note: `pastTenseMessage` is behind the `chatParticipantPrivate` proposed API.
/// It's only used when the extension runs in VS Code Insiders with the proposed
/// API flag enabled. See Appendix A.2 in `docs/design/display-titles.md`.
pub fn generate_past_tense_message(namespace: &str, operation: &str, input: &JsonValue) -> String {
    let tier = tier_for_command(namespace, operation);
    let verb = past_tense_verb_for_operation(operation);
    let noun = noun_for_namespace(namespace, operation);

    match tier {
        DisplayTier::SlugPlusTitle => {
            let slug = extract_slug(namespace, operation, input);
            match slug {
                Some(s) => format!("{verb} {noun} '{s}'"),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::TitleOnly => {
            format!("{verb} {noun}")
        }
        DisplayTier::OptionPreview { option_key } => {
            let option_value = input
                .get(option_key)
                .and_then(|v| v.as_str())
                .map(str::to_string);

            match option_value {
                Some(val) => format!("{verb} {noun} \"{val}\""),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::Generic => {
            format!("{verb} {noun}")
        }
    }
}

/// Generate a post-execution invocation message, enriched with entity titles from result data.
///
/// This is the server-side path used in `display.invocation_message` after the command
/// has executed. It has access to the result data, so it can include entity titles
/// that weren't available at preview time.
///
/// For slug+title tier: `"Completing task 'fix-bug' (Fix the parser edge case)"`
/// For title-only tier: `"Starting phase \"LM Tool Architecture v2\""`
pub fn generate_display_message(
    namespace: &str,
    operation: &str,
    input: &JsonValue,
    result_data: &JsonValue,
) -> String {
    let tier = tier_for_command(namespace, operation);
    let verb = verb_for_operation(operation);
    let noun = noun_for_namespace(namespace, operation);

    match tier {
        DisplayTier::SlugPlusTitle => {
            let slug = extract_slug(namespace, operation, input);
            let title = extract_title_from_result(namespace, result_data);

            match (slug, title) {
                (Some(s), Some(t)) => format!("{verb} {noun} '{s}' ({t})"),
                (Some(s), None) => format!("{verb} {noun} '{s}'"),
                _ => format!("{verb} {noun}"),
            }
        }
        DisplayTier::TitleOnly => {
            // Look up title from result data
            let title = extract_title_from_result(namespace, result_data);
            match title {
                Some(t) => format!("{verb} {noun} \"{t}\""),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::OptionPreview { option_key } => {
            let option_value = input
                .get(option_key)
                .and_then(|v| v.as_str())
                .map(str::to_string);

            match option_value {
                Some(val) => format!("{verb} {noun} \"{val}\""),
                None => format!("{verb} {noun}"),
            }
        }
        DisplayTier::Generic => {
            format!("{verb} {noun}")
        }
    }
}

/// Extract an entity's title/label from the command result data.
///
/// Different entities store their title under different JSON keys.
fn extract_title_from_result(namespace: &str, data: &JsonValue) -> Option<String> {
    let keys: &[&str] = match namespace {
        "task" => &["title", "label"],
        "goal" => &["label", "title"],
        "rfc" | "epoch" | "phase" | "idea" => &["title"],
        "inbox" => &["subject"],
        "axiom" => &["label"],
        _ => return None,
    };

    // Try direct field first, then nested under common result wrappers
    keys.iter()
        .find_map(|key| data.get(*key).and_then(|v| v.as_str()))
        .or_else(|| {
            // Some results nest the entity under a key like "task", "goal", etc.
            let entity = data.get(namespace)?;
            keys.iter()
                .find_map(|key| entity.get(*key).and_then(|v| v.as_str()))
        })
        .map(str::to_string)
}

/// Get the past-tense verb for an operation.
///
/// Used to generate `past_tense_message` — the title shown after tool completion.
/// Maps the same operations as `verb_for_operation` but in past tense.
fn past_tense_verb_for_operation(operation: &str) -> &'static str {
    match operation {
        "list" => "Listed",
        "add" => "Added",
        "complete" => "Completed",
        "start" => "Started",
        "remove" => "Removed",
        "update" => "Updated",
        "log" => "Logged progress on",
        "reorder" => "Reordered",
        "create" => "Created",
        "promote" => "Promoted",
        "show" => "Showed",
        "finish" => "Finished",
        "status" => "Checked",
        "abandon" => "Abandoned",
        "satisfy" => "Satisfied",
        "unsatisfy" => "Unsatisfied",
        "withdraw" => "Withdrew",
        "archive" => "Archived",
        "rename" => "Renamed",
        "supersede" => "Superseded",
        "edit" => "Edited",
        "ack" => "Acknowledged",
        "resolve" => "Resolved",
        "new" => "Started",
        "review" => "Reviewed",
        "bankrupt" => "Bankrupted",
        "history" => "Showed",
        "run" => "Ran",
        "to-rfc" => "Converted",
        "update-status" => "Updated",
        "paths" => "Showed",
        "restore" => "Restored",
        "context" => "Dumped",
        "prompt" => "Got",
        "chat-history" => "Read",
        "threads" => "Listed",
        "thread.create" => "Created",
        "thread.reply" => "Replied to",
        "thread.status" => "Updated",
        "links.check" => "Checked",
        "execution.tasks" => "Listed",
        _ => "Ran",
    }
}

/// Get the progressive verb for an operation.
fn verb_for_operation(operation: &str) -> &'static str {
    match operation {
        "list" => "Listing",
        "add" => "Adding",
        "complete" => "Completing",
        "start" => "Starting",
        "remove" => "Removing",
        "update" => "Updating",
        "log" => "Logging progress on",
        "reorder" => "Reordering",
        "create" => "Creating",
        "promote" => "Promoting",
        "show" => "Showing",
        "finish" => "Finishing",
        "status" => "Checking",
        "abandon" => "Abandoning",
        "satisfy" => "Satisfying",
        "unsatisfy" => "Unsatisfying",
        "withdraw" => "Withdrawing",
        "archive" => "Archiving",
        "rename" => "Renaming",
        "supersede" => "Superseding",
        "edit" => "Editing",
        "ack" => "Acknowledging",
        "resolve" => "Resolving",
        "new" => "Starting",
        "review" => "Reviewing",
        "bankrupt" => "Bankrupting",
        "history" => "Showing",
        "run" => "Running",
        "to-rfc" => "Converting",
        "update-status" => "Updating",
        "paths" => "Showing",
        "restore" => "Restoring",
        "context" => "Dumping",
        "prompt" => "Getting",
        "chat-history" => "Reading",
        "threads" => "Listing",
        "thread.create" => "Creating",
        "thread.reply" => "Replying to",
        "thread.status" => "Updating",
        "links.check" => "Checking",
        "execution.tasks" => "Listing",
        _ => "Running",
    }
}

/// Get the human-readable noun for a namespace/operation combination.
///
/// Uses singular for actions on specific entities, plural for discovery (list).
fn noun_for_namespace(namespace: &str, operation: &str) -> String {
    let is_list = operation == "list" || operation == "threads";

    match (namespace, operation) {
        // Root commands
        ("", "status") => "project status".to_string(),
        ("", "map") => "project".to_string(),
        ("", "update") => "project upgrades".to_string(),
        ("", "write") => "context file".to_string(),
        ("", "version") => "version".to_string(),

        // Standard entity namespaces
        ("task", _) => if is_list { "tasks" } else { "task" }.to_string(),
        ("goal", _) => if is_list { "goals" } else { "goal" }.to_string(),
        ("rfc", _) => if is_list { "RFCs" } else { "RFC" }.to_string(),
        ("axiom", _) => if is_list { "axioms" } else { "axiom" }.to_string(),
        ("epoch", _) => if is_list { "epochs" } else { "epoch" }.to_string(),
        ("phase", "execution.tasks") => "phase execution tasks".to_string(),
        ("phase", "history") => "phase history".to_string(),
        ("phase", "status") => "phase status".to_string(),
        ("phase", _) => if is_list { "phases" } else { "phase" }.to_string(),

        // Creative/opaque namespaces
        ("idea", "to-rfc") => "idea to RFC".to_string(),
        ("idea", _) => if is_list { "ideas" } else { "idea" }.to_string(),
        ("inbox", _) => if is_list { "inbox" } else { "inbox item" }.to_string(),

        // Infrastructure
        ("commit", "create") => "commit".to_string(),
        ("commit", "status") => "git status".to_string(),
        ("strike", "start") => "surgical strike".to_string(),
        ("strike", "finish") => "surgical strike".to_string(),
        ("strike", "abort") => "surgical strike".to_string(),
        ("context", "paths") => "context paths".to_string(),
        ("context", "restore") => "context".to_string(),
        ("ai", "context") => "AI context".to_string(),
        ("ai", "prompt") => "prompt".to_string(),
        ("ai", "chat-history") => "chat history".to_string(),
        ("plan", "review") => "plan".to_string(),
        ("plan", "update-status") => "plan item status".to_string(),
        ("verify", "run") => "verification".to_string(),
        ("gc", "inbox") => "inbox".to_string(),
        ("docs", "links.check") => "documentation links".to_string(),
        ("run", _) => "task".to_string(),

        // Fallback
        _ => namespace.to_string(),
    }
}

/// Extract the slug from the command input.
///
/// For most slug-bearing namespaces, the slug is the `id` field.
/// For axiom, it's also the `id` field.
fn extract_slug(namespace: &str, _operation: &str, input: &JsonValue) -> Option<String> {
    // The slug field name varies by namespace
    let key = match namespace {
        "axiom" => "id",
        "ai" => "name",  // ai prompt {name}
        "run" => "slug", // run task {slug}
        _ => "id",       // task, goal, rfc all use "id"
    };

    input.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Determine whether a command should show a confirmation dialog before executing.
///
/// Returns `Some(ConfirmationInfo)` for destructive operations that are difficult
/// or impossible to undo, `None` for safe operations.
///
/// See Appendix A.3 in `docs/design/display-titles.md` for the full list.
pub fn confirmation_for_command(
    namespace: &str,
    operation: &str,
    invocation_message: &str,
) -> Option<ConfirmationInfo> {
    match (namespace, operation) {
        ("task", "remove") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This will permanently remove the task and its log history.".to_string(),
        }),
        ("goal", "remove") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This will permanently remove the goal.".to_string(),
        }),
        ("goal", "abandon") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This marks the goal as abandoned. This cannot be undone.".to_string(),
        }),
        ("rfc", "withdraw") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This withdraws the RFC. Withdrawn RFCs cannot be reactivated.".to_string(),
        }),
        ("epoch", "bankrupt") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This is a drastic recovery action that resets the epoch's progress."
                .to_string(),
        }),
        ("phase", "finish") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This will commit all changes, archive the phase, and advance to the next."
                .to_string(),
        }),
        ("strike", "start") => Some(ConfirmationInfo {
            title: format!("{invocation_message}?"),
            message: "This enters surgical strike mode, which focuses work on a specific goal."
                .to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tier_detection_slug_plus_title() {
        assert_eq!(
            tier_for_command("task", "complete"),
            DisplayTier::SlugPlusTitle
        );
        assert_eq!(tier_for_command("goal", "add"), DisplayTier::SlugPlusTitle);
        assert_eq!(tier_for_command("rfc", "show"), DisplayTier::SlugPlusTitle);
    }

    #[test]
    fn test_tier_detection_list_is_generic() {
        assert_eq!(tier_for_command("task", "list"), DisplayTier::Generic);
        assert_eq!(tier_for_command("goal", "list"), DisplayTier::Generic);
        assert_eq!(tier_for_command("rfc", "list"), DisplayTier::Generic);
    }

    #[test]
    fn test_tier_detection_title_only() {
        assert_eq!(tier_for_command("epoch", "start"), DisplayTier::TitleOnly);
        assert_eq!(tier_for_command("phase", "finish"), DisplayTier::TitleOnly);
    }

    #[test]
    fn test_tier_detection_option_preview() {
        assert!(matches!(
            tier_for_command("rfc", "create"),
            DisplayTier::OptionPreview {
                option_key: "title"
            }
        ));
        assert!(matches!(
            tier_for_command("idea", "add"),
            DisplayTier::OptionPreview {
                option_key: "title"
            }
        ));
        assert!(matches!(
            tier_for_command("inbox", "add"),
            DisplayTier::OptionPreview {
                option_key: "subject"
            }
        ));
    }

    #[test]
    fn test_tier_detection_generic() {
        assert_eq!(tier_for_command("strike", "start"), DisplayTier::Generic);
        assert_eq!(tier_for_command("verify", "run"), DisplayTier::Generic);
    }

    #[test]
    fn test_preview_message_slug_with_id() {
        let input = json!({"id": "fix-bug"});
        assert_eq!(
            generate_preview_message("task", "complete", &input),
            "Completing task 'fix-bug'"
        );
    }

    #[test]
    fn test_preview_message_slug_without_id() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("task", "complete", &input),
            "Completing task"
        );
    }

    #[test]
    fn test_preview_message_list() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("task", "list", &input),
            "Listing tasks"
        );
    }

    #[test]
    fn test_preview_message_option_preview_with_value() {
        let input = json!({"title": "New Feature Idea"});
        assert_eq!(
            generate_preview_message("rfc", "create", &input),
            "Creating RFC \"New Feature Idea\""
        );
    }

    #[test]
    fn test_preview_message_option_preview_inbox() {
        let input = json!({"subject": "Bug in parser"});
        assert_eq!(
            generate_preview_message("inbox", "add", &input),
            "Adding inbox item \"Bug in parser\""
        );
    }

    #[test]
    fn test_preview_message_option_preview_without_value() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("rfc", "create", &input),
            "Creating RFC"
        );
    }

    #[test]
    fn test_preview_message_title_only() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("epoch", "start", &input),
            "Starting epoch"
        );
        assert_eq!(
            generate_preview_message("phase", "finish", &input),
            "Finishing phase"
        );
    }

    #[test]
    fn test_preview_message_generic() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("verify", "run", &input),
            "Running verification"
        );
    }

    #[test]
    fn test_preview_message_rfc_with_id() {
        let input = json!({"id": "00224"});
        assert_eq!(
            generate_preview_message("rfc", "show", &input),
            "Showing RFC '00224'"
        );
    }

    #[test]
    fn test_preview_message_root_commands() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("", "status", &input),
            "Checking project status"
        );
    }

    #[test]
    fn test_preview_message_idea_add() {
        let input = json!({"title": "Symbol rename as a tool"});
        assert_eq!(
            generate_preview_message("idea", "add", &input),
            "Adding idea \"Symbol rename as a tool\""
        );
    }

    #[test]
    fn test_preview_message_goal_complete() {
        let input = json!({"id": "preview-roundtrip"});
        assert_eq!(
            generate_preview_message("goal", "complete", &input),
            "Completing goal 'preview-roundtrip'"
        );
    }

    #[test]
    fn test_preview_message_phase_status() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("phase", "status", &input),
            "Checking phase status"
        );
    }

    #[test]
    fn test_preview_message_compound_operations() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("phase", "execution.tasks", &input),
            "Listing phase execution tasks"
        );
        assert_eq!(
            generate_preview_message("docs", "links.check", &input),
            "Checking documentation links"
        );
    }

    #[test]
    fn test_preview_message_strike() {
        let input = json!({});
        assert_eq!(
            generate_preview_message("strike", "start", &input),
            "Starting surgical strike"
        );
        assert_eq!(
            generate_preview_message("strike", "finish", &input),
            "Finishing surgical strike"
        );
    }

    // --- Post-execution display message tests (with title enrichment) ---

    #[test]
    fn test_display_message_task_with_title() {
        let input = json!({"id": "fix-bug"});
        let result = json!({"label": "Fix the parser edge case"});
        assert_eq!(
            generate_display_message("task", "complete", &input, &result),
            "Completing task 'fix-bug' (Fix the parser edge case)"
        );
    }

    #[test]
    fn test_display_message_task_rename_uses_handle_and_title() {
        let input = json!({"id": "old-handle", "to": "new-handle"});
        let result = json!({"task_id": "new-handle", "title": "Fix task addressing"});
        assert_eq!(
            generate_display_message("task", "rename", &input, &result),
            "Renaming task 'old-handle' (Fix task addressing)"
        );
    }

    #[test]
    fn test_display_message_task_without_title() {
        let input = json!({"id": "fix-bug"});
        let result = json!({});
        assert_eq!(
            generate_display_message("task", "complete", &input, &result),
            "Completing task 'fix-bug'"
        );
    }

    #[test]
    fn test_display_message_rfc_with_title() {
        let input = json!({"id": "00224"});
        let result = json!({"title": "The SOAR Loop"});
        assert_eq!(
            generate_display_message("rfc", "show", &input, &result),
            "Showing RFC '00224' (The SOAR Loop)"
        );
    }

    #[test]
    fn test_display_message_phase_with_title() {
        let input = json!({});
        let result = json!({"title": "LM Tool Architecture v2"});
        assert_eq!(
            generate_display_message("phase", "start", &input, &result),
            "Starting phase \"LM Tool Architecture v2\""
        );
    }

    #[test]
    fn test_display_message_phase_without_title() {
        let input = json!({});
        let result = json!({});
        assert_eq!(
            generate_display_message("phase", "finish", &input, &result),
            "Finishing phase"
        );
    }

    #[test]
    fn test_display_message_goal_with_label() {
        let input = json!({"id": "preview-roundtrip"});
        let result = json!({"label": "PER 1: Preview round-trip"});
        assert_eq!(
            generate_display_message("goal", "complete", &input, &result),
            "Completing goal 'preview-roundtrip' (PER 1: Preview round-trip)"
        );
    }

    #[test]
    fn test_display_message_epoch_with_title() {
        let input = json!({});
        let result = json!({"title": "The Goal Loop"});
        assert_eq!(
            generate_display_message("epoch", "start", &input, &result),
            "Starting epoch \"The Goal Loop\""
        );
    }

    #[test]
    fn test_display_message_generic_ignores_result() {
        let input = json!({});
        let result = json!({"anything": "here"});
        assert_eq!(
            generate_display_message("verify", "run", &input, &result),
            "Running verification"
        );
    }

    #[test]
    fn test_display_message_nested_entity() {
        // Some results nest the entity under a key like "task"
        let input = json!({"id": "fix-bug"});
        let result = json!({"task": {"label": "Fix the parser edge case"}});
        assert_eq!(
            generate_display_message("task", "complete", &input, &result),
            "Completing task 'fix-bug' (Fix the parser edge case)"
        );
    }

    // --- Confirmation tests ---

    #[test]
    fn test_confirmation_destructive_ops() {
        let confirm = confirmation_for_command("task", "remove", "Removing task 'fix-bug'");
        assert!(confirm.is_some());
        let info = confirm.unwrap();
        assert_eq!(info.title, "Removing task 'fix-bug'?");
        assert!(info.message.contains("permanently remove"));
    }

    #[test]
    fn test_confirmation_phase_finish() {
        let confirm = confirmation_for_command("phase", "finish", "Finishing phase");
        assert!(confirm.is_some());
        let info = confirm.unwrap();
        assert_eq!(info.title, "Finishing phase?");
        assert!(info.message.contains("commit"));
    }

    #[test]
    fn test_confirmation_safe_ops_return_none() {
        assert!(confirmation_for_command("task", "list", "Listing tasks").is_none());
        assert!(confirmation_for_command("task", "add", "Adding task 'x'").is_none());
        assert!(confirmation_for_command("rfc", "create", "Creating RFC").is_none());
        assert!(confirmation_for_command("goal", "complete", "Completing goal").is_none());
    }

    #[test]
    fn test_confirmation_all_destructive_commands() {
        // Verify all documented destructive commands have confirmations
        let destructive = vec![
            ("task", "remove"),
            ("goal", "remove"),
            ("goal", "abandon"),
            ("rfc", "withdraw"),
            ("epoch", "bankrupt"),
            ("phase", "finish"),
            ("strike", "start"),
        ];
        for (ns, op) in destructive {
            assert!(
                confirmation_for_command(ns, op, "test").is_some(),
                "Expected confirmation for {ns} {op}"
            );
        }
    }

    // --- Past-tense message tests ---

    #[test]
    fn test_past_tense_task_complete() {
        let input = json!({"id": "fix-bug"});
        assert_eq!(
            generate_past_tense_message("task", "complete", &input),
            "Completed task 'fix-bug'"
        );
    }

    #[test]
    fn test_past_tense_task_list() {
        let input = json!({});
        assert_eq!(
            generate_past_tense_message("task", "list", &input),
            "Listed tasks"
        );
    }

    #[test]
    fn test_past_tense_rfc_create() {
        let input = json!({"title": "New Feature Idea"});
        assert_eq!(
            generate_past_tense_message("rfc", "create", &input),
            "Created RFC \"New Feature Idea\""
        );
    }

    #[test]
    fn test_past_tense_phase_finish() {
        let input = json!({});
        assert_eq!(
            generate_past_tense_message("phase", "finish", &input),
            "Finished phase"
        );
    }

    #[test]
    fn test_past_tense_epoch_start() {
        let input = json!({});
        assert_eq!(
            generate_past_tense_message("epoch", "start", &input),
            "Started epoch"
        );
    }

    #[test]
    fn test_past_tense_goal_abandon() {
        let input = json!({"id": "old-goal"});
        assert_eq!(
            generate_past_tense_message("goal", "abandon", &input),
            "Abandoned goal 'old-goal'"
        );
    }

    #[test]
    fn test_past_tense_status() {
        let input = json!({});
        assert_eq!(
            generate_past_tense_message("", "status", &input),
            "Checked project status"
        );
    }

    #[test]
    fn test_past_tense_idea_add() {
        let input = json!({"title": "Symbol rename as a tool"});
        assert_eq!(
            generate_past_tense_message("idea", "add", &input),
            "Added idea \"Symbol rename as a tool\""
        );
    }

    #[test]
    fn test_past_tense_mirrors_preview_structure() {
        // Verify that past-tense messages have the same structure as preview
        // messages, just with different verb forms.
        let input = json!({"id": "test-slug"});
        let preview = generate_preview_message("task", "remove", &input);
        let past = generate_past_tense_message("task", "remove", &input);

        // Both should contain the slug
        assert!(
            preview.contains("'test-slug'"),
            "Preview should contain slug"
        );
        assert!(
            past.contains("'test-slug'"),
            "Past tense should contain slug"
        );

        // Preview uses progressive, past uses past tense
        assert!(
            preview.starts_with("Removing"),
            "Preview should be progressive"
        );
        assert!(
            past.starts_with("Removed"),
            "Past tense should be past tense"
        );
    }
}
