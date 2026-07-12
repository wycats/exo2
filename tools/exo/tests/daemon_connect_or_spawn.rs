#![allow(clippy::disallowed_methods)] // integration tests use real fs/process/timing APIs

//! Integration tests for daemon connect-or-spawn functionality.

#[macro_use]
mod test_support;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_matrix;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Kill any daemon running for a workspace. Must be called at the end of
/// every test that spawns a daemon to prevent contamination of subsequent tests.
fn kill_test_daemon(workspace: &Path) {
    let Ok(paths) = exo::daemon::paths_for_workspace(workspace) else {
        #[cfg(windows)]
        kill_windows_daemons_for_workspace(workspace);
        return;
    };
    if let Ok(pid_str) = std::fs::read_to_string(paths.pid_path()) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
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
            // Wait briefly for cleanup
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    #[cfg(windows)]
    kill_windows_daemons_for_workspace(workspace);
    let _ = paths.endpoint().remove_stale();
    let _ = std::fs::remove_file(paths.pid_path());
    let _ = std::fs::remove_file(paths.lock_path());
    let _ = std::fs::remove_file(paths.identity_path());
}

#[cfg(windows)]
fn kill_windows_daemons_for_workspace(workspace: &Path) {
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
    fn new(workspace: &Path) -> Self {
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

fn run_git_ok(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_init(cwd: &Path) {
    run_git_ok(cwd, &["init"]);
}

fn git_commit_all(cwd: &Path) {
    run_git_ok(cwd, &["add", "."]);
    run_git_ok(
        cwd,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "-m",
            "init",
        ],
    );
}

fn create_primary_and_linked_worktree(temp: &TempDir) -> (PathBuf, PathBuf) {
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&primary).unwrap();
    git_init(&primary);
    test_support::exo_init_with_storage(&primary, "sqlite");
    git_commit_all(&primary);
    run_git_ok(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            "linked-test",
            linked.to_str().unwrap(),
        ],
    );
    (primary, linked)
}

fn assert_db_has_epoch(db_path: &Path, title: &str) {
    let loader = exo::context::SqliteLoader::open(db_path).expect("open sqlite db");
    let state = loader.load_state().expect("load sqlite state");
    assert!(
        state.epochs.iter().any(|epoch| epoch.title == title),
        "expected {} to contain epoch {title:?}",
        db_path.display()
    );
}

fn epoch_count(db_path: &Path, title: &str) -> usize {
    let loader = exo::context::SqliteLoader::open(db_path).expect("open sqlite db");
    loader
        .load_state()
        .expect("load sqlite state")
        .epochs
        .iter()
        .filter(|epoch| epoch.title == title)
        .count()
}

fn runtime_outcome_completed(path: &Path, request_id: &str) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(connection) = exosuit_storage::Connection::open_with_flags(
        path,
        exosuit_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return false;
    };
    connection
        .query_row(
            "SELECT response_json IS NOT NULL FROM daemon_request_outcomes
             WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .unwrap_or(false)
}

async fn send_machine_request(
    stream: exo::daemon_transport::DaemonStream,
    request: &str,
) -> serde_json::Value {
    send_machine_request_with_timeout(stream, request, Duration::from_secs(30)).await
}

