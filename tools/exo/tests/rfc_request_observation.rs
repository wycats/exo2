#![cfg(unix)]
#![allow(clippy::disallowed_methods)]

mod test_support;

use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use std::{path::Path, path::PathBuf};
use tempfile::TempDir;

const COMMAND_BUDGET: Duration = Duration::from_secs(30);

struct DaemonGuard {
    child: Child,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run_git(workspace: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn workspace(temp: &TempDir) -> PathBuf {
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    run_git(&workspace, &["init", "--initial-branch=main"]);
    run_git(&workspace, &["config", "user.name", "Exo Test"]);
    run_git(
        &workspace,
        &["config", "user.email", "exo-test@example.invalid"],
    );
    test_support::exo_init_with_storage(&workspace, "sqlite");
    let rfc_dir = workspace.join("docs/rfcs/stage-1");
    std::fs::create_dir_all(&rfc_dir).unwrap();
    std::fs::write(
        rfc_dir.join("00001-request-observation.md"),
        "<!-- exo:1 ulid:01requestobservation -->\n\n# RFC 1: Request Observation\n",
    )
    .unwrap();
    run_git(&workspace, &["add", "-A"]);
    run_git(&workspace, &["commit", "-m", "initialize workspace"]);
    let head = run_git(&workspace, &["rev-parse", "HEAD"]);
    run_git(
        &workspace,
        &["update-ref", "refs/remotes/origin/main", &head],
    );
    run_git(
        &workspace,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    workspace
}

fn run_exo(
    workspace: &Path,
    direct: bool,
    args: &[&str],
    trace_path: Option<&Path>,
) -> (Output, Duration) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exo"));
    command.args(["--format", "json"]);
    if direct {
        command.arg("--direct");
    }
    command
        .args(args)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(trace_path) = trace_path {
        command.env("GIT_TRACE2_EVENT", trace_path);
    }
    let start = Instant::now();
    let output = command.output().unwrap();
    (output, start.elapsed())
}

fn spawn_daemon(workspace: &Path, trace_path: &Path) -> DaemonGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_exo"))
        .args(["daemon", "run", "--workspace"])
        .arg(workspace)
        .current_dir(workspace)
        .env("EXO_NO_REEXEC", "1")
        .env("HOME", workspace.join(".test-home"))
        .env("XDG_CONFIG_HOME", workspace.join(".test-home/config"))
        .env("GIT_TRACE2_EVENT", trace_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let paths = exo::daemon::paths_for_workspace(workspace).unwrap();
    let start = Instant::now();
    while !paths.endpoint().is_connectable_blocking() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "foreground test daemon did not become connectable"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    DaemonGuard { child }
}

fn reset_trace(trace_path: &Path) {
    let _ = std::fs::remove_file(trace_path);
}

fn git_process_count(trace_path: &Path) -> usize {
    std::fs::read_to_string(trace_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event.get("event").and_then(serde_json::Value::as_str) == Some("start"))
        .count()
}

fn canonical_observation_count(trace_path: &Path) -> usize {
    std::fs::read_to_string(trace_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event.get("event").and_then(serde_json::Value::as_str) == Some("start"))
        .filter(|event| {
            event
                .get("argv")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|argv| {
                    argv.iter()
                        .any(|arg| arg.as_str() == Some("refs/remotes/origin/HEAD^{commit}"))
                })
        })
        .count()
}

fn assert_request_bound(output: &Output, elapsed: Duration, observations: usize) {
    assert!(
        output.status.success(),
        "command failed: stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(observations, 1, "request repeated canonical observation");
    assert!(
        elapsed < COMMAND_BUDGET,
        "request exceeded wall-clock guard {COMMAND_BUDGET:?}: {elapsed:?}"
    );
}

#[test]
fn status_and_task_list_reuse_one_rfc_observation_per_request() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace(&temp);
    let mut direct_results = Vec::new();

    for (args, max_git_processes) in [(&["status"][..], 7), (&["task", "list"][..], 5)] {
        let trace_path = temp.path().join(format!("direct-{}.trace", args.join("-")));
        reset_trace(&trace_path);
        let (output, elapsed) = run_exo(&workspace, true, args, Some(&trace_path));
        assert_request_bound(&output, elapsed, canonical_observation_count(&trace_path));
        let result = serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
        direct_results.push((args.join(" "), result["result"].clone()));
        let git_processes = git_process_count(&trace_path);
        assert!(
            git_processes <= max_git_processes,
            "direct {} launched {git_processes} Git processes; budget is {max_git_processes}",
            args.join(" ")
        );
    }

    let daemon_trace = temp.path().join("daemon.trace");
    let _guard = spawn_daemon(&workspace, &daemon_trace);
    for args in [&["status"][..], &["task", "list"][..]] {
        reset_trace(&daemon_trace);
        let (output, elapsed) = run_exo(&workspace, false, args, None);
        assert_request_bound(&output, elapsed, canonical_observation_count(&daemon_trace));
        let result = serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
        let direct_result = direct_results
            .iter()
            .find(|(command, _)| command == &args.join(" "))
            .map(|(_, result)| result)
            .unwrap();
        assert_eq!(
            &result["result"], direct_result,
            "direct and daemon result payloads diverged"
        );
    }
}
