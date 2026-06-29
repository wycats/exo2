//! Tests for `--format json` on structured IO + plan review.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage};

static REPO_ROOT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn with_repo_root<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    let _guard = REPO_ROOT_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("repo-root test lock");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    f(&root)
}

fn write_minimal_plan(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
}

#[test_matrix(["sqlite"])]
fn toml_read_format_json_emits_valid_json(backend: &str) {
    // Use exosuit.toml which always exists at the repo root.
    let assert = with_repo_root(|repo_root| {
        exo_cmd_with_storage(repo_root, backend)
            .args([
                "--format",
                "json",
                "toml",
                "read",
                "exosuit.toml",
                "--key",
                "tasks",
            ])
            .assert()
            .success()
    });
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );

    // Must be valid JSON envelope with result.
    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected valid json"
    );

    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );
    assert!(value.get("result").is_some());
    // The "tasks" key in exosuit.toml is an object
    assert!(value.get("result").and_then(|v| v.as_object()).is_some());
}

#[test_matrix(["sqlite"])]
fn plan_review_format_json_emits_valid_json(backend: &str) {
    let assert = with_repo_root(|repo_root| {
        exo_cmd_with_storage(repo_root, backend)
            .args(["--format", "json", "plan", "review"])
            .assert()
            .success()
    });
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );

    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected valid json"
    );

    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );

    assert_eq!(
        value
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("plan.review")
    );
    assert!(
        value
            .get("result")
            .and_then(|r| r.get("progress_heuristic"))
            .is_some()
    );
}

#[test_matrix(["sqlite"])]
fn json_read_format_json_emits_valid_json(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let json_path = temp.path().join("sample.json");
    assert!(fs::write(&json_path, r#"{ "a": [1, 2, 3] }"#).is_ok());
    let json_path_str = json_path.to_string_lossy();

    let assert = with_repo_root(|repo_root| {
        exo_cmd_with_storage(repo_root, backend)
            .args([
                "--format",
                "json",
                "json",
                "read",
                json_path_str.as_ref(),
                "--pointer",
                "/a",
            ])
            .assert()
            .success()
    });
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );

    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected valid json"
    );

    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );

    let result = some_or_return!(value.get("result"), "expected result");
    let arr = some_or_return!(result.as_array(), "expected array result");
    assert_eq!(arr.len(), 3);
}

#[test_matrix(["sqlite"])]
fn json_write_format_json_emits_valid_json(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    exo_init_with_storage(temp.path(), backend);
    let json_path = temp.path().join("out.json");
    assert!(fs::write(&json_path, "{}").is_ok());
    let json_path_str = json_path.to_string_lossy();

    let assert = exo_cmd_with_storage(temp.path(), backend)
        .args([
            "--format",
            "json",
            "json",
            "write",
            json_path_str.as_ref(),
            "/a",
            "123",
        ])
        .assert()
        .success();
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );

    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected valid json"
    );

    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );
    assert_eq!(
        value
            .get("result")
            .and_then(|r| r.get("ok"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test_matrix(["sqlite"])]
fn plan_failure_format_json_includes_steering(backend: &str) {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    write_minimal_plan(root, backend);

    let assert = exo_cmd_with_storage(root, backend)
        .args([
            "--format", "json", "epoch", "add", "--title", "Epoch 2", "--after", "nope",
        ])
        .assert()
        .failure();

    // For json format, the CLI emits a single machine-consumable object on stdout.
    let stdout = ok_or_return!(
        String::from_utf8(assert.get_output().stdout.clone()),
        "expected utf8 stdout"
    );
    let value = ok_or_return!(
        serde_json::from_str::<serde_json::Value>(&stdout),
        "expected valid json"
    );

    assert_eq!(
        value
            .get("protocol_version")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("status").and_then(serde_json::Value::as_str),
        Some("error")
    );
    // JSON CLI preserves the same semantic error code as the machine channel.
    assert_eq!(
        value
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(serde_json::Value::as_str),
        Some("not_found")
    );

    // The CLI transport returns steering as an array of suggested actions,
    // not as a machine-channel style next_call object.
    let steering = some_or_return!(value.get("steering"), "expected steering");
    assert!(
        steering.is_array(),
        "expected steering to be an array of suggested actions"
    );
    assert!(
        !steering.as_array().map_or(true, |a| a.is_empty()),
        "expected at least one steering suggestion"
    );
}
