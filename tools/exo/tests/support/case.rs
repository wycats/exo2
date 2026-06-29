use std::path::Path;

use exo::api::handler::handle_request;
use exo::api::protocol::{
    Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, Status,
};

use crate::support::{ExoRun, run_exo_interleaved};

pub struct CliCase {
    pub argv: Vec<&'static str>,
    pub assert: fn(&ExoRun),
}

pub struct ProtocolCase {
    pub request: fn() -> RequestEnvelope,
    pub assert: fn(&ResponseEnvelope),
}

pub struct Case {
    #[allow(dead_code)]
    pub name: &'static str,
    pub cli_human: Option<CliCase>,
    pub cli_json: Option<CliCase>,
    pub protocol: Option<ProtocolCase>,
    pub machine_channel: Option<ProtocolCase>,
}

impl Case {
    pub fn run_all(&self, repo_root: &Path) {
        if let Some(cli) = &self.cli_human {
            let run = run_exo_interleaved(repo_root, &cli.argv);
            (cli.assert)(&run);
        }

        if let Some(cli) = &self.cli_json {
            let run = run_exo_interleaved(repo_root, &cli.argv);
            (cli.assert)(&run);
        }

        if let Some(protocol) = &self.protocol {
            let req = (protocol.request)();
            let resp = handle_request(repo_root, req);
            (protocol.assert)(&resp);
        }

        if let Some(machine) = &self.machine_channel {
            let req = (machine.request)();
            let resp = run_machine_channel_in_process(repo_root, &req);
            (machine.assert)(&resp);
        }
    }
}

pub fn req_call_op(id: &str, op_path: &[&str], input: serde_json::Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: id.to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: op_path.iter().map(|s| (*s).to_string()).collect(),
            },
            input,
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

pub fn run_machine_channel_in_process(
    repo_root: &Path,
    request: &RequestEnvelope,
) -> ResponseEnvelope {
    // Simulate the process boundary: serialize/deserialize, then call the handler.
    let input = serde_json::to_string(request).expect("serialize request");
    let parsed: RequestEnvelope = serde_json::from_str(&input).expect("deserialize request");

    let mut response = handle_request(repo_root, parsed);

    // Match `exo json channel` behavior: attach global verifier reminders.
    let reminders = exo::verifiers::run_global_verifiers(repo_root);
    if !reminders.is_empty() {
        response.reminders = Some(reminders);
    }

    // Ensure the response itself round-trips as JSON too.
    let out = serde_json::to_string(&response).expect("serialize response");
    let _roundtrip: ResponseEnvelope = serde_json::from_str(&out).expect("deserialize response");

    response
}

pub fn assert_docs_links_check_ok(resp: &ResponseEnvelope) {
    assert_eq!(resp.status, Status::Ok);

    let result = resp.result.as_ref().expect("result");
    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(true)
    );

    assert_eq!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0)
    );

    assert_eq!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0)
    );

    assert!(resp.steering.is_none(), "expected no steering on ok");
}

pub fn assert_docs_links_check_needs_fix(resp: &ResponseEnvelope) {
    assert_eq!(resp.status, Status::Ok);
    assert!(resp.error.is_none(), "expected no error");

    let result = resp.result.as_ref().expect("result");
    // ok = false because there are links that need fixing
    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(false)
    );

    // changes should be present for the link that needs fixing
    assert!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .map_or(false, |arr| !arr.is_empty()),
        "expected changes for the link that needs fixing"
    );

    // With changes present, steering should provide next steps
}

pub fn assert_docs_links_fix_ok(resp: &ResponseEnvelope) {
    assert_eq!(resp.status, Status::Ok);
    assert!(resp.error.is_none(), "expected no error");

    let result = resp.result.as_ref().expect("result");
    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(true)
    );

    assert_eq!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0)
    );

    assert_eq!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0)
    );

    assert!(resp.steering.is_none(), "expected no steering on ok");
}

pub fn assert_docs_links_check_warning_only(resp: &ResponseEnvelope) {
    assert_eq!(resp.status, Status::Ok);
    assert!(resp.error.is_none(), "expected no error");

    let result = resp.result.as_ref().expect("result");
    // ok = false because there are warnings (diagnostics not empty)
    // strict=false only changes severity from "error" to "warning", not the ok status
    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(false)
    );

    // diagnostics should contain the warning about the unresolved link
    assert!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .map_or(false, |arr| !arr.is_empty()),
        "expected warning diagnostics for unresolved link"
    );

    assert_eq!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0)
    );

    // No steering because there's no actionable fix (link can't be resolved)
    assert!(
        resp.steering.is_none(),
        "expected no steering on warning-only"
    );
}
