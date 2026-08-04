mod assets;
mod http;
pub(crate) mod planning;
mod snapshot;

use crate::api::protocol::{RequestEnvelope, ResponseEnvelope};
use crate::failure::ExoFailure;
use crate::project::{Project, ProjectResolver};
use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::MutexGuard;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::task::JoinHandle;

const TICKET_LIFETIME: Duration = Duration::from_hours(1);
const SESSION_RENEWAL_LIFETIME: Duration = Duration::from_hours(12);
const SESSION_IDLE_LIFETIME: Duration = Duration::from_mins(30);
const SESSION_PERSIST_INTERVAL: Duration = Duration::from_mins(5);
const SESSION_STORE_SCHEMA_VERSION: u8 = 1;
const MAX_SESSIONS: usize = 64;
const MAX_EVENT_STREAMS: usize = 32;
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
pub(crate) const SESSION_COOKIE_PREFIX: &str = "exo_workbench_session_";

type DispatchFuture = Pin<Box<dyn Future<Output = ResponseEnvelope> + Send>>;
type DispatchFn = dyn Fn(RequestEnvelope) -> DispatchFuture + Send + Sync;
type TerminalReplayFuture = Pin<
    Box<
        dyn Future<
                Output = std::result::Result<
                    Option<ResponseEnvelope>,
                    planning::WorkbenchPlanningError,
                >,
            > + Send,
    >,
>;
type TerminalReplayFn = dyn Fn(RequestEnvelope) -> TerminalReplayFuture + Send + Sync;
type AtomicPreparationProbeFuture = Pin<
    Box<dyn Future<Output = std::result::Result<bool, planning::WorkbenchPlanningError>> + Send>,
>;
type AtomicPreparationProbeFn =
    dyn Fn(RequestEnvelope) -> AtomicPreparationProbeFuture + Send + Sync;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct DaemonRequestDispatcher {
    dispatch: Arc<DispatchFn>,
    replay_terminal: Option<Arc<TerminalReplayFn>>,
    atomic_preparation_probe: Option<Arc<AtomicPreparationProbeFn>>,
}

impl DaemonRequestDispatcher {
    pub fn new<F, Fut>(dispatch: F) -> Self
    where
        F: Fn(RequestEnvelope) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ResponseEnvelope> + Send + 'static,
    {
        Self {
            dispatch: Arc::new(move |request| Box::pin(dispatch(request))),
            replay_terminal: None,
            atomic_preparation_probe: None,
        }
    }

    pub(crate) fn with_terminal_replay<F, Fut>(mut self, replay_terminal: F) -> Self
    where
        F: Fn(RequestEnvelope) -> Fut + Send + Sync + 'static,
        Fut: Future<
                Output = std::result::Result<
                    Option<ResponseEnvelope>,
                    planning::WorkbenchPlanningError,
                >,
            > + Send
            + 'static,
    {
        self.replay_terminal = Some(Arc::new(move |request| Box::pin(replay_terminal(request))));
        self
    }

    pub(crate) fn with_atomic_preparation_probe<F, Fut>(
        mut self,
        atomic_preparation_probe: F,
    ) -> Self
    where
        F: Fn(RequestEnvelope) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<bool, planning::WorkbenchPlanningError>>
            + Send
            + 'static,
    {
        self.atomic_preparation_probe = Some(Arc::new(move |request| {
            Box::pin(atomic_preparation_probe(request))
        }));
        self
    }

    pub async fn dispatch(&self, request: RequestEnvelope) -> ResponseEnvelope {
        (self.dispatch)(request).await
    }

    pub(crate) async fn replay_before_preparation(
        &self,
        request: RequestEnvelope,
    ) -> std::result::Result<Option<ResponseEnvelope>, planning::WorkbenchPlanningError> {
        if let Some(replay_terminal) = self.replay_terminal.as_ref()
            && let Some(response) = replay_terminal(request.clone()).await?
        {
            return Ok(Some(response));
        }
        let Some(atomic_preparation_probe) = self.atomic_preparation_probe.as_ref() else {
            return Ok(None);
        };
        if atomic_preparation_probe(request.clone()).await? {
            return Ok(None);
        }
        Ok(Some(self.dispatch(request).await))
    }
}

impl fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonRequestDispatcher")
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct DaemonRuntimeServices {
    host: WorkbenchHostManager,
}

impl DaemonRuntimeServices {
    pub const fn new(host: WorkbenchHostManager) -> Self {
        Self { host }
    }

    pub fn set_dispatcher(&self, dispatcher: DaemonRequestDispatcher) -> Result<()> {
        self.host.set_dispatcher(dispatcher)
    }

    pub fn launch(&self, workspace_root: &Path) -> Result<WorkbenchLaunchResult> {
        self.host.launch(workspace_root)
    }

    pub fn snapshot(&self, workspace_root: &Path) -> Result<WorkbenchSnapshot> {
        self.host.snapshot(workspace_root)
    }

    pub fn revision_after_write(&self) -> u64 {
        self.host.revision_after_write()
    }

