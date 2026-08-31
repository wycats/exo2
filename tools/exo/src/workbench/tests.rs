#![allow(clippy::disallowed_methods)]

use super::*;
use crate::api::protocol::{Effect, ErrorCode, Op, PROTOCOL_VERSION, ResponseEnvelope, Status};
use crate::context::{SqliteLoader, SqliteWriter};
use crate::process_spawn::CommandSpawnExt as _;
use serde_json::{Value as JsonValue, json};
#[cfg(feature = "ui")]
use std::collections::HashMap;
use std::fs;
#[cfg(feature = "ui")]
use std::io;
#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
use std::io::Read as _;
#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(feature = "ui")]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(feature = "ui")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    project: Arc<Project>,
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct RawHttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    set_cookies: Vec<String>,
    body: Vec<u8>,
}

#[cfg(feature = "ui")]
impl RawHttpResponse {
    fn json(&self) -> JsonValue {
        serde_json::from_slice(&self.body).expect("HTTP response body is JSON")
    }
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("create workbench fixture");
    let root = temp.path().join("workspace");
    fs::create_dir(&root).expect("create fixture workspace");
    run_git(&root, &["init", "-b", "main"]);
    fs::write(root.join("README.md"), "# Workbench fixture\n").expect("write fixture");
    run_git(&root, &["add", "."]);
    run_git(
        &root,
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
    let project = Arc::new(Project::resolve(&root).expect("resolve fixture project"));
    fs::create_dir_all(
        project
            .db_path()
            .parent()
            .expect("project database has a parent"),
    )
    .expect("create project state root");
    drop(SqliteWriter::open(project.db_path()).expect("initialize project database"));
    Fixture {
        _temp: temp,
        root,
        project,
    }
}

fn run_git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    clear_repository_local_git_environment(&mut command);
    let output = command
        .args(args)
        .current_dir(root)
        .output_guarded()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clear_repository_local_git_environment(command: &mut Command) {
    for name in [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
}

#[test]
fn pairing_management_failures_preserve_stable_cli_classification() {
    for (error, code, kind) in [
        (
            PairingManagementError::Invalid,
            ErrorCode::InvalidInput,
            "workbench.invalid_request",
        ),
        (
            PairingManagementError::NotFound,
            ErrorCode::NotFound,
            "workbench.pairing_not_found",
        ),
        (
            PairingManagementError::Unavailable,
            ErrorCode::PreconditionFailed,
            "workbench.pairing_busy",
        ),
    ] {
        let failure = pairing_management_anyhow(error);
        let failure = failure
            .downcast_ref::<ExoFailure>()
            .expect("pairing management failure is structured");
        assert_eq!(failure.error.code, code);
        assert_eq!(
            failure.error.details.as_ref().expect("failure details")["kind"],
            kind
        );
    }
}

fn test_manager(project: Arc<Project>) -> WorkbenchHostManager {
    test_manager_with_identity(project, "test-workbench-instance")
}

fn test_direct_entry() -> WorkbenchEntryBinding {
    WorkbenchEntryBinding::direct("http://127.0.0.1:1".to_string())
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct TestPublishedEntryProvider;

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for TestPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        _direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        let entry = WorkbenchEntryBinding::published(
            format!("https://workbench-{}.test.localhost", &workspace.key[..8]),
            format!("locald-{}", &workspace.key[..12]),
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        ensure_started()?;
        Ok(entry)
    }
}

#[cfg(feature = "ui")]
#[derive(Debug, Default)]
struct RebindTrackingPublishedEntryProvider {
    resolves: AtomicU64,
    rebinds: AtomicU64,
    released_workspace_keys: Mutex<Vec<String>>,
}

#[cfg(feature = "ui")]
impl RebindTrackingPublishedEntryProvider {
    fn released_workspace_keys(&self) -> Vec<String> {
        self.released_workspace_keys
            .lock()
            .expect("released workspace keys")
            .clone()
    }
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct ReplayAuthorityPublishedEntryProvider {
    current: AtomicBool,
    invalidate_before_return: AtomicBool,
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for ReplayAuthorityPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        TestPublishedEntryProvider.resolve(
            workspace,
            direct_origin,
            listener,
            listener_generation,
            authorize,
            ensure_started,
        )
    }

    fn replay_with_current_authority(
        &self,
        entry: &WorkbenchEntryBinding,
        _listener_generation: u64,
        validate: &mut dyn FnMut() -> Option<ResponseEnvelope>,
    ) -> Option<ResponseEnvelope> {
        if !entry.is_published() || !self.current.load(Ordering::Acquire) {
            return None;
        }
        let response = validate()?;
        if self.invalidate_before_return.swap(false, Ordering::AcqRel) {
            self.current.store(false, Ordering::Release);
        }
        self.current.load(Ordering::Acquire).then_some(response)
    }
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for RebindTrackingPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        self.resolves.fetch_add(1, Ordering::AcqRel);
        TestPublishedEntryProvider.resolve(
            workspace,
            direct_origin,
            listener,
            listener_generation,
            authorize,
            ensure_started,
        )
    }

    fn rebind_all(&self, _listener: &TcpListener, _listener_generation: u64) -> Result<()> {
        self.rebinds.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn replay_with_current_authority(
        &self,
        entry: &WorkbenchEntryBinding,
        _listener_generation: u64,
        validate: &mut dyn FnMut() -> Option<ResponseEnvelope>,
    ) -> Option<ResponseEnvelope> {
        if entry.is_published() {
            validate()
        } else {
            None
        }
    }

    fn release_workspace(&self, workspace_key: &str) {
        self.released_workspace_keys
            .lock()
            .expect("released workspace keys")
            .push(workspace_key.to_string());
    }
}

#[cfg(feature = "ui")]
#[derive(Debug, Default)]
struct RetryingPublishedEntryProvider {
    attempts: AtomicU64,
    completed_attempts: AtomicU64,
    allow_success: AtomicBool,
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for RetryingPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        let attempt = self.attempts.fetch_add(1, Ordering::AcqRel) + 1;
        if attempt == 1 {
            anyhow::bail!("injected transient publication failure");
        }
        while !self.allow_success.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = TestPublishedEntryProvider.resolve(
            workspace,
            direct_origin,
            listener,
            listener_generation,
            authorize,
            ensure_started,
        );
        self.completed_attempts.fetch_add(1, Ordering::AcqRel);
        result
    }
}

#[cfg(feature = "ui")]
#[derive(Debug, Default)]
struct BlockingReleasePublishedEntryProvider {
    resolves: AtomicU64,
    release_started: AtomicBool,
    allow_release: AtomicBool,
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for BlockingReleasePublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        self.resolves.fetch_add(1, Ordering::AcqRel);
        TestPublishedEntryProvider.resolve(
            workspace,
            direct_origin,
            listener,
            listener_generation,
            authorize,
            ensure_started,
        )
    }

    fn release_workspace(&self, _workspace_key: &str) {
        self.release_started.store(true, Ordering::Release);
        while !self.allow_release.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct SelectivePublishedEntryProvider {
    rejected_workspace_key: String,
    attempted_workspace_keys: Mutex<Vec<String>>,
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for SelectivePublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        listener: &TcpListener,
        listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        self.attempted_workspace_keys
            .lock()
            .expect("attempted workspace keys")
            .push(workspace.key.clone());
        if workspace.key == self.rejected_workspace_key {
            anyhow::bail!("injected workspace publication failure");
        }
        TestPublishedEntryProvider.resolve(
            workspace,
            direct_origin,
            listener,
            listener_generation,
            authorize,
            ensure_started,
        )
    }
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn replacement_daemon_restores_publication_from_a_live_pairing() {
    let fixture = fixture();
    let first = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "first-published-workbench-instance",
    );
    use_test_published_entries(&first);
    let launch = first
        .launch(&fixture.root)
        .expect("launch first published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let expected = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("expected retained publication");
    let outcome_ledger = crate::daemon_outcomes::RequestOutcomeLedger::open(
        fixture
            ._temp
            .path()
            .join("replacement-launch-outcomes.sqlite"),
    )
    .expect("open replacement launch outcome ledger");
    let original_request =
        workbench_launch_request("restored-publication-old-launch", &fixture.root);
    let original_response = launch_response_envelope(&original_request.id, &launch);
    let original = outcome_ledger.execute_workbench_launch(
        original_request.clone(),
        Effect::Write,
        "first-published-workbench-instance",
        Duration::ZERO,
        |_| original_response.clone(),
        |request_id, response| first.retain_launch_replay(request_id, response),
        |request_id| first.replay_launch_response(request_id),
        |request_id| first.discard_launch_replay(request_id),
    );
    assert_same_response(&original.response, &original_response);
    first
        .inner
        .enroll_pairing(ticket, None, &expected)
        .expect("enroll durable pairing");
    {
        let mut state = first.inner.state.lock().expect("first workbench state");
        state.sessions.clear();
        state.session_grants.clear();
    }
    first
        .inner
        .persist_session_store()
        .expect("persist pairing without a resumable session");
    first.shutdown().await;
    fs::remove_file(&first.inner.host_record_path)
        .expect("remove the prior host record before replacement");

    let mismatched = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("mismatched-published-workbench-instance"),
        Arc::from("mismatched-published-workbench-process-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    mismatched.set_entry_provider(Arc::new(TestReplacementPublishedEntryProvider));
    mismatched
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install mismatched replacement dispatcher");
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let state = mismatched
            .inner
            .state
            .lock()
            .expect("mismatched workbench state");
        assert!(state.origin_bindings.is_empty());
        assert!(state.host.is_none());
        assert_eq!(state.pairing_grants.len(), 1);
    }
    mismatched.shutdown().await;

    let replacement = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("replacement-published-workbench-instance"),
        Arc::from("replacement-published-workbench-process-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
    replacement.set_entry_provider(provider.clone());
    replacement
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install replacement dispatcher");

    wait_for_workbench_condition("replacement publication restoration", || {
        provider.resolves.load(Ordering::Acquire) >= 1
    })
    .await;

    assert_eq!(provider.resolves.load(Ordering::Acquire), 1);
    assert!(replacement.host_status().is_some());
    assert_eq!(
        replacement
            .inner
            .state
            .lock()
            .expect("replacement workbench state")
            .origin_bindings
            .get(&expected.canonical_origin),
        Some(&expected)
    );

    let old_retry = outcome_ledger.execute_workbench_launch(
        original_request,
        Effect::Write,
        "replacement-published-workbench-instance",
        Duration::ZERO,
        |_| panic!("replacement daemon must not execute an old launch request ID"),
        |request_id, response| replacement.retain_launch_replay(request_id, response),
        |request_id| replacement.replay_launch_response(request_id),
        |request_id| replacement.discard_launch_replay(request_id),
    );
    assert!(old_retry.replayed);
    assert_eq!(
        old_retry
            .response
            .error
            .as_ref()
            .and_then(|error| error.details.as_ref())
            .and_then(|details| details["kind"].as_str()),
        Some("workbench.launch_replay_unavailable"),
        "restored publication does not reconstruct prior-daemon replay authority"
    );

    let fresh_launch = replacement
        .launch(&fixture.root)
        .expect("launch through restored publication");
    let fresh_request =
        workbench_launch_request("restored-publication-fresh-launch", &fixture.root);
    let fresh_response = launch_response_envelope(&fresh_request.id, &fresh_launch);
    let fresh = outcome_ledger.execute_workbench_launch(
        fresh_request.clone(),
        Effect::Write,
        "replacement-published-workbench-instance",
        Duration::ZERO,
        |_| fresh_response.clone(),
        |request_id, response| replacement.retain_launch_replay(request_id, response),
        |request_id| replacement.replay_launch_response(request_id),
        |request_id| replacement.discard_launch_replay(request_id),
    );
    assert_same_response(&fresh.response, &fresh_response);
    let fresh_retry = outcome_ledger.execute_workbench_launch(
        fresh_request,
        Effect::Write,
        "replacement-published-workbench-instance",
        Duration::ZERO,
        |_| panic!("same-daemon fresh launch retry must not execute again"),
        |request_id, response| replacement.retain_launch_replay(request_id, response),
        |request_id| replacement.replay_launch_response(request_id),
        |request_id| replacement.discard_launch_replay(request_id),
    );
    assert!(fresh_retry.replayed);
    assert_same_response(&fresh_retry.response, &fresh_response);
    replacement.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn replacement_publication_retries_without_blocking_daemon_startup() {
    let fixture = fixture();
    let first = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "first-retrying-workbench-instance",
    );
    use_test_published_entries(&first);
    let launch = first
        .launch(&fixture.root)
        .expect("launch first published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let expected = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("expected retained publication");
    first
        .inner
        .enroll_pairing(ticket, None, &expected)
        .expect("enroll durable pairing");
    first.shutdown().await;

    let replacement = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("retrying-workbench-instance"),
        Arc::from("retrying-workbench-process-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let provider = Arc::new(RetryingPublishedEntryProvider::default());
    replacement.set_entry_provider(provider.clone());
    let started_at = std::time::Instant::now();
    replacement
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install retrying replacement dispatcher");
    assert!(
        started_at.elapsed() < Duration::from_millis(100),
        "dispatcher installation must not wait for publication readiness"
    );

    wait_for_workbench_condition("second publication restoration attempt", || {
        provider.attempts.load(Ordering::Acquire) >= 2
    })
    .await;
    assert!(replacement.host_status().is_none());
    assert!(
        replacement
            .inner
            .authorization_store_gate
            .try_lock()
            .is_ok(),
        "publication readiness must not hold the authorization store gate"
    );
    provider.allow_success.store(true, Ordering::Release);
    wait_for_workbench_condition("completed publication restoration", || {
        provider.completed_attempts.load(Ordering::Acquire) >= 1
    })
    .await;
    assert!(replacement.host_status().is_some());
    assert_eq!(provider.attempts.load(Ordering::Acquire), 2);
    assert_eq!(provider.completed_attempts.load(Ordering::Acquire), 1);
    replacement.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_cancels_blocked_publication_restoration_without_republishing() {
    let fixture = fixture();
    let first = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "first-shutdown-workbench-instance",
    );
    use_test_published_entries(&first);
    let launch = first
        .launch(&fixture.root)
        .expect("launch first published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let expected = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("expected retained publication");
    first
        .inner
        .enroll_pairing(ticket, None, &expected)
        .expect("enroll durable pairing");
    first.shutdown().await;

    let replacement = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("shutdown-workbench-instance"),
        Arc::from("shutdown-workbench-process-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let provider = Arc::new(RetryingPublishedEntryProvider::default());
    replacement.set_entry_provider(provider.clone());
    replacement
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install shutdown replacement dispatcher");
    wait_for_workbench_condition("blocked publication restoration", || {
        provider.attempts.load(Ordering::Acquire) >= 2
    })
    .await;

    tokio::time::timeout(Duration::from_secs(1), replacement.shutdown())
        .await
        .expect("shutdown must not wait for the blocking restoration worker");
    provider.allow_success.store(true, Ordering::Release);
    wait_for_workbench_condition("cancelled restoration worker completion", || {
        provider.completed_attempts.load(Ordering::Acquire) >= 1
    })
    .await;
    assert!(replacement.host_status().is_none());
    assert!(
        replacement
            .inner
            .state
            .lock()
            .expect("shutdown workbench state")
            .origin_bindings
            .is_empty(),
        "a restoration worker cannot publish after shutdown"
    );
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn replacement_publication_failure_does_not_block_other_workspaces() {
    let fixture = fixture();
    let linked = fixture
        .root
        .parent()
        .expect("fixture parent")
        .join("linked");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            linked.to_str().expect("UTF-8 linked worktree"),
        ],
    );

    let first = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "first-multi-workspace-instance",
    );
    use_test_published_entries(&first);
    let mut retained = Vec::new();
    for workspace in [&fixture.root, &linked] {
        let launch = first.launch(workspace).expect("launch published workbench");
        let (_, ticket) = launch_parts(&launch);
        let payload = published_ticket_payload(ticket);
        let entry = WorkbenchEntryBinding::published(
            payload.canonical_origin,
            payload.project_instance_id,
            payload.workspace_key,
        )
        .expect("retained publication");
        first
            .inner
            .enroll_pairing(ticket, None, &entry)
            .expect("enroll durable pairing");
        retained.push((launch.workspace.key, entry));
    }
    first.shutdown().await;
    retained.sort_by(|left, right| left.0.cmp(&right.0));
    let rejected = retained[0].clone();
    let restored = retained[1].clone();

    let replacement = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("multi-workspace-replacement-instance"),
        Arc::from("multi-workspace-replacement-process-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let provider = Arc::new(SelectivePublishedEntryProvider {
        rejected_workspace_key: rejected.0.clone(),
        attempted_workspace_keys: Mutex::new(Vec::new()),
    });
    replacement.set_entry_provider(provider.clone());
    replacement
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install multi-workspace replacement dispatcher");

    wait_for_workbench_condition("independent workspace publication", || {
        let state = replacement.inner.state.lock().expect("replacement state");
        state.origin_bindings.get(&restored.1.canonical_origin) == Some(&restored.1)
    })
    .await;
    let attempted = provider
        .attempted_workspace_keys
        .lock()
        .expect("attempted workspace keys")
        .clone();
    assert!(attempted.contains(&rejected.0));
    assert!(attempted.contains(&restored.0));
    let state = replacement.inner.state.lock().expect("replacement state");
    assert!(
        !state
            .origin_bindings
            .contains_key(&rejected.1.canonical_origin)
    );
    assert_eq!(
        state.origin_bindings.get(&restored.1.canonical_origin),
        Some(&restored.1)
    );
    drop(state);
    replacement.shutdown().await;
}

#[cfg(feature = "ui")]
fn use_test_published_entries(manager: &WorkbenchHostManager) {
    manager.set_entry_provider(Arc::new(TestPublishedEntryProvider));
}

#[cfg(feature = "ui")]
async fn wait_for_workbench_condition(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {label}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
struct TestLocaldSandbox {
    home: PathBuf,
    data_dir: PathBuf,
    command_socket: PathBuf,
    log_path: PathBuf,
    http_port: u16,
    https_port: u16,
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
struct TestLocaldDaemon {
    child: std::process::Child,
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
impl Drop for TestLocaldDaemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            drop(self.child.kill());
            drop(self.child.wait());
        }
    }
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
impl TestLocaldSandbox {
    fn new(root: &Path) -> Self {
        let home = root.join("home");
        let sandbox_root = home.join(".local/share/locald/sandboxes").join("b37");
        let data_home = sandbox_root.join("data");
        fs::create_dir_all(&data_home).expect("create locald sandbox data home");
        let config_home = sandbox_root.join("config");
        fs::create_dir_all(config_home.join("locald")).expect("create locald sandbox config home");
        fs::write(
            config_home.join("locald/config.toml"),
            "[server]\nsandbox = true\n",
        )
        .expect("write locald sandbox config");
        fs::create_dir_all(sandbox_root.join("state")).expect("create locald sandbox state home");
        let http_port = TcpListener::bind("127.0.0.1:0")
            .expect("reserve sandbox HTTP port")
            .local_addr()
            .expect("sandbox HTTP address")
            .port();
        let https_port = loop {
            let port = TcpListener::bind("127.0.0.1:0")
                .expect("reserve sandbox HTTPS port")
                .local_addr()
                .expect("sandbox HTTPS address")
                .port();
            if port != http_port {
                break port;
            }
        };
        Self {
            home,
            data_dir: data_home.join("locald"),
            command_socket: sandbox_root.join("locald.sock"),
            log_path: root.join("locald-sandbox.log"),
            http_port,
            https_port,
        }
    }

    fn context(&self) -> locald_publisher_client::SandboxPublisherContext {
        locald_publisher_client::SandboxPublisherContext::new(
            locald_publisher_client::protocol::AbsolutePath::try_from(self.data_dir.clone())
                .expect("absolute sandbox data directory"),
            locald_publisher_client::protocol::AbsolutePath::try_from(self.command_socket.clone())
                .expect("absolute sandbox command socket"),
        )
        .expect("sandbox publisher context")
        .with_no_host_suspend_guarantee()
    }

    fn spawn(&self) -> TestLocaldDaemon {
        let log = fs::File::create(&self.log_path).expect("create locald sandbox log");
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        clear_repository_local_git_environment(&mut command);
        command
            .arg("b37_locald_sandbox_daemon_helper")
            .arg("--nocapture")
            .env("EXO_B37_LOCALD_DAEMON_HELPER", "1")
            .env("HOME", &self.home)
            .env(
                "XDG_DATA_HOME",
                self.data_dir.parent().expect("sandbox data home"),
            )
            .env(
                "XDG_CONFIG_HOME",
                self.command_socket
                    .parent()
                    .expect("sandbox root")
                    .join("config"),
            )
            .env(
                "XDG_STATE_HOME",
                self.command_socket
                    .parent()
                    .expect("sandbox root")
                    .join("state"),
            )
            .env("LOCALD_SOCKET", &self.command_socket)
            .env("LOCALD_SANDBOX_ACTIVE", "1")
            .env("LOCALD_SANDBOX_NAME", "b37")
            .env("LOCALD_SANDBOX_NO_HOST_SUSPEND", "1")
            .env("LOCALD_HTTP_PORT", self.http_port.to_string())
            .env("LOCALD_HTTPS_PORT", self.https_port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().expect("clone locald log")))
            .stderr(Stdio::from(log));
        #[cfg(target_os = "linux")]
        // Exercise the explicit sandbox fallback without depending on the CI host's logind state.
        command.env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            format!(
                "unix:path={}",
                self.command_socket
                    .with_file_name("missing-system-bus.sock")
                    .display()
            ),
        );
        TestLocaldDaemon {
            child: command.spawn_guarded().expect("spawn locald sandbox"),
        }
    }

    fn wait_until_active(&self, daemon: &mut TestLocaldDaemon) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let probe_error =
                match locald_publisher_client::probe_sandbox_publisher(&self.context()) {
                    Ok(_) => return,
                    Err(error) => error,
                };
            if let Some(status) = daemon.child.try_wait().expect("inspect locald sandbox") {
                panic!(
                    "locald sandbox exited before publication became active ({status}); last probe error: {probe_error}: {}",
                    fs::read_to_string(&self.log_path).unwrap_or_default()
                );
            }
            assert!(
                std::time::Instant::now() < deadline,
                "locald sandbox did not activate publication; last probe error: {probe_error}: {}",
                fs::read_to_string(&self.log_path).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&self, daemon: &mut TestLocaldDaemon) {
        let mut stream = UnixStream::connect(&self.command_socket)
            .expect("connect to locald sandbox for shutdown");
        serde_json::to_writer(&mut stream, &locald_core::IpcRequest::Shutdown)
            .expect("send locald sandbox shutdown");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("finish locald sandbox shutdown request");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .expect("read locald sandbox shutdown response");
        assert_eq!(
            serde_json::from_slice::<locald_core::IpcResponse>(&response)
                .expect("decode locald sandbox shutdown response"),
            locald_core::IpcResponse::Ok
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if daemon
                .child
                .try_wait()
                .expect("wait for locald sandbox")
                .is_some()
            {
                return;
            }
            if std::time::Instant::now() >= deadline {
                daemon.child.kill().expect("kill stalled locald sandbox");
                daemon.child.wait().expect("reap stalled locald sandbox");
                panic!(
                    "locald sandbox did not stop cleanly: {}",
                    fs::read_to_string(&self.log_path).unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
#[test]
fn b37_locald_sandbox_daemon_helper() {
    if std::env::var_os("EXO_B37_LOCALD_DAEMON_HELPER").is_none() {
        return;
    }
    locald_server::run(true, "exo-b37-proof".to_owned()).expect("run locald sandbox daemon");
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct TestMovedPublishedEntryProvider;

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for TestMovedPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        _direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        let entry = WorkbenchEntryBinding::published(
            "https://workbench-moved.test.localhost".to_string(),
            "locald-stable-project-instance".to_string(),
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        ensure_started()?;
        Ok(entry)
    }
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct TestFailingMovedPublishedEntryProvider;

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for TestFailingMovedPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        _direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        _ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        let entry = WorkbenchEntryBinding::published(
            "https://workbench-moved.test.localhost".to_string(),
            "locald-stable-project-instance".to_string(),
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        anyhow::bail!("injected moved-worktree publication failure")
    }
}

#[cfg(feature = "ui")]
#[derive(Debug)]
struct TestReplacementPublishedEntryProvider;

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for TestReplacementPublishedEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        _direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        let entry = WorkbenchEntryBinding::published(
            "https://workbench-moved.test.localhost".to_string(),
            "locald-replacement-project-instance".to_string(),
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        ensure_started()?;
        Ok(entry)
    }
}

#[cfg(feature = "ui")]
#[derive(Debug, Default)]
struct TestPublishedThenDirectEntryProvider {
    direct: AtomicBool,
    released_workspace_keys: Mutex<Vec<String>>,
}

#[cfg(feature = "ui")]
impl TestPublishedThenDirectEntryProvider {
    fn use_direct_entry(&self) {
        self.direct.store(true, Ordering::Release);
    }

    fn released_workspace_keys(&self) -> Vec<String> {
        self.released_workspace_keys
            .lock()
            .expect("released workspace keys")
            .clone()
    }
}

#[cfg(feature = "ui")]
impl WorkbenchEntryProvider for TestPublishedThenDirectEntryProvider {
    fn resolve(
        &self,
        workspace: &WorkspaceRegistration,
        direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        if self.direct.load(Ordering::Acquire) {
            ensure_started()?;
            return Ok(WorkbenchEntryBinding::direct(direct_origin.to_string()));
        }
        let entry = WorkbenchEntryBinding::published(
            "https://workbench-moved.test.localhost".to_string(),
            "locald-stable-project-instance".to_string(),
            workspace.key.clone(),
        )?;
        authorize(&entry)?;
        ensure_started()?;
        Ok(entry)
    }

    fn release_workspace(&self, workspace_key: &str) {
        self.released_workspace_keys
            .lock()
            .expect("released workspace keys")
            .push(workspace_key.to_string());
    }
}

#[cfg(all(feature = "ui", any(target_os = "linux", target_os = "macos")))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn locald_publication_keeps_two_worktrees_on_one_host_across_daemon_restart() {
    async fn launch_eventually(
        manager: &WorkbenchHostManager,
        workspace: &Path,
    ) -> WorkbenchLaunchResult {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let error = match manager.launch(workspace) {
                Ok(launch) => return launch,
                Err(error) => error,
            };
            assert!(
                std::time::Instant::now() < deadline,
                "published workbench did not recover: {error:#}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn canonical_origin(launch: &WorkbenchLaunchResult) -> &str {
        launch
            .url
            .split_once("/#ticket=")
            .map_or(launch.url.as_str(), |(origin, _)| origin)
    }

    let temp = tempfile::Builder::new()
        .prefix("e")
        .tempdir_in("/tmp")
        .expect("create short b.3.7 proof root");
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    fs::create_dir(&primary).expect("create primary worktree");
    run_git(&primary, &["init", "-b", "main"]);
    fs::write(primary.join("README.md"), "# Exo b.3.7 proof\n").expect("write proof readme");
    run_git(&primary, &["add", "."]);
    run_git(
        &primary,
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
    run_git(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            "b37-linked",
            linked.to_str().expect("UTF-8 linked worktree"),
        ],
    );
    let config = r#"[project]
name = "exo-b37-proof"

[services.workbench]
type = "published"

[services.workbench.health_check]
type = "http"
path = "/api/health"
interval = 1
timeout = 1
"#;
    fs::write(primary.join("locald.toml"), config).expect("write primary locald config");
    fs::write(linked.join("locald.toml"), config).expect("write linked locald config");

    let sandbox = TestLocaldSandbox::new(temp.path());
    let mut daemon = sandbox.spawn();
    sandbox.wait_until_active(&mut daemon);

    let project = Arc::new(Project::resolve(&primary).expect("resolve proof project"));
    fs::create_dir_all(
        project
            .db_path()
            .parent()
            .expect("proof database has a parent"),
    )
    .expect("create proof project state root");
    drop(SqliteWriter::open(project.db_path()).expect("initialize proof project database"));
    let manager = test_manager_with_identity(Arc::clone(&project), "b37-publication-proof");
    let provider = Arc::new(publication::LocaldWorkbenchEntryProvider::with_sandbox(
        sandbox.context(),
    ));
    manager.set_entry_provider(provider.clone());

    let first = launch_eventually(&manager, &primary).await;
    let second = launch_eventually(&manager, &linked).await;
    assert_eq!(first.launch_mode, WorkbenchLaunchMode::Published);
    assert_eq!(second.launch_mode, WorkbenchLaunchMode::Published);
    assert!(!first.reused_host);
    assert!(second.reused_host);
    let first_origin = canonical_origin(&first).to_owned();
    let second_origin = canonical_origin(&second).to_owned();
    assert_ne!(first_origin, second_origin);
    for origin in [&first_origin, &second_origin] {
        assert!(origin.starts_with("https://workbench"));
        assert!(origin.contains(".localhost"));
        assert!(
            !origin.contains("127.0.0.1"),
            "published workbench origin must not expose the private listener: {origin}"
        );
    }
    let (listener_generation, first_project_instance, second_project_instance) = {
        let state = manager.inner.state.lock().expect("workbench state");
        let listener_generation = state.host.as_ref().expect("shared host").generation;
        let first_binding = state
            .origin_bindings
            .get(&first_origin)
            .expect("first published binding");
        let second_binding = state
            .origin_bindings
            .get(&second_origin)
            .expect("second published binding");
        (
            listener_generation,
            first_binding
                .project_instance_id
                .clone()
                .expect("first project instance"),
            second_binding
                .project_instance_id
                .clone()
                .expect("second project instance"),
        )
    };
    assert_ne!(first_project_instance, second_project_instance);
    assert_eq!(provider.publication_count(), 2);
    assert!(provider.all_on_listener_generation(listener_generation));

    provider.mark_publications_terminal_for_test();
    assert_eq!(provider.failed_publication_count(), 2);
    let reacquired_first = launch_eventually(&manager, &primary).await;
    let reacquired_second = launch_eventually(&manager, &linked).await;
    assert_eq!(canonical_origin(&reacquired_first), first_origin);
    assert_eq!(canonical_origin(&reacquired_second), second_origin);
    assert_eq!(provider.failed_publication_count(), 0);
    assert_eq!(provider.publication_count(), 2);

    sandbox.stop(&mut daemon);
    daemon = sandbox.spawn();
    sandbox.wait_until_active(&mut daemon);

    let recovered_first = launch_eventually(&manager, &primary).await;
    let recovered_second = launch_eventually(&manager, &linked).await;
    assert_eq!(canonical_origin(&recovered_first), first_origin);
    assert_eq!(canonical_origin(&recovered_second), second_origin);
    assert!(recovered_first.reused_host);
    assert!(recovered_second.reused_host);
    assert_eq!(provider.publication_count(), 2);
    assert!(provider.all_on_listener_generation(listener_generation));

    let second_entry = WorkbenchEntryBinding::published(
        second_origin.clone(),
        second_project_instance,
        second.workspace.key.clone(),
    )
    .expect("second published entry");
    let second_workspace_key = second.workspace.key.clone();
    let provider_for_release = Arc::clone(&provider);
    let mut release_handle = None;
    let mut authorize_replay = || {
        let (attempting_tx, attempting_rx) = std::sync::mpsc::channel();
        release_handle = Some(std::thread::spawn({
            let second_workspace_key = second_workspace_key.clone();
            let provider = Arc::clone(&provider_for_release);
            move || {
                attempting_tx
                    .send(())
                    .expect("announce publication removal");
                provider.release_workspace(&second_workspace_key);
            }
        }));
        attempting_rx.recv().expect("publication removal started");
        assert!(
            provider.publication_registry_locked_for_test(),
            "replay authority must retain exact provider registry membership"
        );
        Some(launch_response_envelope(
            "provider-removal-race",
            &recovered_second,
        ))
    };
    assert!(
        provider
            .replay_with_current_authority(
                &second_entry,
                listener_generation,
                &mut authorize_replay,
            )
            .is_some(),
        "replay linearizes before a concurrent publication removal"
    );
    release_handle
        .take()
        .expect("publication release thread")
        .join()
        .expect("join publication release thread");
    assert_eq!(provider.publication_count(), 1);

    let (_, first_ticket) = launch_parts(&first);
    let first_entry = WorkbenchEntryBinding::published(
        first_origin.clone(),
        first_project_instance,
        first.workspace.key.clone(),
    )
    .expect("first retained published entry");
    manager
        .inner
        .enroll_pairing(first_ticket, None, &first_entry)
        .expect("enroll retained publication pairing");
    {
        let mut state = manager.inner.state.lock().expect("published state");
        state.sessions.clear();
        state.session_grants.clear();
    }
    manager
        .inner
        .persist_session_store()
        .expect("persist retained publication pairing");
    let mut authorize_replay = || {
        Some(launch_response_envelope(
            "provider-current",
            &recovered_first,
        ))
    };
    assert!(
        provider
            .replay_with_current_authority(
                &first_entry,
                listener_generation,
                &mut authorize_replay,
            )
            .is_some()
    );
    provider.shutdown();
    let mut authorize_replay = || {
        Some(launch_response_envelope(
            "provider-stopped",
            &recovered_first,
        ))
    };
    assert!(
        provider
            .replay_with_current_authority(
                &first_entry,
                listener_generation,
                &mut authorize_replay,
            )
            .is_none(),
        "a shutting-down locald provider cannot authorize replay"
    );
    manager.shutdown().await;
    assert_eq!(provider.publication_count(), 0);

    let replacement = WorkbenchHostManager::new(
        Arc::clone(&project),
        Arc::from("b37-replacement-publication-proof"),
        Arc::from("b37-replacement-publication-process-start"),
        project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let replacement_provider = Arc::new(publication::LocaldWorkbenchEntryProvider::with_sandbox(
        sandbox.context(),
    ));
    replacement.set_entry_provider(replacement_provider.clone());
    replacement
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(Effect::Pure),
                trace: None,
            }
        }))
        .expect("install replacement publication dispatcher");
    wait_for_workbench_condition("real locald publication restoration", || {
        replacement_provider.publication_count() == 1
    })
    .await;
    assert_eq!(replacement_provider.publication_count(), 1);
    assert!(replacement.requires_daemon_residency(unix_seconds()));
    assert_eq!(
        replacement
            .inner
            .state
            .lock()
            .expect("replacement publication state")
            .origin_bindings
            .get(&first_origin),
        Some(&first_entry)
    );
    replacement.shutdown().await;
    assert_eq!(replacement_provider.publication_count(), 0);
    sandbox.stop(&mut daemon);
}

fn test_manager_with_identity(
    project: Arc<Project>,
    instance_id: &'static str,
) -> WorkbenchHostManager {
    let manager = WorkbenchHostManager::new(
        Arc::clone(&project),
        Arc::from(instance_id),
        Arc::from(format!("{instance_id}-process-start")),
        project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    manager
        .set_dispatcher(DaemonRequestDispatcher::new(|request| async move {
            let effect = match &request.op {
                Op::Call(call)
                    if matches!(
                        &call.address,
                        crate::api::protocol::Address::Operation { path }
                            if path.as_slice() == ["lane", "focus"]
                    ) =>
                {
                    Effect::Write
                }
                _ => Effect::Pure,
            };
            ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: request.id,
                status: Status::Ok,
                result: Some(json!({ "kind": "test.dispatch", "ok": true })),
                error: None,
                ticket: None,
                steering: None,
                reminders: None,
                display: None,
                preview: None,
                effect: Some(effect),
                trace: None,
            }
        }))
        .expect("install test dispatcher");
    manager
}

#[cfg(feature = "ui")]
fn launch_parts(launch: &WorkbenchLaunchResult) -> (&str, &str) {
    launch
        .url
        .split_once("/#ticket=")
        .expect("launch URL contains ticket fragment")
}

#[cfg(feature = "ui")]
fn ticket_payload(ticket: &str) -> WorkbenchTicketV1 {
    assert!(ticket.starts_with("v1."), "direct tickets use version 1");
    let payload = ticket.split('.').nth(1).expect("ticket payload");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .expect("decode ticket payload");
    serde_json::from_slice(&bytes).expect("parse ticket payload")
}

#[cfg(feature = "ui")]
fn published_ticket_payload(ticket: &str) -> WorkbenchTicketV2 {
    assert!(ticket.starts_with("v2."), "published tickets use version 2");
    let payload = ticket.split('.').nth(1).expect("ticket payload");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .expect("decode ticket payload");
    serde_json::from_slice(&bytes).expect("parse ticket payload")
}

#[cfg(feature = "ui")]
fn test_pairing_grant(
    selector: String,
    payload: &WorkbenchTicketV2,
    workspace_root: &Path,
    capabilities: &[String],
    created_at: u64,
    last_used_at: u64,
) -> WorkbenchPairingGrantV1 {
    WorkbenchPairingGrantV1 {
        credential_digest: session_credential_digest(&format!("{selector}-secret")),
        selector,
        project_id: payload.project_id.clone(),
        workspace_key: payload.workspace_key.clone(),
        workspace_root: workspace_root.to_path_buf(),
        launch_mode: WorkbenchLaunchMode::Published,
        project_instance_id: payload.project_instance_id.clone(),
        canonical_origin: payload.canonical_origin.clone(),
        capabilities: capabilities.to_vec(),
        created_at,
        last_used_at,
        idle_expires_at: last_used_at.saturating_add(PAIRING_IDLE_LIFETIME.as_secs()),
        absolute_expires_at: created_at.saturating_add(PAIRING_ABSOLUTE_LIFETIME.as_secs()),
        nickname: None,
        revoked_at: None,
        revocation_cause: None,
    }
}

#[cfg(feature = "ui")]
fn test_pairing_session(
    pairing: &WorkbenchPairingGrantV1,
    workspace_root: &Path,
    now: u64,
) -> WorkbenchSession {
    let secret = random_token().expect("session secret");
    WorkbenchSession {
        id: session_credential_digest(&secret),
        selector: random_token().expect("session selector"),
        project_id: pairing.project_id.clone(),
        workspace_key: pairing.workspace_key.clone(),
        workspace_root: workspace_root.to_path_buf(),
        capabilities: pairing.capabilities.clone(),
        entry: pairing.entry(),
        pairing_selector: Some(pairing.selector.clone()),
        created_at: now,
        last_activity: now,
        expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
        last_persisted_at: now,
    }
}

#[cfg(feature = "ui")]
fn launch_response_envelope(id: &str, launch: &WorkbenchLaunchResult) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: id.to_string(),
        status: Status::Ok,
        result: Some(serde_json::to_value(launch).expect("serialize launch")),
        error: None,
        ticket: None,
        steering: None,
        reminders: None,
        display: None,
        preview: None,
        effect: Some(Effect::Write),
        trace: None,
    }
}

#[cfg(feature = "ui")]
fn workbench_launch_request(id: &str, workspace_root: &Path) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: id.to_string(),
        op: Op::Call(crate::api::protocol::CallParams {
            address: crate::api::protocol::Address::Operation {
                path: vec!["workbench".to_string(), "launch".to_string()],
            },
            input: json!({}),
        }),
        workspace_root: Some(workspace_root.to_path_buf()),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    }
}

