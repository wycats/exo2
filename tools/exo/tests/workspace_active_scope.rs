#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::context::{SqliteLoader, SqliteWriter};
use exo::project::Project;
use exosuit_storage::OptionalExtension;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use test_support::{
    exo_active_phase_id, exo_cmd, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_epoch_with_storage, exo_plan_add_phase_with_storage,
    exo_plan_update_status_with_storage,
};

fn git_init(root: &Path) {
    let status = Command::new("git")
        .arg("init")
        .current_dir(root)
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");

    for (key, value) in [
        ("user.email", "exo-tests@example.com"),
        ("user.name", "Exo Tests"),
        ("commit.gpgsign", "false"),
    ] {
        let status = Command::new("git")
            .args(["config", key, value])
            .current_dir(root)
            .status()
            .expect("git config runs");
        assert!(status.success(), "git config {key} failed");
    }
}

fn commit_all(root: &Path, message: &str) {
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .status()
        .expect("git add runs");
    assert!(add.success(), "git add failed");

    let commit = Command::new("git")
        .args(["commit", "--no-gpg-sign", "-m", message])
        .current_dir(root)
        .status()
        .expect("git commit runs");
    assert!(commit.success(), "git commit failed");
}

fn git_worktree_add(primary: &Path, linked: &Path) {
    let status = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            "linked-test",
            linked.to_str().expect("linked path is utf-8"),
        ])
        .current_dir(primary)
        .status()
        .expect("git worktree add runs");
    assert!(status.success(), "git worktree add failed");
}

fn canonical_workspace(root: &Path) -> (Project, PathBuf, String) {
    let project = Project::resolve(root).expect("project resolves");
    let workspace = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .clone();
    let workspace_text = workspace.to_string_lossy().into_owned();
    (project, workspace, workspace_text)
}

fn workspace_pin(root: &Path) -> Option<String> {
    let project = Project::resolve(root).expect("project resolves");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    let loader = SqliteLoader::open(project.db_path()).expect("open db");
    loader
        .load_workspace_active_phase(&workspace_root)
        .expect("load pin")
}

fn phase_owner(root: &Path, phase_id: &str) -> Option<exo::context::sqlite_loader::PhaseOwnerData> {
    let project = Project::resolve(root).expect("project resolves");
    let loader = SqliteLoader::open(project.db_path()).expect("open db");
    loader.load_phase_owner(phase_id).expect("load owner")
}

fn current_branch(root: &Path) -> String {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git symbolic-ref runs");
    assert!(output.status.success(), "git symbolic-ref failed");
    String::from_utf8(output.stdout)
        .expect("branch is utf-8")
        .trim()
        .to_string()
}

fn exo_json(root: &Path, args: &[&str]) -> serde_json::Value {
    let output = exo_cmd(root)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid exo json")
}

fn result_current_owner(json: &serde_json::Value) -> &serde_json::Value {
    json.get("result")
        .and_then(|result| result.get("current_owner"))
        .expect("current_owner in result")
}

#[test]
fn init_claims_bootstrap_phase_owner() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let owner = phase_owner(root, &bootstrap).expect("bootstrap owner");
    assert_eq!(owner.owner_kind, "branch");
    assert_eq!(owner.owner_id, current_branch(root));
}

fn assert_workspace_pin_row(db_path: &Path, workspace_root: &str, expected_phase: &str) {
    let db = exosuit_storage::open_database(db_path).expect("open db");
    let phase: Option<String> = db
        .connection()
        .query_row(
            "SELECT p.text_id
             FROM workspace_active_phase wap
             JOIN phases p ON p.id = wap.phase_id
             WHERE wap.workspace_root = ?1",
            [workspace_root],
            |row| row.get(0),
        )
        .optional()
        .expect("query workspace pin row");
    assert_eq!(phase.as_deref(), Some(expected_phase));
}

