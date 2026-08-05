#![allow(clippy::disallowed_methods)] // integration tests use real fs/process/timing APIs

//! Integration test for daemon idle timeout behavior.
//!
//! Verifies that the daemon exits automatically after the idle timeout
//! period with no client activity.

#[macro_use]
mod test_support;

use exo::daemon_transport::{DaemonClientStream, DaemonEndpoint};
#[cfg(feature = "ui")]
use std::io::{Read, Write};
#[cfg(feature = "ui")]
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use test_case::test_matrix;

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

/// Spawn the daemon with a short timeout for testing.
fn spawn_daemon(workspace: &std::path::Path, timeout_secs: u64) -> std::io::Result<Child> {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["daemon", "run"])
        .arg("--workspace")
        .arg(workspace)
        .arg("--timeout")
        .arg(timeout_secs.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn create_test_workspace(temp: &tempfile::TempDir, backend: &str) -> std::path::PathBuf {
    let workspace = temp.path().to_path_buf();
    git_init(&workspace);
    test_support::exo_init_with_storage(&workspace, backend);
    workspace
}

fn wait_for_daemon_endpoint(child: &mut Child, workspace: &std::path::Path) -> DaemonEndpoint {
    let paths = exo::daemon::paths_for_workspace(workspace).expect("project paths");
    let endpoint = paths.endpoint();
    let start = Instant::now();

    loop {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("daemon exited before endpoint became available: {status:?}");
        }

        if endpoint.is_connectable_blocking() {
            return endpoint;
        }

        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("daemon endpoint did not become available");
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn connect_to_daemon(
    endpoint: &DaemonEndpoint,
    child: &mut Child,
    iteration: usize,
) -> DaemonClientStream {
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("daemon exited before iteration {iteration} with status: {status:?}");
        }

        match endpoint.connect_blocking() {
            Ok(stream) => return stream,
            Err(error) if start.elapsed() <= Duration::from_secs(5) => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                let _ = child.kill();
                panic!("failed to connect to daemon endpoint on iteration {iteration}: {error}");
            }
        }
    }
}

#[cfg(feature = "ui")]
fn redeem_workbench_ticket(origin: &str, ticket: &str) -> String {
    let authority = origin
        .strip_prefix("http://")
        .expect("workbench origin should use HTTP");
    let body = serde_json::json!({ "ticket": ticket }).to_string();
    let request = format!(
        "POST /api/session HTTP/1.1\r\nHost: {authority}\r\nOrigin: {origin}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(authority).expect("connect to workbench host");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set workbench response timeout");
    stream
        .write_all(request.as_bytes())
        .expect("send workbench ticket exchange");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read workbench ticket exchange");
    response
}

#[cfg(feature = "ui")]
fn wait_for_clean_daemon_exit(child: &mut Child, max_wait: Duration) {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "daemon should exit cleanly: {status:?}");
                return;
            }
            Ok(None) if start.elapsed() <= max_wait => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                panic!("daemon did not exit within {max_wait:?}");
            }
            Err(error) => panic!("error checking daemon status: {error}"),
        }
    }
}

#[test_matrix(["sqlite"])]
fn test_daemon_exits_after_idle_timeout(backend: &str) {
    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let workspace = create_test_workspace(&temp, backend);

    // Spawn daemon with 2-second timeout
    let timeout_secs = 2;
    let mut child = spawn_daemon(&workspace, timeout_secs).expect("failed to spawn daemon");

    // Give daemon time to start
    std::thread::sleep(Duration::from_millis(500));

    // Verify daemon is running
    assert!(
        child.try_wait().unwrap().is_none(),
        "daemon should still be running"
    );

    // Wait for idle timeout + buffer
    let start = Instant::now();
    let max_wait = Duration::from_secs(timeout_secs + 3);

    loop {
        if start.elapsed() > max_wait {
            // Kill the daemon if it didn't exit
            let _ = child.kill();
            panic!(
                "daemon did not exit after idle timeout (waited {:?})",
                start.elapsed()
            );
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                // Daemon exited - verify it was a clean exit
                assert!(
                    status.success(),
                    "daemon should exit cleanly, got status: {status:?}"
                );
                break;
            }
            Ok(None) => {
                // Still running, wait a bit
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                panic!("error checking daemon status: {e}");
            }
        }
    }

    // Verify endpoint was cleaned up.
    let paths = exo::daemon::paths_for_workspace(&workspace).expect("project paths");
    #[cfg(unix)]
    assert!(
        !paths.socket_path().exists(),
        "socket should be cleaned up after daemon exit"
    );
    assert!(
        !paths.endpoint().is_connectable_blocking(),
        "daemon endpoint should not accept clients after daemon exit"
    );
}

