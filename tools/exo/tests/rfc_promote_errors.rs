#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::api::protocol::{
    Address, CallParams, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope, Status,
};
use serde_json::json;
use test_support::{exo_init_with_storage, run_machine_channel_in_process};

fn rfc_promote_request(id: &str, stage: i64) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "rfc-promote-error".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["rfc".to_string(), "promote".to_string()],
            },
            input: json!({ "id": id, "stage": stage }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

fn rfc_promote_request_with_json_id(id: &str, stage: i64) -> RequestEnvelope {
    let mut request = rfc_promote_request("0", stage);
    let Op::Call(params) = &mut request.op else {
        unreachable!("request is call")
    };
    params.input = json!({ "id": { "not": id }, "stage": stage });
    request
}

#[test]
fn rfc_promote_machine_channel_rejects_non_numeric_id_as_invalid_input() {
    let temp = tempfile::tempdir().expect("create tempdir");
    exo_init_with_storage(temp.path(), "sqlite");

    let response = run_machine_channel_in_process(
        temp.path(),
        &rfc_promote_request_with_json_id("__invalid_probe__", 999),
    );

    assert_eq!(response.status, Status::Error);
    let error = response.error.expect("error body");
    assert_eq!(error.code, ErrorCode::TypeMismatch);
    assert_eq!(error.message, "Expected string for argument 'id'");
    let details = error.details.expect("error details");
    assert_eq!(details["expected_type"], "string");
    assert_eq!(
        details["actual_value"],
        serde_json::Value::String("{\"not\":\"__invalid_probe__\"}".to_string())
    );
}

#[test]
fn rfc_promote_machine_channel_reports_missing_rfc_as_not_found() {
    let temp = tempfile::tempdir().expect("create tempdir");
    exo_init_with_storage(temp.path(), "sqlite");

    let response = run_machine_channel_in_process(temp.path(), &rfc_promote_request("99999", 1));

    assert_eq!(response.status, Status::Error);
    let error = response.error.expect("error body");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.message,
        "RFC 99999 not found. Use `exo rfc list` to see available RFCs."
    );
    let details = error.details.expect("error details");
    let details = details.get("details").unwrap_or(&details);
    assert_eq!(details["operation"], "rfc.promote");
    assert_eq!(details["rfc_id"], "99999");
    assert_eq!(details["mutation_performed"], false);
    assert_eq!(details["safe_next"], "exo rfc list");
}
