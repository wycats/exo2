//! Integration coverage for task handle repair and alias compatibility.

#[macro_use]
mod test_support;

use exo::context::{SqliteWriter, db_path};
use serde_json::Value;
use test_case::test_matrix;
use test_support::{exo_cmd_with_storage, exo_init_with_storage, exo_phase_start_with_storage};

fn setup_task(root: &std::path::Path, backend: &str) {
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Nested task",
            "--id",
            "parent::child",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();
}

fn run_json(root: &std::path::Path, backend: &str, args: &[&str]) -> Value {
    let output = exo_cmd_with_storage(root, backend)
        .arg("--format")
        .arg("json")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid JSON output")
}

#[test_matrix(["sqlite"])]
fn task_rename_preserves_identity_history_and_aliases(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    setup_task(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["task", "start", "goal-1::parent::child"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "log",
            "goal-1::parent::child",
            "--message",
            "Before rename",
        ])
        .assert()
        .success();

    let db_path = db_path(root, None);
    let writer = SqliteWriter::open(&db_path).expect("open writer");
    writer
        .database()
        .connection()
        .execute(
            "UPDATE tasks SET notes = 'Keep these notes' WHERE text_id = 'parent::child'",
            [],
        )
        .expect("set task notes");
    writer
        .add_task_verification(
            "goal-1::parent::child",
            "test",
            Some("cargo test"),
            "passed",
            None,
        )
        .expect("add verification");
    writer
        .add_inbox_item(
            "task",
            Some("parent::child"),
            "system-observation",
            "fyi",
            "when-relevant",
            None,
            None,
            "Task reference",
            "This reference should follow the canonical handle.",
            None,
        )
        .expect("add task reference");
    let before = writer
        .resolve_task_reference("goal-1::parent::child")
        .expect("resolve task")
        .expect("task exists");
    let before_shape: (i64, Option<String>, Option<String>) = writer
        .database()
        .connection()
        .query_row(
            "SELECT goal_id, sort_key, notes FROM tasks_data WHERE id = ?1",
            [before.row_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read task shape");

    let renamed = run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "goal-1::renamed::child",
        ],
    );
    assert_eq!(renamed["result"]["old_task_id"], "parent::child");
    assert_eq!(renamed["result"]["task_id"], "renamed::child");
    assert_eq!(renamed["result"]["goal_id"], "goal-1");
    assert_eq!(renamed["result"]["title"], "Nested task");

    let writer = SqliteWriter::open(&db_path).expect("reopen writer");
    let current = writer
        .resolve_task_reference("goal-1::renamed::child")
        .expect("resolve renamed task")
        .expect("renamed task exists");
    let old_alias = writer
        .resolve_task_reference("goal-1::parent::child")
        .expect("resolve old alias")
        .expect("old alias exists");
    assert_eq!(current.row_id, before.row_id);
    assert_eq!(old_alias.row_id, before.row_id);
    assert_eq!(current.phase_status, "in-progress");
    let after_shape: (i64, Option<String>, Option<String>) = writer
        .database()
        .connection()
        .query_row(
            "SELECT goal_id, sort_key, notes FROM tasks_data WHERE id = ?1",
            [before.row_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read renamed task shape");
    assert_eq!(after_shape, before_shape);
    let inbox_reference: String = writer
        .database()
        .connection()
        .query_row(
            "SELECT entity_id FROM inbox_data WHERE subject = 'Task reference'",
            [],
            |row| row.get(0),
        )
        .expect("read task inbox reference");
    assert_eq!(inbox_reference, "renamed::child");

    writer
        .database()
        .connection()
        .execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
             SELECT 'goal', id, 'goal-alias' FROM goals_data WHERE text_id = 'goal-1'",
            [],
        )
        .expect("add goal alias");
    for (entity_id, subject) in [
        ("parent::child", "Older task alias"),
        ("goal-1::parent::child", "Qualified older task alias"),
        (
            "goal-alias::parent::child",
            "Goal-alias qualified task alias",
        ),
    ] {
        writer
            .add_inbox_item(
                "task",
                Some(entity_id),
                "system-observation",
                "fyi",
                "when-relevant",
                None,
                None,
                subject,
                "This retained alias should migrate to the next canonical handle.",
                None,
            )
            .expect("add retained-alias inbox reference");
        writer
            .database()
            .connection()
            .execute(
                "INSERT INTO agent_events
                 (text_id, timestamp, event_type, entity_type, entity_id, summary)
                 VALUES (?1, '2026-06-25T00:00:00Z', 'command', 'task', ?2, ?3)",
                (format!("event-{subject}"), entity_id, subject),
            )
            .expect("add retained-alias event reference");
    }

    let alias_log = run_json(
        root,
        backend,
        &[
            "task",
            "log",
            "goal-1::parent::child",
            "--message",
            "Through old alias",
        ],
    );
    assert_eq!(alias_log["result"]["task_id"], "renamed::child");

    let renamed_again = run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "final-child",
        ],
    );
    assert_eq!(renamed_again["result"]["old_task_id"], "renamed::child");
    assert_eq!(renamed_again["result"]["task_id"], "final-child");

    let writer = SqliteWriter::open(&db_path).expect("reopen writer");
    for subject in [
        "Older task alias",
        "Qualified older task alias",
        "Goal-alias qualified task alias",
    ] {
        let inbox_entity_id: String = writer
            .database()
            .connection()
            .query_row(
                "SELECT entity_id FROM inbox_data WHERE subject = ?1",
                [subject],
                |row| row.get(0),
            )
            .expect("read migrated inbox reference");
        let event_entity_id: String = writer
            .database()
            .connection()
            .query_row(
                "SELECT entity_id FROM agent_events WHERE summary = ?1",
                [subject],
                |row| row.get(0),
            )
            .expect("read migrated event reference");
        assert_eq!(inbox_entity_id, "final-child", "{subject}");
        assert_eq!(event_entity_id, "final-child", "{subject}");
    }
    for reference in [
        "goal-1::final-child",
        "goal-1::renamed::child",
        "goal-1::parent::child",
    ] {
        let resolved = writer
            .resolve_task_reference(reference)
            .expect("resolve task reference")
            .expect("task reference exists");
        assert_eq!(resolved.row_id, before.row_id, "{reference}");
        assert_eq!(resolved.task_id, "final-child", "{reference}");
    }

    let conn = writer.database().connection();
    let status: String = conn
        .query_row(
            "SELECT status FROM tasks_data WHERE id = ?1",
            [before.row_id],
            |row| row.get(0),
        )
        .expect("read status");
    let logs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_logs WHERE task_id = ?1",
            [before.row_id],
            |row| row.get(0),
        )
        .expect("count logs");
    let verifications: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_verifications WHERE task_id = ?1",
            [before.row_id],
            |row| row.get(0),
        )
        .expect("count verifications");
    assert_eq!(status, "in-progress");
    assert_eq!(logs, 2);
    assert_eq!(verifications, 1);

    let renamed_back = run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::final-child",
            "--to",
            "parent::child",
        ],
    );
    assert_eq!(renamed_back["result"]["old_task_id"], "final-child");
    assert_eq!(renamed_back["result"]["task_id"], "parent::child");

    let writer = SqliteWriter::open(&db_path).expect("reopen writer");
    for reference in ["goal-1::parent::child", "goal-1::final-child"] {
        let resolved = writer
            .resolve_task_reference(reference)
            .expect("resolve task reference")
            .expect("task reference exists");
        assert_eq!(resolved.row_id, before.row_id, "{reference}");
        assert_eq!(resolved.task_id, "parent::child", "{reference}");
    }
}