    pub(crate) fn project_state_guard(
        &self,
    ) -> std::result::Result<MutexGuard<'_, ()>, planning::WorkbenchPlanningError> {
        self.host
            .inner
            .project_state_gate
            .lock()
            .map_err(|_| planning::WorkbenchPlanningError::internal())
    }

    pub(crate) fn validate_planning_context(
        &self,
        workspace_root: &Path,
        context: &planning::WorkbenchPlanningContext,
    ) -> std::result::Result<(), planning::WorkbenchPlanningError> {
        self.host
            .inner
            .validate_planning_context(workspace_root, context, false)
    }

    pub fn write_events(&self) -> broadcast::Receiver<u64> {
        self.host.write_events()
    }

    pub fn host_status(&self) -> Option<WorkbenchHostStatus> {
        self.host.host_status()
    }

    pub async fn shutdown(&self) {
        self.host.shutdown().await;
    }
}

impl fmt::Debug for DaemonRuntimeServices {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonRuntimeServices")
            .field("host", &self.host)
            .finish()
    }
}

#[derive(Clone)]
pub struct WorkbenchHostManager {
    inner: Arc<WorkbenchHostInner>,
}

impl fmt::Debug for WorkbenchHostManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkbenchHostManager")
            .field("project_id", &self.inner.project.id.as_str())
            .field("instance_id", &self.inner.instance_id)
            .finish()
    }
}

struct WorkbenchHostInner {
    project: Arc<Project>,
    instance_id: Arc<str>,
    process_start_id: Arc<str>,
    runtime: Handle,
    host_record_path: PathBuf,
    session_store_path: PathBuf,
    session_store_gate: Mutex<()>,
    last_activity: Arc<AtomicU64>,
    revision: AtomicU64,
    project_state_gate: Mutex<()>,
    write_tx: broadcast::Sender<u64>,
    dispatcher: OnceLock<DaemonRequestDispatcher>,
    state: Mutex<WorkbenchState>,
    event_admission: Arc<Semaphore>,
    completion_review_admission: Arc<Semaphore>,
}

#[derive(Default)]
struct WorkbenchState {
    host: Option<BoundHost>,
    preferred_port: Option<u16>,
    workspaces_by_root: HashMap<PathBuf, String>,
    workspaces_by_key: HashMap<String, WorkspaceRegistration>,
    pending_capabilities: HashMap<String, PendingCapability>,
    session_grants: HashMap<String, WorkbenchSessionGrantV1>,
    sessions: HashMap<String, WorkbenchSession>,
    completion_review_requests:
        HashMap<planning::CompletionReviewRequestKey, planning::CompletionReviewRequestRecord>,
    completion_reviews: HashMap<String, planning::CompletionReviewRecord>,
    completion_review_sequence: u64,
}

