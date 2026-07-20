//! Daemon mode for exo - socket-based server with idle timeout.
//!
//! This module implements the unified server architecture (RFC 0097) where
//! both CLI and VS Code connect to a single daemon per project.
//!
//! # Architecture
//!
//! - One daemon per project, using project runtime paths
//! - Socket/PID files in project state: `{state_root}/runtime/daemon.sock`
//! - Idle timeout: daemon exits after N seconds with no clients
//! - Connect-or-spawn: clients auto-start daemon if not running
//!
//! # Project Runtime Pattern (RFC 10184)
//!
//! Runtime artifacts live under the resolved project state root, while request
//! handling still receives the workspace root that launched the daemon.
//! Benefits:
//! - Worktrees: linked worktrees share one daemon
//! - Nested projects: project identity determines the daemon boundary
//! - Shadow projects: runtime paths move with shadow state
//!
//! # Connect-or-Spawn Protocol
//!
//! Clients use [`ensure_daemon`] to get a connection:
//! 1. Try to connect to existing socket
//! 2. If socket exists but connection fails, check PID file
//! 3. If PID is stale (process dead), clean up and spawn new daemon
//! 4. Wait for socket to become available
//! 5. Connect and return stream

use crate::api::handler::{
    finalize_atomic_response_after_commit,
    handle_request_with_project_and_diagnostics_as_atomic_writer,
    handle_request_with_project_and_diagnostics_as_writer,
};
use crate::api::protocol::{
    Effect, ErrorBody, ErrorCode, PROTOCOL_VERSION, RecoveryClass, RequestEnvelope,
    ResponseEnvelope, Status,
};
use crate::context::AgentContext;
use crate::daemon_diagnostics::{
    DaemonDiagnostics, DaemonDiagnosticsConfig, effect_name, elapsed_ms, request_op_path,
    response_status,
};
use crate::daemon_outcomes::{
    DAEMON_OUTCOME_DB_NAME, OutcomeExecution, RequestOutcomeLedger, request_command_path,
    request_declared_recovery, resolved_request_recovery,
};
use crate::daemon_transport::{DaemonEndpoint, DaemonStream};
use crate::project::{Project, ProjectResolver, git_common_dir_from_filesystem};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default idle timeout in seconds (5 minutes).
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;
const DEFAULT_DAEMON_MAX_CONNECTIONS: usize = 128;
const DEFAULT_DAEMON_MAX_IN_FLIGHT_REQUESTS: usize = 32;
const DAEMON_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const DAEMON_PROBE_KIND: &str = "daemon_probe";
const DAEMON_PROBE_OK_KIND: &str = "daemon_probe_ok";

#[cfg(windows)]
fn daemon_startup_timeout() -> Duration {
    Duration::from_secs(20)
}

#[cfg(not(windows))]
fn daemon_startup_timeout() -> Duration {
    Duration::from_secs(15)
}

/// Error returned when a daemon caller tries to use a filesystem root as the workspace root.
pub const FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR: &str = "filesystem root is not a valid Exosuit workspace root; run from a git worktree or use project resolve to diagnose";

/// Get current time as seconds since UNIX epoch.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Project-local runtime paths.
///
/// Runtime artifacts are derived from the resolved [`Project`]:
/// - `daemon.sock` - Unix domain socket
/// - `daemon.pid` - PID file
#[derive(Debug, Clone)]
pub struct LocalRuntimePaths {
    workspace_root: PathBuf,
    project_id: String,
    state_root: PathBuf,
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    pid_path: PathBuf,
    lock_path: PathBuf,
    identity_path: PathBuf,
    config_home: Option<PathBuf>,
}

impl LocalRuntimePaths {
    pub fn new(workspace_root: impl Into<PathBuf>, project: &Project) -> Self {
        let runtime_dir = project.runtime_dir();
        let socket_path = project.socket_path();
        let pid_path = project.pid_path();
        let lock_path = runtime_dir.join("daemon.lock");
        let identity_path = runtime_dir.join("daemon.identity.json");
        let config_home = project
            .projects_config_path
            .as_ref()
            .and_then(|path| path.parent())
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Self {
            workspace_root: workspace_root.into(),
            project_id: project.id.to_string(),
            state_root: project.state_root.clone(),
            runtime_dir,
            socket_path,
            pid_path,
            lock_path,
            identity_path,
            config_home,
        }
    }

    /// The project runtime directory.
    pub fn runtime_dir(&self) -> PathBuf {
        self.runtime_dir.clone()
    }

    /// Unix socket path: `{state_root}/runtime/daemon.sock`
    pub fn socket_path(&self) -> PathBuf {
        self.socket_path.clone()
    }

    pub fn endpoint(&self) -> DaemonEndpoint {
        #[cfg(windows)]
        {
            DaemonEndpoint::from_runtime_dir(&self.runtime_dir)
        }
        #[cfg(not(windows))]
        {
            DaemonEndpoint::from_socket_path(&self.socket_path)
        }
    }

    /// PID file path: `{state_root}/runtime/daemon.pid`
    pub fn pid_path(&self) -> PathBuf {
        self.pid_path.clone()
    }

    /// Lock file path: `{state_root}/runtime/daemon.lock`
    pub fn lock_path(&self) -> PathBuf {
        self.lock_path.clone()
    }

    /// Daemon executable identity path: `{state_root}/runtime/daemon.identity.json`
    pub fn identity_path(&self) -> PathBuf {
        self.identity_path.clone()
    }

    /// Durable request/outcome ledger used to recover interrupted mutations.
    pub fn outcome_ledger_path(&self) -> PathBuf {
        self.runtime_dir.join(DAEMON_OUTCOME_DB_NAME)
    }

    /// The workspace root this runtime is for.
    pub fn workspace(&self) -> &Path {
        &self.workspace_root
    }

    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn state_root(&self) -> &Path {
        &self.state_root
    }

    fn config_home(&self) -> Option<&Path> {
        self.config_home.as_deref()
    }