#[cfg(feature = "ui")]
#[test_matrix(["sqlite"])]
fn test_pending_workbench_enrollment_defers_idle_shutdown(backend: &str) {
    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let workspace = create_test_workspace(&temp, backend);
    let timeout_secs = 2;
    let mut child = spawn_daemon(&workspace, timeout_secs).expect("failed to spawn daemon");
    let _endpoint = wait_for_daemon_endpoint(&mut child, &workspace);

    let launch = Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["--format", "json", "workbench", "launch"])
        .current_dir(&workspace)
        .output()
        .expect("launch workbench through daemon");
    assert!(
        launch.status.success(),
        "workbench launch failed: {}",
        String::from_utf8_lossy(&launch.stderr)
    );
    let launch: serde_json::Value =
        serde_json::from_slice(&launch.stdout).expect("workbench launch JSON");
    let url = launch["result"]["url"]
        .as_str()
        .expect("workbench launch URL");
    let (origin, ticket) = url
        .split_once("/#ticket=")
        .expect("workbench launch URL contains ticket");

    std::thread::sleep(Duration::from_secs(timeout_secs + 1));
    assert!(
        child.try_wait().expect("check daemon status").is_none(),
        "pending enrollment should keep the daemon alive beyond its idle timeout"
    );

    let response = redeem_workbench_ticket(origin, ticket);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "ticket exchange should succeed: {response}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        child.try_wait().expect("check daemon status").is_none(),
        "successful enrollment should refresh daemon activity"
    );

    wait_for_clean_daemon_exit(
        &mut child,
        Duration::from_secs(timeout_secs.saturating_add(3)),
    );
}

#[test_matrix(["sqlite"])]
fn test_daemon_activity_resets_timeout(backend: &str) {
    use std::io::{BufRead, BufReader, Write};

    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let workspace = create_test_workspace(&temp, backend);

    // Spawn daemon with 3-second timeout (gives more margin for timing)
    let timeout_secs = 3;
    let mut child = spawn_daemon(&workspace, timeout_secs).expect("failed to spawn daemon");

    let endpoint = wait_for_daemon_endpoint(&mut child, &workspace);

    // Send requests at intervals shorter than timeout, for longer than timeout
    // This should keep the daemon alive
    // With 3s timeout and 1s intervals, we need to send requests frequently enough
    // that the idle checker (which runs every 1.5s) always sees recent activity
    for i in 0..6 {
        // Check if daemon is still running
        if let Ok(Some(status)) = child.try_wait() {
            panic!("daemon exited unexpectedly before iteration {i} with status: {status:?}");
        }

        // Connect and send a simple request
        let mut stream = connect_to_daemon(&endpoint, &mut child, i);

        // Send a help request (using correct Address format)
        let request = r#"{"protocol_version":1,"id":"test","op":{"kind":"help","params":{"address":{"kind":"root"}}}}"#;
        writeln!(stream, "{request}").unwrap();

        // Read response
        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();

        // Verify daemon is still running
        assert!(
            child.try_wait().unwrap().is_none(),
            "daemon should still be running after activity on iteration {i}"
        );

        // Wait 500ms before next request (well under 3s timeout)
        if i < 5 {
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    // Total time so far: ~3 seconds of activity
    // Now stop sending requests and wait for timeout
    std::thread::sleep(Duration::from_secs(timeout_secs + 2));

    // Daemon should have exited
    match child.try_wait() {
        Ok(Some(status)) => {
            assert!(status.success(), "daemon should exit cleanly");
        }
        Ok(None) => {
            let _ = child.kill();
            panic!("daemon should have exited after idle timeout");
        }
        Err(e) => {
            panic!("error checking daemon status: {e}");
        }
    }
}

/// Stress test: rapid burst of requests should not cause premature exit.
#[test_matrix(["sqlite"])]
fn test_daemon_handles_rapid_activity(backend: &str) {
    use std::io::{BufRead, BufReader, Write};

    let temp = tempfile::tempdir().expect("failed to create tempdir");
    let workspace = create_test_workspace(&temp, backend);

    // Spawn daemon with 2-second timeout
    let timeout_secs = 2;
    let mut child = spawn_daemon(&workspace, timeout_secs).expect("failed to spawn daemon");

    let endpoint = wait_for_daemon_endpoint(&mut child, &workspace);

    // Send 20 rapid requests with no delay between them
    for i in 0..20 {
        let mut stream = connect_to_daemon(&endpoint, &mut child, i);

        let request = r#"{"protocol_version":1,"id":"test","op":{"kind":"help","params":{"address":{"kind":"root"}}}}"#;
        writeln!(stream, "{request}").unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
    }

    // Daemon should still be running after rapid burst
    assert!(
        child.try_wait().unwrap().is_none(),
        "daemon should still be running after rapid activity"
    );

    // Now wait for idle timeout
    std::thread::sleep(Duration::from_secs(timeout_secs + 2));

    // Daemon should have exited
    match child.try_wait() {
        Ok(Some(status)) => {
            assert!(status.success(), "daemon should exit cleanly");
        }
        Ok(None) => {
            let _ = child.kill();
            panic!("daemon should have exited after idle timeout");
        }
        Err(e) => {
            panic!("error checking daemon status: {e}");
        }
    }
}
