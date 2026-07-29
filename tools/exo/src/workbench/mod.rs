mod assets;
mod http;
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
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::task::JoinHandle;

const TICKET_LIFETIME: Duration = Duration::from_mins(5);
const SESSION_ABSOLUTE_LIFETIME: Duration = Duration::from_hours(12);
const SESSION_IDLE_LIFETIME: Duration = Duration::from_mins(30);
const MAX_SESSIONS: usize = 64;
const MAX_EVENT_STREAMS: usize = 32;
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
pub(crate) const SESSION_COOKIE_PREFIX: &str = "exo_workbench_session_";

type DispatchFuture = Pin<Box<dyn Future<Output = ResponseEnvelope> + Send>>;
type DispatchFn = dyn Fn(RequestEnvelope) -> DispatchFuture + Send + Sync;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct DaemonRequestDispatcher {
    dispatch: Arc<DispatchFn>,
}

impl DaemonRequestDispatcher {
    pub fn new<F, Fut>(dispatch: F) -> Self
    where
        F: Fn(RequestEnvelope) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ResponseEnvelope> + Send + 'static,
    {
        Self {
            dispatch: Arc::new(move |request| Box::pin(dispatch(request))),
        }
    }

    pub async fn dispatch(&self, request: RequestEnvelope) -> ResponseEnvelope {
        (self.dispatch)(request).await
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
    last_activity: Arc<AtomicU64>,
    revision: AtomicU64,
    write_tx: broadcast::Sender<u64>,
    dispatcher: OnceLock<DaemonRequestDispatcher>,
    state: Mutex<WorkbenchState>,
    event_admission: Arc<Semaphore>,
}

#[derive(Default)]
struct WorkbenchState {
    host: Option<BoundHost>,
    workspaces_by_root: HashMap<PathBuf, String>,
    workspaces_by_key: HashMap<String, WorkspaceRegistration>,
    pending_capabilities: HashMap<String, PendingCapability>,
    sessions: HashMap<String, WorkbenchSession>,
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
    pub(crate) id: String,
    pub(crate) selector: String,
    pub(crate) instance_id: String,
    pub(crate) project_id: String,
    pub(crate) workspace_key: String,
    pub(crate) workspace_root: PathBuf,
    pub(crate) capabilities: Vec<String>,
    pub(crate) created_at: u64,
    pub(crate) last_activity: u64,
    pub(crate) expires_at: u64,
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
    pub workspace: WorkbenchSnapshotWorkspace,
    pub lanes: Vec<WorkbenchLaneSummary>,
    pub focused_lane: Option<WorkbenchLaneDetails>,
    pub phase: Option<WorkbenchPhase>,
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
    pub goals: Vec<WorkbenchGoal>,
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
        Self {
            inner: Arc::new(WorkbenchHostInner {
                project,
                instance_id,
                process_start_id,
                runtime,
                host_record_path: runtime_dir.join("workbench.host.json"),
                last_activity,
                revision: AtomicU64::new(0),
                write_tx,
                dispatcher: OnceLock::new(),
                state: Mutex::new(WorkbenchState::default()),
                event_admission: Arc::new(Semaphore::new(MAX_EVENT_STREAMS)),
            }),
        }
    }

    pub fn set_dispatcher(&self, dispatcher: DaemonRequestDispatcher) -> Result<()> {
        self.inner
            .dispatcher
            .set(dispatcher)
            .map_err(|_| anyhow::anyhow!("daemon request dispatcher was already installed"))
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
            capabilities: vec!["workbench.snapshot".to_string(), "lane.focus".to_string()],
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
        let workspace = self.register_workspace(workspace_root)?;
        snapshot::build(
            &self.inner.project,
            &workspace,
            self.inner.revision.load(Ordering::Acquire),
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
            let Some(host) = state.host.as_mut() else {
                return;
            };
            let _ = host.shutdown.send(true);
            host.task.take()
        };
        if let Some(mut task) = task
            && tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.inner.remove_owned_host_record();
    }

    fn register_workspace(&self, workspace_root: &Path) -> Result<WorkspaceRegistration> {
        let root = self.inner.validate_workspace(workspace_root)?;
        let git = snapshot::sample_git(&root);
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let key = state
            .workspaces_by_root
            .get(&root)
            .cloned()
            .unwrap_or(random_token()?);
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
            branch: git.branch,
            head: git.head,
        };
        state.workspaces_by_root.insert(root, key.clone());
        state.workspaces_by_key.insert(key, workspace.clone());
        drop(state);
        Ok(workspace)
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

        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
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
        state.sessions.retain(|_, session| session.is_live(now));
        if state.sessions.len() >= MAX_SESSIONS {
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
        let session_id = random_token().map_err(|_| TicketExchangeError::Invalid)?;
        let session_key = random_token().map_err(|_| TicketExchangeError::Invalid)?;
        let session_expires_at = now.saturating_add(SESSION_ABSOLUTE_LIFETIME.as_secs());
        state.sessions.insert(
            session_id.clone(),
            WorkbenchSession {
                id: session_id.clone(),
                selector: session_key.clone(),
                instance_id: payload.instance_id,
                project_id: payload.project_id.clone(),
                workspace_key: payload.workspace_key.clone(),
                workspace_root: workspace.root,
                capabilities: payload.capabilities,
                created_at: now,
                last_activity: now,
                expires_at: session_expires_at,
            },
        );
        let result = (
            session_id,
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
        Ok(result)
    }

    pub(crate) fn session(&self, session_key: &str, session_id: &str) -> Option<WorkbenchSession> {
        let now = unix_seconds();
        let mut state = self.state.lock().ok()?;
        state.sessions.retain(|_, session| session.is_live(now));
        let session_is_bound = state.sessions.get(session_id).is_some_and(|session| {
            session.selector == session_key
                && session.instance_id == self.instance_id.as_ref()
                && session.project_id == self.project.id.as_str()
                && state
                    .workspaces_by_key
                    .get(&session.workspace_key)
                    .is_some_and(|workspace| workspace.root == session.workspace_root)
        });
        if !session_is_bound {
            state.sessions.remove(session_id);
            return None;
        }
        let session = state.sessions.get_mut(session_id)?;
        session.last_activity = now;
        let session = session.clone();
        drop(state);
        Some(session)
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

    fn remove_owned_host_record(&self) {
        let owned = std::fs::read(&self.host_record_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<WorkbenchHostRecord>(&bytes).ok())
            .is_some_and(|record| {
                record.instance_id == self.instance_id.as_ref()
                    && record.pid == std::process::id()
                    && record.process_start_id == self.process_start_id.as_ref()
            });
        if owned {
            let _ = std::fs::remove_file(&self.host_record_path);
        }
    }
}

impl WorkbenchSession {
    const fn is_live(&self, now: u64) -> bool {
        self.created_at
            .saturating_add(SESSION_ABSOLUTE_LIFETIME.as_secs())
            > now
            && self.expires_at > now
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

#[cfg(test)]
mod tests;