    /// Ensure the runtime directory exists.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.runtime_dir())?;
        #[cfg(unix)]
        {
            if let Some(parent) = self.socket_path.parent()
                && parent != self.runtime_dir
            {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonEnsureState {
    ConnectedExisting,
    Spawned,
    WaitedForLock,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonEnsureReport {
    pub kind: &'static str,
    pub ok: bool,
    pub workspace_root: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub endpoint: String,
    pub pid_path: PathBuf,
    pub pid: Option<u32>,
    pub instance_id: Option<String>,
    pub probe_ok: bool,
    pub state: DaemonEnsureState,
    pub connected: bool,
    pub spawned: bool,
    pub reused: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatusState {
    RunningCurrent,
    Stopped,
    StaleIdentity,
    Unreachable,
    InvalidWorkspace,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonStatusReport {
    pub kind: &'static str,
    pub ok: bool,
    pub workspace_root: PathBuf,
    pub runtime_dir: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub pid_path: Option<PathBuf>,
    pub identity_path: Option<PathBuf>,
    pub pid: Option<u32>,
    pub instance_id: Option<String>,
    pub pid_alive: Option<bool>,
    pub socket_exists: Option<bool>,
    pub socket_connectable: Option<bool>,
    pub probe_ok: Option<bool>,
    pub identity_exists: Option<bool>,
    pub identity_readable: Option<bool>,
    pub identity_matches_workspace: Option<bool>,
    pub identity_matches_project: Option<bool>,
    pub identity_matches_executable: Option<bool>,
    pub recorded_identity: Option<serde_json::Value>,
    pub current_identity: Option<serde_json::Value>,
    pub state: DaemonStatusState,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeDaemonIdentity {
    workspace_root: PathBuf,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    state_root: Option<PathBuf>,
    executable: RuntimeExecutableIdentity,
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    process_start_id: Option<String>,
}

impl RuntimeDaemonIdentity {
    fn current(paths: &LocalRuntimePaths) -> io::Result<Self> {
        Ok(Self {
            workspace_root: paths.workspace().to_path_buf(),
            project_id: Some(paths.project_id().to_string()),
            state_root: Some(paths.state_root().to_path_buf()),
            executable: RuntimeExecutableIdentity::current()?,
            instance_id: None,
            pid: None,
            process_start_id: None,
        })
    }

    fn for_daemon(paths: &LocalRuntimePaths) -> io::Result<Self> {
        Ok(Self {
            workspace_root: paths.workspace().to_path_buf(),
            project_id: Some(paths.project_id().to_string()),
            state_root: Some(paths.state_root().to_path_buf()),
            executable: RuntimeExecutableIdentity::current()?,
            instance_id: Some(ulid::Ulid::new().to_string().to_lowercase()),
            pid: Some(std::process::id()),
            process_start_id: Some(process_start_identity(std::process::id())?),
        })
    }

    fn matches_project_authority(&self, current: &Self) -> bool {
        match (
            self.project_id.as_deref(),
            self.state_root.as_deref(),
            current.project_id.as_deref(),
            current.state_root.as_deref(),
        ) {
            (Some(recorded_id), Some(recorded_root), Some(current_id), Some(current_root)) => {
                recorded_id == current_id && recorded_root == current_root
            }
            (None, None, Some(_), Some(_)) => self.workspace_root == current.workspace_root,
            _ => false,
        }
    }

    fn matches_runtime(&self, current: &Self) -> bool {
        self.matches_project_authority(current) && self.executable == current.executable
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeExecutableIdentity {
    path: PathBuf,
    len: u64,
    modified_unix_ms: Option<u128>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl RuntimeExecutableIdentity {
    fn current() -> io::Result<Self> {
        let path = exo_executable()?;
        let metadata = std::fs::metadata(&path)?;
        Ok(Self::from_path_and_metadata(path, &metadata))
    }

    fn from_path_and_metadata(path: PathBuf, metadata: &Metadata) -> Self {
        Self {
            path,
            len: metadata.len(),
            modified_unix_ms: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis()),
            #[cfg(unix)]
            dev: {
                use std::os::unix::fs::MetadataExt;
                metadata.dev()
            },
            #[cfg(unix)]
            ino: {
                use std::os::unix::fs::MetadataExt;
                metadata.ino()
            },
        }
    }
}

impl DaemonEnsureReport {
    fn new(paths: &LocalRuntimePaths, state: DaemonEnsureState) -> Self {
        let spawned = state == DaemonEnsureState::Spawned;
        Self {
            kind: "daemon.ensure",
            ok: true,
            workspace_root: paths.workspace().to_path_buf(),
            runtime_dir: paths.runtime_dir(),
            socket_path: paths.socket_path(),
            endpoint: paths.endpoint().display(),
            pid_path: paths.pid_path(),
            pid: read_pid_file(&paths.pid_path()),
            instance_id: read_daemon_identity(paths)
                .ok()
                .and_then(|identity| identity.instance_id),
            probe_ok: true,
            state,
            connected: true,
            spawned,
            reused: !spawned,
            diagnostics: Vec::new(),
        }
    }

    fn diagnostic(&mut self, message: impl Into<String>) {
        self.diagnostics.push(message.into());
    }
}

#[derive(Debug)]
pub struct DaemonEnsureOutcome {
    stream: DaemonStream,
    report: DaemonEnsureReport,
}

impl DaemonEnsureOutcome {
    pub fn into_parts(self) -> (DaemonStream, DaemonEnsureReport) {
        (self.stream, self.report)
    }

    pub fn into_stream(self) -> DaemonStream {
        self.stream
    }

    pub fn into_report(self) -> DaemonEnsureReport {
        self.report
    }
}

fn is_filesystem_root(path: &Path) -> bool {
    path.has_root() && path.parent().is_none()
}

fn invalid_filesystem_root() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR,
    )
}

fn daemon_project_and_workspace(workspace_path: &Path) -> io::Result<(Project, PathBuf)> {
    if is_filesystem_root(workspace_path) {
        return Err(invalid_filesystem_root());
    }

    let project = Project::resolve(workspace_path).map_err(to_io_error)?;
    let workspace_root = project.workspace_root.as_ref().ok_or_else(|| {
        io::Error::other(
            "daemon requires a git worktree workspace root; bare repositories are not supported",
        )
    })?;
    let workspace_root = workspace_root.canonicalize().map_err(|error| {
        io::Error::other(format!(
            "failed to canonicalize daemon workspace root {}: {error}",
            workspace_root.display()
        ))
    })?;

    if is_filesystem_root(&workspace_root) {
        return Err(invalid_filesystem_root());
    }

    Ok((project, workspace_root))
}

/// Resolve a daemon caller path to the canonical git workspace root.
pub fn resolve_daemon_workspace(workspace_path: &Path) -> io::Result<PathBuf> {
    daemon_project_and_workspace(workspace_path).map(|(_, workspace_root)| workspace_root)
}

/// Get the `LocalRuntimePaths` for a workspace.
pub fn paths_for_workspace(workspace_path: &Path) -> io::Result<LocalRuntimePaths> {
    let (project, workspace_root) = daemon_project_and_workspace(workspace_path)?;
    Ok(LocalRuntimePaths::new(workspace_root, &project))
}

/// Get runtime paths for an explicitly resolved project.
///
/// This preserves fixture or caller-supplied project policy when a machine-channel
/// request is routed through the daemon writer lane.
pub fn paths_for_workspace_project(
    workspace_path: &Path,
    project: &Project,
) -> io::Result<LocalRuntimePaths> {
    let workspace_root = project
        .workspace_root
        .clone()
        .unwrap_or_else(|| workspace_path.to_path_buf());
    if is_filesystem_root(&workspace_root) {
        return Err(invalid_filesystem_root());
    }
    Ok(LocalRuntimePaths::new(workspace_root, project))
}

fn to_io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn read_pid_file(pid_path: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|pid| pid.trim().parse().ok())
}

fn write_daemon_identity(paths: &LocalRuntimePaths) -> io::Result<RuntimeDaemonIdentity> {
    let identity = RuntimeDaemonIdentity::for_daemon(paths)?;
    let path = paths.identity_path();
    let content = serde_json::to_vec_pretty(&identity).map_err(io::Error::other)?;
    std::fs::write(path, content)?;
    Ok(identity)
}

fn read_daemon_identity(paths: &LocalRuntimePaths) -> io::Result<RuntimeDaemonIdentity> {
    let content = std::fs::read(paths.identity_path())?;
    serde_json::from_slice(&content).map_err(io::Error::other)
}

fn daemon_identity_matches_current(paths: &LocalRuntimePaths) -> bool {
    let Ok(recorded) = read_daemon_identity(paths) else {
        return false;
    };
    let Ok(current) = RuntimeDaemonIdentity::current(paths) else {
        return false;
    };
    recorded.matches_runtime(&current)
        && recorded.pid.is_some()
        && recorded.pid == read_pid_file(&paths.pid_path())
        && recorded.instance_id.is_some()
        && recorded
            .process_start_id
            .as_deref()
            .is_some_and(|start_id| {
                recorded
                    .pid
                    .and_then(|pid| process_start_identity(pid).ok())
                    .as_deref()
                    == Some(start_id)
            })
}

pub fn daemon_status(workspace_path: &Path) -> DaemonStatusReport {
    match paths_for_workspace(workspace_path) {
        Ok(paths) => daemon_status_for_paths(paths),
        Err(error) => DaemonStatusReport {
            kind: "daemon.status",
            ok: false,
            workspace_root: workspace_path.to_path_buf(),
            runtime_dir: None,
            socket_path: None,
            endpoint: None,
            pid_path: None,
            identity_path: None,
            pid: None,
            instance_id: None,
            pid_alive: None,
            socket_exists: None,
            socket_connectable: None,
            probe_ok: None,
            identity_exists: None,
            identity_readable: None,
            identity_matches_workspace: None,
            identity_matches_project: None,
            identity_matches_executable: None,
            recorded_identity: None,
            current_identity: None,
            state: DaemonStatusState::InvalidWorkspace,
            issue: Some(error.to_string()),
        },
    }
}

pub fn daemon_status_for_project(workspace_path: &Path, project: &Project) -> DaemonStatusReport {
    match paths_for_workspace_project(workspace_path, project) {
        Ok(paths) => daemon_status_for_paths(paths),
        Err(error) => DaemonStatusReport {
            kind: "daemon.status",
            ok: false,
            workspace_root: workspace_path.to_path_buf(),
            runtime_dir: None,
            socket_path: None,
            endpoint: None,
            pid_path: None,
            identity_path: None,
            pid: None,
            instance_id: None,
            pid_alive: None,
            socket_exists: None,
            socket_connectable: None,
            probe_ok: None,
            identity_exists: None,
            identity_readable: None,
            identity_matches_workspace: None,
            identity_matches_project: None,
            identity_matches_executable: None,
            recorded_identity: None,
            current_identity: None,
            state: DaemonStatusState::InvalidWorkspace,
            issue: Some(error.to_string()),
        },
    }
}

fn daemon_status_for_paths(paths: LocalRuntimePaths) -> DaemonStatusReport {
    let pid_path = paths.pid_path();
    let socket_path = paths.socket_path();
    let endpoint = paths.endpoint();
    let identity_path = paths.identity_path();
    let pid = read_pid_file(&pid_path);
    let pid_alive = pid.map(process_alive);
    let pid_path_exists = pid_path.exists();
    let socket_exists = socket_path.exists();
    let identity_exists = identity_path.exists();
    let recorded_identity_result = read_daemon_identity(&paths);
    let identity_readable = recorded_identity_result.is_ok();
    let current_identity_result = RuntimeDaemonIdentity::current(&paths);
    let (socket_connectable, probe_ok) = inspect_daemon_endpoint(
        &paths,
        recorded_identity_result
            .as_ref()
            .ok()
            .and_then(|identity| identity.instance_id.as_deref()),
    );

    let identity_matches_workspace = recorded_identity_result
        .as_ref()
        .ok()
        .map(|identity| identity.workspace_root == paths.workspace());
    let identity_matches_project = recorded_identity_result
        .as_ref()
        .ok()
        .zip(current_identity_result.as_ref().ok())
        .map(|(recorded, current)| recorded.matches_project_authority(current));
    let identity_matches_executable = recorded_identity_result
        .as_ref()
        .ok()
        .zip(current_identity_result.as_ref().ok())
        .map(|(recorded, current)| recorded.executable == current.executable);

    let recorded_identity = recorded_identity_result
        .as_ref()
        .ok()
        .and_then(|identity| serde_json::to_value(identity).ok());
    let instance_id = recorded_identity_result
        .as_ref()
        .ok()
        .and_then(|identity| identity.instance_id.clone());
    let current_identity = current_identity_result
        .as_ref()
        .ok()
        .and_then(|identity| serde_json::to_value(identity).ok());

    let state = if socket_connectable {
        if identity_matches_project == Some(true)
            && identity_matches_executable == Some(true)
            && probe_ok == Some(true)
        {
            DaemonStatusState::RunningCurrent
        } else if identity_matches_project == Some(true)
            && identity_matches_executable == Some(true)
        {
            DaemonStatusState::Unreachable
        } else {
            DaemonStatusState::StaleIdentity
        }
    } else if socket_exists || pid_path_exists || pid_alive == Some(true) {
        DaemonStatusState::Unreachable
    } else {
        DaemonStatusState::Stopped
    };

    let issue = match state {
        DaemonStatusState::RunningCurrent => None,
        DaemonStatusState::Stopped => Some("daemon is stopped".to_string()),
        DaemonStatusState::StaleIdentity => Some(
            "daemon identity is missing or does not match the current executable/project runtime"
                .to_string(),
        ),
        DaemonStatusState::Unreachable => Some(if socket_connectable {
            "daemon socket accepts connections but the bounded instance probe failed".to_string()
        } else {
            "daemon runtime files exist but the socket is not accepting connections".to_string()
        }),
        DaemonStatusState::InvalidWorkspace => None,
    };

    DaemonStatusReport {
        kind: "daemon.status",
        ok: state == DaemonStatusState::RunningCurrent,
        workspace_root: paths.workspace().to_path_buf(),
        runtime_dir: Some(paths.runtime_dir()),
        socket_path: Some(socket_path),
        endpoint: Some(endpoint.display()),
        pid_path: Some(pid_path),
        identity_path: Some(identity_path),
        pid,
        instance_id,
        pid_alive,
        socket_exists: Some(socket_exists),
        socket_connectable: Some(socket_connectable),
        probe_ok,
        identity_exists: Some(identity_exists),
        identity_readable: Some(identity_readable),
        identity_matches_workspace,
        identity_matches_project,
        identity_matches_executable,
        recorded_identity,
        current_identity,
        state,
        issue,
    }
}

fn inspect_daemon_endpoint(
    paths: &LocalRuntimePaths,
    expected_instance_id: Option<&str>,
) -> (bool, Option<bool>) {
    let paths = paths.clone();
    let expected_instance_id = expected_instance_id.map(ToOwned::to_owned);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return (false, None);
        };
        runtime.block_on(async {
            let Ok(mut stream) = connect_to_daemon_paths(&paths).await else {
                return (false, None);
            };
            let probe = probe_daemon_stream_with_timeout(
                &mut stream,
                expected_instance_id.as_deref(),
                DAEMON_PROBE_TIMEOUT,
            )
            .await;
            (true, Some(probe.is_ok()))
        })
    })
    .join()
    .unwrap_or((false, None))
}

#[cfg(target_os = "linux")]
fn process_start_identity(pid: u32) -> io::Result<String> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PID 0 is invalid",
        ));
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close_paren = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed process stat"))?;
    let start_time_ticks = stat[close_paren + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process start time"))?;
    Ok(format!("linux-starttime:{start_time_ticks}"))
}

#[cfg(target_os = "macos")]
fn process_start_identity(pid: u32) -> io::Result<String> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PID 0 is invalid",
        ));
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || start.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process start time is unavailable",
        ));
    }
    Ok(format!("macos-lstart:{start}"))
}

#[cfg(windows)]
fn process_start_identity(pid: u32) -> io::Result<String> {
    let script =
        format!("(Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToUniversalTime().Ticks");
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process start time is unavailable",
        ));
    }
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if start.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process start time is unavailable",
        ));
    }
    Ok(format!("windows-starttime:{start}"))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_start_identity(pid: u32) -> io::Result<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || start.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process start time is unavailable",
        ));
    }
    Ok(format!("unix-lstart:{start}"))
}

fn daemon_process_identity_matches(
    paths: &LocalRuntimePaths,
    pid: u32,
    probed_process_start_id: Option<&str>,
    legacy_endpoint_connected: bool,
) -> bool {
    if probed_process_start_id.is_some_and(|recorded| {
        process_start_identity(pid).is_ok_and(|current| current == recorded)
    }) {
        return true;
    }
    let Ok(identity) = read_daemon_identity(paths) else {
        return false;
    };
    let start_matches = identity.pid == Some(pid)
        && identity
            .process_start_id
            .as_deref()
            .is_some_and(|recorded| {
                process_start_identity(pid).is_ok_and(|current| current == recorded)
            });
    if start_matches {
        return true;
    }

    legacy_endpoint_connected
        && paths_for_workspace(&identity.workspace_root)
            .is_ok_and(|recorded_paths| recorded_paths.runtime_dir() == paths.runtime_dir())
        && legacy_daemon_command_matches(&identity.workspace_root, pid)
}

#[cfg(target_os = "linux")]
fn legacy_daemon_command_matches(workspace_root: &Path, pid: u32) -> bool {
    let Ok(command_line) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args = command_line
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg))
        .collect::<Vec<_>>();
    daemon_command_args_match(workspace_root, args.iter().map(|arg| arg.as_ref()))
}

#[cfg(target_os = "macos")]
fn legacy_daemon_command_matches(workspace_root: &Path, pid: u32) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    output.status.success()
        && daemon_command_text_matches(
            workspace_root,
            String::from_utf8_lossy(&output.stdout).trim(),
        )
}

#[cfg(windows)]
fn legacy_daemon_command_matches(workspace_root: &Path, pid: u32) -> bool {
    let script = format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine");
    let Ok(output) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    else {
        return false;
    };
    output.status.success()
        && daemon_command_text_matches(
            workspace_root,
            String::from_utf8_lossy(&output.stdout).trim(),
        )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn legacy_daemon_command_matches(_workspace_root: &Path, _pid: u32) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn daemon_command_args_match<'a>(
    workspace_root: &Path,
    args: impl Iterator<Item = &'a str>,
) -> bool {
    let args = args.collect::<Vec<_>>();
    let has_daemon_run = args.windows(2).any(|pair| pair == ["daemon", "run"]);
    let workspace = workspace_root.to_string_lossy();
    let has_workspace = args
        .windows(2)
        .any(|pair| pair[0] == "--workspace" && pair[1] == workspace);
    has_daemon_run && has_workspace
}

#[cfg(any(target_os = "macos", windows))]
fn daemon_command_text_matches(workspace_root: &Path, command: &str) -> bool {
    let workspace = workspace_root.to_string_lossy();
    command.contains("daemon run")
        && command.contains("--workspace")
        && command.contains(workspace.as_ref())
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
        Ok(()) | Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn process_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
        })
}

#[cfg(unix)]
fn terminate_pid(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    matches!(
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        ),
        Ok(()) | Err(nix::errno::Errno::ESRCH)
    )
}

