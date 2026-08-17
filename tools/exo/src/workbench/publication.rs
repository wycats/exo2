use super::{
    ResponseEnvelope, WorkbenchEntryBinding, WorkbenchEntryProvider, WorkspaceRegistration,
    workbench_failure,
};
use anyhow::Result;
use locald_core::LocaldConfig;
use locald_publisher_client::protocol::{PublicationState, ServiceName};
use locald_publisher_client::{
    InstalledPublisher, Lease, LeaseState, PublisherClient, SandboxPublisherContext, WaitOutcome,
    probe_installation, probe_sandbox_publisher,
};
#[cfg(test)]
use locald_publisher_client::{
    SystemSuspendAwareClock, UnixCommandSocketDiscovery, UnixPublisherTransport, WakeError,
    WakeMonitor, WakeRegistration, WakeSink,
};
use std::collections::HashMap;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const WORKBENCH_SERVICE_NAME: &str = "workbench";
const PUBLICATION_MONITOR_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PublicationKey {
    workspace_key: String,
    project_instance_id: String,
}

impl PublicationKey {
    fn conflicts_with(&self, replacement: &Self) -> bool {
        self != replacement
            && (self.workspace_key == replacement.workspace_key
                || self.project_instance_id == replacement.project_instance_id)
    }
}

pub(super) struct LocaldWorkbenchEntryProvider {
    client: PublisherClient,
    sandbox: Option<SandboxPublisherContext>,
    shutting_down: AtomicBool,
    publications: Mutex<HashMap<PublicationKey, Arc<ManagedPublication>>>,
}

#[cfg(test)]
#[derive(Debug)]
struct TestNoHostSuspendWakeMonitor;

#[cfg(test)]
#[derive(Debug)]
struct TestNoHostSuspendWakeRegistration;

#[cfg(test)]
impl WakeRegistration for TestNoHostSuspendWakeRegistration {}

#[cfg(test)]
impl WakeMonitor for TestNoHostSuspendWakeMonitor {
    fn register(
        &self,
        _sink: Arc<dyn WakeSink>,
    ) -> std::result::Result<Box<dyn WakeRegistration>, WakeError> {
        Ok(Box::new(TestNoHostSuspendWakeRegistration))
    }
}

impl std::fmt::Debug for LocaldWorkbenchEntryProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocaldWorkbenchEntryProvider")
            .field("sandbox", &self.sandbox)
            .finish_non_exhaustive()
    }
}

