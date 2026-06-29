#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_macros)]
#![allow(clippy::disallowed_methods)] // integration test helpers use real fs APIs

//! Shared helper macros for integration tests.
//!
//! Each file in `tools/exo/tests/*.rs` is compiled as its own crate. These
//! macros reduce boilerplate for asserting `Result`/`Option` values in tests
//! without using `unwrap()`/`expect()`.

macro_rules! ok_or_return {
    ($expr:expr, $msg:literal) => {{
        let __res = $expr;
        assert!(__res.is_ok(), $msg);
        let Ok(__val) = __res else {
            return;
        };
        __val
    }};

    ($expr:expr, $msg:literal; $ret:expr) => {{
        let __res = $expr;
        assert!(__res.is_ok(), $msg);
        let Ok(__val) = __res else {
            return $ret;
        };
        __val
    }};
}

macro_rules! some_or_return {
    ($expr:expr, $msg:literal) => {{
        let __opt = $expr;
        assert!(__opt.is_some(), $msg);
        let Some(__val) = __opt else {
            return;
        };
        __val
    }};

    ($expr:expr, $msg:literal; $ret:expr) => {{
        let __opt = $expr;
        assert!(__opt.is_some(), $msg);
        let Some(__val) = __opt else {
            return $ret;
        };
        __val
    }};
}

pub mod fs {
    use std::io;
    use std::path::Path;

    pub use std::fs::{File, create_dir_all, metadata, set_permissions};

    pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
        use std::io::Write as _;

        let mut file = File::create(path)?;
        file.write_all(contents.as_ref())
    }

    pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
        use std::io::Read as _;

        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
        use std::io::Read as _;

        let mut file = File::open(path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }
}

/// Process-wide fake home directory.
///
/// Sidecar state defaults to `$HOME/exo/sidecars`, so any spawned `exo`
/// process that reaches a sidecar surface with the real `$HOME` leaks state
/// into the developer's actual sidecar repo (and dirties it, which fails
/// `dogfood verify`). Subprocess helpers route `HOME`/`XDG_CONFIG_HOME`
/// here for fixture roots outside the workspace repo.
///
/// In-process handler calls can't be isolated this way (mutating the process
/// env is unsafe and forbidden); tests that invoke sidecar operations
/// in-process must pass a project resolved against fixture policy paths.
static TEST_HOME: std::sync::LazyLock<tempfile::TempDir> = std::sync::LazyLock::new(|| {
    let home = tempfile::tempdir().expect("create isolated test home");
    std::fs::create_dir_all(home.path().join("config")).expect("create test config home");
    home
});

/// The isolated `$HOME` used by all test helpers in this process.
pub fn test_home() -> &'static std::path::Path {
    TEST_HOME.path()
}

static WORKSPACE_REPO_ROOT: std::sync::LazyLock<std::path::PathBuf> =
    std::sync::LazyLock::new(|| {
        // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let _ = p.pop();
        let _ = p.pop();
        p.canonicalize().unwrap_or(p)
    });

/// Whether a test root lives inside the real workspace repo.
///
/// Tests that target the workspace repo (e.g. parity suites running against
/// the actual exo2 checkout) must keep the real `$HOME` so they resolve the
/// developer's sidecar policy. Faking `$HOME` for those would silently flip
/// the state policy to repo-local `.exo/` and write SQL dumps into
/// `docs/agent-context/`.
fn root_is_workspace_repo(root: &std::path::Path) -> bool {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    canonical.starts_with(&*WORKSPACE_REPO_ROOT)
}

/// Apply fake-home isolation for fixture roots outside the workspace repo.
fn isolate_home(cmd: &mut assert_cmd::Command, root: &std::path::Path) {
    if root_is_workspace_repo(root) {
        return;
    }
    let home = test_home();
    cmd.env("HOME", home);
    cmd.env("XDG_CONFIG_HOME", home.join("config"));
}

/// Skip a test at runtime when running against the SQLite backend.
///
/// Use this for tests that exercise functionality not yet implemented in SQLite.
/// The skip is visible in test output as a printed message.
pub fn exo_cmd(root: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root);
    isolate_home(&mut cmd, root);
    // Use --direct to bypass daemon mode in tests
    cmd.arg("--direct");
    cmd
}

/// Compatibility wrapper during test cleanup.
///
/// The `backend` parameter is vestigial; SQLite is the only backend.
pub fn exo_cmd_with_storage(root: &std::path::Path, backend: &str) -> assert_cmd::Command {
    assert_eq!(
        backend, "sqlite",
        "sqlite is the only supported test backend"
    );
    exo_cmd(root)
}