#[cfg(not(unix))]
fn terminate_pid(pid: u32) -> bool {
    let pid_text = pid.to_string();
    Command::new("taskkill")
        .args(["/PID", &pid_text, "/T"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn force_terminate_pid(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    matches!(
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        ),
        Ok(()) | Err(nix::errno::Errno::ESRCH)
    )
}

#[cfg(not(unix))]
fn force_terminate_pid(pid: u32) -> bool {
    let pid_text = pid.to_string();
    Command::new("taskkill")
        .args(["/PID", &pid_text, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

async fn restart_stale_daemon_once(
    paths: &LocalRuntimePaths,
    probed_process: Option<&DaemonProbeResponse>,
    legacy_endpoint_connected: bool,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let process = probed_process
        .map(|process| (process.pid, Some(process.process_start_id.as_str())))
        .or_else(|| read_pid_file(&paths.pid_path()).map(|pid| (pid, None)));
    if let Some((pid, probed_process_start_id)) = process {
        if process_alive(pid) {
            if !daemon_process_identity_matches(
                paths,
                pid,
                probed_process_start_id,
                legacy_endpoint_connected,
            ) {
                diagnostics.push(format!(
                    "refused to signal unverified stale daemon PID {pid}"
                ));
                return diagnostics;
            }
            if terminate_pid(pid) {
                diagnostics.push(format!("terminated stale daemon process {pid}"));
            } else {
                diagnostics.push(format!("failed to terminate stale daemon process {pid}"));
            }

            let start = std::time::Instant::now();
            while process_alive(pid) && start.elapsed() < Duration::from_secs(2) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if process_alive(pid) {
                if !daemon_process_identity_matches(
                    paths,
                    pid,
                    probed_process_start_id,
                    legacy_endpoint_connected,
                ) {
                    diagnostics.push(format!(
                        "refused to force-terminate changed stale daemon PID {pid}"
                    ));
                    return diagnostics;
                }
                if force_terminate_pid(pid) {
                    diagnostics.push(format!("force-terminated stale daemon process {pid}"));
                } else {
                    diagnostics.push(format!(
                        "failed to force-terminate stale daemon process {pid}"
                    ));
                }

                let start = std::time::Instant::now();
                while process_alive(pid) && start.elapsed() < Duration::from_secs(2) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    // Do not unlink the socket or identity here: another ensure caller may
    // have already spawned a replacement daemon by the time the old PID exits.
    // Stale socket cleanup is safe only after this process owns the PID lock.

    diagnostics
}

fn daemon_request_project(startup_project: &Project) -> io::Result<Project> {
    let current = startup_project.refresh_policy().map_err(to_io_error)?;
    if current.runtime_dir() != startup_project.runtime_dir() {
        return Err(io::Error::other(
            "project state policy changed the daemon runtime; reconnect through the current project runtime",
        ));
    }
    Ok(current)
}

#[derive(Debug)]
struct DaemonRequestContext {
    workspace_root: PathBuf,
    project: Project,
}

fn daemon_request_context(
    startup_workspace: &Path,
    startup_project: &Project,
    request: &RequestEnvelope,
) -> io::Result<DaemonRequestContext> {
    let workspace_root = validated_request_workspace(startup_workspace, startup_project, request)?;
    let mut request_project = daemon_request_project(startup_project)?;
    request_project.workspace_root = Some(workspace_root.clone());
    Ok(DaemonRequestContext {
        workspace_root,
        project: request_project,
    })
}

fn validated_request_workspace(
    startup_workspace: &Path,
    startup_project: &Project,
    request: &RequestEnvelope,
) -> io::Result<PathBuf> {
    let Some(requested_workspace) = request.workspace_root.as_deref() else {
        return Ok(startup_workspace.to_path_buf());
    };

    if !requested_workspace.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon request workspace must be an absolute path",
        ));
    }

    let workspace_root = requested_workspace.canonicalize().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon request workspace path could not be canonicalized",
        )
    })?;
    if workspace_root == startup_workspace
        && git_common_dir_from_filesystem(&workspace_root).as_deref()
            == Some(startup_project.git_common_dir.as_path())
    {
        return Ok(startup_workspace.to_path_buf());
    }

    let resolver = startup_project
        .projects_config_path
        .as_deref()
        .map_or_else(ProjectResolver::default, |path| {
            ProjectResolver::default().with_projects_config_path(path)
        });
    let resolved = resolver.resolve(&workspace_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "request workspace does not belong to this daemon's project and state root",
        )
    })?;
    if resolved.id != startup_project.id || resolved.state_root != startup_project.state_root {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "request workspace does not belong to this daemon's project and state root",
        ));
    }

    resolved.workspace_root.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "request workspace does not belong to this daemon's project and state root",
        )
    })
}

fn daemon_workspace_error_response(id: String, error: &io::Error) -> ResponseEnvelope {
    let code = if error.kind() == io::ErrorKind::InvalidInput {
        ErrorCode::InvalidInput
    } else {
        ErrorCode::PreconditionFailed
    };
    daemon_handler_error_response(id, code, error.to_string())
}

fn daemon_busy_response(id: String) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code: ErrorCode::PreconditionFailed,
            message: "daemon request capacity is exhausted; retry later with the same request ID"
                .to_string(),
            details: Some(serde_json::json!({
                "kind": "daemon.busy",
                "retryable": true,
                "retry_with_same_request_id": true,
                "request_outcome_checked": false,
            })),
        }),
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

async fn dispatch_bounded_daemon_request<F>(
    request_id: String,
    admission: Arc<tokio::sync::Semaphore>,
    diagnostics: DaemonDiagnostics,
    task: F,
) -> ResponseEnvelope
where
    F: FnOnce() -> ResponseEnvelope + Send + 'static,
{
    let permit = match admission.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            diagnostics.record(
                "request.busy",
                serde_json::json!({ "request_id": request_id }),
            );
            return daemon_busy_response(request_id);
        }
    };

    match tokio::task::spawn_blocking(move || {
        // Keep the permit on the blocking worker so cancellation of the async
        // connection task cannot admit a replacement while this work continues.
        let _permit = permit;
        task()
    })
    .await
    {
        Ok(response) => response,
        Err(error) => daemon_handler_error_response(
            request_id,
            ErrorCode::Internal,
            format!("daemon handler task failed: {error}"),
        ),
    }
}

fn replay_request_context(
    startup_workspace: &Path,
    startup_project: &Project,
    request: &RequestEnvelope,
) -> io::Result<DaemonRequestContext> {
    let workspace_root = match request.workspace_root.as_deref() {
        None => startup_workspace.to_path_buf(),
        Some(workspace) if !workspace.is_absolute() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "daemon request workspace must be an absolute path",
            ));
        }
        Some(workspace) => match workspace.canonicalize() {
            Ok(_) => validated_request_workspace(startup_workspace, startup_project, request)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                startup_workspace.to_path_buf()
            }
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "daemon request workspace path could not be canonicalized",
                ));
            }
        },
    };
    let mut project = startup_project.clone();
    project.workspace_root = Some(workspace_root.clone());
    Ok(DaemonRequestContext {
        workspace_root,
        project,
    })
}

fn atomic_request_context(
    startup_workspace: &Path,
    startup_project: &Project,
    outcome_ledger: &RequestOutcomeLedger,
    request: &RequestEnvelope,
    instance_id: &str,
) -> io::Result<DaemonRequestContext> {
    if !outcome_ledger
        .atomic_request_needs_preparation(request, &startup_project.db_path(), instance_id)
        .map_err(to_io_error)?
    {
        return replay_request_context(startup_workspace, startup_project, request);
    }

    let context = daemon_request_context(startup_workspace, startup_project, request)?;
    AgentContext::prepare_request_transaction(&context.workspace_root, Some(&context.project))
        .map_err(to_io_error)?;
    Ok(context)
}

fn execute_ledgered_daemon_request(
    startup_workspace: &Path,
    startup_project: &Project,
    outcome_ledger: &RequestOutcomeLedger,
    request: RequestEnvelope,
    effect: Effect,
    instance_id: &str,
    diagnostics: &DaemonDiagnostics,
) -> OutcomeExecution {
    outcome_ledger.execute(
        request,
        effect,
        instance_id,
        Duration::from_secs(30),
        |request| {
            let request_id = request.id.clone();
            let context = match daemon_request_context(startup_workspace, startup_project, &request)
            {
                Ok(context) => context,
                Err(error) => {
                    return daemon_workspace_error_response(request_id, &error);
                }
            };
            handle_request_with_project_and_diagnostics_as_writer(
                &context.workspace_root,
                Some(&context.project),
                request,
                diagnostics,
            )
        },
    )
}

fn spawn_daemon_after_lock(
    paths: &LocalRuntimePaths,
    socket_path: &Path,
) -> std::io::Result<Vec<String>> {
    let mut diagnostics_messages = Vec::new();
    let endpoint = paths.endpoint();
    if endpoint.exists() {
        let _ = endpoint.remove_stale();
        DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir()).record(
            "daemon.socket_remove_stale",
            serde_json::json!({ "socket_path": socket_path }),
        );
        diagnostics_messages.push("removed stale daemon endpoint".to_string());
    }

    spawn_daemon_paths(paths)?;
    diagnostics_messages.push("spawned daemon process".to_string());
    Ok(diagnostics_messages)
}

async fn restart_after_socket_wait_failure(
    paths: &LocalRuntimePaths,
    socket_path: &Path,
    diagnostics: &DaemonDiagnostics,
    diagnostics_messages: &mut Vec<String>,
    context: &str,
    error: std::io::Error,
) -> std::io::Result<()> {
    diagnostics.record(
        "daemon.socket_wait_failed",
        serde_json::json!({
            "socket_path": socket_path,
            "pid_path": paths.pid_path(),
            "context": context,
            "error": error.to_string(),
            "action": "restart_once"
        }),
    );
    diagnostics_messages.push(format!("daemon socket wait failed {context}: {error}"));
    diagnostics_messages.append(&mut restart_stale_daemon_once(paths, None, false).await);

    if let Some(lock_file) = try_lock_pid_file(&paths.lock_path()) {
        diagnostics.record(
            "daemon.pid_lock_after_restart",
            serde_json::json!({ "acquired": true, "context": context }),
        );
        diagnostics_messages.push("acquired daemon PID lock after stale restart".to_string());
        drop(lock_file);
        diagnostics_messages.append(&mut spawn_daemon_after_lock(paths, socket_path)?);
        wait_for_socket_paths(paths, daemon_startup_timeout()).await
    } else {
        diagnostics.record(
            "daemon.pid_lock_after_restart",
            serde_json::json!({ "acquired": false, "context": context }),
        );
        diagnostics_messages
            .push("daemon PID lock still held after stale restart; waited again".to_string());
        wait_for_socket_paths(paths, daemon_startup_timeout()).await
    }
}

// ============================================================================
// Connect-or-Spawn Helpers
// ============================================================================

/// Try to acquire an exclusive lock on the PID file.
///
/// This is used to prevent double-spawn race conditions. If we can acquire
/// the lock, the daemon is not running (or crashed without cleanup).
///
/// Returns `Some(file)` if lock acquired, `None` if another process holds it.
fn try_lock_pid_file(pid_path: &Path) -> Option<File> {
    use std::fs::OpenOptions;

    // Open or create the PID file
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(pid_path)
        .ok()?;

    // Try non-blocking exclusive lock using fs2
    match file.try_lock_exclusive() {
        Ok(()) => Some(file),
        Err(_) => None, // Lock held by another process
    }
}

/// Try to connect to an existing daemon socket.
///
/// Returns `Ok(stream)` if connection succeeds, `Err` otherwise.
pub async fn connect_to_daemon(workspace_path: &Path) -> std::io::Result<DaemonStream> {
    let paths = paths_for_workspace(workspace_path)?;
    connect_to_daemon_paths(&paths).await
}

async fn connect_to_daemon_paths(paths: &LocalRuntimePaths) -> std::io::Result<DaemonStream> {
    paths.endpoint().connect().await
}

async fn probe_daemon_stream<S>(stream: &mut S, expected_instance_id: &str) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    probe_daemon_stream_with_timeout(stream, Some(expected_instance_id), DAEMON_PROBE_TIMEOUT)
        .await
        .map(|_| ())
}

#[derive(Debug)]
struct DaemonProbeResponse {
    instance_id: String,
    pid: u32,
    process_start_id: String,
}

async fn discover_daemon_stream<S>(stream: &mut S) -> std::io::Result<DaemonProbeResponse>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    probe_daemon_stream_with_timeout(stream, None, DAEMON_PROBE_TIMEOUT).await
}

async fn probe_daemon_stream_with_timeout<S>(
    stream: &mut S,
    expected_instance_id: Option<&str>,
    timeout: Duration,
) -> std::io::Result<DaemonProbeResponse>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let nonce = ulid::Ulid::new().to_string().to_lowercase();
    let mut request = serde_json::to_vec(&serde_json::json!({
        "kind": DAEMON_PROBE_KIND,
        "nonce": nonce,
    }))
    .map_err(io::Error::other)?;
    request.push(b'\n');
    stream.write_all(&request).await?;
    stream.flush().await?;

    let mut response = String::new();
    let read = async {
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await
    };
    let bytes_read = tokio::time::timeout(timeout, read)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "daemon probe timed out"))??;
    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "daemon closed the probe connection",
        ));
    }

    let response: serde_json::Value = serde_json::from_str(&response).map_err(io::Error::other)?;
    let kind = response.get("kind").and_then(serde_json::Value::as_str);
    let response_nonce = response.get("nonce").and_then(serde_json::Value::as_str);
    let instance_id = response
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon probe omitted instance id",
            )
        })?;
    let pid = response
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "daemon probe omitted PID"))?;
    let process_start_id = response
        .get("process_start_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon probe omitted process start identity",
            )
        })?;
    if kind != Some(DAEMON_PROBE_OK_KIND)
        || response_nonce != Some(nonce.as_str())
        || expected_instance_id.is_some_and(|expected| instance_id != expected)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon probe response did not match the expected runtime instance",
        ));
    }

    Ok(DaemonProbeResponse {
        instance_id: instance_id.to_string(),
        pid,
        process_start_id: process_start_id.to_string(),
    })
}

async fn connect_and_probe_daemon_paths(
    paths: &LocalRuntimePaths,
) -> std::io::Result<DaemonStream> {
    let identity = read_daemon_identity(paths)?;
    let instance_id = identity.instance_id.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon identity does not include an instance id",
        )
    })?;
    let mut stream = connect_to_daemon_paths(paths).await?;
    probe_daemon_stream(&mut stream, instance_id).await?;
    Ok(stream)
}