impl LocaldWorkbenchEntryProvider {
    pub(super) fn production() -> Self {
        Self {
            client: PublisherClient::production(),
            sandbox: None,
            shutting_down: AtomicBool::new(false),
            publications: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "the explicit sandbox constructor is exercised by the hermetic integration proof"
    )]
    pub(super) fn with_sandbox(sandbox: SandboxPublisherContext) -> Self {
        Self {
            client: PublisherClient::with_wake_monitor(
                Arc::new(UnixCommandSocketDiscovery),
                Arc::new(UnixPublisherTransport),
                Arc::new(SystemSuspendAwareClock),
                Arc::new(TestNoHostSuspendWakeMonitor),
            ),
            sandbox: Some(sandbox),
            shutting_down: AtomicBool::new(false),
            publications: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(super) fn publication_count(&self) -> usize {
        self.publications
            .lock()
            .map_or(0, |publications| publications.len())
    }

    #[cfg(test)]
    pub(super) fn failed_publication_count(&self) -> usize {
        self.publications.lock().map_or(0, |publications| {
            publications
                .values()
                .filter(|publication| publication.requires_replacement().unwrap_or(true))
                .count()
        })
    }

    #[cfg(test)]
    pub(super) fn publication_registry_locked_for_test(&self) -> bool {
        matches!(
            self.publications.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        )
    }

    #[cfg(test)]
    pub(super) fn mark_publications_terminal_for_test(&self) {
        if let Ok(publications) = self.publications.lock() {
            for publication in publications.values() {
                if let Ok(mut state) = publication.state.lock() {
                    state.last_error = Some("injected terminal lease state".to_string());
                }
            }
        }
    }

    fn resolve_installation(&self) -> Result<Option<InstalledPublisher>> {
        if let Some(sandbox) = &self.sandbox {
            return probe_sandbox_publisher(sandbox).map(Some).map_err(|error| {
                publication_error(
                    "workbench.publisher_sandbox_unavailable",
                    "The explicitly selected locald sandbox publisher is unavailable",
                    error,
                )
            });
        }
        probe_installation().map_err(|error| {
            publication_error(
                "workbench.publisher_installation_invalid",
                "The locald publisher installation is unavailable or unsafe",
                error,
            )
        })
    }

    fn remove_conflicting_publications(&self, key: &PublicationKey) {
        let removed = {
            let Ok(mut publications) = self.publications.lock() else {
                return;
            };
            publications
                .extract_if(|candidate, _| candidate.conflicts_with(key))
                .map(|(_, publication)| publication)
                .collect::<Vec<_>>()
        };
        for publication in removed {
            publication.stop_and_release();
        }
    }

    fn current_publication(&self, key: &PublicationKey) -> Result<Option<Arc<ManagedPublication>>> {
        let removed = {
            let mut publications = self
                .publications
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench publication registry is unavailable"))?;
            let Some(publication) = publications.get(key) else {
                return Ok(None);
            };
            if publication.requires_replacement()? {
                publications.remove(key)
            } else {
                return Ok(Some(Arc::clone(publication)));
            }
        };
        if let Some(publication) = removed {
            publication.stop_and_release();
        }
        Ok(None)
    }

    fn insert_publication(&self, publication: Arc<ManagedPublication>) -> Result<()> {
        let mut publications = self
            .publications
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication registry is unavailable"))?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(publication_stopping_error());
        }
        publications.insert(publication.key.clone(), publication);
        Ok(())
    }

    fn ensure_running(&self) -> Result<()> {
        if self.shutting_down.load(Ordering::Acquire) {
            Err(publication_stopping_error())
        } else {
            Ok(())
        }
    }
}

impl WorkbenchEntryProvider for LocaldWorkbenchEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        self.ensure_running()?;
        let opt_in = published_workbench_opt_in(&workspace.root)?;
        if !opt_in {
            self.release_workspace(&workspace.key);
            ensure_started()?;
            return Ok(WorkbenchEntryBinding::direct(direct_origin.to_string()));
        }

