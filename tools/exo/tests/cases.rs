#![allow(missing_docs)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

mod support;
#[macro_use]
mod test_support;

use support::{LineEvent, Stream};

use std::fs;
use std::path::Path;

use std::sync::{Mutex, OnceLock};
use support::case::{
    Case, CliCase, ProtocolCase, assert_docs_links_check_needs_fix, assert_docs_links_check_ok,
    assert_docs_links_check_warning_only, assert_docs_links_fix_ok, req_call_op,
};
use support::template::{Fragment, HoleKind, match_template};
use test_case::test_matrix;
use test_support::{
    exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_task_with_storage, exo_plan_update_status_with_storage,
};

static REPO_ROOT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn with_repo_root<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = REPO_ROOT_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("repo-root test lock");
    let root = repo_root();
    f(&root)
}

fn rel_to_repo_root(abs: &Path) -> String {
    let root = repo_root();
    let Ok(rel) = abs.strip_prefix(&root) else {
        assert!(abs.starts_with(&root), "path under repo root");
        return abs.to_string_lossy().replace('\\', "/");
    };

    rel.to_string_lossy().replace('\\', "/")
}

fn assert_cli_envelope<'a>(
    value: &'a serde_json::Value,
    expected_status: &str,
) -> &'a serde_json::Value {
    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "expected protocol_version=1"
    );
    assert!(value.get("id").is_some(), "expected id");
    assert_eq!(
        value.get("status").and_then(|v| v.as_str()),
        Some(expected_status)
    );
    value
}

fn assert_cli_error_envelope(
    value: &serde_json::Value,
) -> (&serde_json::Value, &serde_json::Value) {
    assert_cli_envelope(value, "error");

    assert!(value.get("error").is_some(), "expected error");
    let Some(error) = value.get("error") else {
        return (value, value);
    };

    assert!(value.get("steering").is_some(), "expected steering");
    let Some(steering) = value.get("steering") else {
        return (error, value);
    };

    // CLI invoke_json errors now return steering as a suggestion array, while
    // protocol errors return next_call help metadata.
    if steering.get("next_call").is_some() {
        assert_eq!(
            steering
                .get("next_call")
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str()),
            Some("help")
        );

        assert!(
            steering
                .get("next_call")
                .and_then(|v| v.get("params"))
                .is_some(),
            "expected next_call.params"
        );
        let Some(params) = steering.get("next_call").and_then(|v| v.get("params")) else {
            return (error, steering);
        };
        assert_eq!(
            params
                .get("address")
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str()),
            Some("root")
        );
    } else {
        assert!(steering.is_array(), "expected steering array");
    }

    (error, steering)
}

// NOTE: Avoid holding TempDir in a static. It prevents cleanup and leaves
// untracked directories in the repo root. Tests that need temp workspaces should
// create them per-test so they are deleted when dropped.

fn case_rfc_show_missing() -> Case {
    fn protocol_request() -> exo::api::protocol::RequestEnvelope {
        req_call_op(
            "rfc-show-missing",
            &["rfc", "show"],
            serde_json::json!({"id": "99999"}),
        )
    }

    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        let expected_prefix = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- List RFCs: exo rfc list".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  View all RFCs to find valid IDs.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Show RFC status: exo rfc status".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  See RFCs grouped by stage.".to_string(),
            },
        ];

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        assert!(
            !run.interleaved_lines.is_empty(),
            "expected stderr interleaving"
        );

        let template = [
            Fragment::Lit("RFC "),
            Fragment::Hole(HoleKind::Regex(r"^\d+$")),
            Fragment::Lit(" not found. Use `exo rfc list` to see available RFCs."),
        ];
        let (error_index, caps) = run
            .interleaved_lines
            .iter()
            .enumerate()
            .find_map(|(index, event)| {
                if event.stream != Stream::Stderr {
                    return None;
                }
                match_template(&template, &event.line)
                    .ok()
                    .map(|caps| (index, caps))
            })
            .expect("stderr contains RFC not found line");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].text, "99999");

        assert_eq!(
            run.interleaved_lines[error_index + 1..],
            expected_prefix,
            "unexpected stderr interleaving"
        );
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        let error = value.get("error").expect("error");
        assert_eq!(error.get("code").and_then(|v| v.as_str()), Some("internal"));
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("RFC 99999 not found. Use `exo rfc list` to see available RFCs.")
        );
    }

    fn assert_protocol(resp: &exo::api::protocol::ResponseEnvelope) {
        assert_eq!(resp.status, exo::api::protocol::Status::Error);
        let error = resp.error.as_ref().expect("error");
        assert_eq!(error.code, exo::api::protocol::ErrorCode::Internal);
        assert_eq!(
            error.message,
            "RFC 99999 not found. Use `exo rfc list` to see available RFCs."
        );
    }

    Case {
        name: "rfc_show_missing",
        cli_human: Some(CliCase {
            argv: vec!["rfc", "show", "99999"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "rfc", "show", "99999"],
            assert: assert_cli_json,
        }),
        protocol: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
        machine_channel: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
    }
}