/// Spawn a new daemon process for the workspace.
///
/// The daemon is spawned as a detached background process using the current
/// executable with `daemon run --workspace <path>`.
///
/// Returns `Ok(())` if spawn succeeds, `Err` otherwise.
pub fn spawn_daemon(workspace_path: &Path) -> std::io::Result<()> {
    let paths = paths_for_workspace(workspace_path)?;
    spawn_daemon_paths(&paths)
}

fn spawn_daemon_paths(paths: &LocalRuntimePaths) -> std::io::Result<()> {
    let exe = exo_executable()?;
    let mut preserved_env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            key == "HOME"
                || key == "XDG_CONFIG_HOME"
                || (cfg!(windows)
                    && matches!(
                        key.to_ascii_uppercase().as_str(),
                        "USERPROFILE" | "APPDATA" | "HOMEDRIVE" | "HOMEPATH"
                    ))
        })
        .chain(
            crate::daemon_diagnostics::enabled_env_vars().map(|(key, value)| (key.into(), value)),
        )
        .collect();
    if let Some(config_home) = paths.config_home() {
        preserved_env.retain(|(key, _)| key != "XDG_CONFIG_HOME");
        preserved_env.push(("XDG_CONFIG_HOME".into(), config_home.as_os_str().into()));
        #[cfg(windows)]
        {
            preserved_env.retain(|(key, _)| !key.to_string_lossy().eq_ignore_ascii_case("APPDATA"));
            preserved_env.push(("APPDATA".into(), config_home.as_os_str().into()));
        }
    }

    spawn_daemon_process(&exe, paths, preserved_env)
}

#[cfg(not(windows))]
fn spawn_daemon_process(
    exe: &Path,
    paths: &LocalRuntimePaths,
    preserved_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> std::io::Result<()> {
    // Spawn detached: redirect stdin/stdout/stderr to null, don't wait.
    let mut command = Command::new(exe);
    command
        .arg("daemon")
        .arg("run")
        .arg("--workspace")
        .arg(paths.workspace())
        .envs(preserved_env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Null stdio does not detach the child from the caller's process group.
    // Put the daemon in its own group so terminal and runner cleanup for the
    // short-lived `daemon ensure` command cannot terminate a live daemon.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command.spawn()?;

    Ok(())
}

#[cfg(windows)]
fn spawn_daemon_process(
    exe: &Path,
    paths: &LocalRuntimePaths,
    preserved_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> std::io::Result<()> {
    fn ps_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn windows_command_line_quote(value: &str) -> String {
        if !value.is_empty() && !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
            return value.to_string();
        }

        let mut quoted = String::from("\"");
        let mut backslashes = 0;
        for ch in value.chars() {
            match ch {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                    quoted.push('"');
                    backslashes = 0;
                }
                _ => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes));
                    quoted.push(ch);
                    backslashes = 0;
                }
            }
        }
        quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
        quoted.push('"');
        quoted
    }

    fn preserved_env_value(
        env: &[(std::ffi::OsString, std::ffi::OsString)],
        name: &str,
    ) -> Option<std::ffi::OsString> {
        env.iter()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }

    let diagnostics_enabled =
        preserved_env_value(&preserved_env, crate::daemon_diagnostics::ENABLED_ENV).is_some();
    let diagnostics_path = preserved_env_value(&preserved_env, crate::daemon_diagnostics::PATH_ENV);

    let mut daemon_args = vec![
        "daemon".to_string(),
        "run".to_string(),
        "--workspace".to_string(),
        paths.workspace().display().to_string(),
    ];
    if diagnostics_enabled {
        daemon_args.push("--diagnostics-enabled".to_string());
        if let Some(path) = diagnostics_path {
            daemon_args.push("--diagnostics-path".to_string());
            daemon_args.push(path.to_string_lossy().into_owned());
        }
    }

    let argument_list = daemon_args
        .iter()
        .map(|arg| windows_command_line_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let env_script = preserved_env
        .iter()
        .map(|(key, value)| {
            format!(
                "$env:{} = {}",
                key.to_string_lossy(),
                ps_quote(&value.to_string_lossy())
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let launch_script = format!(
        "{}{}Start-Process -WindowStyle Hidden -FilePath {} -ArgumentList {}",
        env_script,
        if env_script.is_empty() { "" } else { "; " },
        ps_quote(&exe.display().to_string()),
        ps_quote(&argument_list)
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &launch_script])
        .envs(preserved_env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "failed to launch detached daemon process via Start-Process: {status}"
        )));
    }

    Ok(())
}

fn exo_executable() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_exo") {
        return Ok(PathBuf::from(path));
    }

    let exe = std::env::current_exe()?;
    if exe.file_stem().and_then(|stem| stem.to_str()) == Some("exo") {
        return Ok(exe);
    }

    if let Some(parent) = exe.parent() {
        let candidate = parent.join("exo");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Some(parent) = exe.parent().and_then(Path::parent) {
        let candidate = parent.join("exo");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "could not resolve exo binary; current executable is {}",
            exe.display()
        ),
    ))
}

/// Wait for the daemon socket to become available.
///
/// Polls with exponential backoff starting at 10ms, capped at 100ms.
/// Times out after the specified duration.
pub async fn wait_for_socket(workspace_path: &Path, timeout: Duration) -> std::io::Result<()> {
    let paths = paths_for_workspace(workspace_path)?;
    wait_for_socket_paths(&paths, timeout).await
}

async fn wait_for_socket_paths(
    paths: &LocalRuntimePaths,
    timeout: Duration,
) -> std::io::Result<()> {
    let endpoint = paths.endpoint();
    let start = std::time::Instant::now();
    let mut delay = Duration::from_millis(10);
    let max_delay = Duration::from_millis(100);

    loop {
        if endpoint.connect().await.is_ok() {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("daemon endpoint not available after {:?}", start.elapsed()),
            ));
        }

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }
}

/// Ensure a daemon is running and return a connection to it.
///
/// This implements the connect-or-spawn protocol:
/// 1. Try to connect to existing socket
/// 2. If connection fails and socket exists, check if PID is stale
/// 3. If stale, clean up old socket/PID files
/// 4. Spawn new daemon
/// 5. Wait for socket to become available
/// 6. Connect and return stream
///
/// # Errors
///
/// Returns an error if:
/// - Spawning the daemon fails
/// - The socket doesn't become available within the timeout
/// - Connection to the new daemon fails
pub async fn ensure_daemon(workspace_path: &Path) -> std::io::Result<DaemonStream> {
    Ok(ensure_daemon_with_report(workspace_path)
        .await?
        .into_stream())
}

/// Ensure a daemon is running and return both the connection and lifecycle report.
pub async fn ensure_daemon_with_report(
    workspace_path: &Path,
) -> std::io::Result<DaemonEnsureOutcome> {
    let paths = paths_for_workspace(workspace_path)?;
    ensure_daemon_paths_with_report(paths).await
}

/// Force-restart the workspace daemon and return both the new connection and
/// lifecycle report.
pub async fn restart_daemon_with_report(
    workspace_path: &Path,
) -> std::io::Result<DaemonEnsureOutcome> {
    let paths = paths_for_workspace(workspace_path)?;
    restart_daemon_paths_with_report(paths).await
}

/// Ensure a daemon for an explicitly resolved project.
pub async fn ensure_daemon_with_report_for_project(
    workspace_path: &Path,
    project: &Project,
) -> std::io::Result<DaemonEnsureOutcome> {
    let paths = paths_for_workspace_project(workspace_path, project)?;
    ensure_daemon_paths_with_report(paths).await
}

async fn restart_daemon_paths_with_report(
    paths: LocalRuntimePaths,
) -> std::io::Result<DaemonEnsureOutcome> {
    paths.ensure_dir()?;

    let socket_path = paths.socket_path();
    let pid_path = paths.pid_path();
    let lock_path = paths.lock_path();
    let diagnostics = DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir());
    let mut diagnostics_messages = vec!["forced daemon restart".to_string()];
    diagnostics.record(
        "daemon.restart",
        serde_json::json!({
            "workspace_root": paths.workspace(),
            "socket_path": socket_path,
            "pid_path": pid_path,
            "lock_path": lock_path,
        }),
    );
    diagnostics_messages.append(&mut restart_stale_daemon_once(&paths, None, false).await);

    let (state, spawned) = if let Some(lock_file) = try_lock_pid_file(&lock_path) {
        diagnostics.record(
            "daemon.lock_after_forced_restart",
            serde_json::json!({ "acquired": true, "lock_path": lock_path }),
        );
        diagnostics_messages.push("acquired daemon lock after forced restart".to_string());
        drop(lock_file);
        diagnostics_messages.append(&mut spawn_daemon_after_lock(&paths, &socket_path)?);
        if let Err(error) = wait_for_socket_paths(&paths, daemon_startup_timeout()).await {
            restart_after_socket_wait_failure(
                &paths,
                &socket_path,
                &diagnostics,
                &mut diagnostics_messages,
                "after forced restart",
                error,
            )
            .await?;
        }
        (DaemonEnsureState::Spawned, true)
    } else {
        diagnostics.record(
            "daemon.lock_after_forced_restart",
            serde_json::json!({ "acquired": false, "lock_path": lock_path }),
        );
        diagnostics_messages
            .push("daemon lock still held after forced restart; waited".to_string());
        wait_for_socket_paths(&paths, daemon_startup_timeout()).await?;
        (DaemonEnsureState::WaitedForLock, false)
    };

    let stream = connect_and_probe_daemon_paths(&paths).await?;
    let mut report = DaemonEnsureReport::new(&paths, state);
    report.kind = "daemon.restart";
    report.spawned = spawned;
    report.reused = !spawned;
    report.pid = read_pid_file(&paths.pid_path());
    report.diagnostics.append(&mut diagnostics_messages);
    report.diagnostic("connected to daemon socket");
    Ok(DaemonEnsureOutcome { stream, report })
}

async fn ensure_daemon_paths_with_report(
    paths: LocalRuntimePaths,
) -> std::io::Result<DaemonEnsureOutcome> {
    // Step 1: Reuse an existing daemon only when its runtime identity is
    // current and the process answers a bounded probe for that exact instance.
    let mut stale_restart_diagnostics = if let Ok(mut stream) =
        connect_to_daemon_paths(&paths).await
    {
        let recorded_identity = read_daemon_identity(&paths).ok();
        let instance_id = recorded_identity
            .as_ref()
            .and_then(|identity| identity.instance_id.as_deref());
        let identity_current = daemon_identity_matches_current(&paths);
        let (probe_error, observed_probe) = match instance_id {
            Some(instance_id) if identity_current => {
                match probe_daemon_stream_with_timeout(
                    &mut stream,
                    Some(instance_id),
                    DAEMON_PROBE_TIMEOUT,
                )
                .await
                {
                    Ok(_) => {
                        let mut report =
                            DaemonEnsureReport::new(&paths, DaemonEnsureState::ConnectedExisting);
                        report.diagnostic("connected to responsive daemon instance");
                        return Ok(DaemonEnsureOutcome { stream, report });
                    }
                    Err(error) => (error, None),
                }
            }
            _ => match discover_daemon_stream(&mut stream).await {
                Ok(observed) => (
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "daemon runtime identity is stale or incomplete",
                    ),
                    Some(observed),
                ),
                Err(error) => (error, None),
            },
        };

        drop(stream);
        let diagnostics = DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir());
        diagnostics.record(
            "daemon.identity_stale",
            serde_json::json!({
                "identity_path": paths.identity_path(),
                "workspace_root": paths.workspace(),
                "probe_error": probe_error.to_string(),
                "observed_instance_id": observed_probe.as_ref().map(|probe| probe.instance_id.as_str()),
                "observed_pid": observed_probe.as_ref().map(|probe| probe.pid),
                "action": "restart"
            }),
        );
        restart_stale_daemon_once(&paths, observed_probe.as_ref(), true).await
    } else {
        Vec::new()
    };

    // Step 2: Use flock to atomically check if daemon is running
    // This prevents the TOCTOU race where two clients both see "no daemon"
    // and both try to spawn.
    let socket_path = paths.socket_path();
    let lock_path = paths.lock_path();

    // Ensure runtime directory exists before trying to lock
    paths.ensure_dir()?;

    let (state, spawned, mut diagnostics_messages) =
        if let Some(lock_file) = try_lock_pid_file(&lock_path) {
            let diagnostics = DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir());
            diagnostics.record("daemon.pid_lock", serde_json::json!({ "acquired": true }));
            let mut diagnostics_messages = vec!["acquired daemon PID lock".to_string()];
            diagnostics_messages.append(&mut stale_restart_diagnostics);

            // Drop the lock before spawning so daemon can acquire it
            drop(lock_file);

            // Step 3: Spawn new daemon
            diagnostics_messages.append(&mut spawn_daemon_after_lock(&paths, &socket_path)?);

            // Step 4: Wait for the daemon endpoint to become connectable.
            if let Err(error) = wait_for_socket_paths(&paths, daemon_startup_timeout()).await {
                restart_after_socket_wait_failure(
                    &paths,
                    &socket_path,
                    &diagnostics,
                    &mut diagnostics_messages,
                    "after spawn",
                    error,
                )
                .await?;
            }
            (DaemonEnsureState::Spawned, true, diagnostics_messages)
        } else {
            let diagnostics = DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir());
            diagnostics.record("daemon.pid_lock", serde_json::json!({ "acquired": false }));
            // Lock held by another process - daemon is running or another client
            // is spawning it. Wait for socket to appear.
            let mut diagnostics_messages = vec![
                "daemon PID lock is held by another process".to_string(),
                "waited for daemon socket".to_string(),
            ];
            diagnostics_messages.append(&mut stale_restart_diagnostics);
            match wait_for_socket_paths(&paths, daemon_startup_timeout()).await {
                Ok(()) => (
                    DaemonEnsureState::WaitedForLock,
                    false,
                    diagnostics_messages,
                ),
                Err(error) => {
                    restart_after_socket_wait_failure(
                        &paths,
                        &socket_path,
                        &diagnostics,
                        &mut diagnostics_messages,
                        "while PID lock was held",
                        error,
                    )
                    .await?;
                    (DaemonEnsureState::Spawned, true, diagnostics_messages)
                }
            }
        };

    // Step 5: Connect to daemon
    let stream = connect_and_probe_daemon_paths(&paths).await?;
    let mut report = DaemonEnsureReport::new(&paths, state);
    report.spawned = spawned;
    report.reused = !spawned;
    report.pid = read_pid_file(&paths.pid_path());
    report.diagnostics.append(&mut diagnostics_messages);
    report.diagnostic("connected to daemon socket");
    Ok(DaemonEnsureOutcome { stream, report })
}

