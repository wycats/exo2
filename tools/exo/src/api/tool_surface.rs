use crate::api::protocol::{CallParams, ListParams, Op, Page, RequestEnvelope};
use crate::command_spec::{ArgId, ArgKind, ArgSpec, CommandNode, CommandSpec, ValueKind};

const DEFAULT_PAGE_LIMIT: u32 = 20;

fn arg_id(id: &str) -> ArgId {
    ArgId(id.to_string())
}

/// `CommandSpec` describing the machine-channel/tool surface.
///
/// This spec is the shared source of truth for compilation from both argv and tool/JSON requests.
pub fn command_spec() -> CommandSpec {
    let mut root = CommandNode::leaf("root", "exo");

    // context paths
    let mut context = CommandNode::leaf("context", "context");
    context
        .children
        .push(CommandNode::leaf("context_paths", "paths"));

    // docs links (check/fix)
    let mut docs = CommandNode::leaf("docs", "docs");
    let mut links = CommandNode::leaf("docs_links", "links");
    links
        .children
        .push(CommandNode::leaf("docs_links_check", "check"));
    links
        .children
        .push(CommandNode::leaf("docs_links_fix", "fix"));
    docs.children.push(links);

    // run task (exec)
    let mut run = CommandNode::leaf("run", "run");

    let mut run_task = CommandNode::leaf("run_task", "task");
    run_task.args.push(ArgSpec {
        id: arg_id("id"),
        long: Some("id".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::String,
        },
        required: true,
        repeatable: false,
    });
    run.children.push(run_task);

    // run tasks (list)
    let mut run_tasks = CommandNode::leaf("run_tasks", "tasks");
    run_tasks.args.push(ArgSpec {
        id: arg_id("cursor"),
        long: Some("cursor".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::String,
        },
        required: false,
        repeatable: false,
    });
    run_tasks.args.push(ArgSpec {
        id: arg_id("limit"),
        long: Some("limit".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::Int,
        },
        required: false,
        repeatable: false,
    });
    run.children.push(run_tasks);

    // phase execution tasks (list)
    let mut phase = CommandNode::leaf("phase", "phase");
    let mut execution = CommandNode::leaf("phase_execution", "execution");
    let mut phase_execution_tasks = CommandNode::leaf("phase_execution_tasks", "tasks");
    phase_execution_tasks.args.push(ArgSpec {
        id: arg_id("cursor"),
        long: Some("cursor".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::String,
        },
        required: false,
        repeatable: false,
    });
    phase_execution_tasks.args.push(ArgSpec {
        id: arg_id("limit"),
        long: Some("limit".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::Int,
        },
        required: false,
        repeatable: false,
    });
    execution.children.push(phase_execution_tasks);
    phase.children.push(execution);

    // feedback threads (list)
    let mut feedback = CommandNode::leaf("feedback", "feedback");
    let mut feedback_threads = CommandNode::leaf("feedback_threads", "threads");
    feedback_threads.args.push(ArgSpec {
        id: arg_id("cursor"),
        long: Some("cursor".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::String,
        },
        required: false,
        repeatable: false,
    });
    feedback_threads.args.push(ArgSpec {
        id: arg_id("limit"),
        long: Some("limit".to_string()),
        short: None,
        kind: ArgKind::Option {
            value: ValueKind::Int,
        },
        required: false,
        repeatable: false,
    });
    feedback.children.push(feedback_threads);

    // feedback thread mutations (call)
    let mut feedback_thread = CommandNode::leaf("feedback_thread", "thread");
    feedback_thread
        .children
        .push(CommandNode::leaf("feedback_thread_create", "create"));
    feedback_thread
        .children
        .push(CommandNode::leaf("feedback_thread_reply", "reply"));
    feedback_thread
        .children
        .push(CommandNode::leaf("feedback_thread_status", "status"));
    feedback.children.push(feedback_thread);

    // rfc (call)
    let mut rfc = CommandNode::leaf("rfc", "rfc");
    let mut rfc_show = CommandNode::leaf("rfc_show", "show");
    rfc_show.args.push(ArgSpec {
        id: arg_id("id"),
        long: None,
        short: None,
        kind: ArgKind::Positional {
            value: ValueKind::String,
        },
        required: true,
        repeatable: false,
    });
    rfc.children.push(rfc_show);

    root.children.push(context);
    root.children.push(docs);
    root.children.push(feedback);
    root.children.push(phase);
    root.children.push(rfc);
    root.children.push(run);

    CommandSpec::new(root)
}

/// Convert a machine-channel request into argv tokens.
///
/// This is intentionally lossy (e.g. pagination defaults are normalized away) so that
/// frontends converge on the same `Invocation`.
pub fn request_to_argv(request: &RequestEnvelope) -> Option<Vec<String>> {
    match &request.op {
        Op::Help(_) => None,
        Op::Call(params) => Some(call_params_to_argv(params)),
        Op::List(params) => Some(list_params_to_argv(params)),
    }
}

pub fn call_params_to_argv(params: &CallParams) -> Vec<String> {
    call_to_argv(params)
}

pub fn list_params_to_argv(params: &ListParams) -> Vec<String> {
    list_to_argv(params)
}

fn call_to_argv(params: &CallParams) -> Vec<String> {
    let mut argv = Vec::new();

    if let crate::api::protocol::Address::Operation { path } = &params.address {
        argv.extend(path.iter().cloned());
    }

    // Encode only known input fields for known operations.
    // This keeps the mapping stable and prevents accidental coupling to JSON payload shape.
    match &params.address {
        crate::api::protocol::Address::Operation { path }
            if path.len() == 2 && path[0] == "run" && path[1] == "task" =>
        {
            if let Some(task_id) = params
                .input
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                argv.push("--id".to_string());
                argv.push(task_id);
            }
        }
        crate::api::protocol::Address::Operation { path }
            if path.len() == 2 && path[0] == "rfc" && path[1] == "show" =>
        {
            if let Some(id) = params.input.get("id") {
                if let Some(s) = id.as_str() {
                    argv.push(s.to_string());
                } else if let Some(n) = id.as_i64() {
                    argv.push(n.to_string());
                }
            }
        }
        _ => {}
    }

    argv
}

fn list_to_argv(params: &ListParams) -> Vec<String> {
    let mut argv = Vec::new();

    if let crate::api::protocol::Address::Namespace { path } = &params.address {
        argv.extend(path.iter().cloned());
        argv.push(params.kind.clone());
    }

    normalize_page_args(&mut argv, &params.page);
    argv
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
