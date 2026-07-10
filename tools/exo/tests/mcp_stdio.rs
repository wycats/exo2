mod test_support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use exo::project::Project;
use serde_json::{Value as JsonValue, json};
#[cfg(unix)]
use std::path::{Path, PathBuf};

fn git_config_identity(root: &std::path::Path) {
    for args in [
        ["config", "user.email", "exo-tests@example.invalid"],
        ["config", "user.name", "Exo Tests"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git config");
        assert!(
            output.status.success(),
            "git config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn git_init(root: &std::path::Path) {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_config_identity(root);
    let output = Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(root)
        .output()
        .expect("run git branch");
    assert!(
        output.status.success(),
        "git branch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_status_porcelain(root: &std::path::Path) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .expect("run git status");
    assert!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git status stdout is utf-8")
}

fn git_output(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout is utf-8")
}

fn git_success(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_sidecar_baseline(root: &std::path::Path) {
    git_success(root, &["add", "-A"]);
    git_success(
        root,
        &["commit", "--allow-empty", "-m", "Baseline sidecar state"],
    );
}

fn sidecar_write_owner_marker_path(
    sidecar_root: &std::path::Path,
    key: &str,
) -> std::path::PathBuf {
    let mut sanitized = String::new();
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }
    let sanitized = sanitized.trim_matches('-');
    let prefix = if sanitized.is_empty() {
        "sidecar"
    } else {
        sanitized
    };
    let digest = blake3::hash(key.as_bytes()).to_hex().to_string();
    sidecar_root
        .join(".git/exo-write-owners")
        .join(format!("{prefix}-{}.json", &digest[..12]))
}

fn write_sidecar_write_owner_marker(
    sidecar_root: &std::path::Path,
    key: &str,
    pid: u32,
    workspace_root: &std::path::Path,
) {
    let state_root = sidecar_root.join("projects").join(key);
    let marker_path = sidecar_write_owner_marker_path(sidecar_root, key);
    std::fs::create_dir_all(marker_path.parent().expect("marker parent"))
        .expect("create owner marker dir");
    let machine = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let marker = json!({
        "version": 1,
        "sidecar_key": key,
        "sidecar_root": sidecar_root,
        "workspace_root": workspace_root,
        "state_root": state_root,
        "db_path": state_root.join("cache/exo.db"),
        "runtime_dir": state_root.join("runtime"),
        "pid": pid,
        "executable_path": null,
        "executable_blake3": null,
        "process_start_id": null,
        "machine": machine,
        "acquired_at_ms": 1,
        "refreshed_at_ms": 1,
    });
    std::fs::write(
        marker_path,
        serde_json::to_string_pretty(&marker).expect("serialize owner marker"),
    )
    .expect("write owner marker");
}

fn append_task(root: &std::path::Path, name: &str, cmd: &str) {
    let config_path = root.join("exosuit.toml");
    let mut config = std::fs::read_to_string(&config_path).expect("read exosuit.toml");
    config.push_str("\n[tasks.");
    config.push_str(name);
    config.push_str("]\n");
    config.push_str("cmd = ");
    config.push_str(&toml::Value::String(cmd.to_string()).to_string());
    config.push('\n');
    config.push_str("desc = \"test task\"\n");
    config.push_str("cwd = \"root\"\n");
    std::fs::write(config_path, config).expect("write exosuit.toml");
}

fn print_ok_command() -> &'static str {
    if cfg!(windows) {
        "echo|set /p dummy=ok"
    } else {
        "printf ok"
    }
}

fn exit_success_command() -> std::process::Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        command
    } else {
        Command::new("true")
    }
}

fn write_malformed_rfc_anchor_fixture(root: &std::path::Path) {
    for (id, title) in [
        ("0001", "Interface Compatibility"),
        ("0002", "Self Contained Infrastructure"),
        ("0003", "Local HTTPS"),
    ] {
        test_support::exo_cmd(root)
            .args([
                "rfc",
                "create",
                title,
                "--id",
                id,
                "--stage",
                "0",
                "--feature",
                "rfc",
            ])
            .assert()
            .success();
    }
    let path = root.join("docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md");
    std::fs::write(
        path,
        "<!-- exo:1 -->\n\n# RFC 4: Local v0 Rehearsal Contract\n\nBody.\n",
    )
    .expect("write malformed RFC anchor fixture");
}

fn write_message(stdin: &mut impl Write, message: JsonValue) {
    let line = serde_json::to_string(&message).expect("serialize MCP message");
    writeln!(stdin, "{line}").expect("write MCP message");
    stdin.flush().expect("flush MCP message");
}

fn read_message(stdout: &mut impl BufRead) -> JsonValue {
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read MCP message");
    assert!(!line.is_empty(), "expected MCP response line");
    serde_json::from_str(&line).expect("parse MCP response")
}

fn tool_text(call: &JsonValue) -> &str {
    call["result"]["content"][0]["text"]
        .as_str()
        .expect("text content")
}

fn assert_no_structured_content(call: &JsonValue) {
    assert!(
        call["result"].get("structuredContent").is_none(),
        "ordinary MCP result should be text-only: {call}"
    );
}

fn structured_content(call: &JsonValue) -> &JsonValue {
    call["result"]
        .get("structuredContent")
        .expect("structuredContent")
}

fn call_exo_run(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: impl Into<JsonValue>,
    command: &str,
) -> JsonValue {
    write_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": id.into(),
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": command }
            }
        }),
    );
    read_message(stdout)
}

fn prepare_projection_only_workspace(
    root: &std::path::Path,
    epoch_title: &str,
) -> std::path::PathBuf {
    git_init(root);
    test_support::exo_init_with_storage(root, "sqlite");
    test_support::exo_cmd(root)
        .args(["epoch", "add", "--title", epoch_title])
        .assert()
        .success();
    exo::context::write_sql_dump(root);

    let db_path = Project::resolve(root).expect("resolve project").db_path();
    std::fs::remove_file(&db_path).expect("delete project db");
    assert!(!db_path.exists(), "project db should start absent");
    assert!(root.join("docs/agent-context/epochs.sql").exists());
    db_path
}

fn call_proxy_status(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: impl Into<JsonValue>,
) -> JsonValue {
    write_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": id.into(),
            "method": "exo/proxy/status",
        }),
    );
    read_message(stdout)
}

#[cfg(unix)]
fn call_initialize(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: impl Into<JsonValue>,
) -> JsonValue {
    write_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": id.into(),
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    read_message(stdout)
}

fn initialize_mcp_server(
    root: &std::path::Path,
) -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut child = spawn_mcp_server(root);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    let initialize = read_message(&mut stdout);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION
    );
    assert!(initialize["result"]["capabilities"]["tools"].is_object());

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );

    (child, stdin, stdout)
}

fn spawn_mcp_server(root: &std::path::Path) -> std::process::Child {
    spawn_mcp_server_with_env(root, [])
}

fn spawn_exo_mcp_proxy(root: &std::path::Path) -> std::process::Child {
    spawn_exo_mcp_proxy_with_env(root, [])
}

fn spawn_exo_mcp_proxy_with_env<const N: usize>(
    root: &std::path::Path,
    envs: [(&str, &std::path::Path); N],
) -> std::process::Child {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("exo-mcp"));
    command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in envs {
        command.env(name, value);
    }
    command.spawn().expect("spawn exo-mcp")
}

#[test]
fn exo_mcp_proxy_health_reports_invalid_dogfood_activation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let activation = temp.path().join("activation.json");
    std::fs::write(&activation, "not json").expect("write invalid activation");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("exo-mcp"))
        .arg("--proxy-health")
        .current_dir(temp.path())
        .env(exo::dogfood_activation::DOGFOOD_ACTIVATION_ENV, &activation)
        .output()
        .expect("run proxy health");
    assert!(output.status.success());

    let health: JsonValue = serde_json::from_slice(&output.stdout).expect("proxy health JSON");
    assert_eq!(health["kind"], "exo-mcp.proxy-health");
    assert_eq!(health["ok"], false);
    assert_eq!(health["status"]["activation"]["configured"], true);
    assert_eq!(health["status"]["activation"]["ok"], false);
    assert_eq!(
        health["status"]["activation"]["state"],
        "invalid_activation"
    );
    assert!(health["status"]["activation"].get("path").is_none());
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_health_reaps_worker_rejected_by_activation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source_worker = temp.path().join("source-exo");
    std::fs::write(&source_worker, "expected source worker").expect("write source worker");
    let exo = assert_cmd::cargo::cargo_bin!("exo");
    let exo_mcp = assert_cmd::cargo::cargo_bin!("exo-mcp");
    let activation = temp.path().join("activation.json");
    let binary = |path: &std::path::Path| {
        serde_json::json!({
            "path": path,
            "blake3": "unused-by-this-fixture",
            "size_bytes": 0,
            "modified_unix_ms": null,
        })
    };
    std::fs::write(
        &activation,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "source": {
                "exo": binary(&source_worker),
                "exo_mcp": binary(&exo_mcp),
            },
            "installed": {
                "exo": binary(&exo),
                "exo_mcp": binary(&exo_mcp),
            },
        }))
        .expect("serialize activation"),
    )
    .expect("write activation");

    let output = Command::new(&exo_mcp)
        .arg("--proxy-health")
        .current_dir(temp.path())
        .env(exo::dogfood_activation::DOGFOOD_ACTIVATION_ENV, &activation)
        .env("EXO_MCP_WORKER", &exo)
        .output()
        .expect("run proxy health");
    assert!(output.status.success());

    let health: JsonValue = serde_json::from_slice(&output.stdout).expect("proxy health JSON");
    assert_eq!(health["ok"], false, "{health}");
    assert!(
        health["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("does not match the current source build")),
        "{health}"
    );
    assert!(health["status"]["worker"].is_null(), "{health}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_revalidates_activation_after_worker_hot_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exo-worker");
    let source_worker = temp.path().join("activation-source-exo");
    install_exo_worker_binary(&worker);
    std::fs::copy(&worker, &source_worker).expect("copy activation source worker");
    let permissions = std::fs::metadata(&worker)
        .expect("worker metadata")
        .permissions();
    std::fs::set_permissions(&source_worker, permissions).expect("set source worker permissions");

    let exo = assert_cmd::cargo::cargo_bin!("exo");
    let exo_mcp = assert_cmd::cargo::cargo_bin!("exo-mcp");
    let activation = temp.path().join("activation.json");
    write_dogfood_activation(&activation, &source_worker, &exo_mcp, &exo, &exo_mcp);

    let mut child = spawn_exo_mcp_proxy_with_env(
        temp.path(),
        [
            ("EXO_MCP_WORKER", worker.as_path()),
            (
                exo::dogfood_activation::DOGFOOD_ACTIVATION_ENV,
                activation.as_path(),
            ),
        ],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initial = call_exo_run(&mut stdin, &mut stdout, 1, "status");
    assert_eq!(initial["result"]["isError"], false, "{initial}");

    std::fs::write(&source_worker, "updated source activation identity")
        .expect("change source activation identity");
    install_exo_worker_binary(&worker);

    let restarted = call_exo_run(&mut stdin, &mut stdout, 2, "status");
    assert_eq!(restarted["error"]["code"], -32000, "{restarted}");
    let status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-activation-failure");
    assert!(status["result"]["worker"].is_null(), "{status}");
    assert!(
        status["result"]["last_error"]
            .as_str()
            .is_some_and(|error| error.contains("does not match the current source build")),
        "{status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_reloads_replaced_activation_record() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exo-worker");
    let first_source = temp.path().join("first-source-exo");
    let second_source = temp.path().join("second-source-exo");
    install_exo_worker_binary(&worker);
    for source in [&first_source, &second_source] {
        std::fs::copy(&worker, source).expect("copy source worker");
        let permissions = std::fs::metadata(&worker)
            .expect("worker metadata")
            .permissions();
        std::fs::set_permissions(source, permissions).expect("set source worker permissions");
    }

    let exo = assert_cmd::cargo::cargo_bin!("exo");
    let exo_mcp = assert_cmd::cargo::cargo_bin!("exo-mcp");
    let activation = temp.path().join("activation.json");
    write_dogfood_activation(&activation, &first_source, &exo_mcp, &exo, &exo_mcp);

    let mut child = spawn_exo_mcp_proxy_with_env(
        temp.path(),
        [
            ("EXO_MCP_WORKER", worker.as_path()),
            (
                exo::dogfood_activation::DOGFOOD_ACTIVATION_ENV,
                activation.as_path(),
            ),
        ],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initial = call_exo_run(&mut stdin, &mut stdout, 1, "status");
    assert_eq!(initial["result"]["isError"], false, "{initial}");

    std::fs::write(&first_source, "stale source activation identity")
        .expect("change first source identity");
    write_dogfood_activation(&activation, &second_source, &exo_mcp, &exo, &exo_mcp);

    let status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-activation-refresh");
    assert_eq!(
        status["result"]["activation"]["state"], "current",
        "{status}"
    );
    assert_eq!(status["result"]["activation"]["ok"], true, "{status}");

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
fn write_dogfood_activation(
    path: &std::path::Path,
    source_exo: &std::path::Path,
    source_mcp: &std::path::Path,
    installed_exo: &std::path::Path,
    installed_mcp: &std::path::Path,
) {
    let binary = |path: &std::path::Path| {
        serde_json::json!({
            "path": path,
            "blake3": "unused-by-this-fixture",
            "size_bytes": 0,
            "modified_unix_ms": null,
        })
    };
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "source": {
                "exo": binary(source_exo),
                "exo_mcp": binary(source_mcp),
            },
            "installed": {
                "exo": binary(installed_exo),
                "exo_mcp": binary(installed_mcp),
            },
        }))
        .expect("serialize activation"),
    )
    .expect("write activation");
}

