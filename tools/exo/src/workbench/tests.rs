#![allow(clippy::disallowed_methods)]

use super::*;
use crate::api::protocol::{Effect, Op, PROTOCOL_VERSION, ResponseEnvelope, Status};
use crate::context::SqliteWriter;
use serde_json::{Value as JsonValue, json};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    project: Arc<Project>,
}

#[derive(Debug)]
struct RawHttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

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
    let manager = WorkbenchHostManager::new(
        Arc::clone(&project),
        Arc::from("test-workbench-instance"),
        Arc::from("test-process-start"),
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

fn launch_parts(launch: &WorkbenchLaunchResult) -> (&str, &str) {
    launch
        .url
        .split_once("/#ticket=")
        .expect("launch URL contains ticket fragment")
}

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

    let (session_id, session) = manager
        .inner
        .redeem_ticket(ticket)
        .expect("redeem launch ticket");
    assert_eq!(session.project_id, fixture.project.id.as_str());
    assert_eq!(session.workspace_key, first.workspace.key);
    assert_eq!(
        manager.inner.redeem_ticket(ticket),
        Err(TicketExchangeError::Invalid)
    );

    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .get_mut(&session_id)
        .expect("redeemed session")
        .instance_id = "replacement-daemon".to_string();
    assert!(
        manager.inner.session(&session_id).is_none(),
        "a session from another daemon generation is invalid"
    );

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
    assert!(
        !manager.inner.host_record_path.exists(),
        "owned host record is removed on shutdown"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_tickets_and_session_bounds_fail_closed_without_consuming_live_tickets() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    let launch = manager.launch(&fixture.root).expect("launch workbench");
    let (_, ticket) = launch_parts(&launch);
    let payload = ticket_payload(ticket);
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
            state.sessions.insert(
                session_id.clone(),
                WorkbenchSession {
                    id: session_id,
                    instance_id: manager.inner.instance_id.to_string(),
                    project_id: fixture.project.id.to_string(),
                    workspace_key: payload.workspace_key.clone(),
                    workspace_root: workspace_root.clone(),
                    capabilities: payload.capabilities.clone(),
                    created_at: now,
                    last_activity: now,
                    expires_at: now + SESSION_ABSOLUTE_LIFETIME.as_secs(),
                },
            );
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
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .remove("bounded-session-0");
    let (session_id, _) = manager
        .inner
        .redeem_ticket(ticket)
        .expect("ticket remains redeemable after capacity returns");

    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let session = state.sessions.get_mut(&session_id).expect("new session");
        session.created_at = now.saturating_sub(SESSION_ABSOLUTE_LIFETIME.as_secs() + 1);
        session.expires_at = now.saturating_add(SESSION_ABSOLUTE_LIFETIME.as_secs());
        session.last_activity = now;
        drop(state);
    }
    assert!(
        manager.inner.session(&session_id).is_none(),
        "sessions expire at the absolute lifetime even when recently active"
    );

    let idle_launch = manager.launch(&fixture.root).expect("launch idle session");
    let (_, idle_ticket) = launch_parts(&idle_launch);
    let (idle_session_id, _) = manager
        .inner
        .redeem_ticket(idle_ticket)
        .expect("redeem idle session ticket");
    {
        let mut state = manager.inner.state.lock().expect("workbench state");
        let session = state
            .sessions
            .get_mut(&idle_session_id)
            .expect("idle session");
        session.last_activity = now.saturating_sub(SESSION_IDLE_LIFETIME.as_secs() + 1);
        drop(state);
    }
    assert!(
        manager.inner.session(&idle_session_id).is_none(),
        "idle sessions expire"
    );
    manager.shutdown().await;
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

    let session_id = cookie_pair
        .strip_prefix(&format!("{SESSION_COOKIE_NAME}="))
        .expect("session cookie name");
    manager
        .inner
        .state
        .lock()
        .expect("workbench state")
        .sessions
        .get_mut(session_id)
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

#[tokio::test(flavor = "multi_thread")]
async fn matching_host_record_is_removed_but_foreign_generation_is_preserved() {
    let fixture = fixture();
    let manager = test_manager(Arc::clone(&fixture.project));
    manager.launch(&fixture.root).expect("launch workbench");
    manager.shutdown().await;

    let foreign = WorkbenchHostRecord {
        schema_version: 1,
        instance_id: "foreign-instance".to_string(),
        pid: std::process::id(),
        process_start_id: "foreign-start".to_string(),
        origin: "http://127.0.0.1:1".to_string(),
        assets_hash: "blake3:foreign".to_string(),
        server_task_alive: false,
        started_at: timestamp_now(),
        updated_at: timestamp_now(),
        last_error: None,
    };
    write_host_record(&manager.inner.host_record_path, &foreign).expect("write foreign record");
    manager.inner.remove_owned_host_record();
    assert!(
        manager.inner.host_record_path.exists(),
        "cleanup must preserve a foreign runtime generation"
    );
}

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