#[test]
fn phase_start_claims_branch_owner_for_named_non_codex_worktree() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let phase = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Branch Phase", None, None);
    exo_plan_update_status_with_storage(root, "sqlite", &bootstrap, "completed");

    exo_cmd(root)
        .args(["phase", "start", &phase])
        .assert()
        .success();

    let owner = phase_owner(root, &phase).expect("phase owner");
    assert_eq!(owner.owner_kind, "branch");
    let branch = current_branch(root);
    assert_eq!(owner.owner_id, branch);

    let status = exo_json(root, &["--format", "json", "status"]);
    let current_owner = result_current_owner(&status);
    assert_eq!(
        current_owner.get("owner_kind").and_then(|v| v.as_str()),
        Some("branch")
    );
    assert_eq!(
        current_owner.get("owner_id").and_then(|v| v.as_str()),
        Some(branch.as_str())
    );
    assert_eq!(
        current_owner.get("owner_basis").and_then(|v| v.as_str()),
        Some("branch")
    );
    assert_eq!(
        current_owner.get("branch").and_then(|v| v.as_str()),
        Some(branch.as_str())
    );

    let phase_status = exo_json(root, &["--format", "json", "phase", "status"]);
    let current_owner = result_current_owner(&phase_status);
    assert_eq!(
        current_owner.get("owner_basis").and_then(|v| v.as_str()),
        Some("branch")
    );

    exo_cmd(root)
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("basis: branch"));
    exo_cmd(root)
        .args(["phase", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("basis: branch"));
}

#[test]
fn phase_start_claims_workspace_owner_for_detached_worktree() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");
    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    exo_plan_update_status_with_storage(root, "sqlite", &bootstrap, "completed");
    commit_all(root, "init");

    let status = Command::new("git")
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(root)
        .status()
        .expect("git checkout --detach runs");
    assert!(status.success(), "git checkout --detach failed");

    let phase =
        exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Detached Phase", None, None);

    exo_cmd(root)
        .args(["phase", "start", &phase])
        .assert()
        .success();

    let owner = phase_owner(root, &phase).expect("phase owner");
    assert_eq!(owner.owner_kind, "workspace");
    assert!(owner.owner_id.starts_with("workspace:"));

    let status = exo_json(root, &["--format", "json", "status"]);
    let current_owner = result_current_owner(&status);
    assert_eq!(
        current_owner.get("owner_kind").and_then(|v| v.as_str()),
        Some("workspace")
    );
    assert_eq!(
        current_owner.get("owner_basis").and_then(|v| v.as_str()),
        Some("detached_workspace")
    );
    assert!(
        current_owner
            .get("owner_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.starts_with("workspace:"))
    );
    assert!(current_owner.get("branch").is_none());
}

#[test]
fn phase_start_claims_workspace_owner_for_codex_worktree() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path().join(".codex/worktrees/abc/exo2");
    std::fs::create_dir_all(&root).expect("create codex worktree path");
    git_init(&root);
    exo_init_with_storage(&root, "sqlite");

    let bootstrap = exo_active_phase_id(&root);
    let epoch = test_support::exo_active_epoch_id(&root);
    let phase =
        exo_plan_add_phase_with_storage(&root, "sqlite", &epoch, "Workspace Phase", None, None);
    exo_plan_update_status_with_storage(&root, "sqlite", &bootstrap, "completed");

    exo_cmd(&root)
        .args(["phase", "start", &phase])
        .assert()
        .success();

    let owner = phase_owner(&root, &phase).expect("phase owner");
    assert_eq!(owner.owner_kind, "workspace");
    assert!(owner.owner_id.starts_with("workspace:"));
    let branch = current_branch(&root);

    let status = exo_json(&root, &["--format", "json", "status"]);
    let current_owner = result_current_owner(&status);
    assert_eq!(
        current_owner.get("owner_kind").and_then(|v| v.as_str()),
        Some("workspace")
    );
    assert_eq!(
        current_owner.get("owner_basis").and_then(|v| v.as_str()),
        Some("codex_workspace")
    );
    assert_eq!(
        current_owner.get("branch").and_then(|v| v.as_str()),
        Some(branch.as_str())
    );
}

#[test]
fn init_sets_workspace_active_phase_pin() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);

    exo_init_with_storage(root, "sqlite");
    let active_phase = exo_active_phase_id(root);

    assert_eq!(workspace_pin(root), Some(active_phase));
}