#[cfg(unix)]
fn install_exo_worker_binary(path: &std::path::Path) {
    let source = assert_cmd::cargo::cargo_bin!("exo");
    if path.exists() {
        std::fs::remove_file(path).expect("remove old worker binary");
    }
    std::fs::copy(&source, path).expect("copy worker binary");
    let permissions = std::fs::metadata(&source)
        .expect("source worker metadata")
        .permissions();
    std::fs::set_permissions(path, permissions).expect("copy worker permissions");
    wait_until_executable(path);
}

#[cfg(unix)]
fn wait_until_executable(path: &std::path::Path) {
    let mut last_error = None;
    for _ in 0..50 {
        match Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(_) => return,
            Err(error) if error.raw_os_error() == Some(26) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => panic!("worker binary is not executable: {error}"),
        }
    }
    panic!(
        "worker binary remained text-busy: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    );
}

#[cfg(unix)]
fn install_exo_mcp_proxy_binary(path: &std::path::Path) {
    let source = assert_cmd::cargo::cargo_bin!("exo-mcp");
    if path.exists() {
        std::fs::remove_file(path).expect("remove old proxy binary");
    }
    std::fs::copy(&source, path).expect("copy proxy binary");
    let permissions = std::fs::metadata(&source)
        .expect("source proxy metadata")
        .permissions();
    std::fs::set_permissions(path, permissions).expect("copy proxy permissions");
}

#[cfg(unix)]
fn chmod_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)
        .expect("worker metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod worker");
}

#[cfg(unix)]
fn replace_executable_with_shell_stub(path: &Path) {
    std::fs::remove_file(path).expect("remove executable before replacement");
    std::fs::write(path, "#!/bin/sh\nexit 99\n").expect("write replacement executable");
    chmod_executable(path);
}

#[cfg(unix)]
fn write_stateful_fake_worker(
    root: &Path,
    name: &str,
    effect: &str,
    mode: &str,
) -> (PathBuf, PathBuf) {
    let worker = root.join(name);
    let identity_file = root.join(format!("{name}.identity.json"));
    let call_count_file = root.join(format!("{name}.calls"));
    let classify_count_file = root.join(format!("{name}.classify"));
    let outcome_id_file = root.join(format!("{name}.outcome-id"));
    let script = format!(
        r#"#!/usr/bin/env python3
import json
import pathlib
import sys

IDENTITY_FILE = pathlib.Path({identity_file})
CALL_COUNT_FILE = pathlib.Path({call_count_file})
CLASSIFY_COUNT_FILE = pathlib.Path({classify_count_file})
OUTCOME_ID_FILE = pathlib.Path({outcome_id_file})
SELF_PATH = pathlib.Path(sys.argv[0])
EFFECT = {effect}
MODE = {mode}

def emit(value):
    print(json.dumps(value), flush=True)

def call_count():
    try:
        return int(CALL_COUNT_FILE.read_text().strip())
    except Exception:
        return 0

def set_call_count(value):
    CALL_COUNT_FILE.write_text(str(value))

def classify_count():
    try:
        return int(CLASSIFY_COUNT_FILE.read_text().strip())
    except Exception:
        return 0

def set_classify_count(value):
    CLASSIFY_COUNT_FILE.write_text(str(value))

def refresh_identity_file():
    data = json.loads(IDENTITY_FILE.read_text())
    stat = SELF_PATH.stat()
    identity = data["result"]["identity"]["executable_identity"]
    identity["len"] = stat.st_size
    identity["modified_unix_ms"] = int(stat.st_mtime_ns / 1_000_000)
    identity["dev"] = stat.st_dev
    identity["ino"] = stat.st_ino
    identity["changed_unix_s"] = int(stat.st_ctime_ns / 1_000_000_000)
    identity["changed_unix_ns"] = int(stat.st_ctime_ns % 1_000_000_000)
    IDENTITY_FILE.write_text(json.dumps(data))

for raw_line in sys.stdin:
    if not raw_line.strip():
        continue
    message = json.loads(raw_line)
    method = message.get("method")
    request_id = message.get("id")
    if method == "worker/hello":
        print(IDENTITY_FILE.read_text(), flush=True)
    elif method == "worker/listTools":
        if MODE == "exit_list_first_then_success" and call_count() == 0:
            set_call_count(1)
            sys.exit(0)
        if MODE == "wrong_id":
            request_id = "wrong-worker-response-id"
        emit({{
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {{
                "tools": [{{
                    "name": "exo-run",
                    "description": "fake exo-run",
                    "inputSchema": {{"type": "object"}}
                }}]
            }}
        }})
    elif method == "worker/classify":
        count = classify_count() + 1
        set_classify_count(count)
        if MODE == "exit_classify_first_then_success" and count == 1:
            sys.exit(0)
        if MODE == "exit_classify_after_first_call" and call_count() > 0:
            sys.exit(0)
        if MODE == "mutate_self_after_first_classify" and count == 1:
            SELF_PATH.write_text(SELF_PATH.read_text() + "\n# replaced after classify\n")
            refresh_identity_file()
        if MODE == "tool_error_classification" or (
            MODE == "tool_error_after_first_call" and call_count() > 0
        ):
            emit({{
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {{
                    "tool_name": "exo-run",
                    "classification": "tool_error",
                    "effect": "pure",
                    "retry_policy": "no_retry",
                    "requires_confirmation": False,
                    "request_summary": {{
                        "tool_name": "exo-run",
                        "command": message.get("params", {{}}).get("arguments", {{}}).get("command", "")
                    }},
                    "has_auth": False,
                    "has_workflow_confirmation": False,
                    "tool_result": {{
                        "content": [{{"type": "text", "text": "classified tool error"}}],
                        "isError": True
                    }},
                    "tool_schema_identity": "fake-tool-schema",
                    "command_spec_identity": "fake-command-spec"
                }}
            }})
            continue
        if MODE == "classify_error":
            emit({{
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {{
                    "code": -32602,
                    "message": "fake classification failure"
                }}
            }})
            continue
        arguments = message.get("params", {{}}).get("arguments", {{}})
        command = arguments.get("command", "")
        effect = EFFECT
        if MODE == "mutate_self_after_first_classify" and count > 1:
            effect = "exec"
        emit({{
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {{
                "tool_name": "exo-run",
                "effect": effect,
                "retry_policy": "auto_retry_read" if effect == "pure" else "auto_recover_outcome",
                "requires_confirmation": effect == "exec",
                "request_summary": {{
                    "tool_name": "exo-run",
                    "command": command
                }},
                "has_auth": "auth" in arguments,
                "has_workflow_confirmation": "workflowConfirmation" in arguments,
                "tool_schema_identity": "fake-tool-schema",
                "command_spec_identity": "fake-command-spec"
            }}
        }})
    elif method == "worker/call":
        count = call_count() + 1
        set_call_count(count)
        outcome_id = message.get("params", {{}}).get("_exo_outcome_request_id", "")
        if MODE == "exit_first_then_success" and count == 1:
            OUTCOME_ID_FILE.write_text(outcome_id)
            sys.exit(0)
        if MODE == "exit_first_then_success" and (
            not outcome_id or OUTCOME_ID_FILE.read_text() != outcome_id
        ):
            emit({{
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {{
                    "content": [{{"type": "text", "text": "outcome request id changed"}}],
                    "isError": True
                }}
            }})
            continue
        if MODE == "exit_classify_after_first_call" and count == 1:
            sys.exit(0)
        if MODE == "tool_error_after_first_call" and count == 1:
            sys.exit(0)
        if MODE == "exit_always":
            sys.exit(0)
        if MODE == "wrong_id_on_call":
            request_id = "wrong-worker-call-id"
        emit({{
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {{
                "content": [{{"type": "text", "text": "retried pure read"}}],
                "isError": False
            }}
        }})
    else:
        emit({{
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {{
                "code": -32601,
                "message": "fake worker method not found"
            }}
        }})
"#,
        identity_file = serde_json::to_string(&identity_file).expect("identity path json"),
        call_count_file = serde_json::to_string(&call_count_file).expect("call count path json"),
        classify_count_file =
            serde_json::to_string(&classify_count_file).expect("classify count path json"),
        outcome_id_file = serde_json::to_string(&outcome_id_file).expect("outcome id path json"),
        effect = serde_json::to_string(effect).expect("effect json"),
        mode = serde_json::to_string(mode).expect("mode json"),
    );
    std::fs::write(&worker, script).expect("write fake worker");
    chmod_executable(&worker);

    let executable_identity =
        exo::mcp::executable_identity_for_path(&worker).expect("fake worker identity");
    let hello = json!({
        "jsonrpc": "2.0",
        "id": "exo-proxy-worker-hello",
        "result": {
            "identity": {
                "executable_path": worker,
                "executable_identity": executable_identity,
                "worker_protocol_version": exo::mcp::MCP_WORKER_PROTOCOL_VERSION,
                "tool_schema_identity": "fake-tool-schema",
                "command_spec_identity": "fake-command-spec"
            }
        }
    });
    std::fs::write(
        &identity_file,
        serde_json::to_string(&hello).expect("serialize fake worker hello"),
    )
    .expect("write fake worker identity");

    (worker, call_count_file)
}