struct BoundHost {
    origin: String,
    expected_host: String,
    secret: [u8; 32],
    assets_hash: String,
    started_at: String,
    server_task_alive: bool,
    updated_at: String,
    last_error: Option<String>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceRegistration {
    pub(crate) key: String,
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    pub(crate) branch: Option<String>,
    pub(crate) head: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingCapability {
    workspace_key: String,
    expires_at: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkbenchSession {
    /// Stable, non-secret digest used for server-side correlation and replay binding.
    pub(crate) id: String,
    pub(crate) selector: String,
    pub(crate) project_id: String,
    pub(crate) workspace_key: String,
    pub(crate) workspace_root: PathBuf,
    pub(crate) capabilities: Vec<String>,
    pub(crate) created_at: u64,
    pub(crate) last_activity: u64,
    pub(crate) expires_at: u64,
    last_persisted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchSessionGrantV1 {
    credential_digest: String,
    selector: String,
    project_id: String,
    workspace_key: String,
    workspace_root: PathBuf,
    capabilities: Vec<String>,
    created_at: u64,
    last_activity: u64,
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchSessionStoreV1 {
    schema_version: u8,
    project_id: String,
    sessions: Vec<WorkbenchSessionGrantV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkbenchTicketV1 {
    version: u8,
    capability_id: String,
    instance_id: String,
    project_id: String,
    workspace_key: String,
    capabilities: Vec<String>,
    issued_at: u64,
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchLaunchResult {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub url: String,
    pub expires_at: String,
    pub expires_in_seconds: u64,
    pub reused_host: bool,
    pub project: WorkbenchProjectIdentity,
    pub workspace: WorkbenchWorkspaceIdentity,
    pub daemon: WorkbenchDaemonIdentity,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchProjectIdentity {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchWorkspaceIdentity {
    pub key: String,
    pub label: String,
    pub branch: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchDaemonIdentity {
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkbenchHostRecord {
    pub schema_version: u8,
    pub instance_id: String,
    pub pid: u32,
    pub process_start_id: String,
    pub origin: String,
    pub assets_hash: String,
    pub server_task_alive: bool,
    pub started_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchHostStatus {
    pub origin: String,
    pub assets_hash: String,
    pub server_task_alive: bool,
    pub updated_at: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkbenchSnapshot {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub observed_at: String,
    pub revision: u64,
    pub project: WorkbenchProjectIdentity,
    pub daemon: WorkbenchDaemonIdentity,
    pub workspace: WorkbenchSnapshotWorkspace,
    pub lanes: Vec<WorkbenchLaneSummary>,
    pub focused_lane: Option<WorkbenchLaneDetails>,
    pub phase: Option<WorkbenchPhase>,
    pub between_phases_context: Option<WorkbenchBetweenPhasesContext>,
    pub steering: WorkbenchSteering,
    pub diagnostics: Vec<WorkbenchDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchSnapshotWorkspace {
    pub key: String,
    pub label: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchLaneSummary {
    pub id: String,
    pub title: String,
    pub state: String,
    pub phase_id: String,
    pub phase_title: String,
    pub phase_status: String,
    pub focused_here: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchLaneDetails {
    #[serde(flatten)]
    pub summary: WorkbenchLaneSummary,
    pub intent: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchPhase {
    pub id: String,
    pub title: String,
    pub status: String,
    pub planning_available: bool,
    pub goals: Vec<WorkbenchGoal>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchBetweenPhasesContext {
    pub epoch_id: String,
    pub epoch_title: String,
    pub completed_phase: Option<WorkbenchCompletedPhaseSummary>,
    pub next_phase: Option<WorkbenchNextPhasePreview>,
    pub pending_phases: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchCompletedPhaseSummary {
    pub id: String,
    pub title: String,
    pub completed_at: String,
    pub goal_count: usize,
    pub completed_goals: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchNextPhasePreview {
    pub id: String,
    pub title: String,
    pub goal_count: usize,
    pub rfc_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchGoal {
    pub id: String,
    pub title: String,
    pub status: String,
    pub tasks: Vec<WorkbenchTask>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub progress: Vec<WorkbenchTaskProgress>,
    #[serde(skip_serializing_if = "is_false")]
    pub progress_truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchTaskProgress {
    pub message: String,
    pub created_at: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkbenchSteering {
    pub situation: String,
    pub next_actions: Vec<WorkbenchSuggestedAction>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkbenchSuggestedAction {
    pub label: String,
    pub command: String,
    pub rationale: String,
    pub intent: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct WorkbenchSessionResult {
    pub(crate) kind: &'static str,
    pub(crate) ok: bool,
    pub(crate) schema_version: u8,
    pub(crate) session_key: String,
    pub(crate) project_id: String,
    pub(crate) workspace_key: String,
    pub(crate) expires_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketExchangeError {
    Invalid,
    Busy,
    Unavailable,
}

impl WorkbenchHostManager {
    pub fn new(
        project: Arc<Project>,
        instance_id: Arc<str>,
        process_start_id: Arc<str>,
        runtime_dir: PathBuf,
        last_activity: Arc<AtomicU64>,
        runtime: Handle,
    ) -> Self {
        let (write_tx, _) = broadcast::channel(16);
        let host_record_path = runtime_dir.join("workbench.host.json");
        let session_store_path = runtime_dir.join("workbench.sessions.json");
        let mut state = WorkbenchState::default();
        match read_session_store(&session_store_path, project.id.as_str(), unix_seconds()) {
            Ok(grants) => state.session_grants = grants,
            Err(error) => {
                eprintln!(
                    "exo daemon: failed to read workbench session store at {}: {error}",
                    session_store_path.display()
                );
            }
        }
        if !state.session_grants.is_empty() {
            state.preferred_port = resumable_host_port(&host_record_path);
        }
        Self {
            inner: Arc::new(WorkbenchHostInner {
                project,
                instance_id,
                process_start_id,
                runtime,
                host_record_path,
                session_store_path,
                session_store_gate: Mutex::new(()),
                last_activity,
                revision: AtomicU64::new(0),
                project_state_gate: Mutex::new(()),
                write_tx,
                dispatcher: OnceLock::new(),
                state: Mutex::new(state),
                event_admission: Arc::new(Semaphore::new(MAX_EVENT_STREAMS)),
                completion_review_admission: Arc::new(Semaphore::new(
                    planning::MAX_COMPLETION_REVIEWS_IN_FLIGHT,
                )),
            }),
        }
    }

    pub fn set_dispatcher(&self, dispatcher: DaemonRequestDispatcher) -> Result<()> {
        self.inner
            .dispatcher
            .set(dispatcher)
            .map_err(|_| anyhow::anyhow!("daemon request dispatcher was already installed"))?;
        let resume_host = self
            .inner
            .state
            .lock()
            .is_ok_and(|state| assets::available() && state.preferred_port.is_some());
        if resume_host && let Err(error) = self.ensure_host() {
            eprintln!("exo daemon: failed to resume the prior workbench origin: {error}");
        }
        Ok(())
    }

    pub fn launch(&self, workspace_root: &Path) -> Result<WorkbenchLaunchResult> {
        if !assets::available() {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.ui_unavailable",
                "This Exo binary was built without the embedded workbench UI",
            )));
        }

        let workspace = self.register_workspace(workspace_root)?;
        let (origin, reused_host, secret) = self.ensure_host()?;
        let issued_at = unix_seconds();
        let expires_at = issued_at.saturating_add(TICKET_LIFETIME.as_secs());
        let capability_id = random_token()?;
        let payload = WorkbenchTicketV1 {
            version: 1,
            capability_id: capability_id.clone(),
            instance_id: self.inner.instance_id.to_string(),
            project_id: self.inner.project.id.to_string(),
            workspace_key: workspace.key.clone(),
            capabilities: std::iter::once("workbench.snapshot")
                .chain(std::iter::once("lane.focus"))
                .chain(planning::PLANNING_CAPABILITIES)
                .map(str::to_string)
                .collect(),
            issued_at,
            expires_at,
        };
        let ticket = sign_ticket(&secret, &payload)?;

        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        state
            .pending_capabilities
            .retain(|_, pending| pending.expires_at > issued_at);
        state.pending_capabilities.insert(
            capability_id,
            PendingCapability {
                workspace_key: workspace.key.clone(),
                expires_at,
            },
        );
        drop(state);

        Ok(WorkbenchLaunchResult {
            kind: "workbench.launch",
            ok: true,
            schema_version: 1,
            url: format!("{origin}/#ticket={ticket}"),
            expires_at: timestamp_for_unix_seconds(expires_at),
            expires_in_seconds: TICKET_LIFETIME.as_secs(),
            reused_host,
            project: WorkbenchProjectIdentity {
                id: self.inner.project.id.to_string(),
            },
            workspace: WorkbenchWorkspaceIdentity {
                key: workspace.key,
                label: workspace.label,
                branch: workspace.branch,
                head: workspace.head,
            },
            daemon: WorkbenchDaemonIdentity {
                instance_id: self.inner.instance_id.to_string(),
            },
        })
    }

    pub fn snapshot(&self, workspace_root: &Path) -> Result<WorkbenchSnapshot> {
        self.snapshot_with_before_state_gate(workspace_root, || {})
    }

    fn snapshot_with_before_state_gate(
        &self,
        workspace_root: &Path,
        before_state_gate: impl FnOnce(),
    ) -> Result<WorkbenchSnapshot> {
        let (workspace, git) = self.register_workspace_with_git(workspace_root)?;
        before_state_gate();
        let _project_state_guard = self.inner.project_state_gate.lock().map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.snapshot_unavailable",
                "The workbench snapshot is temporarily unavailable",
            ))
        })?;
        snapshot::build_with_git(
            &self.inner.project,
            &workspace,
            self.inner.revision.load(Ordering::Acquire),
            &self.inner.instance_id,
            git,
        )
        .map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.snapshot_unavailable",
                "The workbench snapshot is temporarily unavailable",
            ))
        })
    }

    pub fn revision_after_write(&self) -> u64 {
        let revision = self
            .inner
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let _ = self.inner.write_tx.send(revision);
        revision
    }

    pub fn write_events(&self) -> broadcast::Receiver<u64> {
        self.inner.write_tx.subscribe()
    }

    pub fn host_status(&self) -> Option<WorkbenchHostStatus> {
        let status = {
            let state = self.inner.state.lock().ok()?;
            let host = state.host.as_ref()?;
            WorkbenchHostStatus {
                origin: host.origin.clone(),
                assets_hash: host.assets_hash.clone(),
                server_task_alive: host.server_task_alive,
                updated_at: host.updated_at.clone(),
                last_error: host.last_error.clone(),
            }
        };
        Some(status)
    }

    pub async fn shutdown(&self) {
        let task = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            state.host.as_mut().and_then(|host| {
                let _ = host.shutdown.send(true);
                host.task.take()
            })
        };
        if let Some(mut task) = task
            && tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        if let Err(error) = self.inner.persist_session_store() {
            eprintln!("exo daemon: failed to persist workbench sessions during shutdown: {error}");
        }
        self.inner.mark_host_inactive();
    }

    fn register_workspace(&self, workspace_root: &Path) -> Result<WorkspaceRegistration> {
        self.register_workspace_with_git(workspace_root)
            .map(|(workspace, _)| workspace)
    }

    fn register_workspace_with_git(
        &self,
        workspace_root: &Path,
    ) -> Result<(WorkspaceRegistration, snapshot::GitSnapshot)> {
        let root = self.inner.validate_workspace(workspace_root)?;
        let git = snapshot::sample_git(&root);
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let now = unix_seconds();
        let key = state.workspaces_by_root.get(&root).cloned().or_else(|| {
            state
                .session_grants
                .values()
                .filter(|grant| {
                    grant.project_id == self.inner.project.id.as_str()
                        && grant.workspace_root == root
                        && grant.is_live(now)
                })
                .max_by_key(|grant| grant.last_activity)
                .map(|grant| grant.workspace_key.clone())
        });
        let key = key.unwrap_or(random_token()?);
        let label = git
            .branch
            .clone()
            .or_else(|| {
                git.head
                    .as_deref()
                    .map(|head| format!("detached@{}", &head[..head.len().min(8)]))
            })
            .unwrap_or_else(|| "detached".to_string());
        let workspace = WorkspaceRegistration {
            key: key.clone(),
            root: root.clone(),
            label,
            branch: git.branch.clone(),
            head: git.head.clone(),
        };
        state.workspaces_by_root.insert(root, key.clone());
        state.workspaces_by_key.insert(key, workspace.clone());
        drop(state);
        Ok((workspace, git))
    }

    fn ensure_host(&self) -> Result<(String, bool, [u8; 32])> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        if let Some(host) = state.host.as_ref()
            && host.server_task_alive
        {
            return Ok((host.origin.clone(), true, host.secret));
        }
        if self.inner.dispatcher.get().is_none() {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.host_unavailable",
                "The daemon workbench dispatcher is not ready",
            )));
        }

        let listener = if let Some(port) = state.preferred_port {
            match TcpListener::bind(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
            )) {
                Ok(listener) => Ok(listener),
                Err(preferred_error) => {
                    eprintln!(
                        "exo daemon: prior workbench port {port} is unavailable; binding a new loopback port: {preferred_error}"
                    );
                    TcpListener::bind(SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::LOCALHOST),
                        0,
                    ))
                }
            }
        } else {
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        }
        .map_err(|error| {
            anyhow::Error::new(
                workbench_failure(
                    "workbench.host_unavailable",
                    "The local workbench host could not bind a loopback port",
                )
                .with_details(serde_json::json!({
                    "kind": "workbench.host_unavailable",
                    "error": error.to_string(),
                })),
            )
        })?;
        listener
            .set_nonblocking(true)
            .context("configure workbench listener")?;
        let address = listener.local_addr().context("read workbench address")?;
        state.preferred_port = Some(address.port());
        let expected_host = address.to_string();
        let origin = format!("http://{expected_host}");
        let secret = random_bytes()?;
        let started_at = timestamp_now();
        let assets_hash = assets::hash();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let tokio_listener =
            tokio::net::TcpListener::from_std(listener).context("adopt workbench listener")?;
        let weak = Arc::downgrade(&self.inner);
        let task = self.inner.runtime.spawn(async move {
            let result = http::serve(tokio_listener, Weak::clone(&weak), shutdown_rx).await;
            if let Some(inner) = weak.upgrade() {
                inner.server_stopped(result.err().map(|error| error.to_string()));
            }
        });
        let updated_at = timestamp_now();
        state.host = Some(BoundHost {
            origin: origin.clone(),
            expected_host,
            secret,
            assets_hash,
            started_at,
            server_task_alive: true,
            updated_at,
            last_error: None,
            shutdown,
            task: Some(task),
        });
        drop(state);
        self.inner.persist_host_record();
        Ok((origin, false, secret))
    }
}

impl WorkbenchHostInner {
    pub(crate) fn dispatcher(&self) -> Option<&DaemonRequestDispatcher> {
        self.dispatcher.get()
    }

    pub(crate) fn expected_host(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.host.as_ref().map(|host| host.expected_host.clone()))
    }

    pub(crate) fn origin(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.host.as_ref().map(|host| host.origin.clone()))
    }

    pub(crate) fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<u64> {
        self.write_tx.subscribe()
    }

    pub(crate) fn event_admission(&self) -> Arc<Semaphore> {
        Arc::clone(&self.event_admission)
    }

    pub(crate) fn completion_review_admission(&self) -> Arc<Semaphore> {
        Arc::clone(&self.completion_review_admission)
    }

    pub(crate) fn touch_daemon_activity(&self) {
        self.last_activity.store(unix_seconds(), Ordering::Relaxed);
    }

    pub(crate) fn redeem_ticket(
        &self,
        ticket: &str,
    ) -> Result<(String, WorkbenchSessionResult), TicketExchangeError> {
        let mut parts = ticket.split('.');
        if parts.next() != Some("v1") {
            return Err(TicketExchangeError::Invalid);
        }
        let (Some(payload_part), Some(signature_part), None) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(TicketExchangeError::Invalid);
        };
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_part)
            .map_err(|_| TicketExchangeError::Invalid)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_part)
            .map_err(|_| TicketExchangeError::Invalid)?;
        let secret = self
            .state
            .lock()
            .map_err(|_| TicketExchangeError::Invalid)?
            .host
            .as_ref()
            .map(|host| host.secret)
            .ok_or(TicketExchangeError::Invalid)?;
        let mut verifier =
            HmacSha256::new_from_slice(&secret).map_err(|_| TicketExchangeError::Invalid)?;
        verifier.update(&payload_bytes);
        verifier
            .verify_slice(&signature)
            .map_err(|_| TicketExchangeError::Invalid)?;
        let payload: WorkbenchTicketV1 =
            serde_json::from_slice(&payload_bytes).map_err(|_| TicketExchangeError::Invalid)?;
        let now = unix_seconds();
        if payload.version != 1
            || payload.instance_id != self.instance_id.as_ref()
            || payload.project_id != self.project.id.as_str()
            || payload.expires_at <= now
            || payload.issued_at > now
        {
            return Err(TicketExchangeError::Invalid);
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| TicketExchangeError::Invalid)?;
        state
            .pending_capabilities
            .retain(|_, pending| pending.expires_at > now);
        retain_live_sessions(&mut state, now);
        if state.session_grants.len() >= MAX_SESSIONS {
            return Err(TicketExchangeError::Busy);
        }
        let pending = state
            .pending_capabilities
            .remove(&payload.capability_id)
            .ok_or(TicketExchangeError::Invalid)?;
        if pending.workspace_key != payload.workspace_key
            || pending.expires_at != payload.expires_at
        {
            return Err(TicketExchangeError::Invalid);
        }
        let workspace = state
            .workspaces_by_key
            .get(&payload.workspace_key)
            .cloned()
            .ok_or(TicketExchangeError::Invalid)?;
        let session_secret = random_token().map_err(|_| TicketExchangeError::Invalid)?;
        let credential_digest = session_credential_digest(&session_secret);
        let session_key = random_token().map_err(|_| TicketExchangeError::Invalid)?;
        let session_expires_at = now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs());
        let session = WorkbenchSession {
            id: credential_digest.clone(),
            selector: session_key.clone(),
            project_id: payload.project_id.clone(),
            workspace_key: payload.workspace_key.clone(),
            workspace_root: workspace.root,
            capabilities: payload.capabilities,
            created_at: now,
            last_activity: now,
            expires_at: session_expires_at,
            last_persisted_at: now,
        };
        state.session_grants.insert(
            credential_digest.clone(),
            WorkbenchSessionGrantV1::from(&session),
        );
        state.sessions.insert(credential_digest.clone(), session);
        let result = (
            session_secret,
            WorkbenchSessionResult {
                kind: "workbench.session",
                ok: true,
                schema_version: 1,
                session_key,
                project_id: payload.project_id,
                workspace_key: payload.workspace_key,
                expires_at: timestamp_for_unix_seconds(session_expires_at),
            },
        );
        drop(state);
        if self.persist_session_store().is_err() {
            if let Ok(mut state) = self.state.lock() {
                state.sessions.remove(&credential_digest);
                state.session_grants.remove(&credential_digest);
                state
                    .pending_capabilities
                    .insert(payload.capability_id, pending);
            }
            return Err(TicketExchangeError::Unavailable);
        }
        Ok(result)
    }

    pub(crate) fn session(
        &self,
        session_key: &str,
        session_secret: &str,
    ) -> Option<WorkbenchSession> {
        self.session_by_digest(session_key, &session_credential_digest(session_secret))
    }

    pub(crate) fn session_by_digest(
        &self,
        session_key: &str,
        credential_digest: &str,
    ) -> Option<WorkbenchSession> {
        if !self.restore_session(session_key, credential_digest) {
            return None;
        }
        let now = unix_seconds();
        let mut state = self.state.lock().ok()?;
        retain_live_sessions(&mut state, now);
        let session_is_bound = state
            .sessions
            .get(credential_digest)
            .is_some_and(|session| {
                session.selector == session_key
                    && session.project_id == self.project.id.as_str()
                    && state
                        .workspaces_by_key
                        .get(&session.workspace_key)
                        .is_some_and(|workspace| workspace.root == session.workspace_root)
            });
        if !session_is_bound {
            return None;
        }
        let (session, persist_activity) = {
            let session = state.sessions.get_mut(credential_digest)?;
            session.last_activity = now;
            let persist_activity =
                now.saturating_sub(session.last_persisted_at) >= SESSION_PERSIST_INTERVAL.as_secs();
            if persist_activity {
                session.last_persisted_at = now;
            }
            (session.clone(), persist_activity)
        };
        if let Some(grant) = state.session_grants.get_mut(credential_digest) {
            grant.last_activity = now;
        }
        drop(state);
        if persist_activity && let Err(error) = self.persist_session_store() {
            eprintln!("exo daemon: failed to persist workbench session activity: {error}");
        }
        Some(session)
    }

    pub(crate) fn renew_session(
        &self,
        session_key: &str,
        session_secret: &str,
    ) -> Result<Option<WorkbenchSessionResult>> {
        let session = match self.session(session_key, session_secret) {
            Some(session) => session,
            None => return Ok(None),
        };
        if self
            .validate_session_workspace(&session.workspace_root)
            .is_err()
        {
            return Ok(None);
        }
        let now = unix_seconds();
        let expires_at = now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs());
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let renewed = {
            let current = state
                .sessions
                .get_mut(&session.id)
                .filter(|current| current.selector == session_key);
            let Some(current) = current else {
                return Ok(None);
            };
            current.last_activity = now;
            current.last_persisted_at = now;
            current.expires_at = expires_at;
            current.clone()
        };
        state
            .session_grants
            .insert(session.id.clone(), WorkbenchSessionGrantV1::from(&renewed));
        drop(state);
        self.persist_session_store()
            .context("persist renewed workbench session")?;
        Ok(Some(WorkbenchSessionResult {
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: renewed.selector,
            project_id: renewed.project_id,
            workspace_key: renewed.workspace_key,
            expires_at: timestamp_for_unix_seconds(expires_at),
        }))
    }

