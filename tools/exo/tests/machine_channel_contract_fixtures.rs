//! RFC JSON fixture parity checks for the machine channel.

#[macro_use]
mod test_support;

use exo::api::protocol::RequestEnvelope;
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use test_support::run_machine_channel_in_process;

static REPO_ROOT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // best-effort: pop twice; if it fails, keep what we have.
    let _ = p.pop();
    let _ = p.pop();
    p
}

fn run_channel(request_json: &str) -> JsonValue {
    let _guard = REPO_ROOT_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("repo-root test lock");
    let request: RequestEnvelope = ok_or_return!(
        serde_json::from_str(request_json),
        "expected valid request envelope";
        JsonValue::Null
    );

    let resp = run_machine_channel_in_process(&repo_root(), &request);
    ok_or_return!(
        serde_json::to_value(resp),
        "expected response to serialize";
        JsonValue::Null
    )
}

fn run_channel_expect_ok(request_json: &str) -> JsonValue {
    let resp = run_channel(request_json);
    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));
    resp
}

fn run_channel_expect_error(request_json: &str) -> JsonValue {
    let resp = run_channel(request_json);
    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("error"));
    resp
}

#[test]
fn machine_channel_error_includes_steering() {
    let resp = run_channel_expect_error(
        r#"{"protocol_version":1,"id":"req-inv-1","op":{"kind":"call","params":{"address":{"kind":"operation","path":["no","such","operation"]},"input":{}}}}"#,
    );

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("error"));
    assert!(resp.get("error").is_some(), "error body present");
    assert!(resp.get("steering").is_some(), "steering present");
}

#[test]
fn machine_channel_fixtures_help_root() {
    // Matches the RFC JSON fixture.
    let resp = run_channel_expect_ok(
        r#"{"protocol_version":1,"id":"req-1","op":{"kind":"help","params":{"address":{"kind":"root"}}}}"#,
    );

    assert_eq!(
        resp.get("protocol_version").and_then(JsonValue::as_u64),
        Some(1)
    );
    assert_eq!(resp.get("id").and_then(|v| v.as_str()), Some("req-1"));
    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));

    let result = some_or_return!(resp.get("result"), "expected result");
    assert_eq!(result.get("title").and_then(|v| v.as_str()), Some("exo"));

    let namespaces = some_or_return!(
        result.get("namespaces").and_then(|v| v.as_array()),
        "expected namespaces array"
    );
    assert!(namespaces.iter().any(|ns| {
        ns.get("path")
            .and_then(|p| p.as_array())
            .is_some_and(|p| p.iter().any(|seg| seg.as_str() == Some("phase")))
    }));

    let next_calls = some_or_return!(
        result.get("next_calls").and_then(|v| v.as_array()),
        "expected next_calls array"
    );
    assert!(!next_calls.is_empty());
}

#[test]
fn machine_channel_fixtures_call_context_paths() {
    // Matches the RFC JSON fixture.
    let resp = run_channel_expect_ok(
        r#"{"protocol_version":1,"id":"req-2","op":{"kind":"call","params":{"address":{"kind":"operation","path":["context","paths"]},"input":{}}}}"#,
    );

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));

    let result = some_or_return!(resp.get("result"), "expected result");
    // The Command trait implementation wraps paths in a `paths` object
    let paths = some_or_return!(result.get("paths"), "expected result.paths");
    assert!(paths.get("plan").is_some(), "paths.plan present");
    let tasks = some_or_return!(
        paths.get("tasks").and_then(|v| v.as_str()),
        "expected paths.tasks"
    );
    assert!(
        tasks.ends_with("tasks.sql"),
        "expected tasks.sql path, got {tasks}"
    );
}

