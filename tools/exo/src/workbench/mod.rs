mod assets;
mod http;
pub(crate) mod planning;
#[cfg(unix)]
mod publication;
#[cfg(not(unix))]
#[path = "publication_unavailable.rs"]
mod publication;
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
#[cfg(target_os = "macos")]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, broadcast, oneshot, watch};
use tokio::task::JoinHandle;

const TICKET_LIFETIME: Duration = Duration::from_hours(1);
const SESSION_RENEWAL_LIFETIME: Duration = Duration::from_hours(12);
const SESSION_IDLE_LIFETIME: Duration = Duration::from_mins(30);
const SESSION_PERSIST_INTERVAL: Duration = Duration::from_mins(5);
const AUTHORIZATION_STORE_SCHEMA_VERSION: u8 = 2;
const WORKSPACE_STORE_SCHEMA_VERSION: u8 = 1;
const WORKSPACE_OBSERVATION_FRESH_LIFETIME: Duration = Duration::from_mins(5);
const WORKSPACE_STORE_PERSIST_INTERVAL: Duration = Duration::from_mins(1);
const PAIRING_IDLE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const PAIRING_ABSOLUTE_LIFETIME: Duration = Duration::from_secs(180 * 24 * 60 * 60);
const RESUME_OUTCOME_LIFETIME: Duration = Duration::from_hours(24);
const TERMINAL_RESUME_OUTCOME_LIFETIME: Duration = Duration::from_mins(5);
const MAX_SESSIONS: usize = 64;
const MAX_ACTIVE_PAIRINGS: usize = 64;
const MAX_ACTIVE_PAIRINGS_PER_WORKSPACE: usize = 8;
const MAX_RETAINED_REVOKED_PAIRINGS: usize = 64;
const MAX_RETAINED_REVOKED_PAIRINGS_PER_WORKSPACE: usize = 8;
const MAX_RESUME_OUTCOMES: usize = 256;
const MAX_RESUME_OUTCOMES_PER_PAIRING: usize = 32;
const MAX_PAIRING_NICKNAME_CHARS: usize = 80;
const PAIRING_SELECTOR_DISPLAY_CHARS: usize = 12;
const MAX_PROJECT_WORKSPACES: usize = 128;
const MAX_EVENT_STREAMS: usize = 32;
const PUBLICATION_RESTORE_INITIAL_RETRY: Duration = Duration::from_millis(250);
const PUBLICATION_RESTORE_MAX_RETRY: Duration = Duration::from_secs(30);
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
pub(crate) const SESSION_COOKIE_PREFIX: &str = "exo_workbench_session_";
pub(crate) const PAIRING_COOKIE_NAME: &str = "exo_workbench_pairing";

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

    pub fn inspect(&self, workspace_root: &Path, lane_id: &str) -> Result<WorkbenchLaneInspection> {
        self.host.inspect(workspace_root, lane_id)
    }

    pub fn pairings(&self) -> Result<WorkbenchPairingListResult> {
        self.host.inner.list_pairings(None, None, false)
    }

    pub fn revoke_pairing(&self, selector: &str) -> Result<WorkbenchPairingMutationResult> {
        self.host
            .inner
            .revoke_pairing(selector, None)
            .map_err(pairing_management_anyhow)
    }

    pub fn forget_pairing(&self, selector: &str) -> Result<WorkbenchPairingMutationResult> {
        self.host
            .inner
            .forget_pairing(selector, None)
            .map_err(pairing_management_anyhow)
    }

    pub fn rename_pairing(
        &self,
        selector: &str,
        nickname: &str,
    ) -> Result<WorkbenchPairingMutationResult> {
        self.host
            .inner
            .rename_pairing(selector, nickname, None)
            .map_err(pairing_management_anyhow)
    }

    pub fn observe_workspace(&self, workspace_root: &Path) -> Result<()> {
        self.host.observe_workspace(workspace_root)
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
    authorization_store_path: PathBuf,
    workspace_store_path: PathBuf,
    authorization_store_gate: Mutex<()>,
    workspace_store_gate: Mutex<()>,
    last_activity: Arc<AtomicU64>,
    revision: AtomicU64,
    project_state_gate: Mutex<()>,
    write_tx: broadcast::Sender<u64>,
    dispatcher: OnceLock<DaemonRequestDispatcher>,
    entry_provider: Mutex<Arc<dyn WorkbenchEntryProvider>>,
    host_launch_gate: Mutex<()>,
    host_generation: AtomicU64,
    publication_restore_shutdown: watch::Sender<bool>,
    publication_restore_task: Mutex<Option<JoinHandle<()>>>,
    shutting_down: AtomicBool,
    state: Mutex<WorkbenchState>,
    event_admission: Arc<Semaphore>,
    completion_review_admission: Arc<Semaphore>,
}

#[derive(Default)]
struct WorkbenchState {
    host: Option<BoundHost>,
    retiring_hosts: Vec<BoundHost>,
    origin_bindings: HashMap<String, WorkbenchEntryBinding>,
    released_publication_workspaces: HashSet<String>,
    preferred_port: Option<u16>,
    workspaces_by_root: HashMap<PathBuf, String>,
    workspaces_by_key: HashMap<String, WorkspaceRegistration>,
    pending_capabilities: HashMap<String, PendingCapability>,
    session_grants: HashMap<String, WorkbenchSessionGrantV1>,
    sessions: HashMap<String, WorkbenchSession>,
    pairing_grants: HashMap<String, WorkbenchPairingGrantV1>,
    resume_outcomes: HashMap<WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1>,
    completion_review_requests:
        HashMap<planning::CompletionReviewRequestKey, planning::CompletionReviewRequestRecord>,
    completion_reviews: HashMap<String, planning::CompletionReviewRecord>,
    completion_review_sequence: u64,
    workspace_store_dirty: bool,
    workspace_store_persisted_at: u64,
}

struct BoundHost {
    generation: u64,
    origin: String,
    expected_host: String,
    publication_listener: Option<TcpListener>,
    secret: [u8; 32],
    assets_hash: String,
    started_at: String,
    server_task_alive: bool,
    updated_at: String,
    last_error: Option<String>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

trait WorkbenchEntryProvider: Send + Sync {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding>;

    fn release_workspace(&self, _workspace_key: &str) {}

    fn rebind_all(&self, _listener: &TcpListener, _listener_generation: u64) -> Result<()> {
        Ok(())
    }

    fn all_on_listener_generation(&self, _listener_generation: u64) -> bool {
        true
    }

    fn shutdown(&self) {}
}

enum HostLaunchPlan {
    Existing {
        generation: u64,
        origin: String,
        listener: TcpListener,
        secret: [u8; 32],
    },
    Candidate(PendingHost),
}

struct PendingHost {
    generation: u64,
    origin: String,
    expected_host: String,
    listener: Option<TcpListener>,
    secret: [u8; 32],
}

impl HostLaunchPlan {
    const fn generation(&self) -> u64 {
        match self {
            Self::Existing { generation, .. } => *generation,
            Self::Candidate(host) => host.generation,
        }
    }