#[test_matrix(["sqlite"])]
fn task_lifecycle_commands_accept_retained_aliases(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    setup_task(root, backend);
    run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "current-task",
        ],
    );

    for (args, expected_title) in [
        (
            vec!["task", "start", "goal-1::parent::child"],
            "Nested task",
        ),
        (
            vec![
                "task",
                "update",
                "goal-1::parent::child",
                "--title",
                "Updated task title",
            ],
            "Updated task title",
        ),
        (
            vec!["task", "reorder", "goal-1::parent::child", "top"],
            "Updated task title",
        ),
    ] {
        let output = run_json(root, backend, &args);
        assert_eq!(output["result"]["task_id"], "current-task", "{args:?}");
        if output["result"].get("title").is_some() {
            assert_eq!(output["result"]["title"], expected_title, "{args:?}");
        }
    }

    let writer = SqliteWriter::open(db_path(root, None)).expect("open writer");
    writer
        .add_inbox_item(
            "task",
            Some("current-task"),
            "user-feedback",
            "claim",
            "immediate",
            Some("high"),
            None,
            "Outcome approved",
            "The task outcome is ready to record.",
            None,
        )
        .expect("add completion claim");

    let completed = run_json(
        root,
        backend,
        &[
            "task",
            "complete",
            "goal-1::parent::child",
            "--log",
            "Completed through retained alias",
        ],
    );
    assert_eq!(completed["result"]["task_id"], "current-task");

    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Removable task",
            "--id",
            "remove-old",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();
    run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::remove-old",
            "--to",
            "remove-current",
        ],
    );
    let removed = run_json(root, backend, &["task", "remove", "goal-1::remove-old"]);
    assert_eq!(removed["result"]["task_id"], "remove-current");
}