#[cfg(feature = "ui")]
fn assert_same_response(left: &ResponseEnvelope, right: &ResponseEnvelope) {
    assert_eq!(
        serde_json::to_value(left).expect("serialize left response"),
        serde_json::to_value(right).expect("serialize right response")
    );
}

#[cfg(feature = "ui")]
#[tokio::test]
async fn launch_replay_requires_live_pending_capability_host_and_workspace_registration() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));

    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let response = launch_response_envelope("launch-live", &launch);
    manager
        .retain_launch_replay("launch-live", &response)
        .expect("retain launch replay");
    assert_same_response(
        &manager
            .replay_launch_response("launch-live")
            .expect("current launch replays"),
        &response,
    );

    let (_, ticket) = launch_parts(&launch);
    let signing_secret = manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .host
        .as_ref()
        .expect("live host")
        .secret;
    for path in [
        &manager.inner.host_record_path,
        &manager.inner.authorization_store_path,
        &manager.inner.workspace_store_path,
    ] {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        assert!(
            !bytes
                .windows(ticket.len())
                .any(|window| window == ticket.as_bytes()),
            "launch bearer persisted in {}",
            path.display()
        );
        assert!(
            !bytes
                .windows(signing_secret.len())
                .any(|window| window == signing_secret),
            "launch signing material persisted in {}",
            path.display()
        );
    }
    manager
        .inner
        .redeem_ticket(ticket)
        .expect("consume launch capability");
    assert!(manager.replay_launch_response("launch-live").is_none());

    let expiring = manager
        .launch(&fixture.root)
        .expect("launch expiring ticket");
    let expiring_response = launch_response_envelope("launch-expired", &expiring);
    manager
        .retain_launch_replay("launch-expired", &expiring_response)
        .expect("retain expiring replay");
    let payload = ticket_payload(launch_parts(&expiring).1);
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .pending_capabilities
        .get_mut(&payload.capability_id)
        .expect("pending capability")
        .expires_at = unix_seconds().saturating_sub(1);
    assert!(manager.replay_launch_response("launch-expired").is_none());

    let host_bound = manager
        .launch(&fixture.root)
        .expect("launch host-bound ticket");
    let host_response = launch_response_envelope("launch-host", &host_bound);
    manager
        .retain_launch_replay("launch-host", &host_response)
        .expect("retain host-bound replay");
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .host
        .as_mut()
        .expect("live host")
        .generation += 1;
    assert!(manager.replay_launch_response("launch-host").is_none());

    let workspace_bound = manager
        .launch(&fixture.root)
        .expect("launch workspace-bound ticket");
    let workspace_response = launch_response_envelope("launch-workspace", &workspace_bound);
    manager
        .retain_launch_replay("launch-workspace", &workspace_response)
        .expect("retain workspace-bound replay");
    let generation_updated = manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .workspace_registration_generations
        .get_mut(&workspace_bound.workspace.key)
        .is_some_and(|generation| {
            *generation = generation.saturating_add(1);
            true
        });
    assert!(
        generation_updated,
        "workspace registration has a generation"
    );
    assert!(manager.replay_launch_response("launch-workspace").is_none());

    let cache_bound = manager
        .launch(&fixture.root)
        .expect("launch cache-bound ticket");
    let cache_response = launch_response_envelope("launch-cache-identity", &cache_bound);
    manager
        .retain_launch_replay("launch-cache-identity", &cache_response)
        .expect("retain cache-bound replay");
    assert!(
        manager
            .replay_launch_response_with_before_relock("launch-cache-identity", || {
                let mut state = manager.inner.state.lock().expect("workbench state");
                let replacement = Arc::new(
                    (**state
                        .launch_replays
                        .get("launch-cache-identity")
                        .expect("cached launch replay"))
                    .clone(),
                );
                state
                    .launch_replays
                    .insert("launch-cache-identity".to_string(), replacement);
            })
            .is_none(),
        "replay must reject a cache entry replaced during external validation"
    );
    assert_same_response(
        &manager
            .replay_launch_response("launch-cache-identity")
            .expect("replacement replay remains available"),
        &cache_response,
    );

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test]
async fn launch_replay_retention_uses_issuance_time_host_and_workspace_identity() {
    let direct_fixture = fixture();
    let manager = test_manager(Arc::clone(&direct_fixture.project));

    let host_bound = manager
        .launch(&direct_fixture.root)
        .expect("launch host race fixture");
    let host_response = launch_response_envelope("launch-host-race", &host_bound);
    let (_, host_ticket) = launch_parts(&host_bound);
    let original_host_generation = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let host = state.host.as_mut().expect("live host");
        let original = host.generation;
        host.generation = host.generation.saturating_add(1);
        original
    };
    assert_eq!(
        manager
            .inner
            .redeem_ticket(host_ticket)
            .expect_err("host replacement before redemption must fail closed"),
        TicketExchangeError::Invalid
    );
    let host_error = manager
        .retain_launch_replay("launch-host-race", &host_response)
        .expect_err("host replacement before retention must fail closed");
    assert!(
        host_error.to_string().contains("not pending"),
        "{host_error:#}"
    );
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .host
        .as_mut()
        .expect("live host")
        .generation = original_host_generation;

    let workspace_bound = manager
        .launch(&direct_fixture.root)
        .expect("launch workspace race fixture");
    let workspace_response = launch_response_envelope("launch-workspace-race", &workspace_bound);
    let (_, workspace_ticket) = launch_parts(&workspace_bound);
    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let key = workspace_bound.workspace.key.clone();
        let registration = state
            .workspaces_by_key
            .remove(&key)
            .expect("issued workspace registration");
        state.workspaces_by_root.remove(&registration.root);
        state.workspace_registration_generations.remove(&key);
        state.next_workspace_registration_generation = state
            .next_workspace_registration_generation
            .saturating_add(1);
        let replacement_generation = state.next_workspace_registration_generation;
        state
            .workspace_registration_generations
            .insert(key.clone(), replacement_generation);
        state
            .workspaces_by_root
            .insert(registration.root.clone(), key.clone());
        state.workspaces_by_key.insert(key, registration);
    }
    assert_eq!(
        manager
            .inner
            .redeem_ticket(workspace_ticket)
            .expect_err("remove and re-register before redemption must fail closed"),
        TicketExchangeError::Invalid
    );
    let workspace_error = manager
        .retain_launch_replay("launch-workspace-race", &workspace_response)
        .expect_err("remove and re-register before retention must fail closed");
    assert!(
        workspace_error.to_string().contains("not pending"),
        "{workspace_error:#}"
    );
    manager.shutdown().await;

    let published_fixture = fixture();
    let published = test_manager(Arc::clone(&published_fixture.project));
    use_test_published_entries(&published);
    let published_launch = published
        .launch(&published_fixture.root)
        .expect("launch published enrollment race fixture");
    let (_, published_ticket) = launch_parts(&published_launch);
    let payload = published_ticket_payload(published_ticket);
    let published_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key.clone(),
    )
    .expect("published entry");
    {
        let mut state = published.inner.state.lock().expect("workbench state");
        let generation = state
            .workspace_registration_generations
            .get_mut(&payload.workspace_key)
            .expect("published workspace registration generation");
        *generation = generation.saturating_add(1);
    }
    assert_eq!(
        published
            .inner
            .enroll_pairing(published_ticket, None, &published_entry)
            .expect_err("registration replacement before enrollment must fail closed"),
        PairingExchangeError::Invalid
    );
    let published_response =
        launch_response_envelope("launch-published-workspace-race", &published_launch);
    assert!(
        published
            .retain_launch_replay("launch-published-workspace-race", &published_response)
            .is_err(),
        "registration replacement before published replay retention must fail closed"
    );
    published.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test]
async fn launch_replay_is_fenced_and_cleared_by_host_shutdown() {
    let direct_fixture = fixture();
    let manager = test_manager(Arc::clone(&direct_fixture.project));

    let rejected = manager
        .launch(&direct_fixture.root)
        .expect("launch shutdown-retention fixture");
    let rejected_response = launch_response_envelope("launch-retain-shutdown", &rejected);
    manager.inner.shutting_down.store(true, Ordering::Release);
    assert!(
        manager
            .retain_launch_replay("launch-retain-shutdown", &rejected_response)
            .is_err(),
        "shutdown rejects newly retained replay authority"
    );
    manager.inner.shutting_down.store(false, Ordering::Release);

    let direct = manager
        .launch(&direct_fixture.root)
        .expect("launch direct shutdown fixture");
    let direct_response = launch_response_envelope("launch-direct-shutdown", &direct);
    manager
        .retain_launch_replay("launch-direct-shutdown", &direct_response)
        .expect("retain direct replay");
    assert!(
        manager
            .replay_launch_response_with_before_relock("launch-direct-shutdown", || {
                manager.inner.shutting_down.store(true, Ordering::Release);
            })
            .is_none(),
        "direct replay rechecks shutdown before returning"
    );
    manager.inner.shutting_down.store(false, Ordering::Release);

    let retained = manager
        .launch(&direct_fixture.root)
        .expect("launch shutdown-clear fixture");
    let retained_response = launch_response_envelope("launch-shutdown-clear", &retained);
    manager
        .retain_launch_replay("launch-shutdown-clear", &retained_response)
        .expect("retain replay before shutdown");
    manager.shutdown().await;
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("shutdown workbench state")
            .launch_replays
            .is_empty(),
        "shutdown clears bearer-bearing replay responses"
    );

    let published_fixture = fixture();
    let published = test_manager(Arc::clone(&published_fixture.project));
    let provider = Arc::new(ReplayAuthorityPublishedEntryProvider {
        current: AtomicBool::new(true),
        invalidate_before_return: AtomicBool::new(false),
    });
    published.set_entry_provider(provider);
    let launch = published
        .launch(&published_fixture.root)
        .expect("launch published shutdown fixture");
    let response = launch_response_envelope("launch-published-shutdown", &launch);
    published
        .retain_launch_replay("launch-published-shutdown", &response)
        .expect("retain published replay");
    published.inner.shutting_down.store(true, Ordering::Release);
    assert!(
        published
            .replay_launch_response("launch-published-shutdown")
            .is_none(),
        "published replay is fenced while the host shuts down"
    );
    published
        .inner
        .shutting_down
        .store(false, Ordering::Release);
    published.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test]
async fn published_launch_replay_requires_current_entry_and_publication_authority() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(ReplayAuthorityPublishedEntryProvider {
        current: AtomicBool::new(true),
        invalidate_before_return: AtomicBool::new(false),
    });
    manager.set_entry_provider(provider.clone());

    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let response = launch_response_envelope("launch-published", &launch);
    manager
        .retain_launch_replay("launch-published", &response)
        .expect("retain published replay");
    assert_same_response(
        &manager
            .replay_launch_response("launch-published")
            .expect("published authority is current"),
        &response,
    );

    let authority_gap = manager
        .launch(&fixture.root)
        .expect("launch published authority-gap fixture");
    let authority_gap_response =
        launch_response_envelope("launch-published-authority-gap", &authority_gap);
    manager
        .retain_launch_replay("launch-published-authority-gap", &authority_gap_response)
        .expect("retain published authority-gap replay");
    assert!(
        manager
            .replay_launch_response_with_before_relock("launch-published-authority-gap", || {
                provider.current.store(false, Ordering::Release)
            },)
            .is_none(),
        "published replay rechecks provider authority after external validation"
    );
    provider.current.store(true, Ordering::Release);

    let return_gap = manager
        .launch(&fixture.root)
        .expect("launch published return-gap fixture");
    let return_gap_response = launch_response_envelope("launch-published-return-gap", &return_gap);
    manager
        .retain_launch_replay("launch-published-return-gap", &return_gap_response)
        .expect("retain published return-gap replay");
    provider
        .invalidate_before_return
        .store(true, Ordering::Release);
    assert!(
        manager
            .replay_launch_response("launch-published-return-gap")
            .is_none(),
        "published replay rejects authority lost after final manager validation"
    );
    provider.current.store(true, Ordering::Release);

    let mut origin_mismatch = launch_response_envelope("launch-origin-mismatch", &launch);
    let (_, ticket) = launch_parts(&launch);
    origin_mismatch.result.as_mut().expect("launch result")["url"] =
        json!(format!("https://wrong-origin.localhost/#ticket={ticket}"));
    let error = manager
        .retain_launch_replay("launch-origin-mismatch", &origin_mismatch)
        .expect_err("response origin must match the verified ticket entry");
    assert!(error.to_string().contains("origin"), "{error:#}");
    assert!(
        manager
            .replay_launch_response("launch-origin-mismatch")
            .is_none()
    );

    provider.current.store(false, Ordering::Release);
    assert!(manager.replay_launch_response("launch-published").is_none());

    let entry_mismatch = manager
        .launch(&fixture.root)
        .expect("launch for entry mismatch");
    let mismatch_response = launch_response_envelope("launch-entry", &entry_mismatch);
    manager
        .retain_launch_replay("launch-entry", &mismatch_response)
        .expect("retain entry replay");
    provider.current.store(true, Ordering::Release);
    let origin = entry_mismatch
        .url
        .split_once("/#ticket=")
        .expect("published URL")
        .0;
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .origin_bindings
        .remove(origin);
    assert!(manager.replay_launch_response("launch-entry").is_none());

    manager.shutdown().await;
}