/// Delete agent events older than 7 days (RFC 10183 retention policy).
///
/// Best-effort: silently ignores errors if the database doesn't exist
/// or the table hasn't been created yet.
pub fn cleanup_old_events(workspace_root: &Path) {
    let project = Project::resolve(workspace_root).ok();
    cleanup_old_events_with_project(workspace_root, project.as_ref());
}

pub fn cleanup_old_events_with_project(workspace_root: &Path, project: Option<&Project>) {
    let db_path = crate::context::db_path(workspace_root, project);
    let _ = crate::event_db::with_event_db(&db_path, |conn| {
        conn.execute(
            "DELETE FROM agent_events WHERE timestamp < datetime('now', '-7 days')",
            [],
        )
    });
}

/// Run the daemon server for a workspace.
///
/// This is the entry point for `exo daemon run --workspace <path>`.
/// The daemon listens on the platform daemon endpoint and handles requests using the
/// same `handle_request` function as the stdio-based JSON server.
pub async fn run_daemon(
    workspace_path: PathBuf,
    idle_timeout_secs: Option<u64>,
    diagnostics_config: Option<DaemonDiagnosticsConfig>,
) {
    let timeout = idle_timeout_secs.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
    let (project, workspace_path) = match daemon_project_and_workspace(&workspace_path) {
        Ok((project, workspace_path)) => (Arc::new(project), workspace_path),
        Err(error) => {
            eprintln!("exo daemon: failed to resolve project: {error}");
            return;
        }
    };
    let paths = LocalRuntimePaths::new(&workspace_path, &project);
    let diagnostics = DaemonDiagnostics::from_runtime_dir_with_config(
        &paths.runtime_dir(),
        diagnostics_config.as_ref(),
    );
    let workspace = Arc::new(workspace_path);

    // Ensure project runtime directory exists
    if let Err(e) = paths.ensure_dir() {
        eprintln!("exo daemon: failed to create runtime dir: {e}");
        return;
    }

    // Acquire exclusive lock on PID file - this prevents double-spawn
    let pid_path = paths.pid_path();
    let lock_path = paths.lock_path();
    let lock_file = match try_lock_pid_file(&lock_path) {
        Some(f) => {
            diagnostics.record(
                "daemon.pid_lock",
                serde_json::json!({ "acquired": true, "pid_path": pid_path, "lock_path": lock_path }),
            );
            f
        }
        None => {
            diagnostics.record(
                "daemon.pid_lock",
                serde_json::json!({ "acquired": false, "pid_path": pid_path, "lock_path": lock_path }),
            );
            eprintln!("exo daemon: another daemon is already running (lock held)");
            return;
        }
    };

    // Keep the lock file separate from the PID metadata so Windows clients can
    // read daemon.pid while the daemon holds the exclusive spawn lock.
    if let Err(e) = std::fs::write(&pid_path, std::process::id().to_string()) {
        eprintln!("exo daemon: failed to write PID file: {e}");
        return;
    }
    diagnostics.record(
        "daemon.pid_written",
        serde_json::json!({ "pid": std::process::id(), "pid_path": paths.pid_path() }),
    );
    let runtime_identity = match write_daemon_identity(&paths) {
        Ok(identity) => {
            diagnostics.record(
                "daemon.identity_written",
                serde_json::json!({
                    "identity_path": paths.identity_path(),
                    "instance_id": identity.instance_id.as_deref(),
                }),
            );
            identity
        }
        Err(error) => {
            diagnostics.record(
                "daemon.identity_write_failed",
                serde_json::json!({
                    "identity_path": paths.identity_path(),
                    "error": error.to_string(),
                }),
            );
            eprintln!("exo daemon: failed to write executable identity: {error}");
            return;
        }
    };
    let instance_id: Arc<str> = runtime_identity
        .instance_id
        .expect("daemon runtime identity includes an instance id")
        .into();
    let process_start_id: Arc<str> = runtime_identity
        .process_start_id
        .expect("daemon runtime identity includes a process start identity")
        .into();
    let outcome_ledger = match RequestOutcomeLedger::open(paths.outcome_ledger_path()) {
        Ok(ledger) => Arc::new(ledger),
        Err(error) => {
            diagnostics.record(
                "daemon.outcome_ledger_open_failed",
                serde_json::json!({
                    "path": paths.outcome_ledger_path(),
                    "error": error.to_string(),
                }),
            );
            eprintln!("exo daemon: failed to open request outcome ledger: {error}");
            return;
        }
    };

    // Keep lock_file alive - dropping it releases the lock
    // We'll drop it explicitly at the end for clarity

    eprintln!(
        "exo daemon: starting for workspace {} (timeout: {}s)",
        workspace.display(),
        timeout
    );
    eprintln!("exo daemon: endpoint at {}", paths.endpoint().display());
    diagnostics.record(
        "daemon.start",
        serde_json::json!({
            "workspace": workspace.display().to_string(),
            "runtime_dir": paths.runtime_dir(),
            "socket_path": paths.socket_path(),
            "endpoint": paths.endpoint().display(),
            "timeout_secs": timeout,
        }),
    );

    // Remove stale endpoint if the platform exposes it as a filesystem entry.
    let endpoint = paths.endpoint();
    if endpoint.exists() {
        let _ = endpoint.remove_stale();
        diagnostics.record(
            "daemon.socket_remove_stale",
            serde_json::json!({ "socket_path": paths.socket_path(), "endpoint": endpoint.display() }),
        );
    }

    // Last activity timestamp for idle timeout
    let last_activity = Arc::new(AtomicU64::new(now_secs()));

    // Broadcast channel for write_happened notifications.
    // When any client's command produces effect: "write", all other
    // connected clients receive a notification so they can revalidate
    // their cached traces.
    let (write_tx, _) = tokio::sync::broadcast::channel::<()>(16);

    // Run the socket server
    let paths_clone = paths.clone();
    let diagnostics_clone = diagnostics.clone();
    let handler_diagnostics = diagnostics.clone();
    let last_activity_clone = Arc::clone(&last_activity);
    let cleanup_workspace = Arc::clone(&workspace);
    let cleanup_project = Arc::clone(&project);
    let request_project = Arc::clone(&project);
    let request_outcome_ledger = Arc::clone(&outcome_ledger);
    let request_instance_id = Arc::clone(&instance_id);
    let request_admission = Arc::new(tokio::sync::Semaphore::new(
        DEFAULT_DAEMON_MAX_IN_FLIGHT_REQUESTS,
    ));
    let server_handle = tokio::spawn(async move {
        run_socket_server(
            &paths_clone,
            Arc::clone(&workspace),
            Arc::clone(&project),
            Arc::clone(&instance_id),
            Arc::clone(&process_start_id),
            last_activity_clone,
            write_tx,
            diagnostics_clone,
            move |req: RequestEnvelope| {
                let workspace = Arc::clone(&workspace);
                let project = Arc::clone(&request_project);
                let diagnostics = handler_diagnostics.clone();
                let outcome_ledger = Arc::clone(&request_outcome_ledger);
                let instance_id = Arc::clone(&request_instance_id);
                let request_admission = Arc::clone(&request_admission);
                async move {
                    let request_id = req.id.clone();
                    let handler_request_id = request_id.clone();
                    dispatch_bounded_daemon_request(
                        request_id,
                        request_admission,
                        diagnostics.clone(),
                        move || {
                        if let Ok(Some(outcome)) =
                            outcome_ledger.terminal_outcome_before_preparation(&req)
                        {
                            return outcome.response;
                        }
                        let declared_recovery = request_declared_recovery(&req);
                        let reserved_recovery = outcome_ledger
                            .reserved_request_recovery_before_preparation(&req)
                            .ok()
                            .flatten();
                        let canonical_atomic_replay = declared_recovery.is_some_and(|recovery| {
                            recovery.recovery_class == RecoveryClass::AtomicProjectState
                                && outcome_ledger
                                    .atomic_request_needs_preparation(
                                        &req,
                                        &project.db_path(),
                                        &instance_id,
                                    )
                                    .is_ok_and(|needs_preparation| !needs_preparation)
                        });
                        let recovery = if canonical_atomic_replay {
                            declared_recovery
                        } else if reserved_recovery.is_some() {
                            reserved_recovery
                        } else {
                            let request_workspace = match validated_request_workspace(
                                &workspace,
                                project.as_ref(),
                                &req,
                            ) {
                                Ok(workspace) => workspace,
                                Err(error) => {
                                    return daemon_workspace_error_response(
                                        handler_request_id,
                                        &error,
                                    );
                                }
                            };
                            resolved_request_recovery(&request_workspace, &req)
                        };
                        match recovery {
                            Some(recovery)
                                if recovery.recovery_class
                                    == RecoveryClass::AtomicProjectState =>
                            {
                                let Some((namespace, operation)) = request_command_path(&req)
                                else {
                                    return daemon_handler_error_response(
                                        handler_request_id,
                                        ErrorCode::InvalidInput,
                                        "atomic request is missing a command path".to_string(),
                                    );
                                };
                                let request_context = match atomic_request_context(
                                    &workspace,
                                    project.as_ref(),
                                    &outcome_ledger,
                                    &req,
                                    &instance_id,
                                ) {
                                    Ok(project) => project,
                                    Err(error) => {
                                        return daemon_workspace_error_response(
                                            handler_request_id,
                                            &error,
                                        );
                                    }
                                };
                                let request_workspace = request_context.workspace_root;
                                let request_project = request_context.project;
                                outcome_ledger
                                    .execute_atomic_project_state(
                                        req,
                                        recovery.effect,
                                        &instance_id,
                                        Duration::from_secs(30),
                                        &request_project.db_path(),
                                        |req| {
                                            handle_request_with_project_and_diagnostics_as_atomic_writer(
                                                &request_workspace,
                                                Some(&request_project),
                                                req,
                                                &diagnostics,
                                            )
                                        },
                                        |response| {
                                            finalize_atomic_response_after_commit(
                                                &request_workspace,
                                                Some(&request_project),
                                                &namespace,
                                                &operation,
                                                recovery.effect,
                                                response,
                                                &diagnostics,
                                            )
                                        },
                                    )
                                    .response
                            }
                            Some(recovery)
                                if matches!(recovery.effect, Effect::Write | Effect::Exec) =>
                            {
                                execute_ledgered_daemon_request(
                                    &workspace,
                                    project.as_ref(),
                                    &outcome_ledger,
                                    req,
                                    recovery.effect,
                                    &instance_id,
                                    &diagnostics,
                                )
                                    .response
                            }
                            _ => {
                                let request_context = match daemon_request_context(
                                    &workspace,
                                    project.as_ref(),
                                    &req,
                                ) {
                                    Ok(context) => context,
                                    Err(error) => {
                                        return daemon_workspace_error_response(
                                            handler_request_id,
                                            &error,
                                        );
                                    }
                                };
                                handle_request_with_project_and_diagnostics_as_writer(
                                    &request_context.workspace_root,
                                    Some(&request_context.project),
                                    req,
                                    &diagnostics,
                                )
                            }
                        }
                    },
                    )
                    .await
                }
            },
        )
        .await;
    });
    if cleanup_project.db_path().exists() {
        tokio::task::spawn_blocking(move || {
            cleanup_old_events_with_project(&cleanup_workspace, Some(cleanup_project.as_ref()));
        });
    }

    // Idle timeout checker task.
    //
    // Polling strategy: We check every `timeout/2` seconds whether the time since
    // last activity exceeds the timeout. This means:
    // - Worst case: daemon exits up to `timeout/2` seconds after the actual timeout
    // - Best case: daemon exits immediately after the timeout
    // - Tradeoff: More frequent polling = more responsive but more CPU wake-ups
    //
    // Note: Uses wall-clock time via SystemTime. If the system clock jumps forward
    // (e.g., NTP correction), the daemon may exit early. Clock jumps backward will
    // delay exit. For a dev tool with 5-minute default timeout, this is acceptable.
    let timeout_duration = Duration::from_secs(timeout);
    let check_interval = timeout_duration / 2;
    let last_activity_checker = Arc::clone(&last_activity);
    let idle_checker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(check_interval).await;
            let last = last_activity_checker.load(Ordering::Relaxed);
            let elapsed = now_secs().saturating_sub(last);
            if elapsed >= timeout {
                eprintln!("exo daemon: idle timeout reached, shutting down");
                return;
            }
        }
    });

    #[cfg(unix)]
    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("exo daemon: failed to install SIGTERM handler: {error}");
            return;
        }
    };

    #[cfg(unix)]
    tokio::select! {
        _ = server_handle => {},
        _ = idle_checker => {
            // Idle timeout reached - exit gracefully
        },
        _ = tokio::signal::ctrl_c() => {
            eprintln!("exo daemon: shutting down (SIGINT)");
        },
        _ = sigterm.recv() => {
            eprintln!("exo daemon: shutting down (SIGTERM)");
        }
    }

    #[cfg(not(unix))]
    tokio::select! {
        _ = server_handle => {},
        _ = idle_checker => {
            // Idle timeout reached - exit gracefully
        },
        _ = tokio::signal::ctrl_c() => {
            eprintln!("exo daemon: shutting down (CTRL-C)");
        }
    }

    // Cleanup: remove endpoint and PID files, then release lock
    eprintln!("exo daemon: cleaning up");
    let _ = paths.endpoint().remove_stale();
    let _ = std::fs::remove_file(paths.pid_path());
    let _ = std::fs::remove_file(paths.lock_path());
    let _ = std::fs::remove_file(paths.identity_path());
    diagnostics.record(
        "daemon.cleanup",
        serde_json::json!({
            "socket_path": paths.socket_path(),
            "endpoint": paths.endpoint().display(),
            "pid_path": paths.pid_path()
        }),
    );

    // Explicitly drop the lock file to release the flock
    // (This happens automatically, but being explicit is clearer)
    drop(lock_file);
}