pub fn exo_init(root: &std::path::Path) {
    exo_init_with_storage(root, "sqlite");
}

/// Legacy no-op during test cleanup.
pub fn write_exosuit_storage_config(root: &std::path::Path, backend: &str) {
    let _ = root;
    assert_eq!(
        backend, "sqlite",
        "sqlite is the only supported test backend"
    );
}

/// Initialize a project for the given storage backend.
///
/// The `backend` parameter is vestigial; SQLite is the only backend.
pub fn exo_init_with_storage(root: &std::path::Path, backend: &str) {
    assert_eq!(
        backend, "sqlite",
        "sqlite is the only supported test backend"
    );
    // Create minimal exosuit.toml (required for AgentContext::load)
    std::fs::write(
        root.join("exosuit.toml"),
        "[storage]\nbackend = \"sqlite\"\n",
    )
    .expect("write exosuit.toml");

    // Init creates SQLite database and scaffolds the workspace.
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root);
    isolate_home(&mut cmd, root);
    cmd.args(["--direct", "init", "--defaults"]);
    cmd.assert().success();

    // Write SQL dump files so the workspace is git-ready
    exo::context::write_sql_dump(root);
}

/// Add an epoch using the specified storage backend. Returns the epoch ID.
pub fn exo_plan_add_epoch_with_storage(
    root: &std::path::Path,
    backend: &str,
    title: &str,
) -> String {
    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json", "epoch", "add", "--title", title])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from add-epoch");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_epoch(root: &std::path::Path, title: &str) -> String {
    let output = exo_cmd(root)
        .args(["--format", "json", "epoch", "add", "--title", title])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // Parse JSON output to get the generated ID
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from add-epoch");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_epoch_after(root: &std::path::Path, title: &str, after: &str) -> String {
    let output = exo_cmd(root)
        .args([
            "--format", "json", "epoch", "add", "--title", title, "--after", after,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from add-epoch");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_phase(
    root: &std::path::Path,
    epoch_id: &str,
    title: &str,
    after: Option<&str>,
    rfcs: Option<&str>,
) -> String {
    let mut cmd = exo_cmd(root);
    cmd.args([
        "--format", "json", "phase", "add", "--title", title, "--epoch", epoch_id,
    ]);
    if let Some(after_id) = after {
        cmd.args(["--after", after_id]);
    }
    if let Some(rfcs) = rfcs {
        cmd.args(["--rfcs", rfcs]);
    }
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from phase add");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_phase_before(
    root: &std::path::Path,
    epoch_id: &str,
    title: &str,
    before: &str,
) -> String {
    let mut cmd = exo_cmd(root);
    cmd.args([
        "--format", "json", "phase", "add", "--title", title, "--epoch", epoch_id, "--before",
        before,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from phase add --before");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_phase_first(root: &std::path::Path, epoch_id: &str, title: &str) -> String {
    let mut cmd = exo_cmd(root);
    cmd.args([
        "--format", "json", "phase", "add", "--title", title, "--epoch", epoch_id, "--first",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from phase add --first");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

pub fn exo_plan_add_task(root: &std::path::Path, phase_id: &str, id: &str, label: &str) {
    exo_cmd(root)
        .args(["goal", "add", label, "--id", id, "--phase", phase_id])
        .assert()
        .success();
}

pub fn exo_plan_add_task_with_storage(
    root: &std::path::Path,
    backend: &str,
    phase_id: &str,
    id: &str,
    label: &str,
) {
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", label, "--id", id, "--phase", phase_id])
        .assert()
        .success();
}

pub fn exo_plan_update_status(root: &std::path::Path, id: &str, status: &str) {
    exo_cmd(root)
        .args(["plan", "update-status", id, status])
        .assert()
        .success();
}

pub fn exo_plan_update_status_with_storage(
    root: &std::path::Path,
    backend: &str,
    id: &str,
    status: &str,
) {
    exo_cmd_with_storage(root, backend)
        .args(["plan", "update-status", id, status])
        .assert()
        .success();
}

pub fn exo_phase_start(root: &std::path::Path) {
    exo_cmd(root).args(["phase", "start"]).assert().success();
}

pub fn exo_phase_start_with_storage(root: &std::path::Path, backend: &str) {
    exo_cmd_with_storage(root, backend)
        .args(["phase", "start"])
        .assert()
        .success();
}

pub fn exo_plan_add_phase_with_storage(
    root: &std::path::Path,
    backend: &str,
    epoch_id: &str,
    title: &str,
    after: Option<&str>,
    rfcs: Option<&str>,
) -> String {
    let mut cmd = exo_cmd_with_storage(root, backend);
    cmd.args([
        "--format", "json", "phase", "add", "--title", title, "--epoch", epoch_id,
    ]);
    if let Some(after_id) = after {
        cmd.args(["--after", after_id]);
    }
    if let Some(rfcs) = rfcs {
        cmd.args(["--rfcs", rfcs]);
    }
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from phase add");
    json.get("result")
        .and_then(|r| r.get("id"))
        .and_then(|v| v.as_str())
        .expect("expected id in result")
        .to_string()
}

/// Get the active phase ID from a workspace.
/// Useful after `exo_init` which now bootstraps a "Getting Started" epoch
/// with an active "Bootstrap" phase.
pub fn exo_active_phase_id(root: &std::path::Path) -> String {
    let output = exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid json from status");
    json.get("result")
        .and_then(|r| r.get("phase_id"))
        .and_then(|v| v.as_str())
        .expect("expected phase_id in status result")
        .to_string()
}

/// Get the active epoch ID from a workspace.
/// Useful after `exo_init` which now bootstraps a "Getting Started" epoch.
pub fn exo_active_epoch_id(root: &std::path::Path) -> String {
    let output = exo_cmd(root)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid json from epoch list");
    // Find the in-progress epoch
    let epochs = json
        .get("result")
        .and_then(|r| r.get("epochs"))
        .and_then(|e| e.as_array())
        .expect("expected epochs array");
    for epoch in epochs {
        if epoch.get("status").and_then(|s| s.as_str()) == Some("in-progress") {
            return epoch
                .get("id")
                .and_then(|v| v.as_str())
                .expect("expected epoch id")
                .to_string();
        }
    }
    panic!("no active epoch found");
}

pub fn write_implementation_plan(root: &std::path::Path, content: &str) {
    let impl_plan_dir = root.join("docs/agent-context/current");
    std::fs::create_dir_all(&impl_plan_dir).expect("create implementation-plan dir");
    std::fs::write(impl_plan_dir.join("implementation-plan.toml"), content)
        .expect("write implementation-plan.toml");
}

pub fn exo_rfc_create(
    root: &std::path::Path,
    title: &str,
    id: &str,
    stage: &str,
    feature: &str,
    body: Option<&str>,
) {
    let mut cmd = exo_cmd(root);
    cmd.args([
        "rfc",
        "create",
        title,
        "--id",
        id,
        "--stage",
        stage,
        "--feature",
        feature,
    ]);
    if let Some(body) = body {
        cmd.args(["--body", body]);
    }
    cmd.assert().success();
}

pub fn run_machine_channel_in_process(
    repo_root: &std::path::Path,
    request: &exo::api::protocol::RequestEnvelope,
) -> exo::api::protocol::ResponseEnvelope {
    run_machine_channel_in_process_with_project(repo_root, None, request)
}

pub fn run_machine_channel_in_process_with_project(
    repo_root: &std::path::Path,
    project: Option<&exo::project::Project>,
    request: &exo::api::protocol::RequestEnvelope,
) -> exo::api::protocol::ResponseEnvelope {
    // Simulate the process boundary: serialize/deserialize, then call the handler.
    // Note: this runs the handler in-process. Requests that can touch sidecar
    // surfaces should pass a project resolved against fixture policy paths.
    let input = serde_json::to_string(request).expect("serialize request");
    let parsed: exo::api::protocol::RequestEnvelope =
        serde_json::from_str(&input).expect("deserialize request");

    let mut response = match project {
        Some(project) => {
            exo::api::handler::handle_request_with_project(repo_root, Some(project), parsed)
        }
        None => exo::api::handler::handle_request(repo_root, parsed),
    };

    // Match machine-channel behavior: attach global verifier reminders.
    let reminders = exo::verifiers::run_global_verifiers(repo_root);
    if !reminders.is_empty() {
        response.reminders = Some(reminders);
    }

    // Ensure the response itself round-trips as JSON too.
    let out = serde_json::to_string(&response).expect("serialize response");
    let _roundtrip: exo::api::protocol::ResponseEnvelope =
        serde_json::from_str(&out).expect("deserialize response");

    response
}

pub fn confirmed_machine_channel_request(
    mut request: exo::api::protocol::RequestEnvelope,
) -> exo::api::protocol::RequestEnvelope {
    if let exo::api::protocol::Op::Call(params) = &request.op {
        let ticket = exo::command::transport::ticket_for_exec_call(&params.address, &params.input);
        request.auth = Some(exo::api::protocol::Auth {
            ticket,
            confirm: true,
        });
    }

    request
}
