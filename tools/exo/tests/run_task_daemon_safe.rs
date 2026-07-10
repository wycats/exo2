#![allow(clippy::disallowed_methods)] // integration tests use real fs/process APIs

mod test_support;

use fs2::FileExt;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_matrix;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use exo::api::protocol::{
    Address, CallParams, Effect, Op, PROTOCOL_VERSION, RequestEnvelope, Status,
};
use exo::command::command_spec::CommandSpec;
use exo::command::registry::default_registry;
use exo::command::run::TASK_DIRECT_MODE_ENV;
use exo::daemon::LocalRuntimePaths;
use exo::daemon_transport::DaemonEndpoint;
use exo::project::ProjectResolver;

static DAEMON_INTEGRATION_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

fn git_init(root: &Path) {
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

fn git_success(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("git stdout is utf-8")
}

fn git_config_identity(root: &Path) {
    git_success(root, &["config", "user.name", "Exo Tests"]);
    git_success(root, &["config", "user.email", "exo-tests@example.test"]);
}

fn create_test_workspace(dir: &TempDir, backend: &str) -> PathBuf {
    let workspace = dir.path().to_path_buf();
    git_init(&workspace);
    test_support::exo_init_with_storage(&workspace, backend);
    workspace
}

fn create_test_workspace_at(root: &Path) {
    std::fs::create_dir_all(root).expect("create workspace");
    git_init(root);
    test_support::exo_init_with_storage(root, "sqlite");
}

fn append_task(root: &Path, name: &str, cmd: &str) {
    let config_path = root.join("exosuit.toml");
    let mut config = std::fs::read_to_string(&config_path).expect("read exosuit.toml");
    config.push_str("\n[tasks.");
    config.push_str(name);
    config.push_str("]\n");
    config.push_str("cmd = ");
    config.push_str(&toml::Value::String(cmd.to_string()).to_string());
    config.push_str("\n");
    config.push_str("desc = \"test task\"\n");
    config.push_str("cwd = \"root\"\n");
    std::fs::write(config_path, config).expect("write exosuit.toml");
}

#[cfg(not(windows))]
fn shell_quote(value: &Path) -> String {
    format!("'{}'", value.display().to_string().replace('\'', "'\\''"))
}

fn task_print_direct_mode_command() -> String {
    #[cfg(windows)]
    {
        format!(r#"echo|set /p dummy=%{TASK_DIRECT_MODE_ENV}%"#)
    }

    #[cfg(not(windows))]
    {
        format!(r#"printf '%s' "${{{TASK_DIRECT_MODE_ENV}:-}}""#)
    }
}

fn task_nested_status_command(exo_bin: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            r#"{} status >NUL && echo|set /p dummy=done"#,
            exo_bin.display()
        )
    }

    #[cfg(not(windows))]
    {
        format!("{} status >/dev/null && printf done", shell_quote(exo_bin))
    }
}

fn task_slow_build_command(marker: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            r#"echo started>{} & ping -n 11 127.0.0.1 >NUL & echo|set /p dummy=done"#,
            marker.display()
        )
    }

    #[cfg(not(windows))]
    {
        format!(
            "printf started > {}; sleep 5; printf done",
            shell_quote(marker)
        )
    }
}

fn exo_direct_with_env(
    root: &Path,
    home: &Path,
    config_home: &Path,
    args: &[&str],
) -> std::process::Output {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("exo"));
    command.current_dir(root);
    apply_test_home_env(&mut command, home, config_home);
    command
        .arg("--direct")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output().expect("run exo")
}

fn apply_test_home_env(command: &mut Command, home: &Path, config_home: &Path) {
    command
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home);
    #[cfg(windows)]
    command.env("USERPROFILE", home).env("APPDATA", config_home);
}

fn kill_test_daemon(root: &Path) {
    let Ok(paths) = exo::daemon::paths_for_workspace(root) else {
        #[cfg(windows)]
        kill_windows_daemons_for_workspace(root);
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
    }
    #[cfg(windows)]
    kill_windows_daemons_for_workspace(root);
    let _ = paths.endpoint().remove_stale();
    let _ = std::fs::remove_file(paths.pid_path());
}

#[cfg(windows)]
fn kill_windows_daemons_for_workspace(root: &Path) {
    let root = root.display().to_string();
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
        .env("EXO_TEST_WORKSPACE", root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

struct DaemonProcessGuard {
    child: Child,
}

impl DaemonProcessGuard {
    fn new(child: Child) -> Self {
        Self { child }
    }
}

impl Drop for DaemonProcessGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct DaemonGuard {
    root: PathBuf,
}

impl DaemonGuard {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        kill_test_daemon(&self.root);
    }
}

fn run_task_request(task_id: &str) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: format!("run-task-{task_id}"),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["run".to_string(), "task".to_string()],
            },
            input: serde_json::json!({ "id": task_id }),
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

fn status_request() -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "status-while-task-runs".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["status".to_string()],
            },
            input: serde_json::json!({}),
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

fn project_resolve_request(id: String) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["project".to_string(), "resolve".to_string()],
            },
            input: serde_json::json!({}),
        }),
        workspace_root: None,
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