    #[allow(
        clippy::missing_const_for_fn,
        reason = "String-to-str coercion is not const on the supported Rust toolchain"
    )]
    fn origin(&self) -> &str {
        match self {
            Self::Existing { origin, .. } => origin,
            Self::Candidate(host) => &host.origin,
        }
    }

    const fn secret(&self) -> [u8; 32] {
        match self {
            Self::Existing { secret, .. } => *secret,
            Self::Candidate(host) => host.secret,
        }
    }

    const fn reused(&self) -> bool {
        matches!(self, Self::Existing { .. })
    }

    fn listener(&self) -> Result<&TcpListener> {
        match self {
            Self::Existing { listener, .. } => Ok(listener),
            Self::Candidate(host) => host
                .listener
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pending workbench listener is unavailable")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkbenchLaunchMode {
    #[default]
    DirectLoopback,
    Published,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchEntryBinding {
    launch_mode: WorkbenchLaunchMode,
    canonical_origin: String,
    project_instance_id: Option<String>,
    workspace_key: Option<String>,
}

impl WorkbenchEntryBinding {
    fn direct(canonical_origin: String) -> Self {
        Self {
            launch_mode: WorkbenchLaunchMode::DirectLoopback,
            canonical_origin,
            project_instance_id: None,
            workspace_key: None,
        }
    }

    fn published(
        canonical_origin: String,
        project_instance_id: String,
        workspace_key: String,
    ) -> Result<Self> {
        if !canonical_origin.starts_with("https://")
            || expected_host_from_origin(&canonical_origin).is_none()
            || project_instance_id.is_empty()
            || workspace_key.is_empty()
        {
            return Err(anyhow::anyhow!("invalid published workbench entry binding"));
        }
        Ok(Self {
            launch_mode: WorkbenchLaunchMode::Published,
            canonical_origin,
            project_instance_id: Some(project_instance_id),
            workspace_key: Some(workspace_key),
        })
    }

    fn expected_host(&self) -> Option<&str> {
        expected_host_from_origin(&self.canonical_origin)
    }

    const fn is_published(&self) -> bool {
        matches!(self.launch_mode, WorkbenchLaunchMode::Published)
    }

    fn conflicts_with_transition(
        &self,
        workspace_key: &str,
        replacement: &WorkbenchEntryBinding,
    ) -> bool {
        self.workspace_key.as_deref() == Some(workspace_key)
            || self.canonical_origin == replacement.canonical_origin
            || replacement
                .project_instance_id
                .as_ref()
                .is_some_and(|project_instance_id| {
                    self.project_instance_id.as_ref() == Some(project_instance_id)
                })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRegistration {
    pub(crate) key: String,
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    pub(crate) branch: Option<String>,
    pub(crate) head: Option<String>,
    pub(crate) dirty: Option<bool>,
    pub(crate) observed_at: Option<u64>,
    pub(crate) registered_at: u64,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceProjection {
    pub(super) registration: WorkspaceRegistration,
    pub(super) availability: &'static str,
    pub(super) current: bool,
}

#[derive(Debug, Clone)]
struct PendingCapability {
    workspace_key: String,
    entry: WorkbenchEntryBinding,
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
    entry: WorkbenchEntryBinding,
    pairing_selector: Option<String>,
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
    #[serde(default)]
    entry: Option<WorkbenchEntryBinding>,
    #[serde(default)]
    pairing_selector: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchPairingGrantV1 {
    selector: String,
    credential_digest: String,
    project_id: String,
    workspace_key: String,
    workspace_root: PathBuf,
    launch_mode: WorkbenchLaunchMode,
    project_instance_id: String,
    canonical_origin: String,
    capabilities: Vec<String>,
    created_at: u64,
    last_used_at: u64,
    idle_expires_at: u64,
    absolute_expires_at: u64,
    nickname: Option<String>,
    #[serde(default)]
    revoked_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revocation_cause: Option<WorkbenchPairingRevocationCause>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkbenchPairingRevocationCause {
    Explicit,
    Replaced,
    WorkspaceMissing,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct WorkbenchResumeOutcomeKey {
    pairing_selector: String,
    request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchResumeOutcomeV1 {
    pairing_selector: String,
    request_id: String,
    created_at: u64,
    retained_until: u64,
    #[serde(flatten)]
    result: WorkbenchResumeOutcomeResultV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum WorkbenchResumeOutcomeResultV1 {
    Session {
        session_selector: String,
        session_credential_digest: String,
        session_expires_at: u64,
    },
    Terminal {
        terminal_error: WorkbenchResumeTerminalErrorV1,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkbenchResumeTerminalErrorV1 {
    Invalid,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchAuthorizationStoreV2 {
    schema_version: u8,
    project_id: String,
    sessions: Vec<WorkbenchSessionGrantV1>,
    pairings: Vec<WorkbenchPairingGrantV1>,
    resume_outcomes: Vec<WorkbenchResumeOutcomeV1>,
}

#[derive(Default)]
struct RestoredAuthorizationState {
    sessions: HashMap<String, WorkbenchSessionGrantV1>,
    pairings: HashMap<String, WorkbenchPairingGrantV1>,
    resume_outcomes: HashMap<WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1>,
}

struct WorkbenchAuthorizationRollback {
    session_grants: HashMap<String, WorkbenchSessionGrantV1>,
    sessions: HashMap<String, WorkbenchSession>,
    pairing_grants: HashMap<String, WorkbenchPairingGrantV1>,
    resume_outcomes: HashMap<WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchWorkspaceStoreEntryV1 {
    key: String,
    root: PathBuf,
    label: String,
    branch: Option<String>,
    head: Option<String>,
    dirty: Option<bool>,
    observed_at: Option<u64>,
    registered_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkbenchWorkspaceStoreV1 {
    schema_version: u8,
    project_id: String,
    workspaces: Vec<WorkbenchWorkspaceStoreEntryV1>,
}

impl From<WorkspaceRegistration> for WorkbenchWorkspaceStoreEntryV1 {
    fn from(workspace: WorkspaceRegistration) -> Self {
        Self {
            key: workspace.key,
            root: workspace.root,
            label: workspace.label,
            branch: workspace.branch,
            head: workspace.head,
            dirty: workspace.dirty,
            observed_at: workspace.observed_at,
            registered_at: workspace.registered_at,
        }
    }
}

impl From<WorkbenchWorkspaceStoreEntryV1> for WorkspaceRegistration {
    fn from(workspace: WorkbenchWorkspaceStoreEntryV1) -> Self {
        Self {
            key: workspace.key,
            root: workspace.root,
            label: workspace.label,
            branch: workspace.branch,
            head: workspace.head,
            dirty: workspace.dirty,
            observed_at: workspace.observed_at,
            registered_at: workspace.registered_at,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkbenchTicketV2 {
    version: u8,
    capability_id: String,
    instance_id: String,
    project_id: String,
    workspace_key: String,
    entry_mode: WorkbenchLaunchMode,
    project_instance_id: String,
    canonical_origin: String,
    capabilities: Vec<String>,
    issued_at: u64,
    expires_at: u64,
}

#[derive(Debug, Clone)]
struct VerifiedWorkbenchTicket {
    capability_id: String,
    project_id: String,
    workspace_key: String,
    entry: WorkbenchEntryBinding,
    capabilities: Vec<String>,
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchLaunchResult {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub launch_mode: WorkbenchLaunchMode,
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
    pub project_workspaces: Vec<WorkbenchProjectWorkspaceSummary>,
    pub lanes: Vec<WorkbenchLaneSummary>,
    pub focused_lane: Option<WorkbenchLaneDetails>,
    pub phase: Option<WorkbenchPhase>,
    pub between_phases_context: Option<WorkbenchBetweenPhasesContext>,
    pub steering: WorkbenchSteering,
    pub diagnostics: Vec<WorkbenchDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkbenchLaneInspection {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub observed_at: String,
    pub revision: u64,
    pub project: WorkbenchProjectIdentity,
    pub daemon: WorkbenchDaemonIdentity,
    pub workspace: WorkbenchSnapshotWorkspace,
    pub relationship: String,
    pub can_focus_here: bool,
    pub lane: WorkbenchLaneDetails,
    pub phase: WorkbenchPhase,
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
pub struct WorkbenchProjectWorkspaceSummary {
    pub key: String,
    pub label: String,
    pub current: bool,
    pub availability: String,
    pub observed_at: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub dirty: Option<bool>,
    pub focused_lane: Option<WorkbenchWorkspaceLaneSummary>,
    pub active_phase: Option<WorkbenchWorkspacePhaseSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchWorkspaceLaneSummary {
    pub id: String,
    pub title: String,
    pub state: String,
    pub phase_id: String,
    pub phase_title: String,
    pub phase_status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchWorkspacePhaseSummary {
    pub id: String,
    pub title: String,
    pub status: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub outcome_truncated: bool,
    pub tasks: Vec<WorkbenchTask>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchTask {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub outcome_truncated: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairingExchangeError {
    Invalid,
    Expired,
    Limit,
    Busy,
    Unavailable,
}

#[derive(Debug, Clone)]
pub(crate) struct PairingEnrollmentResult {
    pub(crate) pairing_cookie: String,
    pub(crate) pairing_max_age: u64,
    pub(crate) session_secret: String,
    pub(crate) session: WorkbenchSessionResult,
}

#[derive(Debug, Clone)]
pub(crate) struct PairingResumeResult {
    pub(crate) pairing_cookie: String,
    pub(crate) pairing_max_age: u64,
    pub(crate) session_secret: String,
    pub(crate) session: WorkbenchSessionResult,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkbenchPairingStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchPairingSummary {
    pub selector: String,
    pub workspace_label: String,
    pub created_at: String,
    pub last_used_at: String,
    pub expires_at: String,
    pub nickname: Option<String>,
    pub status: WorkbenchPairingStatus,
    pub revoked_at: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchPairingListResult {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub pairings: Vec<WorkbenchPairingSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkbenchPairingMutationResult {
    pub kind: &'static str,
    pub ok: bool,
    pub schema_version: u8,
    pub selector: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairingManagementError {
    Invalid,
    NotFound,
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
        let authorization_store_path = runtime_dir.join("workbench.authorizations.json");
        let legacy_session_store_path = runtime_dir.join("workbench.sessions.json");
        let workspace_store_path = runtime_dir.join("workbench.workspaces.json");
        let (publication_restore_shutdown, _) = watch::channel(false);
        let now = unix_seconds();
        let mut state = WorkbenchState::default();
        match load_authorization_state(
            &authorization_store_path,
            &legacy_session_store_path,
            project.id.as_str(),
            now,
        ) {
            Ok(restored) => {
                state.session_grants = restored.sessions;
                state.pairing_grants = restored.pairings;
                state.resume_outcomes = restored.resume_outcomes;
            }
            Err(error) => {
                eprintln!(
                    "exo daemon: failed to read workbench authorization store at {}: {error}",
                    authorization_store_path.display()
                );
            }
        }
        match read_workspace_store(&workspace_store_path, project.id.as_str()) {
            Ok(workspaces) => {
                for workspace in workspaces {
                    state
                        .workspaces_by_root
                        .insert(workspace.root.clone(), workspace.key.clone());
                    state
                        .workspaces_by_key
                        .insert(workspace.key.clone(), workspace);
                }
            }
            Err(error) => {
                eprintln!(
                    "exo daemon: failed to read workbench workspace store at {}: {error}",
                    workspace_store_path.display()
                );
            }
        }
        state.workspace_store_persisted_at = now;
        if !state.session_grants.is_empty()
            || state
                .pairing_grants
                .values()
                .any(|pairing| pairing.is_live(now))
        {
            state.preferred_port = resumable_host_port(&host_record_path);
        }
        Self {
            inner: Arc::new(WorkbenchHostInner {
                project,
                instance_id,
                process_start_id,
                runtime,
                host_record_path,
                authorization_store_path,
                workspace_store_path,
                authorization_store_gate: Mutex::new(()),
                workspace_store_gate: Mutex::new(()),
                last_activity,
                revision: AtomicU64::new(0),
                project_state_gate: Mutex::new(()),
                write_tx,
                dispatcher: OnceLock::new(),
                entry_provider: Mutex::new(Arc::new(
                    publication::LocaldWorkbenchEntryProvider::production(),
                )),
                host_launch_gate: Mutex::new(()),
                host_generation: AtomicU64::new(0),
                publication_restore_shutdown,
                publication_restore_task: Mutex::new(None),
                shutting_down: AtomicBool::new(false),
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
        let resume_host = self.inner.state.lock().is_ok_and(|state| {
            assets::available()
                && (state.preferred_port.is_some()
                    || state
                        .pairing_grants
                        .values()
                        .any(|pairing| pairing.is_live(unix_seconds())))
        });
        if resume_host {
            self.start_publication_restore_task()?;
        }
        Ok(())
    }

    fn start_publication_restore_task(&self) -> Result<()> {
        let mut restore_task = self
            .inner
            .publication_restore_task
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication restoration is unavailable"))?;
        if restore_task
            .as_ref()
            .is_some_and(|restore_task| !restore_task.is_finished())
        {
            return Ok(());
        }

        let weak = Arc::downgrade(&self.inner);
        let mut shutdown = self.inner.publication_restore_shutdown.subscribe();
        let task = self.inner.runtime.spawn(async move {
            let mut retry_delay = PUBLICATION_RESTORE_INITIAL_RETRY;
            loop {
                if *shutdown.borrow() {
                    return;
                }
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let manager = WorkbenchHostManager { inner };
                let runtime = manager.inner.runtime.clone();
                let (restored_tx, restored_rx) = oneshot::channel();
                let restored = match thread::Builder::new()
                    .name("exo-workbench-publication-restore".to_string())
                    .spawn(move || {
                        let _runtime_guard = runtime.enter();
                        let _ = restored_tx.send(manager.restore_prior_host());
                    })
                {
                    Ok(worker) => {
                        drop(worker);
                        tokio::select! {
                            _ = shutdown.changed() => return,
                            restored = restored_rx => restored
                                .map_err(|_| {
                                    anyhow::anyhow!(
                                        "workbench publication restoration worker stopped unexpectedly"
                                    )
                                })
                                .and_then(|restored| restored),
                        }
                    }
                    Err(error) => Err(anyhow::Error::new(error).context(
                        "start workbench publication restoration worker",
                    )),
                };
                match restored {
                    Ok(()) => return,
                    Err(error) => {
                        eprintln!(
                            "exo daemon: failed to resume every prior workbench origin; retrying: {error:#}"
                        );
                    }
                }

                tokio::select! {
                    _ = shutdown.changed() => return,
                    _ = tokio::time::sleep(retry_delay) => {}
                }
                retry_delay = retry_delay
                    .saturating_mul(2)
                    .min(PUBLICATION_RESTORE_MAX_RETRY);
            }
        });
        *restore_task = Some(task);
        Ok(())
    }

    fn restore_prior_host(&self) -> Result<()> {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            anyhow::bail!("workbench publication restoration is shutting down");
        }
        let publications = self.retained_publications()?;
        if publications.is_empty() {
            self.ensure_host()?;
            return Ok(());
        }

        let _launch_gate = self
            .inner
            .host_launch_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench launch coordination is unavailable"))?;
        let provider = self
            .inner
            .entry_provider
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench entry provider is unavailable"))?
            .clone();
        let mut host_plan = self.prepare_host()?;
        let direct_origin = host_plan.origin().to_string();
        let listener_generation = host_plan.generation();
        let publication_listener = host_plan
            .listener()?
            .try_clone()
            .context("retain restored workbench publication listener")?;

        let mut first_failure = None;
        let mut failure_count = 0usize;
        for (workspace, expected) in publications {
            let previous_bindings = {
                let state = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
                state
                    .origin_bindings
                    .iter()
                    .filter(|(_, binding)| {
                        binding.conflicts_with_transition(&workspace.key, &expected)
                    })
                    .map(|(origin, binding)| (origin.clone(), binding.clone()))
                    .collect::<Vec<_>>()
            };
            let mut authorized = false;
            let mut authorize = |entry: &WorkbenchEntryBinding| {
                let _authorization_gate =
                    self.inner.authorization_store_gate.lock().map_err(|_| {
                        anyhow::anyhow!("workbench authorization store is unavailable")
                    })?;
                if self.inner.shutting_down.load(Ordering::Acquire) {
                    anyhow::bail!("workbench publication restoration is shutting down");
                }
                if entry != &expected {
                    return Err(anyhow::Error::new(workbench_failure(
                        "workbench.publisher_binding_changed",
                        "The retained workbench pairing no longer matches the published service",
                    )));
                }
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
                if !workspace_has_live_published_binding(
                    &state,
                    &workspace.key,
                    &workspace.root,
                    &expected,
                    unix_seconds(),
                ) {
                    return Err(anyhow::Error::new(workbench_failure(
                        "workbench.publisher_authority_expired",
                        "The retained workbench pairing is no longer live",
                    )));
                }
                state
                    .origin_bindings
                    .retain(|_, binding| !binding.conflicts_with_transition(&workspace.key, entry));
                state
                    .origin_bindings
                    .insert(entry.canonical_origin.clone(), entry.clone());
                authorized = true;
                Ok(())
            };
            let rebind_provider = Arc::clone(&provider);
            let mut ensure_started = || {
                if self.inner.shutting_down.load(Ordering::Acquire) {
                    anyhow::bail!("workbench publication restoration is shutting down");
                }
                if self.start_host_plan(&mut host_plan)? {
                    rebind_provider.rebind_all(&publication_listener, listener_generation)?;
                    self.retire_replaced_hosts();
                }
                Ok(())
            };
            let resolved = provider.resolve(
                &workspace,
                &direct_origin,
                &publication_listener,
                listener_generation,
                &mut authorize,
                &mut ensure_started,
            );
            let authority_live = self.inner.state.lock().is_ok_and(|state| {
                workspace_has_live_published_binding(
                    &state,
                    &workspace.key,
                    &workspace.root,
                    &expected,
                    unix_seconds(),
                )
            });
            let restore_failed = match resolved.as_ref() {
                Ok(entry) => {
                    entry != &expected
                        || !authority_live
                        || self.inner.shutting_down.load(Ordering::Acquire)
                }
                Err(_) => true,
            };
            if restore_failed {
                provider.release_workspace(&workspace.key);
                if authorized && let Ok(mut state) = self.inner.state.lock() {
                    state.origin_bindings.retain(|_, binding| {
                        !binding.conflicts_with_transition(&workspace.key, &expected)
                    });
                    state.origin_bindings.extend(previous_bindings);
                    state
                        .released_publication_workspaces
                        .insert(workspace.key.clone());
                }
                let error = match resolved {
                    Ok(_) if self.inner.shutting_down.load(Ordering::Acquire) => {
                        anyhow::anyhow!("workbench publication restoration is shutting down")
                    }
                    Ok(_) => anyhow::Error::new(workbench_failure(
                        "workbench.publisher_binding_changed",
                        "The retained workbench pairing no longer authorizes the published service",
                    )),
                    Err(error) => error,
                };
                failure_count += 1;
                if first_failure.is_none() {
                    first_failure = Some((workspace.key.clone(), error));
                }
            } else if let Ok(mut state) = self.inner.state.lock() {
                state.released_publication_workspaces.remove(&workspace.key);
            }
        }
        if provider.all_on_listener_generation(listener_generation) {
            self.retire_replaced_hosts();
        }
        match first_failure {
            None => Ok(()),
            Some((workspace_key, error)) => Err(error.context(format!(
                "failed to restore {failure_count} retained workbench publication(s); first workspace {workspace_key}"
            ))),
        }
    }

    fn retained_publications(&self) -> Result<Vec<(WorkspaceRegistration, WorkbenchEntryBinding)>> {
        let now = unix_seconds();
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let mut publications =
            HashMap::<String, (WorkspaceRegistration, WorkbenchEntryBinding)>::new();
        for pairing in state
            .pairing_grants
            .values()
            .filter(|pairing| pairing.is_live(now))
        {
            let Some(workspace) = state.workspaces_by_key.get(&pairing.workspace_key) else {
                continue;
            };
            if workspace.root != pairing.workspace_root {
                continue;
            }
            let entry = WorkbenchEntryBinding::published(
                pairing.canonical_origin.clone(),
                pairing.project_instance_id.clone(),
                pairing.workspace_key.clone(),
            )?;
            if let Some((_, previous)) = publications.get(&workspace.key)
                && previous != &entry
            {
                return Err(anyhow::Error::new(workbench_failure(
                    "workbench.publisher_binding_changed",
                    "Retained workbench pairings disagree about the published service binding",
                )));
            }
            publications.insert(workspace.key.clone(), (workspace.clone(), entry));
        }
        drop(state);

        let mut publications = publications.into_values().collect::<Vec<_>>();
        publications.retain(|(workspace, _)| {
            self.inner
                .validate_workspace(&workspace.root)
                .is_ok_and(|root| root == workspace.root)
        });
        publications.sort_by(|left, right| left.0.key.cmp(&right.0.key));
        Ok(publications)
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "published-entry fixtures compile only when the UI test feature is enabled"
    )]
    fn set_entry_provider(&self, provider: Arc<dyn WorkbenchEntryProvider>) {
        *self
            .inner
            .entry_provider
            .lock()
            .expect("workbench entry provider") = provider;
    }

    pub fn launch(&self, workspace_root: &Path) -> Result<WorkbenchLaunchResult> {
        if !assets::available() {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.ui_unavailable",
                "This Exo binary was built without the embedded workbench UI",
            )));
        }

        let workspace = self.register_workspace(workspace_root)?;
        let _launch_gate = self
            .inner
            .host_launch_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench launch coordination is unavailable"))?;
        if self.inner.shutting_down.load(Ordering::Acquire) {
            anyhow::bail!("workbench host is shutting down");
        }
        let _authorization_gate = self
            .inner
            .authorization_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench authorization store is unavailable"))?;
        let entry_provider = self
            .inner
            .entry_provider
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench entry provider is unavailable"))?
            .clone();
        let mut host_plan = self.prepare_host()?;
        let direct_origin = host_plan.origin().to_string();
        let listener_generation = host_plan.generation();
        let reused_host = host_plan.reused();
        let secret = host_plan.secret();
        let publication_listener = host_plan
            .listener()?
            .try_clone()
            .context("retain workbench publication listener")?;
        let mut entry_transition = None;
        let rebind_provider = Arc::clone(&entry_provider);
        let mut authorize = |entry: &WorkbenchEntryBinding| {
            if entry_transition.is_none() {
                let state = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
                let previous_bindings = state
                    .origin_bindings
                    .iter()
                    .filter(|(_, binding)| binding.conflicts_with_transition(&workspace.key, entry))
                    .map(|(origin, binding)| (origin.clone(), binding.clone()))
                    .collect::<Vec<_>>();
                entry_transition = Some((entry.clone(), previous_bindings, None));
            }
            let rollback = self
                .inner
                .reconcile_published_workspace_move_locked(&workspace, entry)?;
            if let Some((_, _, transition_rollback)) = entry_transition.as_mut()
                && transition_rollback.is_none()
            {
                *transition_rollback = rollback;
            }
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            state
                .origin_bindings
                .retain(|_, existing| !existing.conflicts_with_transition(&workspace.key, entry));
            state
                .origin_bindings
                .insert(entry.canonical_origin.clone(), entry.clone());
            drop(state);
            Ok(())
        };
        let mut ensure_started = || {
            if self.start_host_plan(&mut host_plan)? {
                rebind_provider.rebind_all(&publication_listener, listener_generation)?;
                self.retire_replaced_hosts();
            }
            Ok(())
        };
        let resolved = entry_provider.resolve(
            &workspace,
            &direct_origin,
            &publication_listener,
            listener_generation,
            &mut authorize,
            &mut ensure_started,
        );
        let resolved = resolved.and_then(|entry| {
            if self.inner.shutting_down.load(Ordering::Acquire) {
                entry_provider.release_workspace(&workspace.key);
                anyhow::bail!("workbench host is shutting down");
            }
            Ok(entry)
        });
        let entry = match resolved {
            Ok(entry) => entry,
            Err(error) => {
                if let Some((replacement, previous_entry_bindings, authorization_rollback)) =
                    entry_transition
                {
                    if let Some(authorization_rollback) = authorization_rollback
                        && let Err(rollback_error) = self
                            .inner
                            .restore_published_workspace_move_locked(authorization_rollback)
                    {
                        return Err(error.context(format!(
                            "failed to restore prior workbench pairing authority: {rollback_error:#}"
                        )));
                    }
                    if let Ok(mut state) = self.inner.state.lock() {
                        state.origin_bindings.retain(|_, existing| {
                            !existing.conflicts_with_transition(&workspace.key, &replacement)
                        });
                        state.origin_bindings.extend(previous_entry_bindings);
                    }
                }
                return Err(error);
            }
        };
        if entry.is_published()
            && let Ok(mut state) = self.inner.state.lock()
        {
            state.released_publication_workspaces.remove(&workspace.key);
        }
        if entry_provider.all_on_listener_generation(listener_generation) {
            self.retire_replaced_hosts();
        }
        if !entry.is_published() {
            let removed_workspace_keys = self.reconcile_direct_workspace_move_locked(&workspace)?;
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            state.origin_bindings.retain(|_, existing| {
                existing.workspace_key.as_deref() != Some(workspace.key.as_str())
            });
            drop(state);
            self.inner
                .release_workspace_publications(&removed_workspace_keys);
        }
        let issued_at = unix_seconds();
        let expires_at = issued_at.saturating_add(TICKET_LIFETIME.as_secs());
        let capability_id = random_token()?;
        let capabilities = std::iter::once("workbench.snapshot")
            .chain(std::iter::once("workbench.inspect"))
            .chain(std::iter::once("lane.focus"))
            .chain(entry.is_published().then_some("workbench.pairing.manage"))
            .chain(planning::PLANNING_CAPABILITIES)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let ticket = if entry.is_published() {
            let project_instance_id = entry.project_instance_id.clone().ok_or_else(|| {
                anyhow::anyhow!("published workbench entry has no project instance")
            })?;
            sign_ticket(
                &secret,
                &WorkbenchTicketV2 {
                    version: 2,
                    capability_id: capability_id.clone(),
                    instance_id: self.inner.instance_id.to_string(),
                    project_id: self.inner.project.id.to_string(),
                    workspace_key: workspace.key.clone(),
                    entry_mode: entry.launch_mode,
                    project_instance_id,
                    canonical_origin: entry.canonical_origin.clone(),
                    capabilities,
                    issued_at,
                    expires_at,
                },
            )?
        } else {
            sign_ticket(
                &secret,
                &WorkbenchTicketV1 {
                    version: 1,
                    capability_id: capability_id.clone(),
                    instance_id: self.inner.instance_id.to_string(),
                    project_id: self.inner.project.id.to_string(),
                    workspace_key: workspace.key.clone(),
                    capabilities,
                    issued_at,
                    expires_at,
                },
            )?
        };

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
                entry: entry.clone(),
                expires_at,
            },
        );
        drop(state);

        Ok(WorkbenchLaunchResult {
            kind: "workbench.launch",
            ok: true,
            schema_version: 2,
            launch_mode: entry.launch_mode,
            url: format!("{}/#ticket={ticket}", entry.canonical_origin),
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

    pub(crate) fn requires_daemon_residency(&self, now: u64) -> bool {
        self.requires_daemon_residency_with_assets(now, assets::available())
    }

    fn requires_daemon_residency_with_assets(&self, now: u64, assets_available: bool) -> bool {
        if !assets_available {
            return false;
        }
        let Ok(_launch_gate) = self.inner.host_launch_gate.lock() else {
            return false;
        };
        let Ok(mut state) = self.inner.state.lock() else {
            return false;
        };
        state
            .pending_capabilities
            .retain(|_, pending| pending.expires_at > now);
        let inactive_publications = state
            .origin_bindings
            .values()
            .filter(|binding| binding.is_published())
            .filter_map(|binding| binding.workspace_key.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|workspace_key| {
                !workspace_has_live_published_authority(&state, workspace_key, now)
                    && !state
                        .released_publication_workspaces
                        .contains(workspace_key)
            })
            .collect::<Vec<_>>();
        state
            .released_publication_workspaces
            .extend(inactive_publications.iter().cloned());
        let resident = !state.pending_capabilities.is_empty()
            || state
                .pairing_grants
                .values()
                .any(|pairing| pairing.is_live(now));
        drop(state);
        self.inner
            .release_workspace_publications(&inactive_publications);
        resident
    }

    pub fn snapshot(&self, workspace_root: &Path) -> Result<WorkbenchSnapshot> {
        self.snapshot_with_before_state_gate(workspace_root, || {})
    }

    pub fn inspect(&self, workspace_root: &Path, lane_id: &str) -> Result<WorkbenchLaneInspection> {
        let (workspace, git) = self.register_workspace_with_git(workspace_root)?;
        let _project_state_guard = self.inner.project_state_gate.lock().map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.inspection_unavailable",
                "The lane inspection is temporarily unavailable",
            ))
        })?;
        snapshot::inspect_with_git(
            &self.inner.project,
            &workspace,
            self.inner.revision.load(Ordering::Acquire),
            &self.inner.instance_id,
            lane_id,
            git,
        )
        .map_err(|error| {
            if error.downcast_ref::<ExoFailure>().is_some() {
                error
            } else {
                anyhow::Error::new(workbench_failure(
                    "workbench.inspection_unavailable",
                    "The lane inspection is temporarily unavailable",
                ))
            }
        })
    }

    fn snapshot_with_before_state_gate(
        &self,
        workspace_root: &Path,
        before_state_gate: impl FnOnce(),
    ) -> Result<WorkbenchSnapshot> {
        let (workspace, git) = self.register_workspace_with_git(workspace_root)?;
        let project_workspaces = self.project_workspace_projections(&workspace.key)?;
        before_state_gate();
        let _project_state_guard = self.inner.project_state_gate.lock().map_err(|_| {
            anyhow::Error::new(workbench_failure(
                "workbench.snapshot_unavailable",
                "The workbench snapshot is temporarily unavailable",
            ))
        })?;
        snapshot::build_with_git_and_workspaces(
            &self.inner.project,
            &workspace,
            self.inner.revision.load(Ordering::Acquire),
            &self.inner.instance_id,
            git,
            project_workspaces,
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
        self.inner.shutting_down.store(true, Ordering::Release);
        let _ = self.inner.publication_restore_shutdown.send(true);
        let entry_provider = self
            .inner
            .entry_provider
            .lock()
            .map(|provider| Arc::clone(&provider))
            .ok();
        if let Some(entry_provider) = entry_provider {
            let _ = tokio::task::spawn_blocking(move || entry_provider.shutdown()).await;
        }
        let restore_task = self
            .inner
            .publication_restore_task
            .lock()
            .ok()
            .and_then(|mut restore_task| restore_task.take());
        if let Some(mut restore_task) = restore_task
            && tokio::time::timeout(Duration::from_secs(2), &mut restore_task)
                .await
                .is_err()
        {
            restore_task.abort();
            let _ = restore_task.await;
        }

        let tasks = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            let mut tasks = Vec::new();
            if let Some(host) = state.host.as_mut() {
                let _ = host.shutdown.send(true);
                tasks.extend(host.task.take());
            }
            for host in &mut state.retiring_hosts {
                let _ = host.shutdown.send(true);
                tasks.extend(host.task.take());
            }
            if let Some(host) = state.host.as_mut() {
                host.publication_listener = None;
            }
            for host in &mut state.retiring_hosts {
                host.publication_listener = None;
            }
            tasks
        };
        for mut task in tasks {
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        if let Err(error) = self.inner.persist_workspace_store_if_due(true) {
            eprintln!(
                "exo daemon: failed to persist workbench workspaces during shutdown: {error}"
            );
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

    pub fn observe_workspace(&self, workspace_root: &Path) -> Result<()> {
        self.register_workspace(workspace_root).map(drop)
    }

    fn release_workspace_publications(&self, workspace_keys: &[String]) {
        if workspace_keys.is_empty() {
            return;
        }
        let Ok(_launch_gate) = self.inner.host_launch_gate.lock() else {
            return;
        };
        self.inner.release_workspace_publications(workspace_keys);
    }

    fn reconcile_direct_workspace_move_locked(
        &self,
        workspace: &WorkspaceRegistration,
    ) -> Result<Vec<String>> {
        let Some(worktree_index) = self.inner.project.worktree_index() else {
            return Ok(Vec::new());
        };
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let removed_roots = state
            .workspaces_by_key
            .values()
            .filter(|candidate| {
                candidate.key != workspace.key
                    && !worktree_index.contains_key(&candidate.root)
                    && workspace_registrations_share_git_identity(candidate, workspace)
            })
            .map(|candidate| candidate.root.clone())
            .collect::<Vec<_>>();
        let removed_workspace_keys = removed_roots
            .iter()
            .filter_map(|root| state.workspaces_by_root.get(root).cloned())
            .collect::<Vec<_>>();
        self.inner
            .invalidate_removed_workspace_authorizations_locked(
                &mut state,
                &removed_workspace_keys,
                unix_seconds(),
            )?;
        for root in removed_roots {
            if let Some(key) = state.workspaces_by_root.remove(&root) {
                state.workspaces_by_key.remove(&key);
            }
        }
        state.origin_bindings.retain(|_, binding| {
            binding
                .workspace_key
                .as_ref()
                .is_none_or(|key| !removed_workspace_keys.contains(key))
        });
        state.workspace_store_dirty |= !removed_workspace_keys.is_empty();
        drop(state);
        if let Err(error) = self
            .inner
            .persist_workspace_store_if_due(!removed_workspace_keys.is_empty())
        {
            eprintln!("exo daemon: failed to persist direct worktree move: {error}");
        }
        Ok(removed_workspace_keys)
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
        let key = key.unwrap_or_else(|| deterministic_workspace_key(&self.inner.project.id, &root));
        if state
            .workspaces_by_key
            .get(&key)
            .is_some_and(|workspace| workspace.root != root)
        {
            return Err(anyhow::anyhow!("workbench workspace key collision"));
        }
        let previous = state.workspaces_by_key.get(&key).cloned();
        let observed_git = git.branch.is_some() || git.head.is_some() || git.dirty.is_some();
        let branch = if observed_git {
            git.branch.clone()
        } else {
            previous
                .as_ref()
                .and_then(|workspace| workspace.branch.clone())
        };
        let head = if observed_git {
            git.head.clone()
        } else {
            previous
                .as_ref()
                .and_then(|workspace| workspace.head.clone())
        };
        let dirty = if observed_git {
            git.dirty
        } else {
            previous.as_ref().and_then(|workspace| workspace.dirty)
        };
        let label = workspace_label(branch.as_deref(), head.as_deref(), &key);
        let workspace = WorkspaceRegistration {
            key: key.clone(),
            root: root.clone(),
            label,
            branch,
            head,
            dirty,
            observed_at: observed_git.then_some(now).or_else(|| {
                previous
                    .as_ref()
                    .and_then(|workspace| workspace.observed_at)
            }),
            registered_at: previous
                .as_ref()
                .map_or(now, |workspace| workspace.registered_at),
        };
        let changed = previous.as_ref() != Some(&workspace);
        let new_registration = previous.is_none();
        state.workspaces_by_root.insert(root, key.clone());
        state
            .workspaces_by_key
            .insert(key.clone(), workspace.clone());
        let evicted = retain_project_workspace_limit(&mut state, &key);
        let registry_trimmed = !evicted.is_empty();
        state.origin_bindings.retain(|_, binding| {
            binding
                .workspace_key
                .as_ref()
                .is_none_or(|workspace_key| !evicted.contains(workspace_key))
        });
        state.workspace_store_dirty |= changed || registry_trimmed;
        drop(state);
        self.release_workspace_publications(&evicted);
        if let Err(error) = self
            .inner
            .persist_workspace_store_if_due(new_registration || registry_trimmed)
        {
            eprintln!("exo daemon: failed to persist workspace observation: {error}");
        }
        Ok((workspace, git))
    }

    fn project_workspace_projections(
        &self,
        current_workspace_key: &str,
    ) -> Result<Vec<WorkspaceProjection>> {
        self.project_workspace_projections_with_before_authorization_gate(
            current_workspace_key,
            || {},
        )
    }

    fn project_workspace_projections_with_before_authorization_gate(
        &self,
        current_workspace_key: &str,
        before_authorization_gate: impl FnOnce(),
    ) -> Result<Vec<WorkspaceProjection>> {
        let now = unix_seconds();
        before_authorization_gate();
        let _authorization_gate = self
            .inner
            .authorization_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench authorization store is unavailable"))?;
        let worktree_index = self.inner.project.worktree_index();
        let (known_roots, current_root, live_grant_keys_by_root) = {
            let state = self
                .inner
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            let mut live_grant_keys_by_root = HashMap::<PathBuf, (u64, String)>::new();
            for grant in state.session_grants.values().filter(|grant| {
                grant.project_id == self.inner.project.id.as_str() && grant.is_live(now)
            }) {
                let replace = live_grant_keys_by_root
                    .get(&grant.workspace_root)
                    .is_none_or(|(last_activity, _)| *last_activity < grant.last_activity);
                if replace {
                    live_grant_keys_by_root.insert(
                        grant.workspace_root.clone(),
                        (grant.last_activity, grant.workspace_key.clone()),
                    );
                }
            }
            (
                state
                    .workspaces_by_root
                    .keys()
                    .cloned()
                    .collect::<HashSet<_>>(),
                state
                    .workspaces_by_key
                    .get(current_workspace_key)
                    .map(|workspace| workspace.root.clone()),
                live_grant_keys_by_root,
            )
        };

        let mut discovered = Vec::new();
        let discovery_capacity = MAX_PROJECT_WORKSPACES.saturating_sub(known_roots.len());
        if let Some(index) = worktree_index.as_ref() {
            let mut roots = index.keys().cloned().collect::<Vec<_>>();
            roots.sort();
            for root in roots {
                if known_roots.contains(&root) || discovered.len() >= discovery_capacity {
                    continue;
                }
                let key = live_grant_keys_by_root
                    .get(&root)
                    .map(|(_, key)| key.clone())
                    .unwrap_or_else(|| deterministic_workspace_key(&self.inner.project.id, &root));
                let git = if index.get(&root) == Some(&false)
                    && self
                        .inner
                        .validate_workspace(&root)
                        .is_ok_and(|resolved| resolved == root)
                {
                    snapshot::sample_git_identity(&root)
                } else {
                    snapshot::GitSnapshot::unavailable()
                };
                discovered.push(WorkspaceRegistration {
                    key: key.clone(),
                    root,
                    label: workspace_label(git.branch.as_deref(), git.head.as_deref(), &key),
                    branch: git.branch,
                    head: git.head,
                    dirty: None,
                    observed_at: None,
                    registered_at: now,
                });
            }
        }

        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let mut changed = false;
        let mut removed_workspace_keys = Vec::new();
        for workspace in discovered {
            if insert_discovered_workspace(&mut state, workspace) {
                changed = true;
            }
        }
        if let Some(index) = worktree_index.as_ref() {
            let removed = state
                .workspaces_by_root
                .keys()
                .filter(|root| {
                    current_root.as_ref().is_none_or(|current| current != *root)
                        && !index.contains_key(*root)
                })
                .cloned()
                .collect::<Vec<_>>();
            let missing_workspace_keys = removed
                .iter()
                .filter_map(|root| state.workspaces_by_root.get(root).cloned())
                .collect::<Vec<_>>();
            self.inner
                .invalidate_removed_workspace_authorizations_locked(
                    &mut state,
                    &missing_workspace_keys,
                    now,
                )?;
            for root in removed {
                if let Some(key) = state.workspaces_by_root.remove(&root) {
                    state.workspaces_by_key.remove(&key);
                    removed_workspace_keys.push(key);
                    changed = true;
                }
            }
        }
        let evicted = retain_project_workspace_limit(&mut state, current_workspace_key);
        changed |= !evicted.is_empty();
        removed_workspace_keys.extend(evicted);
        state.origin_bindings.retain(|_, binding| {
            binding
                .workspace_key
                .as_ref()
                .is_none_or(|workspace_key| !removed_workspace_keys.contains(workspace_key))
        });
        state.workspace_store_dirty |= changed;
        #[allow(
            clippy::needless_collect,
            reason = "workspace registrations must outlive the state lock before publication release"
        )]
        let registrations = state
            .workspaces_by_key
            .values()
            .cloned()
            .collect::<Vec<_>>();
        drop(state);
        drop(_authorization_gate);
        self.release_workspace_publications(&removed_workspace_keys);

        if let Err(error) = self.inner.persist_workspace_store_if_due(changed) {
            eprintln!("exo daemon: failed to persist workspace registrations: {error}");
        }

        let mut projections = registrations
            .into_iter()
            .map(|registration| {
                let current = registration.key == current_workspace_key;
                let unavailable = !current
                    && (worktree_index.as_ref().is_some_and(|index| {
                        index.get(&registration.root).copied().unwrap_or(true)
                    }) || !self
                        .inner
                        .validate_workspace(&registration.root)
                        .is_ok_and(|resolved| resolved == registration.root.as_path()));
                let availability = if unavailable {
                    "unavailable"
                } else if current
                    || registration.observed_at.is_some_and(|observed_at| {
                        observed_at.saturating_add(WORKSPACE_OBSERVATION_FRESH_LIFETIME.as_secs())
                            > now
                    })
                {
                    "live"
                } else {
                    "stale"
                };
                WorkspaceProjection {
                    registration,
                    availability,
                    current,
                }
            })
            .collect::<Vec<_>>();
        projections.sort_by(|left, right| {
            right
                .current
                .cmp(&left.current)
                .then_with(|| left.registration.label.cmp(&right.registration.label))
                .then_with(|| left.registration.key.cmp(&right.registration.key))
        });
        projections.truncate(MAX_PROJECT_WORKSPACES);
        Ok(projections)
    }

    fn ensure_host(&self) -> Result<(String, bool, [u8; 32])> {
        let _launch_gate = self
            .inner
            .host_launch_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench launch coordination is unavailable"))?;
        let provider = self
            .inner
            .entry_provider
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench entry provider is unavailable"))?
            .clone();
        let mut plan = self.prepare_host()?;
        let origin = plan.origin().to_string();
        let reused = plan.reused();
        let secret = plan.secret();
        let generation = plan.generation();
        let listener = plan
            .listener()?
            .try_clone()
            .context("retain workbench publication listener")?;
        if self.start_host_plan(&mut plan)? {
            provider.rebind_all(&listener, generation)?;
            self.retire_replaced_hosts();
        }
        Ok((origin, reused, secret))
    }

    fn prepare_host(&self) -> Result<HostLaunchPlan> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        if let Some(host) = state.host.as_ref()
            && host.server_task_alive
        {
            return Ok(HostLaunchPlan::Existing {
                generation: host.generation,
                origin: host.origin.clone(),
                listener: host
                    .publication_listener
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("live workbench listener is unavailable"))?
                    .try_clone()
                    .context("retain live workbench listener")?,
                secret: host.secret,
            });
        }
        if let Some(host) = state.host.as_ref()
            && let Some(listener) = host.publication_listener.as_ref()
        {
            let generation = self
                .inner
                .host_generation
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            return Ok(HostLaunchPlan::Candidate(PendingHost {
                generation,
                origin: host.origin.clone(),
                expected_host: host.expected_host.clone(),
                listener: Some(
                    listener
                        .try_clone()
                        .context("retain stopped workbench listener")?,
                ),
                secret: host.secret,
            }));
        }
        if self.inner.dispatcher.get().is_none() {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.host_unavailable",
                "The daemon workbench dispatcher is not ready",
            )));
        }
        let preferred_port = state.preferred_port;
        let retained_secret = state.host.as_ref().map(|host| host.secret);
        drop(state);

        let listener = if let Some(port) = preferred_port {
            match bind_workbench_listener(port) {
                Ok(listener) => Ok(listener),
                Err(preferred_error) => {
                    eprintln!(
                        "exo daemon: prior workbench port {port} is unavailable; binding a new loopback port: {preferred_error}"
                    );
                    bind_workbench_listener(0)
                }
            }
        } else {
            bind_workbench_listener(0)
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
        let expected_host = address.to_string();
        let origin = format!("http://{expected_host}");
        let generation = self
            .inner
            .host_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        Ok(HostLaunchPlan::Candidate(PendingHost {
            generation,
            origin,
            expected_host,
            listener: Some(listener),
            secret: retained_secret.map_or_else(random_bytes, Ok)?,
        }))
    }

    fn start_host_plan(&self, plan: &mut HostLaunchPlan) -> Result<bool> {
        let HostLaunchPlan::Candidate(candidate) = plan else {
            return Ok(false);
        };
        let listener = candidate
            .listener
            .take()
            .ok_or_else(|| anyhow::anyhow!("workbench candidate listener was already consumed"))?;
        let publication_listener = listener
            .try_clone()
            .context("retain workbench publication listener")?;
        let host_publication_listener = publication_listener
            .try_clone()
            .context("clone workbench publication listener")?;
        let started_at = timestamp_now();
        let assets_hash = assets::hash();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let tokio_listener =
            tokio::net::TcpListener::from_std(listener).context("adopt workbench listener")?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let weak = Arc::downgrade(&self.inner);
        let host_generation = candidate.generation;
        let task = self.inner.runtime.spawn(async move {
            let result = http::serve(tokio_listener, Weak::clone(&weak), shutdown_rx).await;
            if let Some(inner) = weak.upgrade() {
                inner.server_stopped(host_generation, result.err().map(|error| error.to_string()));
            }
        });
        let updated_at = timestamp_now();
        let host = BoundHost {
            generation: candidate.generation,
            origin: candidate.origin.clone(),
            expected_host: candidate.expected_host.clone(),
            publication_listener: Some(host_publication_listener),
            secret: candidate.secret,
            assets_hash,
            started_at,
            server_task_alive: true,
            updated_at,
            last_error: None,
            shutdown,
            task: Some(task),
        };
        state.preferred_port = publication_listener
            .local_addr()
            .ok()
            .map(|address| address.port());
        if let Some(replaced) = state.host.replace(host) {
            state.retiring_hosts.push(replaced);
        }
        *plan = HostLaunchPlan::Existing {
            generation: candidate.generation,
            origin: candidate.origin.clone(),
            listener: publication_listener,
            secret: candidate.secret,
        };
        drop(state);
        self.inner.persist_host_record();
        Ok(true)
    }

    fn retire_replaced_hosts(&self) {
        let retired = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            std::mem::take(&mut state.retiring_hosts)
        };
        for mut host in retired {
            let _ = host.shutdown.send(true);
            if let Some(task) = host.task.take() {
                task.abort();
            }
        }
    }
}

fn bind_workbench_listener(port: u16) -> std::io::Result<TcpListener> {
    #[cfg(target_os = "macos")]
    let _descriptor_guard = locald_publisher_client::ProcessSpawnBarrier::global()
        .enter_descriptor_acquisition_before(Instant::now() + Duration::from_secs(5))
        .map_err(std::io::Error::other)?;
    TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

fn insert_discovered_workspace(
    state: &mut WorkbenchState,
    workspace: WorkspaceRegistration,
) -> bool {
    if state.workspaces_by_root.contains_key(&workspace.root)
        || state.workspaces_by_key.contains_key(&workspace.key)
    {
        return false;
    }
    state
        .workspaces_by_root
        .insert(workspace.root.clone(), workspace.key.clone());
    state
        .workspaces_by_key
        .insert(workspace.key.clone(), workspace);
    true
}

fn workspace_registrations_share_git_identity(
    previous: &WorkspaceRegistration,
    candidate: &WorkspaceRegistration,
) -> bool {
    match (&previous.branch, &candidate.branch) {
        (Some(previous), Some(candidate)) => previous == candidate,
        (None, None) => previous.head.is_some() && previous.head == candidate.head,
        _ => false,
    }
}

fn retain_project_workspace_limit(
    state: &mut WorkbenchState,
    current_workspace_key: &str,
) -> Vec<String> {
    if state.workspaces_by_key.len() <= MAX_PROJECT_WORKSPACES {
        return Vec::new();
    }

    let now = unix_seconds();
    let mut protected = HashSet::from([current_workspace_key.to_string()]);
    protected.extend(
        state
            .pending_capabilities
            .values()
            .filter(|pending| pending.expires_at > now)
            .map(|pending| pending.workspace_key.clone()),
    );
    protected.extend(
        state
            .sessions
            .values()
            .filter(|session| session.is_live(now))
            .map(|session| session.workspace_key.clone()),
    );
    protected.extend(
        state
            .session_grants
            .values()
            .filter(|grant| grant.is_live(now))
            .map(|grant| grant.workspace_key.clone()),
    );
    protected.extend(
        state
            .pairing_grants
            .values()
            .filter(|pairing| pairing.is_live(now))
            .map(|pairing| pairing.workspace_key.clone()),
    );
    protected.retain(|key| state.workspaces_by_key.contains_key(key));

    let mut registrations = state
        .workspaces_by_key
        .values()
        .cloned()
        .collect::<Vec<_>>();
    registrations.sort_by(|left, right| {
        (right.key == current_workspace_key)
            .cmp(&(left.key == current_workspace_key))
            .then_with(|| {
                protected
                    .contains(&right.key)
                    .cmp(&protected.contains(&left.key))
            })
            .then_with(|| right.observed_at.cmp(&left.observed_at))
            .then_with(|| right.registered_at.cmp(&left.registered_at))
            .then_with(|| left.key.cmp(&right.key))
    });
    let retained = registrations
        .into_iter()
        .take(MAX_PROJECT_WORKSPACES.max(protected.len()))
        .map(|workspace| workspace.key)
        .collect::<HashSet<_>>();
    let removed = state
        .workspaces_by_key
        .keys()
        .filter(|key| !retained.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    state
        .workspaces_by_key
        .retain(|key, _| retained.contains(key));
    state
        .workspaces_by_root
        .retain(|_, key| retained.contains(key));
    removed
}

impl WorkbenchHostInner {
    fn release_workspace_publications(&self, workspace_keys: &[String]) {
        let Ok(provider) = self.entry_provider.lock().map(|provider| provider.clone()) else {
            return;
        };
        for workspace_key in workspace_keys {
            provider.release_workspace(workspace_key);
        }
    }

    pub(crate) fn dispatcher(&self) -> Option<&DaemonRequestDispatcher> {
        self.dispatcher.get()
    }

    pub(crate) fn entry_binding_for_request(
        &self,
        host: &str,
        origin: &str,
    ) -> Option<WorkbenchEntryBinding> {
        let state = self.state.lock().ok()?;
        if let Some(bound) = state.host.as_ref()
            && bound.expected_host == host
            && bound.origin == origin
        {
            return Some(WorkbenchEntryBinding::direct(bound.origin.clone()));
        }
        state
            .origin_bindings
            .get(origin)
            .filter(|binding| binding.expected_host() == Some(host))
            .cloned()
    }

    pub(crate) fn published_binding_for_host(&self, host: &str) -> Option<WorkbenchEntryBinding> {
        self.state
            .lock()
            .ok()?
            .origin_bindings
            .values()
            .find(|binding| binding.is_published() && binding.expected_host() == Some(host))
            .cloned()
    }

    pub(crate) fn entry_binding_for_host(&self, host: &str) -> Option<WorkbenchEntryBinding> {
        let state = self.state.lock().ok()?;
        if let Some(bound) = state.host.as_ref()
            && bound.expected_host == host
        {
            return Some(WorkbenchEntryBinding::direct(bound.origin.clone()));
        }
        state
            .origin_bindings
            .values()
            .find(|binding| binding.expected_host() == Some(host))
            .cloned()
    }

    pub(crate) fn session_matches_entry(
        &self,
        session: &WorkbenchSession,
        entry: &WorkbenchEntryBinding,
    ) -> bool {
        if session.entry.launch_mode != entry.launch_mode {
            return false;
        }
        match entry.launch_mode {
            WorkbenchLaunchMode::DirectLoopback => {
                session.entry.canonical_origin == entry.canonical_origin
            }
            WorkbenchLaunchMode::Published => session.entry == *entry,
        }
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

    fn verify_ticket(&self, ticket: &str) -> Result<VerifiedWorkbenchTicket, TicketExchangeError> {
        let mut parts = ticket.split('.');
        let envelope_version = match parts.next() {
            Some("v1") => 1,
            Some("v2") => 2,
            _ => return Err(TicketExchangeError::Invalid),
        };
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
        let (secret, direct_origin) = {
            let state = self
                .state
                .lock()
                .map_err(|_| TicketExchangeError::Invalid)?;
            let host = state.host.as_ref().ok_or(TicketExchangeError::Invalid)?;
            (host.secret, host.origin.clone())
        };
        let mut verifier =
            HmacSha256::new_from_slice(&secret).map_err(|_| TicketExchangeError::Invalid)?;
        verifier.update(&payload_bytes);
        verifier
            .verify_slice(&signature)
            .map_err(|_| TicketExchangeError::Invalid)?;
        let now = unix_seconds();
        match envelope_version {
            1 => {
                let payload: WorkbenchTicketV1 = serde_json::from_slice(&payload_bytes)
                    .map_err(|_| TicketExchangeError::Invalid)?;
                if payload.version != 1
                    || payload.instance_id != self.instance_id.as_ref()
                    || payload.project_id != self.project.id.as_str()
                    || payload.expires_at <= now
                    || payload.issued_at > now
                {
                    return Err(TicketExchangeError::Invalid);
                }
                Ok(VerifiedWorkbenchTicket {
                    capability_id: payload.capability_id,
                    project_id: payload.project_id,
                    workspace_key: payload.workspace_key,
                    entry: WorkbenchEntryBinding::direct(direct_origin),
                    capabilities: payload.capabilities,
                    expires_at: payload.expires_at,
                })
            }
            2 => {
                let payload: WorkbenchTicketV2 = serde_json::from_slice(&payload_bytes)
                    .map_err(|_| TicketExchangeError::Invalid)?;
                if payload.version != 2
                    || payload.instance_id != self.instance_id.as_ref()
                    || payload.project_id != self.project.id.as_str()
                    || payload.expires_at <= now
                    || payload.issued_at > now
                    || payload.entry_mode != WorkbenchLaunchMode::Published
                {
                    return Err(TicketExchangeError::Invalid);
                }
                let entry = WorkbenchEntryBinding::published(
                    payload.canonical_origin,
                    payload.project_instance_id,
                    payload.workspace_key.clone(),
                )
                .map_err(|_| TicketExchangeError::Invalid)?;
                Ok(VerifiedWorkbenchTicket {
                    capability_id: payload.capability_id,
                    project_id: payload.project_id,
                    workspace_key: payload.workspace_key,
                    entry,
                    capabilities: payload.capabilities,
                    expires_at: payload.expires_at,
                })
            }
            _ => Err(TicketExchangeError::Invalid),
        }
    }

    pub(crate) fn redeem_ticket(
        &self,
        ticket: &str,
    ) -> Result<(String, WorkbenchSessionResult), TicketExchangeError> {
        let payload = self.verify_ticket(ticket)?;
        if payload.entry.launch_mode != WorkbenchLaunchMode::DirectLoopback {
            return Err(TicketExchangeError::Invalid);
        }
        let now = unix_seconds();
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| TicketExchangeError::Unavailable)?;
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
            || pending.entry != payload.entry
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
            capabilities: upgraded_session_capabilities(payload.capabilities),
            entry: payload.entry,
            pairing_selector: None,
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
        self.touch_daemon_activity();
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
        if self.persist_session_store_locked().is_err() {
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

    fn reconcile_published_workspace_move_locked(
        &self,
        workspace: &WorkspaceRegistration,
        entry: &WorkbenchEntryBinding,
    ) -> Result<Option<WorkbenchAuthorizationRollback>> {
        if !entry.is_published() {
            return Ok(None);
        }
        let project_instance_id = entry
            .project_instance_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("published workbench entry has no project instance"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let now = unix_seconds();
        retain_live_authorizations(&mut state, now);
        let moved_pairings = state
            .pairing_grants
            .values()
            .filter(|pairing| {
                pairing.project_id == self.project.id.as_str()
                    && pairing.project_instance_id == project_instance_id
                    && (pairing.revoked_at.is_none()
                        || pairing.revocation_cause
                            == Some(WorkbenchPairingRevocationCause::WorkspaceMissing))
                    && (pairing.workspace_key != workspace.key
                        || pairing.workspace_root != workspace.root
                        || pairing.canonical_origin != entry.canonical_origin)
            })
            .map(|pairing| pairing.selector.clone())
            .collect::<HashSet<_>>();
        let replaced_pairings = state
            .pairing_grants
            .values()
            .filter(|pairing| {
                pairing.project_id == self.project.id.as_str()
                    && pairing.workspace_key == workspace.key
                    && pairing.project_instance_id != project_instance_id
                    && pairing.revocation_cause != Some(WorkbenchPairingRevocationCause::Replaced)
            })
            .map(|pairing| pairing.selector.clone())
            .collect::<HashSet<_>>();
        if moved_pairings.is_empty() && replaced_pairings.is_empty() {
            return Ok(None);
        }

        let rollback = WorkbenchAuthorizationRollback {
            session_grants: state.session_grants.clone(),
            sessions: state.sessions.clone(),
            pairing_grants: state.pairing_grants.clone(),
            resume_outcomes: state.resume_outcomes.clone(),
        };
        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        for selector in &moved_pairings {
            let pairing = candidate_pairings
                .get_mut(selector)
                .expect("selected pairing exists");
            pairing.workspace_key = workspace.key.clone();
            pairing.workspace_root = workspace.root.clone();
            pairing.canonical_origin = entry.canonical_origin.clone();
            pairing.restore_missing_workspace_move();
        }
        for selector in &replaced_pairings {
            candidate_pairings
                .get_mut(selector)
                .expect("replaced pairing exists")
                .revoke(now, WorkbenchPairingRevocationCause::Replaced);
        }
        candidate_sessions.retain(|_, session| {
            session.pairing_selector.as_ref().is_none_or(|selector| {
                !moved_pairings.contains(selector) && !replaced_pairings.contains(selector)
            })
        });
        candidate_outcomes.retain(|key, outcome| {
            (!moved_pairings.contains(&key.pairing_selector) || outcome.is_terminal())
                && (!replaced_pairings.contains(&key.pairing_selector) || outcome.is_terminal())
        });
        prune_retained_revoked_pairings(&mut candidate_pairings);
        retain_candidate_resume_outcomes(&mut candidate_outcomes, &candidate_pairings, now);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .context("persist moved workbench pairing authority")?;

        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        state.sessions.retain(|_, session| {
            session.pairing_selector.as_ref().is_none_or(|selector| {
                !moved_pairings.contains(selector) && !replaced_pairings.contains(selector)
            })
        });
        state.origin_bindings.retain(|_, binding| {
            binding.project_instance_id.as_deref() != Some(project_instance_id)
                || binding.canonical_origin == entry.canonical_origin
        });
        Ok(Some(rollback))
    }

    fn restore_published_workspace_move_locked(
        &self,
        rollback: WorkbenchAuthorizationRollback,
    ) -> Result<()> {
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &rollback.session_grants,
            &rollback.pairing_grants,
            &rollback.resume_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .context("restore prior workbench pairing authority")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        state.session_grants = rollback.session_grants;
        state.sessions = rollback.sessions;
        state.pairing_grants = rollback.pairing_grants;
        state.resume_outcomes = rollback.resume_outcomes;
        Ok(())
    }

    pub(crate) fn enroll_pairing(
        &self,
        ticket: &str,
        presented_pairing: Option<(&str, &str)>,
        request_entry: &WorkbenchEntryBinding,
    ) -> Result<PairingEnrollmentResult, PairingExchangeError> {
        let payload = self
            .verify_ticket(ticket)
            .map_err(pairing_error_from_ticket)?;
        if !payload.entry.is_published() || payload.entry != *request_entry {
            return Err(PairingExchangeError::Invalid);
        }
        let now = unix_seconds();
        let new_pairing_secret = random_token().map_err(|_| PairingExchangeError::Unavailable)?;
        let new_pairing_selector = random_token().map_err(|_| PairingExchangeError::Unavailable)?;
        let session_secret = random_token().map_err(|_| PairingExchangeError::Unavailable)?;
        let session_selector = random_token().map_err(|_| PairingExchangeError::Unavailable)?;
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| PairingExchangeError::Unavailable)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingExchangeError::Unavailable)?;
        retain_live_authorizations(&mut state, now);
        if state.session_grants.len() >= MAX_SESSIONS {
            return Err(PairingExchangeError::Busy);
        }
        let pending = state
            .pending_capabilities
            .get(&payload.capability_id)
            .filter(|pending| {
                pending.workspace_key == payload.workspace_key
                    && pending.entry == payload.entry
                    && pending.expires_at == payload.expires_at
            })
            .cloned()
            .ok_or(PairingExchangeError::Invalid)?;
        let workspace = state
            .workspaces_by_key
            .get(&payload.workspace_key)
            .cloned()
            .ok_or(PairingExchangeError::Invalid)?;
        let workspace_root = self
            .validate_session_workspace(&workspace.root)
            .map_err(|_| PairingExchangeError::Expired)?;
        let capabilities = upgraded_session_capabilities(payload.capabilities.clone());

        let presented_pairing = presented_pairing.and_then(|(selector, secret)| {
            state
                .pairing_grants
                .get(selector)
                .filter(|pairing| {
                    pairing.is_live(now)
                        && pairing.credential_digest == session_credential_digest(secret)
                        && pairing.entry() == payload.entry
                })
                .map(|pairing| (pairing.clone(), secret.to_string()))
        });
        let replaced_pairing_selector = presented_pairing
            .as_ref()
            .filter(|(pairing, _)| pairing.capabilities != capabilities)
            .map(|(pairing, _)| pairing.selector.clone());
        let reusable_pairing =
            presented_pairing.filter(|(pairing, _)| pairing.capabilities == capabilities);
        let creating_pairing = reusable_pairing.is_none();
        if creating_pairing {
            let project_pairings = state
                .pairing_grants
                .values()
                .filter(|pairing| pairing.is_live(now))
                .count()
                .saturating_sub(usize::from(replaced_pairing_selector.is_some()));
            let workspace_pairings = state
                .pairing_grants
                .values()
                .filter(|pairing| {
                    pairing.workspace_key == payload.workspace_key && pairing.is_live(now)
                })
                .filter(|pairing| {
                    replaced_pairing_selector.as_deref() != Some(pairing.selector.as_str())
                })
                .count();
            if project_pairings >= MAX_ACTIVE_PAIRINGS
                || workspace_pairings >= MAX_ACTIVE_PAIRINGS_PER_WORKSPACE
            {
                return Err(PairingExchangeError::Limit);
            }
        }

        let (mut pairing, pairing_secret) = reusable_pairing.unwrap_or_else(|| {
            let absolute_expires_at = now.saturating_add(PAIRING_ABSOLUTE_LIFETIME.as_secs());
            (
                WorkbenchPairingGrantV1 {
                    selector: new_pairing_selector,
                    credential_digest: session_credential_digest(&new_pairing_secret),
                    project_id: payload.project_id.clone(),
                    workspace_key: payload.workspace_key.clone(),
                    workspace_root: workspace_root.clone(),
                    launch_mode: WorkbenchLaunchMode::Published,
                    project_instance_id: payload
                        .entry
                        .project_instance_id
                        .clone()
                        .expect("published entry has project instance"),
                    canonical_origin: payload.entry.canonical_origin.clone(),
                    capabilities: capabilities.clone(),
                    created_at: now,
                    last_used_at: now,
                    idle_expires_at: now.saturating_add(PAIRING_IDLE_LIFETIME.as_secs()),
                    absolute_expires_at,
                    nickname: None,
                    revoked_at: None,
                    revocation_cause: None,
                },
                new_pairing_secret,
            )
        });
        pairing.last_used_at = now;
        pairing.idle_expires_at = now
            .saturating_add(PAIRING_IDLE_LIFETIME.as_secs())
            .min(pairing.absolute_expires_at);

        let session_expires_at = now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs());
        let session = WorkbenchSession {
            id: session_credential_digest(&session_secret),
            selector: session_selector.clone(),
            project_id: payload.project_id.clone(),
            workspace_key: payload.workspace_key.clone(),
            workspace_root,
            capabilities,
            entry: payload.entry,
            pairing_selector: Some(pairing.selector.clone()),
            created_at: now,
            last_activity: now,
            expires_at: session_expires_at,
            last_persisted_at: now,
        };
        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        if let Some(replaced_selector) = replaced_pairing_selector.as_deref() {
            candidate_sessions.retain(|_, session| {
                session.pairing_selector.as_deref() != Some(replaced_selector)
            });
            candidate_pairings
                .get_mut(replaced_selector)
                .expect("replaced pairing exists")
                .revoke(now, WorkbenchPairingRevocationCause::Replaced);
            candidate_outcomes.retain(|key, outcome| {
                key.pairing_selector.as_str() != replaced_selector || outcome.is_terminal()
            });
        }
        candidate_sessions.insert(session.id.clone(), WorkbenchSessionGrantV1::from(&session));
        candidate_pairings.insert(pairing.selector.clone(), pairing.clone());
        prune_retained_revoked_pairings(&mut candidate_pairings);
        retain_candidate_resume_outcomes(&mut candidate_outcomes, &candidate_pairings, now);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .map_err(|_| PairingExchangeError::Unavailable)?;

        state.pending_capabilities.remove(&payload.capability_id);
        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        if let Some(replaced_selector) = replaced_pairing_selector.as_deref() {
            state.sessions.retain(|_, session| {
                session.pairing_selector.as_deref() != Some(replaced_selector)
            });
        }
        state.sessions.insert(session.id.clone(), session);
        drop(state);
        self.touch_daemon_activity();
        let pairing_max_age = pairing
            .idle_expires_at
            .min(pairing.absolute_expires_at)
            .saturating_sub(now);
        Ok(PairingEnrollmentResult {
            pairing_cookie: pairing_cookie_value(&pairing.selector, &pairing_secret),
            pairing_max_age,
            session_secret,
            session: WorkbenchSessionResult {
                kind: "workbench.session",
                ok: true,
                schema_version: 1,
                session_key: session_selector,
                project_id: payload.project_id,
                workspace_key: pending.workspace_key,
                expires_at: timestamp_for_unix_seconds(session_expires_at),
            },
        })
    }

    pub(crate) fn resume_pairing(
        &self,
        pairing_selector: &str,
        pairing_secret: &str,
        request_id: &str,
        request_entry: &WorkbenchEntryBinding,
    ) -> Result<PairingResumeResult, PairingExchangeError> {
        if !valid_public_token(pairing_selector)
            || !valid_public_token(pairing_secret)
            || !valid_public_token(request_id)
            || !request_entry.is_published()
        {
            return Err(PairingExchangeError::Invalid);
        }
        let now = unix_seconds();
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| PairingExchangeError::Unavailable)?;
        let key = WorkbenchResumeOutcomeKey {
            pairing_selector: pairing_selector.to_string(),
            request_id: request_id.to_string(),
        };
        let pairing = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PairingExchangeError::Unavailable)?;
            retain_live_authorizations(&mut state, now);
            let pairing = state
                .pairing_grants
                .get(pairing_selector)
                .cloned()
                .ok_or(PairingExchangeError::Expired)?;
            if pairing.credential_digest != session_credential_digest(pairing_secret) {
                return Err(PairingExchangeError::Invalid);
            }
            if let Some(error) = state
                .resume_outcomes
                .get(&key)
                .and_then(WorkbenchResumeOutcomeV1::terminal_error)
            {
                return Err(error);
            }
            if !pairing.is_live(now) {
                return Err(PairingExchangeError::Expired);
            }
            pairing
        };
        if pairing.entry() != *request_entry {
            let mut state = self
                .state
                .lock()
                .map_err(|_| PairingExchangeError::Unavailable)?;
            retain_live_authorizations(&mut state, now);
            self.persist_terminal_resume_outcome_locked(
                &mut state,
                key.clone(),
                WorkbenchResumeTerminalErrorV1::Invalid,
                now,
            )?;
            return Err(PairingExchangeError::Invalid);
        }
        let workspace_root = match self.validate_session_workspace(&pairing.workspace_root) {
            Ok(root) => root,
            Err(_) => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| PairingExchangeError::Unavailable)?;
                retain_live_authorizations(&mut state, now);
                ensure_resume_outcome_capacity(&state, &key)?;
                let terminal =
                    terminal_resume_outcome(&key, WorkbenchResumeTerminalErrorV1::Expired, now);
                self.persist_pairing_revocation_locked(
                    &mut state,
                    pairing_selector,
                    now,
                    Some((key, terminal)),
                )
                .map_err(|_| PairingExchangeError::Unavailable)?;
                return Err(PairingExchangeError::Expired);
            }
        };

        let session_secret = derive_pairing_token(
            pairing_secret,
            request_id,
            b"exo.workbench.resume.session.secret.v1",
        )?;
        let session_selector = derive_pairing_token(
            pairing_secret,
            request_id,
            b"exo.workbench.resume.session.selector.v1",
        )?;
        let session_id = session_credential_digest(&session_secret);
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingExchangeError::Unavailable)?;
        retain_live_authorizations(&mut state, now);
        if state
            .pairing_grants
            .get(pairing_selector)
            .filter(|pairing| pairing.is_live(now))
            != Some(&pairing)
        {
            return Err(PairingExchangeError::Expired);
        }
        let replay = state.resume_outcomes.get(&key).cloned();
        if let Some(error) = replay
            .as_ref()
            .and_then(WorkbenchResumeOutcomeV1::terminal_error)
        {
            return Err(error);
        }
        if let Some(WorkbenchResumeOutcomeV1 {
            result:
                WorkbenchResumeOutcomeResultV1::Session {
                    session_selector: replay_selector,
                    session_credential_digest: replay_digest,
                    ..
                },
            ..
        }) = replay.as_ref()
            && (replay_selector != &session_selector || replay_digest != &session_id)
        {
            return Err(PairingExchangeError::Invalid);
        }
        if replay.is_none() {
            ensure_resume_outcome_capacity(&state, &key)?;
            if state.session_grants.len() >= MAX_SESSIONS {
                return Err(PairingExchangeError::Busy);
            }
        }

        let mut updated_pairing = pairing.clone();
        updated_pairing.last_used_at = now;
        updated_pairing.idle_expires_at = now
            .saturating_add(PAIRING_IDLE_LIFETIME.as_secs())
            .min(updated_pairing.absolute_expires_at);
        let session_expires_at = replay
            .as_ref()
            .and_then(|outcome| match &outcome.result {
                WorkbenchResumeOutcomeResultV1::Session {
                    session_expires_at, ..
                } => Some(*session_expires_at),
                WorkbenchResumeOutcomeResultV1::Terminal { .. } => None,
            })
            .unwrap_or_else(|| now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()))
            .max(now.saturating_add(1));
        let session = WorkbenchSession {
            id: session_id.clone(),
            selector: session_selector.clone(),
            project_id: pairing.project_id.clone(),
            workspace_key: pairing.workspace_key.clone(),
            workspace_root,
            capabilities: pairing.capabilities.clone(),
            entry: pairing.entry(),
            pairing_selector: Some(pairing.selector.clone()),
            created_at: replay.as_ref().map_or(now, |outcome| outcome.created_at),
            last_activity: now,
            expires_at: session_expires_at,
            last_persisted_at: now,
        };
        let outcome = WorkbenchResumeOutcomeV1 {
            pairing_selector: pairing.selector.clone(),
            request_id: request_id.to_string(),
            created_at: session.created_at,
            retained_until: session_expires_at
                .min(now.saturating_add(RESUME_OUTCOME_LIFETIME.as_secs())),
            result: WorkbenchResumeOutcomeResultV1::Session {
                session_selector: session_selector.clone(),
                session_credential_digest: session_id.clone(),
                session_expires_at,
            },
        };
        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        candidate_sessions.insert(session_id.clone(), WorkbenchSessionGrantV1::from(&session));
        candidate_pairings.insert(pairing.selector.clone(), updated_pairing.clone());
        candidate_outcomes.insert(key, outcome);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .map_err(|_| PairingExchangeError::Unavailable)?;
        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        state.sessions.insert(session_id, session);
        drop(state);
        self.touch_daemon_activity();
        Ok(PairingResumeResult {
            pairing_cookie: pairing_cookie_value(pairing_selector, pairing_secret),
            pairing_max_age: updated_pairing
                .idle_expires_at
                .min(updated_pairing.absolute_expires_at)
                .saturating_sub(now),
            session_secret,
            session: WorkbenchSessionResult {
                kind: "workbench.session",
                ok: true,
                schema_version: 1,
                session_key: session_selector,
                project_id: pairing.project_id,
                workspace_key: pairing.workspace_key,
                expires_at: timestamp_for_unix_seconds(session_expires_at),
            },
        })
    }

    fn persist_terminal_resume_outcome_locked(
        &self,
        state: &mut WorkbenchState,
        key: WorkbenchResumeOutcomeKey,
        error: WorkbenchResumeTerminalErrorV1,
        now: u64,
    ) -> Result<(), PairingExchangeError> {
        if state.resume_outcomes.contains_key(&key) {
            return Ok(());
        }
        ensure_resume_outcome_capacity(state, &key)?;
        let mut candidate_outcomes = state.resume_outcomes.clone();
        candidate_outcomes.insert(key.clone(), terminal_resume_outcome(&key, error, now));
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &state.session_grants,
            &state.pairing_grants,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .map_err(|_| PairingExchangeError::Unavailable)?;
        state.resume_outcomes = candidate_outcomes;
        Ok(())
    }

    fn persist_pairing_revocation_locked(
        &self,
        state: &mut WorkbenchState,
        selector: &str,
        now: u64,
        terminal_outcome: Option<(WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1)>,
    ) -> Result<()> {
        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        candidate_pairings
            .get_mut(selector)
            .ok_or_else(|| anyhow::anyhow!("workbench pairing is unavailable"))?
            .revoke(now, WorkbenchPairingRevocationCause::Explicit);
        candidate_sessions
            .retain(|_, session| session.pairing_selector.as_deref() != Some(selector));
        candidate_outcomes
            .retain(|key, outcome| key.pairing_selector != selector || outcome.is_terminal());
        if let Some((key, outcome)) = terminal_outcome {
            candidate_outcomes.insert(key, outcome);
        }
        prune_retained_revoked_pairings(&mut candidate_pairings);
        retain_candidate_resume_outcomes(&mut candidate_outcomes, &candidate_pairings, now);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)?;
        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        state
            .sessions
            .retain(|_, session| session.pairing_selector.as_deref() != Some(selector));
        retain_live_sessions(state, now);
        Ok(())
    }

    pub(crate) fn list_pairings(
        &self,
        current_selector: Option<&str>,
        workspace_key: Option<&str>,
        abbreviated_selectors: bool,
    ) -> Result<WorkbenchPairingListResult> {
        let now = unix_seconds();
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
        let mut pairings = state
            .pairing_grants
            .values()
            .filter(|pairing| {
                pairing.is_retained(now)
                    && workspace_key.is_none_or(|key| pairing.workspace_key == key)
            })
            .map(|pairing| {
                let selector = if abbreviated_selectors {
                    pairing
                        .selector
                        .chars()
                        .take(PAIRING_SELECTOR_DISPLAY_CHARS)
                        .collect()
                } else {
                    pairing.selector.clone()
                };
                WorkbenchPairingSummary {
                    selector,
                    workspace_label: state
                        .workspaces_by_key
                        .get(&pairing.workspace_key)
                        .map(|workspace| workspace.label.clone())
                        .unwrap_or_else(|| workspace_label(None, None, &pairing.workspace_key)),
                    created_at: timestamp_for_unix_seconds(pairing.created_at),
                    last_used_at: timestamp_for_unix_seconds(pairing.last_used_at),
                    expires_at: timestamp_for_unix_seconds(
                        pairing.idle_expires_at.min(pairing.absolute_expires_at),
                    ),
                    nickname: pairing.nickname.clone(),
                    status: if pairing.revoked_at.is_some() {
                        WorkbenchPairingStatus::Revoked
                    } else {
                        WorkbenchPairingStatus::Active
                    },
                    revoked_at: pairing.revoked_at.map(timestamp_for_unix_seconds),
                    current: pairing.is_live(now)
                        && current_selector == Some(pairing.selector.as_str()),
                }
            })
            .collect::<Vec<_>>();
        pairings.sort_by(|left, right| {
            matches!(left.status, WorkbenchPairingStatus::Revoked)
                .cmp(&matches!(right.status, WorkbenchPairingStatus::Revoked))
                .then_with(|| right.last_used_at.cmp(&left.last_used_at))
                .then_with(|| left.selector.cmp(&right.selector))
        });
        Ok(WorkbenchPairingListResult {
            kind: "workbench.pairing.list",
            ok: true,
            schema_version: 1,
            pairings,
        })
    }

    pub(crate) fn revoke_pairing(
        &self,
        selector_reference: &str,
        workspace_key: Option<&str>,
    ) -> std::result::Result<WorkbenchPairingMutationResult, PairingManagementError> {
        if !valid_pairing_selector_reference(selector_reference) {
            return Err(PairingManagementError::Invalid);
        }
        let _launch_gate = self
            .host_launch_gate
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let now = unix_seconds();
        retain_live_authorizations(&mut state, now);
        let selector = resolve_pairing_selector(&state, selector_reference, workspace_key)
            .ok_or(PairingManagementError::NotFound)?;
        let pairing_workspace_key = state
            .pairing_grants
            .get(&selector)
            .expect("resolved pairing exists")
            .workspace_key
            .clone();
        self.persist_pairing_revocation_locked(&mut state, &selector, now, None)
            .map_err(|_| PairingManagementError::Unavailable)?;
        let release_publication =
            mark_inactive_workspace_publication_released(&mut state, &pairing_workspace_key, now);
        drop(state);
        if release_publication {
            WorkbenchHostInner::release_workspace_publications(self, &[pairing_workspace_key]);
        }
        Ok(WorkbenchPairingMutationResult {
            kind: "workbench.pairing.revoke",
            ok: true,
            schema_version: 1,
            selector,
        })
    }

    pub(crate) fn forget_pairing(
        &self,
        selector_reference: &str,
        workspace_key: Option<&str>,
    ) -> std::result::Result<WorkbenchPairingMutationResult, PairingManagementError> {
        if !valid_pairing_selector_reference(selector_reference) {
            return Err(PairingManagementError::Invalid);
        }
        let _launch_gate = self
            .host_launch_gate
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let now = unix_seconds();
        retain_live_authorizations(&mut state, now);
        let selector = resolve_pairing_selector(&state, selector_reference, workspace_key)
            .ok_or(PairingManagementError::NotFound)?;
        let pairing_workspace_key = state
            .pairing_grants
            .get(&selector)
            .expect("resolved pairing exists")
            .workspace_key
            .clone();
        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        candidate_pairings.remove(&selector);
        candidate_sessions
            .retain(|_, session| session.pairing_selector.as_deref() != Some(selector.as_str()));
        candidate_outcomes.retain(|key, _| key.pairing_selector != selector);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .map_err(|_| PairingManagementError::Unavailable)?;
        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        state
            .sessions
            .retain(|_, session| session.pairing_selector.as_deref() != Some(selector.as_str()));
        retain_live_sessions(&mut state, now);
        let release_publication =
            mark_inactive_workspace_publication_released(&mut state, &pairing_workspace_key, now);
        drop(state);
        if release_publication {
            WorkbenchHostInner::release_workspace_publications(self, &[pairing_workspace_key]);
        }
        Ok(WorkbenchPairingMutationResult {
            kind: "workbench.pairing.forget",
            ok: true,
            schema_version: 1,
            selector,
        })
    }

    pub(crate) fn rename_pairing(
        &self,
        selector_reference: &str,
        nickname: &str,
        workspace_key: Option<&str>,
    ) -> std::result::Result<WorkbenchPairingMutationResult, PairingManagementError> {
        let nickname = nickname.trim();
        if !valid_pairing_selector_reference(selector_reference)
            || nickname.is_empty()
            || nickname.chars().count() > MAX_PAIRING_NICKNAME_CHARS
        {
            return Err(PairingManagementError::Invalid);
        }
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        let now = unix_seconds();
        retain_live_authorizations(&mut state, now);
        let selector = resolve_pairing_selector(&state, selector_reference, workspace_key)
            .ok_or(PairingManagementError::NotFound)?;
        let mut candidate_pairings = state.pairing_grants.clone();
        candidate_pairings
            .get_mut(&selector)
            .expect("resolved pairing exists")
            .nickname = Some(nickname.to_string());
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &state.session_grants,
            &candidate_pairings,
            &state.resume_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .map_err(|_| PairingManagementError::Unavailable)?;
        state.pairing_grants = candidate_pairings;
        Ok(WorkbenchPairingMutationResult {
            kind: "workbench.pairing.rename",
            ok: true,
            schema_version: 1,
            selector,
        })
    }

    pub(crate) fn pairing_session_selectors(
        &self,
        pairing_selector: &str,
    ) -> std::result::Result<Vec<String>, PairingManagementError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PairingManagementError::Unavailable)?;
        retain_live_authorizations(&mut state, unix_seconds());
        if !state.pairing_grants.contains_key(pairing_selector) {
            return Err(PairingManagementError::NotFound);
        }
        let mut selectors = state
            .session_grants
            .values()
            .filter(|session| session.pairing_selector.as_deref() == Some(pairing_selector))
            .map(|session| session.selector.clone())
            .collect::<HashSet<_>>();
        selectors.extend(
            state
                .sessions
                .values()
                .filter(|session| session.pairing_selector.as_deref() == Some(pairing_selector))
                .map(|session| session.selector.clone()),
        );
        let mut selectors = selectors.into_iter().collect::<Vec<_>>();
        selectors.sort();
        Ok(selectors)
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
        let _gate = self.authorization_store_gate.lock().ok()?;
        if !self.restore_session(session_key, credential_digest) {
            return None;
        }
        let now = unix_seconds();
        let mut state = self.state.lock().ok()?;
        retain_live_authorizations(&mut state, now);
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
        if persist_activity && let Err(error) = self.persist_session_store_locked() {
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
        let _gate = self
            .authorization_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench authorization store is unavailable"))?;
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
        self.persist_session_store_locked()
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
        let observed_git = git.branch.is_some() || git.head.is_some() || git.dirty.is_some();

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
        let previous = state.workspaces_by_key.get(&grant.workspace_key).cloned();
        let entry = grant.entry.clone().unwrap_or_else(|| {
            state
                .host
                .as_ref()
                .map(|host| WorkbenchEntryBinding::direct(host.origin.clone()))
                .unwrap_or_else(|| WorkbenchEntryBinding::direct(String::new()))
        });
        if entry.canonical_origin.is_empty() {
            return false;
        }
        let branch = if observed_git {
            git.branch
        } else {
            previous
                .as_ref()
                .and_then(|workspace| workspace.branch.clone())
        };
        let head = if observed_git {
            git.head
        } else {
            previous
                .as_ref()
                .and_then(|workspace| workspace.head.clone())
        };
        let dirty = if observed_git {
            git.dirty
        } else {
            previous.as_ref().and_then(|workspace| workspace.dirty)
        };
        let workspace = WorkspaceRegistration {
            key: grant.workspace_key.clone(),
            root: root.clone(),
            label: workspace_label(branch.as_deref(), head.as_deref(), &grant.workspace_key),
            branch,
            head,
            dirty,
            observed_at: observed_git.then_some(now).or_else(|| {
                previous
                    .as_ref()
                    .and_then(|workspace| workspace.observed_at)
            }),
            registered_at: previous
                .as_ref()
                .map_or(grant.created_at, |workspace| workspace.registered_at),
        };
        state
            .workspaces_by_root
            .insert(root, grant.workspace_key.clone());
        state
            .workspaces_by_key
            .insert(grant.workspace_key.clone(), workspace);
        let evicted = retain_project_workspace_limit(&mut state, &grant.workspace_key);
        state.origin_bindings.retain(|_, binding| {
            binding
                .workspace_key
                .as_ref()
                .is_none_or(|workspace_key| !evicted.contains(workspace_key))
        });
        state.workspace_store_dirty = true;
        state.sessions.insert(
            credential_digest.to_string(),
            WorkbenchSession {
                id: credential_digest.to_string(),
                selector: grant.selector,
                project_id: grant.project_id,
                workspace_key: grant.workspace_key,
                workspace_root: grant.workspace_root,
                capabilities: grant.capabilities,
                entry,
                pairing_selector: grant.pairing_selector,
                created_at: grant.created_at,
                last_activity: grant.last_activity,
                expires_at: grant.expires_at,
                last_persisted_at: grant.last_activity,
            },
        );
        drop(state);
        if let Ok(provider) = self.entry_provider.lock().map(|provider| provider.clone()) {
            for workspace_key in evicted {
                provider.release_workspace(&workspace_key);
            }
        }
        if let Err(error) = self.persist_workspace_store_if_due(true) {
            eprintln!("exo daemon: failed to persist restored workspace: {error}");
        }
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
            .authorization_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench session store is unavailable"))?;
        self.persist_session_store_locked()
    }

    fn persist_session_store_locked(&self) -> Result<()> {
        let store = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            authorization_store_from_state(&state, self.project.id.as_str())
        };
        write_authorization_store(&self.authorization_store_path, &store)
            .with_context(|| format!("write {}", self.authorization_store_path.display()))
    }

    fn invalidate_removed_workspace_authorizations_locked(
        &self,
        state: &mut WorkbenchState,
        workspace_keys: &[String],
        now: u64,
    ) -> Result<()> {
        if workspace_keys.is_empty() {
            return Ok(());
        }
        let removed = workspace_keys.iter().cloned().collect::<HashSet<_>>();
        let pairing_selectors = state
            .pairing_grants
            .values()
            .filter(|pairing| removed.contains(&pairing.workspace_key))
            .map(|pairing| pairing.selector.clone())
            .collect::<HashSet<_>>();
        let has_sessions = state
            .session_grants
            .values()
            .any(|session| removed.contains(&session.workspace_key));
        let has_live_pairings = pairing_selectors.iter().any(|selector| {
            state
                .pairing_grants
                .get(selector)
                .is_some_and(|pairing| pairing.revoked_at.is_none())
        });
        let has_nonterminal_outcomes = state.resume_outcomes.iter().any(|(key, outcome)| {
            pairing_selectors.contains(&key.pairing_selector) && !outcome.is_terminal()
        });
        if !has_sessions && !has_live_pairings && !has_nonterminal_outcomes {
            state
                .sessions
                .retain(|_, session| !removed.contains(&session.workspace_key));
            return Ok(());
        }

        let mut candidate_sessions = state.session_grants.clone();
        let mut candidate_pairings = state.pairing_grants.clone();
        let mut candidate_outcomes = state.resume_outcomes.clone();
        candidate_sessions.retain(|_, session| !removed.contains(&session.workspace_key));
        for selector in &pairing_selectors {
            candidate_pairings
                .get_mut(selector)
                .expect("removed workspace pairing exists")
                .revoke(now, WorkbenchPairingRevocationCause::WorkspaceMissing);
        }
        candidate_outcomes.retain(|key, outcome| {
            !pairing_selectors.contains(&key.pairing_selector) || outcome.is_terminal()
        });
        prune_retained_revoked_pairings(&mut candidate_pairings);
        retain_candidate_resume_outcomes(&mut candidate_outcomes, &candidate_pairings, now);
        let store = authorization_store_from_collections(
            self.project.id.as_str(),
            &candidate_sessions,
            &candidate_pairings,
            &candidate_outcomes,
        );
        write_authorization_store(&self.authorization_store_path, &store)
            .context("persist removed worktree authorization invalidation")?;
        state.session_grants = candidate_sessions;
        state.pairing_grants = candidate_pairings;
        state.resume_outcomes = candidate_outcomes;
        state
            .sessions
            .retain(|_, session| !removed.contains(&session.workspace_key));
        Ok(())
    }

    fn persist_workspace_store_if_due(&self, force: bool) -> Result<()> {
        let _gate = self
            .workspace_store_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench workspace store is unavailable"))?;
        let now = unix_seconds();
        let store = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench runtime state is unavailable"))?;
            if !state.workspace_store_dirty
                || (!force
                    && state
                        .workspace_store_persisted_at
                        .saturating_add(WORKSPACE_STORE_PERSIST_INTERVAL.as_secs())
                        > now)
            {
                return Ok(());
            }
            let mut workspaces = state
                .workspaces_by_key
                .values()
                .cloned()
                .map(WorkbenchWorkspaceStoreEntryV1::from)
                .collect::<Vec<_>>();
            workspaces.sort_by(|left, right| left.key.cmp(&right.key));
            workspaces.truncate(MAX_PROJECT_WORKSPACES);
            state.workspace_store_dirty = false;
            state.workspace_store_persisted_at = now;
            WorkbenchWorkspaceStoreV1 {
                schema_version: WORKSPACE_STORE_SCHEMA_VERSION,
                project_id: self.project.id.to_string(),
                workspaces,
            }
        };
        if let Err(error) = write_workspace_store(&self.workspace_store_path, &store) {
            if let Ok(mut state) = self.state.lock() {
                state.workspace_store_dirty = true;
            }
            return Err(error);
        }
        Ok(())
    }

    fn server_stopped(&self, generation: u64, error: Option<String>) {
        if let Ok(mut state) = self.state.lock()
            && let Some(host) = state.host.as_mut()
            && host.generation == generation
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
            host.publication_listener = None;
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

fn upgraded_session_capabilities(mut capabilities: Vec<String>) -> Vec<String> {
    let snapshot_index = capabilities
        .iter()
        .position(|capability| capability == "workbench.snapshot");
    let has_inspection = capabilities
        .iter()
        .any(|capability| capability == "workbench.inspect");
    if let Some(snapshot_index) = snapshot_index
        && !has_inspection
    {
        capabilities.insert(snapshot_index + 1, "workbench.inspect".to_string());
    }
    capabilities
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

impl WorkbenchPairingGrantV1 {
    const fn is_live(&self, now: u64) -> bool {
        matches!(self.launch_mode, WorkbenchLaunchMode::Published)
            && self.revoked_at.is_none()
            && self.created_at <= now
            && self.last_used_at <= now
            && self.idle_expires_at > now
            && self.absolute_expires_at > now
    }

    const fn is_retained(&self, now: u64) -> bool {
        if let Some(revoked_at) = self.revoked_at {
            matches!(self.launch_mode, WorkbenchLaunchMode::Published)
                && self.created_at <= self.last_used_at
                && self.last_used_at <= revoked_at
                && revoked_at <= now
        } else {
            self.is_live(now)
        }
    }

    fn revoke(&mut self, now: u64, cause: WorkbenchPairingRevocationCause) {
        self.revoked_at.get_or_insert(now);
        match cause {
            WorkbenchPairingRevocationCause::WorkspaceMissing => {
                if self.revocation_cause.is_none() {
                    self.revocation_cause = Some(cause);
                }
            }
            WorkbenchPairingRevocationCause::Explicit
            | WorkbenchPairingRevocationCause::Replaced => {
                self.revocation_cause = Some(cause);
            }
        }
    }

    fn restore_missing_workspace_move(&mut self) {
        if self.revocation_cause == Some(WorkbenchPairingRevocationCause::WorkspaceMissing) {
            self.revoked_at = None;
            self.revocation_cause = None;
        }
    }

    fn entry(&self) -> WorkbenchEntryBinding {
        WorkbenchEntryBinding {
            launch_mode: WorkbenchLaunchMode::Published,
            canonical_origin: self.canonical_origin.clone(),
            project_instance_id: Some(self.project_instance_id.clone()),
            workspace_key: Some(self.workspace_key.clone()),
        }
    }
}

impl WorkbenchResumeOutcomeV1 {
    fn terminal_error(&self) -> Option<PairingExchangeError> {
        match &self.result {
            WorkbenchResumeOutcomeResultV1::Terminal {
                terminal_error: WorkbenchResumeTerminalErrorV1::Invalid,
            } => Some(PairingExchangeError::Invalid),
            WorkbenchResumeOutcomeResultV1::Terminal {
                terminal_error: WorkbenchResumeTerminalErrorV1::Expired,
            } => Some(PairingExchangeError::Expired),
            WorkbenchResumeOutcomeResultV1::Session { .. } => None,
        }
    }

    const fn is_terminal(&self) -> bool {
        matches!(
            &self.result,
            WorkbenchResumeOutcomeResultV1::Terminal { .. }
        )
    }

    fn valid(&self) -> bool {
        valid_public_token(&self.pairing_selector)
            && valid_public_token(&self.request_id)
            && match &self.result {
                WorkbenchResumeOutcomeResultV1::Session {
                    session_selector,
                    session_credential_digest,
                    session_expires_at,
                } => {
                    valid_public_token(session_selector)
                        && valid_credential_digest(session_credential_digest)
                        && *session_expires_at >= self.created_at
                }
                WorkbenchResumeOutcomeResultV1::Terminal { .. } => true,
            }
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
            entry: Some(session.entry.clone()),
            pairing_selector: session.pairing_selector.clone(),
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

fn retain_live_authorizations(state: &mut WorkbenchState, now: u64) {
    retain_live_sessions(state, now);
    state
        .pairing_grants
        .retain(|_, pairing| pairing.is_retained(now));
    prune_retained_revoked_pairings(&mut state.pairing_grants);
    retain_candidate_resume_outcomes(&mut state.resume_outcomes, &state.pairing_grants, now);
    let live_pairings = state
        .pairing_grants
        .values()
        .filter(|pairing| pairing.is_live(now))
        .map(|pairing| pairing.selector.clone())
        .collect::<HashSet<_>>();
    state.session_grants.retain(|_, session| {
        session
            .pairing_selector
            .as_ref()
            .is_none_or(|selector| live_pairings.contains(selector))
    });
    state.sessions.retain(|_, session| {
        session
            .pairing_selector
            .as_ref()
            .is_none_or(|selector| live_pairings.contains(selector))
    });
}

fn retain_candidate_resume_outcomes(
    outcomes: &mut HashMap<WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1>,
    pairings: &HashMap<String, WorkbenchPairingGrantV1>,
    now: u64,
) {
    outcomes.retain(|key, outcome| {
        outcome.retained_until > now
            && pairings
                .get(&key.pairing_selector)
                .is_some_and(|pairing| outcome.is_terminal() || pairing.is_live(now))
    });
}

fn ensure_resume_outcome_capacity(
    state: &WorkbenchState,
    key: &WorkbenchResumeOutcomeKey,
) -> Result<(), PairingExchangeError> {
    if state.resume_outcomes.contains_key(key) {
        return Ok(());
    }
    let pairing_outcomes = state
        .resume_outcomes
        .keys()
        .filter(|candidate| candidate.pairing_selector == key.pairing_selector)
        .count();
    if state.resume_outcomes.len() >= MAX_RESUME_OUTCOMES
        || pairing_outcomes >= MAX_RESUME_OUTCOMES_PER_PAIRING
    {
        return Err(PairingExchangeError::Busy);
    }
    Ok(())
}

fn terminal_resume_outcome(
    key: &WorkbenchResumeOutcomeKey,
    error: WorkbenchResumeTerminalErrorV1,
    now: u64,
) -> WorkbenchResumeOutcomeV1 {
    WorkbenchResumeOutcomeV1 {
        pairing_selector: key.pairing_selector.clone(),
        request_id: key.request_id.clone(),
        created_at: now,
        retained_until: now.saturating_add(TERMINAL_RESUME_OUTCOME_LIFETIME.as_secs()),
        result: WorkbenchResumeOutcomeResultV1::Terminal {
            terminal_error: error,
        },
    }
}

fn workspace_has_live_published_authority(
    state: &WorkbenchState,
    workspace_key: &str,
    now: u64,
) -> bool {
    state.pending_capabilities.values().any(|pending| {
        pending.workspace_key == workspace_key
            && pending.expires_at > now
            && pending.entry.is_published()
    }) || state.pairing_grants.values().any(|pairing| {
        pairing.workspace_key == workspace_key
            && pairing.launch_mode == WorkbenchLaunchMode::Published
            && pairing.is_live(now)
    })
}

fn workspace_has_live_published_binding(
    state: &WorkbenchState,
    workspace_key: &str,
    workspace_root: &Path,
    expected: &WorkbenchEntryBinding,
    now: u64,
) -> bool {
    state.pairing_grants.values().any(|pairing| {
        pairing.workspace_key == workspace_key
            && pairing.workspace_root == workspace_root
            && pairing.is_live(now)
            && pairing.project_instance_id == expected.project_instance_id.as_deref().unwrap_or("")
            && pairing.canonical_origin == expected.canonical_origin
    })
}

fn mark_inactive_workspace_publication_released(
    state: &mut WorkbenchState,
    workspace_key: &str,
    now: u64,
) -> bool {
    if workspace_has_live_published_authority(state, workspace_key, now) {
        return false;
    }
    state
        .released_publication_workspaces
        .insert(workspace_key.to_string())
}

fn prune_retained_revoked_pairings(pairings: &mut HashMap<String, WorkbenchPairingGrantV1>) {
    let mut revoked = pairings
        .values()
        .filter_map(|pairing| {
            pairing.revoked_at.map(|revoked_at| {
                (
                    revoked_at,
                    pairing.selector.clone(),
                    pairing.workspace_key.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    revoked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));

    let mut kept = 0_usize;
    let mut kept_by_workspace = HashMap::<String, usize>::new();
    let mut remove = HashSet::new();
    for (_, selector, workspace_key) in revoked {
        let workspace_kept = kept_by_workspace.entry(workspace_key).or_default();
        if kept < MAX_RETAINED_REVOKED_PAIRINGS
            && *workspace_kept < MAX_RETAINED_REVOKED_PAIRINGS_PER_WORKSPACE
        {
            kept += 1;
            *workspace_kept += 1;
        } else {
            remove.insert(selector);
        }
    }
    pairings.retain(|selector, _| !remove.contains(selector));
}

fn valid_pairing_selector_reference(value: &str) -> bool {
    (8..=43).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn resolve_pairing_selector(
    state: &WorkbenchState,
    selector_reference: &str,
    workspace_key: Option<&str>,
) -> Option<String> {
    let mut matches = state
        .pairing_grants
        .values()
        .filter(|pairing| {
            pairing.selector.starts_with(selector_reference)
                && workspace_key.is_none_or(|key| pairing.workspace_key == key)
        })
        .map(|pairing| pairing.selector.clone());
    let selector = matches.next()?;
    matches.next().is_none().then_some(selector)
}

fn pairing_management_anyhow(error: PairingManagementError) -> anyhow::Error {
    let (code, kind, message) = match error {
        PairingManagementError::Invalid => (
            crate::api::protocol::ErrorCode::InvalidInput,
            "workbench.invalid_request",
            "The workbench pairing request is invalid",
        ),
        PairingManagementError::NotFound => (
            crate::api::protocol::ErrorCode::NotFound,
            "workbench.pairing_not_found",
            "The workbench pairing was not found",
        ),
        PairingManagementError::Unavailable => (
            crate::api::protocol::ErrorCode::PreconditionFailed,
            "workbench.pairing_busy",
            "The workbench pairing store is temporarily unavailable",
        ),
    };
    anyhow::Error::new(
        ExoFailure::new(code, message, ExoFailure::orienting_steering(vec![]))
            .with_details(serde_json::json!({ "kind": kind })),
    )
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

fn deterministic_workspace_key(project_id: &crate::project::ProjectId, root: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(root.as_os_str().as_encoded_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize().as_bytes())
}

fn workspace_label(branch: Option<&str>, head: Option<&str>, key: &str) -> String {
    branch
        .map(ToString::to_string)
        .or_else(|| head.map(|head| format!("detached@{}", &head[..head.len().min(8)])))
        .unwrap_or_else(|| format!("Workspace {}", &key[..key.len().min(8)]))
}

fn random_bytes() -> Result<[u8; 32]> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("read workbench random bytes")?;
    Ok(bytes)
}

fn sign_ticket<T: Serialize>(secret: &[u8; 32], payload: &T) -> Result<String> {
    let payload_bytes = serde_json::to_vec(payload)?;
    let version = serde_json::to_value(payload)?
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .filter(|version| matches!(version, 1 | 2))
        .ok_or_else(|| anyhow::anyhow!("workbench ticket version is invalid"))?;
    let mut signer =
        HmacSha256::new_from_slice(secret).context("initialize workbench ticket signer")?;
    signer.update(&payload_bytes);
    Ok(format!(
        "v{version}.{}.{}",
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

fn pairing_cookie_value(selector: &str, secret: &str) -> String {
    format!("v1.{selector}.{secret}")
}

fn derive_pairing_token(
    pairing_secret: &str,
    request_id: &str,
    domain: &[u8],
) -> Result<String, PairingExchangeError> {
    let secret = URL_SAFE_NO_PAD
        .decode(pairing_secret)
        .map_err(|_| PairingExchangeError::Invalid)?;
    if secret.len() != 32 {
        return Err(PairingExchangeError::Invalid);
    }
    let mut derivation =
        HmacSha256::new_from_slice(&secret).map_err(|_| PairingExchangeError::Invalid)?;
    derivation.update(domain);
    derivation.update(&[0]);
    derivation.update(request_id.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(derivation.finalize().into_bytes()))
}

const fn pairing_error_from_ticket(error: TicketExchangeError) -> PairingExchangeError {
    match error {
        TicketExchangeError::Invalid => PairingExchangeError::Invalid,
        TicketExchangeError::Busy => PairingExchangeError::Busy,
        TicketExchangeError::Unavailable => PairingExchangeError::Unavailable,
    }
}

fn valid_credential_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn expected_host_from_origin(origin: &str) -> Option<&str> {
    let authority = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))?;
    (!authority.is_empty()
        && !authority.contains(['/', '?', '#', '@'])
        && authority.bytes().all(|byte| !byte.is_ascii_whitespace()))
    .then_some(authority)
}

fn valid_pairing_grant(pairing: &WorkbenchPairingGrantV1, project_id: &str, now: u64) -> bool {
    pairing.project_id == project_id
        && pairing.workspace_root.is_absolute()
        && pairing.is_retained(now)
        && pairing.idle_expires_at >= pairing.created_at
        && pairing.absolute_expires_at >= pairing.created_at
        && valid_public_token(&pairing.selector)
        && valid_credential_digest(&pairing.credential_digest)
        && !pairing.project_instance_id.is_empty()
        && pairing.canonical_origin.starts_with("https://")
        && expected_host_from_origin(&pairing.canonical_origin).is_some()
        && pairing
            .revocation_cause
            .is_none_or(|_| pairing.revoked_at.is_some())
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

pub(super) fn timestamp_for_unix_seconds(seconds: u64) -> String {
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

fn load_authorization_state(
    authorization_path: &Path,
    legacy_session_path: &Path,
    project_id: &str,
    now: u64,
) -> Result<RestoredAuthorizationState> {
    if let Some(restored) = read_authorization_store(authorization_path, project_id, now)? {
        return Ok(restored);
    }
    let Some(sessions) = read_legacy_session_store(legacy_session_path, project_id, now)? else {
        return Ok(RestoredAuthorizationState::default());
    };
    let restored = RestoredAuthorizationState {
        sessions,
        ..RestoredAuthorizationState::default()
    };
    let store = authorization_store_from_collections(
        project_id,
        &restored.sessions,
        &restored.pairings,
        &restored.resume_outcomes,
    );
    write_authorization_store(authorization_path, &store)
        .context("migrate version-1 workbench sessions")?;
    if let Err(error) = archive_legacy_session_store(legacy_session_path, now) {
        eprintln!(
            "exo daemon: preserved migrated workbench sessions at {} because the legacy store could not be archived: {error}",
            legacy_session_path.display()
        );
    }
    Ok(restored)
}

fn read_authorization_store(
    path: &Path,
    project_id: &str,
    now: u64,
) -> Result<Option<RestoredAuthorizationState>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).context("decode workbench authorization store")?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    let mut restored = RestoredAuthorizationState::default();
    match schema_version {
        Some(AUTHORIZATION_STORE_SCHEMA_VERSION) => {
            let store: WorkbenchAuthorizationStoreV2 = serde_json::from_value(value)
                .context("decode version-2 workbench authorization store")?;
            if store.project_id != project_id {
                return Ok(Some(restored));
            }
            restored.sessions = restore_session_grants(store.sessions, project_id, now);
            restored.pairings = store
                .pairings
                .into_iter()
                .filter(|pairing| valid_pairing_grant(pairing, project_id, now))
                .map(|pairing| (pairing.selector.clone(), pairing))
                .collect();
            prune_retained_revoked_pairings(&mut restored.pairings);
            restored.resume_outcomes = store
                .resume_outcomes
                .into_iter()
                .filter(|outcome| {
                    outcome.retained_until > now
                        && outcome.valid()
                        && restored.pairings.contains_key(&outcome.pairing_selector)
                })
                .map(|outcome| {
                    (
                        WorkbenchResumeOutcomeKey {
                            pairing_selector: outcome.pairing_selector.clone(),
                            request_id: outcome.request_id.clone(),
                        },
                        outcome,
                    )
                })
                .collect();
        }
        _ => {}
    }
    Ok(Some(restored))
}

fn read_legacy_session_store(
    path: &Path,
    project_id: &str,
    now: u64,
) -> Result<Option<HashMap<String, WorkbenchSessionGrantV1>>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut store: WorkbenchSessionStoreV1 =
        serde_json::from_slice(&bytes).context("decode version-1 workbench session store")?;
    if store.schema_version != 1 || store.project_id != project_id {
        return Ok(Some(HashMap::new()));
    }
    for grant in &mut store.sessions {
        grant.entry = None;
        grant.pairing_selector = None;
    }
    Ok(Some(restore_session_grants(
        store.sessions,
        project_id,
        now,
    )))
}

fn archive_legacy_session_store(path: &Path, now: u64) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("legacy workbench session store has no parent"))?;
    let preferred = parent.join("workbench.sessions.v1.json");
    let archive = if !preferred.exists() {
        preferred
    } else {
        (0_u16..=u16::MAX)
            .map(|attempt| parent.join(format!("workbench.sessions.v1.{now}.{attempt}.json")))
            .find(|candidate| !candidate.exists())
            .ok_or_else(|| {
                anyhow::anyhow!("no legacy workbench session archive name is available")
            })?
    };
    std::fs::rename(path, archive).context("archive version-1 workbench session store")?;
    Ok(())
}

fn restore_session_grants(
    grants: Vec<WorkbenchSessionGrantV1>,
    project_id: &str,
    now: u64,
) -> HashMap<String, WorkbenchSessionGrantV1> {
    grants
        .into_iter()
        .filter(|grant| {
            grant.project_id == project_id
                && grant.workspace_root.is_absolute()
                && grant.is_live(now)
                && valid_public_token(&grant.selector)
                && valid_credential_digest(&grant.credential_digest)
        })
        .map(|mut grant| {
            grant.capabilities = upgraded_session_capabilities(grant.capabilities);
            (grant.credential_digest.clone(), grant)
        })
        .collect()
}

fn authorization_store_from_state(
    state: &WorkbenchState,
    project_id: &str,
) -> WorkbenchAuthorizationStoreV2 {
    authorization_store_from_collections(
        project_id,
        &state.session_grants,
        &state.pairing_grants,
        &state.resume_outcomes,
    )
}

fn authorization_store_from_collections(
    project_id: &str,
    session_grants: &HashMap<String, WorkbenchSessionGrantV1>,
    pairing_grants: &HashMap<String, WorkbenchPairingGrantV1>,
    outcomes: &HashMap<WorkbenchResumeOutcomeKey, WorkbenchResumeOutcomeV1>,
) -> WorkbenchAuthorizationStoreV2 {
    let mut sessions = session_grants.values().cloned().collect::<Vec<_>>();
    sessions.sort_by(|left, right| {
        left.selector
            .cmp(&right.selector)
            .then_with(|| left.credential_digest.cmp(&right.credential_digest))
    });
    let mut pairings = pairing_grants.values().cloned().collect::<Vec<_>>();
    pairings.sort_by(|left, right| left.selector.cmp(&right.selector));
    let mut resume_outcomes = outcomes.values().cloned().collect::<Vec<_>>();
    resume_outcomes.sort_by(|left, right| {
        left.pairing_selector
            .cmp(&right.pairing_selector)
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    WorkbenchAuthorizationStoreV2 {
        schema_version: AUTHORIZATION_STORE_SCHEMA_VERSION,
        project_id: project_id.to_string(),
        sessions,
        pairings,
        resume_outcomes,
    }
}

fn write_authorization_store(path: &Path, store: &WorkbenchAuthorizationStoreV2) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("workbench session store path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let content = serde_json::to_vec(store)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".workbench.authorizations.json.exo-tmp.")
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

fn read_workspace_store(path: &Path, project_id: &str) -> Result<Vec<WorkspaceRegistration>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let store: WorkbenchWorkspaceStoreV1 =
        serde_json::from_slice(&bytes).context("decode workbench workspace store")?;
    if store.schema_version != WORKSPACE_STORE_SCHEMA_VERSION || store.project_id != project_id {
        return Ok(Vec::new());
    }

    let mut keys = HashSet::new();
    let mut roots = HashSet::new();
    Ok(store
        .workspaces
        .into_iter()
        .filter(|workspace| {
            workspace.root.is_absolute()
                && valid_public_token(&workspace.key)
                && keys.insert(workspace.key.clone())
                && roots.insert(workspace.root.clone())
        })
        .take(MAX_PROJECT_WORKSPACES)
        .map(WorkspaceRegistration::from)
        .collect())
}

fn write_workspace_store(path: &Path, store: &WorkbenchWorkspaceStoreV1) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("workbench workspace store path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let content = serde_json::to_vec(store)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".workbench.workspaces.json.exo-tmp.")
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