#[test]
fn rust_snapshot_serialization_matches_the_cockpit_contract_fixture() {
    let summary = WorkbenchLaneSummary {
        id: "lane-fixture".to_string(),
        title: "Local workbench host".to_string(),
        state: "executing".to_string(),
        phase_id: "phase-fixture".to_string(),
        phase_title: "Workbench foundation".to_string(),
        phase_status: "in-progress".to_string(),
        phase_completed_at: None,
        focused_here: true,
    };
    let snapshot = WorkbenchSnapshot {
        kind: "workbench.snapshot",
        ok: true,
        schema_version: 4,
        observed_at: "2026-07-28T20:00:00.000Z".to_string(),
        revision: 7,
        project: WorkbenchProjectIdentity {
            id: "project-fixture".to_string(),
        },
        daemon: WorkbenchDaemonIdentity {
            instance_id: "daemon-fixture".to_string(),
        },
        workspace: WorkbenchSnapshotWorkspace {
            key: "workspace-fixture".to_string(),
            label: "main".to_string(),
            branch: Some("main".to_string()),
            head: Some("0123456789abcdef".to_string()),
            detached: false,
            dirty: true,
        },
        project_workspaces: vec![
            WorkbenchProjectWorkspaceSummary {
                key: "workspace-fixture".to_string(),
                label: "main".to_string(),
                current: true,
                availability: "live".to_string(),
                observed_at: Some("2026-07-28T20:00:00Z".to_string()),
                branch: Some("main".to_string()),
                head: Some("0123456789abcdef".to_string()),
                detached: false,
                dirty: Some(true),
                focused_lane: Some(WorkbenchWorkspaceLaneSummary {
                    id: "lane-fixture".to_string(),
                    title: "Local workbench host".to_string(),
                    state: "executing".to_string(),
                    phase_id: "phase-fixture".to_string(),
                    phase_title: "Workbench foundation".to_string(),
                    phase_status: "in-progress".to_string(),
                }),
                active_phase: Some(WorkbenchWorkspacePhaseSummary {
                    id: "phase-fixture".to_string(),
                    title: "Workbench foundation".to_string(),
                    status: "in-progress".to_string(),
                }),
            },
            WorkbenchProjectWorkspaceSummary {
                key: "workspace-sibling".to_string(),
                label: "feature/dashboard".to_string(),
                current: false,
                availability: "stale".to_string(),
                observed_at: Some("2026-07-28T19:55:00Z".to_string()),
                branch: Some("feature/dashboard".to_string()),
                head: Some("fedcba9876543210".to_string()),
                detached: false,
                dirty: None,
                focused_lane: None,
                active_phase: None,
            },
        ],
        lanes: vec![summary.clone()],
        focused_lane: Some(WorkbenchLaneDetails {
            summary,
            intent: "Build the host and launch substrate".to_string(),
            created_at: "2026-07-28T19:00:00Z".to_string(),
            updated_at: "2026-07-28T19:30:00Z".to_string(),
        }),
        phase: Some(WorkbenchPhase {
            id: "phase-fixture".to_string(),
            title: "Workbench foundation".to_string(),
            status: "in-progress".to_string(),
            planning_available: true,
            goals: vec![WorkbenchGoal {
                id: "host-goal".to_string(),
                title: "Establish local host and launch".to_string(),
                status: "in-progress".to_string(),
                outcome: None,
                outcome_truncated: false,
                tasks: vec![WorkbenchTask {
                    id: "implement-host".to_string(),
                    title: "Implement host".to_string(),
                    status: "in-progress".to_string(),
                    outcome: None,
                    outcome_truncated: false,
                    progress: vec![WorkbenchTaskProgress {
                        message: "Captured browser evidence.".to_string(),
                        created_at: "2026-07-28T19:45:00Z".to_string(),
                    }],
                    progress_truncated: false,
                }],
            }],
        }),
        between_phases_context: None,
        steering: WorkbenchSteering {
            situation: "The local host implementation is active.".to_string(),
            next_actions: vec![WorkbenchSuggestedAction {
                label: "Continue implementation".to_string(),
                command: "task log host-goal::implement-host --message <message>".to_string(),
                rationale: "Keep the active task current.".to_string(),
                intent: "record".to_string(),
                confidence: Some(0.95),
            }],
        },
        diagnostics: vec![],
    };
    let fixture: JsonValue = serde_json::from_str(include_str!(
        "../../../../packages/exosuit-cockpit/src/lib/workbench-snapshot.v4.json"
    ))
    .expect("parse cockpit snapshot fixture");

    assert_eq!(
        serde_json::to_value(snapshot).expect("serialize Rust snapshot"),
        fixture
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn project_workspace_projection_is_path_free_fresh_and_focus_preserving() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("project-workspace-sibling");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workspace-sibling",
            linked.to_str().expect("linked path"),
        ],
    );
    fs::write(linked.join("sibling-dirty.txt"), "dirty\n").expect("dirty sibling worktree");

    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Workspace Faces", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Workspace Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    let current_lane = writer
        .add_workbench_lane("Current workspace", "Work in the primary checkout", &phase)
        .expect("add current lane");
    let sibling_lane = writer
        .add_workbench_lane("Sibling workspace", "Work in the linked checkout", &phase)
        .expect("add sibling lane");
    let current_root = fixture.root.canonicalize().expect("canonical current root");
    let sibling_root = linked.canonicalize().expect("canonical sibling root");
    writer
        .focus_workbench_lane(&current_root.to_string_lossy(), &current_lane, &phase)
        .expect("focus current lane");
    writer
        .focus_workbench_lane(&sibling_root.to_string_lossy(), &sibling_lane, &phase)
        .expect("focus sibling lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let legacy_sibling_key = "w".repeat(43);
    let legacy_selector = "s".repeat(43);
    let legacy_credential_digest = "a".repeat(64);
    let now = unix_seconds();
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .session_grants
        .insert(
            legacy_credential_digest.clone(),
            WorkbenchSessionGrantV1 {
                credential_digest: legacy_credential_digest,
                selector: legacy_selector,
                project_id: fixture.project.id.to_string(),
                workspace_key: legacy_sibling_key.clone(),
                workspace_root: sibling_root.clone(),
                capabilities: vec!["workbench.snapshot".to_string()],
                entry: None,
                pairing_selector: None,
                created_at: now,
                last_activity: now,
                expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
            },
        );
    manager
        .inner
        .persist_session_store()
        .expect("persist legacy sibling session grant");
    let discovered = manager
        .snapshot(&current_root)
        .expect("discover sibling workspace");
    let discovered_sibling = discovered
        .project_workspaces
        .iter()
        .find(|workspace| !workspace.current)
        .expect("discovered sibling workspace summary");
    assert_eq!(discovered_sibling.key, legacy_sibling_key);
    assert_eq!(discovered_sibling.label, "workspace-sibling");
    assert_eq!(discovered_sibling.availability, "stale");
    assert!(discovered_sibling.observed_at.is_none());

    manager
        .observe_workspace(&sibling_root)
        .expect("observe sibling workspace");
    let snapshot = manager
        .snapshot(&current_root)
        .expect("read project snapshot");
    assert_eq!(snapshot.schema_version, 4);
    assert_eq!(snapshot.project_workspaces.len(), 2);

    let current = snapshot
        .project_workspaces
        .iter()
        .find(|workspace| workspace.current)
        .expect("current workspace summary");
    assert_eq!(current.key, snapshot.workspace.key);
    assert_eq!(current.availability, "live");
    assert_eq!(
        current.focused_lane.as_ref().map(|lane| lane.id.as_str()),
        Some(current_lane.as_str())
    );

    let sibling = snapshot
        .project_workspaces
        .iter()
        .find(|workspace| !workspace.current)
        .expect("sibling workspace summary");
    assert_eq!(sibling.label, "workspace-sibling");
    assert_eq!(sibling.availability, "live");
    assert_eq!(sibling.dirty, Some(true));
    assert_eq!(
        sibling.focused_lane.as_ref().map(|lane| lane.id.as_str()),
        Some(sibling_lane.as_str())
    );
    assert_eq!(
        sibling.active_phase.as_ref().map(|phase| phase.id.as_str()),
        Some(phase.as_str())
    );
    let sibling_key = sibling.key.clone();
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let published_launch = manager
        .launch(&sibling_root)
        .expect("launch published sibling workbench");
    assert_eq!(published_launch.workspace.key, sibling_key);
    let (published_origin, published_ticket) = launch_parts(&published_launch);
    let published_entry = WorkbenchEntryBinding::published(
        published_origin.to_string(),
        "locald-stable-project-instance".to_string(),
        sibling_key.clone(),
    )
    .expect("published sibling entry");
    let enrollment = manager
        .inner
        .enroll_pairing(published_ticket, None, &published_entry)
        .expect("enroll sibling pairing");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();
    manager
        .inner
        .resume_pairing(
            &pairing_selector,
            &pairing_secret,
            &"p".repeat(43),
            &published_entry,
        )
        .expect("create pairing-derived sibling session");
    let serialized = serde_json::to_string(&snapshot).expect("serialize project snapshot");
    assert!(!serialized.contains(&current_root.display().to_string()));
    assert!(!serialized.contains(&sibling_root.display().to_string()));

    let replacement = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "replacement-workbench-instance",
    );
    let restored = replacement
        .snapshot(&current_root)
        .expect("restore project workspace observations");
    assert_eq!(
        restored
            .project_workspaces
            .iter()
            .find(|workspace| workspace.label == "workspace-sibling")
            .map(|workspace| workspace.key.as_str()),
        Some(sibling_key.as_str())
    );
    replacement.shutdown().await;

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let sibling = state
            .workspaces_by_key
            .get_mut(&sibling_key)
            .expect("registered sibling workspace");
        sibling.observed_at = Some(
            unix_seconds()
                .saturating_sub(WORKSPACE_OBSERVATION_FRESH_LIFETIME.as_secs())
                .saturating_sub(1),
        );
    }
    let stale = manager
        .snapshot(&current_root)
        .expect("read stale workspace");
    assert_eq!(
        stale
            .project_workspaces
            .iter()
            .find(|workspace| workspace.key == sibling_key)
            .map(|workspace| workspace.availability.as_str()),
        Some("stale")
    );

    fs::remove_dir_all(&sibling_root).expect("remove sibling worktree directory");
    let unavailable = manager
        .snapshot(&current_root)
        .expect("read unavailable workspace");
    assert_eq!(
        unavailable
            .project_workspaces
            .iter()
            .find(|workspace| workspace.key == sibling_key)
            .map(|workspace| workspace.availability.as_str()),
        Some("unavailable")
    );

    run_git(&fixture.root, &["worktree", "prune", "--expire", "now"]);
    let pruned = manager
        .snapshot(&current_root)
        .expect("read pruned project");
    assert!(
        pruned
            .project_workspaces
            .iter()
            .all(|workspace| workspace.key != sibling_key),
        "Git worktree removal ends workspace retention"
    );
    let state = manager.inner.state.lock().expect("workbench state");
    assert!(
        state
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some()),
        "Git worktree removal revokes retained pairing authority"
    );
    assert!(
        state
            .session_grants
            .values()
            .all(|session| session.workspace_key != sibling_key)
    );
    assert!(
        state
            .sessions
            .values()
            .all(|session| session.workspace_key != sibling_key)
    );
    assert!(
        !state
            .workspace_registration_generations
            .contains_key(&sibling_key),
        "Git worktree removal drops its replay-authority generation"
    );
    drop(state);
    let store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read removed workspace authorization store"),
    )
    .expect("decode removed workspace authorization store");
    assert!(
        store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == pairing_selector)
            .is_some_and(|pairing| {
                pairing.revoked_at.is_some()
                    && pairing.revocation_cause
                        == Some(WorkbenchPairingRevocationCause::WorkspaceMissing)
            })
    );
    assert!(
        store
            .sessions
            .iter()
            .all(|session| session.workspace_key != sibling_key)
    );
    manager.shutdown().await;
}

#[test]
fn discovered_workspace_does_not_replace_a_concurrent_live_observation() {
    let root = PathBuf::from("/tmp/exo-workbench-concurrent-observation");
    let live = WorkspaceRegistration {
        key: "workspace-key".to_string(),
        root: root.clone(),
        label: "live-workspace".to_string(),
        branch: Some("wycats/live-workspace".to_string()),
        head: Some("0123456789abcdef".to_string()),
        dirty: Some(true),
        observed_at: Some(42),
        registered_at: 21,
    };
    let mut state = WorkbenchState::default();
    state
        .workspaces_by_root
        .insert(root.clone(), live.key.clone());
    state
        .workspaces_by_key
        .insert(live.key.clone(), live.clone());

    let inserted = insert_discovered_workspace(
        &mut state,
        WorkspaceRegistration {
            key: live.key.clone(),
            root,
            label: "discovered-workspace".to_string(),
            branch: Some("wycats/discovered-workspace".to_string()),
            head: Some("fedcba9876543210".to_string()),
            dirty: None,
            observed_at: None,
            registered_at: 84,
        },
    );

    assert!(!inserted);
    assert_eq!(state.workspaces_by_key.get(&live.key), Some(&live));
}