    fn restore_session(&self, session_key: &str, credential_digest: &str) -> bool {
        let now = unix_seconds();
        let grant = {
            let Ok(mut state) = self.state.lock() else {
                return false;
            };
            retain_live_sessions(&mut state, now);
            if state.sessions.contains_key(credential_digest) {
                return true;
            }
            state.session_grants.get(credential_digest).cloned()
        };
        let Some(grant) = grant else {
            return false;
        };
        if grant.selector != session_key
            || grant.project_id != self.project.id.as_str()
            || !grant.is_live(now)
        {
            return false;
        }
        let Ok(root) = self.validate_workspace(&grant.workspace_root) else {
            return false;
        };
        if root != grant.workspace_root {
            return false;
        }
        let git = snapshot::sample_git(&root);
        let label = git
            .branch
            .clone()
            .or_else(|| {
                git.head
                    .as_deref()
                    .map(|head| format!("detached@{}", &head[..head.len().min(8)]))
            })
            .unwrap_or_else(|| "detached".to_string());
        let workspace = WorkspaceRegistration {
            key: grant.workspace_key.clone(),
            root: root.clone(),
            label,
            branch: git.branch,
            head: git.head,
        };

        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .session_grants
            .get(credential_digest)
            .is_none_or(|current| current != &grant)
            || state
                .workspaces_by_root
                .get(&root)
                .is_some_and(|key| key != &grant.workspace_key)
            || state
                .workspaces_by_key
                .get(&grant.workspace_key)
                .is_some_and(|registered| registered.root != root)
        {
            return false;
        }
        state
            .workspaces_by_root
            .insert(root, grant.workspace_key.clone());
        state
            .workspaces_by_key
            .insert(grant.workspace_key.clone(), workspace);
        state.sessions.insert(
            credential_digest.to_string(),
            WorkbenchSession {
                id: credential_digest.to_string(),
                selector: grant.selector,
                project_id: grant.project_id,
                workspace_key: grant.workspace_key,
                workspace_root: grant.workspace_root,
                capabilities: grant.capabilities,
                created_at: grant.created_at,
                last_activity: grant.last_activity,
                expires_at: grant.expires_at,
                last_persisted_at: grant.last_activity,
            },
        );
        true
    }