const fn daemon_handler_error_response(
    id: String,
    code: ErrorCode,
    message: String,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id,
        status: Status::Error,
        result: None,
        error: Some(ErrorBody {
            code,
            message,
            details: None,
        }),
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: None,
        trace: None,
    }
}

/// Log a file-save event to the `agent_events` table.
///
/// Best-effort: errors are silently ignored — event logging should never
/// block request processing.
fn log_file_save_event(
    workspace_root: &Path,
    project: Option<&Project>,
    agent_id: Option<&str>,
    summary: &str,
) {
    let db_path = crate::context::db_path(workspace_root, project);

    let text_id = ulid::Ulid::new().to_string().to_lowercase();
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Best-effort: event logging must never block request handling.
    let _ = crate::event_db::with_event_db(&db_path, |conn| {
        conn.execute(
            "INSERT INTO agent_events (text_id, timestamp, agent_id, event_type, summary)
             VALUES (?1, ?2, ?3, 'file_save', ?4)",
            exosuit_storage::params![text_id, timestamp, agent_id, summary],
        )
    });
}

/// Unsolicited notification sent to clients when a write occurs.
const WRITE_HAPPENED_NOTIFICATION: &str =
    r#"{"protocol_version":1,"id":"_notify","status":"ok","result":{"kind":"write_happened"}}"#;

