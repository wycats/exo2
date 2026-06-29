//! JSON channel list: phase execution tasks.

mod test_support;

use exo::api::protocol::{
    Address, ListParams, Op, PROTOCOL_VERSION, Page, RequestEnvelope, Status,
};
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
fn json_channel_list_phase_execution_tasks_returns_ok_with_items() {
    // Storage-dependent: runs against the repo's configured backend in exosuit.toml.
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "t3".to_string(),
        op: Op::List(ListParams {
            address: Address::Namespace {
                path: vec!["phase".to_string(), "execution".to_string()],
            },
            kind: "tasks".to_string(),
            page: Page {
                cursor: None,
                limit: 2,
            },
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let resp = run_machine_channel_in_process(&repo_root(), &request);

    assert_eq!(resp.status, Status::Ok);
    let result = resp.result.as_ref().expect("expected result");
    assert!(result.get("items").is_some(), "expected items");
    assert!(result.get("page").is_some(), "expected page");
}