        let Some(installation) = self.resolve_installation()? else {
            self.release_workspace(&workspace.key);
            ensure_started()?;
            return Ok(WorkbenchEntryBinding::direct(direct_origin.to_string()));
        };
        let project_locator = workspace.root.clone().try_into().map_err(|error| {
            publication_error(
                "workbench.publisher_project_invalid",
                "The workbench workspace is not a valid absolute locald project locator",
                error,
            )
        })?;
        let project = self
            .client
            .for_project(&installation, project_locator)
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_project_unavailable",
                    "locald could not resolve the exact workbench project instance",
                    error,
                )
            })?;
        let project_instance_id = project.project_instance_id().to_string();
        let key = PublicationKey {
            workspace_key: workspace.key.clone(),
            project_instance_id: project_instance_id.clone(),
        };
        if let Some(publication) = self.current_publication(&key)? {
            let entry = publication.entry()?;
            authorize(&entry)?;
            ensure_started()?;
            publication.ensure_listener(listener, listener_generation)?;
            publication.wait_ready()?;
            self.ensure_running()?;
            self.remove_conflicting_publications(&key);
            return Ok(entry);
        }

        let service_name = ServiceName::parse(WORKBENCH_SERVICE_NAME).map_err(|error| {
            publication_error(
                "workbench.publisher_service_invalid",
                "The workbench publication service name is invalid",
                error,
            )
        })?;
        let prepared = project.prepare(service_name).map_err(|error| {
            publication_error(
                "workbench.publisher_prepare_failed",
                "locald could not prepare the declared workbench publication",
                error,
            )
        })?;
        let origin = prepared.origin().clone();
        let entry = WorkbenchEntryBinding::published(
            origin.to_string(),
            project_instance_id,
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        let installed = prepared
            .confirm_origin_installed(&origin)
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_origin_changed",
                    "The locald workbench origin changed before acquisition",
                    error,
                )
            })?;
        let lease = installed.acquire(listener).map_err(|error| {
            publication_error(
                "workbench.publisher_acquire_failed",
                "locald could not acquire the declared workbench publication",
                error,
            )
        })?;
        if let Err(error) = ensure_started() {
            drop(lease.release());
            return Err(error);
        }
        let publication = ManagedPublication::new(
            key,
            entry.clone(),
            self.client.clone(),
            installation,
            lease,
            listener_generation,
        );
        if let Err(error) = publication.wait_ready() {
            publication.stop_and_release();
            return Err(error);
        }
        if let Err(error) = self.ensure_running() {
            publication.stop_and_release();
            return Err(error);
        }
        if let Err(error) = publication.start_monitor() {
            publication.stop_and_release();
            return Err(error);
        }
        if let Err(error) = self.insert_publication(Arc::clone(&publication)) {
            publication.stop_and_release();
            return Err(error);
        }
        self.remove_conflicting_publications(&publication.key);
        Ok(entry)
    }

    fn release_workspace(&self, workspace_key: &str) {
        let removed = {
            let Ok(mut publications) = self.publications.lock() else {
                return;
            };
            publications
                .extract_if(|key, _| key.workspace_key == workspace_key)
                .map(|(_, publication)| publication)
                .collect::<Vec<_>>()
        };
        for publication in removed {
            publication.stop_and_release();
        }
    }

    fn rebind_all(&self, listener: &TcpListener, listener_generation: u64) -> Result<()> {
        let publications: Vec<Arc<ManagedPublication>> = self
            .publications
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication registry is unavailable"))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for publication in publications {
            if let Err(error) = publication.ensure_listener(listener, listener_generation) {
                failures.push(format!("{}: {error}", publication.key.workspace_key));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow::Error::new(
                workbench_failure(
                    "workbench.publisher_rebind_failed",
                    "One or more workbench publications could not move to the replacement listener",
                )
                .with_details(serde_json::json!({
                    "kind": "workbench.publisher_rebind_failed",
                    "failures": failures,
                })),
            ))
        }
    }

    fn all_on_listener_generation(&self, listener_generation: u64) -> bool {
        self.publications.lock().is_ok_and(|publications| {
            publications.values().all(|publication| {
                publication
                    .state
                    .lock()
                    .is_ok_and(|state| state.listener_generation == listener_generation)
            })
        })
    }

    fn replay_with_current_authority(
        &self,
        entry: &WorkbenchEntryBinding,
        listener_generation: u64,
        validate: &mut dyn FnMut() -> Option<ResponseEnvelope>,
    ) -> Option<ResponseEnvelope> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let (Some(workspace_key), Some(project_instance_id)) = (
            entry.workspace_key.as_ref(),
            entry.project_instance_id.as_ref(),
        ) else {
            return None;
        };
        let key = PublicationKey {
            workspace_key: workspace_key.clone(),
            project_instance_id: project_instance_id.clone(),
        };
        let publications = self.publications.lock().ok()?;
        let publication = publications.get(&key)?;

        // Keep registry membership, lifecycle authority, and lease state continuous through the
        // final manager-state validation. Provider removal and lease changes then linearize on
        // one side of the returned replay rather than inside its validation interval.
        let _authority = publication.lifecycle.enter().ok()?;
        if self.shutting_down.load(Ordering::Acquire) || publication.entry != *entry {
            return None;
        }
        let state = publication.state.lock().ok()?;
        if state.listener_generation != listener_generation || state.last_error.is_some() {
            return None;
        }
        let lease = state.lease.as_ref()?;
        let before = lease.snapshot();
        if !matches!(before.state(), LeaseState::Active)
            || before.publication_state() != PublicationState::Ready
        {
            return None;
        }

        let response = validate()?;
        let after = lease.snapshot();
        if after.sequence() != before.sequence()
            || !matches!(after.state(), LeaseState::Active)
            || after.publication_state() != PublicationState::Ready
            || publication.lifecycle.is_stopping()
            || self.shutting_down.load(Ordering::Acquire)
        {
            return None;
        }
        Some(response)
    }

    fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        let publications: Vec<Arc<ManagedPublication>> = self
            .publications
            .lock()
            .map(|mut publications| publications.drain().map(|(_, value)| value).collect())
            .unwrap_or_default();
        for publication in &publications {
            publication.begin_stop();
        }
        for publication in publications {
            publication.join_and_release();
        }
    }
}

