use crate::api::protocol::{
    Address, Effect, HelpNamespace, HelpOperation, HelpResult, NextCall, NextCallKind,
};
use serde_json::json;

fn ns(path: &[&str], summary: &str) -> HelpNamespace {
    HelpNamespace {
        path: path.iter().map(std::string::ToString::to_string).collect(),
        summary: summary.to_string(),
    }
}

fn op(path: &str, effect: Effect, summary: &str) -> HelpOperation {
    HelpOperation {
        path: path.to_string(),
        effect,
        summary: summary.to_string(),
    }
}

fn next_help_namespace(path: &[&str]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "namespace", "path": path }
        }),
    }
}

fn next_help_operation(path: &[&str]) -> NextCall {
    NextCall {
        kind: NextCallKind::Help,
        params: json!({
            "address": { "kind": "operation", "path": path }
        }),
    }
}

pub fn help(address: &Address) -> Option<HelpResult> {
    match address {
        Address::Root => Some(help_root()),
        Address::Namespace { path } => help_namespace(path),
        Address::Operation { path } => help_operation(path),
    }
}

fn help_root() -> HelpResult {
    // Keep this small and high-signal. These are navigation entry points.
    // Only advertise namespaces that are actually supported by the handler.
    let mut namespaces = vec![
        ns(&["context"], "Context artifacts"),
        ns(&["docs"], "Document processing and coherence helpers"),
        ns(&["phase"], "Phase lifecycle and execution"),
        ns(&["run"], "Project workflows"),
    ];

    namespaces.sort_by(|a, b| a.path.cmp(&b.path));

    HelpResult {
        title: "root".to_string(),
        summary: "Discover and invoke Exosuit capabilities via the help ladder.".to_string(),
        namespaces,
        operations: vec![],
        next_calls: vec![next_help_namespace(&["phase"])],
    }
}

fn help_namespace(path: &[String]) -> Option<HelpResult> {
    if path.len() == 1 && path[0] == "phase" {
        let namespaces = vec![ns(
            &["phase", "execution"],
            "Phase execution artifact (tasks + log + verification)",
        )];

        // Note: We intentionally do not advertise phase.start/status/finish here yet.
        // Those operations exist in the human CLI, but are not implemented on the
        // machine channel handler.
        let operations: Vec<HelpOperation> = vec![];

        Some(HelpResult {
            title: "phase".to_string(),
            summary: "Start/finish phases and manage phase execution.".to_string(),
            namespaces,
            operations,
            next_calls: vec![next_help_namespace(&["phase", "execution"])],
        })
    } else if path.len() == 2 && path[0] == "phase" && path[1] == "execution" {
        let namespaces: Vec<HelpNamespace> = vec![];

        let mut operations = vec![
            op(
                "phase.execution.task.list",
                Effect::Pure,
                "List tasks in the phase execution artifact",
            ),
            op(
                "phase.execution.task.update_status",
                Effect::Write,
                "Update task status",
            ),
            op(
                "phase.execution.task.append_log",
                Effect::Write,
                "Append a log/walkthrough entry to a task",
            ),
            op(
                "phase.execution.task.add_verification",
                Effect::Write,
                "Add a verification entry to a task",
            ),
        ];
        operations.sort_by(|a, b| a.path.cmp(&b.path));

        Some(HelpResult {
            title: "phase.execution".to_string(),
            summary:
                "Operate on the canonical phase execution artifact (tasks + verification + log)."
                    .to_string(),
            namespaces,
            operations,
            next_calls: vec![],
        })
    } else if path.len() == 1 && path[0] == "context" {
        let namespaces: Vec<HelpNamespace> = vec![];

        let mut operations = vec![op(
            "context.paths",
            Effect::Pure,
            "Return canonical paths to Exosuit-managed context artifacts",
        )];
        operations.sort_by(|a, b| a.path.cmp(&b.path));

        Some(HelpResult {
            title: "context".to_string(),
            summary: "Work with Exosuit agent-context artifacts.".to_string(),
            namespaces,
            operations,
            next_calls: vec![next_help_operation(&["context", "paths"])],
        })
    } else if path.len() == 1 && path[0] == "docs" {
        let namespaces = vec![ns(
            &["docs", "links"],
            "Resolve and compile `exo:` links in markdown",
        )];

        Some(HelpResult {
            title: "docs".to_string(),
            summary: "Document processing helpers (coherence tooling).".to_string(),
            namespaces,
            operations: vec![],
            next_calls: vec![next_help_namespace(&["docs", "links"])],
        })
    } else if path.len() == 2 && path[0] == "docs" && path[1] == "links" {
        let namespaces: Vec<HelpNamespace> = vec![];

        let mut operations = vec![
            op(
                "docs.links.check",
                Effect::Pure,
                "Check markdown files for `exo:` links and report required rewrites",
            ),
            op(
                "docs.links.fix",
                Effect::Write,
                "Rewrite `exo:` links in markdown files to GitHub-clickable relative links",
            ),
        ];
        operations.sort_by(|a, b| a.path.cmp(&b.path));

        Some(HelpResult {
            title: "docs.links".to_string(),
            summary: "Compile `exo:` links into relative markdown links.".to_string(),
            namespaces,
            operations,
            next_calls: vec![],
        })
    } else if path.len() == 1 && path[0] == "run" {
        let namespaces: Vec<HelpNamespace> = vec![];

        let operations = vec![op(
            "run.task",
            Effect::Exec,
            "Execute an exosuit.toml task by id",
        )];

        Some(HelpResult {
            title: "run".to_string(),
            summary: "Project workflow/task execution.".to_string(),
            namespaces,
            operations,
            next_calls: vec![next_help_operation(&["run", "task"])],
        })
    } else {
        None
    }
}

fn help_operation(path: &[String]) -> Option<HelpResult> {
    if path.len() == 2 && path[0] == "context" && path[1] == "paths" {
        Some(HelpResult {
            title: "context.paths".to_string(),
            summary: "Return canonical paths to Exosuit-managed context artifacts (effect: pure)."
                .to_string(),
            namespaces: vec![],
            operations: vec![],
            next_calls: vec![],
        })
    } else if path.len() == 2 && path[0] == "run" && path[1] == "task" {
        Some(HelpResult {
            title: "run.task".to_string(),
            summary: "Execute an exosuit.toml task by id (effect: exec).".to_string(),
            namespaces: vec![],
            operations: vec![],
            next_calls: vec![],
        })
    } else if path.len() == 3 && path[0] == "docs" && path[1] == "links" && path[2] == "check" {
        Some(HelpResult {
            title: "docs.links.check".to_string(),
            summary:
                "Check markdown files for `exo:` links and report required rewrites (effect: pure)."
                    .to_string(),
            namespaces: vec![],
            operations: vec![],
            next_calls: vec![],
        })
    } else if path.len() == 3 && path[0] == "docs" && path[1] == "links" && path[2] == "fix" {
        Some(HelpResult {
            title: "docs.links.fix".to_string(),
            summary: "Rewrite `exo:` links in markdown files to relative links (effect: write)."
                .to_string(),
            namespaces: vec![],
            operations: vec![],
            next_calls: vec![],
        })
    } else {
        None
    }
}