#[test]
fn project_workspace_registry_limit_keeps_current_and_fresh_observations() {
    let mut state = WorkbenchState::default();
    for index in 0..MAX_PROJECT_WORKSPACES {
        let key = format!("workspace-{index:03}");
        let root = PathBuf::from(format!("/tmp/exo-workbench-{index:03}"));
        state.workspaces_by_root.insert(root.clone(), key.clone());
        state
            .workspace_registration_generations
            .insert(key.clone(), index as u64);
        state.workspaces_by_key.insert(
            key.clone(),
            WorkspaceRegistration {
                key,
                root,
                label: format!("Workspace {index:03}"),
                branch: None,
                head: None,
                dirty: None,
                observed_at: Some(index as u64),
                registered_at: index as u64,
            },
        );
    }
    let current_key = "workspace-current".to_string();
    let current_root = PathBuf::from("/tmp/exo-workbench-current");
    state
        .workspaces_by_root
        .insert(current_root.clone(), current_key.clone());
    state
        .workspace_registration_generations
        .insert(current_key.clone(), MAX_PROJECT_WORKSPACES as u64);
    state.workspaces_by_key.insert(
        current_key.clone(),
        WorkspaceRegistration {
            key: current_key.clone(),
            root: current_root,
            label: "Current workspace".to_string(),
            branch: None,
            head: None,
            dirty: None,
            observed_at: None,
            registered_at: MAX_PROJECT_WORKSPACES as u64,
        },
    );
    let now = unix_seconds();
    state.pending_capabilities.insert(
        "pending-capability".to_string(),
        PendingCapability {
            workspace_key: "workspace-000".to_string(),
            workspace_root: PathBuf::from("/tmp/exo-workbench-000"),
            workspace_registration_generation: 0,
            entry: test_direct_entry(),
            host_generation: 1,
            expires_at: now.saturating_add(TICKET_LIFETIME.as_secs()),
        },
    );
    state.sessions.insert(
        "live-session".to_string(),
        WorkbenchSession {
            id: "live-session".to_string(),
            selector: "live-selector".to_string(),
            project_id: "project-fixture".to_string(),
            workspace_key: "workspace-001".to_string(),
            workspace_root: PathBuf::from("/tmp/exo-workbench-001"),
            capabilities: vec!["workbench.snapshot".to_string()],
            entry: test_direct_entry(),
            pairing_selector: None,
            created_at: now,
            last_activity: now,
            expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
            last_persisted_at: now,
        },
    );
    state.session_grants.insert(
        "live-grant".to_string(),
        WorkbenchSessionGrantV1 {
            credential_digest: "live-grant".to_string(),
            selector: "grant-selector".to_string(),
            project_id: "project-fixture".to_string(),
            workspace_key: "workspace-002".to_string(),
            workspace_root: PathBuf::from("/tmp/exo-workbench-002"),
            capabilities: vec!["workbench.snapshot".to_string()],
            entry: None,
            pairing_selector: None,
            created_at: now,
            last_activity: now,
            expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
        },
    );
    state.pairing_grants.insert(
        "live-pairing".to_string(),
        WorkbenchPairingGrantV1 {
            selector: "live-pairing".to_string(),
            credential_digest: session_credential_digest("pairing-secret"),
            project_id: "project-fixture".to_string(),
            workspace_key: "workspace-003".to_string(),
            workspace_root: PathBuf::from("/tmp/exo-workbench-003"),
            launch_mode: WorkbenchLaunchMode::Published,
            project_instance_id: "project-instance".to_string(),
            canonical_origin: "https://workbench.test.localhost".to_string(),
            capabilities: vec!["workbench.snapshot".to_string()],
            created_at: now,
            last_used_at: now,
            idle_expires_at: now.saturating_add(PAIRING_IDLE_LIFETIME.as_secs()),
            absolute_expires_at: now.saturating_add(PAIRING_ABSOLUTE_LIFETIME.as_secs()),
            nickname: None,
            revoked_at: None,
            revocation_cause: None,
        },
    );

    assert!(!retain_project_workspace_limit(&mut state, &current_key).is_empty());
    assert_eq!(state.workspaces_by_key.len(), MAX_PROJECT_WORKSPACES);
    assert_eq!(state.workspaces_by_root.len(), MAX_PROJECT_WORKSPACES);
    assert_eq!(
        state.workspace_registration_generations.len(),
        MAX_PROJECT_WORKSPACES
    );
    assert!(state.workspaces_by_key.contains_key(&current_key));
    assert!(
        !state.workspaces_by_key.contains_key("workspace-000"),
        "an issuance-invalid pending capability must not protect a stale workspace"
    );
    assert!(state.workspaces_by_key.contains_key("workspace-001"));
    assert!(state.workspaces_by_key.contains_key("workspace-002"));
    assert!(state.workspaces_by_key.contains_key("workspace-003"));
    assert!(state.workspaces_by_key.contains_key("workspace-127"));
    assert!(state.workspaces_by_key.contains_key("workspace-004"));
    assert!(
        !state
            .workspace_registration_generations
            .contains_key("workspace-000")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unborn_workspace_is_a_live_project_workspace_observation() {
    let temp = tempfile::tempdir().expect("create unborn workbench fixture");
    let root = temp.path().join("workspace");
    fs::create_dir(&root).expect("create unborn workspace");
    run_git(&root, &["init", "-b", "main"]);
    let project = Arc::new(Project::resolve(&root).expect("resolve unborn project"));
    fs::create_dir_all(
        project
            .db_path()
            .parent()
            .expect("project database has a parent"),
    )
    .expect("create project state root");
    drop(SqliteWriter::open(project.db_path()).expect("initialize project database"));

    let manager = test_manager(project);
    let snapshot = manager.snapshot(&root).expect("read unborn snapshot");
    assert_eq!(snapshot.workspace.branch.as_deref(), Some("main"));
    assert!(snapshot.workspace.head.is_none());
    assert_eq!(snapshot.project_workspaces.len(), 1);
    let workspace = &snapshot.project_workspaces[0];
    assert!(workspace.current);
    assert_eq!(workspace.availability, "live");
    assert_eq!(workspace.branch.as_deref(), Some("main"));
    assert!(workspace.head.is_none());
    assert!(workspace.dirty.is_some());
    assert!(workspace.observed_at.is_some());

    let now = unix_seconds();
    let credential_digest = "u".repeat(64);
    let selector = "unborn-session-selector";
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .session_grants
        .insert(
            credential_digest.clone(),
            WorkbenchSessionGrantV1 {
                credential_digest: credential_digest.clone(),
                selector: selector.to_string(),
                project_id: manager.inner.project.id.to_string(),
                workspace_key: snapshot.workspace.key.clone(),
                workspace_root: root.canonicalize().expect("canonical unborn workspace"),
                capabilities: vec!["workbench.snapshot".to_string()],
                entry: Some(test_direct_entry()),
                pairing_selector: None,
                created_at: now,
                last_activity: now,
                expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
            },
        );
    assert!(manager.inner.restore_session(selector, &credential_digest));
    let restored = manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .workspaces_by_key
        .get(&snapshot.workspace.key)
        .cloned()
        .expect("restored unborn workspace");
    assert_eq!(restored.branch.as_deref(), Some("main"));
    assert!(restored.head.is_none());
    assert!(restored.dirty.is_some());
    assert!(restored.observed_at.is_some());
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn current_submodule_workspace_is_retained_without_a_worktree_index_entry() {
    let temp = tempfile::tempdir().expect("create submodule workbench fixture");
    let source = temp.path().join("submodule-source");
    fs::create_dir(&source).expect("create submodule source");
    run_git(&source, &["init", "-b", "main"]);
    fs::write(source.join("README.md"), "# Submodule source\n").expect("write source");
    run_git(&source, &["add", "."]);
    run_git(
        &source,
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

    let parent = temp.path().join("parent");
    fs::create_dir(&parent).expect("create parent repository");
    run_git(&parent, &["init", "-b", "main"]);
    fs::write(parent.join("README.md"), "# Parent\n").expect("write parent");
    run_git(&parent, &["add", "."]);
    run_git(
        &parent,
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
    run_git(
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
    let project = Arc::new(Project::resolve(&child).expect("resolve submodule project"));
    assert!(
        !project
            .worktree_index()
            .is_some_and(|worktrees| worktrees.contains_key(&child))
    );
    fs::create_dir_all(
        project
            .db_path()
            .parent()
            .expect("project database has a parent"),
    )
    .expect("create project state root");
    drop(SqliteWriter::open(project.db_path()).expect("initialize project database"));

    let manager = test_manager(project);
    let snapshot = manager
        .snapshot(&child)
        .expect("read submodule workspace snapshot");
    assert_eq!(snapshot.project_workspaces.len(), 1);
    assert!(snapshot.project_workspaces[0].current);
    assert_eq!(snapshot.project_workspaces[0].availability, "live");
    assert_eq!(snapshot.project_workspaces[0].key, snapshot.workspace.key);
    manager.shutdown().await;
}

#[test]
fn rust_lane_inspection_serialization_matches_the_cockpit_contract_fixture() {
    let inspection = WorkbenchLaneInspection {
        kind: "workbench.lane_inspection",
        ok: true,
        schema_version: 2,
        observed_at: "2026-08-06T20:00:00.000Z".to_string(),
        revision: 9,
        project: WorkbenchProjectIdentity {
            id: "project-fixture".to_string(),
        },
        daemon: WorkbenchDaemonIdentity {
            instance_id: "daemon-fixture".to_string(),
        },
        workspace: WorkbenchSnapshotWorkspace {
            key: "workspace-fixture".to_string(),
            label: "main".to_string(),
            branch: Some("main".to_string()),
            head: Some("0123456789abcdef".to_string()),
            detached: false,
            dirty: true,
        },
        relationship: "historical".to_string(),
        can_focus_here: false,
        lane: WorkbenchLaneDetails {
            summary: WorkbenchLaneSummary {
                id: "lane-history".to_string(),
                title: "Completed cockpit foundation".to_string(),
                state: "executing".to_string(),
                phase_id: "phase-history".to_string(),
                phase_title: "Cockpit foundation".to_string(),
                phase_status: "completed".to_string(),
                phase_completed_at: Some("2026-08-05T20:00:00+00:00".to_string()),
                focused_here: false,
            },
            intent: "Establish the first useful lane workbench".to_string(),
            created_at: "2026-07-28T19:00:00Z".to_string(),
            updated_at: "2026-08-05T19:30:00Z".to_string(),
        },
        phase: WorkbenchPhase {
            id: "phase-history".to_string(),
            title: "Cockpit foundation".to_string(),
            status: "completed".to_string(),
            planning_available: false,
            goals: vec![WorkbenchGoal {
                id: "foundation-goal".to_string(),
                title: "Build the first cockpit".to_string(),
                status: "completed".to_string(),
                outcome: Some(
                    "The first lane-centered cockpit is available for dogfood.".to_string(),
                ),
                outcome_truncated: false,
                tasks: vec![WorkbenchTask {
                    id: "ship-foundation".to_string(),
                    title: "Ship the foundation".to_string(),
                    status: "completed".to_string(),
                    outcome: Some("The reviewed foundation landed cleanly.".to_string()),
                    outcome_truncated: false,
                    progress: vec![WorkbenchTaskProgress {
                        message: "Validated the final browser flow.".to_string(),
                        created_at: "2026-08-05T18:45:00Z".to_string(),
                    }],
                    progress_truncated: false,
                }],
            }],
        },
    };
    let fixture: JsonValue = serde_json::from_str(include_str!(
        "../../../../packages/exosuit-cockpit/src/lib/workbench-lane-inspection.v2.json"
    ))
    .expect("parse cockpit inspection fixture");

    assert_eq!(
        serde_json::to_value(inspection).expect("serialize Rust lane inspection"),
        fixture
    );
}

#[cfg(not(feature = "ui"))]
#[tokio::test(flavor = "multi_thread")]
async fn ui_disabled_launch_returns_the_stable_unavailable_error() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let error = manager
        .launch(&fixture.root)
        .expect_err("UI-disabled launch must fail");
    let failure = error
        .downcast_ref::<crate::failure::ExoFailure>()
        .expect("structured workbench failure");
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "workbench.ui_unavailable"
    );
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn launch_tickets_are_signed_one_time_and_runtime_local() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let first = manager.launch(&fixture.root).expect("launch workbench");
    let (origin, ticket) = launch_parts(&first);
    assert_eq!(first.schema_version, 2);
    assert_eq!(first.launch_mode, WorkbenchLaunchMode::DirectLoopback);
    assert_eq!(first.expires_in_seconds, 3_600);
    let payload = ticket_payload(ticket);
    assert_eq!(payload.expires_at - payload.issued_at, 3_600);
    assert!(!first.reused_host);
    assert_eq!(first.project.id, fixture.project.id.as_str());
    assert!(
        !first
            .workspace
            .key
            .contains(fixture.root.to_string_lossy().as_ref())
    );

    let second = manager.launch(&fixture.root).expect("reuse workbench");
    assert!(second.reused_host);
    assert_eq!(launch_parts(&second).0, origin);
    assert_eq!(second.workspace.key, first.workspace.key);
    assert_ne!(launch_parts(&second).1, ticket);

    let record_text =
        fs::read_to_string(&manager.inner.host_record_path).expect("read workbench host record");
    assert!(!record_text.contains(ticket));
    assert!(!record_text.contains(&fixture.root.display().to_string()));
    assert!(!record_text.contains(&fixture.project.state_root.display().to_string()));

    let (session_secret, session) = manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem launch ticket");
    assert_eq!(session.project_id, fixture.project.id.as_str());
    assert_eq!(session.workspace_key, first.workspace.key);
    assert_eq!(
        manager.inner.redeem_ticket(ticket),
        Err(TicketExchangeError::Invalid)
    );

    assert!(
        manager
            .inner
            .session(&session.session_key, &session_secret)
            .is_some(),
        "the redeemed session is authenticated by its independent cookie secret"
    );
    let session_store = fs::read_to_string(&manager.inner.authorization_store_path)
        .expect("read workbench session store");
    assert!(!session_store.contains(&session_secret));
    assert!(session_store.contains(&session_credential_digest(&session_secret)));

    let mut tampered = launch_parts(&second).1.as_bytes().to_vec();
    let last = tampered.last_mut().expect("ticket has signature");
    *last = if *last == b'a' { b'b' } else { b'a' };
    assert_eq!(
        manager
            .inner
            .redeem_ticket(std::str::from_utf8(&tampered).expect("tampered ticket remains UTF-8")),
        Err(TicketExchangeError::Invalid)
    );

    manager.shutdown().await;
    let inactive_record: WorkbenchHostRecord = serde_json::from_slice(
        &fs::read(&manager.inner.host_record_path).expect("read inactive host record"),
    )
    .expect("decode inactive host record");
    assert_eq!(inactive_record.origin, origin);
    assert!(
        !inactive_record.server_task_alive,
        "shutdown retains the compatible origin without claiming a live host"
    );
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn direct_ticket_redemption_waits_for_authorization_transition() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let launch = manager
        .launch(&fixture.root)
        .expect("launch direct workbench");
    let ticket = launch_parts(&launch).1.to_string();
    let gate = manager
        .inner
        .authorization_store_gate
        .lock()
        .expect("hold authorization transition");
    let redeeming_manager = manager.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).expect("report redemption start");
        tx.send(redeeming_manager.inner.redeem_ticket(&ticket))
            .expect("report ticket redemption");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("redemption worker starts");
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
        "direct redemption must wait for the authorization transition"
    );
    drop(gate);
    let (session_secret, session) = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("ticket redemption resumes")
        .expect("redeem direct ticket");
    worker.join().expect("join ticket redemption worker");
    assert!(
        manager
            .inner
            .session(&session.session_key, &session_secret)
            .is_some(),
        "the serialized direct session remains usable"
    );
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn launch_replay_rejects_an_exact_linked_worktree_replaced_by_another_root() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("launch-replay-linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-launch-replay-linked",
            linked.to_str().expect("linked path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    let launch = manager
        .launch(&linked_root)
        .expect("launch linked worktree");
    let response = launch_response_envelope("launch-removed-worktree", &launch);
    manager
        .retain_launch_replay("launch-removed-worktree", &response)
        .expect("retain linked-worktree launch replay");
    assert_same_response(
        &manager
            .replay_launch_response("launch-removed-worktree")
            .expect("linked worktree initially replays"),
        &response,
    );

    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked path"),
        ],
    );
    std::os::unix::fs::symlink(&fixture.root, &linked)
        .expect("replace linked worktree with primary-worktree symlink");

    assert!(
        manager
            .replay_launch_response("launch-removed-worktree")
            .is_none(),
        "a substituted exact workspace must invalidate launch replay"
    );
    assert!(
        !manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .launch_replays
            .contains_key("launch-removed-worktree"),
        "a failed workspace validation must discard only that replay"
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn unexpected_host_stop_reuses_listener_and_rebinds_publications() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
    manager.set_entry_provider(provider.clone());

    let first = manager
        .launch(&fixture.root)
        .expect("launch initial published workbench");
    let private_origin = manager.host_status().expect("initial host status").origin;
    let (generation, task) = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let host = state.host.as_mut().expect("bound workbench host");
        (
            host.generation,
            host.task.take().expect("running workbench server task"),
        )
    };
    task.abort();
    let _ = task.await;
    manager
        .inner
        .server_stopped(generation, Some("injected server stop".to_string()));

    let restarted = manager
        .launch(&fixture.root)
        .expect("restart published workbench on retained listener");
    assert_eq!(launch_parts(&restarted).0, launch_parts(&first).0);
    assert_eq!(
        manager.host_status().expect("restarted host status").origin,
        private_origin,
        "an unexpected stop must retain the exact private listener origin"
    );
    assert!(
        provider.rebinds.load(Ordering::Acquire) >= 2,
        "initial start and restart must both rebind the published authority"
    );

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_and_resume_are_durable_exact_and_origin_bound() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (published_origin, ticket) = launch_parts(&launch);
    assert!(published_origin.starts_with("https://workbench-"));
    let published_ticket = published_ticket_payload(ticket);
    assert_eq!(published_ticket.version, 2);
    assert_eq!(published_ticket.entry_mode, WorkbenchLaunchMode::Published);
    assert_eq!(published_ticket.canonical_origin, published_origin);
    assert_eq!(published_ticket.workspace_key, launch.workspace.key);
    assert!(published_ticket.project_instance_id.starts_with("locald-"));
    let published_host = expected_host_from_origin(published_origin).expect("published host");
    let private_origin = manager.host_status().expect("private host").origin;

    let enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("enroll published browser");
    assert_eq!(enrollment.status, 200, "{enrollment:?}");
    let pairing_cookie = response_cookie(&enrollment, PAIRING_COOKIE_NAME)
        .expect("pairing cookie")
        .to_string();
    assert!(
        enrollment
            .set_cookies
            .iter()
            .any(|cookie| cookie.starts_with(SESSION_COOKIE_PREFIX)),
        "enrollment also creates the first active session"
    );
    let pairing_secret = pairing_cookie
        .split('.')
        .nth(2)
        .expect("pairing cookie secret");
    let persisted = fs::read_to_string(&manager.inner.authorization_store_path)
        .expect("read authorization store");
    let store: WorkbenchAuthorizationStoreV2 =
        serde_json::from_str(&persisted).expect("decode authorization store");
    assert_eq!(store.schema_version, 2);
    assert_eq!(store.pairings.len(), 1);
    assert_eq!(store.sessions.len(), 1);
    assert!(!persisted.contains(pairing_secret));

    let request_id = "r".repeat(43);
    let resume = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": request_id }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume paired browser");
    assert_eq!(resume.status, 200, "{resume:?}");
    let resumed_session_cookie = response_cookie_prefix(&resume, SESSION_COOKIE_PREFIX)
        .expect("resumed session cookie")
        .to_string();

    let replay = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": request_id }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("replay paired resume");
    assert_eq!(replay.status, 200, "{replay:?}");
    assert_eq!(
        response_cookie_prefix(&replay, SESSION_COOKIE_PREFIX),
        Some(resumed_session_cookie.as_str()),
        "same pairing and request ID reproduce the exact session credential"
    );

    let health = raw_http_via(
        &private_origin,
        published_host,
        "GET",
        "/api/health",
        None,
        None,
        None,
    )
    .await
    .expect("read published health");
    assert_eq!(health.status, 204);
    assert!(health.body.is_empty());

    let store_bytes = fs::read(&manager.inner.authorization_store_path).expect("read replay store");
    let store_json: serde_json::Value =
        serde_json::from_slice(&store_bytes).expect("decode replay store JSON");
    let persisted_outcome = &store_json["resume_outcomes"][0];
    assert!(persisted_outcome.get("result").is_none());
    assert!(persisted_outcome.get("session_selector").is_some());
    assert!(persisted_outcome.get("session_credential_digest").is_some());
    assert!(persisted_outcome.get("session_expires_at").is_some());
    let store: WorkbenchAuthorizationStoreV2 =
        serde_json::from_slice(&store_bytes).expect("decode replay store");
    assert_eq!(store.pairings.len(), 1);
    assert_eq!(store.resume_outcomes.len(), 1);
    assert_eq!(
        store
            .sessions
            .iter()
            .filter(|session| session.pairing_selector.is_some())
            .count(),
        2,
        "enrollment and one unique resume create two active sessions"
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_evicts_the_oldest_inactive_workspace_pairing() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let request_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin.clone(),
        payload.project_instance_id.clone(),
        payload.workspace_key.clone(),
    )
    .expect("published request entry");
    let capabilities = upgraded_session_capabilities(payload.capabilities.clone());
    let now = unix_seconds();
    let selectors = (b'a'..=b'h')
        .map(|byte| char::from(byte).to_string().repeat(43))
        .collect::<Vec<_>>();
    let oldest_selector = selectors[0].clone();
    let survivor_selectors = selectors[1..].to_vec();
    let terminal_request_id = "r".repeat(43);
    let outcome_key = WorkbenchResumeOutcomeKey {
        pairing_selector: oldest_selector.clone(),
        request_id: terminal_request_id.clone(),
    };

    let store = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        for (index, selector) in selectors.iter().enumerate() {
            let pairing = test_pairing_grant(
                selector.clone(),
                &payload,
                &fixture.root,
                &capabilities,
                now.saturating_sub(200 - index as u64),
                now.saturating_sub(100 - index as u64),
            );
            state.pairing_grants.insert(selector.clone(), pairing);
        }
        state.resume_outcomes.insert(
            outcome_key.clone(),
            terminal_resume_outcome(&outcome_key, WorkbenchResumeTerminalErrorV1::Invalid, now),
        );
        let orphaned_memory_session = test_pairing_session(
            state
                .pairing_grants
                .get(&oldest_selector)
                .expect("oldest pairing"),
            &fixture.root,
            now,
        );
        state
            .sessions
            .insert(orphaned_memory_session.id.clone(), orphaned_memory_session);
        authorization_store_from_collections(
            fixture.project.id.as_str(),
            &state.session_grants,
            &state.pairing_grants,
            &state.resume_outcomes,
        )
    };
    write_authorization_store(&manager.inner.authorization_store_path, &store)
        .expect("persist capacity fixture");

    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &request_entry)
        .expect("evict oldest inactive pairing");
    let enrolled_selector = enrollment
        .pairing_cookie
        .split('.')
        .nth(1)
        .expect("enrolled pairing selector");
    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(
        state
            .pairing_grants
            .values()
            .filter(|pairing| pairing.is_live(now))
            .count(),
        MAX_ACTIVE_PAIRINGS_PER_WORKSPACE
    );
    assert_eq!(
        state
            .pairing_grants
            .get(&oldest_selector)
            .and_then(|pairing| pairing.revocation_cause),
        Some(WorkbenchPairingRevocationCause::Replaced)
    );
    assert!(state.pairing_grants.contains_key(enrolled_selector));
    assert!(
        survivor_selectors
            .iter()
            .all(|selector| state.pairing_grants.contains_key(selector)),
        "newer inactive pairings remain retained"
    );
    assert_eq!(
        state
            .resume_outcomes
            .get(&outcome_key)
            .and_then(WorkbenchResumeOutcomeV1::terminal_error),
        Some(PairingExchangeError::Invalid)
    );
    assert!(
        state.sessions.values().all(|session| {
            session.pairing_selector.as_deref() != Some(oldest_selector.as_str())
        }),
        "in-memory sessions are removed only after the store write succeeds"
    );
    drop(state);

    let persisted: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path).expect("read authorization store"),
    )
    .expect("decode authorization store");
    assert_eq!(
        persisted
            .pairings
            .iter()
            .filter(|pairing| pairing.is_live(now))
            .count(),
        MAX_ACTIVE_PAIRINGS_PER_WORKSPACE
    );
    assert_eq!(
        persisted
            .pairings
            .iter()
            .find(|pairing| pairing.selector == oldest_selector)
            .and_then(|pairing| pairing.revocation_cause),
        Some(WorkbenchPairingRevocationCause::Replaced)
    );
    assert_eq!(
        persisted
            .resume_outcomes
            .iter()
            .find(|outcome| outcome.pairing_selector == oldest_selector)
            .and_then(WorkbenchResumeOutcomeV1::terminal_error),
        Some(PairingExchangeError::Invalid)
    );
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &oldest_selector,
                &format!("{oldest_selector}-secret"),
                &terminal_request_id,
                &request_entry,
            )
            .expect_err("evicted pairing replays its terminal resume result"),
        PairingExchangeError::Invalid
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_reuses_a_compatible_pairing_at_workspace_capacity() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let request_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin.clone(),
        payload.project_instance_id.clone(),
        payload.workspace_key.clone(),
    )
    .expect("published request entry");
    let capabilities = upgraded_session_capabilities(payload.capabilities.clone());
    let now = unix_seconds();
    let selectors = (b'a'..=b'h')
        .map(|byte| char::from(byte).to_string().repeat(43))
        .collect::<Vec<_>>();
    let reusable_selector = selectors[3].clone();
    let reusable_secret = format!("{reusable_selector}-secret");

    let store = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        for (index, selector) in selectors.iter().enumerate() {
            let pairing = test_pairing_grant(
                selector.clone(),
                &payload,
                &fixture.root,
                &capabilities,
                now.saturating_sub(200 - index as u64),
                now.saturating_sub(100 - index as u64),
            );
            state.pairing_grants.insert(selector.clone(), pairing);
        }
        authorization_store_from_collections(
            fixture.project.id.as_str(),
            &state.session_grants,
            &state.pairing_grants,
            &state.resume_outcomes,
        )
    };
    write_authorization_store(&manager.inner.authorization_store_path, &store)
        .expect("persist capacity fixture");

    let enrollment = manager
        .inner
        .enroll_pairing(
            ticket,
            Some((&reusable_selector, &reusable_secret)),
            &request_entry,
        )
        .expect("reuse compatible pairing at capacity");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    assert_eq!(pairing_parts.next(), Some(reusable_selector.as_str()));
    assert_eq!(pairing_parts.next(), Some(reusable_secret.as_str()));

    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(
        state.pairing_grants.len(),
        MAX_ACTIVE_PAIRINGS_PER_WORKSPACE
    );
    assert!(
        selectors
            .iter()
            .all(|selector| state.pairing_grants.contains_key(selector)),
        "reusing a compatible pairing does not evict another workspace pairing"
    );
    assert_eq!(state.session_grants.len(), 1);
    assert!(state.session_grants.values().all(|session| {
        session.pairing_selector.as_deref() == Some(reusable_selector.as_str())
    }));
    drop(state);
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_at_project_capacity_does_not_evict_another_workspace() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let request_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin.clone(),
        payload.project_instance_id.clone(),
        payload.workspace_key.clone(),
    )
    .expect("published request entry");
    let capabilities = upgraded_session_capabilities(payload.capabilities.clone());
    let now = unix_seconds();
    let selectors = (0..MAX_ACTIVE_PAIRINGS)
        .map(|index| format!("{index:043}"))
        .collect::<Vec<_>>();

    let store = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        for (index, selector) in selectors.iter().enumerate() {
            let mut pairing = test_pairing_grant(
                selector.clone(),
                &payload,
                &fixture.root,
                &capabilities,
                now.saturating_sub(200 - index as u64),
                now.saturating_sub(100 - index as u64),
            );
            pairing.workspace_key = format!("other-workspace-{index}");
            state.pairing_grants.insert(selector.clone(), pairing);
        }
        authorization_store_from_collections(
            fixture.project.id.as_str(),
            &state.session_grants,
            &state.pairing_grants,
            &state.resume_outcomes,
        )
    };
    write_authorization_store(&manager.inner.authorization_store_path, &store)
        .expect("persist project capacity fixture");

    assert!(
        matches!(
            manager.inner.enroll_pairing(ticket, None, &request_entry),
            Err(PairingExchangeError::Limit)
        ),
        "the project-wide cap remains a hard boundary"
    );
    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(state.pairing_grants.len(), MAX_ACTIVE_PAIRINGS);
    assert!(
        selectors
            .iter()
            .all(|selector| state.pairing_grants.contains_key(selector)),
        "project capacity never evicts authority from another workspace"
    );
    assert!(
        state
            .pending_capabilities
            .contains_key(&payload.capability_id),
        "the capacity result leaves the enrollment ticket retryable"
    );
    drop(state);

    let persisted: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path).expect("read authorization store"),
    )
    .expect("decode authorization store");
    assert_eq!(persisted.pairings.len(), MAX_ACTIVE_PAIRINGS);
    assert!(
        selectors.iter().all(|selector| {
            persisted
                .pairings
                .iter()
                .any(|pairing| pairing.selector == *selector)
        }),
        "the persisted project authority is unchanged"
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_preserves_active_pairings_and_retries_after_inactivity() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let request_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin.clone(),
        payload.project_instance_id.clone(),
        payload.workspace_key.clone(),
    )
    .expect("published request entry");
    let capabilities = upgraded_session_capabilities(payload.capabilities.clone());
    let now = unix_seconds();
    let selectors = (b'a'..=b'h')
        .map(|byte| char::from(byte).to_string().repeat(43))
        .collect::<Vec<_>>();
    let oldest_selector = selectors[0].clone();

    let store = {
        let mut state = manager.inner.state.lock().expect("workbench state");
        for (index, selector) in selectors.iter().enumerate() {
            let pairing = test_pairing_grant(
                selector.clone(),
                &payload,
                &fixture.root,
                &capabilities,
                now.saturating_sub(200 - index as u64),
                now.saturating_sub(100 - index as u64),
            );
            let session = test_pairing_session(&pairing, &fixture.root, now);
            state
                .session_grants
                .insert(session.id.clone(), WorkbenchSessionGrantV1::from(&session));
            state.sessions.insert(session.id.clone(), session);
            state.pairing_grants.insert(selector.clone(), pairing);
        }
        authorization_store_from_collections(
            fixture.project.id.as_str(),
            &state.session_grants,
            &state.pairing_grants,
            &state.resume_outcomes,
        )
    };
    write_authorization_store(&manager.inner.authorization_store_path, &store)
        .expect("persist active capacity fixture");

    assert!(
        matches!(
            manager.inner.enroll_pairing(ticket, None, &request_entry),
            Err(PairingExchangeError::Limit)
        ),
        "a workspace whose pairings all have live persisted sessions remains protected"
    );
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pending_capabilities
            .contains_key(&payload.capability_id),
        "the capacity result leaves the enrollment ticket retryable"
    );

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let session_id = state
            .session_grants
            .iter()
            .find(|(_, session)| {
                session.pairing_selector.as_deref() == Some(oldest_selector.as_str())
            })
            .map(|(session_id, _)| session_id.clone())
            .expect("oldest pairing session");
        state
            .session_grants
            .get_mut(&session_id)
            .expect("persisted session")
            .expires_at = now;
        state
            .sessions
            .get_mut(&session_id)
            .expect("in-memory session")
            .expires_at = now;
    }

    manager
        .inner
        .enroll_pairing(ticket, None, &request_entry)
        .expect("retry after oldest pairing becomes inactive");
    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(
        state
            .pairing_grants
            .get(&oldest_selector)
            .and_then(|pairing| pairing.revocation_cause),
        Some(WorkbenchPairingRevocationCause::Replaced)
    );
    assert!(selectors[1..].iter().all(|selector| {
        state
            .pairing_grants
            .get(selector)
            .is_some_and(|pairing| pairing.is_live(now))
    }));
    assert_eq!(
        state
            .pairing_grants
            .values()
            .filter(|pairing| pairing.is_live(now))
            .count(),
        MAX_ACTIVE_PAIRINGS_PER_WORKSPACE
    );
    drop(state);
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[test]
fn inactive_pairing_lru_uses_creation_time_then_selector_as_tie_breakers() {
    let now = 1_000;
    let payload = WorkbenchTicketV2 {
        version: 2,
        capability_id: "c".repeat(43),
        instance_id: "test-instance".to_string(),
        project_id: "test-project".to_string(),
        workspace_key: "test-workspace".to_string(),
        entry_mode: WorkbenchLaunchMode::Published,
        project_instance_id: "test-project-instance".to_string(),
        canonical_origin: "https://workbench.test.localhost".to_string(),
        capabilities: vec!["workbench.snapshot".to_string()],
        issued_at: now,
        expires_at: now + 60,
    };
    let capabilities = upgraded_session_capabilities(payload.capabilities.clone());
    let later_selector = "z".repeat(43);
    let lexical_selector = "a".repeat(43);
    let mut pairings = HashMap::from([
        (
            later_selector.clone(),
            test_pairing_grant(
                later_selector.clone(),
                &payload,
                Path::new("/test/workspace"),
                &capabilities,
                100,
                200,
            ),
        ),
        (
            lexical_selector.clone(),
            test_pairing_grant(
                lexical_selector.clone(),
                &payload,
                Path::new("/test/workspace"),
                &capabilities,
                101,
                200,
            ),
        ),
    ]);
    assert_eq!(
        least_recently_used_inactive_pairing(
            &pairings,
            &HashMap::new(),
            &payload.workspace_key,
            None,
            now,
        ),
        Some(later_selector.clone()),
        "an earlier creation wins when last use ties"
    );

    pairings
        .get_mut(&later_selector)
        .expect("later lexical pairing")
        .created_at = 101;
    assert_eq!(
        least_recently_used_inactive_pairing(
            &pairings,
            &HashMap::new(),
            &payload.workspace_key,
            None,
            now,
        ),
        Some(lexical_selector),
        "selector byte order breaks an exact timestamp tie"
    );
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn published_enrollment_revalidates_the_exact_worktree_before_commit() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("enrollment-disappears");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-enrollment-disappears",
            linked.to_str().expect("linked worktree path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&linked_root)
        .expect("launch published linked worktree");
    let (published_origin, ticket) = launch_parts(&launch);
    let published_host = expected_host_from_origin(published_origin).expect("published host");
    let private_origin = manager.host_status().expect("private host").origin;

    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked worktree path"),
        ],
    );
    let enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("reject enrollment for removed worktree");
    assert_eq!(enrollment.status, 401, "{enrollment:?}");
    assert_eq!(enrollment.json()["kind"], "workbench.pairing_expired");

    let state = manager.inner.state.lock().expect("workbench state");
    assert!(state.pairing_grants.is_empty());
    assert!(state.session_grants.is_empty());
    drop(state);
    assert!(
        !manager.inner.authorization_store_path.exists(),
        "failed enrollment must not commit an authorization store"
    );
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn missing_worktree_revokes_pairing_and_replays_terminal_resume() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("resume-disappears");
    let branch = "workbench-resume-disappears";
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            linked.to_str().expect("linked worktree path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&linked_root)
        .expect("launch published linked worktree");
    let (_, ticket) = launch_parts(&launch);
    let ticket_payload = published_ticket_payload(ticket);
    let request_entry = WorkbenchEntryBinding::published(
        ticket_payload.canonical_origin,
        ticket_payload.project_instance_id,
        ticket_payload.workspace_key,
    )
    .expect("published request entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &request_entry)
        .expect("enroll linked worktree pairing");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();
    let request_id = "m".repeat(43);

    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked worktree path"),
        ],
    );
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &request_id,
                &request_entry,
            )
            .expect_err("removed worktree rejects resume"),
        PairingExchangeError::Expired
    );

    let store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read revoked authorization store"),
    )
    .expect("decode revoked authorization store");
    assert!(
        store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == pairing_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some())
    );
    assert!(store.sessions.is_empty());
    assert_eq!(store.resume_outcomes.len(), 1);
    assert_eq!(
        store.resume_outcomes[0].terminal_error(),
        Some(PairingExchangeError::Expired)
    );
    manager.shutdown().await;

    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            linked.to_str().expect("linked worktree path"),
            branch,
        ],
    );
    let replacement = test_manager(Arc::clone(&fixture.project));
    assert_eq!(
        replacement
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &request_id,
                &request_entry,
            )
            .expect_err("recreated worktree replays terminal result"),
        PairingExchangeError::Expired,
        "the same request ID replays its terminal result after path recreation"
    );
    assert_eq!(
        replacement
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &"n".repeat(43),
                &request_entry,
            )
            .expect_err("recreated path cannot restore authority"),
        PairingExchangeError::Expired,
        "path recreation cannot restore revoked pairing authority"
    );
    replacement.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn terminal_resume_outcome_survives_authenticated_worktree_rebind() {
    let fixture = fixture();
    let original = fixture._temp.path().join("terminal-before-move");
    let moved = fixture._temp.path().join("terminal-after-move");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-terminal-move",
            original.to_str().expect("original linked path"),
        ],
    );
    let original_root = original
        .canonicalize()
        .expect("canonical original worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let launch = manager
        .launch(&original_root)
        .expect("launch original published workbench");
    let (_, ticket) = launch_parts(&launch);
    let original_entry = WorkbenchEntryBinding::published(
        "https://workbench-moved.test.localhost".to_string(),
        "locald-stable-project-instance".to_string(),
        launch.workspace.key.clone(),
    )
    .expect("original published entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &original_entry)
        .expect("enroll original pairing");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();

    run_git(
        &fixture.root,
        &[
            "worktree",
            "move",
            original.to_str().expect("original linked path"),
            moved.to_str().expect("moved linked path"),
        ],
    );
    let moved_root = moved.canonicalize().expect("canonical moved worktree");
    let moved_workspace = manager
        .register_workspace(&moved_root)
        .expect("register moved workspace without rebinding pairing");
    let moved_entry = WorkbenchEntryBinding::published(
        "https://workbench-moved.test.localhost".to_string(),
        "locald-stable-project-instance".to_string(),
        moved_workspace.key.clone(),
    )
    .expect("moved published entry");
    let request_id = "t".repeat(43);
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &request_id,
                &moved_entry,
            )
            .expect_err("unreconciled entry rejects resume"),
        PairingExchangeError::Invalid
    );

    manager
        .launch(&moved_root)
        .expect("authenticated launch reconciles moved worktree");
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &request_id,
                &moved_entry,
            )
            .expect_err("reconciled entry replays terminal result"),
        PairingExchangeError::Invalid,
        "the same request ID keeps its terminal result after reconciliation"
    );
    assert!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &"u".repeat(43),
                &moved_entry,
            )
            .is_ok(),
        "a new request ID may use the reconciled pairing"
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_reenrollment_revokes_a_capability_mismatched_pairing() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let first_launch = manager
        .launch(&fixture.root)
        .expect("launch first published enrollment");
    let (published_origin, first_ticket) = launch_parts(&first_launch);
    let published_host = expected_host_from_origin(published_origin).expect("published host");
    let private_origin = manager.host_status().expect("private host").origin;
    let first_enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": first_ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("enroll first published browser");
    assert_eq!(first_enrollment.status, 200, "{first_enrollment:?}");
    let first_pairing_cookie = response_cookie(&first_enrollment, PAIRING_COOKIE_NAME)
        .expect("first pairing cookie")
        .to_string();
    let first_selector = first_pairing_cookie
        .split('.')
        .nth(1)
        .expect("first pairing selector")
        .to_string();

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let pairing = state
            .pairing_grants
            .get_mut(&first_selector)
            .expect("first pairing grant");
        pairing
            .capabilities
            .retain(|capability| capability != "lane.focus");
    }
    manager
        .inner
        .persist_session_store()
        .expect("persist capability-mismatched pairing");

    let second_launch = manager
        .launch(&fixture.root)
        .expect("launch replacement published enrollment");
    let (_, second_ticket) = launch_parts(&second_launch);
    let replacement = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": second_ticket }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={first_pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("replace capability-mismatched pairing");
    assert_eq!(replacement.status, 200, "{replacement:?}");
    let replacement_cookie =
        response_cookie(&replacement, PAIRING_COOKIE_NAME).expect("replacement pairing cookie");
    let replacement_selector = replacement_cookie
        .split('.')
        .nth(1)
        .expect("replacement pairing selector");
    assert_ne!(replacement_selector, first_selector);

    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(state.pairing_grants.len(), 2);
    assert!(
        state
            .pairing_grants
            .get(&first_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some())
    );
    assert!(state.pairing_grants.contains_key(replacement_selector));
    assert!(
        state.session_grants.values().all(|session| {
            session.pairing_selector.as_deref() != Some(first_selector.as_str())
        })
    );
    assert!(
        state
            .resume_outcomes
            .keys()
            .all(|key| key.pairing_selector != first_selector)
    );
    drop(state);

    let store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path).expect("read replacement store"),
    )
    .expect("decode replacement store");
    assert_eq!(store.pairings.len(), 2);
    assert!(
        store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == first_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some())
    );
    assert!(
        store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == replacement_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_none())
    );
    assert!(
        store.sessions.iter().all(|session| {
            session.pairing_selector.as_deref() != Some(first_selector.as_str())
        })
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn published_pairing_management_is_scoped_path_free_and_revocable() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let launch = manager
        .launch(&fixture.root)
        .expect("launch published workbench");
    let (published_origin, ticket) = launch_parts(&launch);
    let published_host = expected_host_from_origin(published_origin).expect("published host");
    let private_origin = manager.host_status().expect("private host").origin;
    let enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("enroll published browser");
    assert_eq!(enrollment.status, 200, "{enrollment:?}");
    let pairing_cookie = response_cookie(&enrollment, PAIRING_COOKIE_NAME)
        .expect("pairing cookie")
        .to_string();
    let pairing_selector = pairing_cookie
        .split('.')
        .nth(1)
        .expect("pairing selector")
        .to_string();
    let session_cookie = response_cookie_prefix(&enrollment, SESSION_COOKIE_PREFIX)
        .expect("session cookie")
        .to_string();
    let session_key = session_cookie
        .split_once('=')
        .and_then(|(name, _)| name.strip_prefix(SESSION_COOKIE_PREFIX))
        .expect("session key")
        .to_string();
    let browser_cookies = format!("{session_cookie}; {PAIRING_COOKIE_NAME}={pairing_cookie}");

    manager
        .inner
        .rename_pairing(&pairing_selector, "Daily planning browser", None)
        .expect("rename pairing");
    let cli_projection = manager
        .inner
        .list_pairings(None, None, false)
        .expect("list CLI pairings");
    assert_eq!(cli_projection.pairings.len(), 1);
    assert_eq!(cli_projection.pairings[0].selector, pairing_selector);
    assert_eq!(
        cli_projection.pairings[0].nickname.as_deref(),
        Some("Daily planning browser")
    );

    let browser_projection = raw_http_via(
        &private_origin,
        published_host,
        "GET",
        &format!("/api/pairings?session_key={session_key}"),
        None,
        Some(&browser_cookies),
        Some(published_origin),
    )
    .await
    .expect("list browser pairings");
    assert_eq!(browser_projection.status, 200, "{browser_projection:?}");
    let projection = browser_projection.json();
    assert_eq!(projection["pairings"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        projection["pairings"][0]["selector"],
        pairing_selector.chars().take(12).collect::<String>()
    );
    assert_eq!(projection["pairings"][0]["current"], true);
    assert_eq!(
        projection["pairings"][0]["nickname"],
        "Daily planning browser"
    );
    let serialized_projection = serde_json::to_string(&projection).expect("serialize projection");
    assert!(!serialized_projection.contains(&fixture.root.display().to_string()));

    let browser_rename = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/rename",
        Some(
            json!({
                "schema_version": 1,
                "session_key": session_key,
                "selector": pairing_selector.chars().take(12).collect::<String>(),
                "nickname": "Cockpit browser"
            })
            .to_string(),
        ),
        Some(&browser_cookies),
        Some(published_origin),
    )
    .await
    .expect("rename browser pairing");
    assert_eq!(browser_rename.status, 200, "{browser_rename:?}");
    assert_eq!(browser_rename.json()["kind"], "workbench.pairing.rename");
    assert_eq!(
        manager
            .inner
            .list_pairings(None, None, false)
            .expect("list renamed pairings")
            .pairings[0]
            .nickname
            .as_deref(),
        Some("Cockpit browser")
    );
    assert_eq!(
        manager.inner.rename_pairing(
            &pairing_selector,
            "Wrong workspace",
            Some("sibling-workspace")
        ),
        Err(PairingManagementError::NotFound)
    );

    let persisted_before_failure = fs::read(&manager.inner.authorization_store_path)
        .expect("read authorization store before failed revocation");
    let backup_path = manager
        .inner
        .authorization_store_path
        .with_extension("json.before-revoke");
    fs::rename(&manager.inner.authorization_store_path, &backup_path)
        .expect("move authorization store aside");
    fs::create_dir(&manager.inner.authorization_store_path)
        .expect("block authorization-store replacement");
    assert_eq!(
        manager.inner.revoke_pairing(&pairing_selector, None),
        Err(PairingManagementError::Unavailable)
    );
    let state_after_failure = manager.inner.state.lock().expect("workbench state");
    assert!(
        state_after_failure
            .pairing_grants
            .contains_key(&pairing_selector)
    );
    assert!(
        state_after_failure
            .session_grants
            .values()
            .any(|grant| grant.pairing_selector.as_deref() == Some(&pairing_selector))
    );
    drop(state_after_failure);
    fs::remove_dir(&manager.inner.authorization_store_path)
        .expect("remove blocked authorization-store destination");
    fs::rename(&backup_path, &manager.inner.authorization_store_path)
        .expect("restore authorization store after failed revocation");
    assert_eq!(
        fs::read(&manager.inner.authorization_store_path)
            .expect("read restored authorization store"),
        persisted_before_failure,
        "failed revocation must leave durable authorization unchanged"
    );

    let revoke = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/revoke",
        Some(
            json!({
                "schema_version": 1,
                "session_key": session_key,
                "selector": pairing_selector.chars().take(12).collect::<String>(),
            })
            .to_string(),
        ),
        Some(&browser_cookies),
        Some(published_origin),
    )
    .await
    .expect("revoke browser pairing");
    assert_eq!(revoke.status, 200, "{revoke:?}");
    let revoked_projection = manager
        .inner
        .list_pairings(None, None, false)
        .expect("list revoked pairings");
    assert_eq!(revoked_projection.pairings.len(), 1);
    assert_eq!(
        revoked_projection.pairings[0].status,
        WorkbenchPairingStatus::Revoked
    );
    assert!(revoked_projection.pairings[0].revoked_at.is_some());
    assert!(!revoked_projection.pairings[0].current);
    let persisted_revocation: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read authorization store after revocation"),
    )
    .expect("decode authorization store after revocation");
    assert_eq!(persisted_revocation.pairings.len(), 1);
    assert!(persisted_revocation.pairings[0].revoked_at.is_some());
    assert!(persisted_revocation.sessions.is_empty());
    assert!(persisted_revocation.resume_outcomes.is_empty());
    let restored_revocation = read_authorization_store(
        &manager.inner.authorization_store_path,
        fixture.project.id.as_str(),
        unix_seconds(),
    )
    .expect("read retained revocation")
    .expect("retained authorization state");
    assert!(
        restored_revocation
            .pairings
            .get(&pairing_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some())
    );
    let resume = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "z".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume revoked pairing");
    assert_eq!(resume.status, 401);

    let second_launch = manager
        .launch(&fixture.root)
        .expect("launch replacement enrollment");
    let (_, second_ticket) = launch_parts(&second_launch);
    let second_enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": second_ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("enroll replacement browser");
    let second_pairing_cookie =
        response_cookie(&second_enrollment, PAIRING_COOKIE_NAME).expect("second pairing cookie");
    let second_pairing_selector = second_pairing_cookie
        .split('.')
        .nth(1)
        .expect("second pairing selector")
        .to_string();
    let second_session_cookie = response_cookie_prefix(&second_enrollment, SESSION_COOKIE_PREFIX)
        .expect("second session cookie");
    let second_session_key = second_session_cookie
        .split_once('=')
        .and_then(|(name, _)| name.strip_prefix(SESSION_COOKIE_PREFIX))
        .expect("second session key");
    let mut forgotten_session_cookie_names = vec![
        second_session_cookie
            .split_once('=')
            .map(|(name, _)| name.to_string())
            .expect("enrollment session cookie name"),
    ];
    for request_marker in ['p', 'q'] {
        let resumed_tab = raw_http_via(
            &private_origin,
            published_host,
            "POST",
            "/api/pairing/resume",
            Some(
                json!({
                    "schema_version": 1,
                    "request_id": request_marker.to_string().repeat(43),
                })
                .to_string(),
            ),
            Some(&format!("{PAIRING_COOKIE_NAME}={second_pairing_cookie}")),
            Some(published_origin),
        )
        .await
        .expect("resume another browser tab");
        assert_eq!(resumed_tab.status, 200, "{resumed_tab:?}");
        forgotten_session_cookie_names.push(
            response_cookie_prefix(&resumed_tab, SESSION_COOKIE_PREFIX)
                .and_then(|cookie| cookie.split_once('=').map(|(name, _)| name.to_string()))
                .expect("resumed tab session cookie name"),
        );
    }
    let forget = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/forget",
        Some(
            json!({
                "schema_version": 1,
                "session_key": second_session_key,
            })
            .to_string(),
        ),
        Some(&format!(
            "{second_session_cookie}; {PAIRING_COOKIE_NAME}={second_pairing_cookie}"
        )),
        Some(published_origin),
    )
    .await
    .expect("forget current browser pairing");
    assert_eq!(forget.status, 200, "{forget:?}");
    assert_eq!(forget.json()["kind"], "workbench.pairing.forget");
    assert_eq!(
        forget
            .set_cookies
            .iter()
            .filter(|cookie| cookie.contains("Max-Age=0"))
            .count(),
        forgotten_session_cookie_names.len() + 1,
        "browser-local Forget expires the pairing cookie and every active tab session"
    );
    for cookie_name in forgotten_session_cookie_names {
        assert!(
            forget.set_cookies.iter().any(|cookie| {
                cookie.starts_with(&format!("{cookie_name}=")) && cookie.contains("Max-Age=0")
            }),
            "browser-local Forget must expire {cookie_name}"
        );
    }
    assert_eq!(
        manager
            .inner
            .list_pairings(None, None, false)
            .expect("list pairings after browser forget")
            .pairings
            .len(),
        2,
        "forgetting local browser state must not revoke durable authority"
    );
    let resume_after_browser_forget = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "y".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={second_pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume after browser-local forget");
    assert_eq!(resume_after_browser_forget.status, 200);

    manager
        .inner
        .forget_pairing(&pairing_selector, None)
        .expect("forget retained revoked pairing");
    let after_retained_forget = manager
        .inner
        .list_pairings(None, None, false)
        .expect("list after retained pairing forget");
    assert_eq!(after_retained_forget.pairings.len(), 1);
    assert_eq!(
        after_retained_forget.pairings[0].selector,
        second_pairing_selector
    );
    assert_eq!(
        after_retained_forget.pairings[0].status,
        WorkbenchPairingStatus::Active
    );

    manager
        .inner
        .forget_pairing(&second_pairing_selector, None)
        .expect("forget active pairing");
    assert!(
        manager
            .inner
            .list_pairings(None, None, false)
            .expect("list after active pairing forget")
            .pairings
            .is_empty()
    );
    let resume_after_cli_forget = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "x".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={second_pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume after CLI pairing forget");
    assert_eq!(resume_after_cli_forget.status, 401);
    manager.shutdown().await;
}

#[test]
fn retained_revoked_pairings_have_separate_project_and_workspace_bounds() {
    let fixture = fixture();
    let now = unix_seconds();
    let mut pairings = HashMap::new();
    let active_selector = "active-pairing-selector".to_string();
    pairings.insert(
        active_selector.clone(),
        WorkbenchPairingGrantV1 {
            selector: active_selector.clone(),
            credential_digest: "a".repeat(64),
            project_id: fixture.project.id.as_str().to_string(),
            workspace_key: "workspace-a".to_string(),
            workspace_root: fixture.root.clone(),
            launch_mode: WorkbenchLaunchMode::Published,
            project_instance_id: "project-instance".to_string(),
            canonical_origin: "https://workbench.fixture.localhost".to_string(),
            capabilities: vec!["workbench.snapshot".to_string()],
            created_at: now.saturating_sub(100),
            last_used_at: now.saturating_sub(50),
            idle_expires_at: now.saturating_add(100),
            absolute_expires_at: now.saturating_add(200),
            nickname: None,
            revoked_at: None,
            revocation_cause: None,
        },
    );
    for index in 0..(MAX_RETAINED_REVOKED_PAIRINGS_PER_WORKSPACE + 3) {
        let selector = format!("revoked-pairing-{index:02}");
        pairings.insert(
            selector.clone(),
            WorkbenchPairingGrantV1 {
                selector,
                credential_digest: format!("{index:064x}"),
                project_id: fixture.project.id.as_str().to_string(),
                workspace_key: "workspace-a".to_string(),
                workspace_root: fixture.root.clone(),
                launch_mode: WorkbenchLaunchMode::Published,
                project_instance_id: "project-instance".to_string(),
                canonical_origin: "https://workbench.fixture.localhost".to_string(),
                capabilities: vec!["workbench.snapshot".to_string()],
                created_at: now.saturating_sub(100),
                last_used_at: now.saturating_sub(50),
                idle_expires_at: now.saturating_add(100),
                absolute_expires_at: now.saturating_add(200),
                nickname: None,
                revoked_at: Some(
                    now.saturating_sub(
                        u64::try_from(MAX_RETAINED_REVOKED_PAIRINGS_PER_WORKSPACE + 3 - index)
                            .expect("fixture index fits u64"),
                    ),
                ),
                revocation_cause: Some(WorkbenchPairingRevocationCause::Explicit),
            },
        );
    }

    prune_retained_revoked_pairings(&mut pairings);

    assert!(pairings.contains_key(&active_selector));
    let retained = pairings
        .values()
        .filter(|pairing| pairing.revoked_at.is_some())
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), MAX_RETAINED_REVOKED_PAIRINGS_PER_WORKSPACE);
    assert!(pairings.contains_key("revoked-pairing-10"));
    assert!(!pairings.contains_key("revoked-pairing-00"));

    let revoked_template = retained[0].clone();
    let active = pairings
        .get(&active_selector)
        .expect("active pairing")
        .clone();
    let mut project_pairings = HashMap::from([(active_selector.clone(), active)]);
    for index in 0..(MAX_RETAINED_REVOKED_PAIRINGS + 3) {
        let selector = format!("project-revoked-pairing-{index:02}");
        let mut pairing = revoked_template.clone();
        pairing.selector = selector.clone();
        pairing.workspace_key = format!("workspace-{index:02}");
        pairing.revoked_at = Some(
            now.saturating_sub(
                u64::try_from(MAX_RETAINED_REVOKED_PAIRINGS + 3 - index)
                    .expect("fixture index fits u64"),
            ),
        );
        project_pairings.insert(selector, pairing);
    }

    prune_retained_revoked_pairings(&mut project_pairings);

    assert!(project_pairings.contains_key(&active_selector));
    assert_eq!(
        project_pairings
            .values()
            .filter(|pairing| pairing.revoked_at.is_some())
            .count(),
        MAX_RETAINED_REVOKED_PAIRINGS
    );
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn published_pairing_is_scoped_to_the_exact_linked_worktree_origin() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("pairing-linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-pairing-linked",
            linked.to_str().expect("linked path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");

    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Pairing Isolation", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Pairing Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    let primary_lane = writer
        .add_workbench_lane("Primary", "Primary worktree lane", &phase)
        .expect("add primary lane");
    let linked_lane = writer
        .add_workbench_lane("Linked", "Linked worktree lane", &phase)
        .expect("add linked lane");
    let primary_root = fixture
        .root
        .canonicalize()
        .expect("canonical primary worktree");
    writer
        .focus_workbench_lane(&primary_root.to_string_lossy(), &primary_lane, &phase)
        .expect("focus primary lane");
    writer
        .focus_workbench_lane(&linked_root.to_string_lossy(), &linked_lane, &phase)
        .expect("focus linked lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    use_test_published_entries(&manager);
    let primary_launch = manager
        .launch(&primary_root)
        .expect("launch primary published workbench");
    let linked_launch = manager
        .launch(&linked_root)
        .expect("launch linked published workbench");
    let (primary_origin, primary_ticket) = launch_parts(&primary_launch);
    let (linked_origin, _) = launch_parts(&linked_launch);
    assert_ne!(primary_launch.workspace.key, linked_launch.workspace.key);
    assert_ne!(primary_origin, linked_origin);
    let primary_host = expected_host_from_origin(primary_origin).expect("primary host");
    let linked_host = expected_host_from_origin(linked_origin).expect("linked host");
    let private_origin = manager.host_status().expect("private host").origin;

    let enrollment = raw_http_via(
        &private_origin,
        primary_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": primary_ticket }).to_string()),
        None,
        Some(primary_origin),
    )
    .await
    .expect("enroll primary browser");
    assert_eq!(enrollment.status, 200, "{enrollment:?}");
    let pairing_cookie =
        response_cookie(&enrollment, PAIRING_COOKIE_NAME).expect("primary pairing cookie");

    let cross_workspace_resume = raw_http_via(
        &private_origin,
        linked_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "x".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(linked_origin),
    )
    .await
    .expect("attempt cross-worktree resume");
    assert_eq!(cross_workspace_resume.status, 401);
    assert_eq!(
        cross_workspace_resume.json()["kind"],
        "workbench.pairing_invalid"
    );

    let mismatched_origin = raw_http_via(
        &private_origin,
        primary_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "y".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(linked_origin),
    )
    .await
    .expect("attempt mismatched-origin resume");
    assert_eq!(mismatched_origin.status, 403);

    assert_eq!(
        manager
            .snapshot(&primary_root)
            .expect("snapshot primary focus")
            .focused_lane
            .as_ref()
            .map(|lane| lane.summary.id.as_str()),
        Some(primary_lane.as_str())
    );
    assert_eq!(
        manager
            .snapshot(&linked_root)
            .expect("snapshot linked focus")
            .focused_lane
            .as_ref()
            .map(|lane| lane.summary.id.as_str()),
        Some(linked_lane.as_str())
    );
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn authenticated_published_launch_rebinds_a_moved_worktree_pairing() {
    let fixture = fixture();
    let original = fixture._temp.path().join("pairing-before-move");
    let moved = fixture._temp.path().join("pairing-after-move");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-pairing-move",
            original.to_str().expect("original linked path"),
        ],
    );
    let original_root = original
        .canonicalize()
        .expect("canonical original worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let first_launch = manager
        .launch(&original_root)
        .expect("launch original published workbench");
    let original_workspace_key = first_launch.workspace.key.clone();
    let (published_origin, ticket) = launch_parts(&first_launch);
    let published_host = expected_host_from_origin(published_origin).expect("published host");
    let private_origin = manager.host_status().expect("private host").origin;
    let enrollment = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/enroll",
        Some(json!({ "schema_version": 1, "ticket": ticket }).to_string()),
        None,
        Some(published_origin),
    )
    .await
    .expect("enroll before move");
    assert_eq!(enrollment.status, 200, "{enrollment:?}");
    let pairing_cookie = response_cookie(&enrollment, PAIRING_COOKIE_NAME)
        .expect("pairing cookie")
        .to_string();
    let pairing_selector = pairing_cookie
        .split('.')
        .nth(1)
        .expect("pairing selector")
        .to_string();
    let first_resume = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "b".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume before move");
    assert_eq!(first_resume.status, 200, "{first_resume:?}");

    run_git(
        &fixture.root,
        &[
            "worktree",
            "move",
            original.to_str().expect("original linked path"),
            moved.to_str().expect("moved linked path"),
        ],
    );
    let moved_root = moved.canonicalize().expect("canonical moved worktree");
    manager
        .snapshot(&fixture.root)
        .expect("sibling snapshot observes the moved worktree");
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state after sibling snapshot")
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| {
                pairing.revoked_at.is_some()
                    && pairing.revocation_cause
                        == Some(WorkbenchPairingRevocationCause::WorkspaceMissing)
            }),
        "Git move evidence alone must not preserve pairing authority"
    );
    let revoked_store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read authorization store after sibling snapshot"),
    )
    .expect("decode authorization store after sibling snapshot");
    assert!(
        revoked_store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == pairing_selector)
            .is_some_and(|pairing| {
                pairing.revoked_at.is_some()
                    && pairing.revocation_cause
                        == Some(WorkbenchPairingRevocationCause::WorkspaceMissing)
            }),
        "missing workspace authority remains revoked until exact published-instance proof"
    );
    let moved_launch = manager
        .launch(&moved_root)
        .expect("authenticated launch after worktree move");
    assert_eq!(launch_parts(&moved_launch).0, published_origin);
    assert_ne!(moved_launch.workspace.key, original_workspace_key);
    let moved_workspace_key = moved_launch.workspace.key.clone();
    let store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path).expect("read moved authorization store"),
    )
    .expect("decode moved authorization store");
    assert_eq!(store.pairings.len(), 1);
    assert_eq!(store.pairings[0].workspace_key, moved_workspace_key);
    assert_eq!(store.pairings[0].workspace_root, moved_root);
    assert!(store.pairings[0].revoked_at.is_none());
    assert!(store.pairings[0].revocation_cause.is_none());
    assert!(store.resume_outcomes.is_empty());
    assert!(
        store
            .sessions
            .iter()
            .all(|session| session.pairing_selector.is_none()),
        "moving the authenticated project instance invalidates derived sessions"
    );

    let resumed_after_move = raw_http_via(
        &private_origin,
        published_host,
        "POST",
        "/api/pairing/resume",
        Some(json!({ "schema_version": 1, "request_id": "c".repeat(43) }).to_string()),
        Some(&format!("{PAIRING_COOKIE_NAME}={pairing_cookie}")),
        Some(published_origin),
    )
    .await
    .expect("resume retained pairing after move");
    assert_eq!(resumed_after_move.status, 200, "{resumed_after_move:?}");
    assert_eq!(
        resumed_after_move.json()["workspace_key"],
        moved_workspace_key
    );
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn direct_launch_after_worktree_move_releases_stale_publication_authority() {
    let fixture = fixture();
    let original = fixture._temp.path().join("direct-before-move");
    let moved = fixture._temp.path().join("direct-after-move");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-direct-move",
            original.to_str().expect("original linked path"),
        ],
    );
    let original_root = original
        .canonicalize()
        .expect("canonical original worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(TestPublishedThenDirectEntryProvider::default());
    manager.set_entry_provider(provider.clone());
    let published_launch = manager
        .launch(&original_root)
        .expect("launch original published workbench");
    let original_workspace_key = published_launch.workspace.key.clone();
    let (_, ticket) = launch_parts(&published_launch);
    let payload = published_ticket_payload(ticket);
    let published_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin.clone(),
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("published request entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &published_entry)
        .expect("enroll original pairing");
    let pairing_selector = enrollment
        .pairing_cookie
        .split('.')
        .nth(1)
        .expect("pairing selector")
        .to_string();

    run_git(
        &fixture.root,
        &[
            "worktree",
            "move",
            original.to_str().expect("original linked path"),
            moved.to_str().expect("moved linked path"),
        ],
    );
    let moved_root = moved.canonicalize().expect("canonical moved worktree");
    provider.use_direct_entry();
    let direct_launch = manager
        .launch(&moved_root)
        .expect("launch moved worktree through direct fallback");
    assert_eq!(
        direct_launch.launch_mode,
        WorkbenchLaunchMode::DirectLoopback
    );
    assert_ne!(direct_launch.workspace.key, original_workspace_key);
    assert_eq!(
        provider.released_workspace_keys(),
        vec![original_workspace_key.clone()],
        "direct fallback must release the stale published workspace"
    );

    let state = manager.inner.state.lock().expect("workbench state");
    assert!(
        !state
            .workspaces_by_key
            .contains_key(&original_workspace_key)
    );
    assert!(
        state
            .origin_bindings
            .values()
            .all(|binding| binding.workspace_key.as_deref() != Some(&original_workspace_key))
    );
    assert!(
        state
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| {
                pairing.revoked_at.is_some()
                    && pairing.revocation_cause
                        == Some(WorkbenchPairingRevocationCause::WorkspaceMissing)
            })
    );
    drop(state);
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn workspace_projection_samples_membership_after_authorization_coordination() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("projection-race-linked");
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(TestPublishedThenDirectEntryProvider::default());
    manager.set_entry_provider(provider.clone());
    let current = manager
        .register_workspace(&fixture.root)
        .expect("register current workspace");
    let linked_workspace_key = Mutex::new(None::<String>);

    let projections = manager
        .project_workspace_projections_with_before_authorization_gate(&current.key, || {
            run_git(
                &fixture.root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    "workbench-projection-race",
                    linked.to_str().expect("linked worktree path"),
                ],
            );
            let linked_root = linked.canonicalize().expect("canonical linked worktree");
            let launch = manager
                .launch(&linked_root)
                .expect("publish linked worktree during projection coordination");
            *linked_workspace_key.lock().expect("linked workspace key") =
                Some(launch.workspace.key);
        })
        .expect("project workspaces after concurrent registration");
    let linked_workspace_key = linked_workspace_key
        .into_inner()
        .expect("linked workspace key mutex")
        .expect("linked workspace key recorded");

    assert!(
        projections
            .iter()
            .any(|workspace| workspace.registration.key == linked_workspace_key)
    );
    let state = manager.inner.state.lock().expect("workbench state");
    assert!(state.workspaces_by_key.contains_key(&linked_workspace_key));
    assert!(
        state
            .origin_bindings
            .values()
            .any(|binding| binding.workspace_key.as_deref() == Some(&linked_workspace_key))
    );
    drop(state);
    assert!(provider.released_workspace_keys().is_empty());
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn replacement_project_instance_cannot_leave_a_restorable_missing_workspace_pairing() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("replacement-after-removal");
    let branch = "workbench-replacement-after-removal";
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            linked.to_str().expect("linked worktree path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let original_launch = manager
        .launch(&linked_root)
        .expect("launch original published workbench");
    let (_, ticket) = launch_parts(&original_launch);
    let payload = published_ticket_payload(ticket);
    let original_entry = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("original published entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &original_entry)
        .expect("enroll original pairing");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();

    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked worktree path"),
        ],
    );
    manager
        .snapshot(&fixture.root)
        .expect("observe removed worktree");
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| {
                pairing.revocation_cause == Some(WorkbenchPairingRevocationCause::WorkspaceMissing)
            })
    );

    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            linked.to_str().expect("linked worktree path"),
            branch,
        ],
    );
    let replacement_root = linked
        .canonicalize()
        .expect("canonical replacement worktree");
    manager.set_entry_provider(Arc::new(TestReplacementPublishedEntryProvider));
    manager
        .launch(&replacement_root)
        .expect("launch replacement project instance");
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| {
                pairing.revoked_at.is_some()
                    && pairing.revocation_cause == Some(WorkbenchPairingRevocationCause::Replaced)
            }),
        "a different project instance makes the old pairing permanently replaced"
    );

    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    manager
        .launch(&replacement_root)
        .expect("relaunch original project instance identity");
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &"z".repeat(43),
                &original_entry,
            )
            .expect_err("replaced pairing remains revoked"),
        PairingExchangeError::Expired
    );
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn failed_moved_worktree_publication_restores_the_previous_origin_binding() {
    let fixture = fixture();
    let original = fixture._temp.path().join("binding-before-move");
    let moved = fixture._temp.path().join("binding-after-move");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-binding-move",
            original.to_str().expect("original linked path"),
        ],
    );
    let original_root = original
        .canonicalize()
        .expect("canonical original worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let original_launch = manager
        .launch(&original_root)
        .expect("launch original published workbench");
    let original_workspace_key = original_launch.workspace.key.clone();
    let (published_origin, ticket) = launch_parts(&original_launch);
    let published_origin = published_origin.to_string();
    let original_entry = WorkbenchEntryBinding::published(
        published_origin.clone(),
        "locald-stable-project-instance".to_string(),
        original_workspace_key.clone(),
    )
    .expect("original published entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &original_entry)
        .expect("enroll pairing before failed move");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();
    manager
        .inner
        .resume_pairing(
            &pairing_selector,
            &pairing_secret,
            &"f".repeat(43),
            &original_entry,
        )
        .expect("record pairing session before failed move");
    let authorization_before_move: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read authorization store before failed move"),
    )
    .expect("decode authorization store before failed move");

    run_git(
        &fixture.root,
        &[
            "worktree",
            "move",
            original.to_str().expect("original linked path"),
            moved.to_str().expect("moved linked path"),
        ],
    );
    let moved_root = moved.canonicalize().expect("canonical moved worktree");
    manager.set_entry_provider(Arc::new(TestFailingMovedPublishedEntryProvider));
    let error = manager
        .launch(&moved_root)
        .expect_err("moved publication fails after temporary authorization");
    assert!(
        error
            .to_string()
            .contains("injected moved-worktree publication failure")
    );

    let state = manager.inner.state.lock().expect("workbench state");
    let restored = state
        .origin_bindings
        .get(&published_origin)
        .expect("previous origin binding restored");
    assert_eq!(
        restored.workspace_key.as_deref(),
        Some(original_workspace_key.as_str())
    );
    assert_eq!(
        state
            .origin_bindings
            .values()
            .filter(|binding| {
                binding.project_instance_id.as_deref() == Some("locald-stable-project-instance")
            })
            .count(),
        1
    );
    drop(state);
    let authorization_after_failure: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read authorization store after failed move"),
    )
    .expect("decode authorization store after failed move");
    assert_eq!(authorization_after_failure, authorization_before_move);
    manager.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn replacement_project_instance_revokes_stale_workspace_pairing_authority() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.set_entry_provider(Arc::new(TestMovedPublishedEntryProvider));
    let launch = manager
        .launch(&fixture.root)
        .expect("launch original published workbench");
    let (_, ticket) = launch_parts(&launch);
    let original_entry = WorkbenchEntryBinding::published(
        "https://workbench-moved.test.localhost".to_string(),
        "locald-stable-project-instance".to_string(),
        launch.workspace.key.clone(),
    )
    .expect("original published entry");
    let enrollment = manager
        .inner
        .enroll_pairing(ticket, None, &original_entry)
        .expect("enroll original pairing");
    let mut pairing_parts = enrollment.pairing_cookie.split('.');
    assert_eq!(pairing_parts.next(), Some("v1"));
    let pairing_selector = pairing_parts.next().expect("pairing selector").to_string();
    let pairing_secret = pairing_parts.next().expect("pairing secret").to_string();
    manager
        .inner
        .resume_pairing(
            &pairing_selector,
            &pairing_secret,
            &"r".repeat(43),
            &original_entry,
        )
        .expect("record original resume outcome");
    let terminal_request_id = "t".repeat(43);
    let mismatched_entry = WorkbenchEntryBinding::published(
        "https://replacement-mismatch.test.localhost".to_string(),
        "locald-stable-project-instance".to_string(),
        launch.workspace.key.clone(),
    )
    .expect("mismatched published entry");
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &terminal_request_id,
                &mismatched_entry,
            )
            .expect_err("record terminal resume outcome"),
        PairingExchangeError::Invalid
    );

    manager.set_entry_provider(Arc::new(TestReplacementPublishedEntryProvider));
    manager
        .launch(&fixture.root)
        .expect("launch replacement project instance");

    let state = manager.inner.state.lock().expect("workbench state");
    assert!(
        state
            .pairing_grants
            .get(&pairing_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some()),
        "the replaced pairing remains as revoked audit identity"
    );
    assert!(
        state.session_grants.values().all(|session| {
            session.pairing_selector.as_deref() != Some(pairing_selector.as_str())
        })
    );
    assert!(
        state.sessions.values().all(|session| {
            session.pairing_selector.as_deref() != Some(pairing_selector.as_str())
        })
    );
    assert_eq!(state.resume_outcomes.len(), 1);
    assert_eq!(
        state
            .resume_outcomes
            .get(&WorkbenchResumeOutcomeKey {
                pairing_selector: pairing_selector.clone(),
                request_id: terminal_request_id.clone(),
            })
            .and_then(WorkbenchResumeOutcomeV1::terminal_error),
        Some(PairingExchangeError::Invalid)
    );
    drop(state);
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &terminal_request_id,
                &original_entry,
            )
            .expect_err("replacement replays terminal resume result"),
        PairingExchangeError::Invalid
    );
    assert_eq!(
        manager
            .inner
            .resume_pairing(
                &pairing_selector,
                &pairing_secret,
                &"u".repeat(43),
                &original_entry,
            )
            .expect_err("replacement pairing rejects a new resume request"),
        PairingExchangeError::Expired
    );

    let store: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read replacement authorization store"),
    )
    .expect("decode replacement authorization store");
    assert!(
        store
            .pairings
            .iter()
            .find(|pairing| pairing.selector == pairing_selector)
            .is_some_and(|pairing| pairing.revoked_at.is_some())
    );
    assert!(
        store.sessions.iter().all(|session| {
            session.pairing_selector.as_deref() != Some(pairing_selector.as_str())
        })
    );
    assert_eq!(store.resume_outcomes.len(), 1);
    assert_eq!(store.resume_outcomes[0].pairing_selector, pairing_selector);
    assert_eq!(store.resume_outcomes[0].request_id, terminal_request_id);
    assert_eq!(
        store.resume_outcomes[0].terminal_error(),
        Some(PairingExchangeError::Invalid)
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn workbench_residency_tracks_enrollment_pairing_and_expiration() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
    manager.set_entry_provider(provider.clone());

    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let workspace_key = launch.workspace.key.clone();
    let (_, ticket) = launch_parts(&launch);
    let now = unix_seconds();
    assert!(manager.requires_daemon_residency(now));
    assert!(!manager.requires_daemon_residency_with_assets(now, false));

    let payload = published_ticket_payload(ticket);
    let entry = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("published workbench entry");
    manager
        .inner
        .enroll_pairing(ticket, None, &entry)
        .expect("enroll durable pairing");
    assert!(manager.requires_daemon_residency(now));

    let pairing_selector = manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .pairing_grants
        .keys()
        .next()
        .expect("durable pairing")
        .clone();
    manager
        .inner
        .revoke_pairing(&pairing_selector, None)
        .expect("revoke durable pairing");
    assert!(!manager.requires_daemon_residency(now));
    assert_eq!(
        provider.released_workspace_keys(),
        vec![workspace_key.clone()]
    );

    let expiring = manager
        .launch(&fixture.root)
        .expect("launch workbench again");
    let expiring_response = launch_response_envelope("launch-expiring", &expiring);
    manager
        .retain_launch_replay("launch-expiring", &expiring_response)
        .expect("retain expiring launch replay");
    let (_, expiring_ticket) = launch_parts(&expiring);
    let expiring_payload = published_ticket_payload(expiring_ticket);
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .pending_capabilities
        .get_mut(&expiring_payload.capability_id)
        .expect("pending enrollment")
        .expires_at = now;
    assert!(!manager.requires_daemon_residency(now));
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .launch_replays
            .is_empty(),
        "residency maintenance prunes replay responses for expired capabilities"
    );
    assert_eq!(
        provider.released_workspace_keys(),
        vec![workspace_key.clone(), workspace_key]
    );

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn issuance_invalid_pending_launches_do_not_hold_publication_residency() {
    for replace_workspace in [false, true] {
        for forget in [false, true] {
            let fixture = fixture();
            let manager = test_manager(Arc::clone(&fixture.project));
            let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
            manager.set_entry_provider(provider.clone());

            let paired = manager
                .launch(&fixture.root)
                .expect("launch paired workbench");
            let workspace_key = paired.workspace.key.clone();
            let (_, paired_ticket) = launch_parts(&paired);
            let paired_payload = published_ticket_payload(paired_ticket);
            let entry = WorkbenchEntryBinding::published(
                paired_payload.canonical_origin,
                paired_payload.project_instance_id,
                paired_payload.workspace_key,
            )
            .expect("published workbench entry");
            manager
                .inner
                .enroll_pairing(paired_ticket, None, &entry)
                .expect("enroll durable pairing");
            let selector = manager
                .inner
                .state
                .lock()
                .expect("workbench state")
                .pairing_grants
                .keys()
                .next()
                .expect("durable pairing")
                .clone();

            let pending = manager
                .launch(&fixture.root)
                .expect("launch pending workbench");
            let request_id = format!(
                "launch-stale-{}-{}",
                if replace_workspace {
                    "workspace"
                } else {
                    "host"
                },
                if forget { "forget" } else { "revoke" }
            );
            manager
                .retain_launch_replay(
                    &request_id,
                    &launch_response_envelope(&request_id, &pending),
                )
                .expect("retain pending replay");
            let pending_payload = published_ticket_payload(launch_parts(&pending).1);
            {
                let mut state = manager.inner.state.lock().expect("workbench state");
                if replace_workspace {
                    let generation = state
                        .workspace_registration_generations
                        .get_mut(&workspace_key)
                        .expect("workspace registration generation");
                    *generation = generation.saturating_add(1);
                } else {
                    state.host.as_mut().expect("live workbench host").generation += 1;
                }
            }

            if forget {
                manager
                    .inner
                    .forget_pairing(&selector, None)
                    .expect("forget durable pairing");
            } else {
                manager
                    .inner
                    .revoke_pairing(&selector, None)
                    .expect("revoke durable pairing");
            }
            assert!(!manager.requires_daemon_residency(unix_seconds()));
            assert_eq!(provider.released_workspace_keys(), vec![workspace_key]);
            let state = manager.inner.state.lock().expect("workbench state");
            assert!(
                !state
                    .pending_capabilities
                    .contains_key(&pending_payload.capability_id)
            );
            assert!(!state.launch_replays.contains_key(&request_id));
            drop(state);

            manager.shutdown().await;
        }
    }
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn expired_pending_launch_releases_only_its_workspace_with_a_live_sibling() {
    let fixture = fixture();
    let linked = fixture
        ._temp
        .path()
        .join("residency-expiry-linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "residency-expiry-linked",
            linked.to_str().expect("UTF-8 linked worktree"),
        ],
    );
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
    manager.set_entry_provider(provider.clone());

    let expiring = manager
        .launch(&fixture.root)
        .expect("launch expiring workspace");
    let expiring_workspace_key = expiring.workspace.key.clone();
    let expiring_request_id = "launch-expiring-with-live-sibling";
    manager
        .retain_launch_replay(
            expiring_request_id,
            &launch_response_envelope(expiring_request_id, &expiring),
        )
        .expect("retain expiring replay");
    let expiring_payload = published_ticket_payload(launch_parts(&expiring).1);

    let sibling = manager.launch(&linked).expect("launch sibling workbench");
    let sibling_workspace_key = sibling.workspace.key.clone();
    let (_, sibling_ticket) = launch_parts(&sibling);
    let sibling_payload = published_ticket_payload(sibling_ticket);
    let sibling_entry = WorkbenchEntryBinding::published(
        sibling_payload.canonical_origin,
        sibling_payload.project_instance_id,
        sibling_payload.workspace_key,
    )
    .expect("sibling published entry");
    manager
        .inner
        .enroll_pairing(sibling_ticket, None, &sibling_entry)
        .expect("enroll sibling pairing");

    let now = unix_seconds();
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .pending_capabilities
        .get_mut(&expiring_payload.capability_id)
        .expect("expiring pending capability")
        .expires_at = now;

    assert!(manager.requires_daemon_residency(now));
    assert_eq!(
        provider.released_workspace_keys(),
        vec![expiring_workspace_key.clone()]
    );
    let state = manager.inner.state.lock().expect("workbench state");
    assert!(
        !state
            .pending_capabilities
            .contains_key(&expiring_payload.capability_id)
    );
    assert!(!state.launch_replays.contains_key(expiring_request_id));
    assert!(
        state.pairing_grants.values().any(|pairing| {
            pairing.workspace_key == sibling_workspace_key && pairing.is_live(now)
        })
    );
    drop(state);
    assert!(
        !provider
            .released_workspace_keys()
            .contains(&sibling_workspace_key)
    );

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn expired_publication_release_finishes_before_a_fresh_workspace_launch() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(BlockingReleasePublishedEntryProvider::default());
    manager.set_entry_provider(provider.clone());

    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = published_ticket_payload(ticket);
    let entry = WorkbenchEntryBinding::published(
        payload.canonical_origin,
        payload.project_instance_id,
        payload.workspace_key,
    )
    .expect("published workbench entry");
    manager
        .inner
        .enroll_pairing(ticket, None, &entry)
        .expect("enroll durable pairing");
    let now = unix_seconds();
    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let pairing = state
            .pairing_grants
            .values_mut()
            .next()
            .expect("durable pairing");
        pairing.idle_expires_at = now;
    }

    let maintenance_manager = manager.clone();
    let maintenance = tokio::task::spawn_blocking(move || {
        assert!(!maintenance_manager.requires_daemon_residency(now));
    });
    wait_for_workbench_condition("publication release", || {
        provider.release_started.load(Ordering::Acquire)
    })
    .await;

    let launch_manager = manager.clone();
    let launch_root = fixture.root.clone();
    let relaunch = tokio::task::spawn_blocking(move || launch_manager.launch(&launch_root));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        provider.resolves.load(Ordering::Acquire),
        1,
        "a fresh launch must wait until the previous publication release finishes"
    );

    provider.allow_release.store(true, Ordering::Release);
    maintenance.await.expect("join publication maintenance");
    relaunch
        .await
        .expect("join fresh launch")
        .expect("fresh launch after release");
    assert_eq!(provider.resolves.load(Ordering::Acquire), 2);
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn pairing_mutations_release_authorization_before_publication_io() {
    for forget in [false, true] {
        let fixture = fixture();
        let manager = test_manager(Arc::clone(&fixture.project));
        let provider = Arc::new(BlockingReleasePublishedEntryProvider::default());
        manager.set_entry_provider(provider.clone());

        let launch = manager.launch(&fixture.root).expect("launch workbench");
        let (_, ticket) = launch_parts(&launch);
        let payload = published_ticket_payload(ticket);
        let entry = WorkbenchEntryBinding::published(
            payload.canonical_origin,
            payload.project_instance_id,
            payload.workspace_key,
        )
        .expect("published workbench entry");
        manager
            .inner
            .enroll_pairing(ticket, None, &entry)
            .expect("enroll durable pairing");
        let selector = manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pairing_grants
            .keys()
            .next()
            .expect("durable pairing")
            .clone();

        let mutation_manager = manager.clone();
        let mutation = tokio::task::spawn_blocking(move || {
            if forget {
                mutation_manager.inner.forget_pairing(&selector, None)
            } else {
                mutation_manager.inner.revoke_pairing(&selector, None)
            }
        });
        wait_for_workbench_condition("blocked pairing publication release", || {
            provider.release_started.load(Ordering::Acquire)
        })
        .await;
        assert!(
            manager.inner.authorization_store_gate.try_lock().is_ok(),
            "pairing mutation publication I/O must not hold the authorization store gate"
        );

        provider.allow_release.store(true, Ordering::Release);
        mutation
            .await
            .expect("join pairing mutation")
            .expect("complete pairing mutation");
        manager.shutdown().await;
    }
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn forgetting_one_workspace_publication_preserves_another_live_workspace() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("residency-linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "residency-linked",
            linked.to_str().expect("UTF-8 linked worktree"),
        ],
    );
    let manager = test_manager(Arc::clone(&fixture.project));
    let provider = Arc::new(RebindTrackingPublishedEntryProvider::default());
    manager.set_entry_provider(provider.clone());

    let mut pairings = Vec::new();
    for workspace in [&fixture.root, &linked] {
        let launch = manager
            .launch(workspace)
            .expect("launch published workbench");
        let workspace_key = launch.workspace.key.clone();
        let (_, ticket) = launch_parts(&launch);
        let payload = published_ticket_payload(ticket);
        let entry = WorkbenchEntryBinding::published(
            payload.canonical_origin,
            payload.project_instance_id,
            payload.workspace_key,
        )
        .expect("published entry");
        manager
            .inner
            .enroll_pairing(ticket, None, &entry)
            .expect("enroll durable pairing");
        let selector = manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pairing_grants
            .values()
            .find(|pairing| pairing.workspace_key == workspace_key)
            .expect("workspace pairing")
            .selector
            .clone();
        pairings.push((workspace_key, selector));
    }

    manager
        .inner
        .forget_pairing(&pairings[0].1, None)
        .expect("forget first workspace pairing");
    assert!(manager.requires_daemon_residency(unix_seconds()));
    assert_eq!(
        provider.released_workspace_keys(),
        vec![pairings[0].0.clone()]
    );
    let state = manager.inner.state.lock().expect("workbench state");
    assert!(
        state
            .origin_bindings
            .values()
            .any(|binding| binding.workspace_key.as_deref() == Some(pairings[0].0.as_str()))
    );
    assert!(
        state
            .origin_bindings
            .values()
            .any(|binding| binding.workspace_key.as_deref() == Some(pairings[1].0.as_str()))
    );
    drop(state);

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn ticket_persistence_failure_restores_the_one_time_capability() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let response = launch_response_envelope("launch-persistence-rollback", &launch);
    manager
        .retain_launch_replay("launch-persistence-rollback", &response)
        .expect("retain launch replay");
    let (_, ticket) = launch_parts(&launch);
    let payload = ticket_payload(ticket);
    fs::create_dir(&manager.inner.authorization_store_path)
        .expect("block the session store with a directory");

    let mut replay_during_persistence = None;
    assert_eq!(
        manager.inner.redeem_ticket_with_before_persist(ticket, || {
            replay_during_persistence =
                manager.replay_launch_response("launch-persistence-rollback");
        }),
        Err(TicketExchangeError::Unavailable)
    );
    assert_same_response(
        &replay_during_persistence.expect("launch replays until redemption is durable"),
        &response,
    );
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pending_capabilities
            .contains_key(&payload.capability_id),
        "a failed durable exchange must restore the one-time capability"
    );
    assert_same_response(
        &manager
            .replay_launch_response("launch-persistence-rollback")
            .expect("failed persistence preserves the launch replay"),
        &response,
    );

    fs::remove_dir(&manager.inner.authorization_store_path)
        .expect("restore the session store destination");
    manager
        .inner
        .redeem_ticket(ticket)
        .expect("retry the restored ticket");
    assert!(
        manager
            .replay_launch_response("launch-persistence-rollback")
            .is_none(),
        "successful redemption removes the launch replay"
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn sessions_restore_and_renew_across_compatible_host_replacement() {
    let fixture = fixture();
    let first_manager =
        test_manager_with_identity(Arc::clone(&fixture.project), "first-workbench-instance");
    let launch = first_manager
        .launch(&fixture.root)
        .expect("launch first workbench");
    let (first_origin, ticket) = launch_parts(&launch);
    let first_origin = first_origin.to_string();
    let (session_secret, session) = first_manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem first session");
    let digest = session_credential_digest(&session_secret);
    let short_expiry = unix_seconds().saturating_add(60);
    {
        let mut state = first_manager
            .inner
            .state
            .lock()
            .expect("first workbench state");
        state
            .sessions
            .get_mut(&digest)
            .expect("first live session")
            .expires_at = short_expiry;
        state
            .session_grants
            .get_mut(&digest)
            .expect("first durable session")
            .expires_at = short_expiry;
        state
            .session_grants
            .get_mut(&digest)
            .expect("first durable session")
            .capabilities
            .retain(|capability| capability != "workbench.inspect");
    }
    first_manager
        .inner
        .persist_session_store()
        .expect("persist short session");
    let store_path = first_manager.inner.authorization_store_path.clone();
    let persisted = fs::read_to_string(&store_path).expect("read durable session");
    assert!(!persisted.contains(&session_secret));
    assert!(persisted.contains(&digest));
    assert!(
        !persisted.contains("workbench.inspect"),
        "the fixture must represent a session minted before inspection shipped"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&store_path)
                .expect("durable session metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    first_manager.shutdown().await;

    let replacement = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "replacement-workbench-instance",
    );
    wait_for_workbench_condition("compatible session host restoration", || {
        replacement.host_status().is_some()
    })
    .await;
    let replacement_origin = replacement
        .host_status()
        .expect("replacement resumes the compatible workbench host")
        .origin;
    assert_eq!(
        replacement_origin, first_origin,
        "a compatible replacement must preserve the browser's local origin"
    );
    let restored = replacement
        .inner
        .session(&session.session_key, &session_secret)
        .expect("restore durable session");
    assert_eq!(
        restored.workspace_root,
        fixture.root.canonicalize().expect("canonical workspace")
    );
    assert_eq!(restored.workspace_key, session.workspace_key);
    assert_eq!(restored.id, digest);
    assert!(
        restored.allows("workbench.inspect"),
        "a snapshot-capable durable session gains the compatible inspection read"
    );
    let stale_generation = planning::WorkbenchPlanningContext {
        schema_version: 1,
        session_id: restored.id.clone(),
        expected_daemon_instance_id: "first-workbench-instance".to_string(),
        expected_revision: replacement.inner.current_revision(),
        expected_phase_id: "phase-from-prior-generation".to_string(),
        operation: planning::WorkbenchPlanningContextOperation::TaskStart {
            task_id: "task-from-prior-generation".to_string(),
        },
    };
    let stale_generation = replacement
        .inner
        .validate_planning_context(&restored.workspace_root, &stale_generation, false)
        .expect_err("a prior daemon generation must not reuse a reset revision");
    assert_eq!(
        stale_generation
            .response("stale-generation".to_string())
            .error
            .expect("stale generation error")
            .details
            .expect("stale generation details")["kind"],
        "workbench.stale_snapshot"
    );
    assert!(
        replacement
            .inner
            .session("wrong-workspace-selector", &session_secret)
            .is_none(),
        "the cookie secret is not authorization without its public selector"
    );

    let renewed = replacement
        .inner
        .renew_session(&session.session_key, &session_secret)
        .expect("renew durable session")
        .expect("renewed session result");
    let renewed_expiry = DateTime::parse_from_rfc3339(&renewed.expires_at)
        .expect("parse renewed expiry")
        .timestamp();
    assert!(
        renewed_expiry > i64::try_from(short_expiry).expect("short expiry fits i64"),
        "renewal extends the durable grant and browser cookie horizon"
    );

    let cookie = format!(
        "{SESSION_COOKIE_PREFIX}{}={session_secret}",
        session.session_key
    );
    let renewed_over_same_origin = raw_http(
        &first_origin,
        "POST",
        "/api/session/renew",
        Some(json!({ "session_key": session.session_key }).to_string()),
        Some(&cookie),
        Some(&first_origin),
    )
    .await
    .expect("renew the restored session over the original browser origin");
    assert_eq!(
        renewed_over_same_origin.status, 200,
        "{renewed_over_same_origin:?}"
    );
    assert_eq!(renewed_over_same_origin.json()["kind"], "workbench.session");
    assert!(
        fs::read_to_string(&store_path)
            .expect("read upgraded durable session")
            .contains("workbench.inspect"),
        "renewal persists the upgraded capability set"
    );
    replacement.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn session_activity_survives_shutdown_inside_the_persistence_interval() {
    let fixture = fixture();
    let manager =
        test_manager_with_identity(Arc::clone(&fixture.project), "activity-workbench-instance");
    let launch = manager
        .launch(&fixture.root)
        .expect("launch activity workbench");
    let (_, ticket) = launch_parts(&launch);
    let (session_secret, session) = manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem activity session");
    let digest = session_credential_digest(&session_secret);
    let persisted_at = unix_seconds();
    let stale_activity = persisted_at.saturating_sub(1);
    {
        let mut state = manager
            .inner
            .state
            .lock()
            .expect("activity workbench state");
        let live = state
            .sessions
            .get_mut(&digest)
            .expect("live activity session");
        live.last_activity = stale_activity;
        live.last_persisted_at = persisted_at;
        state
            .session_grants
            .get_mut(&digest)
            .expect("durable activity session")
            .last_activity = stale_activity;
    }
    manager
        .inner
        .persist_session_store()
        .expect("persist stale activity fixture");

    let touched = manager
        .inner
        .session(&session.session_key, &session_secret)
        .expect("authenticate activity session");
    assert!(touched.last_activity > stale_activity);
    assert_eq!(
        manager
            .inner
            .state
            .lock()
            .expect("touched workbench state")
            .session_grants
            .get(&digest)
            .expect("touched durable grant")
            .last_activity,
        touched.last_activity,
        "the durable grant must track every authenticated activity touch"
    );
    let before_shutdown: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read throttled authorization store"),
    )
    .expect("decode throttled session store");
    assert_eq!(
        before_shutdown
            .sessions
            .iter()
            .find(|grant| grant.credential_digest == digest)
            .expect("persisted throttled grant")
            .last_activity,
        stale_activity,
        "the normal persistence interval still throttles filesystem writes"
    );

    manager.shutdown().await;
    let after_shutdown: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&manager.inner.authorization_store_path)
            .expect("read shutdown authorization store"),
    )
    .expect("decode shutdown session store");
    assert_eq!(
        after_shutdown
            .sessions
            .iter()
            .find(|grant| grant.credential_digest == digest)
            .expect("persisted shutdown grant")
            .last_activity,
        touched.last_activity,
        "shutdown must persist the latest authenticated activity"
    );
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn version_one_sessions_migrate_to_the_rollback_safe_authorization_store() {
    let fixture = fixture();
    let runtime_dir = fixture.project.runtime_dir();
    fs::create_dir_all(&runtime_dir).expect("create runtime directory");
    let legacy_path = runtime_dir.join("workbench.sessions.json");
    let archive_path = runtime_dir.join("workbench.sessions.v1.json");
    let authorization_path = runtime_dir.join("workbench.authorizations.json");
    let now = unix_seconds();
    let selector = "m".repeat(43);
    let credential_digest = "a".repeat(64);
    let legacy = WorkbenchSessionStoreV1 {
        schema_version: 1,
        project_id: fixture.project.id.to_string(),
        sessions: vec![WorkbenchSessionGrantV1 {
            credential_digest: credential_digest.clone(),
            selector: selector.clone(),
            project_id: fixture.project.id.to_string(),
            workspace_key: "w".repeat(43),
            workspace_root: fixture.root.canonicalize().expect("canonical workspace"),
            capabilities: vec!["workbench.snapshot".to_string()],
            entry: Some(
                WorkbenchEntryBinding::published(
                    "https://legacy.test.localhost".to_string(),
                    "legacy-project-instance".to_string(),
                    "w".repeat(43),
                )
                .expect("legacy published-shaped entry"),
            ),
            pairing_selector: Some("p".repeat(43)),
            created_at: now,
            last_activity: now,
            expires_at: now.saturating_add(SESSION_RENEWAL_LIFETIME.as_secs()),
        }],
    };
    fs::write(
        &legacy_path,
        serde_json::to_vec(&legacy).expect("serialize legacy store"),
    )
    .expect("write legacy session store");

    let manager = test_manager(Arc::clone(&fixture.project));
    assert_eq!(manager.inner.authorization_store_path, authorization_path);
    assert!(authorization_path.is_file());
    assert!(!legacy_path.exists());
    assert!(archive_path.is_file());
    let migrated: WorkbenchAuthorizationStoreV2 = serde_json::from_slice(
        &fs::read(&authorization_path).expect("read migrated authorization store"),
    )
    .expect("decode migrated authorization store");
    assert_eq!(migrated.schema_version, 2);
    assert!(migrated.pairings.is_empty());
    assert!(migrated.resume_outcomes.is_empty());
    assert_eq!(migrated.sessions.len(), 1);
    assert_eq!(migrated.sessions[0].selector, selector);
    assert!(migrated.sessions[0].entry.is_none());
    assert!(migrated.sessions[0].pairing_selector.is_none());
    assert_eq!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .session_grants
            .get(&credential_digest)
            .and_then(|grant| grant.entry.as_ref()),
        None
    );
    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn expired_durable_sessions_are_not_restored_by_a_replacement_host() {
    let fixture = fixture();
    let first_manager =
        test_manager_with_identity(Arc::clone(&fixture.project), "expiring-workbench-instance");
    let launch = first_manager
        .launch(&fixture.root)
        .expect("launch expiring workbench");
    let (_, ticket) = launch_parts(&launch);
    let (session_secret, session) = first_manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem expiring session");
    let digest = session_credential_digest(&session_secret);
    let expired_at = unix_seconds().saturating_sub(1);
    {
        let mut state = first_manager
            .inner
            .state
            .lock()
            .expect("expiring workbench state");
        state
            .sessions
            .get_mut(&digest)
            .expect("live expiring session")
            .expires_at = expired_at;
        state
            .session_grants
            .get_mut(&digest)
            .expect("durable expiring session")
            .expires_at = expired_at;
    }
    first_manager
        .inner
        .persist_session_store()
        .expect("persist expired session fixture");
    assert!(
        fs::read_to_string(&first_manager.inner.authorization_store_path)
            .expect("read expired session fixture")
            .contains(&digest),
        "the replacement must reject the persisted grant rather than relying on its removal"
    );
    first_manager.shutdown().await;

    let replacement =
        test_manager_with_identity(Arc::clone(&fixture.project), "expired-replacement-instance");
    assert!(
        replacement
            .inner
            .session(&session.session_key, &session_secret)
            .is_none(),
        "an expired durable grant must not restore across host replacement"
    );
    assert!(
        replacement
            .inner
            .state
            .lock()
            .expect("replacement workbench state")
            .session_grants
            .is_empty(),
        "expired grants are filtered while loading the durable store"
    );
    replacement.shutdown().await;
}

