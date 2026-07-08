use serde_json::{Value as JsonValue, json};
use std::path::Path;
use std::time::Instant;

use crate::api::display::{
    confirmation_for_command, generate_display_message, generate_past_tense_message,
    generate_preview_message,
};
use crate::api::protocol;
use crate::api::protocol::{
    Address, Auth, CallParams, Display, Effect, ErrorBody, ErrorCode, HelpNamespace, HelpOperation,
    HelpParams, HelpResult, ListParams, NextCall, NextCallKind, Op, Page, PreviewDisplay,
    RequestEnvelope, ResponseEnvelope, Status, Steering,
};
use crate::command::command_spec::CommandSpec as NewCommandSpec;
use crate::command::registry::{build_command_from_invocation, default_registry};
use crate::command::router::{DiagnosticCode, Invocation, RoutingDiagnostic};
use crate::command::traits::{CommandInvokeResult, invoke_command_box_json};
use crate::command::transport::{MachineChannelTransport, ticket_for_exec_call};
use crate::daemon::DaemonEnsureState;
use crate::daemon_diagnostics::{
    DaemonDiagnostics, effect_name, elapsed_ms, request_op_path, response_status,
};
use crate::failure::ExoFailure;
use crate::project::Project;
use crate::router::compile_argv;

const DEFAULT_PAGE_LIMIT: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandlerRuntime {
    External,
    SidecarWriter,
}

