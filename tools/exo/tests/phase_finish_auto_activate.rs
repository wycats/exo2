#![allow(clippy::disallowed_methods)] // integration tests use real fs/process APIs

//! Test `phase finish` behavior: no archive side effects, no auto-activation, RFC surfacing.
//!
//! After finishing a phase, the system should:
//! - Mark the phase as completed
//! - NOT auto-activate the next phase (leave in between-phases state)
//! - Surface RFC promotion suggestions in the output

use exo::project::Project;
use std::path::Path;
use test_case::test_matrix;
use test_support::{
    exo_active_epoch_id, exo_active_phase_id, exo_cmd_with_storage, exo_init_with_storage,
    exo_plan_add_epoch_with_storage, exo_plan_add_phase_with_storage, write_implementation_plan,
};

mod test_support;

/// Helper to set up a git repo
fn init_git_repo(path: &Path) {
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .status()
        .expect("git config user.email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .status()
        .expect("git config user.name");
    std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(path)
        .status()
        .expect("git config commit.gpgsign");

    // Create and commit a file so we have a clean working tree
    std::fs::write(path.join("README.md"), "# Test\n").expect("write README");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .status()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-q", "-m", "init"])
        .current_dir(path)
        .status()
        .expect("git commit");
}

fn git_init_only(path: &Path) {
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status()
        .expect("git init");
}

#[test_matrix(["sqlite"])]
fn phase_finish_does_not_auto_activate_next_phase(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Set up: bootstrapped active phase plus two pending phases in the same epoch.
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    let epoch_id = exo_active_epoch_id(root);
    exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 2", None, None);
    exo_plan_add_phase_with_storage(root, backend, &epoch_id, "Phase 3", None, None);
    write_implementation_plan(
        root,
        &format!(
            "[phase]\nid = \"{phase_id}\"\ntitle = \"Phase 1\"\n\n[plan]\n\n[verification]\nautomated = []\nmanual = []\n",
        ),
    );

    // Git repo must be clean AFTER all setup
    init_git_repo(root);

    // Run phase finish
    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "finish"])
        .assert()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "phase finish should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Should NOT mention auto-activation
    assert!(
        !stdout.contains("Phase auto-activated"),
        "Should NOT auto-activate.\nstdout: {stdout}"
    );

    // Phase status should show no active phase (between-phases state)
    let json_output = exo_cmd_with_storage(root, backend)
        .args(["phase", "status", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&json_output))
        .expect("parse phase status JSON");
    assert!(
        json["phase_id"].is_null(),
        "Should be in between-phases state (no active phase).\njson: {json}"
    );
}

#[test_matrix(["sqlite"])]
fn phase_finish_does_not_archive_current_directory(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);

    let current_dir = root.join("docs/agent-context/current");
    let archive_dir = root.join("docs/agent-context/archive");

    // Write files to current/ that used to be archived by phase finish.
    write_implementation_plan(
        root,
        &format!(
            "[phase]\nid = \"{phase_id}\"\n\n[plan]\n\n[verification]\nautomated = []\nmanual = []\n",
        ),
    );
    std::fs::write(
        current_dir.join("walkthrough.md"),
        "# Phase 1 Walkthrough\n",
    )
    .expect("write walkthrough.md");

    // Git repo must be clean AFTER all setup
    init_git_repo(root);

    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "finish"])
        .assert()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "phase finish should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Phase finish should not create a phase archive.
    let phase_archive = archive_dir.join(&phase_id);
    assert!(
        !phase_archive.exists(),
        "Phase finish should not create archive directory at {:?}",
        phase_archive
    );
}

#[test_matrix(["sqlite"])]
fn phase_finish_does_not_activate_phase_in_different_epoch(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Set up: bootstrapped active phase in epoch-1, plus a pending phase in epoch-2.
    exo_init_with_storage(root, backend);
    let phase_id = exo_active_phase_id(root);
    let epoch2_id = exo_plan_add_epoch_with_storage(root, backend, "Second Epoch");
    exo_plan_add_phase_with_storage(root, backend, &epoch2_id, "Phase 2", None, None);
    write_implementation_plan(
        root,
        &format!(
            "[phase]\nid = \"{phase_id}\"\ntitle = \"Phase 1\"\n\n[plan]\n\n[verification]\nautomated = []\nmanual = []\n",
        ),
    );

    // Git repo must be clean AFTER all setup
    init_git_repo(root);

    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "finish"])
        .assert()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "phase finish should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Should NOT mention auto-activation
    assert!(
        !stdout.contains("Phase auto-activated"),
        "Should NOT auto-activate phase in different epoch.\nstdout: {stdout}"
    );

    // Phase status should show no active phase (between-phases state)
    let json_output = exo_cmd_with_storage(root, backend)
        .args(["phase", "status", "--format", "json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&json_output))
        .expect("parse phase status JSON");
    assert!(
        json["phase_id"].is_null(),
        "Should be in between-phases state (no active phase).\njson: {json}"
    );
}

#[test_matrix(["sqlite"])]
fn phase_finish_uses_project_db_path_in_git_repo(backend: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    git_init_only(root);
    exo_init_with_storage(root, backend);
    let project = Project::resolve(root).expect("resolve project");
    let phase_id = exo_active_phase_id(root);
    write_implementation_plan(
        root,
        &format!(
            "[phase]\nid = \"{phase_id}\"\ntitle = \"Phase 1\"\n\n[plan]\n\n[verification]\nautomated = []\nmanual = []\n",
        ),
    );

    assert!(project.db_path().exists(), "project DB should exist");
    assert!(
        !root.join(".cache/exo.db").exists(),
        "legacy root DB should not exist"
    );

    // Git repo must be clean AFTER all setup.
    init_git_repo(root);

    let output = exo_cmd_with_storage(root, backend)
        .args(["phase", "finish"])
        .assert()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "phase finish should succeed using project DB.\nstdout: {stdout}\nstderr: {stderr}"
    );

    assert!(project.db_path().exists(), "project DB should remain");
    assert!(
        !root.join(".cache/exo.db").exists(),
        "phase finish should not recreate legacy root DB"
    );
}
