#![allow(clippy::disallowed_methods)] // integration tests use real fs/process/timing APIs

//! Parity tests between direct and daemon modes.
//!
//! These tests verify that commands produce identical results whether
//! executed directly or through the daemon.

#[macro_use]
mod test_support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_matrix;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn git_init(root: &std::path::Path) {
    let output = Command::new("git")
        .arg("init")
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run git init");

    assert!(
        output.status.success(),
        "git init failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn kill_test_daemon(workspace: &std::path::Path) {
    let Ok(paths) = exo::daemon::paths_for_workspace(workspace) else {
        #[cfg(windows)]
        kill_windows_daemons_for_workspace(workspace);
        return;
    };
    if let Ok(pid_str) = std::fs::read_to_string(paths.pid_path())
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        #[cfg(unix)]
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
        #[cfg(windows)]
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(200));
    }
    #[cfg(windows)]
    kill_windows_daemons_for_workspace(workspace);
    let _ = paths.endpoint().remove_stale();
    let _ = std::fs::remove_file(paths.pid_path());
}

#[cfg(windows)]
fn kill_windows_daemons_for_workspace(workspace: &std::path::Path) {
    let workspace = workspace.display().to_string();
    let command = r#"
$workspace = $env:EXO_TEST_WORKSPACE
Get-CimInstance Win32_Process -Filter "name = 'exo.exe'" |
  Where-Object {
    $_.CommandLine -like "*daemon run --workspace*" -and
    $_.CommandLine -like "*$workspace*"
  } |
  ForEach-Object { taskkill /PID $_.ProcessId /T /F | Out-Null }
"#;
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", command])
        .env("EXO_TEST_WORKSPACE", workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    std::thread::sleep(Duration::from_millis(200));
}

struct DaemonGuard {
    workspace: PathBuf,
}

impl DaemonGuard {
    fn new(workspace: &std::path::Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        kill_test_daemon(&self.workspace);
    }
}

/// Create a workspace with required files for the given backend.
fn create_test_workspace(dir: &TempDir, backend: &str) -> PathBuf {
    let workspace = dir.path().to_path_buf();
    git_init(&workspace);
    test_support::exo_init_with_storage(&workspace, backend);
    workspace
}

/// Send a request to the daemon and get a response.
async fn send_daemon_request(
    workspace: &std::path::Path,
    request_json: &str,
) -> Result<serde_json::Value, String> {
    let stream = exo::daemon::ensure_daemon(workspace)
        .await
        .map_err(|e| format!("Failed to connect to daemon: {e}"))?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    // Send request
    writer
        .write_all(request_json.as_bytes())
        .await
        .map_err(|e| format!("Failed to write request: {e}"))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("Failed to write newline: {e}"))?;

    // Read response with timeout
    let response = tokio::time::timeout(Duration::from_secs(30), lines.next_line())
        .await
        .map_err(|_| "Timeout waiting for response".to_string())?
        .map_err(|e| format!("IO error: {e}"))?
        .ok_or_else(|| "No response received".to_string())?;

    serde_json::from_str(&response).map_err(|e| format!("Invalid JSON response: {e}"))
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_status_parity(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    let daemon_result = exo::daemon::ensure_daemon(&workspace).await;
    let stream = daemon_result.expect("daemon should spawn real exo binary");
    drop(stream);

    // Send status request through daemon
    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "parity-test-1",
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["status"] },
                "input": {}
            }
        }
    });

    let response = send_daemon_request(&workspace, &request.to_string()).await;

    let resp = response.expect("daemon status request should produce a response");
    assert_eq!(
        resp.get("id").and_then(|v| v.as_str()),
        Some("parity-test-1")
    );
    assert!(resp.get("status").is_some());
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_help_parity(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    let daemon_result = exo::daemon::ensure_daemon(&workspace).await;
    let stream = daemon_result.expect("daemon should spawn real exo binary");
    drop(stream);

    // Send help request through daemon
    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "parity-test-2",
        "op": {
            "kind": "help",
            "params": {
                "address": { "kind": "root" }
            }
        }
    });

    let response = send_daemon_request(&workspace, &request.to_string()).await;

    let resp = response.expect("daemon help request should produce a response");
    assert_eq!(
        resp.get("id").and_then(|v| v.as_str()),
        Some("parity-test-2")
    );
    assert_eq!(
        resp.get("status").and_then(|v| v.as_str()),
        Some("ok"),
        "Help should always succeed"
    );
    let result = resp.get("result");
    assert!(result.is_some(), "Help should return a result");
}

/// Test that the daemon handles invalid requests gracefully.
#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_error_handling_parity(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    let daemon_result = exo::daemon::ensure_daemon(&workspace).await;
    let stream = daemon_result.expect("daemon should spawn real exo binary");
    drop(stream);

    // Send request for unknown command
    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "parity-test-3",
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["nonexistent", "command"] },
                "input": {}
            }
        }
    });

    let response = send_daemon_request(&workspace, &request.to_string()).await;

    let resp = response.expect("daemon unknown-command request should produce a response");
    assert_eq!(
        resp.get("id").and_then(|v| v.as_str()),
        Some("parity-test-3")
    );
    assert_eq!(
        resp.get("status").and_then(|v| v.as_str()),
        Some("error"),
        "Unknown command should return error"
    );
    assert!(resp.get("error").is_some(), "Should have error details");
}
