//! Integration tests for the Phase 2 event pipeline:
//! command capture, retention cleanup, and event-based boundary detection.

#![allow(missing_docs)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

#[macro_use]
mod test_support;

use test_support::exo_init_with_storage;

/// After running a command through `handle_request` (the handler path),
/// verify that a `command` event was logged into `agent_events`.
///
/// Note: `--direct` CLI mode bypasses `handle_request` and does NOT log
/// events. This test uses the protocol handler path which is what the
/// daemon uses in production.
#[test]
fn command_capture_e2e() {
    use exo::api::handler::handle_request;
    use exo::api::protocol::{Address, CallParams, Op, PROTOCOL_VERSION, RequestEnvelope, Status};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");

    // Dispatch through handle_request (the daemon handler path that logs events)
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "test-1".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["goal".to_string(), "add".to_string()],
            },
            input: serde_json::json!({
                "label": "Test Goal",
                "id": "test-goal"
            }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: Some("test-agent".to_string()),
    };
    let response = handle_request(root, request);
    assert_eq!(response.status, Status::Ok);

    // Open the DB and check for command events
    let db_path = root.join(".cache/exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open db");
    let count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE event_type = 'command'",
            [],
            |row| row.get(0),
        )
        .expect("query agent_events");

    assert!(
        count > 0,
        "expected at least one command event, got {count}"
    );

    // Verify the goal add event specifically
    let goal_event_count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE namespace = 'goal' AND operation = 'add'",
            [],
            |row| row.get(0),
        )
        .expect("query goal event");
    assert_eq!(
        goal_event_count, 1,
        "expected exactly one goal.add event, got {goal_event_count}"
    );

    // Verify agent_id was captured
    let agent_id: Option<String> = db
        .connection()
        .query_row(
            "SELECT agent_id FROM agent_events WHERE namespace = 'goal' AND operation = 'add'",
            [],
            |row| row.get(0),
        )
        .expect("query agent_id");
    assert_eq!(agent_id.as_deref(), Some("test-agent"));
}

/// Retention cleanup deletes events older than 7 days but keeps recent ones.
#[test]
fn retention_cleanup_removes_old_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();

    exo_init_with_storage(root, "sqlite");

    let db_path = root.join(".cache/exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open db");
    let conn = db.connection();

    // Insert an old event (10 days ago)
    conn.execute(
        "INSERT INTO agent_events (text_id, timestamp, event_type, namespace, operation, summary)
         VALUES ('old-event', datetime('now', '-10 days'), 'command', 'test', 'old', 'old event')",
        [],
    )
    .expect("insert old event");

    // Insert a recent event (1 hour ago)
    conn.execute(
        "INSERT INTO agent_events (text_id, timestamp, event_type, namespace, operation, summary)
         VALUES ('recent-event', datetime('now', '-1 hours'), 'command', 'test', 'recent', 'recent event')",
        [],
    )
    .expect("insert recent event");

    // Verify both exist
    let count_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE text_id IN ('old-event', 'recent-event')",
            [],
            |row| row.get(0),
        )
        .expect("count before");
    assert_eq!(count_before, 2, "expected 2 test events before cleanup");

    // Run cleanup
    drop(db);
    exo::daemon::cleanup_old_events(root);

    // Reopen and verify
    let db = exosuit_storage::open_database(&db_path).expect("reopen db");
    let conn = db.connection();

    let old_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE text_id = 'old-event'",
            [],
            |row| row.get(0),
        )
        .expect("count old");
    assert_eq!(old_count, 0, "old event should have been deleted");

    let recent_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_events WHERE text_id = 'recent-event'",
            [],
            |row| row.get(0),
        )
        .expect("count recent");
    assert_eq!(recent_count, 1, "recent event should survive cleanup");
}