#[test]
fn phase_start_sets_workspace_pin_without_demoting_other_active_phases() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let next = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Next", None, None);
    let other_epoch = exo_plan_add_epoch_with_storage(root, "sqlite", "Other Epoch");
    let other = exo_plan_add_phase_with_storage(root, "sqlite", &other_epoch, "Other", None, None);

    exo_plan_update_status_with_storage(root, "sqlite", &bootstrap, "completed");
    exo_plan_update_status_with_storage(root, "sqlite", &other, "in-progress");

    exo_cmd_with_storage(root, "sqlite")
        .args(["phase", "start"])
        .assert()
        .success();

    assert_eq!(workspace_pin(root), Some(next.clone()));

    let project = Project::resolve(root).expect("project resolves");
    let state = SqliteLoader::open(project.db_path())
        .expect("open db")
        .load_state()
        .expect("load state");
    let next_phase = state.find_phase_by_id(&next).expect("next phase");
    let other_phase = state.find_phase_by_id(&other).expect("other phase");
    assert_eq!(next_phase.phase.status, "in-progress");
    assert_eq!(other_phase.phase.status, "in-progress");
}

#[test]
fn active_reads_use_workspace_pin_not_first_global_in_progress_phase() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let pinned = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Pinned", None, None);
    exo_cmd(root)
        .args([
            "goal",
            "add",
            "Bootstrap Goal",
            "--id",
            "bootstrap-goal",
            "--phase",
            &bootstrap,
        ])
        .assert()
        .success();
    exo_cmd(root)
        .args([
            "goal",
            "add",
            "Pinned Goal",
            "--id",
            "pinned-goal",
            "--phase",
            &pinned,
        ])
        .assert()
        .success();
    exo_plan_update_status_with_storage(root, "sqlite", &pinned, "in-progress");

    let project = Project::resolve(root).expect("project resolves");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    SqliteWriter::open(project.db_path())
        .expect("open writer")
        .set_workspace_active_phase(&workspace_root, &pinned)
        .expect("set pin");

    let status = exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).expect("valid status json");
    assert_eq!(
        status
            .get("result")
            .and_then(|r| r.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(pinned.as_str())
    );

    exo_cmd(root)
        .args(["goal", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pinned-goal"))
        .stdout(predicate::str::contains("bootstrap-goal").not());
}

#[test]
fn linked_worktrees_share_project_db_but_keep_distinct_workspace_active_pins() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&primary).expect("create primary");
    git_init(&primary);
    exo_init_with_storage(&primary, "sqlite");
    commit_all(&primary, "init");
    git_worktree_add(&primary, &linked);

    let epoch = test_support::exo_active_epoch_id(&primary);
    let phase_a =
        exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase A", None, None);
    let phase_b =
        exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase B", None, None);
    exo_plan_update_status_with_storage(&primary, "sqlite", &phase_a, "in-progress");
    exo_plan_update_status_with_storage(&primary, "sqlite", &phase_b, "in-progress");

    exo_cmd(&primary)
        .args([
            "goal",
            "add",
            "Primary Workspace Goal",
            "--id",
            "primary-workspace-goal",
            "--phase",
            &phase_a,
        ])
        .assert()
        .success();
    exo_cmd(&primary)
        .args([
            "goal",
            "add",
            "Linked Workspace Goal",
            "--id",
            "linked-workspace-goal",
            "--phase",
            &phase_b,
        ])
        .assert()
        .success();

    let (primary_project, primary_workspace, primary_workspace_text) =
        canonical_workspace(&primary);
    let (linked_project, linked_workspace, linked_workspace_text) = canonical_workspace(&linked);
    assert_eq!(primary_project.db_path(), linked_project.db_path());
    assert_ne!(primary_workspace, linked_workspace);

    let writer = SqliteWriter::open(primary_project.db_path()).expect("open writer");
    writer
        .add_task(
            "primary-workspace-goal",
            "primary-workspace-task",
            "Primary Workspace Task",
            None,
        )
        .expect("add primary task");
    writer
        .add_task(
            "linked-workspace-goal",
            "linked-workspace-task",
            "Linked Workspace Task",
            None,
        )
        .expect("add linked task");
    writer
        .set_workspace_active_phase(&primary_workspace_text, &phase_a)
        .expect("pin primary workspace");
    writer
        .set_workspace_active_phase(&linked_workspace_text, &phase_b)
        .expect("pin linked workspace");

    assert_workspace_pin_row(
        &primary_project.db_path(),
        &primary_workspace_text,
        &phase_a,
    );
    assert_workspace_pin_row(&primary_project.db_path(), &linked_workspace_text, &phase_b);
    assert_eq!(workspace_pin(&primary), Some(phase_a.clone()));
    assert_eq!(workspace_pin(&linked), Some(phase_b.clone()));

    let primary_status = exo_cmd(&primary)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let primary_status: serde_json::Value =
        serde_json::from_slice(&primary_status).expect("valid primary status json");
    assert_eq!(
        primary_status
            .get("result")
            .and_then(|r| r.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(phase_a.as_str())
    );

    let linked_status = exo_cmd(&linked)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let linked_status: serde_json::Value =
        serde_json::from_slice(&linked_status).expect("valid linked status json");
    assert_eq!(
        linked_status
            .get("result")
            .and_then(|r| r.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(phase_b.as_str())
    );

    let primary_plan = exo_json(&primary, &["--format", "json", "plan", "review"]);
    assert_eq!(
        primary_plan
            .get("result")
            .and_then(|r| r.get("active_phase"))
            .and_then(|active| active.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(phase_a.as_str())
    );

    let linked_plan = exo_json(&linked, &["--format", "json", "plan", "review"]);
    assert_eq!(
        linked_plan
            .get("result")
            .and_then(|r| r.get("active_phase"))
            .and_then(|active| active.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(phase_b.as_str())
    );

    exo_cmd(&primary)
        .args(["plan", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Plan state also contains other in-progress phases",
        ))
        .stdout(predicate::str::contains("Phase B"));

    exo_cmd(&primary)
        .args(["goal", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("primary-workspace-goal"))
        .stdout(predicate::str::contains("linked-workspace-goal").not());
    exo_cmd(&linked)
        .args(["goal", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked-workspace-goal"))
        .stdout(predicate::str::contains("primary-workspace-goal").not());

    exo_cmd(&primary)
        .args(["task", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("primary-workspace-task"))
        .stdout(predicate::str::contains("linked-workspace-task").not());
    exo_cmd(&linked)
        .args(["task", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked-workspace-task"))
        .stdout(predicate::str::contains("primary-workspace-task").not());
}

#[test]
fn plan_review_keeps_whole_plan_diagnostics_when_workspace_is_pinned_later() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let phase_a = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Phase A", None, None);
    let phase_b = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Phase B", None, None);
    let phase_c = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Phase C", None, None);
    exo_plan_update_status_with_storage(root, "sqlite", &bootstrap, "completed");
    exo_plan_update_status_with_storage(root, "sqlite", &phase_a, "in-progress");
    exo_plan_update_status_with_storage(root, "sqlite", &phase_b, "completed");
    exo_plan_update_status_with_storage(root, "sqlite", &phase_c, "in-progress");

    let (project, _, workspace_text) = canonical_workspace(root);
    SqliteWriter::open(project.db_path())
        .expect("open writer")
        .set_workspace_active_phase(&workspace_text, &phase_c)
        .expect("pin workspace to later phase");

    let review = exo_json(root, &["--format", "json", "plan", "review"]);
    assert_eq!(
        review
            .get("result")
            .and_then(|r| r.get("active_phase"))
            .and_then(|active| active.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(phase_c.as_str())
    );
    let non_linearity = review
        .get("result")
        .and_then(|r| r.get("non_linearity"))
        .and_then(|v| v.as_array())
        .expect("non-linearity diagnostics");
    assert!(
        non_linearity
            .iter()
            .any(|phase| phase.get("phase_id").and_then(|v| v.as_str()) == Some(phase_b.as_str())),
        "completed phase after the earliest in-progress phase should remain diagnostic: {review}"
    );

    exo_cmd(root)
        .args(["plan", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Plan state also contains other in-progress phases",
        ))
        .stdout(predicate::str::contains("Phase A"))
        .stdout(predicate::str::contains("Non-Linearity Detected"))
        .stdout(predicate::str::contains("Phase B"));
}

#[test]
fn linked_worktrees_claim_distinct_phase_owners_and_block_foreign_mutation() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&primary).expect("create primary");
    git_init(&primary);
    exo_init_with_storage(&primary, "sqlite");
    commit_all(&primary, "init");
    git_worktree_add(&primary, &linked);

    let bootstrap = exo_active_phase_id(&primary);
    let epoch = test_support::exo_active_epoch_id(&primary);
    let phase_a =
        exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase A", None, None);
    let phase_b =
        exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase B", None, None);
    let phase_c =
        exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase C", None, None);
    exo_plan_update_status_with_storage(&primary, "sqlite", &bootstrap, "completed");

    exo_cmd(&primary)
        .args(["phase", "start", &phase_a])
        .assert()
        .success();
    exo_cmd(&linked)
        .args(["epoch", "start", &epoch])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase B"));

    assert_eq!(workspace_pin(&primary), Some(phase_a.clone()));
    assert_eq!(workspace_pin(&linked), Some(phase_b.clone()));

    let owner_a = phase_owner(&primary, &phase_a).expect("phase a owner");
    let owner_b = phase_owner(&linked, &phase_b).expect("phase b owner");
    assert_eq!(owner_a.owner_kind, "branch");
    assert_eq!(owner_a.owner_id, current_branch(&primary));
    assert_eq!(owner_b.owner_kind, "branch");
    assert_eq!(owner_b.owner_id, current_branch(&linked));

    exo_cmd(&primary)
        .args(["phase", "start", &phase_b])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    let project = Project::resolve(&primary).expect("project resolves");
    let state = SqliteLoader::open(project.db_path())
        .expect("open db")
        .load_state()
        .expect("load state");
    assert_eq!(
        state
            .find_phase_by_id(&phase_a)
            .expect("phase a")
            .phase
            .status,
        "in-progress",
        "failed phase start must not demote current phase"
    );

    exo_cmd(&primary)
        .args(["goal", "add", "Primary Goal", "--id", "primary-goal"])
        .assert()
        .success();
    exo_cmd(&primary)
        .args(["goal", "add", "Collision Goal", "--id", &phase_b])
        .assert()
        .success();
    exo_cmd(&primary)
        .args(["plan", "update-status", "primary-goal", "in-progress"])
        .assert()
        .success();
    exo_cmd(&linked)
        .args(["goal", "reorder", "primary-goal", "bottom"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["goal", "remove", "primary-goal", "--phase", &phase_b])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));

    exo_cmd(&linked)
        .args(["phase", "start", &phase_a])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["epoch", "bankrupt", &epoch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));

    exo_cmd(&linked)
        .args(["phase", "focus", &phase_a])
        .assert()
        .success();
    exo_cmd(&linked)
        .args([
            "goal",
            "add",
            "Foreign Goal",
            "--id",
            "foreign-goal",
            "--phase",
            &phase_a,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["plan", "update-status", &phase_a, "completed"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["plan", "update-status", "primary-goal", "in-progress"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["plan", "update-status", &phase_b, "in-progress"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["plan", "move-goals", &phase_a, &phase_b, "primary-goal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));

    let inbox = exo_cmd(&primary)
        .args(["--format", "json", "inbox", "add", "Foreign Inbox Goal"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inbox: serde_json::Value = serde_json::from_slice(&inbox).expect("valid inbox json");
    let inbox_id = inbox
        .get("result")
        .and_then(|result| result.get("id"))
        .and_then(|id| id.as_str())
        .expect("inbox id");
    exo_cmd(&linked)
        .args(["inbox", "resolve", inbox_id, "--promote", "goal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&linked)
        .args(["strike", "start", "--name", "foreign", "--goal", "Nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("owned by another branch"));
    exo_cmd(&primary)
        .args(["goal", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("primary-goal"));

    let status = exo_cmd(&linked)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).expect("valid status json");
    assert_eq!(
        status
            .get("result")
            .and_then(|r| r.get("phase_owner"))
            .and_then(|r| r.get("ownedElsewhere"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    let current_owner = result_current_owner(&status);
    let linked_branch = current_branch(&linked);
    assert_eq!(
        current_owner.get("owner_kind").and_then(|v| v.as_str()),
        Some("branch")
    );
    assert_eq!(
        current_owner.get("owner_basis").and_then(|v| v.as_str()),
        Some("branch")
    );
    assert_eq!(
        current_owner.get("branch").and_then(|v| v.as_str()),
        Some(linked_branch.as_str())
    );

    exo_cmd(&linked)
        .args(["phase", "start", &phase_c])
        .assert()
        .success()
        .stdout(predicate::str::contains("Phase C"));
    assert_eq!(workspace_pin(&linked), Some(phase_c.clone()));
}

#[test]
fn phase_start_take_over_replaces_foreign_owner() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&primary).expect("create primary");
    git_init(&primary);
    exo_init_with_storage(&primary, "sqlite");
    commit_all(&primary, "init");
    git_worktree_add(&primary, &linked);

    let bootstrap = exo_active_phase_id(&primary);
    let epoch = test_support::exo_active_epoch_id(&primary);
    let phase = exo_plan_add_phase_with_storage(&primary, "sqlite", &epoch, "Phase A", None, None);
    exo_plan_update_status_with_storage(&primary, "sqlite", &bootstrap, "completed");

    exo_cmd(&primary)
        .args(["phase", "start", &phase])
        .assert()
        .success();
    exo_cmd(&linked)
        .args(["phase", "start", &phase, "--take-over"])
        .assert()
        .success();

    let owner = phase_owner(&linked, &phase).expect("phase owner");
    assert_eq!(owner.owner_kind, "branch");
    assert_eq!(owner.owner_id, current_branch(&linked));
}

#[test]
fn stale_workspace_owner_is_reported_and_can_be_released() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let project = Project::resolve(root).expect("project resolves");
    SqliteWriter::open(project.db_path())
        .expect("open writer")
        .set_phase_owner(
            &bootstrap,
            "workspace",
            "workspace:missing:abc123",
            Some("workspace:missing:abc123"),
            Some("/tmp/exo-missing-workspace-root"),
        )
        .expect("set owner");

    let list = exo_cmd(root)
        .args(["--format", "json", "phase", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list: serde_json::Value = serde_json::from_slice(&list).expect("valid phase list json");
    assert_eq!(
        list.get("result")
            .and_then(|r| r.get("phases"))
            .and_then(|p| p.as_array())
            .and_then(|phases| phases.first())
            .and_then(|phase| phase.get("stale_owner"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    exo_cmd(root)
        .args(["phase", "release", &bootstrap])
        .assert()
        .success();
    assert!(phase_owner(root, &bootstrap).is_none());
}

#[test]
fn plan_review_reports_no_current_phase_for_unpinned_ambiguous_workspace() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let second = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Second", None, None);
    exo_plan_update_status_with_storage(root, "sqlite", &second, "in-progress");

    let project = Project::resolve(root).expect("project resolves");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    SqliteWriter::open(project.db_path())
        .expect("open writer")
        .clear_workspace_active_phase(&workspace_root)
        .expect("clear workspace active phase");

    let status = exo_json(root, &["--format", "json", "status"]);
    assert_eq!(status.get("result").and_then(|r| r.get("phase_id")), None);

    let details = exo_json(root, &["--format", "json", "phase", "read-details"]);
    assert_eq!(details.get("result"), Some(&serde_json::Value::Null));

    let review = exo_json(root, &["--format", "json", "plan", "review"]);
    assert_eq!(
        review.get("result").and_then(|r| r.get("active_phase")),
        Some(&serde_json::Value::Null)
    );
    let in_progress = review
        .get("result")
        .and_then(|r| r.get("in_progress_phases"))
        .and_then(|v| v.as_array())
        .expect("in-progress diagnostics");
    assert!(
        in_progress
            .iter()
            .any(|phase| phase.get("phase_id").and_then(|v| v.as_str()) == Some(bootstrap.as_str()))
    );
    assert!(
        in_progress
            .iter()
            .any(|phase| phase.get("phase_id").and_then(|v| v.as_str()) == Some(second.as_str()))
    );
    assert!(
        review
            .get("result")
            .and_then(|r| r.get("non_linearity"))
            .and_then(|v| v.as_array())
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(
        review
            .get("result")
            .and_then(|r| r.get("progress_heuristic"))
            .and_then(|progress| progress.get("mode"))
            .and_then(|v| v.as_str()),
        Some("ORIENT")
    );
    assert_eq!(
        review
            .get("result")
            .and_then(|r| r.get("progress_heuristic"))
            .and_then(|progress| progress.get("pending_phases_in_active_epoch")),
        Some(&serde_json::Value::Null)
    );

    exo_cmd(root)
        .args(["plan", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("**Active Phase**: None"))
        .stdout(predicate::str::contains(
            "Plan state contains in-progress phases",
        ));
}

#[test]
fn phase_finish_uses_workspace_pin_and_keeps_anchor() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = test_support::exo_active_epoch_id(root);
    let pinned = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Pinned", None, None);
    exo_plan_update_status_with_storage(root, "sqlite", &pinned, "in-progress");

    let project = Project::resolve(root).expect("project resolves");
    let workspace_root = project
        .workspace_root
        .as_ref()
        .expect("workspace root")
        .to_string_lossy()
        .into_owned();
    SqliteWriter::open(project.db_path())
        .expect("open writer")
        .set_workspace_active_phase(&workspace_root, &pinned)
        .expect("set pin");

    commit_all(root, "setup");

    exo_cmd(root).args(["phase", "finish"]).assert().success();

    assert_eq!(workspace_pin(root), Some(pinned.clone()));

    let state = SqliteLoader::open(project.db_path())
        .expect("open db")
        .load_state()
        .expect("load state");
    assert_eq!(
        state
            .find_phase_by_id(&pinned)
            .expect("pinned")
            .phase
            .status,
        "completed"
    );
    assert_eq!(
        state
            .find_phase_by_id(&bootstrap)
            .expect("bootstrap")
            .phase
            .status,
        "in-progress"
    );

    let status = exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).expect("valid status json");
    assert_eq!(status.get("result").and_then(|r| r.get("phase_id")), None);
}

#[test]
fn epoch_start_pins_requested_epoch_for_current_workspace() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let epoch = exo_plan_add_epoch_with_storage(root, "sqlite", "Requested Epoch");
    let phase =
        exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Requested Phase", None, None);

    exo_cmd_with_storage(root, "sqlite")
        .args(["epoch", "start", &epoch])
        .assert()
        .success();

    assert_eq!(workspace_pin(root), Some(phase.clone()));

    let project = Project::resolve(root).expect("project resolves");
    let state = SqliteLoader::open(project.db_path())
        .expect("open db")
        .load_state()
        .expect("load state");
    assert_eq!(
        state
            .find_phase_by_id(&phase)
            .expect("requested phase")
            .phase
            .status,
        "in-progress"
    );
    assert_eq!(
        state
            .find_phase_by_id(&bootstrap)
            .expect("bootstrap")
            .phase
            .status,
        "in-progress"
    );
}

#[test]
fn no_workspace_root_uses_single_global_active_fallback() {
    let temp = ok_or_return!(tempfile::tempdir(), "failed to create tempdir");
    let root = temp.path();
    exo_init_with_storage(root, "sqlite");

    let bootstrap = exo_active_phase_id(root);
    let status = exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).expect("valid status json");
    assert_eq!(
        status
            .get("result")
            .and_then(|r| r.get("phase_id"))
            .and_then(|v| v.as_str()),
        Some(bootstrap.as_str())
    );

    let epoch = test_support::exo_active_epoch_id(root);
    let second = exo_plan_add_phase_with_storage(root, "sqlite", &epoch, "Second", None, None);
    exo_plan_update_status_with_storage(root, "sqlite", &second, "in-progress");

    let status = exo_cmd(root)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: serde_json::Value = serde_json::from_slice(&status).expect("valid status json");
    assert_eq!(status.get("result").and_then(|r| r.get("phase_id")), None);
}