/// Log a command execution event to the `agent_events` table (RFC 10183).
///
/// Best-effort: errors are silently ignored — event logging should never
/// block command execution. Uses the shared event-DB connection cache
/// (`crate::event_db`) so the daemon doesn't open a fresh connection per
/// request.
#[allow(clippy::too_many_arguments)]
fn log_command_event(
    workspace_root: &Path,
    project: Option<&Project>,
    agent_id: Option<&str>,
    namespace: &str,
    operation: &str,
    input: &JsonValue,
    result: &JsonValue,
    effect: Effect,
    duration_ms: u64,
    summary: &str,
) -> bool {
    let db_path = crate::context::db_path(workspace_root, project);

    let text_id = ulid::Ulid::new().to_string().to_lowercase();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let effect_str = match effect {
        Effect::Pure => "read",
        Effect::Write | Effect::Exec => "write",
    };

    // Infer entity from namespace + input["id"]
    let entity_type = if namespace.is_empty() {
        None
    } else {
        Some(namespace)
    };
    let entity_id = if namespace == "task" {
        result
            .get("task_id")
            .and_then(JsonValue::as_str)
            .or_else(|| input.get("id").and_then(JsonValue::as_str))
    } else {
        input.get("id").and_then(JsonValue::as_str)
    };

    crate::event_db::with_event_db(&db_path, |conn| {
        conn.execute(
            "INSERT INTO agent_events (text_id, timestamp, agent_id, event_type, namespace, operation, entity_type, entity_id, effect, duration_ms, summary)
             VALUES (?1, ?2, ?3, 'command', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            exosuit_storage::params![
                text_id,
                timestamp,
                agent_id,
                namespace,
                operation,
                entity_type,
                entity_id,
                effect_str,
                i64::try_from(duration_ms).unwrap_or(i64::MAX),
                summary,
            ],
        )
    })
    .is_some()
}

/// Generate display metadata from a command result.
///
/// All display fields are derived from the structured data and the command path.
/// The `invocation_message` uses tier-based templates enriched with entity titles
/// from the result data (e.g., "Completing task 'fix-bug' (Fix the parser edge case)").
/// The `summary` is a one-line description of the result.
/// The `body` is a full markdown rendering of the result (for list commands, etc.).
fn make_display(
    namespace: &str,
    operation: &str,
    input: &JsonValue,
    invoke_result: &CommandInvokeResult,
) -> Option<Display> {
    let invocation_message =
        generate_display_message(namespace, operation, input, &invoke_result.data);
    let summary = generate_summary_from_data(namespace, operation, input, &invoke_result.data);
    let body = generate_body_from_data(namespace, operation, &invoke_result.data);

    Some(Display {
        invocation_message,
        summary,
        body,
    })
}

/// Generate a summary from the result data when no human message is available.
fn generate_summary_from_data(
    namespace: &str,
    operation: &str,
    input: &JsonValue,
    data: &JsonValue,
) -> String {
    // Root status → phase title + progress mode
    if namespace.is_empty() && operation == "status" {
        let phase = data
            .get("phase_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("No active phase");
        let mode = data
            .get("progress_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if mode.is_empty() {
            return format!("Phase: {phase}");
        }
        return format!("{phase} ({mode})");
    }

    // Try to extract count from list results
    if operation == "list" {
        let items_key = match namespace {
            "task" => "tasks",
            "goal" => "goals",
            "rfc" => "rfcs",
            "epoch" => "epochs",
            "idea" => "ideas",
            "inbox" => "items",
            _ => "items",
        };

        if let Some(items) = data.get(items_key).and_then(JsonValue::as_array) {
            let count = items.len();
            let noun = match namespace {
                "inbox" => {
                    if count == 1 {
                        "inbox item".to_string()
                    } else {
                        "inbox items".to_string()
                    }
                }
                _ => {
                    if count == 1 {
                        namespace.to_string()
                    } else {
                        format!("{namespace}s")
                    }
                }
            };
            return format!("{count} {noun}");
        }
    }

    // Phase history → count
    if namespace == "phase" && operation == "history" {
        let count = data
            .get("phases")
            .and_then(JsonValue::as_array)
            .map_or(0, Vec::len);
        return format!("{count} completed phases");
    }

    // Epoch status → title + phase count
    if namespace == "epoch" && operation == "status" {
        let title = data
            .get("title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Epoch");
        let phase_count = data
            .get("phases")
            .and_then(JsonValue::as_array)
            .map_or(0, std::vec::Vec::len);
        let status = data
            .get("status")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        return format!("{title} ({status}, {phase_count} phases)");
    }

    // Phase status → title
    if namespace == "phase" && operation == "status" {
        let title = data
            .get("phase_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Phase");
        let epoch = data
            .get("epoch_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if epoch.is_empty() {
            return format!("Phase: {title}");
        }
        return format!("{title} (in {epoch})");
    }

    // Mutations with ok + kind → human-readable summary using input data
    if data.get("ok").and_then(JsonValue::as_bool) == Some(true)
        && let Some(kind) = data.get("kind").and_then(JsonValue::as_str)
    {
        return generate_mutation_summary(kind, input, data);
    }

    format!("{namespace} {operation}: done")
}

/// Generate a human-readable summary for mutation results.
///
/// Instead of "task.add: OK", produce something like
/// "Added task 'root-status' to goal display-body-coverage".
fn generate_mutation_summary(kind: &str, input: &JsonValue, data: &JsonValue) -> String {
    let id = input
        .get("id")
        .and_then(JsonValue::as_str)
        .or_else(|| data.get("task_id").and_then(JsonValue::as_str))
        .or_else(|| data.get("goal_id").and_then(JsonValue::as_str))
        .or_else(|| data.get("phase_id").and_then(JsonValue::as_str))
        .or_else(|| data.get("id").and_then(JsonValue::as_str));
    let label = input.get("label").and_then(JsonValue::as_str);
    let title = data.get("title").and_then(JsonValue::as_str).or(label);
    let goal = input
        .get("goal")
        .and_then(JsonValue::as_str)
        .or_else(|| data.get("goal_id").and_then(JsonValue::as_str));

    match kind {
        "task.add" => {
            let name = id.or(label).unwrap_or("task");
            let subject = title.map_or_else(
                || format!("task '{name}'"),
                |title| format!("task '{name}' ({title})"),
            );
            match goal {
                Some(g) => format!("Added {subject} to goal {g}"),
                None => format!("Added {subject}"),
            }
        }
        "task.complete" => {
            let name = id.unwrap_or("task");
            match input.get("log").and_then(JsonValue::as_str) {
                Some(log) => format!("Completed task '{name}': {log}"),
                None => format!("Completed task '{name}'"),
            }
        }
        "task.start" => {
            let name = id.unwrap_or("task");
            format!("Started task '{name}'")
        }
        "task.remove" => {
            let name = id.unwrap_or("task");
            format!("Removed task '{name}'")
        }
        "task.update" => {
            let name = id.unwrap_or("task");
            match label {
                Some(l) => format!("Updated task '{name}' → '{l}'"),
                None => format!("Updated task '{name}'"),
            }
        }
        "task.rename" => {
            let old = data
                .get("old_task_id")
                .and_then(JsonValue::as_str)
                .unwrap_or("task");
            let new = data
                .get("task_id")
                .and_then(JsonValue::as_str)
                .unwrap_or("task");
            let title = data.get("title").and_then(JsonValue::as_str);
            match title {
                Some(title) => format!("Renamed task '{old}' ({title}) to '{new}'"),
                None => format!("Renamed task '{old}' to '{new}'"),
            }
        }
        "goal.add" => {
            let name = id.unwrap_or_else(|| label.unwrap_or("goal"));
            title.map_or_else(
                || format!("Added goal '{name}'"),
                |title| format!("Added goal '{name}' ({title})"),
            )
        }
        "goal.complete" => {
            let name = id.unwrap_or("goal");
            match input.get("log").and_then(JsonValue::as_str) {
                Some(log) => format!("Completed goal '{name}': {log}"),
                None => format!("Completed goal '{name}'"),
            }
        }
        "idea.add" => {
            let title = label
                .or_else(|| input.get("title").and_then(JsonValue::as_str))
                .unwrap_or("idea");
            format!("Added idea: {title}")
        }
        "inbox.add" => {
            let subject = input
                .get("subject")
                .and_then(JsonValue::as_str)
                .unwrap_or("item");
            format!("Added inbox item: {subject}")
        }
        "phase.start" => {
            let title = data.get("title").and_then(JsonValue::as_str).or(label);
            title.map_or_else(
                || "Started phase".to_string(),
                |title| format!("Started phase \"{title}\""),
            )
        }
        "phase.add" => {
            let title = data
                .get("title")
                .and_then(JsonValue::as_str)
                .or(label)
                .unwrap_or("");
            if title.is_empty() {
                "Added phase".to_string()
            } else {
                format!("Added phase \"{title}\"")
            }
        }
        "phase.move" => {
            let title = data.get("title").and_then(JsonValue::as_str);
            let subject =
                title.map_or_else(|| "phase".to_string(), |title| format!("phase \"{title}\""));
            match data.get("position").and_then(JsonValue::as_str) {
                Some(position) => format!("Moved {subject} to {position}"),
                None => format!("Moved {subject}"),
            }
        }
        "phase.reorder" => {
            let title = data.get("title").and_then(JsonValue::as_str);
            let position = data
                .get("position")
                .and_then(JsonValue::as_str)
                .unwrap_or("requested position");
            title.map_or_else(
                || format!("Reordered phase to {position}"),
                |title| format!("Reordered phase \"{title}\" to {position}"),
            )
        }
        "phase.finish" => {
            let msg = input.get("message").and_then(JsonValue::as_str);
            match msg {
                Some(m) => format!("Finished phase: {m}"),
                None => "Finished phase".to_string(),
            }
        }
        "rfc.create" => {
            let title = input
                .get("title")
                .and_then(JsonValue::as_str)
                .unwrap_or("RFC");
            format!("Created RFC: {title}")
        }
        _ => {
            // Fallback: humanize the kind string
            let humanized = kind.replace('.', " ");
            match label.or(id) {
                Some(name) => format!("{humanized}: {name}"),
                None => format!("{humanized}: done"),
            }
        }
    }
}

/// Generate a plain-text body from the result data for rich display.
///
/// Returns `None` when the summary alone is sufficient (e.g., simple ok results).
/// Returns `Some(text)` for list results and other data-rich responses.
///
/// **Important**: Tool output is rendered as plain text, not markdown.
/// Do not use markdown syntax (`**`, `#`, `|` tables, backticks).
fn generate_body_from_data(namespace: &str, operation: &str, data: &JsonValue) -> Option<String> {
    if namespace == "sidecar"
        && operation == "repo"
        && value_str(data, &["kind"]) == Some("sidecar.repo.status")
    {
        return Some(render_sidecar_repo_status(data));
    }

    // Task list
    if namespace == "task"
        && operation == "list"
        && let Some(tasks) = data.get("tasks").and_then(JsonValue::as_array)
    {
        if tasks.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for t in tasks {
            let id = t.get("id").and_then(JsonValue::as_str).unwrap_or("?");
            let label = t.get("label").and_then(JsonValue::as_str).unwrap_or("");
            let status = t.get("status").and_then(JsonValue::as_str).unwrap_or("");
            let icon = match status {
                "completed" => "✅",
                "in-progress" => "🔄",
                _ => "⏳",
            };
            let goal_scope = t
                .get("goal_id")
                .and_then(JsonValue::as_str)
                .map_or(String::new(), |g| format!(" ({g})"));
            lines.push(format!("{icon} {id}{goal_scope} — {label}"));
        }
        return Some(lines.join("\n"));
    }

    // Goal list
    if namespace == "goal"
        && operation == "list"
        && let Some(goals) = data.get("goals").and_then(JsonValue::as_array)
    {
        if goals.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for g in goals {
            let id = g.get("id").and_then(JsonValue::as_str).unwrap_or("?");
            let label = g.get("label").and_then(JsonValue::as_str).unwrap_or("");
            let status = g.get("status").and_then(JsonValue::as_str).unwrap_or("");
            let icon = match status {
                "completed" => "✅",
                "abandoned" => "⛔",
                "in-progress" => "🔄",
                _ => "⏳",
            };
            lines.push(format!("{icon} {id} — {label}"));
        }
        return Some(lines.join("\n"));
    }

    // RFC list
    if namespace == "rfc"
        && operation == "list"
        && let Some(rfcs) = data.get("rfcs").and_then(JsonValue::as_array)
    {
        if rfcs.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for r in rfcs {
            let id = r.get("id").and_then(JsonValue::as_str).unwrap_or("?");
            let title = r.get("title").and_then(JsonValue::as_str).unwrap_or("");
            let stage = r.get("stage").and_then(JsonValue::as_u64).unwrap_or(0);
            let status = value_str(r, &["status"]).unwrap_or("active");
            if status == "active" {
                lines.push(format!("[Stage {stage}] {id}: {title}"));
            } else {
                lines.push(format!(
                    "[{}] {id}: {title}",
                    lifecycle_status_label(status)
                ));
            }
        }
        return Some(lines.join("\n"));
    }

    // RFC show → compact identity, stage, and path.
    if namespace == "rfc" && operation == "show" {
        let id = value_str(data, &["id"]).unwrap_or("?");
        let title = value_str(data, &["title"]).unwrap_or("Untitled RFC");
        let stage = data.get("stage").and_then(JsonValue::as_u64).unwrap_or(0);
        let status = value_str(data, &["status"]).unwrap_or("active");
        let feature = value_str(data, &["feature"]).unwrap_or("(none)");
        let filename = value_str(data, &["filename"]).unwrap_or("(unknown path)");
        if status == "active" {
            let mut lines = vec![
                format!("RFC {id}: {title}"),
                format!("Stage: {stage} ({})", rfc_stage_name(stage)),
            ];
            let superseded_by = value_str(data, &["superseded_by", "supersededBy"]);
            if let Some(by) = superseded_by {
                lines.push(format!("Superseded by: RFC {by}"));
            }
            lines.push(format!("Feature: {feature}"));
            lines.push(format!("File: {filename}"));
            if superseded_by.is_none() {
                lines.push(format!("Next: rfc promote {id} --stage <int>"));
            }
            return Some(lines.join("\n"));
        }

        let mut lines = vec![
            format!("RFC {id}: {title}"),
            format!("Status: {}", lifecycle_status_label(status)),
        ];
        if let Some(reason) = value_str(data, &["archived_reason", "archivedReason"])
            .or_else(|| value_str(data, &["withdrawal_reason", "withdrawalReason"]))
        {
            lines.push(format!("Reason: {reason}"));
        }
        if let Some(by) = value_str(data, &["superseded_by", "supersededBy"]) {
            lines.push(format!("Superseded by: RFC {by}"));
        }
        lines.push(format!("Feature: {feature}"));
        lines.push(format!("File: {filename}"));
        return Some(lines.join("\n"));
    }

    // RFC status → grouped stage view with follow-up handles.
    if namespace == "rfc"
        && operation == "status"
        && let Some(stages) = data.get("stages").and_then(JsonValue::as_array)
    {
        let total = data.get("total").and_then(JsonValue::as_u64).unwrap_or(0);
        let mut lines = vec![format!("RFC Status: {total} total")];
        for group in stages {
            let Some(rfcs) = group.get("rfcs").and_then(JsonValue::as_array) else {
                continue;
            };
            if rfcs.is_empty() {
                continue;
            }
            let stage = group.get("stage").and_then(JsonValue::as_u64).unwrap_or(0);
            let stage_name =
                value_str(group, &["stage_name"]).unwrap_or_else(|| rfc_stage_name(stage));
            lines.push(String::new());
            lines.push(format!("Stage {stage}: {stage_name}"));
            for rfc in rfcs {
                let id = value_str(rfc, &["id"]).unwrap_or("?");
                let title = value_str(rfc, &["title"]).unwrap_or("Untitled RFC");
                let filename = value_str(rfc, &["filename"]).unwrap_or("(unknown path)");
                lines.push(format!("  RFC {id}: {title} ({filename})"));
            }
        }
        if let Some(lifecycle) = data.get("lifecycle").and_then(JsonValue::as_array) {
            for group in lifecycle {
                let Some(rfcs) = group.get("rfcs").and_then(JsonValue::as_array) else {
                    continue;
                };
                if rfcs.is_empty() {
                    continue;
                }
                let status_name = value_str(group, &["status_name", "statusName"])
                    .or_else(|| value_str(group, &["status"]).map(lifecycle_status_label))
                    .unwrap_or("Other");
                lines.push(String::new());
                lines.push(format!("{status_name} RFCs"));
                for rfc in rfcs {
                    let id = value_str(rfc, &["id"]).unwrap_or("?");
                    let title = value_str(rfc, &["title"]).unwrap_or("Untitled RFC");
                    let filename = value_str(rfc, &["filename"]).unwrap_or("(unknown path)");
                    lines.push(format!("  RFC {id}: {title} ({filename})"));
                }
            }
        }
        if let Some(repairs) = data.get("repairs").and_then(JsonValue::as_array)
            && !repairs.is_empty()
        {
            lines.push(String::new());
            lines.push("RFC identity repairs".to_string());
            for repair in repairs {
                let id = value_str(repair, &["id"]).unwrap_or("?");
                let current =
                    value_str(repair, &["current_path", "currentPath"]).unwrap_or("(unknown path)");
                let expected = value_str(repair, &["expected_path", "expectedPath"])
                    .unwrap_or("(unknown path)");
                lines.push(format!("  RFC {id}: {current} -> {expected}"));
                lines.push(format!("    Next: rfc repair {id}"));
            }
        }
        lines.push(String::new());
        lines.push("Show details: rfc show <id>".to_string());
        return Some(lines.join("\n"));
    }

    // RFC pipeline → active-phase RFC motion and promotion requirements.
    if namespace == "rfc" && operation == "pipeline" {
        let entries = data.get("entries").and_then(JsonValue::as_array)?;
        let phase_title = value_str(data, &["phaseTitle", "phase_title"]);
        if entries.is_empty() {
            return Some(match phase_title {
                Some(title) => format!("No RFCs linked to phase \"{title}\"."),
                None => "No RFCs linked to the active phase.".to_string(),
            });
        }

        let mut lines = Vec::new();
        match phase_title {
            Some(title) => lines.push(format!("RFC Pipeline for {title}")),
            None => lines.push("RFC Pipeline".to_string()),
        }

        for entry in entries {
            let id = value_str(entry, &["id"]).unwrap_or("?");
            let title = value_str(entry, &["title"]).unwrap_or("Untitled RFC");
            let role = value_str(entry, &["role"]).unwrap_or("linked");
            let current = entry
                .get("currentStage")
                .and_then(JsonValue::as_u64)
                .map_or_else(|| "?".to_string(), |stage| stage.to_string());
            let target = entry
                .get("targetStage")
                .and_then(JsonValue::as_u64)
                .map_or_else(|| "-".to_string(), |stage| stage.to_string());
            let path = value_str(entry, &["path"]).unwrap_or("(unknown path)");
            lines.push(format!(
                "  RFC {id}: {title} ({role}, stage {current} -> {target}, {path})"
            ));
            if let Some(requirement) = value_str(entry, &["promotionRequirement"]) {
                lines.push(format!("    Requirement: {requirement}"));
            }
        }
        lines.push(String::new());
        lines.push("Promote: rfc promote <id> --stage <int>".to_string());
        return Some(lines.join("\n"));
    }

    // Epoch list
    if namespace == "epoch"
        && operation == "list"
        && let Some(epochs) = data.get("epochs").and_then(JsonValue::as_array)
    {
        if epochs.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for e in epochs {
            let title = e.get("title").and_then(JsonValue::as_str).unwrap_or("");
            let status = e.get("status").and_then(JsonValue::as_str).unwrap_or("");
            let phase_count = e
                .get("phase_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            let icon = match status {
                "completed" => "✅",
                "in-progress" => "🔄",
                _ => "⏳",
            };
            lines.push(format!("{icon} {title} ({phase_count} phases)"));
        }
        return Some(lines.join("\n"));
    }

    // Epoch status → phase list
    if namespace == "epoch"
        && operation == "status"
        && let Some(phases) = data.get("phases").and_then(JsonValue::as_array)
    {
        let mut lines = Vec::new();
        let title = data
            .get("title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Epoch");
        let status = data
            .get("status")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        lines.push(format!("{title} ({status})"));
        lines.push(String::new());
        if !phases.is_empty() {
            for p in phases {
                let p_title = p.get("title").and_then(JsonValue::as_str).unwrap_or("?");
                let p_status = p.get("status").and_then(JsonValue::as_str).unwrap_or("");
                let icon = match p_status {
                    "completed" => "✅",
                    "in-progress" => "🔄",
                    _ => "⏳",
                };
                lines.push(format!("{icon} {p_title}"));
            }
        }
        return Some(lines.join("\n"));
    }

    // Phase status → goals + tasks + steering
    if namespace == "phase" && operation == "status" && data.get("phase_title").is_some() {
        let mut lines = Vec::new();
        let phase_title = data
            .get("phase_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("Phase");
        let epoch_title = data
            .get("epoch_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        lines.push(phase_title.to_string());
        if !epoch_title.is_empty() {
            lines.push(format!("Epoch: {epoch_title}"));
        }
        if let Some(rfcs) = data.get("rfcs").and_then(JsonValue::as_array)
            && !rfcs.is_empty()
        {
            let ids = rfcs
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>();
            if !ids.is_empty() {
                lines.push(format!("RFCs: {}", ids.join(", ")));
            }
        }
        lines.push(String::new());

        // Goals
        if let Some(goals) = data.get("goals").and_then(JsonValue::as_array)
            && !goals.is_empty()
        {
            lines.push("Goals:".to_string());
            for g in goals {
                let label = g.get("name").and_then(JsonValue::as_str).unwrap_or("?");
                let status = g.get("status").and_then(JsonValue::as_str).unwrap_or("");
                let icon = match status {
                    "completed" => "✅",
                    "abandoned" => "⛔",
                    "in-progress" => "🔄",
                    _ => "⏳",
                };
                lines.push(format!("  {icon} {label}"));
            }
            lines.push(String::new());
        }

        // Steering next actions
        if let Some(steering) = data.get("steering")
            && let Some(actions) = steering.get("next_actions").and_then(JsonValue::as_array)
            && !actions.is_empty()
        {
            lines.push("Next actions:".to_string());
            for a in actions {
                let label = a.get("label").and_then(JsonValue::as_str).unwrap_or("?");
                let command = a.get("command").and_then(JsonValue::as_str).unwrap_or("");
                lines.push(format!("  → {label}: {command}"));
            }
        }

        return Some(lines.join("\n"));
    }

    // Phase list → phase handles for future-phase planning.
    if namespace == "phase"
        && operation == "list"
        && let Some(phases) = data.get("phases").and_then(JsonValue::as_array)
    {
        if phases.is_empty() {
            return Some("No phases found.".to_string());
        }

        let epoch_title = data
            .get("epoch_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("active epoch");
        let mut lines = vec![format!("Phases in {epoch_title}:")];
        for phase in phases {
            let title = value_str(phase, &["title"]).unwrap_or("Untitled phase");
            let status = value_str(phase, &["status"]).unwrap_or("unknown");
            let position = phase
                .get("position")
                .and_then(JsonValue::as_u64)
                .map_or_else(|| "?".to_string(), |position| position.to_string());
            let goal_count = phase
                .get("goal_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            lines.push(format!(
                "{position}. {title} ({status}, {goal_count} goals)"
            ));
        }
        return Some(lines.join("\n"));
    }

    // Plan read/snapshot → compact epoch and phase handles.
    if namespace == "plan"
        && matches!(operation, "read" | "snapshot")
        && let Some(text) = render_plan_tree(data)
    {
        return Some(text);
    }

    // Phase read-details → compact phase, goal, and task handles.
    if namespace == "phase" && operation == "read-details" {
        if data.is_null() {
            return Some("No phase details found. Run: plan read".to_string());
        }
        if let Some(text) = render_phase_details(data) {
            return Some(text);
        }
    }

    // Phase read-goals → compact goal handles.
    if namespace == "phase"
        && operation == "read-goals"
        && let Some(goals) = data.as_array()
    {
        return Some(render_goals(goals));
    }

    // Phase read-tasks → compact task handles.
    if namespace == "phase"
        && operation == "read-tasks"
        && let Some(tasks) = data.as_array()
    {
        return Some(render_tasks(tasks));
    }

    // Phase execution.tasks → compact paginated task handles.
    if namespace == "phase"
        && operation == "execution.tasks"
        && let Some(items) = data.get("items").and_then(JsonValue::as_array)
    {
        let mut lines = vec!["Phase execution tasks:".to_string()];
        lines.extend(render_tasks(items).lines().map(str::to_string));
        if data.get("has_more").and_then(JsonValue::as_bool) == Some(true)
            && let Some(cursor) = data.get("next_cursor").and_then(JsonValue::as_str)
        {
            lines.push(format!(
                "Next page: phase execution.tasks --cursor {cursor}"
            ));
        }
        return Some(lines.join("\n"));
    }

    // Project resolve → canonical project and state/runtime paths.
    if namespace == "project"
        && operation == "resolve"
        && let Some(text) = render_project_resolve(data)
    {
        return Some(text);
    }

    // map --next → one suggested action in agent-facing command form.
    if namespace.is_empty()
        && operation == "map"
        && let Some(text) = render_map(data)
    {
        return Some(text);
    }

    // Root status → project overview
    if namespace.is_empty() && operation == "status" {
        let mut lines = Vec::new();
        let phase = data
            .get("phase_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("No active phase");
        let epoch = data
            .get("epoch_title")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let mode = data
            .get("progress_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let goals_done = data
            .get("completed_goals")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
        let goals_pending = data
            .get("pending_goals")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);

        lines.push(phase.to_string());
        if !epoch.is_empty() {
            lines.push(format!("Epoch: {epoch}"));
        }
        if !mode.is_empty() {
            lines.push(format!("Mode: {mode}"));
        }
        lines.push(format!(
            "Goals: {goals_done} completed, {goals_pending} pending"
        ));

        // Git status
        if let Some(git) = data.get("git_summary") {
            let added = git.get("added").and_then(JsonValue::as_u64).unwrap_or(0);
            let modified = git.get("modified").and_then(JsonValue::as_u64).unwrap_or(0);
            let deleted = git.get("deleted").and_then(JsonValue::as_u64).unwrap_or(0);
            let untracked = git
                .get("untracked")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            if added + modified + deleted + untracked > 0 {
                lines.push(format!(
                    "Git: {added} added, {modified} modified, {deleted} deleted, {untracked} untracked"
                ));
            } else {
                lines.push("Git: clean".to_string());
            }
        }

        if let Some(steering) = data.get("steering")
            && let Some(digests) = steering
                .get("completion_digests")
                .and_then(JsonValue::as_array)
            && !digests.is_empty()
        {
            lines.push(String::new());
            lines.push("Completed outcomes to review:".to_string());
            for digest in digests {
                let entity_type = digest
                    .get("entity_type")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("entity");
                let entity_id = digest
                    .get("entity_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("?");
                if let Some(claims) = digest.get("claims").and_then(JsonValue::as_array) {
                    for claim in claims {
                        let subject = claim
                            .get("subject")
                            .and_then(JsonValue::as_str)
                            .unwrap_or("Completion claim");
                        lines.push(format!("  • {entity_type} {entity_id}: {subject}"));
                        if let Some(body) = claim.get("body").and_then(JsonValue::as_str)
                            && !body.is_empty()
                        {
                            lines.push(format!("    {body}"));
                        }
                    }
                }
            }
        }

        // Steering next actions
        if let Some(steering) = data.get("steering")
            && let Some(actions) = steering.get("next_actions").and_then(JsonValue::as_array)
            && !actions.is_empty()
        {
            lines.push(String::new());
            lines.push("Next actions:".to_string());
            for a in actions {
                let label = a.get("label").and_then(JsonValue::as_str).unwrap_or("?");
                let command = a.get("command").and_then(JsonValue::as_str).unwrap_or("");
                lines.push(format!("  → {label}: {command}"));
            }
        }

        return Some(lines.join("\n"));
    }

    // Idea list → markdown list
    if namespace == "idea"
        && operation == "list"
        && let Some(ideas) = data.get("ideas").and_then(JsonValue::as_array)
    {
        if ideas.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for idea in ideas {
            let title = idea.get("title").and_then(JsonValue::as_str).unwrap_or("?");
            let status = idea.get("status").and_then(JsonValue::as_str).unwrap_or("");
            let icon = match status {
                "archived" => "📦",
                "promoted" => "🚀",
                _ => "💡",
            };
            lines.push(format!("{icon} {title}"));
        }
        return Some(lines.join("\n"));
    }

    // Inbox list → markdown list
    if namespace == "inbox"
        && operation == "list"
        && let Some(items) = data.get("items").and_then(JsonValue::as_array)
    {
        if items.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        for item in items {
            let subject = item
                .get("subject")
                .and_then(JsonValue::as_str)
                .unwrap_or("?");
            let status = item.get("status").and_then(JsonValue::as_str).unwrap_or("");
            let category = item
                .get("category")
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            let icon = match status {
                "resolved" => "✅",
                "acknowledged" => "👁",
                _ => "📥",
            };
            let cat = if category.is_empty() {
                String::new()
            } else {
                format!(" [{category}]")
            };
            lines.push(format!("{icon}{cat} {subject}"));
        }
        return Some(lines.join("\n"));
    }

    // Phase history → completed phases list
    if namespace == "phase"
        && operation == "history"
        && let Some(phases) = data.get("phases").and_then(JsonValue::as_array)
    {
        if phases.is_empty() {
            return Some("No completed phases.".to_string());
        }
        let mut lines = Vec::new();
        // Group by epoch
        let mut current_epoch = String::new();
        for p in phases {
            let epoch = p
                .get("epoch_title")
                .and_then(JsonValue::as_str)
                .unwrap_or("Unknown Epoch");
            if epoch != current_epoch {
                if !current_epoch.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("{epoch}:"));
                current_epoch = epoch.to_string();
            }
            let title = p.get("title").and_then(JsonValue::as_str).unwrap_or("?");
            lines.push(format!("  ✅ {title}"));
        }
        return Some(lines.join("\n"));
    }

    // Task complete / task start / simple ok — summary is enough
    if data.get("ok").and_then(JsonValue::as_bool) == Some(true) {
        // Include message if present
        if let Some(msg) = data.get("message").and_then(JsonValue::as_str)
            && !msg.is_empty()
        {
            return Some(msg.to_string());
        }
        return None;
    }

    // For anything else with substantial data, don't generate a body
    // (the model gets the structured data via the result field)
    None
}

fn value_str<'a>(value: &'a JsonValue, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(JsonValue::as_str))
}

fn render_sidecar_repo_status(data: &JsonValue) -> String {
    let state = if data
        .get("repo_clean")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        "clean"
    } else {
        "dirty"
    };
    let branch = value_str(data, &["branch"]).unwrap_or("<detached>");
    let sidecar_root = value_str(data, &["sidecar_root", "sidecarRoot"]).unwrap_or("(unknown)");
    let mut lines = vec![format!(
        "Sidecar repo {state} on {branch} at {sidecar_root}"
    )];

    if let Some(ownership) = data.get("ownership")
        && ownership
            .get("ok")
            .and_then(JsonValue::as_bool)
            .is_some_and(|ok| !ok)
    {
        let issue = value_str(ownership, &["issue"])
            .or_else(|| value_str(ownership, &["state"]))
            .unwrap_or("blocked");
        let owner = ownership
            .get("owner_pid")
            .and_then(JsonValue::as_u64)
            .map(|pid| format!(" {pid}"))
            .unwrap_or_default();
        let line = if let Some(details) =
            issue.strip_prefix("sidecar write ownership marker is invalid: ")
        {
            format!("Ownership marker invalid: {details}")
        } else if let Some(details) =
            issue.strip_prefix("failed to read sidecar write ownership marker: ")
        {
            format!("Ownership marker unreadable: {details}")
        } else if issue.contains("active runtime") || issue.contains("live runtime") {
            format!("Ownership blocked by active runtime{owner}: {issue}")
        } else {
            format!("Ownership blocked{owner}: {issue}")
        };
        let mut line = line;
        if let Some(workspace_root) = value_str(ownership, &["owner_workspace_root"]) {
            line.push_str(&format!(" ({workspace_root})"));
        }
        lines.push(line);
    }

    if let Some(issue) = value_str(data, &["issue"]) {
        lines.push(format!("Issue: {issue}"));
    }
    if let Some(actions) = data.get("next_actions").and_then(JsonValue::as_array)
        && !actions.is_empty()
    {
        lines.push("Next actions:".to_string());
        for action in actions {
            if let Some(command) = value_str(action, &["command"]) {
                lines.push(format!("  -> {command}"));
            }
        }
    }

    if let Some(debts) = data
        .get("foreign_checkpoint_debt")
        .and_then(JsonValue::as_array)
        && !debts.is_empty()
    {
        lines.push("Foreign checkpoint debt:".to_string());
        for debt in debts {
            let project = value_str(debt, &["project"]).unwrap_or("?");
            let file_count = debt
                .get("files")
                .and_then(JsonValue::as_array)
                .map_or(0, Vec::len);
            lines.push(format!("  - {project}: {file_count} file(s)"));
            if let Some(actions) = debt.get("next_actions").and_then(JsonValue::as_array) {
                for action in actions {
                    if let Some(command) = value_str(action, &["command"]) {
                        lines.push(format!("    -> {command}"));
                    }
                }
            }
        }
    }

    lines.join("\n")
}

const fn rfc_stage_name(stage: u64) -> &'static str {
    match stage {
        0 => "Idea",
        1 => "Proposal",
        2 => "Draft",
        3 => "Candidate",
        4 => "Stable",
        _ => "Unknown",
    }
}

fn lifecycle_status_label(status: &str) -> &str {
    match status {
        "archived" => "Archived",
        "withdrawn" => "Withdrawn",
        "superseded" => "Superseded",
        "active" => "Active",
        _ => "Other",
    }
}

fn render_plan_tree(data: &JsonValue) -> Option<String> {
    let plan = data.get("plan").unwrap_or(data);
    let epochs = plan.get("epochs").and_then(JsonValue::as_array)?;
    let mut lines = Vec::new();
    lines.push(format!("Plan: {} epochs", epochs.len()));

    for epoch in epochs {
        let epoch_title = value_str(epoch, &["title"]).unwrap_or("Untitled epoch");
        let epoch_status = value_str(epoch, &["status"]).unwrap_or("unknown");
        lines.push(format!("Epoch: {epoch_title} ({epoch_status})"));

        if let Some(phases) = epoch.get("phases").and_then(JsonValue::as_array) {
            for phase in phases {
                let phase_title = value_str(phase, &["title"]).unwrap_or("Untitled phase");
                let phase_status = value_str(phase, &["status"]).unwrap_or("unknown");
                let goals = phase
                    .get("goals")
                    .and_then(JsonValue::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                lines.push(format!(
                    "  Phase: {phase_title} ({phase_status}, {} goals)",
                    goals.len()
                ));
                for goal in goals {
                    let goal_id = value_str(goal, &["id", "goal_id"]).unwrap_or("?");
                    let goal_title = value_str(goal, &["title", "label", "name"]).unwrap_or("");
                    let goal_status = value_str(goal, &["status"]).unwrap_or("unknown");
                    if goal_title.is_empty() {
                        lines.push(format!("    Goal {goal_id} ({goal_status})"));
                    } else {
                        lines.push(format!("    Goal {goal_id}: {goal_title} ({goal_status})"));
                    }
                }
            }
        }
    }

    Some(lines.join("\n"))
}

fn render_phase_details(data: &JsonValue) -> Option<String> {
    let phase_title = value_str(data, &["phaseTitle", "phase_title"]).unwrap_or("Untitled phase");
    let epoch_title = value_str(data, &["epochTitle", "epoch_title"]);
    let mut lines = Vec::new();

    match epoch_title {
        Some(epoch) => lines.push(format!("Phase: {phase_title} (epoch: {epoch})")),
        None => lines.push(format!("Phase: {phase_title}")),
    }

    if let Some(rfcs) = data.get("rfcs").and_then(JsonValue::as_array)
        && !rfcs.is_empty()
    {
        let ids = rfcs
            .iter()
            .filter_map(JsonValue::as_str)
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            lines.push(format!("RFCs: {}", ids.join(", ")));
        }
    }

    if let Some(goals) = data.get("goals").and_then(JsonValue::as_array) {
        lines.push(String::new());
        lines.push("Goals:".to_string());
        lines.extend(
            render_goals_with_tasks(goals)
                .lines()
                .map(|line| format!("  {line}")),
        );
    }

    if let Some(progress) = data.get("progress") {
        let goals_done = progress.get("goalsCompleted").and_then(JsonValue::as_u64);
        let goals_total = progress.get("goalsTotal").and_then(JsonValue::as_u64);
        let tasks_done = progress.get("tasksCompleted").and_then(JsonValue::as_u64);
        let tasks_total = progress.get("tasksTotal").and_then(JsonValue::as_u64);
        if let (Some(done), Some(total)) = (goals_done, goals_total) {
            lines.push(format!("Progress: {done}/{total} goals"));
        }
        if let (Some(done), Some(total)) = (tasks_done, tasks_total) {
            lines.push(format!("Progress: {done}/{total} tasks"));
        }
    }

    Some(lines.join("\n"))
}

fn render_goals(goals: &[JsonValue]) -> String {
    if goals.is_empty() {
        return "No goals.".to_string();
    }

    goals
        .iter()
        .map(|goal| {
            let id = value_str(goal, &["id", "goal_id"]).unwrap_or("?");
            let title = value_str(goal, &["title", "label", "name"]).unwrap_or("");
            let status = value_str(goal, &["status"]).unwrap_or("unknown");
            if title.is_empty() {
                format!("{id} ({status})")
            } else {
                format!("{id}: {title} ({status})")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_goals_with_tasks(goals: &[JsonValue]) -> String {
    if goals.is_empty() {
        return "No goals.".to_string();
    }

    let mut lines = Vec::new();
    for goal in goals {
        let id = value_str(goal, &["id", "goal_id"]).unwrap_or("?");
        let title = value_str(goal, &["title", "label", "name"]).unwrap_or("");
        let status = value_str(goal, &["status"]).unwrap_or("unknown");
        if title.is_empty() {
            lines.push(format!("{id} ({status})"));
        } else {
            lines.push(format!("{id}: {title} ({status})"));
        }

        if let Some(tasks) = goal.get("tasks").and_then(JsonValue::as_array)
            && !tasks.is_empty()
        {
            for task in tasks {
                let task_id = value_str(task, &["id", "task_id"]).unwrap_or("?");
                let task_title = value_str(task, &["title", "label"]).unwrap_or("");
                let task_status = value_str(task, &["status"]).unwrap_or("unknown");
                if task_title.is_empty() {
                    lines.push(format!("  Task {task_id} ({task_status})"));
                } else {
                    lines.push(format!("  Task {task_id}: {task_title} ({task_status})"));
                }
            }
        }
    }

    lines.join("\n")
}

fn render_tasks(tasks: &[JsonValue]) -> String {
    if tasks.is_empty() {
        return "No tasks.".to_string();
    }

    tasks
        .iter()
        .map(|task| {
            let id = value_str(task, &["id", "task_id"]).unwrap_or("?");
            let title = value_str(task, &["title", "label"]).unwrap_or("");
            let status = value_str(task, &["status"]).unwrap_or("unknown");
            let goal = value_str(task, &["goalId", "goal_id"]);
            let scope = goal.map_or(String::new(), |goal| format!(" goal:{goal}"));
            if title.is_empty() {
                format!("{id}{scope} ({status})")
            } else {
                format!("{id}{scope}: {title} ({status})")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_project_resolve(data: &JsonValue) -> Option<String> {
    let project = data.get("project")?;
    let paths = data.get("paths")?;
    let id = value_str(project, &["id"]).unwrap_or("?");
    let policy = value_str(project, &["policy"]).unwrap_or("unknown");
    let mut lines = vec![format!("Project {id} ({policy})")];

    if let Some(sidecar_key) = value_str(project, &["sidecar_key"]) {
        lines.push(format!("sidecar_key: {sidecar_key}"));
    }

    for (label, key) in [
        ("state_root", "state_root"),
        ("db_path", "db_path"),
        ("runtime_dir", "runtime_dir"),
        ("socket_path", "socket_path"),
        ("pid_path", "pid_path"),
        ("sidecar_manifest_path", "sidecar_manifest_path"),
        ("sidecar_projection_dir", "sidecar_projection_dir"),
    ] {
        if let Some(value) = value_str(paths, &[key]) {
            lines.push(format!("{label}: {value}"));
        }
    }

    Some(lines.join("\n"))
}

fn render_map(data: &JsonValue) -> Option<String> {
    if data.get("preconditions").is_some() || data.get("effects").is_some() {
        let command = value_str(data, &["command"]).unwrap_or("unknown command");
        let mut lines = vec![format!("Why: {}", exo_run_command(command))];

        if let Some(suggested) = data.get("suggested")
            && let Some(label) = value_str(suggested, &["label"])
        {
            lines.push(format!("Suggested action: {label}"));
        }

        if let Some(preconditions) = data.get("preconditions").and_then(JsonValue::as_array)
            && !preconditions.is_empty()
        {
            lines.push("Preconditions:".to_string());
            for item in preconditions {
                if let Some(text) = item.as_str() {
                    lines.push(format!("  {text}"));
                }
            }
        }

        if let Some(effects) = data.get("effects").and_then(JsonValue::as_array)
            && !effects.is_empty()
        {
            lines.push("Effects:".to_string());
            for item in effects {
                if let Some(text) = item.as_str() {
                    lines.push(format!("  {text}"));
                }
            }
        }

        return Some(lines.join("\n"));
    }

    if let Some(command) = value_str(data, &["command"]) {
        let label = value_str(data, &["label"]).unwrap_or("Next action");
        let rationale = value_str(data, &["rationale"]);
        let mut lines = vec![format!("{label}: {}", exo_run_command(command))];
        if let Some(rationale) = rationale {
            lines.push(rationale.to_string());
        }
        return Some(lines.join("\n"));
    }

    let mut lines = Vec::new();
    for (heading, key) in [
        ("Next actions:", "next_actions"),
        ("Repair actions:", "repair_actions"),
    ] {
        let Some(actions) = data.get(key).and_then(JsonValue::as_array) else {
            continue;
        };
        if actions.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(heading.to_string());
        for action in actions {
            let label = value_str(action, &["label"]).unwrap_or("Action");
            let command = value_str(action, &["command"]);
            match command {
                Some(command) => lines.push(format!("  {label}: {}", exo_run_command(command))),
                None => lines.push(format!("  {label}")),
            }
        }
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn exo_run_command(command: &str) -> &str {
    command.strip_prefix("exo ").unwrap_or(command)
}

pub fn handle_request(workspace_root: &Path, request: RequestEnvelope) -> ResponseEnvelope {
    let project = Project::resolve(workspace_root).ok();
    handle_request_with_project(workspace_root, project.as_ref(), request)
}

pub fn handle_request_with_project(
    workspace_root: &Path,
    project: Option<&Project>,
    request: RequestEnvelope,
) -> ResponseEnvelope {
    handle_request_with_project_and_diagnostics(
        workspace_root,
        project,
        request,
        &DaemonDiagnostics::disabled(),
    )
}

pub fn handle_request_with_project_and_diagnostics(
    workspace_root: &Path,
    project: Option<&Project>,
    request: RequestEnvelope,
    diagnostics: &DaemonDiagnostics,
) -> ResponseEnvelope {
    handle_request_with_project_and_diagnostics_in_runtime(
        workspace_root,
        project,
        request,
        diagnostics,
        HandlerRuntime::External,
    )
}

pub fn handle_request_with_project_and_diagnostics_as_writer(
    workspace_root: &Path,
    project: Option<&Project>,
    request: RequestEnvelope,
    diagnostics: &DaemonDiagnostics,
) -> ResponseEnvelope {
    handle_request_with_project_and_diagnostics_in_runtime(
        workspace_root,
        project,
        request,
        diagnostics,
        HandlerRuntime::SidecarWriter,
    )
}

fn handle_request_with_project_and_diagnostics_in_runtime(
    workspace_root: &Path,
    project: Option<&Project>,
    request: RequestEnvelope,
    diagnostics: &DaemonDiagnostics,
    runtime: HandlerRuntime,
) -> ResponseEnvelope {
    let request_id = request.id.clone();
    let op_path = request_op_path(&request);
    let start = Instant::now();
    diagnostics.record(
        "request.handler_start",
        json!({ "request_id": request_id, "op_path": op_path }),
    );

    let response = handle_request_inner(workspace_root, project, request, diagnostics, runtime);

    diagnostics.record(
        "request.handler_end",
        json!({
            "request_id": response.id,
            "op_path": op_path,
            "status": response_status(&response),
            "effect": response.effect.map(effect_name),
            "elapsed_ms": elapsed_ms(start.elapsed()),
        }),
    );

    response
}

fn handle_request_inner(
    workspace_root: &Path,
    project: Option<&Project>,
    request: RequestEnvelope,
    diagnostics: &DaemonDiagnostics,
    runtime: HandlerRuntime,
) -> ResponseEnvelope {
    if request.protocol_version != protocol::PROTOCOL_VERSION {
        return error(
            request.id,
            ErrorCode::VersionMismatch,
            format!(
                "Unsupported protocol_version {} (expected {})",
                request.protocol_version,
                protocol::PROTOCOL_VERSION
            ),
            Some(json!({
                "expected": protocol::PROTOCOL_VERSION,
                "received": request.protocol_version
            })),
            steer_help_root(),
        );
    }

    let spec = NewCommandSpec::from_registry(&default_registry());

    match request.op {
        Op::Help(HelpParams { address }) => handle_help(request.id, address, &spec),
        Op::List(params) => handle_list(workspace_root, project, request.id, &params, &spec),
        Op::Call(params) => handle_call(
            workspace_root,
            project,
            request.id,
            &params,
            request.auth,
            request.agent_id,
            request.workflow_confirmation,
            diagnostics,
            runtime,
        ),
        Op::Preview(params) => handle_preview(request.id, &params),
    }
}

fn handle_help(id: String, address: Address, spec: &NewCommandSpec) -> ResponseEnvelope {
    if let Some(result) = help_for_address(spec, &address) {
        ok(
            id,
            serde_json::to_value(result).unwrap_or_else(|_| json!({})),
        )
    } else {
        error(
            id,
            ErrorCode::UnknownAddress,
            "Unknown address".to_string(),
            Some(serde_json::to_value(address).unwrap_or_else(|_| json!({}))),
            steer_help_root(),
        )
    }
}

/// Handle `Op::Preview`: parse the address, generate a display title, return without executing.
///
/// This is the pre-execution path used by `prepareInvocation` in the VS Code LM tool.
/// It returns only a `PreviewDisplay` (invocation message + optional confirmation),
/// not the full result/steering that `Op::Call` produces.
fn handle_preview(id: String, params: &CallParams) -> ResponseEnvelope {
    // Extract namespace and operation, handling compound operations
    let compound_op_storage: String;

    let (namespace, operation) = match &params.address {
        Address::Operation { path } => {
            if path.len() == 1 {
                ("", path[0].as_str())
            } else if path.len() == 2 {
                (path[0].as_str(), path[1].as_str())
            } else if path.len() == 3 {
                compound_op_storage = format!("{}.{}", path[1], path[2]);
                (path[0].as_str(), compound_op_storage.as_str())
            } else {
                return error(
                    id,
                    ErrorCode::InvalidInput,
                    "Invalid address path length".to_string(),
                    Some(json!({ "path": path })),
                    steer_help_root(),
                );
            }
        }
        _ => {
            return error(
                id,
                ErrorCode::InvalidInput,
                "Expected operation address for preview".to_string(),
                None,
                steer_help_root(),
            );
        }
    };

    // Generate the invocation message using tier-based templates.
    let invocation_message = generate_preview_message(namespace, operation, &params.input);

    // Generate past-tense variant for post-completion display (proposed API).
    let past_tense_message = Some(generate_past_tense_message(
        namespace,
        operation,
        &params.input,
    ));

    // Check if this command needs a confirmation dialog (destructive ops).
    let confirmation = confirmation_for_command(namespace, operation, &invocation_message);

    ResponseEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id,
        status: Status::Ok,
        result: None,
        error: None,
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: Some(PreviewDisplay {
            invocation_message,
            past_tense_message,
            confirmation,
        }),
        effect: None,
        trace: None,
    }
}

const fn daemon_writer_ensure_outcome(state: DaemonEnsureState) -> &'static str {
    match state {
        DaemonEnsureState::ConnectedExisting | DaemonEnsureState::WaitedForLock => {
            "writer_available"
        }
        DaemonEnsureState::Spawned => "writer_started",
    }
}

fn should_use_daemon_writer_lane(
    runtime: HandlerRuntime,
    project: Option<&Project>,
    effect: Effect,
) -> bool {
    runtime == HandlerRuntime::External
        && project.is_some()
        && matches!(effect, Effect::Write | Effect::Exec)
}

#[allow(clippy::too_many_arguments)]
fn call_ensured_daemon_writer(
    workspace_root: &Path,
    project: Option<&Project>,
    id: String,
    params: &CallParams,
    auth: Option<&Auth>,
    agent_id: Option<&String>,
    workflow_confirmation: Option<&crate::api::protocol::WorkflowConfirmationInput>,
    diagnostics: &DaemonDiagnostics,
) -> ResponseEnvelope {
    diagnostics.record(
        "request.daemon_writer_ensure_start",
        json!({ "request_id": id }),
    );

    let writer_request = RequestEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id: id.clone(),
        op: Op::Call(params.clone()),
        auth: auth.cloned(),
        workflow_confirmation: workflow_confirmation.cloned(),
        agent_id: agent_id.cloned(),
    };
    let writer_response = match project {
        Some(project) => crate::daemon_client::send_request_with_project_recovery_report(
            workspace_root,
            project,
            &writer_request,
        ),
        None => {
            crate::daemon_client::send_request_with_recovery_report(workspace_root, &writer_request)
        }
    };

    let (response, report) = match writer_response {
        Ok(outcome) => outcome,
        Err(connect_error) => {
            diagnostics.record(
                "request.daemon_writer_ensure_end",
                json!({
                    "request_id": id,
                    "status": "error",
                    "outcome": "writer_incompatible",
                    "error": connect_error.to_string(),
                }),
            );
            return error(
                id,
                ErrorCode::PreconditionFailed,
                format!("daemon writer ensure failed: {connect_error}"),
                Some(json!({
                    "kind": "daemon.writer_ensure",
                    "outcome": "writer_incompatible",
                    "error": connect_error.to_string(),
                })),
                steer_help_root(),
            );
        }
    };

    let outcome = daemon_writer_ensure_outcome(report.state);
    diagnostics.record(
        "request.daemon_writer_ensure_end",
        json!({
            "request_id": id,
            "status": "ok",
            "outcome": outcome,
            "report": report,
        }),
    );
    response
}

#[allow(clippy::too_many_arguments)]
fn handle_call(
    workspace_root: &Path,
    project: Option<&Project>,
    id: String,
    params: &crate::api::protocol::CallParams,
    auth: Option<Auth>,
    agent_id: Option<String>,
    workflow_confirmation: Option<crate::api::protocol::WorkflowConfirmationInput>,
    diagnostics: &DaemonDiagnostics,
    runtime: HandlerRuntime,
) -> ResponseEnvelope {
    // Extract namespace and operation from address
    let (namespace, operation) = match &params.address {
        crate::api::protocol::Address::Operation { path } => {
            if path.len() == 1 {
                ("", path[0].as_str())
            } else if path.len() == 2 {
                (path[0].as_str(), path[1].as_str())
            } else if path.len() == 3 {
                // Handle compound operations like ["docs", "links", "check"]
                // The operation is "links.check"
                let compound_op = format!("{}.{}", path[1], path[2]);
                return handle_call_with_namespace_operation(
                    workspace_root,
                    project,
                    id,
                    params,
                    auth,
                    agent_id,
                    workflow_confirmation,
                    &path[0],
                    &compound_op,
                    diagnostics,
                    runtime,
                );
            } else {
                return error(
                    id,
                    ErrorCode::InvalidInput,
                    "Invalid address path length".to_string(),
                    Some(json!({ "path": path })),
                    steer_help_root(),
                );
            }
        }
        _ => {
            return error(
                id,
                ErrorCode::InvalidInput,
                "Expected operation address".to_string(),
                None,
                steer_help_root(),
            );
        }
    };

    handle_call_with_namespace_operation(
        workspace_root,
        project,
        id,
        params,
        auth,
        agent_id,
        workflow_confirmation,
        namespace,
        operation,
        diagnostics,
        runtime,
    )
}

#[allow(clippy::too_many_arguments)]
fn handle_call_with_namespace_operation(
    workspace_root: &Path,
    project: Option<&Project>,
    id: String,
    params: &crate::api::protocol::CallParams,
    auth: Option<Auth>,
    agent_id: Option<String>,
    workflow_confirmation: Option<crate::api::protocol::WorkflowConfirmationInput>,
    namespace: &str,
    operation: &str,
    diagnostics: &DaemonDiagnostics,
    runtime: HandlerRuntime,
) -> ResponseEnvelope {
    // Build the new CommandSpec from registry for Invocation::from_json
    let registry = default_registry();
    let new_spec = NewCommandSpec::from_registry(&registry);

    // Route through new CommandSpec (primary path). Surface diagnostic errors.
    let route_start = Instant::now();
    match Invocation::from_json(&params.input, namespace, operation, &new_spec) {
        Ok(invocation) => {
            diagnostics.record(
                "request.route_end",
                json!({
                    "namespace": namespace,
                    "operation": operation,
                    "status": "ok",
                    "elapsed_ms": elapsed_ms(route_start.elapsed()),
                }),
            );
            let build_start = Instant::now();
            let cmd_box = match build_command_from_invocation(&invocation, workspace_root) {
                Ok(Some(cmd_box)) => {
                    diagnostics.record(
                        "request.build_end",
                        json!({
                            "namespace": namespace,
                            "operation": operation,
                            "status": "ok",
                            "elapsed_ms": elapsed_ms(build_start.elapsed()),
                        }),
                    );
                    cmd_box
                }
                Ok(None) => {
                    diagnostics.record(
                        "request.build_end",
                        json!({
                            "namespace": namespace,
                            "operation": operation,
                            "status": "unavailable",
                            "elapsed_ms": elapsed_ms(build_start.elapsed()),
                        }),
                    );
                    return error(
                        id,
                        ErrorCode::UnknownCommand,
                        format!(
                            "Operation {namespace}.{operation} is not available via machine channel"
                        ),
                        Some(json!({
                            "namespace": namespace,
                            "operation": operation,
                            "address": params.address,
                            "mutation_performed": false,
                            "safe_next": if namespace.is_empty() {
                                "exo help".to_string()
                            } else {
                                format!("exo help {namespace}")
                            }
                        })),
                        if namespace.is_empty() {
                            steer_help_root()
                        } else {
                            steer_help_namespace(namespace)
                        },
                    );
                }
                Err(error) => {
                    diagnostics.record(
                        "request.build_end",
                        json!({
                            "namespace": namespace,
                            "operation": operation,
                            "status": "error",
                            "elapsed_ms": elapsed_ms(build_start.elapsed()),
                        }),
                    );
                    return command_construction_error_to_response(id, error);
                }
            };

            let effect = cmd_box.effect();
            if should_use_daemon_writer_lane(runtime, project, effect) {
                return call_ensured_daemon_writer(
                    workspace_root,
                    project,
                    id,
                    params,
                    auth.as_ref(),
                    agent_id.as_ref(),
                    workflow_confirmation.as_ref(),
                    diagnostics,
                );
            }

            let expected_ticket = ticket_for_exec_call(&params.address, &params.input);
            let agent_id_for_log = agent_id.as_deref().map(String::from);
            let transport = MachineChannelTransport {
                workspace_root,
                project,
                request_id: id.clone(),
                auth,
                expected_ticket: Some(expected_ticket),
                agent_id,
                workflow_confirmation,
            };

            diagnostics.record(
                "request.sidecar_post_write_preflight_start",
                json!({ "namespace": namespace, "operation": operation, "effect": effect_name(effect) }),
            );
            if let Err(error) = crate::post_write::preflight_sidecar_post_write(
                project, namespace, operation, effect,
            ) {
                diagnostics.record(
                    "request.sidecar_post_write_preflight_end",
                    json!({
                        "namespace": namespace,
                        "operation": operation,
                        "status": "error",
                        "error": error.to_string(),
                    }),
                );
                return command_construction_error_to_response(id, error);
            }
            diagnostics.record(
                "request.sidecar_post_write_preflight_end",
                json!({ "namespace": namespace, "operation": operation, "status": "ok" }),
            );

            let start = Instant::now();
            diagnostics.record(
                "request.invoke_start",
                json!({ "namespace": namespace, "operation": operation }),
            );
            match invoke_command_box_json(&cmd_box, &transport) {
                Ok(invoke_result) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    diagnostics.record(
                        "request.invoke_end",
                        json!({
                            "namespace": namespace,
                            "operation": operation,
                            "status": "ok",
                            "effect": effect_name(invoke_result.effect),
                            "elapsed_ms": duration_ms,
                        }),
                    );

                    let display = make_display(namespace, operation, &params.input, &invoke_result);
                    let effect = invoke_result.effect;
                    let mut should_auto_persist =
                        crate::post_write::should_auto_persist_after_success(
                            effect, namespace, operation, project,
                        );

                    if crate::post_write::should_log_command_event(namespace, operation) {
                        // Log command event (RFC 10183)
                        let event_log_start = Instant::now();
                        let summary = display.as_ref().map_or_else(
                            || {
                                if namespace.is_empty() {
                                    operation
                                } else {
                                    namespace
                                }
                            },
                            |d| d.summary.as_str(),
                        );
                        let event_logged = log_command_event(
                            workspace_root,
                            project,
                            agent_id_for_log.as_deref(),
                            namespace,
                            operation,
                            &params.input,
                            &invoke_result.data,
                            effect,
                            duration_ms,
                            summary,
                        );
                        should_auto_persist |=
                            crate::post_write::should_auto_persist_after_command_event(
                                event_logged,
                                effect,
                                namespace,
                                operation,
                                project,
                            );
                        diagnostics.record(
                            "request.event_log_end",
                            json!({
                                "namespace": namespace,
                                "operation": operation,
                                "logged": event_logged,
                                "elapsed_ms": elapsed_ms(event_log_start.elapsed()),
                            }),
                        );
                    }

                    let post_write_report =
                        if crate::post_write::should_write_sql_dump(namespace, operation, effect)
                            || should_auto_persist
                        {
                            let report =
                                crate::post_write::with_sidecar_runtime_lock(project, || {
                                    let persistence_start = Instant::now();
                                    diagnostics.record(
                                        "request.post_write_persistence_start",
                                        json!({ "namespace": namespace, "operation": operation }),
                                    );
                                    let report = crate::post_write::persist_after_success(
                                        workspace_root,
                                        project,
                                        namespace,
                                        operation,
                                        effect,
                                    );
                                    let report_json = match &report {
                                        Ok(report) => json!(report),
                                        Err(error) => json!({ "error": error.to_string() }),
                                    };
                                    diagnostics.record(
                                        "request.post_write_persistence_end",
                                        json!({
                                            "namespace": namespace,
                                            "operation": operation,
                                            "report": report_json,
                                            "elapsed_ms": elapsed_ms(persistence_start.elapsed()),
                                        }),
                                    );
                                    report
                                });
                            match report {
                                Ok(report) => report,
                                Err(error) => {
                                    diagnostics.record(
                                        "request.post_write_persistence_failed",
                                        json!({
                                            "namespace": namespace,
                                            "operation": operation,
                                            "error": error.to_string(),
                                        }),
                                    );
                                    return command_construction_error_to_response(id, error);
                                }
                            }
                        } else {
                            None
                        };

                    let trace = serde_json::to_value(&invoke_result.trace).ok();
                    let (mut result, steering) = split_command_envelope(invoke_result.data);
                    if let Some(report) = post_write_report
                        && let Some(result_object) = result.as_object_mut()
                        && let Ok(report_value) = serde_json::to_value(report)
                    {
                        result_object.insert("post_write".to_string(), report_value);
                    }
                    let mut response = ok_with_steering(id, result, steering, display);
                    response.effect = Some(effect);
                    response.trace = trace;
                    response
                }
                Err(error_response) => {
                    diagnostics.record(
                        "request.invoke_end",
                        json!({
                            "namespace": namespace,
                            "operation": operation,
                            "status": "error",
                            "elapsed_ms": elapsed_ms(start.elapsed()),
                        }),
                    );
                    command_error_to_response(id, error_response, &params.address, &params.input)
                }
            }
        }
        Err(diagnostic) => {
            diagnostics.record(
                "request.route_end",
                json!({
                    "namespace": namespace,
                    "operation": operation,
                    "status": "error",
                    "elapsed_ms": elapsed_ms(route_start.elapsed()),
                }),
            );
            // Surface the routing diagnostic as a structured error
            routing_diagnostic_to_response(id, &diagnostic, namespace)
        }
    }
}

fn command_construction_error_to_response(id: String, err: anyhow::Error) -> ResponseEnvelope {
    if let Some(failure) = err.downcast_ref::<ExoFailure>() {
        let details = merge_error_details(
            failure.error.details.clone(),
            Some(json!(failure.steering.clone())),
        );
        return ResponseEnvelope {
            protocol_version: protocol::PROTOCOL_VERSION,
            id,
            status: failure.status,
            result: None,
            error: Some(ErrorBody {
                code: failure.error.code,
                message: failure.error.message.clone(),
                details,
            }),
            ticket: None,
            steering: None,
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
    }

    error(
        id,
        ErrorCode::Internal,
        err.to_string(),
        None,
        steer_help_root(),
    )
}

/// Convert a `RoutingDiagnostic` into a structured error response.
///
/// Maps `DiagnosticCode` to appropriate `ErrorCode`, includes available
/// alternatives and suggestions in the error details for agent self-correction.
fn routing_diagnostic_to_response(
    id: String,
    diagnostic: &RoutingDiagnostic,
    namespace: &str,
) -> ResponseEnvelope {
    let error_code = match diagnostic.code {
        DiagnosticCode::UnknownNamespace | DiagnosticCode::UnknownOperation => {
            ErrorCode::UnknownCommand
        }
        DiagnosticCode::MissingRequiredArg => ErrorCode::MissingArg,
        DiagnosticCode::InvalidArgType => ErrorCode::TypeMismatch,
        DiagnosticCode::UnknownFlag
        | DiagnosticCode::AmbiguousCommand
        | DiagnosticCode::UnsupportedShellFeature
        | DiagnosticCode::TooManyPositionals => ErrorCode::InvalidInput,
    };

    // Build structured details including available alternatives
    let mut details = json!({
        "diagnostic_code": diagnostic.code,
    });

    if let Some(loc) = &diagnostic.location {
        details["location"] = json!(loc);
    }

    if let Some(ctx) = &diagnostic.context {
        if let Some(path) = &ctx.path {
            details["path"] = json!(path);
        }
        if !ctx.available.is_empty() {
            details["available"] = json!(ctx.available);
        }
        if let Some(expected) = &ctx.expected_type {
            details["expected_type"] = json!(expected);
        }
        if let Some(actual) = &ctx.actual_value {
            details["actual_value"] = json!(actual);
        }
    }

    if !diagnostic.suggestions.is_empty() {
        details["suggestions"] = json!(diagnostic.suggestions);
    }

    // Steer to the relevant help level.
    // For UnknownNamespace, steer to root (the namespace itself is invalid).
    let steering = if !namespace.is_empty() && diagnostic.code != DiagnosticCode::UnknownNamespace {
        steer_help_namespace(namespace)
    } else {
        steer_help_root()
    };

    error(
        id,
        error_code,
        diagnostic.message.clone(),
        Some(details),
        steering,
    )
}

fn steer_help_namespace(namespace: &str) -> Steering {
    Steering {
        next_call: protocol::NextCall {
            kind: protocol::NextCallKind::Help,
            params: json!({ "address": { "kind": "namespace", "path": [namespace] } }),
        },
        priority: None,
        confidence: None,
        context_note: None,
    }
}

fn handle_list(
    workspace_root: &Path,
    project: Option<&Project>,
    id: String,
    params: &ListParams,
    spec: &NewCommandSpec,
) -> ResponseEnvelope {
    let argv = list_params_to_argv(params);
    let compilation = compile_argv(spec, &argv);

    let Some(invocation) = compilation.invocation else {
        return error(
            id,
            ErrorCode::InvalidInput,
            "Invalid list (failed to compile to an invocation)".to_string(),
            Some(json!({
                "argv": argv,
                "diagnostics": compilation.diagnostics,
            })),
            compilation.steering.unwrap_or_else(steer_help_root),
        );
    };

    if let Some(cmd) = default_registry().find(
        invocation.path.namespace.as_str(),
        invocation.path.operation.as_str(),
    ) {
        let input = list_params_to_input(params);
        let transport = MachineChannelTransport {
            workspace_root,
            project,
            request_id: id.clone(),
            auth: None,
            expected_ticket: None,
            agent_id: None,
            workflow_confirmation: None,
        };

        return match cmd.invoke_json(&input, &transport) {
            Ok(response) => {
                let (result, steering) = split_command_envelope(response);
                // TODO: invoke_json doesn't return CommandInvokeResult yet,
                // so we generate display from the data alone
                let namespace = invocation.path.namespace.as_str();
                let operation = invocation.path.operation.as_str();
                let invoke_result = CommandInvokeResult {
                    data: normalize_list_result(result),
                    human_message: None,
                    effect: Effect::Pure,
                    trace: exosuit_storage::Trace::default(),
                };
                let display = make_display(namespace, operation, &input, &invoke_result);
                ok_with_steering(id, invoke_result.data, steering, display)
            }
            Err(error_response) => {
                command_error_to_response(id, error_response, &params.address, &input)
            }
        };
    }

    error(
        id,
        ErrorCode::UnknownAddress,
        "Unknown list address".to_string(),
        Some(json!({ "invocation_path": invocation_path_vec(&invocation) })),
        steer_help_root(),
    )
}

fn invocation_path_vec(invocation: &Invocation) -> Vec<String> {
    let mut segments = vec![invocation.path.namespace.clone()];
    if !invocation.path.operation.is_empty() {
        segments.extend(invocation.path.operation.split('.').map(str::to_string));
    }
    segments
}

#[allow(clippy::missing_const_for_fn)]
fn ok_with_steering(
    id: String,
    result: JsonValue,
    steering: Option<Steering>,
    display: Option<Display>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id,
        status: Status::Ok,
        result: Some(result),
        error: None,
        ticket: None,
        steering,
        reminders: None,
        display,
        preview: None,
        effect: None,
        trace: None,
    }
}

fn split_command_envelope(response: JsonValue) -> (JsonValue, Option<Steering>) {
    let Some(envelope) = response.get("_command_envelope") else {
        return (response, None);
    };

    let result = envelope.get("result").cloned().unwrap_or(JsonValue::Null);
    let steering = envelope
        .get("steering")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());

    (result, steering)
}

#[allow(clippy::missing_const_for_fn)]
fn ok(id: String, result: JsonValue) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id,
        status: Status::Ok,
        result: Some(result),
        error: None,
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

#[allow(clippy::missing_const_for_fn)]
fn error(
    id: String,
    code: ErrorCode,
    message: String,
    details: Option<JsonValue>,
    steering: Steering,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details,
        }),
        ticket: None,
        steering: Some(steering),
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

fn command_error_to_response(
    id: String,
    error_response: JsonValue,
    address: &Address,
    input: &JsonValue,
) -> ResponseEnvelope {
    if error_response.get("status").and_then(JsonValue::as_str) == Some("confirm_required") {
        let ticket = error_response
            .get("ticket")
            .and_then(JsonValue::as_str)
            .map_or_else(
                || ticket_for_exec_call(address, input),
                std::string::ToString::to_string,
            );

        let steering = Steering {
            next_call: protocol::NextCall {
                kind: protocol::NextCallKind::Call,
                params: json!({
                    "address": address,
                    "input": input
                }),
            },
            priority: None,
            confidence: None,
            context_note: None,
        };

        return ResponseEnvelope {
            protocol_version: protocol::PROTOCOL_VERSION,
            id,
            status: Status::ConfirmRequired,
            result: None,
            error: None,
            ticket: Some(ticket),
            steering: Some(steering),
            reminders: None,
            display: None,
            preview: None,
            effect: None,
            trace: None,
        };
    }

    let error_object = error_response.get("error");
    let code = error_object
        .and_then(|err| err.get("code"))
        .and_then(JsonValue::as_str)
        .and_then(parse_error_code)
        .unwrap_or(ErrorCode::Internal);
    let message = error_object
        .and_then(|err| err.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Command invocation failed")
        .to_string();

    let details = merge_error_details(
        error_object.and_then(|err| err.get("details")).cloned(),
        error_response.get("steering").cloned(),
    );

    ResponseEnvelope {
        protocol_version: protocol::PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details,
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

fn parse_error_code(raw: &str) -> Option<ErrorCode> {
    match raw {
        "unknown_address" => Some(ErrorCode::UnknownAddress),
        "unknown_list_kind" => Some(ErrorCode::UnknownListKind),
        "invalid_input" => Some(ErrorCode::InvalidInput),
        "missing_ticket" => Some(ErrorCode::MissingTicket),
        "ticket_invalid" => Some(ErrorCode::TicketInvalid),
        "confirm_required" => Some(ErrorCode::ConfirmRequired),
        "not_found" => Some(ErrorCode::NotFound),
        "internal" => Some(ErrorCode::Internal),
        "version_mismatch" => Some(ErrorCode::VersionMismatch),
        "precondition_failed" => Some(ErrorCode::PreconditionFailed),
        "unknown_command" => Some(ErrorCode::UnknownCommand),
        "missing_arg" => Some(ErrorCode::MissingArg),
        "type_mismatch" => Some(ErrorCode::TypeMismatch),
        _ => None,
    }
}

fn merge_error_details(
    details: Option<JsonValue>,
    steering: Option<JsonValue>,
) -> Option<JsonValue> {
    match (details, steering) {
        (None, None) => None,
        (Some(details), None) => Some(details),
        (None, Some(steering)) => Some(json!({ "steering": steering })),
        (Some(details), Some(steering)) => Some(json!({
            "details": details,
            "steering": steering,
        })),
    }
}

fn list_params_to_argv(params: &ListParams) -> Vec<String> {
    let mut argv = Vec::new();
    if let Address::Namespace { path } = &params.address {
        argv.extend(path.iter().cloned());
        argv.push(params.kind.clone());
    }

    normalize_page_args(&mut argv, &params.page);
    argv
}

fn list_params_to_input(params: &ListParams) -> JsonValue {
    let mut map = serde_json::Map::new();

    if let Some(cursor) = params.page.cursor.as_deref() {
        map.insert("cursor".to_string(), JsonValue::String(cursor.to_string()));
    }

    if params.page.limit != DEFAULT_PAGE_LIMIT {
        map.insert("limit".to_string(), json!(params.page.limit));
    }

    JsonValue::Object(map)
}

fn normalize_list_result(result: JsonValue) -> JsonValue {
    let Some(obj) = result.as_object() else {
        return result;
    };

    let Some(items) = obj.get("items") else {
        return result;
    };

    let next_cursor = obj.get("next_cursor").cloned().unwrap_or(JsonValue::Null);
    let mut normalized = serde_json::Map::new();
    normalized.insert("items".to_string(), items.clone());
    normalized.insert(
        "page".to_string(),
        JsonValue::Object({
            let mut page = serde_json::Map::new();
            page.insert("next_cursor".to_string(), next_cursor);
            page
        }),
    );

    JsonValue::Object(normalized)
}

fn normalize_page_args(argv: &mut Vec<String>, page: &Page) {
    // Normalize away defaults so that callers who omit paging converge.
    if let Some(cursor) = page.cursor.as_deref() {
        argv.push("--cursor".to_string());
        argv.push(cursor.to_string());
    }

    if page.limit != DEFAULT_PAGE_LIMIT {
        argv.push("--limit".to_string());
        argv.push(page.limit.to_string());
    }
}

pub fn help_for_address(spec: &NewCommandSpec, address: &Address) -> Option<HelpResult> {
    match address {
        Address::Root => Some(help_for_root(spec)),
        Address::Namespace { path } => {
            if path.len() == 1 {
                spec.namespaces.get(&path[0]).map(help_for_namespace)
            } else {
                None
            }
        }
        Address::Operation { path } => {
            if path.len() == 2 {
                spec.namespaces
                    .get(&path[0])
                    .and_then(|ns| ns.operation(&path[1]))
                    .map(|op| help_for_operation_spec(op, &path[0]))
            } else if path.len() == 1 {
                spec.root_operations
                    .get(&path[0])
                    .map(|op| help_for_operation_spec(op, ""))
            } else {
                None
            }
        }
    }
}

fn help_for_root(spec: &NewCommandSpec) -> HelpResult {
    let mut namespaces: Vec<HelpNamespace> = spec
        .namespaces
        .values()
        .map(|ns| HelpNamespace {
            path: vec![ns.name.clone()],
            summary: ns.description.clone(),
        })
        .collect();
    namespaces.sort_by(|a, b| a.path.cmp(&b.path));

    let mut operations: Vec<HelpOperation> = spec
        .root_operations
        .values()
        .map(|op| HelpOperation {
            path: op.name.clone(),
            effect: op.effect,
            summary: op.description.clone(),
            args: op.args.clone(),
        })
        .collect();
    operations.sort_by(|a, b| a.path.cmp(&b.path));

    let next_calls = if let Some(ns) = namespaces.first() {
        vec![next_help_namespace(&ns.path)]
    } else if let Some(op) = operations.first() {
        vec![next_help_operation(std::slice::from_ref(&op.path))]
    } else {
        vec![]
    };

    HelpResult {
        title: "exo".to_string(),
        summary: "Discover and invoke Exosuit capabilities via the help ladder.".to_string(),
        namespaces,
        operations,
        next_calls,
    }
}

fn help_for_namespace(ns: &crate::command::command_spec::NamespaceSpec) -> HelpResult {
    let mut operations: Vec<HelpOperation> = ns
        .operations
        .values()
        .map(|op| HelpOperation {
            path: format!("{} {}", ns.name, op.name),
            effect: op.effect,
            summary: op.description.clone(),
            args: op.args.clone(),
        })
        .collect();
    operations.sort_by(|a, b| a.path.cmp(&b.path));

    let next_calls = if let Some(op) = operations.first() {
        let path_segments: Vec<String> = op.path.split(' ').map(String::from).collect();
        vec![next_help_operation(&path_segments)]
    } else {
        vec![]
    };

    HelpResult {
        title: ns.name.clone(),
        summary: ns.description.clone(),
        namespaces: vec![],
        operations,
        next_calls,
    }
}

fn help_for_operation_spec(
    op: &crate::command::command_spec::OperationSpec,
    namespace: &str,
) -> HelpResult {
    let title = if namespace.is_empty() {
        op.name.clone()
    } else {
        format!("{} {}", namespace, op.name)
    };

    // Include the operation itself with full arg metadata
    let operation = HelpOperation {
        path: title.clone(),
        effect: op.effect,
        summary: op.description.clone(),
        args: op.args.clone(),
    };

    HelpResult {
        title,
        summary: op.description.clone(),
        namespaces: vec![],
        operations: vec![operation],
        next_calls: vec![],
    }
}

fn next_help_namespace(path: &[String]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "namespace", "path": path }
        }),
    }
}

fn next_help_operation(path: &[String]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "operation", "path": path }
        }),
    }
}