#[cfg(unix)]
fn fake_worker_call_count(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(unix)]
fn fake_worker_classify_count(root: &Path, name: &str) -> u64 {
    fake_worker_call_count(&root.join(format!("{name}.classify")))
}

fn spawn_mcp_server_with_env<const N: usize>(
    root: &std::path::Path,
    envs: [(&str, &std::path::Path); N],
) -> std::process::Child {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("exo"));
    command
        .current_dir(root)
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in envs {
        command.env(name, value);
    }
    command.spawn().expect("spawn exo mcp serve")
}

#[cfg(unix)]
fn spawn_exo_mcp_proxy_binary_with_worker(
    root: &Path,
    proxy: &Path,
    worker: &Path,
) -> std::process::Child {
    Command::new(proxy)
        .current_dir(root)
        .env("EXO_NO_REEXEC", "1")
        .env("EXO_MCP_WORKER", worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn copied exo-mcp")
}

#[test]
fn exo_mcp_proxy_serves_initialize_tools_list_and_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_exo_mcp_proxy(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    let initialize = read_message(&mut stdout);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = read_message(&mut stdout);
    let tool_names = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&"exo-run"), "{tools}");
    let tool_schema = tools["result"]["tools"].clone();

    let status = call_exo_run(&mut stdin, &mut stdout, 3, "status");
    assert_eq!(status["id"], 3, "{status}");
    assert_eq!(status["result"]["isError"], false, "{status}");
    let text = tool_text(&status);
    assert!(text.contains("Epoch:"), "{text}");

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-status");
    assert_eq!(
        proxy_status["result"]["proxy"]["on_disk"]["matches_startup"], true,
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["proxy"].get("stale").is_none(),
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["proxy"]["worker_protocol_version"].is_number(),
        "{proxy_status}"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-after-call",
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools_after_call = read_message(&mut stdout);
    assert_eq!(
        tools_after_call["id"], "tools-after-call",
        "{tools_after_call}"
    );
    assert_eq!(
        tools_after_call["result"]["tools"], tool_schema,
        "{tools_after_call}"
    );

    let task_list = call_exo_run(
        &mut stdin,
        &mut stdout,
        "task-list-after-discovery",
        "task list",
    );
    assert_eq!(task_list["id"], "task-list-after-discovery", "{task_list}");
    assert_eq!(task_list["result"]["isError"], false, "{task_list}");
    assert!(!tool_text(&task_list).trim().is_empty(), "{task_list}");

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_keeps_serving_after_proxy_binary_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let proxy = temp.path().join("exo-mcp");
    install_exo_mcp_proxy_binary(&proxy);
    let worker_name = "stale-proxy-worker.py";
    let (worker, call_count_file) =
        write_stateful_fake_worker(temp.path(), worker_name, "pure", "normal");
    let mut child = spawn_exo_mcp_proxy_binary_with_worker(temp.path(), &proxy, &worker);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initial_status = call_proxy_status(&mut stdin, &mut stdout, "initial-status");
    assert_eq!(
        initial_status["result"]["proxy"]["on_disk"]["matches_startup"], true,
        "{initial_status}"
    );

    replace_executable_with_shell_stub(&proxy);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "stale-tools",
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = read_message(&mut stdout);
    assert_eq!(tools["id"], "stale-tools", "{tools}");
    assert_eq!(tools["result"]["tools"][0]["name"], "exo-run", "{tools}");
    assert!(
        !serde_json::to_string(&tools)
            .expect("serialize tools/list response")
            .contains("exo.proxy_stale"),
        "{tools}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 0);

    let call = call_exo_run(&mut stdin, &mut stdout, "stale-call", "status");
    assert_eq!(call["id"], "stale-call", "{call}");
    assert_eq!(
        call["result"]["content"][0]["text"], "retried pure read",
        "{call}"
    );
    assert!(
        !serde_json::to_string(&call)
            .expect("serialize tools/call response")
            .contains("exo.proxy_stale"),
        "{call}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 1);
    assert_eq!(fake_worker_classify_count(temp.path(), worker_name), 1);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "stale-status");
    assert_eq!(proxy_status["id"], "stale-status", "{proxy_status}");
    assert!(proxy_status.get("error").is_none(), "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["proxy"]["on_disk"]["matches_startup"], false,
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["proxy"].get("stale").is_none(),
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["worker"].is_object(),
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_retries_interrupted_tools_list_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, count_file) = write_stateful_fake_worker(
        temp.path(),
        "list-restart-worker.py",
        "pure",
        "exit_list_first_then_success",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "discover-after-worker-exit",
            "method": "tools/list",
            "params": {}
        }),
    );
    let response = read_message(&mut stdout);
    assert_eq!(response["id"], "discover-after-worker-exit", "{response}");
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["tools"][0]["name"], "exo-run",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&count_file), 1);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-list-retry");
    assert_eq!(proxy_status["result"]["restart_count"], 1, "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["last_restart_reason"], "worker_exited",
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[test]
fn exo_mcp_proxy_returns_tool_errors_for_invalid_exo_commands() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_exo_mcp_proxy(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(
        &mut stdin,
        &mut stdout,
        "bad-command",
        "definitely-not-a-command",
    );
    assert_eq!(response["id"], "bad-command", "{response}");
    assert!(
        response.get("error").is_none(),
        "invalid commands should be MCP tool errors, not JSON-RPC errors: {response}"
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(
        structured_content(&response)["error"]["code"].is_string(),
        "{response}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[test]
fn mcp_worker_classifies_exo_run_effects_from_command_spec() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("exo"))
        .current_dir(temp.path())
        .args(["mcp", "worker"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn exo mcp worker");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "worker/listTools",
        }),
    );
    let tools = read_message(&mut stdout);
    assert_eq!(tools["result"]["tools"][0]["name"], "exo-run", "{tools}");

    for (id, command, effect, retry_policy) in [
        ("pure", "status", "pure", "auto_retry_read"),
        (
            "argument-dependent-exec",
            "dogfood repair --apply",
            "exec",
            "auto_recover_outcome",
        ),
        (
            "write",
            "task add \"Classified write\"",
            "write",
            "auto_recover_outcome",
        ),
        (
            "exec",
            "strike start --name classify-demo --goal \"Classified exec\"",
            "exec",
            "auto_recover_outcome",
        ),
    ] {
        write_message(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "worker/classify",
                "params": {
                    "name": "exo-run",
                    "arguments": { "command": command }
                }
            }),
        );
        let classification = read_message(&mut stdout);
        assert_eq!(classification["id"], id, "{classification}");
        assert_eq!(
            classification["result"]["effect"], effect,
            "{classification}"
        );
        assert_eq!(
            classification["result"]["retry_policy"], retry_policy,
            "{classification}"
        );
        assert_eq!(
            classification["result"]["request_summary"]["command"], command,
            "{classification}"
        );
        assert!(
            classification["result"]["tool_schema_identity"].is_string(),
            "{classification}"
        );
        assert!(
            classification["result"]["command_spec_identity"].is_string(),
            "{classification}"
        );
    }

    drop(stdin);
    let status = child.wait().expect("wait for exo mcp worker");
    assert!(status.success(), "exo mcp worker exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_blocks_external_worker_methods() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) =
        write_stateful_fake_worker(temp.path(), "boundary-worker.py", "pure", "normal");
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "external-worker-call",
            "method": "worker/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "status" }
            }
        }),
    );
    let response = read_message(&mut stdout);
    assert_eq!(response["id"], "external-worker-call", "{response}");
    assert_eq!(response["error"]["code"], -32601, "{response}");
    assert_eq!(
        response["error"]["data"]["reason"], "worker methods are internal to the Exo MCP proxy",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 0);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_returns_classified_tool_errors_without_calling_worker() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker_name = "tool-error-worker.py";
    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        worker_name,
        "pure",
        "tool_error_classification",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 92, "task add from-changed-state");
    assert_eq!(response["id"], 92, "{response}");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(tool_text(&response), "classified tool error");
    assert_eq!(fake_worker_call_count(&call_count_file), 0);
    assert_eq!(fake_worker_classify_count(temp.path(), worker_name), 1);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_reclassifies_when_worker_is_replaced_before_call() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker_name = "replace-before-call-worker.py";
    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        worker_name,
        "pure",
        "mutate_self_after_first_classify",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 93, "status");
    assert_eq!(response["id"], 93, "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(fake_worker_classify_count(temp.path(), worker_name), 2);
    assert_eq!(fake_worker_call_count(&call_count_file), 1);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-reclassify");
    assert_eq!(proxy_status["result"]["restart_count"], 1, "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["last_restart_reason"], "worker_binary_changed",
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_retry_required_reports_protocol_failures_distinctly() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        "wrong-call-id-worker.py",
        "write",
        "wrong_id_on_call",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 94, "task add protocol-failure");
    assert_eq!(response["id"], 94, "{response}");
    assert_eq!(
        response["error"]["data"]["code"], "exo.retry_required",
        "{response}"
    );
    assert_eq!(
        response["error"]["data"]["worker_restart_reason"], "worker_protocol_error",
        "{response}"
    );
    assert_eq!(
        response["error"]["data"]["request_state"], "may_have_started",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 2);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-protocol-failure");
    assert_eq!(
        proxy_status["result"]["pending_restart_reason"], "worker_protocol_error",
        "{proxy_status}"
    );
    assert!(proxy_status["result"]["worker"].is_null(), "{proxy_status}");
    assert!(
        proxy_status["result"]["last_error"]
            .as_str()
            .expect("last error")
            .contains("response id mismatch"),
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_retries_interrupted_classification_before_calling() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        "classify-restart-worker.py",
        "write",
        "exit_classify_first_then_success",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(
        &mut stdin,
        &mut stdout,
        77,
        "task add \"After classify restart\"",
    );
    assert_eq!(response["id"], 77, "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(tool_text(&response), "retried pure read");
    assert_eq!(fake_worker_call_count(&call_count_file), 1);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-classify-retry");
    assert_eq!(proxy_status["result"]["restart_count"], 1, "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["last_restart_reason"], "worker_exited",
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_rejects_worker_response_id_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, _call_count_file) =
        write_stateful_fake_worker(temp.path(), "wrong-id-worker.py", "pure", "wrong_id");
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "client-tools",
            "method": "tools/list",
        }),
    );
    let response = read_message(&mut stdout);
    assert_eq!(response["id"], "client-tools", "{response}");
    assert_eq!(response["error"]["code"], -32000, "{response}");
    assert!(
        response["error"]["data"]["message"]
            .as_str()
            .expect("error message")
            .contains("response id mismatch"),
        "{response}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_retries_interrupted_pure_worker_call_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        "pure-worker.py",
        "pure",
        "exit_first_then_success",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 42, "status");
    assert_eq!(response["id"], 42, "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(tool_text(&response), "retried pure read");
    assert_eq!(fake_worker_call_count(&call_count_file), 2);

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-pure-retry");
    assert_eq!(proxy_status["result"]["restart_count"], 1, "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["last_restart_reason"], "worker_exited",
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["last_error"].is_null(),
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_returns_retry_required_when_pure_retry_is_interrupted() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        "pure-retry-fails-worker.py",
        "pure",
        "exit_always",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 96, "status");
    assert_eq!(response["id"], 96, "{response}");
    assert_eq!(
        response["error"]["data"]["code"], "exo.retry_required",
        "{response}"
    );
    assert_eq!(response["error"]["data"]["effect"], "pure", "{response}");
    assert_eq!(
        response["error"]["data"]["worker_restart_reason"], "worker_exited",
        "{response}"
    );
    assert_eq!(
        response["error"]["data"]["request_state"], "may_have_started",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 2);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_returns_retry_required_when_retry_reclassification_is_interrupted() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker_name = "pure-retry-classify-fails-worker.py";
    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        worker_name,
        "pure",
        "exit_classify_after_first_call",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 97, "status");
    assert_eq!(response["id"], 97, "{response}");
    assert_eq!(
        response["error"]["data"]["code"], "exo.retry_required",
        "{response}"
    );
    assert_eq!(response["error"]["data"]["effect"], "pure", "{response}");
    assert_eq!(
        response["error"]["data"]["worker_restart_reason"], "worker_exited",
        "{response}"
    );
    assert_eq!(
        response["error"]["data"]["request_state"], "may_have_started",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 1);
    assert_eq!(fake_worker_classify_count(temp.path(), worker_name), 2);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_returns_retry_classified_tool_error_without_calling_again() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker_name = "retry-tool-error-worker.py";
    let (worker, call_count_file) = write_stateful_fake_worker(
        temp.path(),
        worker_name,
        "pure",
        "tool_error_after_first_call",
    );
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 95, "status");
    assert_eq!(response["id"], 95, "{response}");
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(tool_text(&response), "classified tool error");
    assert_eq!(fake_worker_call_count(&call_count_file), 1);
    assert_eq!(fake_worker_classify_count(temp.path(), worker_name), 2);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_recovers_interrupted_write_and_exec_worker_calls() {
    for (effect, command) in [
        ("write", "task complete secret-task --log Done"),
        ("exec", "strike start --name secret-strike --goal Done"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        git_init(temp.path());
        test_support::exo_init_with_storage(temp.path(), "sqlite");

        let (worker, call_count_file) = write_stateful_fake_worker(
            temp.path(),
            &format!("{effect}-worker.py"),
            effect,
            "exit_first_then_success",
        );
        let mut child =
            spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
        let mut stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut stdout = BufReader::new(stdout);

        write_message(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": effect,
                "method": "tools/call",
                "params": {
                    "name": "exo-run",
                    "arguments": {
                        "command": command,
                        "auth": {
                            "ticket": "secret-ticket",
                            "confirm": true
                        },
                        "workflowConfirmation": {
                            "kind": "workflow_completion_confirmation",
                            "entityType": "task",
                            "entityId": "secret-task",
                            "decision": "yes_complete",
                            "outcome": "SECRET-WORKFLOW-OUTCOME"
                        }
                    }
                }
            }),
        );
        let response = read_message(&mut stdout);
        assert_eq!(response["id"], effect, "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(tool_text(&response), "retried pure read");
        assert_eq!(fake_worker_call_count(&call_count_file), 2);

        let serialized = serde_json::to_string(&response).expect("serialize response");
        assert!(!serialized.contains("secret-ticket"), "{serialized}");
        assert!(
            !serialized.contains("SECRET-WORKFLOW-OUTCOME"),
            "{serialized}"
        );

        drop(stdin);
        let status = child.wait().expect("wait for exo-mcp");
        assert!(status.success(), "exo-mcp exited with {status}");
    }
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_does_not_execute_when_classification_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (worker, call_count_file) =
        write_stateful_fake_worker(temp.path(), "classify-worker.py", "pure", "classify_error");
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_exo_run(&mut stdin, &mut stdout, 91, "status");
    assert_eq!(response["id"], 91, "{response}");
    assert_eq!(
        response["error"]["message"], "fake classification failure",
        "{response}"
    );
    assert_eq!(fake_worker_call_count(&call_count_file), 0);

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_restarts_worker_when_worker_binary_changes_between_calls() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exo-worker");
    install_exo_worker_binary(&worker);

    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    let initialize = read_message(&mut stdout);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION
    );

    let status = call_exo_run(&mut stdin, &mut stdout, 2, "status");
    assert_eq!(status["result"]["isError"], false, "{status}");

    let before = call_proxy_status(&mut stdin, &mut stdout, "proxy-before");
    assert_eq!(before["result"]["restart_count"], 0, "{before}");
    assert_eq!(
        before["result"]["last_restart_reason"], "initial_start",
        "{before}"
    );
    assert!(
        before["result"]["proxy"]["executable_path"].is_string(),
        "{before}"
    );
    assert!(
        before["result"]["proxy"]["executable_identity"]["stable_hash"].is_string(),
        "{before}"
    );
    assert_eq!(
        before["result"]["worker"]["identity"]["executable_path"],
        worker.to_string_lossy().as_ref(),
        "{before}"
    );
    assert!(
        before["result"]["worker"]["identity"]["executable_identity"]["stable_hash"].is_string(),
        "{before}"
    );
    assert!(
        before["result"]["worker"]["identity"]["tool_schema_identity"].is_string(),
        "{before}"
    );
    assert!(
        before["result"]["worker"]["identity"]["command_spec_identity"].is_string(),
        "{before}"
    );

    install_exo_worker_binary(&worker);

    let after_replace = call_exo_run(&mut stdin, &mut stdout, 3, "status");
    assert_eq!(after_replace["result"]["isError"], false, "{after_replace}");

    let after = call_proxy_status(&mut stdin, &mut stdout, "proxy-after");
    assert_eq!(after["result"]["restart_count"], 1, "{after}");
    assert_eq!(
        after["result"]["last_restart_reason"], "worker_binary_changed",
        "{after}"
    );
    assert_eq!(
        after["result"]["worker"]["identity"]["executable_path"],
        worker.to_string_lossy().as_ref(),
        "{after}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_refreshes_worker_identity_before_proxy_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exo-worker");
    install_exo_worker_binary(&worker);

    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initialize = call_initialize(&mut stdin, &mut stdout, 1);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION,
        "{initialize}"
    );

    let before = call_proxy_status(&mut stdin, &mut stdout, "proxy-before");
    assert_eq!(before["result"]["restart_count"], 0, "{before}");
    assert_eq!(
        before["result"]["last_restart_reason"], "initial_start",
        "{before}"
    );

    install_exo_worker_binary(&worker);

    let after = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-replace");
    assert_eq!(after["result"]["restart_count"], 1, "{after}");
    assert_eq!(
        after["result"]["last_restart_reason"], "worker_binary_changed",
        "{after}"
    );
    assert!(after["result"]["last_error"].is_null(), "{after}");

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_detects_worker_symlink_target_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let release_one_dir = temp.path().join("release-one");
    let release_two_dir = temp.path().join("release-two");
    std::fs::create_dir(&release_one_dir).expect("create release one");
    std::fs::create_dir(&release_two_dir).expect("create release two");
    let release_one_worker = release_one_dir.join("exo");
    let release_two_worker = release_two_dir.join("exo");
    install_exo_worker_binary(&release_one_worker);
    install_exo_worker_binary(&release_two_worker);

    let current_worker = temp.path().join("current-exo");
    std::os::unix::fs::symlink(&release_one_worker, &current_worker)
        .expect("symlink first worker release");

    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", current_worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initialize = call_initialize(&mut stdin, &mut stdout, 1);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION,
        "{initialize}"
    );

    let before = call_proxy_status(&mut stdin, &mut stdout, "proxy-before-symlink-swap");
    assert_eq!(before["result"]["restart_count"], 0, "{before}");
    assert_eq!(
        before["result"]["last_restart_reason"], "initial_start",
        "{before}"
    );

    std::fs::remove_file(&current_worker).expect("remove current worker symlink");
    std::os::unix::fs::symlink(&release_two_worker, &current_worker)
        .expect("symlink second worker release");

    let after = call_exo_run(&mut stdin, &mut stdout, 2, "status");
    assert_eq!(after["result"]["isError"], false, "{after}");

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-symlink-swap");
    assert_eq!(proxy_status["result"]["restart_count"], 1, "{proxy_status}");
    assert_eq!(
        proxy_status["result"]["last_restart_reason"], "worker_binary_changed",
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["last_error"].is_null(),
        "{proxy_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_records_missing_worker_and_attributes_later_success() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("missing-then-present-worker");
    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let failed_initialize = call_initialize(&mut stdin, &mut stdout, 1);
    assert_eq!(failed_initialize["id"], 1);
    assert_eq!(failed_initialize["error"]["code"], -32000);

    let failed_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-spawn-failure");
    assert!(failed_status["result"]["last_restart_reason"].is_null());
    assert_eq!(
        failed_status["result"]["pending_restart_reason"], "initial_start",
        "{failed_status}"
    );
    assert!(
        failed_status["result"]["last_error"].is_string(),
        "{failed_status}"
    );
    assert!(
        failed_status["result"]["worker"].is_null(),
        "{failed_status}"
    );

    install_exo_worker_binary(&worker);

    let initialize = call_initialize(&mut stdin, &mut stdout, 2);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION,
        "{initialize}"
    );

    let recovered_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-recovery");
    assert_eq!(
        recovered_status["result"]["last_restart_reason"], "initial_start",
        "{recovered_status}"
    );
    assert!(
        recovered_status["result"]["last_error"].is_null(),
        "{recovered_status}"
    );
    assert_eq!(
        recovered_status["result"]["worker"]["identity"]["executable_path"],
        worker.to_string_lossy().as_ref(),
        "{recovered_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_records_failed_hot_restart_reason_until_recovery() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exo-worker");
    install_exo_worker_binary(&worker);

    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initialize = call_initialize(&mut stdin, &mut stdout, 1);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION,
        "{initialize}"
    );

    let status = call_exo_run(&mut stdin, &mut stdout, 2, "status");
    assert_eq!(status["result"]["isError"], false, "{status}");

    std::fs::remove_file(&worker).expect("remove worker before hot restart");
    let failed_after_remove = call_exo_run(&mut stdin, &mut stdout, 3, "status");
    assert_eq!(failed_after_remove["error"]["code"], -32000);

    let failed_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-hot-restart-failed");
    assert_eq!(
        failed_status["result"]["last_restart_reason"], "initial_start",
        "{failed_status}"
    );
    assert_eq!(
        failed_status["result"]["pending_restart_reason"], "worker_binary_changed",
        "{failed_status}"
    );
    assert!(
        failed_status["result"]["last_error"].is_string(),
        "{failed_status}"
    );
    assert!(
        failed_status["result"]["worker"].is_null(),
        "{failed_status}"
    );

    install_exo_worker_binary(&worker);

    let after_restore = call_exo_run(&mut stdin, &mut stdout, 4, "status");
    assert_eq!(after_restore["result"]["isError"], false, "{after_restore}");

    let recovered_status =
        call_proxy_status(&mut stdin, &mut stdout, "proxy-hot-restart-recovered");
    assert_eq!(
        recovered_status["result"]["restart_count"], 1,
        "{recovered_status}"
    );
    assert_eq!(
        recovered_status["result"]["last_restart_reason"], "worker_binary_changed",
        "{recovered_status}"
    );
    assert!(
        recovered_status["result"]["last_error"].is_null(),
        "{recovered_status}"
    );
    assert!(
        recovered_status["result"]["pending_restart_reason"].is_null(),
        "{recovered_status}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_returns_error_when_worker_exits_before_reply() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("exit-worker.sh");
    std::fs::write(&worker, "#!/bin/sh\nread _line\nexit 0\n").expect("write worker");
    let mut permissions = std::fs::metadata(&worker)
        .expect("worker metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&worker, permissions).expect("chmod worker");

    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("exo-mcp"));
    command
        .current_dir(temp.path())
        .env("EXO_MCP_WORKER", &worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn exo-mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );

    let response = read_message(&mut stdout);
    assert_eq!(response["id"], 1);
    assert_eq!(response["error"]["code"], -32000);
    assert!(
        response["error"]["data"]["message"]
            .as_str()
            .expect("error message")
            .contains("worker process closed before replying"),
        "{response}"
    );

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-worker-exit");
    assert!(proxy_status["result"]["last_restart_reason"].is_null());
    assert_eq!(
        proxy_status["result"]["pending_restart_reason"], "initial_start",
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["last_error"]
            .as_str()
            .expect("last_error")
            .contains("worker process closed before replying"),
        "{proxy_status}"
    );
    assert!(proxy_status["result"]["worker"].is_null(), "{proxy_status}");

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_rejects_incompatible_worker_protocol_version() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let worker = temp.path().join("wrong-protocol-worker.sh");
    let executable_identity =
        exo::mcp::executable_identity_for_path(&assert_cmd::cargo::cargo_bin!("exo"))
            .expect("exo binary identity");
    let hello = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": "exo-proxy-worker-hello",
        "result": {
            "identity": {
                "executable_path": worker.to_string_lossy(),
                "executable_identity": executable_identity,
                "worker_protocol_version": exo::mcp::MCP_WORKER_PROTOCOL_VERSION + 1
            }
        }
    }))
    .expect("serialize fake worker hello");
    std::fs::write(
        &worker,
        format!(
            "#!/bin/sh\nread _line\ncat <<'JSON'\n{hello}\nJSON\nwhile read _line; do :; done\n"
        ),
    )
    .expect("write fake worker");
    let mut permissions = std::fs::metadata(&worker)
        .expect("worker metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&worker, permissions).expect("chmod worker");

    let mut child =
        spawn_exo_mcp_proxy_with_env(temp.path(), [("EXO_MCP_WORKER", worker.as_path())]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let response = call_initialize(&mut stdin, &mut stdout, 1);
    assert_eq!(response["id"], 1);
    assert_eq!(response["error"]["code"], -32000);
    assert!(
        response["error"]["data"]["message"]
            .as_str()
            .expect("error message")
            .contains("worker protocol version mismatch"),
        "{response}"
    );

    let proxy_status = call_proxy_status(&mut stdin, &mut stdout, "proxy-after-protocol-mismatch");
    assert!(proxy_status["result"]["last_restart_reason"].is_null());
    assert_eq!(
        proxy_status["result"]["pending_restart_reason"], "initial_start",
        "{proxy_status}"
    );
    assert!(
        proxy_status["result"]["last_error"]
            .as_str()
            .expect("last_error")
            .contains("worker protocol version mismatch"),
        "{proxy_status}"
    );
    assert!(proxy_status["result"]["worker"].is_null(), "{proxy_status}");

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[test]
fn exo_mcp_proxy_forwards_protocol_errors_without_desynchronizing() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_exo_mcp_proxy(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    writeln!(stdin, "not json").expect("write malformed MCP message");
    stdin.flush().expect("flush malformed MCP message");

    let parse_error = read_message(&mut stdout);
    assert_eq!(parse_error["id"], JsonValue::Null);
    assert_eq!(parse_error["error"]["code"], -32700);

    writeln!(stdin, "[]").expect("write batch MCP message");
    stdin.flush().expect("flush batch MCP message");
    let batch_error = read_message(&mut stdout);
    assert_eq!(batch_error["id"], JsonValue::Null);
    assert_eq!(batch_error["error"]["code"], -32600);
    assert!(
        batch_error["error"]["data"]["message"]
            .as_str()
            .expect("batch error message")
            .contains("batches are not supported"),
        "{batch_error}"
    );

    writeln!(stdin, "{{}}").expect("write invalid MCP message");
    stdin.flush().expect("flush invalid MCP message");
    let invalid_error = read_message(&mut stdout);
    assert_eq!(invalid_error["id"], JsonValue::Null);
    assert_eq!(invalid_error["error"]["code"], -32600);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "method": "ping"
        }),
    );
    let null_id_ping = read_message(&mut stdout);
    assert_eq!(null_id_ping["id"], JsonValue::Null);
    assert!(null_id_ping["result"].is_object(), "{null_id_ping}");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    let initialize = read_message(&mut stdout);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[test]