#[test_matrix(["sqlite"])]
fn task_rename_rejects_canonical_and_alias_collisions(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    setup_task(root, backend);
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Other task",
            "--id",
            "other-task",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();

    let collision = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "other-task",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let collision: Value = serde_json::from_slice(&collision).expect("valid collision JSON");
    assert_eq!(collision["error"]["code"], "invalid_input");

    run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "renamed-task",
        ],
    );

    let alias_collision = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Alias collision",
            "--id",
            "parent::child",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let alias_collision: Value =
        serde_json::from_slice(&alias_collision).expect("valid alias collision JSON");
    assert_eq!(alias_collision["error"]["code"], "invalid_input");

    let writer = SqliteWriter::open(db_path(root, None)).expect("open writer");
    assert!(
        writer
            .resolve_task_reference("goal-1::renamed-task")
            .expect("resolve renamed task")
            .is_some()
    );
    assert!(
        writer
            .resolve_task_reference("goal-1::other-task")
            .expect("resolve other task")
            .is_some()
    );
}

#[test_matrix(["sqlite"])]
fn task_rename_normalizes_goal_aliases_and_rejects_mismatched_prefixes(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    setup_task(root, backend);
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Other goal", "--id", "other-goal"])
        .assert()
        .success();

    let writer = SqliteWriter::open(db_path(root, None)).expect("open writer");
    writer
        .database()
        .connection()
        .execute(
            "INSERT INTO entity_aliases(entity_type, entity_id, alias)
             SELECT 'goal', id, 'goal-alias' FROM goals_data WHERE text_id = 'goal-1'",
            [],
        )
        .expect("add goal alias");

    let mismatched = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "other-goal::wrong-task",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let mismatched: Value = serde_json::from_slice(&mismatched).expect("valid mismatch JSON");
    assert_eq!(mismatched["error"]["code"], "invalid_input");

    let renamed = run_json(
        root,
        backend,
        &[
            "task",
            "rename",
            "goal-1::parent::child",
            "--to",
            "goal-alias::normalized-task",
        ],
    );
    assert_eq!(renamed["result"]["task_id"], "normalized-task");

    let empty_prefix = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "rename",
            "goal-1::normalized-task",
            "--to",
            "::invalid",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let empty_prefix: Value = serde_json::from_slice(&empty_prefix).expect("valid error JSON");
    assert_eq!(empty_prefix["error"]["code"], "invalid_input");

    let current = writer
        .resolve_task_reference("goal-1::normalized-task")
        .expect("resolve task")
        .expect("task exists");
    assert_eq!(current.task_id, "normalized-task");
}