#[cfg(all(feature = "ui", unix))]
#[tokio::test(flavor = "multi_thread")]
async fn restored_sessions_keep_their_exact_linked_worktree_boundary() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("session-linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-session-linked",
            linked.to_str().expect("linked path"),
        ],
    );
    let linked_root = linked.canonicalize().expect("canonical linked worktree");
    let first_manager =
        test_manager_with_identity(Arc::clone(&fixture.project), "linked-session-instance");
    let launch = first_manager
        .launch(&linked_root)
        .expect("launch linked worktree");
    let (_, ticket) = launch_parts(&launch);
    let (session_secret, session) = first_manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem linked session");
    first_manager.shutdown().await;

    let replacement =
        test_manager_with_identity(Arc::clone(&fixture.project), "primary-host-instance");
    let primary_launch = replacement
        .launch(&fixture.root)
        .expect("launch replacement from primary worktree");
    assert_ne!(primary_launch.workspace.key, session.workspace_key);
    let restored = replacement
        .inner
        .session(&session.session_key, &session_secret)
        .expect("restore linked session through replacement host");
    assert_eq!(restored.workspace_root, linked_root);
    assert_ne!(
        restored.workspace_root,
        fixture.root.canonicalize().expect("canonical primary"),
        "restoration must not fall back to the host-launching worktree"
    );
    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked path"),
        ],
    );
    assert!(
        replacement
            .inner
            .renew_session(&session.session_key, &session_secret)
            .expect("validate removed linked worktree during renewal")
            .is_none(),
        "renewal must fail closed after the exact retained worktree disappears"
    );
    replacement.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn expired_tickets_and_session_bounds_fail_closed_without_consuming_live_tickets() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = ticket_payload(ticket);
    assert_eq!(
        payload.capabilities,
        std::iter::once("workbench.snapshot".to_string())
            .chain(std::iter::once("workbench.inspect".to_string()))
            .chain(std::iter::once("lane.focus".to_string()))
            .chain(
                planning::PLANNING_CAPABILITIES
                    .iter()
                    .map(ToString::to_string)
            )
            .collect::<Vec<_>>()
    );
    let now = unix_seconds();

    let secret = manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .host
        .as_ref()
        .expect("bound host")
        .secret;
    let expired_payload = WorkbenchTicketV1 {
        capability_id: random_token().expect("expired capability ID"),
        issued_at: now.saturating_sub(10),
        expires_at: now.saturating_sub(1),
        ..payload.clone()
    };
    let expired_ticket = sign_ticket(&secret, &expired_payload).expect("sign expired ticket");
    assert_eq!(
        manager.inner.redeem_ticket(&expired_ticket),
        Err(TicketExchangeError::Invalid)
    );

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let workspace_root = state
            .workspaces_by_key
            .get(&payload.workspace_key)
            .expect("registered workspace")
            .root
            .clone();
        for index in 0..MAX_SESSIONS {
            let session_id = format!("bounded-session-{index}");
            let session = WorkbenchSession {
                id: session_id.clone(),
                selector: format!("bounded-selector-{index}"),
                project_id: fixture.project.id.to_string(),
                workspace_key: payload.workspace_key.clone(),
                workspace_root: workspace_root.clone(),
                capabilities: payload.capabilities.clone(),
                entry: test_direct_entry(),
                pairing_selector: None,
                created_at: now,
                last_activity: now,
                expires_at: now + SESSION_RENEWAL_LIFETIME.as_secs(),
                last_persisted_at: now,
            };
            state
                .session_grants
                .insert(session_id.clone(), WorkbenchSessionGrantV1::from(&session));
            state.sessions.insert(session_id, session);
        }
    }
    assert_eq!(
        manager.inner.redeem_ticket(ticket),
        Err(TicketExchangeError::Busy)
    );
    assert!(
        manager
            .inner
            .state
            .lock()
            .expect("workbench state")
            .pending_capabilities
            .contains_key(&payload.capability_id),
        "session saturation must not consume the one-time ticket"
    );
    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        state.sessions.remove("bounded-session-0");
        state.session_grants.remove("bounded-session-0");
    }
    let (session_secret, session) = manager
        .inner
        .redeem_ticket(ticket)
        .expect("ticket remains redeemable after capacity returns");
    let session_id = session_credential_digest(&session_secret);

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let session = state.sessions.get_mut(&session_id).expect("new session");
        session.expires_at = now.saturating_sub(1);
        session.last_activity = now;
        let grant = state
            .session_grants
            .get_mut(&session_id)
            .expect("new session grant");
        grant.expires_at = now.saturating_sub(1);
        grant.last_activity = now;
        drop(state);
    }
    assert!(
        manager
            .inner
            .session(&session.session_key, &session_secret)
            .is_none(),
        "sessions expire when their renewable grant expires"
    );

    let idle_launch = manager.launch(&fixture.root).expect("launch idle session");
    let (_, idle_ticket) = launch_parts(&idle_launch);
    let (idle_session_secret, idle_session) = manager
        .inner
        .redeem_ticket(idle_ticket)
        .expect("redeem idle session ticket");
    let idle_session_id = session_credential_digest(&idle_session_secret);
    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let session = state
            .sessions
            .get_mut(&idle_session_id)
            .expect("idle session");
        session.last_activity = now.saturating_sub(SESSION_IDLE_LIFETIME.as_secs() + 1);
        state
            .session_grants
            .get_mut(&idle_session_id)
            .expect("idle session grant")
            .last_activity = now.saturating_sub(SESSION_IDLE_LIFETIME.as_secs() + 1);
        drop(state);
    }
    assert!(
        manager
            .inner
            .session(&idle_session.session_key, &idle_session_secret)
            .is_none(),
        "idle sessions expire"
    );
    manager.shutdown().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn retained_workspace_validation_rejects_same_project_root_substitution() {
    let fixture = fixture();
    let linked = fixture._temp.path().join("linked-worktree");
    run_git(
        &fixture.root,
        &[
            "worktree",
            "add",
            "-b",
            "workbench-root-substitution",
            linked.to_str().expect("linked path"),
        ],
    );
    let retained_root = linked.canonicalize().expect("canonical linked worktree");
    let manager = test_manager(Arc::clone(&fixture.project));
    assert_eq!(
        manager
            .inner
            .validate_session_workspace(&retained_root)
            .expect("original linked worktree is valid"),
        retained_root
    );

    run_git(
        &fixture.root,
        &[
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("linked path"),
        ],
    );
    std::os::unix::fs::symlink(&fixture.root, &linked)
        .expect("replace linked worktree with primary-worktree symlink");

    let error = manager
        .inner
        .validate_session_workspace(&retained_root)
        .expect_err("a retained path resolving to another worktree must fail");
    let failure = error
        .downcast_ref::<crate::failure::ExoFailure>()
        .expect("structured workspace failure");
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "workbench.workspace_unavailable"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn event_stream_admission_is_bounded() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let mut permits = Vec::new();
    for _ in 0..MAX_EVENT_STREAMS {
        permits.push(
            Arc::clone(&manager.inner.event_admission)
                .try_acquire_owned()
                .expect("event stream within admission bound"),
        );
    }
    assert!(
        Arc::clone(&manager.inner.event_admission)
            .try_acquire_owned()
            .is_err(),
        "the 33rd concurrent event stream must be rejected"
    );
    drop(permits);
    assert!(
        Arc::clone(&manager.inner.event_admission)
            .try_acquire_owned()
            .is_ok(),
        "event admission returns when a stream closes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_is_workspace_scoped_and_redacts_local_paths() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Workbench Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Workbench Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    writer
        .add_goal(
            &phase,
            "host-and-launch",
            "Build host and launch",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add goal");
    writer
        .add_task(
            "host-and-launch",
            "implement",
            "Implement the workbench host",
            None,
        )
        .expect("add task");
    writer
        .add_task_log("implement", "progress", "Captured browser evidence.")
        .expect("add browser-safe progress");
    writer
        .add_task_log(
            "implement",
            "note",
            &format!("Internal note from {}", fixture.root.display()),
        )
        .expect("add non-progress task note");
    for index in 0..10 {
        writer
            .add_task_log(
                "implement",
                "progress",
                &format!("Recent browser evidence {index}."),
            )
            .expect("add bounded browser-safe progress");
    }
    let lane = writer
        .add_workbench_lane(
            "Host and launch",
            "Deliver the local workbench substrate",
            &phase,
        )
        .expect("add lane");
    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    let workspace_text = workspace.to_string_lossy();
    writer
        .set_workspace_active_phase(&workspace_text, &phase)
        .expect("focus phase");
    writer
        .set_workspace_lane_focus(&workspace_text, &lane)
        .expect("focus lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let snapshot = manager.snapshot(&fixture.root).expect("read snapshot");
    assert_eq!(snapshot.project.id, fixture.project.id.as_str());
    assert_eq!(
        snapshot
            .focused_lane
            .as_ref()
            .map(|lane| lane.summary.id.as_str()),
        Some(lane.as_str())
    );
    assert_eq!(
        snapshot.phase.as_ref().map(|phase| phase.id.as_str()),
        Some(phase.as_str())
    );
    assert_eq!(snapshot.phase.as_ref().expect("phase").goals.len(), 1);
    assert_eq!(
        snapshot.phase.as_ref().expect("phase").goals[0].tasks.len(),
        1
    );
    let progress = &snapshot.phase.as_ref().expect("phase").goals[0].tasks[0].progress;
    assert_eq!(
        progress.len(),
        8,
        "the snapshot exposes only a bounded recent progress window"
    );
    assert_eq!(progress[0].message, "Recent browser evidence 2.");
    assert_eq!(progress[7].message, "Recent browser evidence 9.");
    assert!(!progress[0].created_at.is_empty());
    assert!(
        snapshot.phase.as_ref().expect("phase").goals[0].tasks[0].progress_truncated,
        "the snapshot identifies omitted progress history"
    );
    assert!(
        progress
            .iter()
            .all(|entry| !entry.message.contains("Internal note")),
        "the snapshot withholds non-progress task log kinds"
    );
    assert!(snapshot.diagnostics.is_empty());
    assert!(
        snapshot.phase.as_ref().expect("phase").planning_available,
        "an unowned focused phase is available for planning"
    );
    assert!(snapshot.steering.next_actions.iter().all(|action| matches!(
        action.intent.as_str(),
        "orient" | "plan" | "execute" | "record" | "verify" | "ship"
    )));

    let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
    for forbidden in [
        fixture.root.as_path(),
        fixture.project.state_root.as_path(),
        fixture.project.git_common_dir.as_path(),
        fixture.project.db_path().as_path(),
    ] {
        assert!(
            !serialized.contains(&forbidden.display().to_string()),
            "snapshot must not expose {}: {serialized}",
            forbidden.display()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lane_inspection_projects_bounded_history_without_changing_focus() {
    const MAX_PROGRESS_ENTRIES: usize = 8;
    const MAX_PROGRESS_BYTES: usize = 16 * 1024;
    const MAX_OUTCOME_BYTES: usize = 16 * 1024;

    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Inspection Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Historical Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .add_goal(
            &phase,
            "historical-goal",
            "Complete the historical slice",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add goal");
    writer
        .add_task(
            "historical-goal",
            "historical-task",
            "Land the historical slice",
            None,
        )
        .expect("add task");
    writer
        .add_task_log(
            "historical-task",
            "note",
            &format!("Private note from {}", fixture.root.display()),
        )
        .expect("add private note");
    for index in 0..10 {
        writer
            .add_task_log(
                "historical-task",
                "progress",
                &format!("Historical progress update {index}."),
            )
            .expect("add progress");
    }
    writer
        .add_task_log("historical-task", "progress", "")
        .expect("add empty progress");
    writer
        .add_task_log(
            "historical-task",
            "progress",
            &"x".repeat(MAX_PROGRESS_BYTES + 128),
        )
        .expect("add oversized progress");
    writer
        .complete_task("historical-task", &"y".repeat(MAX_OUTCOME_BYTES + 128))
        .expect("complete task");
    writer
        .update_goal_status("historical-goal", "completed")
        .expect("complete goal");
    writer
        .update_goal_completion_log("historical-goal", "The historical slice is available.")
        .expect("record goal outcome");
    let lane = writer
        .add_workbench_lane(
            "Historical Lane",
            "Explain how the project arrived here",
            &phase,
        )
        .expect("add historical lane");
    writer
        .update_phase_status(&phase, "completed")
        .expect("complete phase");
    drop(writer);

    let loader = SqliteLoader::open(fixture.project.db_path()).expect("open project loader");
    let bounded_details = loader
        .load_phase_details_by_id_with_bounded_history(
            &phase,
            MAX_PROGRESS_ENTRIES,
            MAX_PROGRESS_BYTES,
            MAX_OUTCOME_BYTES,
        )
        .expect("load bounded inspection details")
        .expect("bounded phase details");
    let bounded_logs = &bounded_details.goals[0].tasks[0].logs;
    assert_eq!(bounded_logs.len(), MAX_PROGRESS_ENTRIES + 1);
    assert!(bounded_logs.iter().all(|log| log.kind == "progress"));
    assert!(
        bounded_logs
            .iter()
            .all(|log| log.message.len() <= MAX_PROGRESS_BYTES + 4)
    );
    assert!(
        bounded_details.goals[0].tasks[0]
            .completion_log
            .as_ref()
            .is_some_and(|outcome| outcome.len() <= MAX_OUTCOME_BYTES + 4)
    );
    drop(loader);

    let manager = test_manager(Arc::clone(&fixture.project));
    let before = manager
        .snapshot(&fixture.root)
        .expect("read focus before inspection");
    let inspection = manager
        .inspect(&fixture.root, &lane)
        .expect("inspect historical lane");
    let after = manager
        .snapshot(&fixture.root)
        .expect("read focus after inspection");

    assert_eq!(inspection.kind, "workbench.lane_inspection");
    assert_eq!(inspection.schema_version, 2);
    assert_eq!(inspection.relationship, "historical");
    assert!(!inspection.can_focus_here);
    assert_eq!(inspection.lane.summary.id, lane);
    assert!(!inspection.phase.planning_available);
    assert_eq!(
        inspection.phase.goals[0].outcome.as_deref(),
        Some("The historical slice is available.")
    );
    assert_eq!(
        inspection.phase.goals[0].tasks[0]
            .outcome
            .as_ref()
            .map(String::len),
        Some(MAX_OUTCOME_BYTES)
    );
    assert!(inspection.phase.goals[0].tasks[0].outcome_truncated);
    assert_eq!(inspection.phase.goals[0].tasks[0].progress.len(), 1);
    assert_eq!(
        inspection.phase.goals[0].tasks[0].progress[0].message.len(),
        MAX_PROGRESS_BYTES
    );
    assert!(inspection.phase.goals[0].tasks[0].progress_truncated);
    assert_eq!(before.focused_lane, after.focused_lane);

    let serialized = serde_json::to_string(&inspection).expect("serialize inspection");
    for forbidden in [
        fixture.root.as_path(),
        fixture.project.state_root.as_path(),
        fixture.project.db_path().as_path(),
    ] {
        assert!(
            !serialized.contains(&forbidden.display().to_string()),
            "inspection must not expose {}: {serialized}",
            forbidden.display()
        );
    }

    let error = manager
        .inspect(&fixture.root, "missing-lane")
        .expect_err("missing lane must fail");
    let failure = error
        .downcast_ref::<crate::failure::ExoFailure>()
        .expect("structured missing-lane failure");
    assert_eq!(
        failure.error.code,
        crate::api::protocol::ErrorCode::NotFound
    );
    assert_eq!(
        failure.error.details.as_ref().expect("failure details")["kind"],
        "workbench.lane_not_found"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_projects_truthful_between_phase_context_for_the_workspace() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Trajectory Epoch", None, &[])
        .expect("add epoch");
    let finished_last = writer
        .add_phase(&epoch, "Finished last", "regular", None, &[])
        .expect("add first phase");
    let later_in_roadmap = writer
        .add_phase(&epoch, "Later in roadmap", "regular", None, &[])
        .expect("add later phase");
    let up_next = writer
        .add_phase(&epoch, "Up next", "regular", None, &[])
        .expect("add pending phase");

    writer
        .add_goal(
            &finished_last,
            "finished-goal",
            "Finish the current slice",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add completed goal");
    writer
        .update_goal_status("finished-goal", "completed")
        .expect("complete goal");
    writer
        .update_goal_completion_log(
            "finished-goal",
            &format!("Private evidence from {}", fixture.root.display()),
        )
        .expect("record private completion evidence");
    writer
        .add_goal(
            &up_next,
            "next-goal",
            "Build the next slice",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add next goal");
    writer
        .replace_phase_rfcs(&up_next, &["10204".to_string()])
        .expect("associate next RFC");

    writer
        .update_phase_status(&later_in_roadmap, "completed")
        .expect("complete later roadmap phase first");
    writer
        .update_phase_status(&finished_last, "in-progress")
        .expect("start first roadmap phase later");
    let finished_lane = writer
        .add_workbench_lane(
            "Finished lane",
            "Complete the current trajectory slice",
            &finished_last,
        )
        .expect("add finished lane");
    writer
        .add_workbench_lane("Prepared lane", "Prepare the next slice", &up_next)
        .expect("add prepared lane");
    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    let workspace_text = workspace.to_string_lossy();
    writer
        .focus_workbench_lane(&workspace_text, &finished_lane, &finished_last)
        .expect("focus active lane");
    writer
        .complete_phase_and_clear_lane_focus(&finished_last)
        .expect("complete focused phase");
    writer
        .database()
        .connection()
        .execute(
            "UPDATE phases SET completed_at = ?1 WHERE text_id = ?2",
            ("2026-07-01T12:00:00+00:00", &later_in_roadmap),
        )
        .expect("set older completion evidence");
    writer
        .database()
        .connection()
        .execute(
            "UPDATE phases SET completed_at = ?1 WHERE text_id = ?2",
            ("2026-08-01T12:00:00+00:00", &finished_last),
        )
        .expect("set newer completion evidence");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let snapshot = manager.snapshot(&fixture.root).expect("read snapshot");
    assert_eq!(snapshot.schema_version, 4);
    assert!(snapshot.focused_lane.is_none());
    assert!(snapshot.phase.is_none());
    assert_eq!(
        snapshot
            .lanes
            .iter()
            .find(|lane| lane.id == finished_lane)
            .and_then(|lane| lane.phase_completed_at.as_deref()),
        Some("2026-08-01T12:00:00+00:00")
    );
    assert!(
        snapshot
            .lanes
            .iter()
            .find(|lane| lane.phase_id == up_next)
            .is_some_and(|lane| lane.phase_completed_at.is_none())
    );

    let context = snapshot
        .between_phases_context
        .as_ref()
        .expect("between-phase context");
    assert_eq!(context.epoch_id, epoch);
    assert_eq!(context.pending_phases, 1);
    assert_eq!(
        context
            .completed_phase
            .as_ref()
            .map(|phase| phase.id.as_str()),
        Some(finished_last.as_str())
    );
    assert_eq!(
        context
            .completed_phase
            .as_ref()
            .map(|phase| phase.completed_at.as_str()),
        Some("2026-08-01T12:00:00+00:00")
    );
    assert_eq!(
        context.next_phase.as_ref().map(|phase| phase.id.as_str()),
        Some(up_next.as_str())
    );
    assert_eq!(
        context.next_phase.as_ref().map(|phase| phase.goal_count),
        Some(1)
    );
    assert_eq!(
        context.next_phase.as_ref().map(|phase| phase.rfc_count),
        Some(1)
    );

    let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
    assert!(
        !serialized.contains(&fixture.root.display().to_string()),
        "browser trajectory must omit private completion evidence: {serialized}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_does_not_treat_an_unfocused_active_phase_as_between_phases() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Active Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Active without lane focus", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    writer
        .set_workspace_active_phase(&workspace.to_string_lossy(), &phase)
        .expect("focus phase without a lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let snapshot = manager.snapshot(&fixture.root).expect("read snapshot");
    assert!(snapshot.focused_lane.is_none());
    assert!(snapshot.phase.is_none());
    assert!(snapshot.between_phases_context.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn planning_is_read_only_when_the_focused_phase_is_owned_elsewhere() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Owned Planning Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Owned Planning Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    writer
        .add_goal(
            &phase,
            "owned-goal",
            "Plan under the current owner",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add goal");
    writer
        .add_task("owned-goal", "owned-task", "Respect the owner", None)
        .expect("add task");
    let lane = writer
        .add_workbench_lane("Owned lane", "Respect phase ownership", &phase)
        .expect("add lane");
    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    writer
        .focus_workbench_lane(&workspace.to_string_lossy(), &lane, &phase)
        .expect("focus lane and phase");
    writer
        .set_phase_owner(
            &phase,
            "workspace",
            "workspace:foreign-project:owner",
            Some("workspace:foreign-project:owner"),
            Some("/foreign/workspace"),
        )
        .expect("assign another phase owner");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let registered = manager
        .register_workspace(&fixture.root)
        .expect("register workspace");
    let now = unix_seconds();
    let session = WorkbenchSession {
        id: "owned-phase-session".to_string(),
        selector: "owned-phase-selector".to_string(),
        project_id: fixture.project.id.to_string(),
        workspace_key: registered.key,
        workspace_root: registered.root.clone(),
        capabilities: planning::PLANNING_CAPABILITIES
            .iter()
            .map(ToString::to_string)
            .collect(),
        entry: test_direct_entry(),
        pairing_selector: None,
        created_at: now,
        last_activity: now,
        expires_at: now + SESSION_RENEWAL_LIFETIME.as_secs(),
        last_persisted_at: now,
    };
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .insert(session.id.clone(), session.clone());

    let snapshot = manager.snapshot(&fixture.root).expect("read snapshot");
    assert!(
        !snapshot.phase.as_ref().expect("phase").planning_available,
        "a foreign-owned phase must remain visible but read-only"
    );

    let context = planning::WorkbenchPlanningContext {
        schema_version: 1,
        session_id: session.id,
        expected_daemon_instance_id: "test-workbench-instance".to_string(),
        expected_revision: snapshot.revision,
        expected_phase_id: phase,
        operation: planning::WorkbenchPlanningContextOperation::TaskUpdate {
            task_id: "owned-task".to_string(),
        },
    };
    let error = manager
        .inner
        .validate_planning_context(&registered.root, &context, false)
        .expect_err("foreign-owned planning must be rejected");
    assert_eq!(
        error
            .response("owned-phase-request".to_string())
            .error
            .expect("planning error")
            .details
            .expect("planning details")["kind"],
        "workbench.invalid_transition"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn completion_reviews_are_non_mutating_replayable_and_session_bound() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Planning Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Planning Phase", "regular", None, &[])
        .expect("add phase");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    writer
        .add_goal(
            &phase,
            "planning-goal",
            "Plan through the workbench",
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
        )
        .expect("add goal");
    writer
        .add_task("planning-goal", "review-task", "Review the outcome", None)
        .expect("add task");
    writer
        .update_task_status("review-task", "in-progress")
        .expect("start task");
    let lane = writer
        .add_workbench_lane("Planning lane", "Review task outcomes", &phase)
        .expect("add lane");
    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    writer
        .focus_workbench_lane(&workspace.to_string_lossy(), &lane, &phase)
        .expect("focus lane and phase");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let registered = manager
        .register_workspace(&fixture.root)
        .expect("register workspace");
    let now = unix_seconds();
    let session = WorkbenchSession {
        id: "review-session".to_string(),
        selector: "review-selector".to_string(),
        project_id: fixture.project.id.to_string(),
        workspace_key: registered.key,
        workspace_root: registered.root,
        capabilities: planning::PLANNING_CAPABILITIES
            .iter()
            .map(ToString::to_string)
            .collect(),
        entry: test_direct_entry(),
        pairing_selector: None,
        created_at: now,
        last_activity: now,
        expires_at: now + SESSION_RENEWAL_LIFETIME.as_secs(),
        last_persisted_at: now,
    };
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .insert(session.id.clone(), session.clone());

    let first = manager
        .inner
        .completion_review(
            &session,
            "review-request",
            "test-workbench-instance",
            0,
            &phase,
            "review-task",
            "Implemented the bounded planning contract.",
        )
        .expect("build completion review");
    let replay = manager
        .inner
        .completion_review(
            &session,
            "review-request",
            "test-workbench-instance",
            0,
            &phase,
            "review-task",
            "Implemented the bounded planning contract.",
        )
        .expect("replay completion review");
    assert_eq!(first, replay);
    assert_eq!(first.task_id, "review-task");
    assert!(!first.approval_evidence_present);

    let writer = SqliteWriter::open(fixture.project.db_path()).expect("reopen project writer");
    let task = writer
        .resolve_task_reference("review-task")
        .expect("resolve task")
        .expect("task exists");
    let status: String = writer
        .database()
        .connection()
        .query_row(
            "SELECT status FROM tasks_data WHERE id = ?1",
            [task.row_id],
            |row| row.get(0),
        )
        .expect("read task status");
    assert_eq!(status, "in-progress", "review must not complete the task");
    drop(writer);

    let changed_payload = manager
        .inner
        .completion_review(
            &session,
            "review-request",
            "test-workbench-instance",
            0,
            &phase,
            "review-task",
            "A different outcome.",
        )
        .expect_err("one request ID cannot review a different outcome");
    assert_eq!(
        changed_payload
            .response("changed".to_string())
            .error
            .unwrap()
            .details
            .unwrap()["kind"],
        "workbench.invalid_input"
    );

    let approval = manager
        .inner
        .prepare_completion_approval(
            &session,
            "approval-request",
            "test-workbench-instance",
            0,
            &phase,
            &first.review_id,
            "review-task",
            "Implemented the bounded planning contract.",
        )
        .expect("prepare exact approval");
    assert_eq!(approval.task_id, "review-task");
    assert_eq!(
        approval.proposed_outcome,
        "Implemented the bounded planning contract."
    );
    for (task_id, outcome) in [
        ("other-task", "Implemented the bounded planning contract."),
        ("review-task", "A browser-edited outcome."),
    ] {
        let mismatch = manager
            .inner
            .prepare_completion_approval(
                &session,
                "mismatched-approval",
                "test-workbench-instance",
                0,
                &phase,
                &first.review_id,
                task_id,
                outcome,
            )
            .expect_err("browser approval must exactly match the server-held review");
        assert_eq!(
            mismatch
                .response("mismatch".to_string())
                .error
                .unwrap()
                .details
                .unwrap()["kind"],
            "workbench.review_invalid"
        );
    }

    manager.revision_after_write();
    let stale = manager
        .inner
        .completion_review(
            &session,
            "stale-review",
            "test-workbench-instance",
            0,
            &phase,
            "review-task",
            "Stale outcome.",
        )
        .expect_err("stale review must be rejected");
    assert_eq!(
        stale
            .response("stale".to_string())
            .error
            .unwrap()
            .details
            .unwrap()["kind"],
        "workbench.stale_snapshot"
    );

    for index in 0..planning::MAX_COMPLETION_REVIEWS_PER_SESSION {
        manager
            .inner
            .completion_review(
                &session,
                &format!("bounded-review-{index}"),
                "test-workbench-instance",
                1,
                &phase,
                "review-task",
                "Implemented the bounded planning contract.",
            )
            .expect("build bounded completion review");
    }
    let state = manager.inner.state.lock().expect("workbench state");
    assert_eq!(
        state
            .completion_reviews
            .values()
            .filter(|review| review.session_id == session.id)
            .count(),
        planning::MAX_COMPLETION_REVIEWS_PER_SESSION
    );
    assert_eq!(
        state
            .completion_review_requests
            .keys()
            .filter(|key| key.session_id == session.id)
            .count(),
        planning::MAX_COMPLETION_REVIEWS_PER_SESSION
    );
    assert!(
        !state.completion_reviews.contains_key(&first.review_id),
        "the oldest transient review should be evicted at the session bound"
    );
}

#[test]
fn planning_requests_reject_unknown_fields_and_invalid_text_bounds() {
    let request = json!({
        "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
        "id": "planning-request",
        "session_key": "planning-session",
        "expected_daemon_instance_id": "test-workbench-instance",
        "expected_revision": 3,
        "expected_phase_id": "planning-phase",
        "operation": {
            "kind": "task_add",
            "goal_id": "planning-goal",
            "title": "Plan in the browser",
        },
    });
    serde_json::from_value::<planning::BrowserPlanningRequest>(request.clone())
        .expect("closed planning request");

    let mut unknown_request_field = request.clone();
    unknown_request_field["workspace_root"] = json!("/tmp/not-allowed");
    serde_json::from_value::<planning::BrowserPlanningRequest>(unknown_request_field)
        .expect_err("caller-supplied workspace roots must be rejected");

    let mut unknown_operation_field = request;
    unknown_operation_field["operation"]["command"] = json!("task add");
    serde_json::from_value::<planning::BrowserPlanningRequest>(unknown_operation_field)
        .expect_err("generic command text must be rejected");

    let approval = json!({
        "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
        "id": "planning-approval",
        "session_key": "planning-session",
        "expected_daemon_instance_id": "test-workbench-instance",
        "expected_revision": 3,
        "expected_phase_id": "planning-phase",
        "operation": {
            "kind": "task_complete_approve",
            "review_id": "review-id",
            "task_id": "planning-task",
            "outcome": "Recorded the exact reviewed outcome.",
        },
    });
    serde_json::from_value::<planning::BrowserPlanningRequest>(approval.clone())
        .expect("approval carries exact replay material");
    let mut incomplete_approval = approval;
    incomplete_approval["operation"]
        .as_object_mut()
        .expect("approval operation")
        .remove("outcome");
    serde_json::from_value::<planning::BrowserPlanningRequest>(incomplete_approval)
        .expect_err("approval without exact outcome replay material must be rejected");

    assert_eq!(
        planning::normalize_title("  Plan together  ").expect("trim valid title"),
        "Plan together"
    );
    for invalid in ["", " \t ", "line one\nline two"] {
        planning::normalize_title(invalid).expect_err("invalid title");
    }
    planning::normalize_title(&"x".repeat(512)).expect("512-byte title");
    planning::normalize_title(&"x".repeat(513)).expect_err("oversized title");

    planning::validate_message("Outcome\nwith detail").expect("valid multiline outcome");
    for invalid in ["", " \t "] {
        planning::validate_message(invalid).expect_err("invalid message");
    }
    planning::validate_message(&"x".repeat(16 * 1024)).expect("16 KiB message");
    planning::validate_message(&"x".repeat(16 * 1024 + 1)).expect_err("oversized message");
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_database_reads_share_one_sqlite_snapshot() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Snapshot Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Snapshot Phase", "regular", None, &[])
        .expect("add phase");
    let lane = writer
        .add_workbench_lane("Snapshot lane", "Prove one read view", &phase)
        .expect("add lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let registered = manager
        .register_workspace(&fixture.root)
        .expect("register workspace");
    let workspace_root = registered.root.to_string_lossy().into_owned();
    let snapshot = snapshot::build_with_after_state_hook(
        &fixture.project,
        &registered,
        manager.inner.current_revision(),
        &manager.inner.instance_id,
        || {
            let writer =
                SqliteWriter::open(fixture.project.db_path()).expect("open concurrent writer");
            writer
                .update_phase_status(&phase, "in-progress")
                .expect("start phase after snapshot state read");
            writer
                .focus_workbench_lane(&workspace_root, &lane, &phase)
                .expect("focus lane after snapshot state read");
        },
    )
    .expect("read transactionally consistent snapshot");

    assert!(
        snapshot.focused_lane.is_none(),
        "the snapshot must not combine focus committed after its plan read"
    );
    assert!(snapshot.phase.is_none());

    let refreshed = snapshot::build(
        &fixture.project,
        &registered,
        manager.inner.current_revision(),
        &manager.inner.instance_id,
    )
    .expect("read refreshed snapshot");
    assert_eq!(
        refreshed
            .focused_lane
            .as_ref()
            .map(|lane| lane.summary.id.as_str()),
        Some(lane.as_str())
    );
    assert_eq!(
        refreshed.phase.as_ref().map(|phase| phase.id.as_str()),
        Some(phase.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_revision_and_database_state_share_the_project_state_gate() {
    let fixture = fixture();
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    let epoch = writer
        .add_epoch("Snapshot Gate Epoch", None, &[])
        .expect("add epoch");
    let phase = writer
        .add_phase(&epoch, "Snapshot Gate Phase", "regular", None, &[])
        .expect("add phase");
    let lane = writer
        .add_workbench_lane("Snapshot gate lane", "Serialize the snapshot", &phase)
        .expect("add lane");
    drop(writer);

    let manager = test_manager(Arc::clone(&fixture.project));
    let project_state_guard = manager
        .inner
        .project_state_gate
        .lock()
        .expect("project state gate");
    let (before_gate_tx, before_gate_rx) = std::sync::mpsc::channel();
    let (snapshot_tx, snapshot_rx) = std::sync::mpsc::channel();
    let snapshot_manager = manager.clone();
    let snapshot_root = fixture.root.clone();
    let snapshot_thread = std::thread::spawn(move || {
        let snapshot = snapshot_manager.snapshot_with_before_state_gate(&snapshot_root, || {
            before_gate_tx
                .send(())
                .expect("announce snapshot gate wait");
        });
        snapshot_tx.send(snapshot).expect("send snapshot result");
    });
    before_gate_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("snapshot reached the project state gate");
    assert!(
        snapshot_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "snapshot creation must wait while an atomic project-state write owns the gate"
    );

    let workspace = fixture.root.canonicalize().expect("canonical workspace");
    let writer = SqliteWriter::open(fixture.project.db_path()).expect("open project writer");
    writer
        .update_phase_status(&phase, "in-progress")
        .expect("start phase");
    writer
        .focus_workbench_lane(&workspace.to_string_lossy(), &lane, &phase)
        .expect("focus lane and phase");
    drop(writer);
    let revision = manager.revision_after_write();
    drop(project_state_guard);

    let snapshot = snapshot_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("snapshot completed after the write")
        .expect("read coherent snapshot");
    snapshot_thread.join().expect("join snapshot thread");
    assert_eq!(snapshot.revision, revision);
    assert_eq!(
        snapshot.phase.as_ref().map(|phase| phase.id.as_str()),
        Some(phase.as_str())
    );
    assert_eq!(
        snapshot
            .focused_lane
            .as_ref()
            .map(|lane| lane.summary.id.as_str()),
        Some(lane.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_samples_git_before_entering_the_project_state_gate() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));

    let snapshot = manager
        .snapshot_with_before_state_gate(&fixture.root, || {
            run_git(&fixture.root, &["checkout", "-b", "changed-after-sample"]);
        })
        .expect("capture snapshot");
    assert_eq!(
        snapshot.workspace.branch.as_deref(),
        Some("main"),
        "the snapshot must use Git metadata captured before entering the project-state gate",
    );

    let refreshed = manager
        .snapshot(&fixture.root)
        .expect("capture refreshed snapshot");
    assert_eq!(
        refreshed.workspace.branch.as_deref(),
        Some("changed-after-sample"),
        "the next snapshot must observe workspace changes",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_storage_failures_are_stable_and_path_free() {
    let fixture = fixture();
    let database_path = fixture.project.db_path();
    fs::remove_file(&database_path).expect("remove fixture database");
    fs::create_dir(&database_path).expect("replace fixture database with a directory");
    let manager = test_manager(Arc::clone(&fixture.project));

    let error = manager
        .snapshot(&fixture.root)
        .expect_err("broken snapshot storage must fail");
    let failure = error
        .downcast_ref::<crate::failure::ExoFailure>()
        .expect("structured workbench snapshot failure");
    assert_eq!(
        failure.error.details.as_ref().expect("details")["kind"],
        "workbench.snapshot_unavailable"
    );
    let serialized =
        serde_json::to_string(&failure.error).expect("serialize workbench error response");
    for forbidden in [
        fixture.root.as_path(),
        fixture.project.state_root.as_path(),
        database_path.as_path(),
    ] {
        assert!(
            !serialized.contains(&forbidden.display().to_string()),
            "snapshot failure must not expose {}: {serialized}",
            forbidden.display()
        );
    }
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn http_surface_enforces_origin_session_capability_and_body_bounds() {
    let fixture = fixture();
    let seen = Arc::new(Mutex::new(Vec::<RequestEnvelope>::new()));
    let replay_seen = Arc::new(Mutex::new(Vec::<RequestEnvelope>::new()));
    let preparation_probe_seen = Arc::new(Mutex::new(Vec::<RequestEnvelope>::new()));
    let manager = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("test-http-instance"),
        Arc::from("test-http-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let dispatch_seen = Arc::clone(&seen);
    let replay_capture = Arc::clone(&replay_seen);
    let preparation_probe_capture = Arc::clone(&preparation_probe_seen);
    manager
        .set_dispatcher(
            DaemonRequestDispatcher::new(move |request| {
                let dispatch_seen = Arc::clone(&dispatch_seen);
                async move {
                    dispatch_seen
                        .lock()
                        .expect("record dispatched request")
                        .push(request.clone());
                    let (effect, result) = match &request.op {
                        Op::Call(call)
                            if matches!(
                                &call.address,
                                crate::api::protocol::Address::Operation { path }
                                    if path.as_slice() == ["task", "add"]
                            ) =>
                        {
                            (
                                Effect::Write,
                                json!({
                                    "kind": "task.add",
                                    "ok": true,
                                    "task_id": "browser-task",
                                }),
                            )
                        }
                        Op::Call(call)
                            if matches!(
                                &call.address,
                                crate::api::protocol::Address::Operation { path }
                                    if path.as_slice() == ["task", "complete"]
                            ) =>
                        {
                            (
                                Effect::Write,
                                json!({
                                    "kind": "task.complete",
                                    "ok": true,
                                    "task_id": call.input["id"],
                                }),
                            )
                        }
                        _ => (Effect::Pure, json!({ "kind": "test.dispatch", "ok": true })),
                    };
                    ResponseEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        id: request.id,
                        status: Status::Ok,
                        result: Some(result),
                        error: None,
                        ticket: None,
                        steering: None,
                        reminders: None,
                        display: None,
                        preview: None,
                        effect: Some(effect),
                        trace: None,
                    }
                }
            })
            .with_terminal_replay(move |request| {
                let replay_capture = Arc::clone(&replay_capture);
                async move {
                    replay_capture
                        .lock()
                        .expect("record terminal replay request")
                        .push(request.clone());
                    Ok(
                        (request.id == "browser-terminal-approval").then(|| ResponseEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            id: request.id,
                            status: Status::Ok,
                            result: Some(json!({
                                "kind": "task.complete",
                                "ok": true,
                                "task_id": "terminal-task",
                            })),
                            error: None,
                            ticket: None,
                            steering: None,
                            reminders: None,
                            display: None,
                            preview: None,
                            effect: Some(Effect::Write),
                            trace: None,
                        }),
                    )
                }
            })
            .with_atomic_preparation_probe(move |request| {
                let preparation_probe_capture = Arc::clone(&preparation_probe_capture);
                async move {
                    preparation_probe_capture
                        .lock()
                        .expect("record atomic preparation probe")
                        .push(request.clone());
                    Ok(request.id != "browser-canonical-approval")
                }
            }),
        )
        .expect("install test dispatcher");

    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let (origin, ticket) = launch_parts(&launch);
    let asset = raw_http(origin, "GET", "/", None, None, None)
        .await
        .expect("fetch embedded index");
    assert_eq!(asset.status, 200);
    assert_eq!(
        asset.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert!(
        asset
            .headers
            .get("content-security-policy")
            .is_some_and(|value| value.contains("frame-ancestors 'none'"))
    );

    let foreign_launch = manager
        .launch(&fixture.root)
        .expect("launch foreign-origin ticket");
    let foreign_ticket = launch_parts(&foreign_launch).1;
    let foreign = raw_http(
        origin,
        "POST",
        "/api/session",
        Some(json!({ "ticket": foreign_ticket }).to_string()),
        None,
        Some("http://127.0.0.1:1"),
    )
    .await
    .expect("foreign-origin session exchange");
    assert_eq!(foreign.status, 403);
    assert_eq!(foreign.json()["kind"], "workbench.origin_mismatch");

    let session = raw_http(
        origin,
        "POST",
        "/api/session",
        Some(json!({ "ticket": ticket }).to_string()),
        None,
        Some(origin),
    )
    .await
    .expect("exchange launch ticket");
    assert_eq!(session.status, 200, "{session:?}");
    assert_eq!(session.json()["kind"], "workbench.session");
    let session_key = session.json()["session_key"]
        .as_str()
        .expect("workbench session key")
        .to_string();
    let cookie = session
        .headers
        .get("set-cookie")
        .expect("session cookie")
        .clone();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Max-Age=43200"));
    assert!(!cookie.contains("Domain="));
    assert_eq!(
        session.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    let cookie_pair = cookie.split(';').next().expect("cookie pair");
    let renewed = raw_http(
        origin,
        "POST",
        "/api/session/renew",
        Some(json!({ "session_key": session_key }).to_string()),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("renew browser session");
    assert_eq!(renewed.status, 200, "{renewed:?}");
    assert_eq!(renewed.json()["kind"], "workbench.session");
    assert_eq!(renewed.json()["session_key"], session_key);
    assert!(
        renewed
            .headers
            .get("set-cookie")
            .is_some_and(|value| value.contains("Max-Age=43200"))
    );

    let replay = raw_http(
        origin,
        "POST",
        "/api/session",
        Some(json!({ "ticket": ticket }).to_string()),
        None,
        Some(origin),
    )
    .await
    .expect("replay launch ticket");
    assert_eq!(replay.status, 401);
    assert_eq!(replay.json()["kind"], "workbench.ticket_invalid");

    let command = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": PROTOCOL_VERSION,
                "id": "browser-snapshot",
                "session_key": session_key,
                "operation": { "kind": "snapshot" },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("dispatch browser snapshot");
    assert_eq!(command.status, 200, "{command:?}");
    assert_eq!(command.json()["id"], "browser-snapshot");
    {
        let dispatched = seen.lock().expect("dispatched requests");
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].id, "browser-snapshot");
        assert_eq!(
            dispatched[0].workspace_root,
            Some(fixture.root.canonicalize().expect("canonical fixture root"))
        );
    }
    assert_eq!(
        command.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );

    let inspection = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": PROTOCOL_VERSION,
                "id": "browser-inspection",
                "session_key": session_key,
                "operation": {
                    "kind": "lane_inspect",
                    "lane_id": "lane-history",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("dispatch browser lane inspection");
    assert_eq!(inspection.status, 200, "{inspection:?}");
    {
        let dispatched = seen.lock().expect("dispatched requests");
        assert_eq!(dispatched.len(), 2);
        assert_eq!(dispatched[1].id, "browser-inspection");
        let Op::Call(call) = &dispatched[1].op else {
            panic!("lane inspection must dispatch an operation call");
        };
        assert!(matches!(
            &call.address,
            crate::api::protocol::Address::Operation { path }
                if path.as_slice() == ["workbench", "inspect"]
        ));
        assert_eq!(call.input, json!({ "id": "lane-history" }));
    }

    let review_permits = manager
        .inner
        .completion_review_admission()
        .try_acquire_many_owned(planning::MAX_COMPLETION_REVIEWS_IN_FLIGHT as u32)
        .expect("acquire all completion review permits");
    let review_busy = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
                "id": "browser-busy-review",
                "session_key": session_key,
                "expected_daemon_instance_id": "test-workbench-instance",
                "expected_revision": 4,
                "expected_phase_id": "phase-browser",
                "operation": {
                    "kind": "task_complete_review",
                    "task_id": "browser-task",
                    "outcome": "Reviewed browser completion.",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("bound completion review admission");
    assert_eq!(review_busy.status, 200, "{review_busy:?}");
    assert_eq!(review_busy.json()["status"], "error", "{review_busy:?}");
    assert_eq!(
        review_busy.json()["error"]["details"]["kind"],
        "workbench.busy",
        "{review_busy:?}"
    );
    assert_eq!(
        review_busy.json()["error"]["details"]["retry_with_same_request_id"],
        true,
        "{review_busy:?}"
    );
    drop(review_permits);

    let planning = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
                "id": "browser-task-add",
                "session_key": session_key,
                "expected_daemon_instance_id": "test-workbench-instance",
                "expected_revision": 4,
                "expected_phase_id": "phase-browser",
                "operation": {
                    "kind": "task_add",
                    "goal_id": "goal-browser",
                    "title": "  Add a browser task  ",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("dispatch browser planning request");
    assert_eq!(planning.status, 200, "{planning:?}");
    assert_eq!(
        planning.json()["result"],
        json!({
            "kind": "workbench.task_mutation",
            "ok": true,
            "schema_version": 1,
            "operation": "task_add",
            "task_id": "browser-task",
        })
    );
    {
        let dispatched = seen.lock().expect("dispatched requests");
        assert_eq!(dispatched.len(), 3);
        let Op::Call(call) = &dispatched[2].op else {
            panic!("planning request must be a call");
        };
        assert_eq!(
            call.address,
            crate::api::protocol::Address::Operation {
                path: vec!["task".to_string(), "add".to_string()]
            }
        );
        assert_eq!(call.input["label"], "Add a browser task");
        assert_eq!(call.input["goal"], "goal-browser");
        assert_eq!(
            call.input[planning::PLANNING_CONTEXT_FIELD]["expected_daemon_instance_id"],
            "test-workbench-instance"
        );
        assert_eq!(
            call.input[planning::PLANNING_CONTEXT_FIELD]["expected_revision"],
            4
        );
        assert_eq!(
            call.input[planning::PLANNING_CONTEXT_FIELD]["expected_phase_id"],
            "phase-browser"
        );
        assert!(
            !planning.json().to_string().contains("goal-browser"),
            "browser acknowledgement must not echo planning context"
        );
    }

    let terminal_approval = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
                "id": "browser-terminal-approval",
                "session_key": session_key,
                "expected_daemon_instance_id": "retired-daemon-instance",
                "expected_revision": 3,
                "expected_phase_id": "retired-phase",
                "operation": {
                    "kind": "task_complete_approve",
                    "review_id": "evicted-review",
                    "task_id": "terminal-task",
                    "outcome": "Recorded the exact terminal browser outcome.",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("replay terminal browser approval");
    assert_eq!(terminal_approval.status, 200, "{terminal_approval:?}");
    assert_eq!(
        terminal_approval.json()["result"],
        json!({
            "kind": "workbench.task_mutation",
            "ok": true,
            "schema_version": 1,
            "operation": "task_complete_approve",
            "task_id": "terminal-task",
        })
    );
    assert_eq!(
        seen.lock().expect("dispatched requests").len(),
        3,
        "terminal approval replay must not dispatch a new task completion"
    );
    let replayed = replay_seen.lock().expect("terminal replay requests");
    assert_eq!(replayed.len(), 1);
    let Op::Call(call) = &replayed[0].op else {
        panic!("terminal approval replay must be a task completion call");
    };
    assert_eq!(call.input["id"], "terminal-task");
    assert_eq!(
        call.input["log"],
        "Recorded the exact terminal browser outcome."
    );
    assert_eq!(
        call.input[planning::PLANNING_CONTEXT_FIELD]["expected_daemon_instance_id"],
        "retired-daemon-instance"
    );
    assert_eq!(
        replayed[0]
            .workflow_confirmation
            .as_ref()
            .expect("terminal approval confirmation")
            .outcome,
        "Recorded the exact terminal browser outcome."
    );
    drop(replayed);

    let canonical_approval = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
                "id": "browser-canonical-approval",
                "session_key": session_key,
                "expected_daemon_instance_id": "retired-daemon-instance",
                "expected_revision": 3,
                "expected_phase_id": "retired-phase",
                "operation": {
                    "kind": "task_complete_approve",
                    "review_id": "evicted-canonical-review",
                    "task_id": "canonical-task",
                    "outcome": "Recorded the canonical browser outcome.",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("replay canonical browser approval");
    assert_eq!(canonical_approval.status, 200, "{canonical_approval:?}");
    assert_eq!(
        canonical_approval.json()["result"],
        json!({
            "kind": "workbench.task_mutation",
            "ok": true,
            "schema_version": 1,
            "operation": "task_complete_approve",
            "task_id": "canonical-task",
        })
    );
    let dispatched = seen.lock().expect("dispatched requests");
    assert_eq!(dispatched.len(), 4);
    assert_eq!(dispatched[3].id, "browser-canonical-approval");
    drop(dispatched);
    let probed = preparation_probe_seen
        .lock()
        .expect("atomic preparation probes");
    assert_eq!(probed.len(), 1);
    assert_eq!(probed[0].id, "browser-canonical-approval");
    drop(probed);

    let session_id = cookie_pair
        .strip_prefix(&format!("{SESSION_COOKIE_PREFIX}{session_key}="))
        .expect("session cookie name");
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .get_mut(&session_credential_digest(session_id))
        .expect("HTTP session")
        .capabilities
        .retain(|capability| capability != "lane.focus");
    let denied = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": PROTOCOL_VERSION,
                "id": "browser-denied-focus",
                "session_key": session_key,
                "operation": {
                    "kind": "lane_focus",
                    "lane_id": "lane-denied",
                },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("deny unavailable browser capability");
    assert_eq!(denied.status, 403, "{denied:?}");
    assert_eq!(denied.json()["kind"], "workbench.capability_denied");
    assert_eq!(
        denied.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );

    let oversized = format!(
        "{{\"ticket\":\"{}\"}}",
        "x".repeat(MAX_REQUEST_BODY_BYTES + 1)
    );
    let oversized = raw_http(
        origin,
        "POST",
        "/api/session",
        Some(oversized),
        None,
        Some(origin),
    )
    .await
    .expect("reject oversized body");
    assert_eq!(oversized.status, 413, "{oversized:?}");
    assert_eq!(oversized.json()["kind"], "workbench.busy");

    let invalid = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": PROTOCOL_VERSION,
                "id": "browser-arbitrary",
                "session_key": session_key,
                "operation": { "kind": "task_complete", "task_id": "nope" },
            })
            .to_string(),
        ),
        Some(cookie_pair),
        Some(origin),
    )
    .await
    .expect("reject arbitrary browser command");
    assert_eq!(invalid.status, 400);
    assert_eq!(invalid.json()["kind"], "workbench.invalid_request");

    manager.shutdown().await;
}

#[cfg(feature = "ui")]
#[tokio::test(flavor = "multi_thread")]
async fn malformed_host_record_is_not_resumed() {
    let fixture = fixture();
    let first =
        test_manager_with_identity(Arc::clone(&fixture.project), "incompatible-first-instance");
    let launch = first.launch(&fixture.root).expect("launch first workbench");
    let (_, ticket) = launch_parts(&launch);
    first
        .inner
        .redeem_ticket(ticket)
        .expect("persist one resumable session");
    first.shutdown().await;

    let mut malformed: WorkbenchHostRecord = serde_json::from_slice(
        &fs::read(&first.inner.host_record_path).expect("read retained host record"),
    )
    .expect("decode retained host record");
    malformed.origin = "https://127.0.0.1:1".to_string();
    write_host_record(&first.inner.host_record_path, &malformed)
        .expect("write malformed host record");

    let replacement = test_manager_with_identity(
        Arc::clone(&fixture.project),
        "malformed-replacement-instance",
    );
    assert!(
        replacement.host_status().is_none(),
        "a live grant must not resume an invalid loopback origin"
    );
}

#[cfg(feature = "ui")]
async fn raw_http(
    origin: &str,
    method: &str,
    path: &str,
    body: Option<String>,
    cookie: Option<&str>,
    request_origin: Option<&str>,
) -> io::Result<RawHttpResponse> {
    let authority = origin
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::other("test origin is not HTTP"))?;
    raw_http_via(
        origin,
        authority,
        method,
        path,
        body,
        cookie,
        request_origin,
    )
    .await
}

#[cfg(feature = "ui")]
async fn raw_http_via(
    connect_origin: &str,
    request_host: &str,
    method: &str,
    path: &str,
    body: Option<String>,
    cookie: Option<&str>,
    request_origin: Option<&str>,
) -> io::Result<RawHttpResponse> {
    let authority = connect_origin
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::other("test connection origin is not HTTP"))?;
    let mut stream = tokio::net::TcpStream::connect(authority).await?;
    let body = body.unwrap_or_default();
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {request_host}\r\nConnection: close\r\n");
    if let Some(origin) = request_origin {
        request.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(cookie) = cookie {
        request.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if !body.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(&body);
    stream.write_all(request.as_bytes()).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTP response timed out"))??;
    parse_http_response(&bytes)
}

#[cfg(feature = "ui")]
fn parse_http_response(bytes: &[u8]) -> io::Result<RawHttpResponse> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("HTTP response has no header terminator"))?;
    let head = std::str::from_utf8(&bytes[..split])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| io::Error::other("HTTP response has no status"))?;
    let header_lines = lines.collect::<Vec<_>>();
    let set_cookies = header_lines
        .iter()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, value)| value.trim().to_string())
        .collect::<Vec<_>>();
    let headers = header_lines
        .into_iter()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
        .collect::<HashMap<_, _>>();
    let mut body = bytes[split + 4..].to_vec();
    if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        body = decode_chunked(&body)?;
    }
    Ok(RawHttpResponse {
        status,
        headers,
        set_cookies,
        body,
    })
}

#[cfg(feature = "ui")]
fn response_cookie<'a>(response: &'a RawHttpResponse, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    response
        .set_cookies
        .iter()
        .find_map(|cookie| cookie.split(';').next()?.strip_prefix(&prefix))
}

#[cfg(feature = "ui")]
fn response_cookie_prefix<'a>(response: &'a RawHttpResponse, prefix: &str) -> Option<&'a str> {
    response.set_cookies.iter().find_map(|cookie| {
        let value = cookie.split(';').next()?;
        value.strip_prefix(prefix).map(|_| value)
    })
}

#[cfg(feature = "ui")]
fn decode_chunked(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut remaining = bytes;
    let mut decoded = Vec::new();
    loop {
        let line_end = remaining
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| io::Error::other("chunk has no size terminator"))?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&remaining[..line_end])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            16,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        remaining = &remaining[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        if remaining.len() < size + 2 {
            return Err(io::Error::other("chunk body is truncated"));
        }
        decoded.extend_from_slice(&remaining[..size]);
        remaining = &remaining[size + 2..];
    }
}