struct ManagedPublication {
    key: PublicationKey,
    entry: WorkbenchEntryBinding,
    client: PublisherClient,
    installation: InstalledPublisher,
    state: Mutex<ManagedPublicationState>,
    lifecycle: PublicationLifecycle,
    monitor: Mutex<Option<JoinHandle<()>>>,
}

struct ManagedPublicationState {
    lease: Option<Lease>,
    listener_generation: u64,
    last_error: Option<String>,
}

#[derive(Debug, Default)]
struct PublicationLifecycle {
    stopping: AtomicBool,
    authority_gate: Mutex<()>,
}

impl PublicationLifecycle {
    fn enter(&self) -> Result<MutexGuard<'_, ()>> {
        let guard = self
            .authority_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication lifecycle is unavailable"))?;
        if self.is_stopping() {
            return Err(publication_stopping_error());
        }
        Ok(guard)
    }

    fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    fn enter_for_release(&self) -> MutexGuard<'_, ()> {
        self.authority_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ManagedPublication {
    fn new(
        key: PublicationKey,
        entry: WorkbenchEntryBinding,
        client: PublisherClient,
        installation: InstalledPublisher,
        lease: Lease,
        listener_generation: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            key,
            entry,
            client,
            installation,
            state: Mutex::new(ManagedPublicationState {
                lease: Some(lease),
                listener_generation,
                last_error: None,
            }),
            lifecycle: PublicationLifecycle::default(),
            monitor: Mutex::new(None),
        })
    }

    fn entry(&self) -> Result<WorkbenchEntryBinding> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?;
        if let Some(error) = &state.last_error {
            return Err(anyhow::Error::new(
                workbench_failure(
                    "workbench.publisher_supervision_failed",
                    "The workbench publication requires fresh locald supervision",
                )
                .with_details(serde_json::json!({
                    "kind": "workbench.publisher_supervision_failed",
                    "error": error,
                })),
            ));
        }
        drop(state);
        Ok(self.entry.clone())
    }

    fn requires_replacement(&self) -> Result<bool> {
        if self.lifecycle.is_stopping() {
            return Ok(true);
        }
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?;
        Ok(state.lease.is_none() || state.last_error.is_some())
    }

    fn start_monitor(self: &Arc<Self>) -> Result<()> {
        let weak = Arc::downgrade(self);
        let handle = thread::Builder::new()
            .name(format!(
                "exo-workbench-publication-{}",
                &self.key.workspace_key[..self.key.workspace_key.len().min(12)]
            ))
            .spawn(move || publication_monitor(weak))
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_supervision_failed",
                    "Exo could not start workbench publication supervision",
                    error,
                )
            })?;
        let mut monitor = self
            .monitor
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication monitor is unavailable"))?;
        *monitor = Some(handle);
        drop(monitor);
        Ok(())
    }

    fn lease(&self) -> Result<Lease> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?
            .lease
            .clone()
            .ok_or_else(|| {
                anyhow::Error::new(workbench_failure(
                    "workbench.publisher_lease_lost",
                    "The workbench publication no longer has current locald lease authority",
                ))
            })
    }

    fn ensure_listener(&self, listener: &TcpListener, listener_generation: u64) -> Result<()> {
        let _authority = self.lifecycle.enter()?;
        let lease = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?;
            if state.listener_generation == listener_generation {
                return Ok(());
            }
            state.lease.clone().ok_or_else(|| {
                anyhow::Error::new(workbench_failure(
                    "workbench.publisher_lease_lost",
                    "The workbench publication cannot rebind without current lease authority",
                ))
            })?
        };
        let prepared = lease.prepare_rebind().map_err(|error| {
            publication_error(
                "workbench.publisher_rebind_failed",
                "locald could not prepare the workbench listener replacement",
                error,
            )
        })?;
        let origin = prepared.origin().clone();
        if origin.as_str() != self.entry.canonical_origin {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.publisher_origin_changed",
                "locald returned a different origin for the workbench listener replacement",
            )));
        }
        let installed = prepared
            .confirm_origin_installed(&origin)
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_origin_changed",
                    "The locald workbench origin changed before listener replacement",
                    error,
                )
            })?;
        lease.rebind(installed, listener).map_err(|error| {
            publication_error(
                "workbench.publisher_rebind_failed",
                "locald could not atomically install the replacement workbench listener",
                error,
            )
        })?;
        if self.lifecycle.is_stopping() {
            return Err(publication_stopping_error());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?;
        state.listener_generation = listener_generation;
        state.last_error = None;
        drop(state);
        Ok(())
    }

    fn wait_ready(&self) -> Result<()> {
        let lease = self.lease()?;
        match lease.wait_ready().map_err(|error| {
            publication_error(
                "workbench.publisher_wait_failed",
                "locald could not observe the exact workbench binding as healthy",
                error,
            )
        })? {
            WaitOutcome::Ready(_) => Ok(()),
            WaitOutcome::TimedOut => Err(anyhow::Error::new(workbench_failure(
                "workbench.publisher_not_ready",
                "The published workbench did not become healthy before the readiness deadline",
            ))),
            WaitOutcome::BindingReplaced => Err(anyhow::Error::new(workbench_failure(
                "workbench.publisher_binding_replaced",
                "The published workbench binding changed while launch was waiting for readiness",
            ))),
            WaitOutcome::ReacquisitionRequired(_) => {
                self.reacquire()?;
                match self.lease()?.wait_ready().map_err(|error| {
                    publication_error(
                        "workbench.publisher_wait_failed",
                        "locald could not observe the reacquired workbench binding as healthy",
                        error,
                    )
                })? {
                    WaitOutcome::Ready(_) => Ok(()),
                    _ => Err(anyhow::Error::new(workbench_failure(
                        "workbench.publisher_not_ready",
                        "The reacquired workbench publication did not become exactly ready",
                    ))),
                }
            }
        }
    }

    fn reacquire(&self) -> Result<()> {
        let _authority = self.lifecycle.enter()?;
        let lease = self.lease()?;
        let Some(reacquisition) = lease.reacquisition().map_err(|error| {
            publication_error(
                "workbench.publisher_reacquire_failed",
                "The workbench publication could not retain its listener for reacquisition",
                error,
            )
        })?
        else {
            return Ok(());
        };
        let (prepared, listener) = reacquisition
            .prepare(&self.client, &self.installation)
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_reacquire_failed",
                    "locald could not prepare fresh authority for the same workbench declaration",
                    error,
                )
            })?;
        let origin = prepared.origin().clone();
        if origin.as_str() != self.entry.canonical_origin {
            return Err(anyhow::Error::new(workbench_failure(
                "workbench.publisher_origin_changed",
                "locald changed the workbench origin during lease reacquisition",
            )));
        }
        let installed = prepared
            .confirm_origin_installed(&origin)
            .map_err(|error| {
                publication_error(
                    "workbench.publisher_origin_changed",
                    "The locald workbench origin changed before reacquisition",
                    error,
                )
            })?;
        let replacement = installed.acquire(&listener).map_err(|error| {
            publication_error(
                "workbench.publisher_reacquire_failed",
                "locald could not reacquire the workbench publication",
                error,
            )
        })?;
        if self.lifecycle.is_stopping() {
            drop(replacement.release());
            return Err(publication_stopping_error());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("workbench publication state is unavailable"))?;
        if self.lifecycle.is_stopping() {
            drop(state);
            drop(replacement.release());
            return Err(publication_stopping_error());
        }
        state.lease = Some(replacement);
        state.last_error = None;
        drop(state);
        Ok(())
    }

    fn monitor_once(&self) {
        let Ok(lease) = self.lease() else {
            return;
        };
        let snapshot = lease.snapshot();
        let changed = lease.wait_for_change(snapshot.sequence(), PUBLICATION_MONITOR_INTERVAL);
        let observed = if changed.sequence() == snapshot.sequence() {
            snapshot
        } else {
            changed
        };
        match observed.state() {
            LeaseState::Active => {}
            LeaseState::ReacquisitionRequired(_) => {
                if let Err(error) = self.reacquire()
                    && let Ok(mut state) = self.state.lock()
                {
                    state.last_error = Some(error.to_string());
                }
            }
            LeaseState::AuthorityUncertain | LeaseState::Released => {
                if let Ok(mut state) = self.state.lock() {
                    state.last_error = Some(format!(
                        "locald lease entered terminal state {:?}",
                        observed.state()
                    ));
                }
            }
        }
    }

    fn begin_stop(&self) {
        self.lifecycle.begin_stop();
    }

    fn join_and_release(&self) {
        if let Ok(mut monitor) = self.monitor.lock()
            && let Some(handle) = monitor.take()
        {
            drop(handle.join());
        }
        let _authority = self.lifecycle.enter_for_release();
        let lease = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.lease.take());
        if let Some(lease) = lease {
            drop(lease.release());
        }
    }

    fn stop_and_release(&self) {
        self.begin_stop();
        self.join_and_release();
    }
}