#[test_matrix(["sqlite"])]
fn qualified_task_references_win_over_ambiguous_legacy_canonical_handles(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 2", "--id", "goal-2"])
        .assert()
        .success();

    let writer = SqliteWriter::open(db_path(root, None)).expect("open writer");
    let conn = writer.database().connection();
    conn.execute(
        "INSERT INTO tasks (text_id, title, status, goal_id)
         SELECT 'goal-1::foo', 'Legacy ambiguous task', 'pending', id
         FROM goals_data WHERE text_id = 'goal-2'",
        [],
    )
    .expect("insert legacy ambiguous canonical task");

    let collision = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Qualified task",
            "--id",
            "foo",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let collision: Value = serde_json::from_slice(&collision).expect("valid collision JSON");
    assert_eq!(collision["error"]["code"], "invalid_input");

    conn.execute(
        "INSERT INTO tasks (text_id, title, status, goal_id)
         SELECT 'foo', 'Qualified task', 'pending', id
         FROM goals_data WHERE text_id = 'goal-1'",
        [],
    )
    .expect("insert qualified task for legacy ambiguity fixture");

    let legacy_started = run_json(root, backend, &["task", "start", "goal-2::goal-1::foo"]);
    assert_eq!(legacy_started["result"]["task_id"], "goal-1::foo");

    let qualified_status: String = conn
        .query_row(
            "SELECT status FROM tasks_data WHERE text_id = 'foo'",
            [],
            |row| row.get(0),
        )
        .expect("read qualified task status");
    let legacy_status: String = conn
        .query_row(
            "SELECT status FROM tasks_data WHERE text_id = 'goal-1::foo'",
            [],
            |row| row.get(0),
        )
        .expect("read legacy canonical task status");
    assert_eq!(qualified_status, "pending");
    assert_eq!(legacy_status, "in-progress");

    let started = run_json(root, backend, &["task", "start", "goal-1::foo"]);
    assert_eq!(started["result"]["task_id"], "foo");

    let qualified_status: String = conn
        .query_row(
            "SELECT status FROM tasks_data WHERE text_id = 'foo'",
            [],
            |row| row.get(0),
        )
        .expect("read qualified task status");
    let ambiguous_status: String = conn
        .query_row(
            "SELECT status FROM tasks_data WHERE text_id = 'goal-1::foo'",
            [],
            |row| row.get(0),
        )
        .expect("read ambiguous canonical task status");
    assert_eq!(qualified_status, "in-progress");
    assert_eq!(ambiguous_status, "in-progress");

    writer
        .add_inbox_item(
            "task",
            Some("goal-1::foo"),
            "system-observation",
            "fyi",
            "when-relevant",
            None,
            None,
            "Legacy colliding reference",
            "This reference must not be reassigned during rename.",
            None,
        )
        .expect("add legacy colliding inbox reference");
    conn.execute(
        "INSERT INTO agent_events
         (text_id, timestamp, event_type, entity_type, entity_id, summary)
         VALUES ('legacy-colliding-event', '2026-06-25T00:00:00Z', 'command',
                 'task', 'goal-1::foo', 'Legacy colliding reference')",
        [],
    )
    .expect("add legacy colliding event reference");

    let renamed = run_json(
        root,
        backend,
        &["task", "rename", "goal-1::foo", "--to", "renamed-foo"],
    );
    assert_eq!(renamed["result"]["task_id"], "renamed-foo");
    for (table, label_column) in [("inbox_data", "subject"), ("agent_events", "summary")] {
        let entity_id: String = conn
            .query_row(
                &format!(
                    "SELECT entity_id FROM {table} WHERE {label_column} = 'Legacy colliding reference'"
                ),
                [],
                |row| row.get(0),
            )
            .expect("read preserved colliding reference");
        assert_eq!(entity_id, "goal-1::foo", "{table}");
    }
}

#[test_matrix(["sqlite"])]
fn goal_alias_qualified_collisions_reject_add_and_rename(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 1", "--id", "goal-1"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Goal 2", "--id", "goal-2"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Source task",
            "--id",
            "source-task",
            "--goal",
            "goal-1",
        ])
        .assert()
        .success();

    let writer = SqliteWriter::open(db_path(root, None)).expect("open writer");
    let conn = writer.database().connection();
    conn.execute(
        "INSERT INTO entity_aliases(entity_type, entity_id, alias)
         SELECT 'goal', id, 'goal-alias' FROM goals_data WHERE text_id = 'goal-1'",
        [],
    )
    .expect("add goal alias");
    for task_id in ["goal-alias::added-task", "goal-alias::renamed-task"] {
        conn.execute(
            "INSERT INTO tasks (text_id, title, status, goal_id)
             SELECT ?1, 'Conflicting task', 'pending', id
             FROM goals_data WHERE text_id = 'goal-2'",
            [task_id],
        )
        .expect("insert goal-alias collision fixture");
    }

    let add_collision = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "add",
            "Added task",
            "--id",
            "added-task",
            "--goal",
            "goal-1",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let add_collision: Value =
        serde_json::from_slice(&add_collision).expect("valid add collision JSON");
    assert_eq!(add_collision["error"]["code"], "invalid_input");

    let rename_collision = exo_cmd_with_storage(root, backend)
        .args([
            "--format",
            "json",
            "task",
            "rename",
            "goal-1::source-task",
            "--to",
            "renamed-task",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let rename_collision: Value =
        serde_json::from_slice(&rename_collision).expect("valid rename collision JSON");
    assert_eq!(rename_collision["error"]["code"], "invalid_input");
}