fn case_task_list_json_is_single_value() -> Case {
    fn assert_cli_json(run: &support::ExoRun) {
        assert!(run.status.success(), "expected zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        assert_cli_envelope(&value, "ok");
        let result = value.get("result").expect("result");

        assert_eq!(
            result.get("kind").and_then(|v| v.as_str()),
            Some("task.list")
        );
        assert_eq!(
            result.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );

        let tasks = result
            .get("tasks")
            .and_then(|v| v.as_array())
            .expect("tasks array");
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks[0].get("id").and_then(|v| v.as_str()),
            Some("goal-1::task-1")
        );
    }

    Case {
        name: "task_list_json_is_single_value",
        cli_human: None,
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "task", "list"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_rfc_status_json_is_single_value() -> Case {
    fn assert_cli_json(run: &support::ExoRun) {
        assert!(run.status.success(), "expected zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        assert_cli_envelope(&value, "ok");
        let result = value.get("result").expect("result");

        assert_eq!(
            result.get("kind").and_then(|v| v.as_str()),
            Some("rfc.status")
        );
        assert_eq!(
            result.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(result.get("stages").is_some(), "expected stages in output");
    }

    Case {
        name: "rfc_status_json_is_single_value",
        cli_human: None,
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "rfc", "status"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_docs_links_check_ok() -> Case {
    fn protocol_request() -> exo::api::protocol::RequestEnvelope {
        req_call_op(
            "docs-links-check-ok",
            &["docs", "links", "check"],
            serde_json::json!({
                "targets": {
                    "paths": ["tools/exo/tests/fixtures/docs_links/ok.md"],
                    "globs": []
                },
                "options": { "strict": true }
            }),
        )
    }

    fn assert_protocol(resp: &exo::api::protocol::ResponseEnvelope) {
        assert_docs_links_check_ok(resp);
        let result = resp.result.as_ref().expect("result");
        let summary = result.get("summary").expect("summary");
        assert_eq!(
            summary
                .get("files_scanned")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            summary
                .get("exo_links_found")
                .and_then(serde_json::Value::as_u64),
            Some(0)
        );
    }

    Case {
        name: "docs_links_check_ok",
        cli_human: None,
        cli_json: None,
        protocol: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
        machine_channel: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
    }
}

fn case_docs_links_check_needs_fix() -> Case {
    fn protocol_request() -> exo::api::protocol::RequestEnvelope {
        req_call_op(
            "docs-links-check-needs-fix",
            &["docs", "links", "check"],
            serde_json::json!({
                "targets": {
                    "paths": ["tools/exo/tests/fixtures/docs_links/needs_fix.md"],
                    "globs": []
                },
                "options": { "strict": true }
            }),
        )
    }

    fn assert_protocol(resp: &exo::api::protocol::ResponseEnvelope) {
        assert_docs_links_check_needs_fix(resp);

        let result = resp.result.as_ref().expect("result");
        let summary = result.get("summary").expect("summary");
        assert_eq!(
            summary
                .get("files_scanned")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            summary
                .get("exo_links_found")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
    }

    Case {
        name: "docs_links_check_needs_fix",
        cli_human: None,
        cli_json: None,
        protocol: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
        machine_channel: Some(ProtocolCase {
            request: protocol_request,
            assert: assert_protocol,
        }),
    }
}

fn case_task_add_no_active_phase_or_strike() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        let expected = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: "No active phase found. Use `exo phase start <id>` to start one.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- List tasks: exo task list".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  View all tasks in the active phase.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Show map: exo map".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Get oriented with the project state.".to_string(),
            },
        ];

        assert_eq!(run.interleaved_lines, expected, "unexpected stderr output");
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        let (error, _steering) = assert_cli_error_envelope(&value);
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("invalid_input")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("No active phase found. Use `exo phase start <id>` to start one.")
        );
    }

    Case {
        name: "task_add_no_active_phase_or_strike",
        cli_human: Some(CliCase {
            argv: vec!["task", "add", "Do the thing", "--id", "t-1"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec![
                "--format",
                "json",
                "task",
                "add",
                "Do the thing",
                "--id",
                "t-1",
            ],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn assert_verify_json_failure_stdout(status_success: bool, stdout: &str) {
    assert!(!status_success, "expected non-zero exit status");

    // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json");

    let (error, _steering) = assert_cli_error_envelope(&value);
    // Error code may be "internal" or "invalid_input" depending on the failure path.
    let code = error.get("code").and_then(|v| v.as_str());
    assert!(
        code == Some("internal") || code == Some("invalid_input"),
        "expected error code 'internal' or 'invalid_input', got: {code:?}"
    );

    assert_eq!(
        error.get("message").and_then(|v| v.as_str()),
        Some("Verification failed via exo validate dev (exit 1).")
    );

    let details = error
        .get("details")
        .and_then(|details| details.as_object())
        .expect("verify failure details");

    assert_eq!(
        details.get("runner").and_then(|v| v.as_str()),
        Some("exo validate dev")
    );
    assert_eq!(
        details.get("command").and_then(|v| v.as_str()),
        Some("exo validate dev --color never")
    );
    assert_eq!(details.get("exit_code").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(
        details.get("stdout_tail").and_then(|v| v.as_str()),
        Some("VERIFY_STDOUT")
    );
    assert_eq!(
        details.get("stderr_tail").and_then(|v| v.as_str()),
        Some("VERIFY_STDERR")
    );
    assert!(
        details
            .get("mutating_lane_warning")
            .and_then(|v| v.as_str())
            .is_some_and(|warning| warning.contains("Exohook dev lane")),
        "{details:?}"
    );

    let steering = value.get("steering").expect("steering");
    assert!(
        steering.to_string().contains("exo verify run"),
        "{steering}"
    );
}

fn case_phase_finish_no_active_phase() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        let expected = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: "No active phase found to finish.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Check phase status: exo phase status".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Review current phase completion status before finishing".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Check git status: git status".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Verify all changes are committed before finishing phase".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- List incomplete tasks: exo task list --status pending".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Ensure all tasks are completed before finishing phase".to_string(),
            },
        ];

        assert_eq!(run.interleaved_lines, expected, "unexpected stderr output");
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        let (error, _steering) = assert_cli_error_envelope(&value);
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("not_found")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("No active phase found to finish.")
        );
    }

    Case {
        name: "phase_finish_no_active_phase",
        cli_human: Some(CliCase {
            argv: vec!["phase", "finish"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "phase", "finish"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_phase_finish_dirty_without_message() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        let expected = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: "Working directory is dirty. Please commit your changes or use --message to commit automatically.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Check phase status: exo phase status".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Review current phase completion status before finishing".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Check git status: git status".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Verify all changes are committed before finishing phase".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- List incomplete tasks: exo task list --status pending".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Ensure all tasks are completed before finishing phase".to_string(),
            },
        ];

        assert_eq!(run.interleaved_lines, expected, "unexpected stderr output");
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        // Must be exactly one JSON value (object) on stdout (whitespace/newlines ok).
        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");

        let (error, _steering) = assert_cli_error_envelope(&value);
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("invalid_input")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some(
                "Working directory is dirty. Please commit your changes or use --message to commit automatically."
            )
        );
    }

    Case {
        name: "phase_finish_dirty_without_message",
        cli_human: Some(CliCase {
            argv: vec!["phase", "finish"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "phase", "finish"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_phase_start_no_pending_phase() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        // Should contain error message about no pending phase
        let stderr = &run.stderr;
        assert!(
            stderr.contains("No pending phase") || stderr.contains("no pending phase"),
            "expected 'no pending phase' error in stderr: {}",
            stderr
        );
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");
        let result = assert_cli_envelope(&value, "error");
        let err = result.get("error").expect("error");
        assert_eq!(err.get("code").and_then(|v| v.as_str()), Some("not_found"));
        assert!(
            err.get("message")
                .and_then(|v| v.as_str())
                .map_or(false, |m| m.contains("No pending phase")),
            "expected 'No pending phase' in message"
        );
    }

    Case {
        name: "phase_start_no_pending_phase",
        cli_human: Some(CliCase {
            argv: vec!["phase", "start"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "phase", "start"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_strike_finish_no_active_strike() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        let expected = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: "No active surgical strike to finish.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Show map: exo map".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Use map to orient and get suggested next actions.".to_string(),
            },
        ];

        assert_eq!(run.interleaved_lines, expected, "unexpected stderr output");
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");
        let (error, _steering) = assert_cli_error_envelope(&value);
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("not_found")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("No active surgical strike to finish.")
        );
    }

    Case {
        name: "strike_finish_no_active_strike",
        cli_human: Some(CliCase {
            argv: vec!["strike", "finish"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "strike", "finish"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

fn case_strike_abort_no_active_strike() -> Case {
    fn assert_cli_human(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        assert!(
            run.stdout.trim().is_empty(),
            "expected no stdout in human mode"
        );

        let expected = vec![
            LineEvent {
                stream: Stream::Stderr,
                line: "No active surgical strike to abort.".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: String::new(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "[Next]".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "- Show map: exo map".to_string(),
            },
            LineEvent {
                stream: Stream::Stderr,
                line: "  Use map to orient and get suggested next actions.".to_string(),
            },
        ];

        assert_eq!(run.interleaved_lines, expected, "unexpected stderr output");
    }

    fn assert_cli_json(run: &support::ExoRun) {
        assert!(!run.status.success(), "expected non-zero exit status");

        let value: serde_json::Value = serde_json::from_str(run.stdout.trim()).expect("valid json");
        let (error, _steering) = assert_cli_error_envelope(&value);
        assert_eq!(
            error.get("code").and_then(|v| v.as_str()),
            Some("not_found")
        );
        assert_eq!(
            error.get("message").and_then(|v| v.as_str()),
            Some("No active surgical strike to abort.")
        );
    }

    Case {
        name: "strike_abort_no_active_strike",
        cli_human: Some(CliCase {
            argv: vec!["strike", "abort"],
            assert: assert_cli_human,
        }),
        cli_json: Some(CliCase {
            argv: vec!["--format", "json", "strike", "abort"],
            assert: assert_cli_json,
        }),
        protocol: None,
        machine_channel: None,
    }
}

#[test]
fn rfc_show_missing_runs_across_cli_protocol_and_machine_channel() {
    with_repo_root(|root| {
        let case = case_rfc_show_missing();
        case.run_all(root);
    });
}

#[test]
fn docs_links_check_ok_runs_across_protocol_and_machine_channel() {
    with_repo_root(|root| {
        let case = case_docs_links_check_ok();
        case.run_all(root);
    });
}

#[test]
fn docs_links_check_needs_fix_runs_across_protocol_and_machine_channel() {
    with_repo_root(|root| {
        let case = case_docs_links_check_needs_fix();
        case.run_all(root);
    });
}

#[test]
fn docs_links_fix_runs_across_protocol_and_machine_channel() {
    with_repo_root(|root| {
        let tmp = tempfile::Builder::new()
            .prefix("exo-docs-links-fix-case-")
            .tempdir_in(root)
            .expect("tempdir");

        let md_path = tmp.path().join("input.md");
        fs::write(&md_path, "See [RFC](exo:rfc/8) for details.\n").expect("write markdown");

        let targets_path = rel_to_repo_root(tmp.path());
        let req = req_call_op(
            "docs-links-fix",
            &["docs", "links", "fix"],
            serde_json::json!({
                "targets": { "paths": [targets_path], "globs": [] },
                "options": { "strict": true }
            }),
        );

        let resp_protocol = exo::api::handler::handle_request(root, req.clone());
        assert_docs_links_fix_ok(&resp_protocol);

        let resp_machine = support::case::run_machine_channel_in_process(root, &req);
        assert_docs_links_fix_ok(&resp_machine);
    });
}

#[test]
fn docs_links_check_warning_only_runs_across_protocol_and_machine_channel() {
    with_repo_root(|root| {
        let tmp = tempfile::Builder::new()
            .prefix("exo-docs-links-warn-case-")
            .tempdir_in(root)
            .expect("tempdir");

        let md_path = tmp.path().join("input.md");
        // Intentionally unresolved, and strict=false should downgrade to a warning.
        fs::write(&md_path, "See [Missing](exo:rfc/99999) for details.\n").expect("write markdown");

        let targets_path = rel_to_repo_root(tmp.path());
        let req = req_call_op(
            "docs-links-check-warning-only",
            &["docs", "links", "check"],
            serde_json::json!({
                "targets": { "paths": [targets_path], "globs": [] },
                "options": { "strict": false }
            }),
        );

        let resp_protocol = exo::api::handler::handle_request(root, req.clone());
        assert_docs_links_check_warning_only(&resp_protocol);

        let resp_machine = support::case::run_machine_channel_in_process(root, &req);
        assert_docs_links_check_warning_only(&resp_machine);
    });
}

#[test_matrix(["sqlite"])]
fn task_add_no_active_phase_or_strike_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");

    let case = case_task_add_no_active_phase_or_strike();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn verify_json_is_single_value_on_failure(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);
    // Complete the bootstrap phase so there's no active phase
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");

    std::fs::create_dir_all(root.join(".test-bin")).expect("create test bin");

    // exo verify should prefer exohook through `exo validate dev` when hooks.toml exists.
    // Shadow `exo` so the test proves the selected runner without recursively invoking exo.
    #[cfg(windows)]
    let shim = r#"@echo off
if "%~1"=="validate" if "%~2"=="dev" (
  echo VERIFY_STDOUT
  1>&2 echo VERIFY_STDERR
  exit /b 1
)
1>&2 echo unexpected exo args: %*
exit /b 64
"#;

    #[cfg(not(windows))]
    let shim = r#"#!/usr/bin/env bash
if [[ "$1" == "validate" && "$2" == "dev" ]]; then
  echo VERIFY_STDOUT
  echo VERIFY_STDERR 1>&2
  exit 1
fi
echo "unexpected exo args: $*" 1>&2
exit 64
"#;

    #[cfg(windows)]
    let shim_path = root.join(".test-bin/exo.cmd");
    #[cfg(not(windows))]
    let shim_path = root.join(".test-bin/exo");
    std::fs::write(&shim_path, shim).expect("write exo shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim_path)
            .expect("exo shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim_path, permissions).expect("chmod exo shim");
    }

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![root.join(".test-bin")];
    paths.extend(std::env::split_paths(&old_path));
    let new_path = std::env::join_paths(paths).expect("join PATH");

    let output = exo_cmd_with_storage(root, backend)
        .env("PATH", new_path)
        .args(["--format", "json", "verify"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert_verify_json_failure_stdout(output.status.success(), &stdout);
    assert!(
        !stderr.contains("VERIFY_STDOUT") && !stderr.contains("VERIFY_STDERR"),
        "runner output should be structured in JSON details, not raw stderr: {stderr}"
    );
}

#[test_matrix(["sqlite"])]
fn phase_finish_no_active_phase_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");

    let case = case_phase_finish_no_active_phase();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn phase_finish_dirty_without_message_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);

    // Create a minimal git repo with a dirty working tree.
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(root)
        .status()
        .expect("git config user.email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(root)
        .status()
        .expect("git config user.name");

    let file_path = root.join("README.md");
    std::fs::write(&file_path, "hello\n").expect("write README.md");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-q", "-m", "init"])
        .current_dir(root)
        .status()
        .expect("git commit");

    // Make it dirty.
    std::fs::write(&file_path, "hello dirty\n").expect("write README.md dirty");

    let case = case_phase_finish_dirty_without_message();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn phase_start_no_pending_phase_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");

    let case = case_phase_start_no_pending_phase();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn strike_finish_no_active_strike_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);

    let case = case_strike_finish_no_active_strike();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn strike_abort_no_active_strike_runs_across_cli_human_and_json(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);

    let case = case_strike_abort_no_active_strike();
    case.run_all(root);
}

#[test_matrix(["sqlite"])]
fn task_list_json_is_single_value(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    exo_plan_add_task_with_storage(root, backend, &phase_id, "goal-1", "Goal 1");
    exo_plan_add_task_with_storage(root, backend, &phase_id, "goal-2", "Goal 2");

    // Add tasks under goals via CLI (works on both backends)
    exo_cmd_with_storage(root, backend)
        .args([
            "task", "add", "Task 1", "--id", "task-1", "--goal", "goal-1",
        ])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task", "add", "Task 2", "--id", "task-2", "--goal", "goal-2",
        ])
        .assert()
        .success();

    let case = case_task_list_json_is_single_value();
    case.run_all(root);
}

#[test]
fn rfc_status_json_is_single_value() {
    with_repo_root(|root| {
        let case = case_rfc_status_json_is_single_value();
        case.run_all(root);
    });
}

#[test]
fn template_holes_match_and_capture() {
    let template = [
        Fragment::Lit("hello "),
        Fragment::Hole(HoleKind::Uuid),
        Fragment::Lit(" @ "),
        Fragment::Hole(HoleKind::Rfc3339),
    ];

    let input = "hello 550e8400-e29b-41d4-a716-446655440000 @ 2025-12-19T12:34:56Z";
    let caps = match_template(&template, input).expect("template should match");
    assert_eq!(caps.len(), 2);
    assert_eq!(caps[0].kind, HoleKind::Uuid);
    assert_eq!(caps[1].kind, HoleKind::Rfc3339);
}

#[test]
fn template_regex_hole_rejects() {
    let template = [
        Fragment::Lit("x="),
        Fragment::Hole(HoleKind::Regex(r"^\\d+$")),
    ];

    let err = match_template(&template, "x=abc").unwrap_err();
    assert!(err.message.contains("did not match"));
}