fn publication_monitor(publication: Weak<ManagedPublication>) {
    loop {
        let Some(publication) = publication.upgrade() else {
            return;
        };
        if publication.lifecycle.is_stopping() {
            return;
        }
        publication.monitor_once();
    }
}

fn published_workbench_opt_in(workspace_root: &Path) -> Result<bool> {
    let config_path = workspace_root.join("locald.toml");
    let source = match fs::read_to_string(&config_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(publication_error(
                "workbench.publisher_config_unreadable",
                "The exact workspace locald configuration could not be read",
                error,
            ));
        }
    };
    let config = toml::from_str::<LocaldConfig>(&source).map_err(|error| {
        publication_error(
            "workbench.publisher_config_invalid",
            "The exact workspace locald configuration is invalid",
            error,
        )
    })?;
    let Some(service) = config.services.get(WORKBENCH_SERVICE_NAME) else {
        return Ok(false);
    };
    let Some(published) = service.published() else {
        return Ok(false);
    };
    if published
        .health_check
        .as_ref()
        .map(|health| health.path.as_str())
        != Some("/api/health")
    {
        return Err(anyhow::Error::new(
            workbench_failure(
                "workbench.publisher_config_invalid",
                "The published workbench service must use the HTTP /api/health probe",
            )
            .with_details(serde_json::json!({
                "kind": "workbench.publisher_config_invalid",
            })),
        ));
    }
    Ok(true)
}