/// Run a daemon IPC server that handles JSON-RPC style requests.
async fn run_socket_server<F, Fut>(
    paths: &LocalRuntimePaths,
    workspace_root: Arc<PathBuf>,
    project: Arc<Project>,
    instance_id: Arc<str>,
    process_start_id: Arc<str>,
    last_activity: Arc<AtomicU64>,
    write_tx: tokio::sync::broadcast::Sender<()>,
    diagnostics: DaemonDiagnostics,
    handler: F,
) where
    F: Fn(RequestEnvelope) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = crate::api::protocol::ResponseEnvelope> + Send,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_path = paths.socket_path();
    let endpoint = paths.endpoint();
    let mut listener = match endpoint.bind().await {
        Ok(l) => {
            diagnostics.record(
                "daemon.socket_bind",
                serde_json::json!({ "socket_path": socket_path, "endpoint": endpoint.display() }),
            );
            l
        }
        Err(e) => {
            eprintln!("exo daemon: failed to bind endpoint: {e}");
            return;
        }
    };

    eprintln!("exo daemon: listening on {}", endpoint.display());

    let handler = Arc::new(handler);
    let connection_admission =
        Arc::new(tokio::sync::Semaphore::new(DEFAULT_DAEMON_MAX_CONNECTIONS));

    loop {
        // Acquire before accept so accepted descriptors and connection tasks
        // remain bounded even when clients outpace request execution.
        let connection_permit = Arc::clone(&connection_admission)
            .acquire_owned()
            .await
            .expect("daemon connection admission semaphore remains open");
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("exo daemon: accept error: {e}");
                continue;
            }
        };
        diagnostics.record("socket.accept", serde_json::json!({}));

        let handler = Arc::clone(&handler);
        let last_activity = Arc::clone(&last_activity);
        let workspace_root = Arc::clone(&workspace_root);
        let project = Arc::clone(&project);
        let instance_id = Arc::clone(&instance_id);
        let process_start_id = Arc::clone(&process_start_id);
        let write_tx = write_tx.clone();
        let mut write_rx = write_tx.subscribe();
        let diagnostics = diagnostics.clone();
        tokio::spawn(async move {
            let _connection_permit = connection_permit;
            let (reader, mut writer) = tokio::io::split(stream);
            let mut lines = BufReader::new(reader).lines();

            loop {
                tokio::select! {
                    line_result = lines.next_line() => {
                        let line = match line_result {
                            Ok(Some(line)) => line,
                            _ => break, // Connection closed or error
                        };

                        // Update last activity timestamp on each request
                        last_activity.store(now_secs(), Ordering::Relaxed);

                        // Parse as raw JSON first to distinguish requests from notifications
                        let raw: serde_json::Value = match serde_json::from_str(&line) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("exo daemon: invalid JSON: {e}");
                                continue;
                            }
                        };

                        if raw.get("kind").and_then(|v| v.as_str()) == Some(DAEMON_PROBE_KIND) {
                            let response = serde_json::json!({
                                "kind": DAEMON_PROBE_OK_KIND,
                                "nonce": raw.get("nonce").cloned().unwrap_or(serde_json::Value::Null),
                                "instance_id": instance_id.as_ref(),
                                "pid": std::process::id(),
                                "process_start_id": process_start_id.as_ref(),
                            });
                            let mut data = match serde_json::to_vec(&response) {
                                Ok(data) => data,
                                Err(error) => {
                                    eprintln!("exo daemon: failed to serialize probe response: {error}");
                                    continue;
                                }
                            };
                            data.push(b'\n');
                            if writer.write_all(&data).await.is_err() {
                                break;
                            }
                            continue;
                        }

                        // Notifications have "kind" — fire-and-forget, no response
                        if raw.get("kind").and_then(|v| v.as_str()) == Some("activity_event") {
                            let event_type = raw.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
                            let summary = raw.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                            let agent_id = raw.get("agent_id").and_then(|v| v.as_str());
                            if event_type == "file_save" {
                                log_file_save_event(
                                    &workspace_root,
                                    Some(project.as_ref()),
                                    agent_id,
                                    summary,
                                );
                            } else {
                                eprintln!("exo daemon: unknown activity_event type: {event_type}");
                            }
                            continue;
                        }

                        // Otherwise parse as a request envelope (has "op" key)
                        let request: RequestEnvelope = match serde_json::from_value(raw) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("exo daemon: invalid request: {e}");
                                continue;
                            }
                        };
                        let request_id = request.id.clone();
                        let op_path = request_op_path(&request);

                        let response = handler(request).await;

                        // If this was a write, notify all other clients
                        if response.effect == Some(Effect::Write) {
                            let _ = write_tx.send(());
                        }

                        let mut data = match serde_json::to_vec(&response) {
                            Ok(d) => d,
                            Err(e) => {
                                eprintln!("exo daemon: failed to serialize response: {e}");
                                continue;
                            }
                        };
                        data.push(b'\n');

                        let write_start = std::time::Instant::now();
                        if writer.write_all(&data).await.is_err() {
                            break;
                        }
                        diagnostics.record(
                            "request.write_end",
                            serde_json::json!({
                                "request_id": request_id,
                                "op_path": op_path,
                                "status": response_status(&response),
                                "effect": response.effect.map(effect_name),
                                "elapsed_ms": elapsed_ms(write_start.elapsed()),
                            }),
                        );
                    }
                    _ = write_rx.recv() => {
                        // Another client performed a write — notify this client
                        let mut data = WRITE_HAPPENED_NOTIFICATION.as_bytes().to_vec();
                        data.push(b'\n');
                        if writer.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                }
            }
            diagnostics.record("socket.connection_close", serde_json::json!({}));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{
        MAX_PORTABLE_UNIX_SOCKET_PATH_LEN, ProjectId, SidecarAutoPushPolicy, StatePolicy,
    };
    use std::path::PathBuf;
    use std::process::Command;

    fn run_test_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {} failed in {}: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn create_test_git_repo(temp: &tempfile::TempDir, name: &str) -> PathBuf {
        let workspace = temp.path().join(name);
        std::fs::create_dir(&workspace).expect("create git workspace");
        run_test_git(&workspace, &["init"]);
        std::fs::write(workspace.join("README.md"), name).expect("write test file");
        run_test_git(&workspace, &["add", "."]);
        run_test_git(
            &workspace,
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
        workspace.canonicalize().expect("canonical git workspace")
    }

    fn request_for_workspace(workspace_root: Option<&Path>) -> RequestEnvelope {
        serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "id": "workspace-context-test",
            "workspace_root": workspace_root,
            "op": {
                "kind": "call",
                "params": {
                    "address": { "kind": "operation", "path": ["project", "resolve"] },
                    "input": {}
                }
            }
        }))
        .expect("workspace request")
    }

    #[tokio::test]
    async fn daemon_probe_requires_matching_instance_response() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (mut client, server) = tokio::io::duplex(1024);
        let responder = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            let mut request = String::new();
            reader.read_line(&mut request).await.expect("read probe");
            let request: serde_json::Value = serde_json::from_str(&request).expect("parse probe");
            let response = serde_json::json!({
                "kind": DAEMON_PROBE_OK_KIND,
                "nonce": request["nonce"],
                "instance_id": "instance-a",
                "pid": 42,
                "process_start_id": "test-start-42",
            });
            let mut server = reader.into_inner();
            server
                .write_all(format!("{response}\n").as_bytes())
                .await
                .expect("write probe response");
        });

        probe_daemon_stream_with_timeout(
            &mut client,
            Some("instance-a"),
            Duration::from_millis(100),
        )
        .await
        .expect("matching daemon probe");
        responder.await.expect("probe responder");
    }

    #[tokio::test]
    async fn daemon_probe_times_out_when_socket_does_not_answer() {
        let (mut client, _server) = tokio::io::duplex(1024);
        let error = probe_daemon_stream_with_timeout(
            &mut client,
            Some("instance-a"),
            Duration::from_millis(10),
        )
        .await
        .expect_err("unanswered probe should time out");

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn bounded_daemon_request_admission_returns_stable_busy_response() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let admission = Arc::new(tokio::sync::Semaphore::new(1));
        let invocation_count = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first_admission = Arc::clone(&admission);
        let first_invocation_count = Arc::clone(&invocation_count);

        let first = tokio::spawn(async move {
            dispatch_bounded_daemon_request(
                "first".to_string(),
                first_admission,
                DaemonDiagnostics::disabled(),
                move || {
                    first_invocation_count.fetch_add(1, Ordering::SeqCst);
                    started_tx.send(()).expect("signal first request start");
                    release_rx.recv().expect("release first request");
                    ResponseEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        id: "first".to_string(),
                        status: Status::Ok,
                        result: Some(serde_json::json!({ "ok": true })),
                        error: None,
                        ticket: None,
                        steering: None,
                        reminders: None,
                        display: None,
                        preview: None,
                        effect: None,
                        trace: None,
                    }
                },
            )
            .await
        });

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("first request starts within the admission bound")
            .expect("first request start signal");

        let second_invocation_count = Arc::clone(&invocation_count);
        let busy = tokio::time::timeout(
            Duration::from_millis(250),
            dispatch_bounded_daemon_request(
                "second".to_string(),
                Arc::clone(&admission),
                DaemonDiagnostics::disabled(),
                move || {
                    second_invocation_count.fetch_add(1, Ordering::SeqCst);
                    panic!("busy request must not be dispatched");
                },
            ),
        )
        .await
        .expect("busy response is bounded");

        assert_eq!(busy.status, Status::Error);
        let error = busy.error.expect("busy response error");
        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        assert_eq!(
            error.message,
            "daemon request capacity is exhausted; retry later with the same request ID"
        );
        assert_eq!(
            error.details,
            Some(serde_json::json!({
                "kind": "daemon.busy",
                "retryable": true,
                "retry_with_same_request_id": true,
                "request_outcome_checked": false,
            }))
        );
        assert_eq!(invocation_count.load(Ordering::SeqCst), 1);

        release_tx.send(()).expect("release first request");
        let first = tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .expect("first request finishes within the admission bound")
            .expect("join first request");
        assert_eq!(first.status, Status::Ok);
    }

    fn test_project(workspace: &Path, state_root: PathBuf) -> Project {
        Project {
            id: ProjectId::from_git_common_dir(&workspace.join(".git")),
            git_common_dir: workspace.join(".git"),
            workspace_root: Some(workspace.to_path_buf()),
            policy: StatePolicy::Repo,
            projects_config_path: None,
            state_root,
            sidecar_key: None,
            sidecar_root: None,
            sidecar_auto_commit: false,
            sidecar_auto_push: SidecarAutoPushPolicy::Never,
        }
    }

    #[test]
    fn daemon_request_project_refreshes_mutable_sidecar_policy() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let git_common_dir = temp.path().join("repo/.git");
        let sidecar_root = temp.path().join("sidecars");
        let config_path = temp.path().join("config/exo/projects.toml");
        std::fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config dir");
        let id = ProjectId::from_git_common_dir(&git_common_dir);
        std::fs::write(
            &config_path,
            format!(
                "[projects.\"{id}\"]\nstate = \"sidecar\"\nsidecar_key = \"repo\"\nsidecar_root = {:?}\nauto_commit = false\nauto_push = \"always\"\n",
                sidecar_root.display().to_string()
            ),
        )
        .expect("write projects policy");
        let state_root = sidecar_root.join("projects/repo");
        let startup = Project {
            id,
            git_common_dir,
            workspace_root: Some(temp.path().join("repo")),
            policy: crate::project::StatePolicy::Sidecar,
            projects_config_path: Some(config_path),
            state_root: state_root.clone(),
            sidecar_key: Some("repo".to_string()),
            sidecar_root: Some(sidecar_root),
            sidecar_auto_commit: true,
            sidecar_auto_push: crate::project::SidecarAutoPushPolicy::Never,
        };

        let refreshed = daemon_request_project(&startup).expect("refresh daemon project");
        assert_eq!(refreshed.runtime_dir(), startup.runtime_dir());
        assert!(!refreshed.sidecar_auto_commit);
        assert_eq!(
            refreshed.sidecar_auto_push,
            crate::project::SidecarAutoPushPolicy::Always
        );
    }

    #[test]
    fn daemon_request_context_accepts_linked_worktree_for_same_project_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let linked = temp.path().join("linked");
        run_test_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked-test",
                linked.to_str().expect("linked path"),
            ],
        );
        let linked = linked.canonicalize().expect("canonical linked worktree");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(Some(&linked));

        let context = daemon_request_context(&primary, &startup, &request)
            .expect("linked worktree request context");

        assert_eq!(context.workspace_root, linked);
        assert_eq!(context.project.id, startup.id);
        assert_eq!(context.project.state_root, startup.state_root);
        assert_eq!(context.project.workspace_root, Some(context.workspace_root));
    }

    #[test]
    fn daemon_request_context_accepts_git_submodule_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = create_test_git_repo(&temp, "submodule-source");
        let parent = create_test_git_repo(&temp, "parent");
        run_test_git(
            &parent,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                source.to_str().expect("source path"),
                "modules/child",
            ],
        );
        let child = parent
            .join("modules/child")
            .canonicalize()
            .expect("canonical submodule workspace");
        let startup = Project::resolve(&child).expect("resolve submodule project");
        assert!(
            !startup
                .worktree_index()
                .is_some_and(|worktrees| worktrees.contains_key(&child)),
            "the submodule workspace is not represented by the worktree index"
        );
        let request = request_for_workspace(Some(&child));

        let context = daemon_request_context(&child, &startup, &request)
            .expect("submodule workspace should resolve through project identity");

        assert_eq!(context.workspace_root, child);
        assert_eq!(context.project.id, startup.id);
        assert_eq!(context.project.state_root, startup.state_root);
    }

    #[test]
    fn daemon_request_context_normalizes_nested_directory_to_worktree_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = create_test_git_repo(&temp, "nested-project");
        let nested = workspace.join("nested/directory");
        std::fs::create_dir_all(&nested).expect("create nested directory");
        let startup = Project::resolve(&workspace).expect("resolve project");
        let request = request_for_workspace(Some(&nested));

        let context = daemon_request_context(&workspace, &startup, &request)
            .expect("nested directory should resolve to its worktree root");

        assert_eq!(
            context.workspace_root,
            workspace.canonicalize().expect("canonical worktree root")
        );
        assert_eq!(context.project.workspace_root, Some(context.workspace_root));
    }

    #[test]
    fn daemon_request_context_normalizes_file_path_to_worktree_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = create_test_git_repo(&temp, "file-project");
        let file = workspace.join("README.md");
        let startup = Project::resolve(&workspace).expect("resolve project");
        let request = request_for_workspace(Some(&file));

        let context = daemon_request_context(&workspace, &startup, &request)
            .expect("file path should resolve to its worktree root");

        assert_eq!(
            context.workspace_root,
            workspace.canonicalize().expect("canonical worktree root")
        );
        assert_eq!(context.project.workspace_root, Some(context.workspace_root));
    }

    #[test]
    fn daemon_request_context_rejects_workspace_from_another_project() {
        let primary_temp = tempfile::tempdir().expect("primary tempdir");
        let primary = create_test_git_repo(&primary_temp, "primary");
        let foreign_temp = tempfile::tempdir().expect("foreign tempdir");
        let foreign = create_test_git_repo(&foreign_temp, "foreign");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(Some(&foreign));

        let error = daemon_request_context(&primary, &startup, &request)
            .expect_err("foreign workspace must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.to_string(),
            "request workspace does not belong to this daemon's project and state root"
        );
        let response = daemon_workspace_error_response("foreign-workspace".to_string(), &error);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::PreconditionFailed)
        );
    }

    #[test]
    fn daemon_request_context_reports_unavailable_workspace_without_leaking_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let missing = temp.path().join("missing-worktree");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(Some(&missing));

        let error = daemon_request_context(&primary, &startup, &request)
            .expect_err("missing workspace must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "daemon request workspace path could not be canonicalized"
        );
        assert!(!error.to_string().contains(&missing.to_string_lossy()[..]));
        let response = daemon_workspace_error_response("missing-workspace".to_string(), &error);
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn canonical_replay_rejects_workspace_path_reused_by_another_project() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let linked = temp.path().join("linked");
        run_test_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked-replay-test",
                linked.to_str().expect("linked path"),
            ],
        );
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let mut request: RequestEnvelope = serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "id": "canonical-replay-reused-workspace",
            "workspace_root": linked,
            "op": {
                "kind": "call",
                "params": {
                    "address": { "kind": "operation", "path": ["epoch", "add"] },
                    "input": { "title": "Recorded epoch" }
                }
            }
        }))
        .expect("parse atomic request");
        let ledger = RequestOutcomeLedger::open(temp.path().join("runtime-outcomes.sqlite3"))
            .expect("open runtime ledger");
        std::fs::create_dir_all(startup.db_path().parent().expect("database parent"))
            .expect("create project state directory");
        drop(
            exosuit_storage::open_database(&startup.db_path())
                .expect("initialize project database"),
        );
        let execution = ledger.execute_atomic_project_state(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            &startup.db_path(),
            |request| ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Write),
                trace: None,
            },
            Ok,
        );
        assert_eq!(execution.response.status, Status::Ok);
        exosuit_storage::Connection::open(ledger.path())
            .expect("open runtime outcome database")
            .execute(
                "UPDATE daemon_request_outcomes
                 SET instance_id = 'retired-instance', response_json = NULL, completed_at = NULL
                 WHERE request_id = ?1",
                [&request.id],
            )
            .expect("simulate runtime outcome loss after canonical commit");

        std::fs::remove_dir_all(&linked).expect("remove original linked worktree");
        let foreign = create_test_git_repo(&temp, "linked");
        assert_eq!(
            foreign.canonicalize().expect("canonical foreign repo"),
            linked.canonicalize().expect("canonical reused path")
        );
        assert_eq!(
            startup
                .worktree_index()
                .expect("read retained worktree index")
                .get(&linked.canonicalize().expect("canonical foreign path")),
            Some(&false),
            "the retained Git index alone still accepts the reused path"
        );

        request.workspace_root = Some(linked);
        let error = atomic_request_context(&primary, &startup, &ledger, &request, "instance-b")
            .expect_err("replay must reject a workspace path reused by another project");

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.to_string(),
            "request workspace does not belong to this daemon's project and state root"
        );
    }

    #[test]
    fn explicit_startup_workspace_rejects_path_reused_by_another_project() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let linked = temp.path().join("linked-startup");
        run_test_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked-startup-reuse-test",
                linked.to_str().expect("linked path"),
            ],
        );
        let linked = linked.canonicalize().expect("canonical linked worktree");
        let startup = Project::resolve(&linked).expect("resolve linked startup project");
        let request = request_for_workspace(Some(&linked));

        std::fs::remove_dir_all(&linked).expect("remove startup worktree");
        let foreign = create_test_git_repo(&temp, "linked-startup");
        assert_eq!(
            foreign.canonicalize().expect("canonical foreign repo"),
            linked
        );

        let error = daemon_request_context(&linked, &startup, &request)
            .expect_err("explicit reused startup path must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            error.to_string(),
            "request workspace does not belong to this daemon's project and state root"
        );
    }

    #[test]
    fn replay_request_context_uses_startup_workspace_when_issuing_worktree_is_gone() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let missing = temp.path().join("removed-linked-worktree");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(Some(&missing));

        let context = replay_request_context(&primary, &startup, &request)
            .expect("removed issuing worktree should use the retained daemon workspace");

        assert_eq!(context.workspace_root, primary);
        assert_eq!(context.project.id, startup.id);
        assert_eq!(context.project.state_root, startup.state_root);
    }

    #[test]
    fn daemon_request_context_uses_startup_workspace_for_legacy_request() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(None);

        let context =
            daemon_request_context(&primary, &startup, &request).expect("legacy request context");

        assert_eq!(context.workspace_root, primary);
        assert_eq!(context.project.id, startup.id);
        assert_eq!(context.project.state_root, startup.state_root);
    }

    #[test]
    fn daemon_request_context_reuses_startup_project_for_explicit_startup_workspace() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request = request_for_workspace(Some(&primary));

        let context = daemon_request_context(&primary, &startup, &request)
            .expect("explicit startup workspace context");

        assert_eq!(context.workspace_root, primary);
        assert_eq!(context.project, startup);
    }

    #[test]
    fn validated_startup_workspace_reuses_retained_git_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = create_test_git_repo(&temp, "startup-fast-path");
        let mut startup = Project::resolve(&workspace).expect("resolve project");
        let invalid_config = temp.path().join("config/exo/projects.toml");
        std::fs::create_dir_all(invalid_config.parent().expect("config parent"))
            .expect("create config directory");
        std::fs::write(&invalid_config, "this is not valid TOML = [")
            .expect("write invalid project policy");
        startup.projects_config_path = Some(invalid_config);
        let request = request_for_workspace(Some(&workspace));

        let resolved = validated_request_workspace(&workspace, &startup, &request)
            .expect("matching retained Git identity should avoid policy resolution");

        assert_eq!(
            resolved,
            workspace.canonicalize().expect("canonical workspace")
        );
    }

    #[test]
    fn linked_worktree_local_arguments_drive_recovery_classification() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let linked = temp.path().join("linked");
        run_test_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked-recovery-test",
                linked.to_str().expect("linked path"),
            ],
        );
        let linked = linked.canonicalize().expect("canonical linked worktree");
        std::fs::write(linked.join("replacement.md"), "Replacement body.\n")
            .expect("write linked-worktree body file");
        let startup = Project::resolve(&primary).expect("resolve primary project");
        let request: RequestEnvelope = serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "id": "linked-recovery-classification",
            "workspace_root": linked,
            "op": {
                "kind": "call",
                "params": {
                    "address": { "kind": "operation", "path": ["rfc", "edit"] },
                    "input": {
                        "id": "0001",
                        "body-file": "replacement.md"
                    }
                }
            }
        }))
        .expect("linked RFC edit request");

        assert!(
            resolved_request_recovery(&primary, &request).is_none(),
            "the body file is intentionally absent from the daemon startup workspace"
        );
        let request_workspace = validated_request_workspace(&primary, &startup, &request)
            .expect("validate linked request workspace");
        let recovery = resolved_request_recovery(&request_workspace, &request)
            .expect("classify linked-worktree RFC edit");

        assert_eq!(recovery.effect, Effect::Write);
        assert_eq!(recovery.recovery_class, RecoveryClass::ExternalAtMostOnce);
    }

    #[test]
    fn removed_workspace_retry_preserves_in_flight_at_most_once_authority() {
        let temp = tempfile::tempdir().expect("tempdir");
        let primary = create_test_git_repo(&temp, "primary");
        let linked = temp.path().join("linked-in-flight");
        run_test_git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked-in-flight-test",
                linked.to_str().expect("linked path"),
            ],
        );
        let linked = linked.canonicalize().expect("canonical linked worktree");
        std::fs::write(linked.join("replacement.md"), "Replacement body.\n")
            .expect("write request argument file");
        let project = Project::resolve(&primary).expect("resolve daemon project");
        let request: RequestEnvelope = serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "id": "removed-workspace-in-flight-retry",
            "workspace_root": linked,
            "op": {
                "kind": "call",
                "params": {
                    "address": { "kind": "operation", "path": ["rfc", "edit"] },
                    "input": {
                        "id": "0001",
                        "body-file": "replacement.md"
                    }
                }
            }
        }))
        .expect("parse linked RFC edit request");
        let recovery = resolved_request_recovery(&linked, &request)
            .expect("classify request before its workspace disappears");
        assert_eq!(recovery.effect, Effect::Write);
        assert_eq!(recovery.recovery_class, RecoveryClass::ExternalAtMostOnce);
        let ledger = RequestOutcomeLedger::open(temp.path().join("runtime-outcomes.sqlite3"))
            .expect("open runtime ledger");
        let first = ledger.execute(
            request.clone(),
            recovery.effect,
            "retired-instance",
            Duration::ZERO,
            |request| {
                daemon_handler_error_response(
                    request.id,
                    ErrorCode::Internal,
                    "recorded response".to_string(),
                )
            },
        );
        assert!(!first.replayed);
        exosuit_storage::Connection::open(ledger.path())
            .expect("open runtime outcome database")
            .execute(
                "UPDATE daemon_request_outcomes
                 SET response_json = NULL, completed_at = NULL
                 WHERE request_id = ?1",
                [&request.id],
            )
            .expect("simulate interrupted external request");
        std::fs::remove_dir_all(&linked).expect("remove issuing worktree and body file");

        assert!(
            resolved_request_recovery(&linked, &request).is_none(),
            "current command construction should fail after the workspace disappears"
        );
        let reserved = ledger
            .reserved_request_recovery_before_preparation(&request)
            .expect("read in-flight recovery authority")
            .expect("matching in-flight reservation");
        assert_eq!(reserved, recovery);
        let retry = execute_ledgered_daemon_request(
            &primary,
            &project,
            &ledger,
            request,
            reserved.effect,
            "replacement-instance",
            &DaemonDiagnostics::disabled(),
        );

        assert!(!retry.replayed);
        assert_eq!(retry.response.status, Status::Error);
        assert_eq!(
            retry
                .response
                .error
                .as_ref()
                .and_then(|error| error.details.as_ref())
                .and_then(|details| details.get("kind")),
            Some(&serde_json::json!("daemon.request_outcome_indeterminate"))
        );
    }

    fn recorded_atomic_request(
        temp: &tempfile::TempDir,
        request_id: &str,
    ) -> (RequestOutcomeLedger, RequestEnvelope) {
        let request: RequestEnvelope = serde_json::from_value(serde_json::json!({
            "protocol_version": 1,
            "id": request_id,
            "op": {
                "kind": "call",
                "params": {
                    "address": {
                        "kind": "operation",
                        "path": ["epoch", "add"]
                    },
                    "input": {"title": "Recorded epoch"}
                }
            }
        }))
        .expect("parse request");
        let ledger = RequestOutcomeLedger::open(temp.path().join("runtime-outcomes.sqlite3"))
            .expect("open runtime ledger");
        let execution = ledger.execute(
            request.clone(),
            Effect::Write,
            "instance-a",
            Duration::ZERO,
            |request| {
                daemon_handler_error_response(
                    request.id,
                    ErrorCode::Internal,
                    "recorded response".to_string(),
                )
            },
        );
        assert!(!execution.replayed);
        (ledger, request)
    }

    #[test]
    fn recorded_non_atomic_replay_skips_project_policy_refresh() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let workspace = temp.path().join("repo");
        let config_path = temp.path().join("config/exo/projects.toml");
        std::fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config directory");
        std::fs::write(&config_path, "this is not valid TOML = [")
            .expect("write invalid project policy");
        let mut project = test_project(&workspace, workspace.join(".exo"));
        project.projects_config_path = Some(config_path);
        let (ledger, request) = recorded_atomic_request(&temp, "recorded-write-policy-drift");

        assert!(
            daemon_request_project(&project).is_err(),
            "fixture must fail mutable policy refresh"
        );
        let execution = execute_ledgered_daemon_request(
            &workspace,
            &project,
            &ledger,
            request,
            Effect::Write,
            "instance-a",
            &DaemonDiagnostics::disabled(),
        );

        assert!(execution.replayed);
        assert_eq!(
            execution.response.error.expect("recorded error").message,
            "recorded response"
        );
    }

    #[test]
    fn recorded_non_atomic_conflict_skips_projection_hydration() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let workspace = temp.path().join("repo");
        let projection_dir = workspace.join("docs/agent-context");
        std::fs::create_dir_all(&projection_dir).expect("create projection directory");
        std::fs::write(
            projection_dir.join("epochs.sql"),
            "not valid SQL projection",
        )
        .expect("write invalid projection");
        let project = test_project(&workspace, workspace.clone());
        let (ledger, request) = recorded_atomic_request(&temp, "recorded-write-conflict");
        let mut conflicting_request =
            serde_json::to_value(request).expect("serialize recorded request");
        conflicting_request["op"]["params"]["input"]["title"] =
            serde_json::json!("Conflicting epoch");
        let conflicting_request =
            serde_json::from_value(conflicting_request).expect("parse conflicting request");

        let execution = execute_ledgered_daemon_request(
            &workspace,
            &project,
            &ledger,
            conflicting_request,
            Effect::Write,
            "instance-a",
            &DaemonDiagnostics::disabled(),
        );

        assert!(!execution.replayed);
        assert_eq!(
            execution.response.error.expect("conflict error").details,
            Some(serde_json::json!({
                "kind": "daemon.request_id_conflict",
                "request_id": "recorded-write-conflict",
                "mutation_performed": false,
            }))
        );
        assert!(
            !project.db_path().exists(),
            "request-id conflict must not hydrate the broken projection"
        );
    }

    #[test]
    fn recorded_atomic_replay_skips_project_policy_refresh() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let workspace = temp.path().join("repo");
        let config_path = temp.path().join("config/exo/projects.toml");
        std::fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config directory");
        std::fs::write(&config_path, "this is not valid TOML = [")
            .expect("write invalid project policy");
        let mut project = test_project(&workspace, workspace.join(".exo"));
        project.projects_config_path = Some(config_path);
        let (ledger, request) = recorded_atomic_request(&temp, "recorded-before-policy-drift");

        assert!(
            daemon_request_project(&project).is_err(),
            "fixture must fail mutable policy refresh"
        );
        let replay_context =
            atomic_request_context(&workspace, &project, &ledger, &request, "instance-a")
                .expect("recorded response should bypass policy refresh");
        assert_eq!(replay_context.project, project);
    }

    #[test]
    fn recorded_atomic_replay_skips_projection_hydration() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let workspace = temp.path().join("repo");
        std::fs::create_dir_all(workspace.join("docs/agent-context"))
            .expect("create projection directory");
        std::fs::write(
            workspace.join("docs/agent-context/epochs.sql"),
            "not valid SQL projection",
        )
        .expect("write invalid projection");
        let project = test_project(&workspace, workspace.clone());
        let (ledger, request) = recorded_atomic_request(&temp, "recorded-before-hydration-drift");

        assert!(
            AgentContext::prepare_request_transaction(&workspace, Some(&project)).is_err(),
            "fixture must fail projection hydration"
        );
        let _ = std::fs::remove_dir_all(project.db_path().parent().expect("database parent"));
        let replay_context =
            atomic_request_context(&workspace, &project, &ledger, &request, "instance-a")
                .expect("recorded response should bypass projection hydration");
        assert_eq!(replay_context.project, project);
        assert!(
            !project.db_path().exists(),
            "replay preparation must not recreate the project database"
        );
    }

    #[test]
    fn test_local_runtime_paths() {
        let workspace = PathBuf::from("/home/user/project");
        let project = test_project(&workspace, workspace.join(".exo"));
        let paths = LocalRuntimePaths::new(&workspace, &project);

        assert_eq!(
            paths.runtime_dir(),
            PathBuf::from("/home/user/project/.exo/runtime")
        );
        assert_eq!(
            paths.socket_path(),
            PathBuf::from("/home/user/project/.exo/runtime/daemon.sock")
        );
        assert_eq!(
            paths.pid_path(),
            PathBuf::from("/home/user/project/.exo/runtime/daemon.pid")
        );
    }

    #[test]
    fn daemon_identity_matches_linked_workspace_with_same_project_authority() {
        let primary = PathBuf::from("/home/user/project");
        let linked = PathBuf::from("/home/user/project-linked");
        let project = test_project(&primary, PathBuf::from("/state/projects/project"));
        let recorded = RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&primary, &project))
            .expect("recorded identity");
        let current = RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&linked, &project))
            .expect("linked identity");

        assert_ne!(recorded.workspace_root, current.workspace_root);
        assert!(recorded.matches_project_authority(&current));
        assert!(recorded.matches_runtime(&current));
    }

    #[test]
    fn daemon_identity_rejects_different_project_or_state_root() {
        let workspace = PathBuf::from("/home/user/project");
        let project = test_project(&workspace, PathBuf::from("/state/projects/project"));
        let recorded =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&workspace, &project))
                .expect("recorded identity");

        let mut other_project = project.clone();
        other_project.id = ProjectId::from_git_common_dir(Path::new("/other/.git"));
        let other_project_identity =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&workspace, &other_project))
                .expect("other project identity");
        assert!(!recorded.matches_project_authority(&other_project_identity));

        let mut other_state = project;
        other_state.state_root = PathBuf::from("/state/projects/other");
        let other_state_identity =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&workspace, &other_state))
                .expect("other state identity");
        assert!(!recorded.matches_project_authority(&other_state_identity));
    }

    #[test]
    fn legacy_daemon_identity_is_compatible_only_with_its_recorded_workspace() {
        let primary = PathBuf::from("/home/user/project");
        let linked = PathBuf::from("/home/user/project-linked");
        let project = test_project(&primary, PathBuf::from("/state/projects/project"));
        let mut legacy =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&primary, &project))
                .expect("legacy identity");
        legacy.project_id = None;
        legacy.state_root = None;

        let primary_identity =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&primary, &project))
                .expect("primary identity");
        let linked_identity =
            RuntimeDaemonIdentity::current(&LocalRuntimePaths::new(&linked, &project))
                .expect("linked identity");

        assert!(legacy.matches_project_authority(&primary_identity));
        assert!(!legacy.matches_project_authority(&linked_identity));
    }

    #[test]
    fn long_runtime_paths_use_short_hashed_socket_path() {
        let workspace = PathBuf::from("/tmp/project");
        let long_component = "very-long-sidecar-root-component".repeat(5);
        let state_root = PathBuf::from("/tmp")
            .join(long_component)
            .join("projects")
            .join("exo2");
        let project = test_project(&workspace, state_root.clone());
        let paths = LocalRuntimePaths::new(&workspace, &project);

        assert_eq!(paths.runtime_dir(), state_root.join("runtime"));
        assert_eq!(paths.pid_path(), state_root.join("runtime/daemon.pid"));
        assert!(
            paths.socket_path().starts_with("/tmp/exo-daemon-sockets"),
            "long socket paths should use a stable short temp socket: {}",
            paths.socket_path().display()
        );
        assert!(
            paths.socket_path().to_string_lossy().len() < MAX_PORTABLE_UNIX_SOCKET_PATH_LEN,
            "fallback socket path should be short enough for Unix sockets"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_endpoint_uses_runtime_dir_not_socket_fallback() {
        let workspace = PathBuf::from(r"C:\project");
        let long_component = "very-long-sidecar-root-component".repeat(5);
        let state_root = PathBuf::from(r"C:\tmp")
            .join(long_component)
            .join("projects")
            .join("exo2");
        let project = test_project(&workspace, state_root);
        let paths = LocalRuntimePaths::new(&workspace, &project);

        assert_eq!(
            paths.endpoint().display(),
            DaemonEndpoint::from_runtime_dir(&paths.runtime_dir()).display()
        );
        assert_ne!(
            paths.endpoint().display(),
            DaemonEndpoint::from_socket_path(&paths.socket_path()).display()
        );
    }

    #[test]
    fn daemon_status_uses_short_socket_for_long_runtime_paths() {
        let workspace = PathBuf::from("/tmp/project");
        let long_component = "very-long-sidecar-root-component".repeat(5);
        let state_root = PathBuf::from("/tmp")
            .join(long_component)
            .join("projects")
            .join("exo2");
        let project = test_project(&workspace, state_root);

        let report = daemon_status_for_project(&workspace, &project);

        let socket_path = report
            .socket_path
            .expect("status should report socket path");
        assert!(
            socket_path.starts_with("/tmp/exo-daemon-sockets"),
            "daemon status should report the same short socket path used by the daemon: {}",
            socket_path.display()
        );
        assert!(
            socket_path.to_string_lossy().len() < MAX_PORTABLE_UNIX_SOCKET_PATH_LEN,
            "status fallback socket path should be short enough for Unix sockets"
        );
    }

    #[test]
    fn test_paths_for_workspace_requires_git_project() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();

        let paths = paths_for_workspace(workspace);
        assert!(paths.is_err());
    }

    #[test]
    fn test_paths_for_workspace_uses_project_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        let output = Command::new("git")
            .arg("init")
            .current_dir(workspace)
            .output()
            .unwrap();
        assert!(output.status.success());

        let paths = paths_for_workspace(workspace).unwrap();
        assert!(
            paths.socket_path().ends_with(".exo/runtime/daemon.sock"),
            "socket path should come from project runtime"
        );
        assert!(
            paths.pid_path().ends_with(".exo/runtime/daemon.pid"),
            "PID path should come from project runtime"
        );
        assert!(!paths.socket_path().ends_with(".runtime/daemon.sock"));
    }
}