    pub(crate) fn validate_workspace(&self, workspace_root: &Path) -> Result<PathBuf> {
        let canonical = workspace_root.canonicalize().map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            ))
        })?;
        let resolver = self
            .project
            .projects_config_path
            .as_deref()
            .map_or_else(ProjectResolver::default, |path| {
                ProjectResolver::default().with_projects_config_path(path)
            });
        let resolved = resolver.resolve(&canonical).map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            ))
        })?;
        if resolved.id != self.project.id || resolved.state_root != self.project.state_root {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            )));
        }
        resolved.workspace_root.ok_or_else(|| {
            anyhow::Error::new(workbench_failure(
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            ))
        })
    }

    pub(crate) fn validate_session_workspace(&self, retained_root: &Path) -> Result<PathBuf> {
        let resolved_root = self.validate_workspace(retained_root)?;
        if resolved_root != retained_root {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.workspace_unavailable",
                "The workbench workspace is no longer available",
            )));
        }
        Ok(resolved_root)
    }

    fn persist_session_store(&self) -> Result<()> {
        let _gate = self
            .session_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench session store is unavailable"))?;
        let store = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            let mut sessions = state.session_grants.values().cloned().collect::<Vec<_>>();
            sessions.sort_by(|left, right| {
                left.selector
                    .cmp(&right.selector)
                    .then_with(|| left.credential_digest.cmp(&right.credential_digest))
            });
            WorkbenchSessionStoreV1 {
                schema_version: SESSION_STORE_SCHEMA_VERSION,
                project_id: self.project.id.to_string(),
                sessions,
            }
        };
        write_session_store(&self.session_store_path, &store)
            .with_context(|| format!("write {}", self.session_store_path.display()))
    }

    fn server_stopped(&self, error: Option<String>) {
        if let Ok(mut state) = self.state.lock()
            && let Some(host) = state.host.as_mut()
        {
            host.server_task_alive = false;
            host.updated_at = timestamp_now();
            host.last_error = error;
        }
        self.persist_host_record();
    }

    fn mark_host_inactive(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(host) = state.host.as_mut()
        {
            host.server_task_alive = false;
            host.updated_at = timestamp_now();
        }
        self.persist_host_record();
    }

    fn persist_host_record(&self) {
        let record = {
            let Ok(state) = self.state.lock() else {
                return;
            };
            let Some(host) = state.host.as_ref() else {
                return;
            };
            WorkbenchHostRecord {
                schema_version: 1,
                instance_id: self.instance_id.to_string(),
                pid: std::process::id(),
                process_start_id: self.process_start_id.to_string(),
                origin: host.origin.clone(),
                assets_hash: host.assets_hash.clone(),
                server_task_alive: host.server_task_alive,
                started_at: host.started_at.clone(),
                updated_at: host.updated_at.clone(),
                last_error: host.last_error.clone(),
            }
        };
        if let Err(error) = write_host_record(&self.host_record_path, &record) {
            eprintln!(
                "exo daemon: failed to write workbench host record at {}: {error}",
                self.host_record_path.display()
            );
        }
    }
}