fn mcp_stdio_rfc_reads_are_agent_facing_text() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    let create = call_exo_run(
        &mut stdin,
        &mut stdout,
        10,
        "rfc create \"Agent Facing RFC\" --id 00042 --feature Test --stage 0",
    );
    assert_eq!(create["result"]["isError"], false, "{create}");

    let phase_details = call_exo_run(
        &mut stdin,
        &mut stdout,
        11,
        "phase read-details --format json",
    );
    assert_eq!(phase_details["result"]["isError"], false, "{phase_details}");
    let phase_id = structured_content(&phase_details)["result"]["phaseId"]
        .as_str()
        .expect("phase read-details JSON contains canonical phase id")
        .to_string();

    let link = call_exo_run(
        &mut stdin,
        &mut stdout,
        15,
        &format!("phase update {phase_id} --rfcs 00042"),
    );
    assert_eq!(link["result"]["isError"], false, "{link}");

    let linked_details = call_exo_run(&mut stdin, &mut stdout, 16, "phase read-details");
    assert_eq!(
        linked_details["result"]["isError"], false,
        "{linked_details}"
    );
    assert_no_structured_content(&linked_details);
    let linked_details_text = tool_text(&linked_details);
    assert!(
        linked_details_text.contains("RFCs: 00042"),
        "{linked_details_text}"
    );

    let phase_status = call_exo_run(&mut stdin, &mut stdout, 17, "phase status --full");
    assert_eq!(phase_status["result"]["isError"], false, "{phase_status}");
    assert_no_structured_content(&phase_status);
    let phase_status_text = tool_text(&phase_status);
    assert!(
        phase_status_text.contains("RFCs: 00042"),
        "{phase_status_text}"
    );

    let show = call_exo_run(&mut stdin, &mut stdout, 12, "rfc show 00042");
    assert_eq!(show["result"]["isError"], false, "{show}");
    assert_no_structured_content(&show);
    let show_text = tool_text(&show);
    assert!(
        show_text.contains("RFC 00042: Agent Facing RFC"),
        "{show_text}"
    );
    assert!(show_text.contains("Stage: 0 (Idea)"), "{show_text}");
    assert!(show_text.contains("File:"), "{show_text}");
    assert!(
        show_text.contains("rfc promote 00042 --stage <int>"),
        "{show_text}"
    );

    let status = call_exo_run(&mut stdin, &mut stdout, 13, "rfc status");
    assert_eq!(status["result"]["isError"], false, "{status}");
    assert_no_structured_content(&status);
    let status_text = tool_text(&status);
    assert!(status_text.contains("RFC Status:"), "{status_text}");
    assert!(status_text.contains("Stage 0: Idea"), "{status_text}");
    assert!(
        status_text.contains("RFC 00042: Agent Facing RFC"),
        "{status_text}"
    );
    assert!(
        status_text.contains("Show details: rfc show <id>"),
        "{status_text}"
    );

    let pipeline = call_exo_run(&mut stdin, &mut stdout, 14, "rfc pipeline");
    assert_eq!(pipeline["result"]["isError"], false, "{pipeline}");
    assert_no_structured_content(&pipeline);
    let pipeline_text = tool_text(&pipeline);
    assert!(pipeline_text.contains("RFC Pipeline"), "{pipeline_text}");
    assert!(
        pipeline_text.contains("RFC 00042: Agent Facing RFC"),
        "{pipeline_text}"
    );
    assert!(pipeline_text.contains("stage 0 -> -"), "{pipeline_text}");
    assert!(
        pipeline_text.contains("Promote: rfc promote <id> --stage <int>"),
        "{pipeline_text}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_rfc_withdraw_write_returns_command_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    let create = call_exo_run(
        &mut stdin,
        &mut stdout,
        20,
        "rfc create \"Withdraw Me\" --id 00043 --feature Test --stage 0",
    );
    assert_eq!(create["result"]["isError"], false, "{create}");

    let withdraw = call_exo_run(
        &mut stdin,
        &mut stdout,
        21,
        "rfc withdraw 00043 --reason obsolete",
    );
    assert_eq!(withdraw["result"]["isError"], false, "{withdraw}");
    assert_no_structured_content(&withdraw);
    let withdraw_text = tool_text(&withdraw);
    assert_eq!(withdraw_text, "rfc withdraw: 00043");
    assert!(
        temp.path()
            .join("docs/rfcs/withdrawn/00043-withdraw-me.md")
            .exists(),
        "withdraw should move the RFC"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_serves_exo_run_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": exo::mcp::MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    );
    let initialize = read_message(&mut stdout);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION
    );
    assert!(initialize["result"]["capabilities"]["tools"].is_object());

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    );
    let list = read_message(&mut stdout);
    assert_eq!(list["result"]["tools"][0]["name"], "exo-run");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "status" }
            }
        }),
    );
    let call = read_message(&mut stdout);
    assert_eq!(call["result"]["isError"], false);
    assert_no_structured_content(&call);
    let text = tool_text(&call);
    assert!(text.contains("Mode:"), "{text}");
    assert!(text.contains("Next actions:"), "{text}");

    for (id, command, expected) in [
        (21, "plan read", "Plan:"),
        (22, "phase read-details", "Phase:"),
        (23, "phase read-goals", "No goals."),
        (24, "phase execution.tasks", "Phase execution tasks:"),
        (25, "project resolve", "db_path:"),
        (26, "map --next", ": "),
    ] {
        let call = call_exo_run(&mut stdin, &mut stdout, id, command);
        assert_eq!(call["result"]["isError"], false, "{command}: {call}");
        assert_no_structured_content(&call);
        let text = tool_text(&call);
        assert!(
            !text.trim().ends_with(": done"),
            "{command} should not return a placeholder: {text}"
        );
        assert!(text.contains(expected), "{command}: {text}");
        if command == "map --next" {
            assert!(
                !text.contains(": exo "),
                "map --next should return exo-run shaped commands: {text}"
            );
        }
        if command == "project resolve" {
            assert!(text.contains("pid_path:"), "{text}");
        }
    }

    let missing_details = call_exo_run(
        &mut stdin,
        &mut stdout,
        261,
        "phase read-details missing-phase-id",
    );
    assert_eq!(
        missing_details["result"]["isError"], false,
        "{missing_details}"
    );
    let missing_details_text = tool_text(&missing_details);
    assert!(
        missing_details_text.contains("No phase details found"),
        "{missing_details_text}"
    );

    let map_why = call_exo_run(
        &mut stdin,
        &mut stdout,
        262,
        "map --why \"exo phase status\"",
    );
    assert_eq!(map_why["result"]["isError"], false, "{map_why}");
    let map_why_text = tool_text(&map_why);
    assert!(map_why_text.contains("Why: phase status"), "{map_why_text}");
    assert!(map_why_text.contains("Effects:"), "{map_why_text}");

    let phase_add = call_exo_run(
        &mut stdin,
        &mut stdout,
        27,
        "phase add --title \"Agent Payload Phase\" --format json",
    );
    assert_eq!(phase_add["result"]["isError"], false, "{phase_add}");
    let phase_add_text = tool_text(&phase_add);
    assert!(phase_add_text.contains("Added phase"), "{phase_add_text}");
    assert!(
        phase_add_text.contains("Agent Payload Phase"),
        "{phase_add_text}"
    );
    let future_phase_id = structured_content(&phase_add)["result"]["id"]
        .as_str()
        .expect("phase add JSON contains canonical phase id")
        .to_string();

    let phase_list = call_exo_run(&mut stdin, &mut stdout, 263, "phase list");
    assert_eq!(phase_list["result"]["isError"], false, "{phase_list}");
    let phase_list_text = tool_text(&phase_list);
    assert!(
        phase_list_text.contains("Agent Payload Phase"),
        "{phase_list_text}"
    );

    let goal_add = call_exo_run(
        &mut stdin,
        &mut stdout,
        28,
        "goal add \"Agent Payload Goal\" --id agent-payload-goal",
    );
    assert_eq!(goal_add["result"]["isError"], false, "{goal_add}");
    let goal_add_text = tool_text(&goal_add);
    assert!(
        goal_add_text.contains("Added goal 'agent-payload-goal'"),
        "{goal_add_text}"
    );
    assert!(
        goal_add_text.contains("Agent Payload Goal"),
        "{goal_add_text}"
    );

    let future_goal_command =
        format!("goal add \"Future Phase Goal\" --id future-phase-goal --phase {future_phase_id}");
    let future_goal_add = call_exo_run(&mut stdin, &mut stdout, 264, &future_goal_command);
    assert_eq!(
        future_goal_add["result"]["isError"], false,
        "{future_goal_add}"
    );
    let future_goal_text = tool_text(&future_goal_add);
    assert!(
        future_goal_text.contains("future-phase-goal"),
        "{future_goal_text}"
    );
    assert!(
        !future_goal_text.contains(&future_phase_id),
        "{future_goal_text}"
    );

    let future_phase_goals = call_exo_run(
        &mut stdin,
        &mut stdout,
        265,
        &format!("phase read-goals {future_phase_id}"),
    );
    assert_eq!(
        future_phase_goals["result"]["isError"], false,
        "{future_phase_goals}"
    );
    let future_phase_goals_text = tool_text(&future_phase_goals);
    assert!(
        future_phase_goals_text.contains("future-phase-goal"),
        "{future_phase_goals_text}"
    );

    let future_task_add = call_exo_run(
        &mut stdin,
        &mut stdout,
        266,
        "task add \"Future Phase Task\" --id future-phase-task --goal future-phase-goal",
    );
    assert_eq!(
        future_task_add["result"]["isError"], false,
        "{future_task_add}"
    );
    let future_task_text = tool_text(&future_task_add);
    assert!(
        future_task_text.contains("future-phase-task"),
        "{future_task_text}"
    );
    assert!(
        future_task_text.contains("future-phase-goal"),
        "{future_task_text}"
    );

    let future_phase_tasks = call_exo_run(
        &mut stdin,
        &mut stdout,
        267,
        &format!("phase read-tasks {future_phase_id}"),
    );
    assert_eq!(
        future_phase_tasks["result"]["isError"], false,
        "{future_phase_tasks}"
    );
    let future_phase_tasks_text = tool_text(&future_phase_tasks);
    assert!(
        future_phase_tasks_text.contains("future-phase-task"),
        "{future_phase_tasks_text}"
    );

    for index in 0..9 {
        let goal_id = format!("agent-payload-extra-goal-{index}");
        let command = format!("goal add \"Agent Payload Extra Goal {index}\" --id {goal_id}");
        let extra_goal = call_exo_run(&mut stdin, &mut stdout, 280 + index, &command);
        assert_eq!(extra_goal["result"]["isError"], false, "{extra_goal}");
    }

    let task_add = call_exo_run(
        &mut stdin,
        &mut stdout,
        29,
        "task add \"Agent Payload Task\" --id agent-payload-task --goal agent-payload-goal",
    );
    assert_eq!(task_add["result"]["isError"], false, "{task_add}");
    let task_add_text = tool_text(&task_add);
    assert!(
        task_add_text.contains("Added task 'agent-payload-task'"),
        "{task_add_text}"
    );
    assert!(
        task_add_text.contains("goal agent-payload-goal"),
        "{task_add_text}"
    );

    let plan_read = call_exo_run(&mut stdin, &mut stdout, 30, "plan read");
    assert_eq!(plan_read["result"]["isError"], false, "{plan_read}");
    let plan_text = tool_text(&plan_read);
    assert!(plan_text.contains("Goal agent-payload-goal"), "{plan_text}");
    assert!(
        plan_text.contains("Goal agent-payload-extra-goal-8"),
        "{plan_text}"
    );

    let phase_details = call_exo_run(&mut stdin, &mut stdout, 32, "phase read-details");
    assert_eq!(phase_details["result"]["isError"], false, "{phase_details}");
    let phase_details_text = tool_text(&phase_details);
    assert!(
        phase_details_text.contains("Task agent-payload-task"),
        "{phase_details_text}"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "dogfood verify --skip-receipt --format json" }
            }
        }),
    );
    let dogfood = read_message(&mut stdout);
    assert_eq!(dogfood["result"]["isError"], false, "{dogfood}");
    let dogfood_structured = structured_content(&dogfood);
    assert_eq!(dogfood_structured["status"], "ok");
    assert_eq!(dogfood_structured["result"]["kind"], "dogfood.verify");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "help task" }
            }
        }),
    );
    let help = read_message(&mut stdout);
    assert_eq!(help["result"]["isError"], false);
    assert_no_structured_content(&help);
    let help_text = tool_text(&help);
    assert!(help_text.contains("task add"), "{help_text}");
    assert!(help_text.contains("task complete"), "{help_text}");

    for (id, command, expected) in [
        (41, "task --help", "task complete"),
        (42, "task help", "task complete"),
        (43, "rfc --help", "rfc promote"),
        (44, "rfc help", "rfc promote"),
        (45, "rfc promote --help", "rfc promote <id> --stage <int>"),
        (
            46,
            "rfc repair --help",
            "rfc repair <id> [--path <string>] [--renumber-to <string>]",
        ),
    ] {
        write_message(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "exo-run",
                    "arguments": { "command": command }
                }
            }),
        );
        let help = read_message(&mut stdout);
        assert_eq!(help["result"]["isError"], false, "{command}: {help}");
        assert_no_structured_content(&help);
        let help_text = tool_text(&help);
        assert!(help_text.contains(expected), "{command}: {help_text}");
    }

    for (id, command, expected_usage) in [
        (
            51,
            "task --help --format=json",
            "task complete <id> [--log <string>]",
        ),
        (
            52,
            "rfc promote --help --format json",
            "rfc promote <id> --stage <int>",
        ),
        (
            53,
            "rfc repair --help --format json",
            "rfc repair <id> [--path <string>] [--renumber-to <string>]",
        ),
    ] {
        write_message(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "exo-run",
                    "arguments": { "command": command }
                }
            }),
        );
        let help = read_message(&mut stdout);
        assert_eq!(help["result"]["isError"], false, "{command}: {help}");
        let operations = structured_content(&help)["result"]["operations"]
            .as_array()
            .expect("structured help operations");
        if command.starts_with("task ") {
            let complete = operations
                .iter()
                .find(|operation| operation["path"] == "task complete")
                .expect("task complete operation");
            assert_eq!(complete["usage"], expected_usage);
            assert_eq!(complete["args"][0]["name"], "id");
            assert_eq!(complete["args"][1]["name"], "log");
            assert!(complete["args"][0]["keys"].is_null());
        }
        if command.starts_with("rfc ") {
            let operation_name = if command.contains("repair") {
                "rfc repair"
            } else {
                "rfc promote"
            };
            let operation = operations
                .iter()
                .find(|operation| operation["path"] == operation_name)
                .expect("rfc operation");
            assert_eq!(operation["usage"], expected_usage);
            assert_eq!(operation["args"][0]["name"], "id");
            if operation_name == "rfc promote" {
                assert_eq!(operation["args"][1]["name"], "stage");
                assert_eq!(operation["args"][1]["value_type"], "int");
                assert!(operation["args"][1]["keys"].is_null());
            } else {
                let args = operation["args"].as_array().expect("args");
                assert_eq!(args.len(), 3);
                assert_eq!(args[1]["name"], "path");
                assert_eq!(args[1]["value_type"], "string");
                assert_eq!(args[1]["optional"], true);
                assert_eq!(args[2]["name"], "renumber-to");
                assert_eq!(args[2]["value_type"], "string");
                assert_eq!(args[2]["optional"], true);
            }
        }
    }

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_rejects_shell_syntax_as_tool_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "call-1",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "status | cat" }
            }
        }),
    );
    let call = read_message(&mut stdout);
    assert_eq!(call["result"]["isError"], true);
    assert_eq!(structured_content(&call)["error"]["code"], "invalid_input");

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_replays_execution_confirmation() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    append_task(temp.path(), "print-ok", print_ok_command());

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "needs-auth",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "run task print-ok --format json" }
            }
        }),
    );
    let needs_auth = read_message(&mut stdout);
    assert_eq!(needs_auth["result"]["isError"], true);
    let needs_auth_structured = structured_content(&needs_auth);
    assert_eq!(needs_auth_structured["status"], "confirm_required");
    let ticket = needs_auth_structured["ticket"]
        .as_str()
        .expect("confirmation ticket");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "with-auth",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": "run task print-ok --format json",
                    "auth": { "ticket": ticket, "confirm": true }
                }
            }
        }),
    );
    let with_auth = read_message(&mut stdout);
    assert_eq!(with_auth["result"]["isError"], false);
    let with_auth_structured = structured_content(&with_auth);
    assert_eq!(with_auth_structured["status"], "ok");
    assert_eq!(with_auth_structured["result"]["stdout"], "ok");

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_dogfood_restart_does_not_kill_current_transport() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "restart-needs-auth",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "dogfood restart --format json" }
            }
        }),
    );
    let needs_auth = read_message(&mut stdout);
    assert_eq!(needs_auth["result"]["isError"], true, "{needs_auth:?}");
    let needs_auth_structured = structured_content(&needs_auth);
    assert_eq!(
        needs_auth_structured["status"], "confirm_required",
        "{needs_auth:?}"
    );
    let ticket = needs_auth_structured["ticket"]
        .as_str()
        .expect("confirmation ticket");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "restart-with-auth",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": "dogfood restart --format json",
                    "auth": { "ticket": ticket, "confirm": true }
                }
            }
        }),
    );
    let restarted = read_message(&mut stdout);
    assert_eq!(restarted["result"]["isError"], false, "{restarted:?}");
    let restarted_structured = structured_content(&restarted);
    assert_eq!(restarted_structured["status"], "ok", "{restarted:?}");
    assert_eq!(
        restarted_structured["result"]["daemon"]["killed"], false,
        "dogfood restart should ensure the daemon rather than killing it: {restarted:?}"
    );
    assert_eq!(
        restarted_structured["result"]["mcp"]["killed"]
            .as_array()
            .expect("killed is an array")
            .len(),
        0,
        "dogfood restart should not terminate MCP transports: {restarted:?}"
    );
    let skipped_self = restarted_structured["result"]["mcp"]["skipped_self"]
        .as_array()
        .expect("skipped_self is an array");
    assert_eq!(
        skipped_self.len(),
        1,
        "restart should preserve the MCP transport serving this request: {restarted:?}"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "status-after-restart",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "status" }
            }
        }),
    );
    let status_call = read_message(&mut stdout);
    assert_eq!(status_call["result"]["isError"], false, "{status_call:?}");
    assert_no_structured_content(&status_call);
    assert!(
        !tool_text(&status_call).trim().is_empty(),
        "{status_call:?}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_replays_workflow_confirmation() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    test_support::exo_cmd(temp.path())
        .args(["goal", "add", "Test goal", "--id", "test-goal"])
        .assert()
        .success();
    test_support::exo_cmd(temp.path())
        .args([
            "task",
            "add",
            "Review me",
            "--id",
            "review-me",
            "--goal",
            "test-goal",
        ])
        .assert()
        .success();
    let task_ref = "test-goal::review-me";
    let task_id = "review-me";

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "needs-workflow",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": format!("task complete {task_ref} --log Done --format json")
                }
            }
        }),
    );
    let needs_workflow = read_message(&mut stdout);
    assert_eq!(needs_workflow["result"]["isError"], true);
    let workflow =
        &structured_content(&needs_workflow)["error"]["details"]["workflow_confirmation"];
    assert_eq!(workflow["kind"], "workflow_completion_confirmation");
    assert_eq!(workflow["completion_input"]["entity_type"], "task");
    assert_eq!(workflow["completion_input"]["entity_id"], task_id);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "with-workflow",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": format!("task complete {task_ref} --log Done --format json"),
                    "workflowConfirmation": {
                        "kind": "workflow_completion_confirmation",
                        "entityType": "task",
                        "entityId": task_id,
                        "decision": "yes_complete",
                        "outcome": "Done"
                    }
                }
            }
        }),
    );
    let with_workflow = read_message(&mut stdout);
    assert_eq!(with_workflow["result"]["isError"], false);
    let with_workflow_structured = structured_content(&with_workflow);
    assert_eq!(with_workflow_structured["status"], "ok");
    assert_eq!(with_workflow_structured["result"]["task_id"], task_id);

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_rejects_removed_approved_flag() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    test_support::exo_cmd(temp.path())
        .args(["goal", "add", "Test goal", "--id", "test-goal"])
        .assert()
        .success();
    test_support::exo_cmd(temp.path())
        .args([
            "task",
            "add",
            "Review me",
            "--id",
            "review-me",
            "--goal",
            "test-goal",
        ])
        .assert()
        .success();
    let task_id = "test-goal::review-me";

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    for (id, command) in [
        (
            "task-approved",
            format!("task complete {task_id} --log Done --approved"),
        ),
        (
            "goal-approved",
            "goal complete test-goal --log Done --approved".to_string(),
        ),
    ] {
        let call = call_exo_run(&mut stdin, &mut stdout, id, &command);
        assert_eq!(call["result"]["isError"], true, "{call:?}");
        let structured = structured_content(&call);
        assert!(
            structured["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("Unknown flag '--approved'"),
            "{call:?}"
        );
    }

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");

    let task_output = test_support::exo_cmd(temp.path())
        .args(["--format", "json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task_json: JsonValue = serde_json::from_slice(&task_output).expect("valid task json");
    let task = task_json["result"]["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["id"].as_str() == Some(task_id))
        .expect("test task");
    assert_ne!(task["status"].as_str(), Some("completed"));

    let inbox_output = test_support::exo_cmd(temp.path())
        .args([
            "--format",
            "json",
            "inbox",
            "list",
            "--entity-type",
            "task",
            "--entity-id",
            task_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox_json: JsonValue = serde_json::from_slice(&inbox_output).expect("valid inbox json");
    let items = inbox_json["result"]["items"]
        .as_array()
        .expect("inbox items");
    assert!(items.is_empty(), "machine approval recorded inbox evidence");
}

#[test]
fn mcp_stdio_project_resolve_uses_sidecar_project_state_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .expect("run git init");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let default_project = Project::resolve(temp.path()).expect("resolve default project");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{}]\nstate = \"sidecar\"\nsidecar_key = \"mcp-sidecar\"\nsidecar_root = {:?}\n",
            default_project.id.as_str(),
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write sidecar policy");

    let mut child = spawn_mcp_server_with_env(temp.path(), [("XDG_CONFIG_HOME", &config_home)]);
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "project-resolve",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "project resolve --format json" }
            }
        }),
    );
    let call = read_message(&mut stdout);
    assert_eq!(call["result"]["isError"], false);
    let result = &structured_content(&call)["result"];
    let expected_state_root = sidecar_root
        .join("projects")
        .join("mcp-sidecar")
        .to_string_lossy()
        .to_string();
    let expected_db_path = sidecar_root
        .join("projects")
        .join("mcp-sidecar")
        .join("cache")
        .join("exo.db")
        .to_string_lossy()
        .to_string();
    assert_eq!(result["project"]["policy"], "sidecar");
    assert_eq!(
        result["paths"]["state_root"].as_str(),
        Some(expected_state_root.as_str())
    );
    assert_eq!(
        result["paths"]["db_path"].as_str(),
        Some(expected_db_path.as_str())
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_sidecar_read_records_command_event_without_auto_persisting() {
    let temp = tempfile::tempdir().expect("tempdir");
    Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .expect("run git init");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let config_home = temp.path().join("config");
    let home = temp.path().join("home");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("exo"))
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "mcp-sidecar",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ])
        .output()
        .expect("run sidecar init");
    assert!(
        output.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_config_identity(&sidecar_root);
    commit_sidecar_baseline(&sidecar_root);

    let head_before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let mut child = spawn_mcp_server_with_env(
        temp.path(),
        [("HOME", &home), ("XDG_CONFIG_HOME", &config_home)],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "status",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "status" }
            }
        }),
    );
    let call = read_message(&mut stdout);
    assert_eq!(call["result"]["isError"], false, "{call:?}");
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_eq!(
        git_output(&sidecar_root, &["rev-parse", "HEAD"]),
        head_before
    );

    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(
        !log.contains("Auto-persist Exosuit sidecar state"),
        "pure read should not create auto-persist commit: {log}"
    );

    let db_path = sidecar_root.join("projects/mcp-sidecar/cache/exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    let event_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE event_type = 'command' AND operation = 'status'",
            [],
            |row| row.get(0),
        )
        .expect("count command events");
    assert_eq!(event_count, 1);

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_sidecar_write_event_still_auto_persists() {
    let temp = tempfile::tempdir().expect("tempdir");
    Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .expect("run git init");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let config_home = temp.path().join("config");
    let home = temp.path().join("home");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("exo"))
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "mcp-sidecar-write",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ])
        .output()
        .expect("run sidecar init");
    assert!(
        output.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_config_identity(&sidecar_root);
    commit_sidecar_baseline(&sidecar_root);

    let head_before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let mut child = spawn_mcp_server_with_env(
        temp.path(),
        [("HOME", &home), ("XDG_CONFIG_HOME", &config_home)],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "task-add",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": "idea add $1",
                    "args": ["Sidecar write event"]
                }
            }
        }),
    );
    let call = read_message(&mut stdout);
    assert_eq!(call["result"]["isError"], false, "{call:?}");
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_ne!(
        git_output(&sidecar_root, &["rev-parse", "HEAD"]),
        head_before
    );

    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));

    let daemon_pid: u64 =
        std::fs::read_to_string(sidecar_root.join("projects/mcp-sidecar-write/runtime/daemon.pid"))
            .expect("read daemon pid")
            .trim()
            .parse()
            .expect("daemon pid is numeric");
    let owner_marker: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(sidecar_write_owner_marker_path(
            &sidecar_root,
            "mcp-sidecar-write",
        ))
        .expect("read sidecar write owner marker"),
    )
    .expect("parse sidecar write owner marker");
    assert_eq!(
        owner_marker["pid"], daemon_pid,
        "sidecar writer should be the ensured daemon, not the MCP server"
    );

    let db_path = sidecar_root.join("projects/mcp-sidecar-write/cache/exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    let event_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE event_type = 'command' AND namespace = 'idea' AND operation = 'add'",
            [],
            |row| row.get(0),
        )
        .expect("count command events");
    assert_eq!(event_count, 1);

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_sidecar_write_reclaims_stale_owner_through_daemon_writer() {
    let temp = tempfile::tempdir().expect("tempdir");
    Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .expect("run git init");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let config_home = temp.path().join("config");
    let home = temp.path().join("home");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("exo"))
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "mcp-sidecar-stale",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ])
        .output()
        .expect("run sidecar init");
    assert!(
        output.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_config_identity(&sidecar_root);
    commit_sidecar_baseline(&sidecar_root);
    let head_before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let mut stale_owner = exit_success_command()
        .spawn()
        .expect("spawn stale owner fixture");
    let stale_owner_pid = stale_owner.id();
    stale_owner.wait().expect("wait for stale owner fixture");
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "mcp-sidecar-stale",
        stale_owner_pid,
        &temp.path().join("stale-workspace"),
    );

    let mut child = spawn_mcp_server_with_env(
        temp.path(),
        [("HOME", &home), ("XDG_CONFIG_HOME", &config_home)],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let call = call_exo_run(
        &mut stdin,
        &mut stdout,
        "stale-sidecar-write",
        "idea add \"Stale Sidecar Owner\"",
    );
    assert_eq!(call["result"]["isError"], false, "{call:?}");
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_ne!(
        git_output(&sidecar_root, &["rev-parse", "HEAD"]),
        head_before
    );

    let daemon_pid: u64 =
        std::fs::read_to_string(sidecar_root.join("projects/mcp-sidecar-stale/runtime/daemon.pid"))
            .expect("read daemon pid")
            .trim()
            .parse()
            .expect("daemon pid is numeric");
    let owner_marker: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(sidecar_write_owner_marker_path(
            &sidecar_root,
            "mcp-sidecar-stale",
        ))
        .expect("read sidecar write owner marker"),
    )
    .expect("parse sidecar write owner marker");
    assert_eq!(owner_marker["pid"], daemon_pid);

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_sidecar_write_fails_before_projection_when_owner_blocked() {
    let temp = tempfile::tempdir().expect("tempdir");
    Command::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .expect("run git init");
    test_support::exo_init_with_storage(temp.path(), "sqlite");

    let config_home = temp.path().join("config");
    let home = temp.path().join("home");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("exo"))
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "mcp-sidecar-blocked",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ])
        .output()
        .expect("run sidecar init");
    assert!(
        output.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_config_identity(&sidecar_root);
    commit_sidecar_baseline(&sidecar_root);
    let head_before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    let other_workspace = temp.path().join("other-workspace");
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "mcp-sidecar-blocked",
        std::process::id(),
        &other_workspace,
    );

    let mut child = spawn_mcp_server_with_env(
        temp.path(),
        [("HOME", &home), ("XDG_CONFIG_HOME", &config_home)],
    );
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let call = call_exo_run(
        &mut stdin,
        &mut stdout,
        "blocked-sidecar-write",
        "idea add \"Blocked Sidecar Write\"",
    );
    assert_eq!(call["result"]["isError"], true, "{call:?}");
    let structured = structured_content(&call);
    assert_eq!(
        structured["error"]["code"], "precondition_failed",
        "{call:?}"
    );
    assert!(
        structured["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("another active runtime")),
        "{call:?}"
    );
    assert_eq!(
        git_output(&sidecar_root, &["rev-parse", "HEAD"]),
        head_before
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_serves_exo_run_task_list_with_active_phase() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    let epoch_id = test_support::exo_plan_add_epoch(temp.path(), "MCP Task Epoch");
    let phase_id =
        test_support::exo_plan_add_phase(temp.path(), &epoch_id, "MCP Task Phase", None, None);
    test_support::exo_cmd(temp.path())
        .args(["phase", "start", &phase_id])
        .assert()
        .success();

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": "task-list",
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": { "command": "task list" }
            }
        }),
    );
    let task_list = read_message(&mut stdout);
    assert_eq!(task_list["result"]["isError"], false, "{task_list:?}");
    assert_no_structured_content(&task_list);
    assert!(!tool_text(&task_list).trim().is_empty(), "{task_list:?}");

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_reports_malformed_rfc_anchor_as_repair_debt() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    write_malformed_rfc_anchor_fixture(temp.path());

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    let status = call_exo_run(&mut stdin, &mut stdout, 300, "status");
    assert_eq!(status["result"]["isError"], false, "{status}");

    let task_list = call_exo_run(&mut stdin, &mut stdout, 301, "task list");
    assert_eq!(task_list["result"]["isError"], false, "{task_list}");

    let rfc_status = call_exo_run(&mut stdin, &mut stdout, 302, "rfc status");
    assert_eq!(rfc_status["result"]["isError"], false, "{rfc_status}");
    assert_no_structured_content(&rfc_status);
    let text = tool_text(&rfc_status);
    assert!(text.contains("RFC identity repairs"), "{text}");
    assert!(
        text.contains("docs/rfcs/stage-0/0004-local-v0-rehearsal-contract.md"),
        "{text}"
    );
    assert!(text.contains("exo rfc repair 0004"), "{text}");

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_initializes_even_when_workspace_context_fails_to_load() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    );
    let list = read_message(&mut stdout);
    let tools = list["result"]["tools"].as_array().expect("tools list");
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"].as_str() == Some("exo-run")),
        "{list}"
    );

    let status = call_exo_run(&mut stdin, &mut stdout, 3, "status");
    assert_eq!(status["result"]["isError"], true, "{status}");
    let text = tool_text(&status);
    assert!(
        text.contains("Failed to load agent context") || text.contains("no exosuit.toml"),
        "{text}"
    );

    let project_resolve = call_exo_run(&mut stdin, &mut stdout, 4, "project resolve --format json");
    assert_eq!(
        project_resolve["result"]["isError"], false,
        "{project_resolve}"
    );
    let project_result = &structured_content(&project_resolve)["result"];
    assert_eq!(project_result["kind"], "project.resolve");
    assert_eq!(project_result["ok"], true);

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_imports_projection_before_rfc_status_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = prepare_projection_only_workspace(temp.path(), "MCP Projection Read Epoch");

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    let rfc_status = call_exo_run(&mut stdin, &mut stdout, 4, "rfc status");
    assert_eq!(rfc_status["result"]["isError"], false, "{rfc_status}");
    assert!(
        db_path.exists(),
        "rfc status should import the SQL projection"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[test]
fn mcp_stdio_imports_projection_before_first_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = prepare_projection_only_workspace(temp.path(), "MCP Projection Write Epoch");

    let (mut child, mut stdin, mut stdout) = initialize_mcp_server(temp.path());

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "exo-run",
                "arguments": {
                    "command": "idea add $1",
                    "args": ["MCP projection write idea"]
                }
            }
        }),
    );
    let write = read_message(&mut stdout);
    assert_eq!(write["result"]["isError"], false, "{write}");
    assert!(db_path.exists(), "write should import the SQL projection");

    let epochs = call_exo_run(&mut stdin, &mut stdout, 6, "epoch list");
    assert_eq!(epochs["result"]["isError"], false, "{epochs}");
    assert!(
        tool_text(&epochs).contains("MCP Projection Write Epoch"),
        "{epochs}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}

#[cfg(unix)]
#[test]
fn exo_mcp_proxy_worker_imports_projection_before_first_call() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = prepare_projection_only_workspace(temp.path(), "MCP Proxy Projection Epoch");

    let mut child = spawn_exo_mcp_proxy(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let initialize = call_initialize(&mut stdin, &mut stdout, 7);
    assert_eq!(
        initialize["result"]["protocolVersion"],
        exo::mcp::MCP_PROTOCOL_VERSION,
        "{initialize}"
    );

    let rfc_status = call_exo_run(&mut stdin, &mut stdout, 8, "rfc status");
    assert_eq!(rfc_status["result"]["isError"], false, "{rfc_status}");
    assert!(
        db_path.exists(),
        "worker rfc status should import the SQL projection"
    );

    drop(stdin);
    let status = child.wait().expect("wait for exo-mcp");
    assert!(status.success(), "exo-mcp exited with {status}");
}

#[test]
fn mcp_stdio_task_add_goal_ambiguity_is_invalid_input() {
    let temp = tempfile::tempdir().expect("tempdir");
    test_support::exo_init_with_storage(temp.path(), "sqlite");
    let epoch_id = test_support::exo_plan_add_epoch(temp.path(), "MCP Ambiguity Epoch");
    let phase_id =
        test_support::exo_plan_add_phase(temp.path(), &epoch_id, "MCP Ambiguity Phase", None, None);
    test_support::exo_cmd(temp.path())
        .args(["phase", "start", &phase_id])
        .assert()
        .success();

    let pending_phase_id =
        test_support::exo_plan_add_phase(temp.path(), &epoch_id, "Future MCP Phase", None, None);
    let writer = exo::context::SqliteWriter::open(&exo::context::db_path(temp.path(), None))
        .expect("open sqlite writer");
    writer
        .add_goal(
            &pending_phase_id,
            "future-shared-goal",
            "Future Shared Goal",
            None,
            None,
            None,
            None,
            None,
            None,
            &["shared-goal".to_string()],
        )
        .expect("add future aliased goal");
    drop(writer);
    test_support::exo_cmd(temp.path())
        .args(["goal", "add", "Shared Goal", "--id", "shared-goal"])
        .assert()
        .success();

    let mut child = spawn_mcp_server(temp.path());
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    let call = call_exo_run(
        &mut stdin,
        &mut stdout,
        "ambiguous-task-add",
        "task add \"Ambiguous Task\" --id ambiguous-task --goal shared-goal",
    );
    assert_eq!(call["result"]["isError"], true, "{call:?}");
    let structured = structured_content(&call);
    assert_eq!(structured["error"]["code"], "invalid_input", "{call:?}");
    assert!(
        structured["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ambiguous across active or pending phases"),
        "{call:?}"
    );

    drop(stdin);
    let status = child.wait().expect("wait for mcp server");
    assert!(status.success(), "mcp server exited with {status}");
}