#[test]
fn machine_channel_fixtures_list_phase_execution_tasks_with_optional_paging() {
    // Storage-dependent: runs against the repo's configured backend in exosuit.toml.
    // Matches the RFC JSON fixture, except we keep the limit small to exercise paging.
    let first = run_channel_expect_ok(
        r#"{"protocol_version":1,"id":"req-3","op":{"kind":"list","params":{"address":{"kind":"namespace","path":["phase","execution"]},"kind":"tasks","page":{"limit":1}}}}"#,
    );

    assert_eq!(first.get("status").and_then(|v| v.as_str()), Some("ok"));

    let result = some_or_return!(first.get("result"), "expected result");
    let items = some_or_return!(
        result.get("items").and_then(|v| v.as_array()),
        "expected items array"
    );

    // The repo may or may not have tasks - we just verify the response structure is correct.
    // If there are items, verify paging works.
    if items.is_empty() {
        // Empty items is valid - the implementation plan may have no tasks
        return;
    }

    // If there is a next cursor, it must be a string offset, and a follow-up call should succeed.
    let next_cursor = result
        .get("page")
        .and_then(|p| p.get("next_cursor"))
        .and_then(|v| v.as_str());

    if let Some(cursor) = next_cursor {
        assert!(
            cursor.parse::<usize>().is_ok(),
            "next_cursor should be a numeric string offset"
        );

        let followup = run_channel_expect_ok(&format!(
            "{{\"protocol_version\":1,\"id\":\"req-3b\",\"op\":{{\"kind\":\"list\",\"params\":{{\"address\":{{\"kind\":\"namespace\",\"path\":[\"phase\",\"execution\"]}},\"kind\":\"tasks\",\"page\":{{\"cursor\":\"{cursor}\",\"limit\":1}}}}}}}}"
        ));
        assert_eq!(followup.get("status").and_then(|v| v.as_str()), Some("ok"));
    }
}

#[test]
fn machine_channel_fixtures_version_mismatch_has_steering() {
    // Matches the RFC JSON fixture.
    let resp = run_channel_expect_error(
        r#"{"protocol_version":999,"id":"req-4","op":{"kind":"help","params":{"address":{"kind":"root"}}}}"#,
    );

    assert_eq!(
        resp.get("protocol_version").and_then(JsonValue::as_u64),
        Some(1)
    );
    assert_eq!(resp.get("id").and_then(|v| v.as_str()), Some("req-4"));
    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("error"));

    let err = some_or_return!(resp.get("error"), "expected error");
    assert_eq!(
        err.get("code").and_then(|v| v.as_str()),
        Some("version_mismatch")
    );

    let steering = some_or_return!(resp.get("steering"), "expected steering");
    let next_call = some_or_return!(steering.get("next_call"), "expected next_call");
    assert_eq!(next_call.get("kind").and_then(|v| v.as_str()), Some("help"));
}

#[test]
fn machine_channel_fixtures_unknown_address_has_steering() {
    // Test that help request for a nonexistent namespace returns unknown_address error with steering to root.
    let resp = run_channel_expect_error(
        r#"{"protocol_version":1,"id":"req-5","op":{"kind":"help","params":{"address":{"kind":"namespace","path":["nonexistent","namespace"]}}}}"#,
    );

    assert_eq!(
        resp.get("protocol_version").and_then(JsonValue::as_u64),
        Some(1)
    );
    assert_eq!(resp.get("id").and_then(|v| v.as_str()), Some("req-5"));
    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("error"));

    let err = some_or_return!(resp.get("error"), "expected error");
    assert_eq!(
        err.get("code").and_then(|v| v.as_str()),
        Some("unknown_address")
    );

    let steering = some_or_return!(resp.get("steering"), "expected steering");
    let next_call = some_or_return!(steering.get("next_call"), "expected next_call");
    assert_eq!(next_call.get("kind").and_then(|v| v.as_str()), Some("help"));
    // The steering should point to root since we couldn't find the namespace
    let params = some_or_return!(next_call.get("params"), "expected params");
    let address = some_or_return!(params.get("address"), "expected address");
    assert_eq!(address.get("kind").and_then(|v| v.as_str()), Some("root"));
}
