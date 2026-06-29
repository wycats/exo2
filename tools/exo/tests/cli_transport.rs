use exo::command::Command;
use exo::command::json::JsonSpec;
use exo::command::traits::OutputFormat;
use exo::command::transport::CliTransport;
#[test]
fn cli_transport_json_emits_envelope() {
    let cmd = JsonSpec;
    let transport = CliTransport::new(OutputFormat::Json);

    let output = cmd
        .invoke_json(&serde_json::json!({}), &transport)
        .expect("expected invoke_json success");

    assert_eq!(
        output
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        output.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );
    assert_eq!(
        output
            .get("result")
            .and_then(|v: &serde_json::Value| v.get("ok"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn cli_transport_human_renders_text() {
    let cmd = JsonSpec;
    let transport = CliTransport::new(OutputFormat::Human);

    let output = cmd
        .invoke_json(&serde_json::json!({}), &transport)
        .expect("expected invoke_json success");

    let rendered = transport.render_value(&output);
    assert!(rendered.contains("CommandSpec"));
}
