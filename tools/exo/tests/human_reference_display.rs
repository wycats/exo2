//! Human display uses titles and conversational handles while JSON retains IDs.

#[macro_use]
mod test_support;

use exo::context::{SqliteWriter, db_path};
use predicates::prelude::*;
use serde_json::Value;
use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_phase_start_with_storage, exo_plan_add_epoch_with_storage, exo_plan_add_phase_with_storage,
    exo_plan_update_status_with_storage,
};

fn json_output(root: &std::path::Path, backend: &str, args: &[&str]) -> Value {
    let output = exo_cmd_with_storage(root, backend)
        .args(["--format", "json"])
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid JSON output")
}

#[test_matrix(["sqlite"])]
fn phase_and_status_human_output_uses_titles_and_runnable_actions(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    let status = json_output(root, backend, &["status"]);
    let phase_id = status["result"]["phase_id"].as_str().expect("phase id");
    let phase_title = status["result"]["phase_title"]
        .as_str()
        .expect("phase title");
    let phase_list = json_output(root, backend, &["phase", "list"]);
    let epoch_id = phase_list["result"]["epoch_id"].as_str().expect("epoch id");
    let epoch_title = phase_list["result"]["epoch_title"]
        .as_str()
        .expect("epoch title");

    exo_cmd_with_storage(root, backend)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains(phase_title))
        .stdout(predicate::str::contains(phase_id).not());

    exo_cmd_with_storage(root, backend)
        .args(["epoch", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(epoch_title))
        .stdout(predicate::str::contains(format!(
            "exo epoch start {epoch_id}"
        )));

    exo_cmd_with_storage(root, backend)
        .args(["phase", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(phase_title))
        .stdout(predicate::str::contains(format!(
            "exo phase start {phase_id}"
        )));

    assert_eq!(status["result"]["phase_id"], phase_id);
}

#[test_matrix(["sqlite"])]
fn task_human_output_pairs_handle_with_title(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);

    exo_cmd_with_storage(root, backend)
        .args(["goal", "add", "Addressing", "--id", "addressing"])
        .assert()
        .success();
    exo_cmd_with_storage(root, backend)
        .args([
            "task",
            "add",
            "Repair task handles",
            "--id",
            "repair-handles",
            "--goal",
            "addressing",
        ])
        .assert()
        .success();

    exo_cmd_with_storage(root, backend)
        .args(["task", "start", "addressing::repair-handles"])
        .assert()
        .success()
        .stdout(predicate::str::contains("repair-handles"))
        .stdout(predicate::str::contains("Repair task handles"));

    let result = json_output(
        root,
        backend,
        &["task", "start", "addressing::repair-handles"],
    );
    assert_eq!(result["result"]["task_id"], "repair-handles");
    assert_eq!(result["result"]["title"], "Repair task handles");
}

#[test_matrix(["sqlite"])]
fn phase_list_human_output_includes_runnable_pending_phase_action(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    let epoch_id = exo_active_epoch_id(root);
    let pending_phase_id =
        exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Pending phase", None, None);

    exo_cmd_with_storage(root, backend)
        .args(["phase", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pending phase"))
        .stdout(predicate::str::contains(format!(
            "exo phase start {pending_phase_id}"
        )));
}

#[test_matrix(["sqlite"])]
fn active_phase_work_routes_phase_and_epoch_actions_through_finish(backend: &str) {
    for task_activity in ["start", "log"] {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        exo_init_with_storage(root, backend);
        exo_phase_start_with_storage(root, backend);
        let active_epoch_id = exo_active_epoch_id(root);
        let active_phase_id = exo_active_phase_id(root);
        let pending_phase_id = exo_plan_add_phase_with_storage(
            root,
            backend,
            &active_epoch_id,
            "Pending phase",
            None,
            None,
        );
        let future_epoch_id = exo_plan_add_epoch_with_storage(root, backend, "Future epoch");
        exo_plan_add_phase_with_storage(
            root,
            backend,
            &future_epoch_id,
            "Future epoch phase",
            None,
            None,
        );

        exo_cmd_with_storage(root, backend)
            .args(["goal", "add", "Started goal", "--id", "started-goal"])
            .assert()
            .success();
        exo_cmd_with_storage(root, backend)
            .args([
                "task",
                "add",
                "Task activity",
                "--id",
                "active-task",
                "--goal",
                "started-goal",
            ])
            .assert()
            .success();
        if task_activity == "start" {
            exo_cmd_with_storage(root, backend)
                .args(["task", "start", "started-goal::active-task"])
                .assert()
                .success();
        } else {
            exo_cmd_with_storage(root, backend)
                .args([
                    "task",
                    "log",
                    "started-goal::active-task",
                    "--message",
                    "Work recorded",
                ])
                .assert()
                .success();
        }

        exo_cmd_with_storage(root, backend)
            .args(["phase", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("exo phase finish"))
            .stdout(predicate::str::contains(format!("exo phase start {pending_phase_id}")).not())
            .stdout(predicate::str::contains(format!("exo phase start {active_phase_id}")).not());

        exo_cmd_with_storage(root, backend)
            .args(["epoch", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains(format!("exo epoch start {future_epoch_id}")).not());

        let blocked_switch = exo_cmd_with_storage(root, backend)
            .args(["--format", "json", "phase", "start", &pending_phase_id])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let blocked_switch: Value =
            serde_json::from_slice(&blocked_switch).expect("valid phase-start failure JSON");
        assert_eq!(blocked_switch["error"]["code"], "precondition_failed");
    }
}

#[test_matrix(["sqlite"])]
fn epoch_list_human_output_includes_runnable_review_action(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    let epoch_id = exo_active_epoch_id(root);
    let phase_id = exo_active_phase_id(root);
    exo_plan_update_status_with_storage(root, backend, &phase_id, "completed");

    exo_cmd_with_storage(root, backend)
        .args(["epoch", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "exo epoch review {epoch_id}"
        )));
}

#[test_matrix(["sqlite"])]
fn foreign_owned_phase_uses_takeover_action_and_suppresses_epoch_start(backend: &str) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    exo_init_with_storage(root, backend);
    exo_phase_start_with_storage(root, backend);
    let epoch_id = exo_active_epoch_id(root);
    let phase_id = exo_active_phase_id(root);
    SqliteWriter::open(db_path(root, None))
        .expect("open writer")
        .set_phase_owner(
            &phase_id,
            "workspace",
            "workspace:foreign",
            Some("workspace:foreign"),
            Some("/tmp/exo-foreign-workspace"),
        )
        .expect("set foreign phase owner");

    exo_cmd_with_storage(root, backend)
        .args(["phase", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "exo phase start {phase_id} --take-over"
        )));
    exo_cmd_with_storage(root, backend)
        .args(["epoch", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("exo epoch start {epoch_id}")).not());
}