fn publication_error(
    kind: &'static str,
    message: &'static str,
    error: impl std::fmt::Display,
) -> anyhow::Error {
    anyhow::Error::new(
        workbench_failure(kind, message).with_details(serde_json::json!({
            "kind": kind,
            "error": error.to_string(),
        })),
    )
}

fn publication_stopping_error() -> anyhow::Error {
    anyhow::Error::new(workbench_failure(
        "workbench.publisher_lease_lost",
        "The workbench publication is stopping and cannot change locald authority",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use tempfile::TempDir;

    fn publication_key(workspace_key: &str, project_instance_id: &str) -> PublicationKey {
        PublicationKey {
            workspace_key: workspace_key.to_string(),
            project_instance_id: project_instance_id.to_string(),
        }
    }

    #[test]
    fn publication_identity_conflicts_cover_workspace_replacement_and_worktree_moves() {
        let original = publication_key("workspace-before-move", "stable-project-instance");
        assert!(original.conflicts_with(&publication_key(
            "workspace-after-move",
            "stable-project-instance"
        )));
        assert!(original.conflicts_with(&publication_key(
            "workspace-before-move",
            "replacement-project-instance"
        )));
        assert!(!original.conflicts_with(&publication_key(
            "sibling-workspace",
            "sibling-project-instance"
        )));
        assert!(!original.conflicts_with(&original));
    }

    #[test]
    fn publication_lifecycle_serializes_authority_changes_and_rechecks_stop() {
        let lifecycle = Arc::new(PublicationLifecycle::default());
        let first = lifecycle.enter().expect("enter first authority change");
        let (attempting_tx, attempting_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let contender_lifecycle = Arc::clone(&lifecycle);
        let contender = thread::spawn(move || {
            attempting_tx.send(()).expect("announce contender");
            let stopped = contender_lifecycle.enter().is_err();
            finished_tx.send(stopped).expect("report contender result");
        });

        attempting_rx.recv().expect("contender started");
        assert!(
            finished_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "a second authority change must wait for the first"
        );

        lifecycle.begin_stop();
        drop(first);
        assert!(
            finished_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("contender finished after gate release"),
            "a queued authority change must recheck the stop fence after entering"
        );
        contender.join().expect("join contender");
    }

    #[test]
    fn publication_lifecycle_rejects_authority_changes_after_stop() {
        let lifecycle = PublicationLifecycle::default();
        lifecycle.begin_stop();

        let error = lifecycle
            .enter()
            .expect_err("stopped publication must reject authority changes");
        assert!(error.to_string().contains("publication is stopping"));
    }

    #[test]
    fn provider_shutdown_permanently_fences_new_publications() {
        let provider = LocaldWorkbenchEntryProvider::production();
        provider
            .ensure_running()
            .expect("provider starts available");

        provider.shutdown();

        let error = provider
            .ensure_running()
            .expect_err("shutdown provider must reject publication work");
        assert!(error.to_string().contains("publication is stopping"));
    }

    #[test]
    fn exact_workspace_config_controls_publication_opt_in() {
        let root = TempDir::new().expect("tempdir");
        assert!(!published_workbench_opt_in(root.path()).expect("missing config"));
        fs::write(
            root.path().join("locald.toml"),
            "[project]\nname = \"example\"\n\n[services.workbench]\ntype = \"exec\"\ncommand = \"true\"\n",
        )
        .expect("write direct config");
        assert!(!published_workbench_opt_in(root.path()).expect("direct config"));
        fs::write(
            root.path().join("locald.toml"),
            "[project]\nname = \"example\"\n\n[services.workbench]\ntype = \"published\"\n\n[services.workbench.health_check]\ntype = \"http\"\npath = \"/api/health\"\n",
        )
        .expect("write published config");
        assert!(published_workbench_opt_in(root.path()).expect("published config"));
    }

    #[test]
    fn published_workbench_requires_the_exact_health_endpoint() {
        let root = TempDir::new().expect("tempdir");
        fs::write(
            root.path().join("locald.toml"),
            "[project]\nname = \"example\"\n\n[services.workbench]\ntype = \"published\"\n",
        )
        .expect("write published config without health policy");
        let error = published_workbench_opt_in(root.path()).expect_err("missing health policy");
        assert!(error.to_string().contains("HTTP /api/health"));
    }

    #[test]
    fn invalid_exact_workspace_config_is_actionable() {
        let root = TempDir::new().expect("tempdir");
        fs::write(root.path().join("locald.toml"), "not valid = [").expect("write invalid config");
        let error = published_workbench_opt_in(root.path()).expect_err("invalid config");
        assert!(
            error
                .to_string()
                .contains("exact workspace locald configuration is invalid")
        );
    }
}