async fn send_socket_request_with_timeout(
    endpoint: &DaemonEndpoint,
    request: &RequestEnvelope,
    timeout: Duration,
) -> serde_json::Value {
    let stream = tokio::time::timeout(timeout, endpoint.connect())
        .await
        .expect("daemon endpoint connect timed out")
        .expect("connect daemon endpoint");
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    let request_json = serde_json::to_string(request).expect("serialize request");
    writer
        .write_all(request_json.as_bytes())
        .await
        .expect("write request");
    writer.write_all(b"\n").await.expect("write newline");

    let response = tokio::time::timeout(timeout, lines.next_line())
        .await
        .expect("daemon request timed out")
        .expect("read daemon response")
        .expect("daemon closed connection");

    serde_json::from_str(&response).expect("daemon JSON response")
}

async fn wait_for_file(path: &Path, timeout: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {}", path.display());
}

async fn wait_for_daemon_endpoint(child: &mut Child, endpoint: &DaemonEndpoint) {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if endpoint.connect().await.is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("check daemon status") {
            let mut stderr = String::new();
            if let Some(mut stream) = child.stderr.take() {
                let _ = stream.read_to_string(&mut stderr);
            }
            panic!("daemon exited before socket was ready: {status}; stderr: {stderr}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "timed out waiting for daemon endpoint {}",
        endpoint.display()
    );
}

async fn send_daemon_request_with_timeout(
    root: &Path,
    request: &RequestEnvelope,
) -> serde_json::Value {
    let stream = exo::daemon::ensure_daemon(root)
        .await
        .expect("spawn/connect daemon");
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    let request_json = serde_json::to_string(request).expect("serialize request");
    writer
        .write_all(request_json.as_bytes())
        .await
        .expect("write request");
    writer.write_all(b"\n").await.expect("write newline");

    let response = tokio::time::timeout(std::time::Duration::from_secs(20), lines.next_line())
        .await
        .expect("daemon request timed out")
        .expect("read daemon response")
        .expect("daemon closed connection");

    serde_json::from_str(&response).expect("daemon JSON response")
}

#[test]
fn run_task_is_exec_effect_in_command_spec() {
    let spec = CommandSpec::from_registry(&default_registry());
    let op = spec.operation("run", "task").expect("run task operation");
    assert_eq!(op.effect, Effect::Exec);
}

#[test_matrix(["sqlite"])]
fn task_subprocess_receives_direct_mode_env(backend: &str) {
    let dir = TempDir::new().unwrap();
    let root = create_test_workspace(&dir, backend);
    append_task(&root, "print-direct-env", &task_print_direct_mode_command());

    let request =
        test_support::confirmed_machine_channel_request(run_task_request("print-direct-env"));
    let response = test_support::run_machine_channel_in_process(&root, &request);

    assert_eq!(response.status, Status::Ok);
    assert_eq!(response.effect, Some(Effect::Exec));
    let result = response.result.expect("result");
    assert_eq!(
        result.get("exit_code").and_then(serde_json::Value::as_i64),
        Some(0)
    );
    assert_eq!(
        result.get("stdout").and_then(serde_json::Value::as_str),
        Some("1")
    );
}

#[test_matrix(["sqlite"])]
#[tokio::test]
async fn nested_exo_in_task_completes_without_daemon_deadlock(backend: &str) {
    let _daemon_test_guard = DAEMON_INTEGRATION_TEST_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let root = create_test_workspace(&dir, backend);
    let _guard = DaemonGuard::new(&root);
    let exo_bin = assert_cmd::cargo::cargo_bin!("exo");
    append_task(
        &root,
        "nested-status",
        &task_nested_status_command(&exo_bin),
    );

    let request =
        test_support::confirmed_machine_channel_request(run_task_request("nested-status"));
    let response = send_daemon_request_with_timeout(&root, &request).await;

    assert_eq!(
        response.get("status").and_then(serde_json::Value::as_str),
        Some("ok")
    );
    assert_eq!(
        response.get("effect").and_then(serde_json::Value::as_str),
        Some("exec")
    );
    assert_eq!(
        response
            .get("result")
            .and_then(|result| result.get("stdout"))
            .and_then(serde_json::Value::as_str),
        Some("done")
    );
}

#[tokio::test]
async fn sidecar_parallel_reads_do_not_wait_for_auto_persist() {
    let _daemon_test_guard = DAEMON_INTEGRATION_TEST_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("w");
    let home = dir.path().join("h");
    let config_home = dir.path().join("c");
    let sidecar_root = dir.path().join("s");
    create_test_workspace_at(&root);

    let init = exo_direct_with_env(
        &root,
        &home,
        &config_home,
        &[
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "rt",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ],
    );
    assert!(
        init.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git_config_identity(&sidecar_root);
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(
        &sidecar_root,
        &["commit", "--allow-empty", "-m", "Baseline sidecar state"],
    );

    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&root)
        .expect("resolve sidecar project");
    assert_eq!(project.policy, exo::project::StatePolicy::Sidecar);
    let endpoint = LocalRuntimePaths::new(&root, &project).endpoint();

    let exo_bin = assert_cmd::cargo::cargo_bin!("exo");
    let mut daemon = Command::new(&exo_bin);
    daemon
        .current_dir(&root)
        .args([
            "daemon",
            "run",
            "--workspace",
            root.to_str().expect("workspace root is utf-8"),
            "--timeout",
            "30",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_test_home_env(&mut daemon, &home, &config_home);
    let mut daemon = daemon.spawn().expect("spawn daemon");
    wait_for_daemon_endpoint(&mut daemon, &endpoint).await;
    let _guard = DaemonProcessGuard::new(daemon);

    let head_before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    let auto_persist_commits_before = git_output(
        &sidecar_root,
        &[
            "log",
            "--format=%s",
            "--grep",
            "Auto-persist Exosuit sidecar state",
        ],
    )
    .lines()
    .count();

    let lock_path = sidecar_root.join(".git/exo-state.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .expect("open sidecar runtime lock");
    lock_file.lock_exclusive().expect("lock sidecar runtime");

    let mut handles = Vec::new();
    for index in 0..5 {
        let endpoint = endpoint.clone();
        handles.push(tokio::spawn(async move {
            let request = project_resolve_request(format!("project-resolve-{index}"));
            send_socket_request_with_timeout(&endpoint, &request, Duration::from_secs(10)).await
        }));
    }

    for handle in handles {
        let response = handle.await.expect("join project resolve request");
        assert_eq!(
            response.get("status").and_then(serde_json::Value::as_str),
            Some("ok"),
            "project resolve should answer without waiting for sidecar lock: {response:?}"
        );
    }

    drop(lock_file);

    assert_eq!(
        git_output(&sidecar_root, &["rev-parse", "HEAD"]),
        head_before
    );
    let auto_persist_commits_after = git_output(
        &sidecar_root,
        &[
            "log",
            "--format=%s",
            "--grep",
            "Auto-persist Exosuit sidecar state",
        ],
    )
    .lines()
    .count();
    assert_eq!(
        auto_persist_commits_after, auto_persist_commits_before,
        "pure reads should not create auto-persist commits"
    );

    let db_path = sidecar_root.join("projects/rt/cache/exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    let event_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE event_type = 'command' AND namespace = 'project' AND operation = 'resolve'",
            [],
            |row| row.get(0),
        )
        .expect("count project resolve command events");
    assert_eq!(event_count, 5);
}

#[tokio::test]
async fn sidecar_run_task_does_not_block_concurrent_daemon_reads() {
    let _daemon_test_guard = DAEMON_INTEGRATION_TEST_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("w");
    let home = dir.path().join("h");
    let config_home = dir.path().join("c");
    let sidecar_root = dir.path().join("s");
    let marker = dir.path().join("task-started");
    create_test_workspace_at(&root);

    let init = exo_direct_with_env(
        &root,
        &home,
        &config_home,
        &[
            "--format",
            "json",
            "sidecar",
            "init",
            "--key",
            "rt",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            "--git",
        ],
    );
    assert!(
        init.status.success(),
        "sidecar init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git_config_identity(&sidecar_root);

    append_task(&root, "slow-build", &task_slow_build_command(&marker));

    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&root)
        .expect("resolve sidecar project");
    assert_eq!(project.policy, exo::project::StatePolicy::Sidecar);
    let endpoint = LocalRuntimePaths::new(&root, &project).endpoint();

    let exo_bin = assert_cmd::cargo::cargo_bin!("exo");
    let mut daemon = Command::new(&exo_bin);
    daemon
        .current_dir(&root)
        .args([
            "daemon",
            "run",
            "--workspace",
            root.to_str().expect("workspace root is utf-8"),
            "--timeout",
            "60",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_test_home_env(&mut daemon, &home, &config_home);
    let mut daemon = daemon.spawn().expect("spawn daemon");
    wait_for_daemon_endpoint(&mut daemon, &endpoint).await;
    let _guard = DaemonProcessGuard::new(daemon);

    let run_request =
        test_support::confirmed_machine_channel_request(run_task_request("slow-build"));
    let run_endpoint = endpoint.clone();
    let run_handle = tokio::spawn(async move {
        send_socket_request_with_timeout(&run_endpoint, &run_request, Duration::from_secs(45)).await
    });
    wait_for_file(&marker, Duration::from_secs(5)).await;

    let status_response =
        send_socket_request_with_timeout(&endpoint, &status_request(), Duration::from_secs(30))
            .await;
    assert!(
        !run_handle.is_finished(),
        "status should answer before run task completes"
    );
    assert_eq!(
        status_response
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("ok"),
        "status should answer while run task is still executing: {status_response:?}"
    );

    let run_response = run_handle.await.expect("join run task request");
    assert_eq!(
        run_response
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("ok"),
        "run task response should succeed: {run_response:?}"
    );
    assert_eq!(
        run_response
            .get("effect")
            .and_then(serde_json::Value::as_str),
        Some("exec")
    );
    assert_eq!(
        run_response
            .get("result")
            .and_then(|result| result.get("stdout"))
            .and_then(serde_json::Value::as_str),
        Some("done")
    );
}