fn resumable_host_port(path: &Path) -> Option<u16> {
    let record = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<WorkbenchHostRecord>(&bytes).ok())?;
    if record.schema_version != 1 {
        return None;
    }
    record
        .origin
        .strip_prefix("http://127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
}

impl WorkbenchSession {
    const fn is_live(&self, now: u64) -> bool {
        self.expires_at > now
            && self
                .last_activity
                .saturating_add(SESSION_IDLE_LIFETIME.as_secs())
                > now
    }

    pub(crate) fn allows(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == capability)
    }
}

impl WorkbenchSessionGrantV1 {
    const fn is_live(&self, now: u64) -> bool {
        self.expires_at > now
            && self
                .last_activity
                .saturating_add(SESSION_IDLE_LIFETIME.as_secs())
                > now
    }
}

impl From<&WorkbenchSession> for WorkbenchSessionGrantV1 {
    fn from(session: &WorkbenchSession) -> Self {
        Self {
            credential_digest: session.id.clone(),
            selector: session.selector.clone(),
            project_id: session.project_id.clone(),
            workspace_key: session.workspace_key.clone(),
            workspace_root: session.workspace_root.clone(),
            capabilities: session.capabilities.clone(),
            created_at: session.created_at,
            last_activity: session.last_activity,
            expires_at: session.expires_at,
        }
    }
}