fn steer_help_root() -> Steering {
    Steering {
        next_call: protocol::NextCall {
            kind: protocol::NextCallKind::Help,
            params: json!({ "address": { "kind": "root" } }),
        },
        priority: None,
        confidence: None,
        context_note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::{HelpParams, Op, RequestEnvelope};
    use crate::command_reference::ExoCommandReference;
    use crate::steering::{SuggestedAction, WorkIntent};

    #[test]
    fn help_root_includes_phase_namespace_and_next_call() {
        let req = RequestEnvelope {
            protocol_version: protocol::PROTOCOL_VERSION,
            id: "t1".to_string(),
            op: Op::Help(HelpParams {
                address: Address::Root,
            }),
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        };

        let resp = handle_request(Path::new("/tmp"), req);
        assert_eq!(resp.status, Status::Ok);

        assert!(resp.result.is_some(), "expected response.result");
        let Some(result) = resp.result else {
            return;
        };

        assert!(
            result
                .get("namespaces")
                .and_then(|v| v.as_array())
                .is_some(),
            "expected result.namespaces array"
        );
        let Some(namespaces) = result.get("namespaces").and_then(|v| v.as_array()) else {
            return;
        };

        let has_phase = namespaces.iter().any(|ns| {
            ns.get("path")
                .and_then(|p| p.as_array())
                .is_some_and(|p| p.iter().any(|s| s.as_str() == Some("phase")))
        });
        assert!(has_phase);

        assert!(
            result
                .get("next_calls")
                .and_then(|v| v.as_array())
                .is_some(),
            "expected result.next_calls array"
        );
        let Some(next_calls) = result.get("next_calls").and_then(|v| v.as_array()) else {
            return;
        };
        assert!(!next_calls.is_empty());
    }

    #[test]
    fn version_mismatch_is_error_with_steering() {
        let req = RequestEnvelope {
            protocol_version: 999,
            id: "t2".to_string(),
            op: Op::Help(HelpParams {
                address: Address::Root,
            }),
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        };

        let resp = handle_request(Path::new("/tmp"), req);
        assert_eq!(resp.status, Status::Error);

        assert!(resp.error.is_some(), "expected response.error");
        let Some(err) = resp.error else {
            return;
        };
        assert_eq!(err.code, ErrorCode::VersionMismatch);
        assert!(resp.steering.is_some());
    }

    #[test]
    fn exo_failure_response_preserves_recovery_steering_details() {
        let failure = ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "sidecar local checkpoint failed after durable Exo mutation",
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Retry local sidecar checkpoint",
                ExoCommandReference::new(&["sidecar", "checkpoint"]),
                "Complete the local sidecar checkpoint for this project.",
                WorkIntent::Execute,
                Some(1.0),
            )]),
        )
        .with_details(json!({
            "kind": "sidecar.local_checkpoint",
            "ok": false,
        }));

        let resp = command_construction_error_to_response(
            "checkpoint-failed".to_string(),
            anyhow::Error::new(failure),
        );

        assert_eq!(resp.status, Status::Error);
        assert!(resp.steering.is_none(), "{resp:?}");
        let error = resp.error.expect("error body");
        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        let details = error.details.expect("error details");
        assert_eq!(details["details"]["kind"], "sidecar.local_checkpoint");
        assert!(
            details["steering"]["next_actions"]
                .as_array()
                .expect("next actions")
                .iter()
                .any(|action| action["command"] == "exo sidecar checkpoint"),
            "{details:?}"
        );
    }

    #[test]
    fn goal_complete_summary_includes_completed_outcome() {
        let input = json!({
            "id": "outcome-goal",
            "log": "Outcome shipped"
        });
        let data = json!({
            "kind": "goal.complete",
            "ok": true,
            "goal_id": "outcome-goal",
            "message": "Outcome shipped",
            "outcome": "Outcome shipped"
        });

        assert_eq!(
            generate_mutation_summary("goal.complete", &input, &data),
            "Completed goal 'outcome-goal': Outcome shipped"
        );
    }

    #[test]
    fn task_rename_summary_reports_old_and_new_handles() {
        let input = json!({"id": "old-task", "to": "new-task"});
        let data = json!({
            "kind": "task.rename",
            "ok": true,
            "old_task_id": "old-task",
            "task_id": "new-task",
            "title": "Repair task addressing"
        });

        assert_eq!(
            generate_mutation_summary("task.rename", &input, &data),
            "Renamed task 'old-task' (Repair task addressing) to 'new-task'"
        );
    }

    #[test]
    fn creation_summaries_pair_handles_with_titles() {
        assert_eq!(
            generate_mutation_summary(
                "goal.add",
                &json!({"id": "planning-polish", "label": "Planning polish"}),
                &json!({"kind": "goal.add", "ok": true, "goal_id": "planning-polish"}),
            ),
            "Added goal 'planning-polish' (Planning polish)"
        );
        assert_eq!(
            generate_mutation_summary(
                "task.add",
                &json!({
                    "id": "repair-addressing",
                    "label": "Repair task addressing",
                    "goal": "planning-polish"
                }),
                &json!({"kind": "task.add", "ok": true, "task_id": "repair-addressing"}),
            ),
            "Added task 'repair-addressing' (Repair task addressing) to goal planning-polish"
        );
    }

    #[test]
    fn phase_and_plan_bodies_omit_opaque_ids_from_narrative() {
        let phase_id = "01ktopaquephaseidentifier";
        let epoch_id = "01ktopaqueepochidentifier";
        let plan = json!({
            "plan": {
                "epochs": [{
                    "id": epoch_id,
                    "title": "Planning polish",
                    "status": "in-progress",
                    "phases": [{
                        "id": phase_id,
                        "title": "Human-readable addressing",
                        "status": "in-progress",
                        "goals": [{
                            "id": "addressing",
                            "label": "Address entities conversationally",
                            "status": "in-progress"
                        }]
                    }]
                }]
            }
        });
        let body = render_plan_tree(&plan).expect("plan body");
        assert!(body.contains("Planning polish"), "{body}");
        assert!(body.contains("Human-readable addressing"), "{body}");
        assert!(body.contains("addressing"), "{body}");
        assert!(!body.contains(epoch_id), "{body}");
        assert!(!body.contains(phase_id), "{body}");

        let details = json!({
            "phaseId": phase_id,
            "phaseTitle": "Human-readable addressing",
            "epochTitle": "Planning polish",
            "goals": []
        });
        let body = render_phase_details(&details).expect("phase details body");
        assert!(body.contains("Human-readable addressing"), "{body}");
        assert!(!body.contains(phase_id), "{body}");
    }

    #[test]
    fn goal_list_body_distinguishes_abandoned_goals() {
        let data = json!({
            "kind": "goal.list",
            "ok": true,
            "goals": [
                { "id": "pending-goal", "label": "Pending", "status": "pending" },
                { "id": "abandoned-goal", "label": "Abandoned", "status": "abandoned" },
                { "id": "completed-goal", "label": "Completed", "status": "completed" }
            ]
        });

        let body = generate_body_from_data("goal", "list", &data).expect("goal list body");
        assert!(body.contains("⏳ pending-goal — Pending"));
        assert!(body.contains("⛔ abandoned-goal — Abandoned"));
        assert!(body.contains("✅ completed-goal — Completed"));
    }

    #[test]
    fn phase_status_body_distinguishes_abandoned_goals() {
        let data = json!({
            "phase_title": "Test Phase",
            "goals": [
                { "name": "Pending", "status": "pending" },
                { "name": "Abandoned", "status": "abandoned" },
                { "name": "Completed", "status": "completed" }
            ]
        });

        let body = generate_body_from_data("phase", "status", &data).expect("phase status body");
        assert!(body.contains("⏳ Pending"));
        assert!(body.contains("⛔ Abandoned"));
        assert!(body.contains("✅ Completed"));
    }

    #[test]
    fn rfc_show_body_uses_lifecycle_status_for_archived_rfcs() {
        let data = json!({
            "kind": "rfc.show",
            "ok": true,
            "id": "00022",
            "title": "Unified Project State",
            "stage": 0,
            "status": "archived",
            "feature": "Unknown",
            "filename": "0022-unified-project-state.md",
            "superseded_by": "10176",
            "archived_reason": "Superseded by RFC 10176 as the current SQLite-backed project-state model."
        });

        let body = generate_body_from_data("rfc", "show", &data).expect("rfc show body");
        assert!(body.contains("Status: Archived"), "{body}");
        assert!(body.contains("Superseded by: RFC 10176"), "{body}");
        assert!(!body.contains("Stage: 0 (Idea)"), "{body}");
        assert!(!body.contains("Next: rfc promote"), "{body}");
    }

    #[test]
    fn rfc_show_body_suppresses_promotion_hint_for_superseded_active_rfcs() {
        let data = json!({
            "kind": "rfc.show",
            "ok": true,
            "id": "10114",
            "title": "Strategic Plan Review",
            "stage": 1,
            "status": "active",
            "feature": "Unknown",
            "filename": "10114-strategic-plan-review.md",
            "superseded_by": "10014"
        });

        let body = generate_body_from_data("rfc", "show", &data).expect("rfc show body");
        assert!(body.contains("Stage: 1 (Proposal)"), "{body}");
        assert!(body.contains("Superseded by: RFC 10014"), "{body}");
        assert!(!body.contains("Next: rfc promote"), "{body}");
    }

    #[test]
    fn rfc_list_body_labels_superseded_rfcs() {
        let data = json!({
            "kind": "rfc.list",
            "ok": true,
            "rfcs": [
                {
                    "id": "10114",
                    "title": "Strategic Plan Review",
                    "stage": 1,
                    "status": "superseded",
                    "feature": "Unknown",
                    "filename": "10114-strategic-plan-review.md"
                }
            ]
        });

        let body = generate_body_from_data("rfc", "list", &data).expect("rfc list body");
        assert!(
            body.contains("[Superseded] 10114: Strategic Plan Review"),
            "{body}"
        );
        assert!(!body.contains("[Other]"), "{body}");
    }

    #[test]
    fn rfc_status_body_uses_lifecycle_groups_for_archived_rfcs() {
        let data = json!({
            "kind": "rfc.status",
            "ok": true,
            "total": 1,
            "stages": [
                { "stage": 0, "stage_name": "Idea", "rfcs": [] },
                { "stage": 1, "stage_name": "Proposal", "rfcs": [] },
                { "stage": 2, "stage_name": "Draft", "rfcs": [] },
                { "stage": 3, "stage_name": "Candidate", "rfcs": [] },
                { "stage": 4, "stage_name": "Stable", "rfcs": [] }
            ],
            "lifecycle": [
                {
                    "status": "archived",
                    "status_name": "Archived",
                    "rfcs": [
                        {
                            "id": "00022",
                            "title": "Unified Project State",
                            "stage": 0,
                            "status": "archived",
                            "feature": "Unknown",
                            "filename": "0022-unified-project-state.md"
                        }
                    ]
                }
            ],
            "repairs": []
        });

        let body = generate_body_from_data("rfc", "status", &data).expect("rfc status body");
        assert!(body.contains("Archived RFCs"), "{body}");
        assert!(body.contains("RFC 00022: Unified Project State"), "{body}");
        assert!(!body.contains("Stage 0: Idea"), "{body}");
    }
}
