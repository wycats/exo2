//! JSON channel call: context path discovery.

mod test_support;

use exo::api::protocol::{Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, Status};
use serde_json::json;
use std::path::PathBuf;

use test_support::run_machine_channel_in_process;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

#[test]
fn json_channel_call_context_paths_returns_ok() {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "c1".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["context".to_string(), "paths".to_string()],
            },
            input: json!({}),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let resp = run_machine_channel_in_process(&repo_root(), &request);

    assert_eq!(resp.status, Status::Ok);

    let result = resp.result.as_ref().expect("expected result");
    let plan = result
        .get("plan")
        .or_else(|| result.get("paths").and_then(|v| v.get("plan")));
    assert!(plan.is_some(), "expected plan path");

    let tasks = result
        .get("tasks")
        .or_else(|| result.get("paths").and_then(|v| v.get("tasks")));
    let tasks = tasks.and_then(|v| v.as_str()).expect("expected tasks path");
    assert!(
        tasks.ends_with("tasks.sql"),
        "expected tasks.sql path, got {tasks}"
    );
}