fn retain_live_sessions(state: &mut WorkbenchState, now: u64) {
    state
        .session_grants
        .retain(|_, session| session.is_live(now));
    state.sessions.retain(|_, session| session.is_live(now));
    let live_sessions = state.sessions.keys().cloned().collect::<HashSet<_>>();
    state
        .completion_reviews
        .retain(|_, review| live_sessions.contains(&review.session_id));
    state
        .completion_review_requests
        .retain(|key, _| live_sessions.contains(&key.session_id));
}

pub fn daemon_required_failure() -> ExoFailure {
    workbench_failure(
        "workbench.daemon_required",
        "Workbench commands require the project daemon; rerun without --direct",
    )
}

fn workbench_failure(kind: &'static str, message: &'static str) -> ExoFailure {
    ExoFailure::new(
        crate::api::protocol::ErrorCode::PreconditionFailed,
        message,
        ExoFailure::orienting_steering(vec![]),
    )
    .with_details(serde_json::json!({ "kind": kind }))
}

fn random_bytes() -> Result<[u8; 32]> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("read workbench random bytes")?;
    Ok(bytes)
}

fn sign_ticket(secret: &[u8; 32], payload: &WorkbenchTicketV1) -> Result<String> {
    let payload_bytes = serde_json::to_vec(payload)?;
    let mut signer =
        HmacSha256::new_from_slice(secret).context("initialize workbench ticket signer")?;
    signer.update(&payload_bytes);
    Ok(format!(
        "v1.{}.{}",
        URL_SAFE_NO_PAD.encode(&payload_bytes),
        URL_SAFE_NO_PAD.encode(signer.finalize().into_bytes())
    ))
}

