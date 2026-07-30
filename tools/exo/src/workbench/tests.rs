#![allow(clippy::disallowed_methods)]

use super::*;
use crate::api::protocol::{Effect, Op, PROTOCOL_VERSION, ResponseEnvelope, Status};
use crate::context::SqliteWriter;
use serde_json::{Value as JsonValue, json};
#[cfg(feature = "ui")]
use std::collections::HashMap;
use std::fs;
#[cfg(feature = "ui")]
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
#[cfg(feature = "ui")]
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
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
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn test_manager(project: Arc<Project>) -> WorkbenchHostManager {
    test_manager_with_identity(project, "test-workbench-instance")
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
    let payload = ticket.split('.').nth(1).expect("ticket payload");
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .expect("decode ticket payload");
    serde_json::from_slice(&bytes).expect("parse ticket payload")
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
        focused_here: true,
    };
    let snapshot = WorkbenchSnapshot {
        kind: "workbench.snapshot",
        ok: true,
        schema_version: 1,
        observed_at: "2026-07-28T20:00:00.000Z".to_string(),
        revision: 7,
        project: WorkbenchProjectIdentity {
            id: "project-fixture".to_string(),
        },
        workspace: WorkbenchSnapshotWorkspace {
            key: "workspace-fixture".to_string(),
            label: "main".to_string(),
            branch: Some("main".to_string()),
            head: Some("0123456789abcdef".to_string()),
            detached: false,
            dirty: true,
        },
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
            goals: vec![WorkbenchGoal {
                id: "host-goal".to_string(),
                title: "Establish local host and launch".to_string(),
                status: "in-progress".to_string(),
                tasks: vec![WorkbenchTask {
                    id: "implement-host".to_string(),
                    title: "Implement host".to_string(),
                    status: "in-progress".to_string(),
                }],
            }],
        }),
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
        "../../../../packages/exosuit-cockpit/src/lib/workbench-snapshot.v1.json"
    ))
    .expect("parse cockpit snapshot fixture");

    assert_eq!(
        serde_json::to_value(snapshot).expect("serialize Rust snapshot"),
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
    assert_eq!(first.expires_in_seconds, 300);
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
    let session_store = fs::read_to_string(&manager.inner.session_store_path)
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
    }
    first_manager
        .inner
        .persist_session_store()
        .expect("persist short session");
    let store_path = first_manager.inner.session_store_path.clone();
    let persisted = fs::read_to_string(&store_path).expect("read durable session");
    assert!(!persisted.contains(&session_secret));
    assert!(persisted.contains(&digest));
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
    replacement.shutdown().await;
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
        fs::read_to_string(&first_manager.inner.session_store_path)
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
    assert!(snapshot.diagnostics.is_empty());
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
        .prepare_completion_approval(&session, "approval-request", 0, &phase, &first.review_id)
        .expect("prepare exact approval");
    assert_eq!(approval.task_id, "review-task");
    assert_eq!(
        approval.proposed_outcome,
        "Implemented the bounded planning contract."
    );

    manager.revision_after_write();
    let stale = manager
        .inner
        .completion_review(
            &session,
            "stale-review",
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
}

#[test]
fn planning_requests_reject_unknown_fields_and_invalid_text_bounds() {
    let request = json!({
        "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
        "id": "planning-request",
        "session_key": "planning-session",
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
    let manager = WorkbenchHostManager::new(
        Arc::clone(&fixture.project),
        Arc::from("test-http-instance"),
        Arc::from("test-http-start"),
        fixture.project.runtime_dir(),
        Arc::new(AtomicU64::new(unix_seconds())),
        tokio::runtime::Handle::current(),
    );
    let dispatch_seen = Arc::clone(&seen);
    manager
        .set_dispatcher(DaemonRequestDispatcher::new(move |request| {
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
        }))
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

    let planning = raw_http(
        origin,
        "POST",
        "/api/command",
        Some(
            json!({
                "protocol_version": planning::PLANNING_PROTOCOL_VERSION,
                "id": "browser-task-add",
                "session_key": session_key,
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
        assert_eq!(dispatched.len(), 2);
        let Op::Call(call) = &dispatched[1].op else {
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
    let mut stream = tokio::net::TcpStream::connect(authority).await?;
    let body = body.unwrap_or_default();
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
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
    let headers = lines
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
        body,
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
