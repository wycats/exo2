//! JSON channel help responses.

mod test_support;

use exo::api::protocol::{Address, HelpParams, Op, PROTOCOL_VERSION, RequestEnvelope, Status};
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
fn json_channel_help_root_returns_ok() {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "t1".to_string(),
        op: Op::Help(HelpParams {
            address: Address::Root,
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let resp = run_machine_channel_in_process(&repo_root(), &request);

    assert_eq!(resp.status, Status::Ok);

    let result = resp.result.as_ref().expect("expected result");
    let namespaces = result
        .get("namespaces")
        .and_then(|v| v.as_array())
        .expect("expected namespaces array");
    let namespace_text = serde_json::to_string(namespaces).expect("serialize namespaces");
    assert!(namespace_text.contains("phase"));
    assert!(namespace_text.contains("docs"));
}

#[test]
fn json_channel_help_docs_namespace_lists_operations() {
    // Note: "docs" is the namespace, "links.check" and "links.fix" are operations
    // The path ["docs", "links"] is not valid - use ["docs"] to list operations
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "t3".to_string(),
        op: Op::Help(HelpParams {
            address: Address::Namespace {
                path: vec!["docs".to_string()],
            },
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let resp = run_machine_channel_in_process(&repo_root(), &request);

    assert_eq!(resp.status, Status::Ok);

    let result_text = serde_json::to_string(resp.result.as_ref().expect("expected result"))
        .expect("serialize result");
    assert!(result_text.contains("docs links.check"));
    assert!(result_text.contains("docs links.fix"));
}

#[test]
fn json_channel_rejects_bad_version_with_nonzero_exit() {
    let request = RequestEnvelope {
        protocol_version: 999,
        id: "t2".to_string(),
        op: Op::Help(HelpParams {
            address: Address::Root,
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let resp = run_machine_channel_in_process(&repo_root(), &request);

    assert_eq!(resp.status, Status::Error);
    let error = resp.error.as_ref().expect("expected error");
    assert_eq!(error.code, exo::api::protocol::ErrorCode::VersionMismatch);
    let steering = resp.steering.as_ref().expect("expected steering");
    assert_eq!(
        steering.next_call.kind,
        exo::api::protocol::NextCallKind::Help
    );
}