fn random_token() -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes()?))
}

fn session_credential_digest(session_secret: &str) -> String {
    blake3::hash(session_secret.as_bytes()).to_hex().to_string()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn timestamp_for_unix_seconds(seconds: u64) -> String {
    DateTime::from_timestamp(i64::try_from(seconds).unwrap_or(i64::MAX), 0)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn write_host_record(path: &Path, record: &WorkbenchHostRecord) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("workbench host record path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let content = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".workbench.host.json.exo-tmp.")
        .tempfile_in(parent)?;
    use std::io::Write as _;
    temporary.write_all(&content)?;
    temporary
        .persist(path)
        .map(drop)
        .map_err(|error| error.error)
}

fn read_session_store(
    path: &Path,
    project_id: &str,
    now: u64,
) -> Result<HashMap<String, WorkbenchSessionGrantV1>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let store: WorkbenchSessionStoreV1 =
        serde_json::from_slice(&bytes).context("decode workbench session store")?;
    if store.schema_version != SESSION_STORE_SCHEMA_VERSION || store.project_id != project_id {
        return Ok(HashMap::new());
    }
    Ok(store
        .sessions
        .into_iter()
        .filter(|grant| {
            grant.project_id == project_id
                && grant.workspace_root.is_absolute()
                && grant.is_live(now)
                && valid_public_token(&grant.selector)
                && grant.credential_digest.len() == 64
                && grant
                    .credential_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
        .map(|grant| (grant.credential_digest.clone(), grant))
        .collect())
}

fn write_session_store(path: &Path, store: &WorkbenchSessionStoreV1) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("workbench session store path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let content = serde_json::to_vec(store)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".workbench.sessions.json.exo-tmp.")
        .tempfile_in(parent)?;
    use std::io::Write as _;
    temporary.write_all(&content)?;
    temporary.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temporary
        .persist(path)
        .map(drop)
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn valid_public_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests;