async fn send_machine_request_with_timeout(
    stream: exo::daemon_transport::DaemonStream,
    request: &str,
    timeout: Duration,
) -> serde_json::Value {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    writer.write_all(request.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();

    let response = tokio::time::timeout(timeout, lines.next_line())
        .await
        .expect("timeout waiting for response")
        .expect("IO error")
        .expect("no response");

    serde_json::from_str(&response).expect("valid response json")
}

fn run_exo_status(workspace: &Path, envs: &[(&str, &OsStr)]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exo"));
    command
        .arg("status")
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[cfg(unix)]
fn run_exo_json_command(workspace: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["--format", "json"])
        .args(args)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn run_exo_status_from_subdir(workspace: &Path, subdir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .arg("status")
        .current_dir(subdir)
        .env("EXO_DAEMON_DIAGNOSTICS", "1")
        .env(
            "EXO_DAEMON_DIAG_PATH",
            workspace.join("daemon-subdir-diagnostics.ndjson"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn run_exo_daemon_ensure(workspace: &Path, direct: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exo"));
    command.arg("--format").arg("json");
    if direct {
        command.arg("--direct");
    }
    command
        .arg("daemon")
        .arg("ensure")
        .arg("--workspace")
        .arg(workspace)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn run_exo_daemon_restart(workspace: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .arg("--format")
        .arg("json")
        .arg("--direct")
        .arg("daemon")
        .arg("restart")
        .arg("--workspace")
        .arg(workspace)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn run_exo_daemon_status(workspace: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_exo"))
        .arg("--format")
        .arg("json")
        .arg("--direct")
        .arg("daemon")
        .arg("status")
        .arg("--workspace")
        .arg(workspace)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn parse_cli_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be valid json: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[cfg(unix)]
fn process_group_id(pid: u32) -> i32 {
    let pid = i32::try_from(pid).expect("process ID should fit in i32");
    nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid)))
        .expect("read process group")
        .as_raw()
}

fn read_ndjson(path: &Path) -> Vec<serde_json::Value> {
    let contents = std::fs::read_to_string(path).unwrap();
    let lines = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let last_index = lines.len().saturating_sub(1);

    lines
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| match serde_json::from_str(line) {
            Ok(value) => Some(value),
            Err(_) if index == last_index => None,
            Err(error) => panic!("diagnostics line should be valid json: {error}"),
        })
        .collect()
}

fn event_names(events: &[serde_json::Value]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| event.get("event").and_then(serde_json::Value::as_str))
        .collect()
}

fn assert_has_event(events: &[serde_json::Value], name: &str) {
    assert!(
        event_names(events).contains(&name),
        "expected diagnostics to contain {name:?}; got {:?}",
        event_names(events)
    );
}

fn wait_for_diagnostics_event(path: &Path, name: &str) -> Vec<serde_json::Value> {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if path.exists() {
            let events = read_ndjson(path);
            if event_names(&events).contains(&name) {
                return events;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if path.exists() {
        read_ndjson(path)
    } else {
        Vec::new()
    }
}

/// Create a workspace with required files for the given backend.
fn create_test_workspace(dir: &TempDir, backend: &str) -> PathBuf {
    let workspace = dir.path().to_path_buf();
    git_init(&workspace);
    test_support::exo_init_with_storage(&workspace, backend);
    workspace
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_connect_to_daemon_no_daemon(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);

    // Should fail when no daemon is running
    let result = exo::daemon::connect_to_daemon(&workspace).await;
    assert!(result.is_err());
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_wait_for_socket_timeout(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);

    // Should timeout when no daemon is spawned
    let result = exo::daemon::wait_for_socket(&workspace, Duration::from_millis(100)).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_ensure_daemon_spawns_and_connects(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);

    // Verify no daemon is running initially
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    assert!(!paths.endpoint().is_connectable_blocking());

    // ensure_daemon should spawn a daemon and connect
    let _guard = DaemonGuard::new(&workspace);
    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("ensure_daemon should spawn real exo binary and connect");

    // Verify we can communicate with the daemon
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    // Send a simple help request
    let request = r#"{"protocol_version":1,"id":"test-1","op":{"kind":"help","params":{"address":{"kind":"root"}}}}"#;
    writer.write_all(request.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();

    // Read response
    let response = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .expect("timeout waiting for response")
        .expect("IO error")
        .expect("no response");

    // Verify we got a valid JSON response
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed.get("id").and_then(|v| v.as_str()), Some("test-1"));
    assert_eq!(parsed.get("status").and_then(|v| v.as_str()), Some("ok"));
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn diagnostics_disabled_by_default_does_not_create_runtime_file(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    let diagnostics_path = paths.runtime_dir().join("daemon-diagnostics.ndjson");

    let output = run_exo_status(&workspace, &[]);

    assert!(
        output.status.success(),
        "exo status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !diagnostics_path.exists(),
        "diagnostics file should not exist when EXO_DAEMON_DIAGNOSTICS is absent"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn diagnostics_enabled_with_explicit_path_writes_request_events(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let diagnostics_path = dir.path().join("explicit-daemon-diagnostics.ndjson");

    let output = run_exo_status(
        &workspace,
        &[
            ("EXO_DAEMON_DIAGNOSTICS", OsStr::new("1")),
            ("EXO_DAEMON_DIAG_PATH", diagnostics_path.as_os_str()),
        ],
    );

    assert!(
        output.status.success(),
        "exo status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = wait_for_diagnostics_event(&diagnostics_path, "request.write_end");

    assert_has_event(&events, "daemon.start");
    assert_has_event(&events, "socket.accept");
    assert_has_event(&events, "request.handler_start");
    assert_has_event(&events, "request.handler_end");
    assert_has_event(&events, "request.write_end");
    for event in events {
        assert!(
            event
                .get("event")
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "diagnostic event should include an event name: {event}"
        );
    }
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn diagnostics_env_propagates_through_spawn_daemon(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let diagnostics_path = dir.path().join("spawn-propagated-diagnostics.ndjson");

    let output = run_exo_status(
        &workspace,
        &[
            ("EXO_DAEMON_DIAGNOSTICS", OsStr::new("1")),
            ("EXO_DAEMON_DIAG_PATH", diagnostics_path.as_os_str()),
        ],
    );

    assert!(
        output.status.success(),
        "exo status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = wait_for_diagnostics_event(&diagnostics_path, "daemon.start");

    assert_has_event(&events, "daemon.start");
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_ensure_daemon_reuses_existing(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    // First call spawns daemon
    let stream1 = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("first ensure_daemon should spawn real exo binary");
    drop(stream1);

    // Small delay to ensure daemon is fully ready
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second call should reuse existing daemon (not spawn new one)
    let stream2 = exo::daemon::ensure_daemon(&workspace).await;
    assert!(stream2.is_ok(), "Second ensure_daemon should succeed");

    drop(stream2);
}

#[test_matrix(["sqlite"])]
#[cfg(unix)]
#[tokio::test]
async fn test_stale_socket_cleanup(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    // Create .runtime directory
    std::fs::create_dir_all(paths.runtime_dir()).unwrap();

    // Create a stale socket file (no actual daemon)
    std::fs::write(paths.socket_path(), "").unwrap();

    // Create a stale PID file with a non-existent PID
    // Use PID 1 which exists but we can't connect to, or a very high PID
    std::fs::write(paths.pid_path(), "999999999").unwrap();

    // ensure_daemon should detect stale files, clean up, and spawn new daemon
    let result = exo::daemon::ensure_daemon(&workspace).await;
    assert!(
        result.is_ok(),
        "ensure_daemon should clean stale files and spawn"
    );

    drop(result);
}

#[test_matrix(["sqlite"])]
#[cfg(unix)]
#[tokio::test]
async fn test_ensure_daemon_recovers_when_pid_lock_holds_unlinked_socket(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("initial ensure_daemon should spawn real daemon");
    drop(stream);

    let old_pid: u32 = std::fs::read_to_string(paths.pid_path())
        .expect("read daemon pid")
        .trim()
        .parse()
        .expect("daemon pid is numeric");
    std::fs::remove_file(paths.socket_path()).expect("unlink daemon socket");
    assert!(
        !paths.socket_path().exists(),
        "test setup should remove the filesystem socket path"
    );

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("ensure_daemon should restart a daemon with a held PID lock but missing socket");
    drop(stream);

    let new_pid: u32 = std::fs::read_to_string(paths.pid_path())
        .expect("read replacement daemon pid")
        .trim()
        .parse()
        .expect("replacement daemon pid is numeric");
    assert_ne!(new_pid, old_pid, "stale daemon should be replaced");
    assert!(
        paths.socket_path().exists(),
        "replacement daemon should recreate the socket path"
    );
}

/// Test that concurrent ensure_daemon calls don't cause double-spawn.
///
/// This tests the flock-based race condition prevention:
/// - Multiple clients calling ensure_daemon simultaneously
/// - Only one should spawn the daemon
/// - Others should wait for the socket and connect
#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_concurrent_ensure_daemon_no_double_spawn(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    // Ensure no daemon is running
    assert!(!paths.endpoint().is_connectable_blocking());

    // Spawn multiple concurrent ensure_daemon calls
    let workspace1 = workspace.clone();
    let workspace2 = workspace.clone();
    let workspace3 = workspace.clone();

    let (result1, result2, result3) = tokio::join!(
        exo::daemon::ensure_daemon(&workspace1),
        exo::daemon::ensure_daemon(&workspace2),
        exo::daemon::ensure_daemon(&workspace3),
    );

    assert!(
        result1.is_ok() && result2.is_ok() && result3.is_ok(),
        "All concurrent ensure_daemon calls should succeed"
    );

    // Verify only one daemon is running (check PID file)
    let pid_content = std::fs::read_to_string(paths.pid_path()).unwrap();
    let pid: u32 = pid_content.trim().parse().unwrap();
    assert!(pid > 0, "PID file should contain valid PID");

    drop(result1);
    drop(result2);
    drop(result3);
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_connect_or_spawn_uses_project_runtime_not_workspace_runtime(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("ensure_daemon should create project runtime socket");
    drop(stream);

    assert!(
        paths.endpoint().is_connectable_blocking(),
        "project endpoint should accept connections"
    );
    assert!(paths.pid_path().exists(), "project PID should exist");
    assert!(
        !workspace.join(".runtime").exists(),
        "connect-or-spawn must not create workspace .runtime"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_ensure_cli_returns_actual_runtime_metadata(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let output = run_exo_daemon_ensure(&workspace, true);

    assert!(
        output.status.success(),
        "daemon ensure failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = response.get("result").expect("daemon ensure result");
    assert_eq!(
        result.get("kind").and_then(|v| v.as_str()),
        Some("daemon.ensure")
    );
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        result.get("workspace_root").and_then(|v| v.as_str()),
        Some(workspace.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        result.get("runtime_dir").and_then(|v| v.as_str()),
        Some(paths.runtime_dir().to_str().unwrap())
    );
    assert_eq!(
        result.get("socket_path").and_then(|v| v.as_str()),
        Some(paths.socket_path().to_str().unwrap())
    );
    assert_eq!(
        result.get("pid_path").and_then(|v| v.as_str()),
        Some(paths.pid_path().to_str().unwrap())
    );
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("spawned")
    );
    assert_eq!(
        result.get("connected").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(result.get("spawned").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(result.get("reused").and_then(|v| v.as_bool()), Some(false));
    assert!(
        result.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "daemon ensure should report daemon PID: {result}"
    );
    assert!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .is_some_and(|diagnostics| !diagnostics.is_empty()),
        "daemon ensure should include diagnostics: {result}"
    );
    assert!(
        paths.endpoint().is_connectable_blocking(),
        "project endpoint should accept connections"
    );
    assert!(paths.pid_path().exists(), "project PID should exist");
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_ensure_cli_survives_short_lived_parent_exit(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    // `run_exo_daemon_ensure` is a short-lived parent process: it starts the
    // daemon, returns its report, and exits before this test checks health.
    let ensure = run_exo_daemon_ensure(&workspace, true);
    assert!(
        ensure.status.success(),
        "daemon ensure failed: {}",
        String::from_utf8_lossy(&ensure.stderr)
    );
    #[cfg(unix)]
    {
        let pid = parse_cli_json(&ensure)
            .get("result")
            .and_then(|result| result.get("pid"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .expect("daemon ensure should report a daemon PID");
        assert_ne!(
            process_group_id(pid),
            process_group_id(std::process::id()),
            "spawned daemon must not inherit the short-lived parent's process group"
        );
    }

    tokio::time::sleep(Duration::from_millis(250)).await;
    let status = run_exo_daemon_status(&workspace);
    assert!(
        status.status.success(),
        "daemon status failed after ensure parent exit: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status = parse_cli_json(&status);
    let result = status.get("result").expect("daemon status result");
    assert_eq!(
        result.get("state").and_then(serde_json::Value::as_str),
        Some("running_current"),
        "daemon should remain healthy after its ensure parent exits: {result}"
    );
    assert_eq!(
        result.get("pid_alive").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        result
            .get("socket_connectable")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        result.get("probe_ok").and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_restart_cli_returns_forced_restart_metadata(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    let ensure = run_exo_daemon_ensure(&workspace, true);
    assert!(
        ensure.status.success(),
        "daemon ensure failed before restart: {}",
        String::from_utf8_lossy(&ensure.stderr)
    );

    let output = run_exo_daemon_restart(&workspace);

    assert!(
        output.status.success(),
        "daemon restart failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = response.get("result").expect("daemon restart result");
    assert_eq!(
        result.get("kind").and_then(|v| v.as_str()),
        Some("daemon.restart")
    );
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("spawned")
    );
    assert_eq!(
        result.get("connected").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .is_some_and(|diagnostics| {
                diagnostics
                    .iter()
                    .any(|entry| entry.as_str() == Some("forced daemon restart"))
            }),
        "daemon restart should include forced restart diagnostics: {result}"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_status_reports_stopped_without_starting_daemon(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    kill_test_daemon(&workspace);

    let output = run_exo_daemon_status(&workspace);

    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = response.get("result").expect("daemon status result");
    assert_eq!(
        result.get("kind").and_then(|v| v.as_str()),
        Some("daemon.status")
    );
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("stopped")
    );
    assert_eq!(
        result.get("runtime_dir").and_then(|v| v.as_str()),
        Some(paths.runtime_dir().to_str().unwrap())
    );
    assert_eq!(
        result.get("socket_path").and_then(|v| v.as_str()),
        Some(paths.socket_path().to_str().unwrap())
    );
    assert_eq!(
        result.get("pid_path").and_then(|v| v.as_str()),
        Some(paths.pid_path().to_str().unwrap())
    );
    assert_eq!(
        result.get("identity_path").and_then(|v| v.as_str()),
        Some(paths.identity_path().to_str().unwrap())
    );
    assert_eq!(
        result.get("socket_exists").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        result.get("socket_connectable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        result.get("identity_exists").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(
        !paths.socket_path().exists(),
        "daemon status should inspect without creating a socket"
    );
    assert!(
        !paths.pid_path().exists(),
        "daemon status should inspect without creating a PID file"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_status_reports_running_current_identity(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("ensure_daemon should spawn real exo binary and connect");
    drop(stream);

    let output = run_exo_daemon_status(&workspace);

    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    let result = response.get("result").expect("daemon status result");
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("running_current")
    );
    assert!(
        result.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "daemon status should report a live daemon PID: {result}"
    );
    assert_eq!(
        result.get("pid_alive").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("socket_exists").and_then(|v| v.as_bool()),
        Some(paths.socket_path().exists())
    );
    assert_eq!(
        result.get("socket_connectable").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(result.get("probe_ok").and_then(|v| v.as_bool()), Some(true));
    assert!(
        result
            .get("endpoint")
            .and_then(|v| v.as_str())
            .is_some_and(|endpoint| !endpoint.is_empty()),
        "daemon status should report the platform endpoint: {result}"
    );
    assert_eq!(
        result.get("identity_exists").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("identity_readable").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result
            .get("identity_matches_workspace")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result
            .get("identity_matches_executable")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(result.get("recorded_identity").is_some());
    assert!(result.get("current_identity").is_some());
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_status_reports_stale_identity_without_repairing(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("ensure_daemon should spawn real exo binary and connect");
    drop(stream);
    assert!(
        paths.identity_path().exists(),
        "daemon identity should exist"
    );

    std::fs::remove_file(paths.identity_path()).expect("remove daemon identity");

    let output = run_exo_daemon_status(&workspace);

    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    let result = response.get("result").expect("daemon status result");
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("stale_identity")
    );
    assert_eq!(
        result.get("socket_connectable").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("identity_exists").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        result.get("identity_readable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(
        result
            .get("issue")
            .and_then(|v| v.as_str())
            .is_some_and(|issue| issue.contains("identity")),
        "daemon status should explain stale identity: {result}"
    );
    assert!(
        !paths.identity_path().exists(),
        "daemon status should not repair missing daemon identity"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_status_reports_unreachable_socket_without_starting_daemon(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    kill_test_daemon(&workspace);
    std::fs::create_dir_all(paths.runtime_dir()).unwrap();
    std::fs::write(paths.socket_path(), b"not a socket").unwrap();

    let output = run_exo_daemon_status(&workspace);

    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    let result = response.get("result").expect("daemon status result");
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("unreachable")
    );
    assert_eq!(
        result.get("socket_exists").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        result.get("socket_connectable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(
        !paths.pid_path().exists(),
        "daemon status should not create a PID file"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_status_reports_unreachable_pid_file_without_socket(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    kill_test_daemon(&workspace);
    std::fs::create_dir_all(paths.runtime_dir()).unwrap();
    std::fs::write(paths.pid_path(), b"not-a-pid").unwrap();

    let output = run_exo_daemon_status(&workspace);

    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    let result = response.get("result").expect("daemon status result");
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("unreachable")
    );
    assert!(result.get("pid").is_none_or(serde_json::Value::is_null));
    assert!(
        result
            .get("pid_alive")
            .is_none_or(serde_json::Value::is_null)
    );
    assert_eq!(
        result.get("socket_exists").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn daemon_status_reports_invalid_workspace() {
    let output = Command::new(env!("CARGO_BIN_EXE_exo"))
        .args([
            "--format",
            "json",
            "--direct",
            "daemon",
            "status",
            "--workspace",
            "/",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "daemon status reports invalid workspace as a status payload"
    );
    let response = parse_cli_json(&output);
    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = response.get("result").expect("daemon status result");
    assert_eq!(result.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("invalid_workspace")
    );
    assert!(
        result.get("issue").and_then(|v| v.as_str()).is_some_and(
            |message| message.contains(exo::daemon::FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR)
        ),
        "invalid workspace issue should include root guard message: {result}"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn ensure_daemon_restarts_existing_daemon_when_identity_is_missing(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let first = exo::daemon::ensure_daemon_with_report(&workspace)
        .await
        .expect("first ensure should spawn daemon")
        .into_report();
    let first_pid = first.pid.expect("first ensure should report pid");
    assert!(
        paths.identity_path().exists(),
        "daemon identity should exist"
    );

    std::fs::remove_file(paths.identity_path()).expect("remove daemon identity");

    let second = exo::daemon::ensure_daemon_with_report(&workspace)
        .await
        .expect("second ensure should restart stale daemon")
        .into_report();
    let second_pid = second.pid.expect("second ensure should report pid");

    assert_ne!(
        first_pid, second_pid,
        "missing daemon identity should force automatic daemon restart"
    );
    assert_eq!(second.state, exo::daemon::DaemonEnsureState::Spawned);
    assert!(
        second
            .diagnostics
            .iter()
            .any(|message| message.contains("terminated stale daemon process")),
        "restart diagnostics should mention stale daemon termination: {:?}",
        second.diagnostics
    );
    assert!(
        paths.identity_path().exists(),
        "restarted daemon should write fresh identity"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn ensure_daemon_restarts_probed_daemon_when_pid_file_is_missing_or_invalid(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let first = exo::daemon::ensure_daemon_with_report(&workspace)
        .await
        .expect("first ensure should spawn daemon")
        .into_report();
    let first_pid = first.pid.expect("first ensure should report pid");
    std::fs::remove_file(paths.pid_path()).expect("remove daemon pid file");

    let second = exo::daemon::ensure_daemon_with_report(&workspace)
        .await
        .expect("second ensure should restart daemon discovered by probe")
        .into_report();
    let second_pid = second.pid.expect("second ensure should report pid");

    assert_ne!(
        first_pid, second_pid,
        "missing PID metadata should restart the daemon identified by the socket probe"
    );
    assert!(
        second
            .diagnostics
            .iter()
            .any(|message| message.contains("terminated stale daemon process")),
        "restart diagnostics should mention probed daemon termination: {:?}",
        second.diagnostics
    );

    std::fs::write(paths.pid_path(), "not-a-pid").expect("write invalid daemon pid file");
    let third = exo::daemon::ensure_daemon_with_report(&workspace)
        .await
        .expect("third ensure should restart daemon discovered past invalid PID metadata")
        .into_report();
    let third_pid = third.pid.expect("third ensure should report pid");
    assert_ne!(
        second_pid, third_pid,
        "invalid PID metadata should restart the daemon identified by the socket probe"
    );
    assert!(
        third
            .diagnostics
            .iter()
            .any(|message| message.contains("terminated stale daemon process")),
        "restart diagnostics should mention probed daemon termination: {:?}",
        third.diagnostics
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn ensure_daemon_restarts_existing_daemon_when_workspace_identity_differs(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let (primary, linked) = create_primary_and_linked_worktree(&dir);
    let _guard = DaemonGuard::new(&primary);
    let paths = exo::daemon::paths_for_workspace(&primary).unwrap();

    let first = exo::daemon::ensure_daemon_with_report(&primary)
        .await
        .expect("first ensure should spawn daemon")
        .into_report();
    let first_pid = first.pid.expect("first ensure should report pid");

    let second = exo::daemon::ensure_daemon_with_report(&linked)
        .await
        .expect("linked ensure should restart mismatched daemon")
        .into_report();
    let second_pid = second.pid.expect("second ensure should report pid");

    assert_eq!(paths.socket_path(), second.socket_path);
    assert_ne!(
        first_pid, second_pid,
        "workspace identity mismatch should force automatic daemon restart"
    );
    assert_eq!(second.state, exo::daemon::DaemonEnsureState::Spawned);
    assert!(
        second
            .diagnostics
            .iter()
            .any(|message| message.contains("terminated stale daemon process")),
        "restart diagnostics should mention stale daemon termination: {:?}",
        second.diagnostics
    );
}

#[cfg(unix)]
#[test_matrix(["sqlite"])]
fn tool_calls_repair_a_wedged_daemon_between_status_and_task_list(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    let status = run_exo_json_command(&workspace, &["status"]);
    assert!(
        status.status.success(),
        "initial status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        parse_cli_json(&status)
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("ok")
    );

    let first_pid = std::fs::read_to_string(paths.pid_path())
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(first_pid),
        nix::sys::signal::Signal::SIGSTOP,
    )
    .unwrap();

    let wedged_status = run_exo_daemon_status(&workspace);
    assert!(
        wedged_status.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&wedged_status.stderr)
    );
    let wedged_status = parse_cli_json(&wedged_status);
    let wedged_result = wedged_status.get("result").expect("daemon status result");
    assert_eq!(
        wedged_result
            .get("state")
            .and_then(serde_json::Value::as_str),
        Some("unreachable")
    );
    assert_eq!(
        wedged_result
            .get("socket_connectable")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        wedged_result
            .get("probe_ok")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );

    let tasks = run_exo_json_command(&workspace, &["task", "list"]);
    assert!(
        tasks.status.success(),
        "task list should repair the wedged daemon: {}",
        String::from_utf8_lossy(&tasks.stderr)
    );
    assert_eq!(
        parse_cli_json(&tasks)
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("ok")
    );
    let second_pid = std::fs::read_to_string(paths.pid_path())
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    assert_ne!(
        first_pid, second_pid,
        "the wedged daemon should be replaced"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_ensure_without_direct_bypasses_daemon_dispatch(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);

    let output = run_exo_daemon_ensure(&workspace, false);

    assert!(
        output.status.success(),
        "daemon ensure without --direct should run as a lifecycle command, not daemon dispatch: stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let response = parse_cli_json(&output);
    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));
    assert_eq!(
        response
            .get("result")
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str()),
        Some("daemon.ensure")
    );
}

#[test]
fn daemon_ensure_rejects_filesystem_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_exo"))
        .args([
            "--format",
            "json",
            "--direct",
            "daemon",
            "ensure",
            "--workspace",
            "/",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(!output.status.success(), "daemon ensure / should fail");
    let response = parse_cli_json(&output);
    assert_eq!(
        response.get("status").and_then(|v| v.as_str()),
        Some("error")
    );
    assert!(
        response
            .get("error")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .is_some_and(
                |message| message.contains(exo::daemon::FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR)
            ),
        "error should include root guard message: {response}"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_cli_daemon_dispatch_from_subdir_uses_canonical_project_root(backend: &str) {
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let subdir = workspace.join("nested").join("deeper");
    std::fs::create_dir_all(&subdir).unwrap();

    let output = run_exo_status_from_subdir(&workspace, &subdir);

    assert!(
        output.status.success(),
        "exo status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();
    assert!(
        paths.endpoint().is_connectable_blocking(),
        "project endpoint should accept connections"
    );
    assert!(paths.pid_path().exists(), "project PID should exist");

    let diagnostics_path = workspace.join("daemon-subdir-diagnostics.ndjson");
    let events = wait_for_diagnostics_event(&diagnostics_path, "daemon.start");
    let daemon_start = events
        .iter()
        .find(|event| {
            event.get("event").and_then(serde_json::Value::as_str) == Some("daemon.start")
        })
        .expect("daemon.start event should be recorded");
    assert_eq!(
        daemon_start
            .get("workspace")
            .and_then(serde_json::Value::as_str),
        Some(workspace.canonicalize().unwrap().to_str().unwrap())
    );
}

#[test]
fn test_daemon_run_rejects_filesystem_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["daemon", "run", "--workspace", "/", "--timeout", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(!output.status.success(), "daemon run / should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(exo::daemon::FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR),
        "stderr should include root guard message, got: {stderr}"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn test_primary_and_linked_worktree_share_daemon_runtime(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let (primary, linked) = create_primary_and_linked_worktree(&dir);
    let _guard = DaemonGuard::new(&primary);

    let primary_paths = exo::daemon::paths_for_workspace(&primary).unwrap();
    let linked_paths = exo::daemon::paths_for_workspace(&linked).unwrap();
    assert_eq!(primary_paths.socket_path(), linked_paths.socket_path());
    assert_eq!(primary_paths.pid_path(), linked_paths.pid_path());

    let primary_stream = exo::daemon::ensure_daemon(&primary)
        .await
        .expect("primary should spawn daemon");
    drop(primary_stream);

    assert!(primary_paths.endpoint().is_connectable_blocking());
    assert!(primary_paths.pid_path().exists());

    let linked_stream = exo::daemon::connect_to_daemon(&linked)
        .await
        .expect("linked worktree should connect to primary daemon socket");
    drop(linked_stream);
    assert!(
        !primary.join(".runtime").exists(),
        "primary worktree should not get workspace .runtime"
    );
    assert!(
        !linked.join(".runtime").exists(),
        "linked worktree should not get workspace .runtime"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn linked_worktree_daemon_connection_writes_shared_project_db(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let (primary, linked) = create_primary_and_linked_worktree(&dir);
    let _guard = DaemonGuard::new(&primary);

    let primary_stream = exo::daemon::ensure_daemon(&primary)
        .await
        .expect("primary should spawn daemon");
    drop(primary_stream);

    let linked_stream = exo::daemon::ensure_daemon(&linked)
        .await
        .expect("linked worktree should connect to shared daemon socket");
    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "linked-epoch-add",
        "workspace_root": linked.canonicalize().expect("canonical linked worktree"),
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["epoch", "add"] },
                "input": { "title": "Linked Daemon Epoch" }
            }
        }
    });
    let response = send_machine_request(linked_stream, &request.to_string()).await;

    assert_eq!(response.get("status").and_then(|v| v.as_str()), Some("ok"));

    let primary_project =
        exo::project::Project::resolve(&primary).expect("resolve primary project");
    let linked_project = exo::project::Project::resolve(&linked).expect("resolve linked project");
    assert_eq!(primary_project.db_path(), linked_project.db_path());
    assert_db_has_epoch(&primary_project.db_path(), "Linked Daemon Epoch");

    assert!(
        !primary.join(".runtime").exists(),
        "primary legacy runtime dir should not exist"
    );
    assert!(
        !linked.join(".runtime").exists(),
        "linked legacy runtime dir should not exist"
    );
    assert!(
        !primary.join(".cache/exo.db").exists(),
        "primary legacy root DB should not exist"
    );
    assert!(
        !linked.join(".cache/exo.db").exists(),
        "linked legacy root DB should not exist"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_request_uses_the_issuing_linked_worktree(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let (primary, linked) = create_primary_and_linked_worktree(&dir);
    let _guard = DaemonGuard::new(&primary);

    let primary_stream = exo::daemon::ensure_daemon(&primary)
        .await
        .expect("primary should spawn daemon");
    drop(primary_stream);

    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "linked-project-resolve",
        "workspace_root": linked.canonicalize().expect("canonical linked worktree"),
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["project", "resolve"] },
                "input": {}
            }
        }
    });
    let stream = exo::daemon::connect_to_daemon(&linked)
        .await
        .expect("linked worktree should connect to shared daemon");
    let response = send_machine_request(stream, &request.to_string()).await;

    assert_eq!(response["status"], "ok", "{response}");
    assert_eq!(
        response["result"]["project"]["workspace_root"],
        linked
            .canonicalize()
            .expect("canonical linked worktree")
            .to_string_lossy()
            .as_ref(),
        "daemon command context should use the issuing worktree"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_and_direct_rfc_views_follow_the_issuing_linked_worktree(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let primary = dir.path().join("primary");
    let linked = dir.path().join("linked");
    std::fs::create_dir(&primary).unwrap();
    git_init(&primary);
    test_support::exo_init_with_storage(&primary, backend);
    std::fs::create_dir_all(primary.join("docs/rfcs/stage-1")).unwrap();
    std::fs::write(
        primary.join("docs/rfcs/stage-1/00001-workspace-view.md"),
        "<!-- exo:1 ulid:01daemonworkspace -->\n\n# RFC 1: Workspace View\n\n**Stage**: 1\n\n## Summary\n\nActive.\n",
    )
    .unwrap();
    git_commit_all(&primary);
    run_git_ok(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            "linked-rfc-view",
            linked.to_str().unwrap(),
        ],
    );
    std::fs::create_dir_all(linked.join("docs/rfcs/withdrawn")).unwrap();
    std::fs::rename(
        linked.join("docs/rfcs/stage-1/00001-workspace-view.md"),
        linked.join("docs/rfcs/withdrawn/00001-workspace-view.md"),
    )
    .unwrap();
    std::fs::write(
        linked.join("docs/rfcs/withdrawn/00001-workspace-view.md"),
        "<!-- exo:1 ulid:01daemonworkspace -->\n\n# RFC 1: Workspace View\n\n**Status**: Withdrawn\n**Stage**: 1\n**Reason**: This linked-worktree proposal is complete.\n\n## Summary\n\nHistorical.\n",
    )
    .unwrap();
    git_commit_all(&linked);

    let _guard = DaemonGuard::new(&primary);
    let primary_stream = exo::daemon::ensure_daemon(&primary)
        .await
        .expect("primary should spawn daemon");
    let primary_request = serde_json::json!({
        "protocol_version": 1,
        "id": "primary-rfc-view",
        "workspace_root": primary.canonicalize().expect("canonical primary worktree"),
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["rfc", "show"] },
                "input": { "id": "00001" }
            }
        }
    });
    let primary_response = send_machine_request(primary_stream, &primary_request.to_string()).await;

    let linked_stream = exo::daemon::connect_to_daemon(&linked)
        .await
        .expect("linked worktree should connect to shared daemon");
    let linked_request = serde_json::json!({
        "protocol_version": 1,
        "id": "linked-rfc-view",
        "workspace_root": linked.canonicalize().expect("canonical linked worktree"),
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["rfc", "show"] },
                "input": { "id": "00001" }
            }
        }
    });
    let linked_response = send_machine_request(linked_stream, &linked_request.to_string()).await;

    let direct = test_support::exo_cmd(&linked)
        .args(["--format", "json", "rfc", "show", "00001"])
        .assert()
        .success();
    let direct_response: serde_json::Value = serde_json::from_slice(&direct.get_output().stdout)
        .expect("direct RFC response should be valid JSON");

    assert_eq!(primary_response["status"], "ok", "{primary_response}");
    assert_eq!(primary_response["result"]["status"], "active");
    assert_eq!(primary_response["result"]["workspace_presence"], "present");
    assert_eq!(linked_response["status"], "ok", "{linked_response}");
    assert_eq!(linked_response["result"]["status"], "withdrawn");
    assert_eq!(
        linked_response["result"]["withdrawal_reason"],
        "This linked-worktree proposal is complete."
    );
    for field in [
        "id",
        "stage",
        "status",
        "filename",
        "withdrawal_reason",
        "document_source",
        "workspace_presence",
        "canonical_presence",
        "differs_from_canonical",
    ] {
        assert_eq!(
            linked_response["result"][field], direct_response["result"][field],
            "daemon and direct RFC views should agree for field {field}"
        );
    }
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_rejects_request_workspace_from_another_project(backend: &str) {
    assert_eq!(backend, "sqlite");
    let primary_dir = TempDir::new().unwrap();
    let primary = create_test_workspace(&primary_dir, backend);
    let _guard = DaemonGuard::new(&primary);
    let other_dir = TempDir::new().unwrap();
    let other = create_test_workspace(&other_dir, backend);

    let stream = exo::daemon::ensure_daemon(&primary)
        .await
        .expect("primary should spawn daemon");
    let request = serde_json::json!({
        "protocol_version": 1,
        "id": "foreign-project-resolve",
        "workspace_root": other.canonicalize().expect("canonical foreign workspace"),
        "op": {
            "kind": "call",
            "params": {
                "address": { "kind": "operation", "path": ["project", "resolve"] },
                "input": {}
            }
        }
    });
    let response = send_machine_request(stream, &request.to_string()).await;

    assert_eq!(response["status"], "error", "{response}");
    assert_eq!(
        response["error"]["code"], "precondition_failed",
        "{response}"
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message
                == "request workspace does not belong to this daemon's project and state root"),
        "foreign workspace rejection should identify the project boundary: {response}"
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn atomic_request_hydrates_projection_before_rollback(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let project = exo::project::Project::resolve(&workspace).expect("resolve project");
    let db_path = project.db_path();
    let writer = exo::context::SqliteWriter::open(&db_path).expect("open project writer");
    writer
        .add_epoch("Projected Epoch", Some("projected-epoch"), &[])
        .expect("add projected epoch");
    drop(writer);
    let database = exosuit_storage::open_database(&db_path).expect("open project database");
    let epochs_projection = exosuit_storage::dump_tables(database.connection())
        .expect("dump project state")
        .into_iter()
        .find_map(|(table, contents)| (table == "epochs_data").then_some(contents))
        .expect("epochs projection");
    drop(database);
    let projection_dir = workspace.join("docs/agent-context");
    std::fs::create_dir_all(&projection_dir).expect("create projection directory");
    std::fs::write(projection_dir.join("epochs.sql"), epochs_projection)
        .expect("write projected epoch");
    std::fs::remove_file(&db_path).expect("remove initialized project database");
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));

    let stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("spawn daemon");
    let failed = send_machine_request_with_timeout(
        stream,
        r#"{"protocol_version":1,"id":"failed-after-hydration","op":{"kind":"call","params":{"address":{"kind":"operation","path":["epoch","start"]},"input":{"id":"missing-epoch"}}}}"#,
        Duration::from_secs(60),
    )
    .await;

    assert_eq!(failed["status"], "error", "{failed}");
    assert!(
        failed.get("effect").is_none(),
        "rolled-back request must not report a committed write: {failed}"
    );
    assert_db_has_epoch(&db_path, "Projected Epoch");
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn daemon_replays_recorded_write_outcome_after_client_disconnect(backend: &str) {
    assert_eq!(backend, "sqlite");
    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&workspace);
    let project = exo::project::Project::resolve(&workspace).expect("resolve project");
    let request = r#"{"protocol_version":1,"id":"lost-write-response","op":{"kind":"call","params":{"address":{"kind":"operation","path":["epoch","add"]},"input":{"title":"Recovered Daemon Epoch"}}}}"#;

    let mut first_stream = exo::daemon::ensure_daemon(&workspace)
        .await
        .expect("spawn daemon");
    first_stream.write_all(request.as_bytes()).await.unwrap();
    first_stream.write_all(b"\n").await.unwrap();
    first_stream.flush().await.unwrap();
    drop(first_stream);

    let runtime_ledger_path = exo::daemon::paths_for_workspace(&workspace)
        .expect("daemon paths")
        .outcome_ledger_path();
    let wait_start = std::time::Instant::now();
    while !runtime_outcome_completed(&runtime_ledger_path, "lost-write-response")
        && wait_start.elapsed() < Duration::from_secs(60)
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        runtime_outcome_completed(&runtime_ledger_path, "lost-write-response"),
        "first request should persist its response before outcome replay"
    );
    assert_eq!(
        epoch_count(&project.db_path(), "Recovered Daemon Epoch"),
        1,
        "first request should commit before outcome replay"
    );
    let project_db = exosuit_storage::open_database(project.db_path())
        .expect("open project database after atomic request");
    let canonical_response: String = project_db
        .connection()
        .query_row(
            "SELECT response_json FROM atomic_request_outcomes WHERE request_id = ?1",
            ["lost-write-response"],
            |row| row.get(0),
        )
        .expect("canonical response should commit with project state");
    let canonical_response: serde_json::Value =
        serde_json::from_str(&canonical_response).expect("canonical response json");
    assert!(
        canonical_response["result"].get("post_write").is_none(),
        "canonical outcome stores the core response before idempotent finalization"
    );
    let command_events: i64 = project_db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events
             WHERE event_type = 'command' AND namespace = 'epoch' AND operation = 'add'",
            [],
            |row| row.get(0),
        )
        .expect("count command events");
    assert_eq!(
        command_events, 1,
        "command event should commit with state and canonical response"
    );

    let runtime_ledger = exosuit_storage::Connection::open(&runtime_ledger_path)
        .expect("open runtime outcome ledger");
    runtime_ledger
        .execute(
            "UPDATE daemon_request_outcomes
             SET instance_id = 'retired-instance', response_json = NULL, completed_at = NULL
             WHERE request_id = 'lost-write-response'",
            [],
        )
        .expect("simulate daemon loss after canonical commit");

    let replay_stream = exo::daemon::connect_to_daemon(&workspace)
        .await
        .expect("reconnect to daemon");
    let replay = send_machine_request(replay_stream, request).await;

    assert_eq!(replay["status"], "ok", "{replay}");
    assert_eq!(replay["id"], "lost-write-response", "{replay}");
    assert_eq!(
        epoch_count(&project.db_path(), "Recovered Daemon Epoch"),
        1,
        "replayed request id must not execute the mutation twice"
    );
    let command_events: i64 = project_db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events
             WHERE event_type = 'command' AND namespace = 'epoch' AND operation = 'add'",
            [],
            |row| row.get(0),
        )
        .expect("count command events after replay");
    assert_eq!(
        command_events, 1,
        "canonical replay must not duplicate the command event"
    );
    let runtime_response_recorded: bool = runtime_ledger
        .query_row(
            "SELECT response_json IS NOT NULL FROM daemon_request_outcomes
             WHERE request_id = 'lost-write-response'",
            [],
            |row| row.get(0),
        )
        .expect("read repaired runtime outcome");
    assert!(
        runtime_response_recorded,
        "canonical replay should repopulate the runtime outcome"
    );
    assert!(
        runtime_ledger_path.exists(),
        "daemon should persist request outcomes in the project runtime"
    );
}

/// Test that SIGTERM triggers graceful shutdown with cleanup.
///
/// This verifies:
/// - Daemon responds to SIGTERM
/// - Socket file is removed on shutdown
/// - PID file is removed on shutdown
#[test_matrix(["sqlite"])]
#[cfg(unix)]
#[tokio::test]
async fn test_sigterm_triggers_graceful_shutdown(backend: &str) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let dir = TempDir::new().unwrap();
    let workspace = create_test_workspace(&dir, backend);
    let paths = exo::daemon::paths_for_workspace(&workspace).unwrap();

    // Start a daemon
    let result = exo::daemon::ensure_daemon(&workspace).await;

    let stream = result.expect("ensure_daemon should spawn real exo binary");
    drop(stream);

    // Verify daemon is running
    assert!(paths.socket_path().exists(), "Socket should exist");
    assert!(paths.pid_path().exists(), "PID file should exist");

    // Read the PID
    let pid_content = std::fs::read_to_string(paths.pid_path()).unwrap();
    let pid: i32 = pid_content.trim().parse().unwrap();

    // Send SIGTERM
    kill(Pid::from_raw(pid), Signal::SIGTERM).expect("Failed to send SIGTERM");

    // Wait for cleanup (with timeout)
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);

    while start.elapsed() < timeout {
        if !paths.socket_path().exists() && !paths.pid_path().exists() {
            // Cleanup complete
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Check final state
    assert!(
        !paths.socket_path().exists(),
        "Socket should be cleaned up after SIGTERM"
    );
    assert!(
        !paths.pid_path().exists(),
        "PID file should be cleaned up after SIGTERM"
    );
}
