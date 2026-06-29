#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::api::handler::help_for_address;
use exo::api::protocol::{
    Address, CallParams, ErrorCode, Op, PROTOCOL_VERSION, RequestEnvelope, Status,
};
use exo::command::command_spec::CommandSpec;
use exo::command::registry::default_registry;
use exo::context::{AgentContext, SQLITE_DB_PATH, SqliteLoader, SqliteWriter};
use exo::daemon_transport::DaemonEndpoint;
use exo::project::{Project, ProjectResolver, SidecarAutoPushPolicy};
use serde_json::{Value as JsonValue, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use test_support::{
    exo_active_phase_id, exo_cmd, exo_init, exo_rfc_create, run_machine_channel_in_process,
};

fn git_init(root: &Path) {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("run git init");

    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git command");

    assert!(
        output.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_commit_all(root: &Path) {
    run_git_ok(root, &["add", "."]);
    run_git_ok(
        root,
        &[
            "-c",
            "user.name=Exo Test",
            "-c",
            "user.email=exo@example.invalid",
            "commit",
            "--no-gpg-sign",
            "-m",
            "init",
        ],
    );
}

fn git_worktree_add(primary: &Path, linked: &Path) {
    run_git_ok(
        primary,
        &[
            "worktree",
            "add",
            "-b",
            "linked-test",
            linked.to_str().expect("linked path is utf-8"),
        ],
    );
}

fn git_common_dir(root: &Path) -> PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .expect("run git rev-parse");

    assert!(
        output.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        .canonicalize()
        .expect("canonical git common dir")
}

fn run_git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git command");

    assert!(
        output.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout is utf-8")
}

fn project_db_path(root: &Path) -> PathBuf {
    Project::resolve(root).expect("resolve project").db_path()
}

fn legacy_root_db_path(root: &Path) -> PathBuf {
    root.join(SQLITE_DB_PATH)
}

fn assert_db_has_epoch(db_path: &Path, title: &str) {
    let loader = SqliteLoader::open(db_path).expect("open sqlite db");
    let state = loader.load_state().expect("load sqlite state");
    assert!(
        state.epochs.iter().any(|epoch| epoch.title == title),
        "expected {} to contain epoch {title:?}",
        db_path.display()
    );
}

fn assert_db_lacks_epoch(db_path: &Path, title: &str) {
    let loader = SqliteLoader::open(db_path).expect("open sqlite db");
    let state = loader.load_state().expect("load sqlite state");
    assert!(
        state.epochs.iter().all(|epoch| epoch.title != title),
        "expected {} not to contain epoch {title:?}",
        db_path.display()
    );
}

fn assert_project_resolve_shape(result: &JsonValue, root: &Path) {
    let canonical_root = root.canonicalize().expect("canonical root");
    let common_dir = git_common_dir(root);
    let common_dir_text = common_dir.to_string_lossy().to_string();
    let canonical_root_text = canonical_root.to_string_lossy().to_string();

    assert_eq!(result["kind"], "project.resolve");
    assert_eq!(result["ok"], true);

    let id = result["project"]["id"]
        .as_str()
        .expect("project id is a string");
    assert_eq!(id.len(), 16);
    assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));

    assert_eq!(
        result["project"]["git_common_dir"].as_str(),
        Some(common_dir_text.as_str())
    );
    assert_eq!(
        result["project"]["workspace_root"].as_str(),
        Some(canonical_root_text.as_str())
    );
    assert_eq!(result["project"]["policy"], "repo");

    let state_root = canonical_root.join(".exo");
    let state_root_text = state_root.to_string_lossy().to_string();
    let db_path_text = state_root
        .join("cache")
        .join("exo.db")
        .to_string_lossy()
        .to_string();
    let runtime_dir_text = state_root.join("runtime").to_string_lossy().to_string();
    let socket_path_text = state_root
        .join("runtime/daemon.sock")
        .to_string_lossy()
        .to_string();
    let runtime_dir = state_root.join("runtime");
    #[cfg(windows)]
    let endpoint_text = DaemonEndpoint::from_runtime_dir(&runtime_dir).display();
    #[cfg(not(windows))]
    let endpoint_text =
        DaemonEndpoint::from_socket_path(&runtime_dir.join("daemon.sock")).display();
    let pid_path_text = state_root
        .join("runtime/daemon.pid")
        .to_string_lossy()
        .to_string();
    assert_eq!(
        result["paths"]["state_root"].as_str(),
        Some(state_root_text.as_str())
    );
    assert_eq!(
        result["paths"]["db_path"].as_str(),
        Some(db_path_text.as_str())
    );
    assert_eq!(
        result["paths"]["runtime_dir"].as_str(),
        Some(runtime_dir_text.as_str())
    );
    assert_eq!(
        result["paths"]["socket_path"].as_str(),
        Some(socket_path_text.as_str())
    );
    assert_eq!(
        result["paths"]["endpoint"].as_str(),
        Some(endpoint_text.as_str())
    );
    assert_eq!(
        result["paths"]["pid_path"].as_str(),
        Some(pid_path_text.as_str())
    );
}

fn cli_project_resolve(root: &Path) -> JsonValue {
    let output = exo_cmd(root)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert_eq!(envelope["status"], "ok");
    envelope["result"].clone()
}

fn cli_project_list_with_env(root: &Path, home: &Path, config_home: &Path) -> JsonValue {
    let output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .args(["--direct", "--format", "json", "project", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert_eq!(envelope["status"], "ok");
    envelope["result"].clone()
}

fn cli_project_snapshot_with_env(
    root: &Path,
    home: &Path,
    config_home: &Path,
    project_id: &str,
) -> JsonValue {
    let output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .args([
            "--direct", "--format", "json", "project", "snapshot", project_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert_eq!(envelope["status"], "ok");
    envelope["result"].clone()
}

fn json_path(result: &JsonValue, section: &str, field: &str) -> PathBuf {
    PathBuf::from(
        result[section][field]
            .as_str()
            .unwrap_or_else(|| panic!("{section}.{field} is a string")),
    )
}

#[test]
fn cli_json_project_list_returns_current_project() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    assert_eq!(result["kind"], "project.list");
    assert_eq!(result["ok"], true);

    let current_project_id = result["current_project_id"]
        .as_str()
        .expect("current project id");
    let projects = result["projects"].as_array().expect("projects array");
    let current = projects
        .iter()
        .find(|project| project["id"] == current_project_id)
        .expect("current project in list");
    assert_eq!(current["source"], "current");
    assert_eq!(current["state"], "repo");
    assert_eq!(current["selectable"], true);
    assert_eq!(current["current"], true);
    assert_eq!(
        current["workspace_root"].as_str(),
        Some(
            repo.canonicalize()
                .expect("canonical repo")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[test]
fn cli_json_project_list_includes_policy_and_sidecar_root_projects() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("client-api");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&project_dir).expect("create sidecar project dir");
    git_init(&repo);
    std::fs::write(
        project_dir.join("sidecar.toml"),
        "[sidecar]\nkey = \"client-api\"\nproject_id = \"policy-project\"\n",
    )
    .expect("write sidecar manifest");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.policy-project]\nstate = \"sidecar\"\nsidecar_key = \"client-api\"\nsidecar_root = {:?}\n\n[projects.shadow-only]\nstate = \"shadow\"\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    let projects = result["projects"].as_array().expect("projects array");
    let policy_project = projects
        .iter()
        .find(|project| project["id"] == "policy-project")
        .expect("policy project");
    assert_eq!(policy_project["source"], "sidecar-root");
    assert_eq!(policy_project["state"], "sidecar");
    assert_eq!(policy_project["sidecar_key"], "client-api");
    assert_eq!(policy_project["selectable"], false);
    assert_eq!(
        policy_project["state_root"].as_str(),
        Some(project_dir.to_string_lossy().as_ref())
    );

    let shadow_project = projects
        .iter()
        .find(|project| project["id"] == "shadow-only")
        .expect("shadow policy project");
    let shadow_root = home.join(".exo").join("projects").join("shadow-only");
    assert_eq!(shadow_project["source"], "local-policy");
    assert_eq!(shadow_project["state"], "shadow");
    assert_eq!(
        shadow_project["state_root"].as_str(),
        Some(shadow_root.to_string_lossy().as_ref())
    );
    assert_eq!(
        shadow_project["db_path"].as_str(),
        Some(
            shadow_root
                .join("cache")
                .join("exo.db")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(shadow_project["selectable"], false);
}

#[test]
fn cli_json_project_list_treats_shadow_state_root_as_shadow_policy() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        "[projects.shadow-sentinel]\nstate_root = \"shadow\"\n",
    )
    .expect("write project policy");

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    let projects = result["projects"].as_array().expect("projects array");
    let shadow_project = projects
        .iter()
        .find(|project| project["id"] == "shadow-sentinel")
        .expect("shadow sentinel project");
    let shadow_root = home.join(".exo").join("projects").join("shadow-sentinel");

    assert_eq!(shadow_project["state"], "shadow");
    assert_eq!(
        shadow_project["state_root"].as_str(),
        Some(shadow_root.to_string_lossy().as_ref())
    );
    assert_eq!(
        shadow_project["db_path"].as_str(),
        Some(
            shadow_root
                .join("cache")
                .join("exo.db")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        shadow_project["runtime_dir"].as_str(),
        Some(shadow_root.join("runtime").to_string_lossy().as_ref())
    );
}

#[test]
fn cli_project_snapshot_reads_sidecar_project_state_without_workspace_root() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let workspace = temp.path().join("client-api-workspace");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("client-api");
    let db_path = project_dir.join("cache").join("exo.db");
    let workspace_key = workspace.to_string_lossy().to_string();
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create sidecar cache");
    git_init(&repo);
    git_init(&workspace);
    exo_init(&repo);
    let project_id = Project::resolve(&workspace)
        .expect("resolve workspace project")
        .id
        .as_str()
        .to_string();
    std::fs::write(
        project_dir.join("sidecar.toml"),
        format!("[sidecar]\nkey = \"client-api\"\nproject_id = \"{project_id}\"\n"),
    )
    .expect("write sidecar manifest");
    let writer = SqliteWriter::open(&db_path).expect("open sidecar db");
    let epoch_id = writer
        .add_epoch("Sidecar Epoch", None, &[])
        .expect("add sidecar epoch");
    let phase_id = writer
        .add_phase(&epoch_id, "Sidecar Phase", "regular", None, &[])
        .expect("add sidecar phase");
    writer
        .update_phase_status(&phase_id, "in-progress")
        .expect("activate sidecar phase");
    writer
        .set_workspace_active_phase(&workspace_key, &phase_id)
        .expect("set sidecar workspace active phase");

    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{project_id}]\nstate = \"sidecar\"\nsidecar_key = \"client-api\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_snapshot_with_env(&repo, &home, &config_home, &project_id);

    assert_eq!(result["kind"], "project.snapshot");
    assert_eq!(result["ok"], true);
    assert_eq!(result["project"]["id"], project_id);
    assert_eq!(result["project"]["state_readable"], true);
    assert_eq!(result["project"]["workspace_available"], true);
    assert_eq!(result["project"]["commands_available"], true);
    assert_eq!(result["project"]["selectable"], true);
    assert_eq!(result["workspace_key"], workspace_key);
    assert_eq!(result["roots"]["status"]["phase_title"], "Sidecar Phase");
    assert_eq!(result["roots"]["status"]["epoch_title"], "Sidecar Epoch");
    assert_eq!(result["roots"]["status"]["workspace_key"], workspace_key);
    assert_eq!(result["roots"]["status"]["workspace_root"], workspace_key);
    assert_eq!(
        result["roots"]["status"]["state_root"].as_str(),
        Some(project_dir.to_string_lossy().as_ref())
    );
    assert_eq!(result["roots"]["status"]["commands_available"], true);
    let diagnostics = result["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics.iter().all(|diagnostic| diagnostic["message"]
            .as_str()
            .is_none_or(|message| !message.contains("No canonical local checkout"))),
        "{diagnostics:?}"
    );
}

#[test]
fn cli_project_snapshot_rejects_checkout_from_different_project_for_commands() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let workspace = temp.path().join("wrong-workspace");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("client-api");
    let db_path = project_dir.join("cache").join("exo.db");
    let workspace_key = workspace.to_string_lossy().to_string();
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create sidecar cache");
    git_init(&repo);
    git_init(&workspace);
    exo_init(&repo);
    std::fs::write(
        project_dir.join("sidecar.toml"),
        "[sidecar]\nkey = \"client-api\"\nproject_id = \"policy-project\"\n",
    )
    .expect("write sidecar manifest");
    let writer = SqliteWriter::open(&db_path).expect("open sidecar db");
    let epoch_id = writer
        .add_epoch("Sidecar Epoch", None, &[])
        .expect("add sidecar epoch");
    let phase_id = writer
        .add_phase(&epoch_id, "Sidecar Phase", "regular", None, &[])
        .expect("add sidecar phase");
    writer
        .update_phase_status(&phase_id, "in-progress")
        .expect("activate sidecar phase");
    writer
        .set_workspace_active_phase(&workspace_key, &phase_id)
        .expect("set sidecar workspace active phase");

    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.policy-project]\nstate = \"sidecar\"\nsidecar_key = \"client-api\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_snapshot_with_env(&repo, &home, &config_home, "policy-project");

    assert_eq!(result["kind"], "project.snapshot");
    assert_eq!(result["ok"], true);
    assert_eq!(result["project"]["id"], "policy-project");
    assert_eq!(result["project"]["state_readable"], true);
    assert_eq!(result["project"]["workspace_available"], false);
    assert_eq!(result["project"]["commands_available"], false);
    assert_eq!(result["roots"]["status"]["commands_available"], false);
    let diagnostics = result["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic["message"].as_str().is_some_and(|message| {
                    message.contains("belongs to project")
                        && message.contains("not selected project policy-project")
                })
            ),
        "{diagnostics:?}"
    );
}

#[test]
fn cli_json_project_list_reports_sidecar_entry_diagnostics() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("missing-manifest");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&project_dir).expect("create sidecar project dir");
    git_init(&repo);
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.sidecar-policy]\nstate = \"sidecar\"\nsidecar_key = \"missing-manifest\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    let projects = result["projects"].as_array().expect("projects array");
    let sidecar_project = projects
        .iter()
        .find(|project| project["id"] == "missing-manifest")
        .expect("sidecar root project");
    let diagnostics = sidecar_project["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("Missing sidecar manifest"))),
        "{diagnostics:?}"
    );
}

#[test]
fn cli_json_project_list_reports_malformed_sidecar_manifest_diagnostics() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("broken-manifest");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&project_dir).expect("create sidecar project dir");
    git_init(&repo);
    std::fs::write(
        project_dir.join("sidecar.toml"),
        "[sidecar\nkey = \"oops\"\n",
    )
    .expect("write malformed manifest");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.sidecar-policy]\nstate = \"sidecar\"\nsidecar_key = \"broken-manifest\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    let projects = result["projects"].as_array().expect("projects array");
    let sidecar_project = projects
        .iter()
        .find(|project| project["id"] == "broken-manifest")
        .expect("sidecar root project");
    let diagnostics = sidecar_project["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("Failed to parse"))),
        "{diagnostics:?}"
    );
}

#[test]
fn cli_json_project_list_reports_stale_policy_entries_as_catalog_diagnostics() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("missing-sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.stale-policy]\nstate = \"sidecar\"\nsidecar_key = \"coverage-sidecar\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let result = cli_project_list_with_env(&repo, &home, &config_home);
    let projects = result["projects"].as_array().expect("projects array");
    assert!(
        projects
            .iter()
            .all(|project| project["id"] != "stale-policy"),
        "stale local-policy sidecar should not be returned as a project: {projects:?}"
    );
    let diagnostics = result["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic["message"].as_str().is_some_and(|message| {
                    message.contains("stale sidecar policy")
                        && message.contains("coverage-sidecar")
                        && message.contains("does not exist")
                })
            ),
        "{diagnostics:?}"
    );
}

#[test]
fn cli_project_repair_stale_sidecars_previews_and_applies_policy_cleanup() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let valid_project_dir = sidecar_root.join("projects").join("live-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&valid_project_dir).expect("create valid sidecar project");
    git_init(&repo);
    exo_init(&repo);
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.stale-policy]\nstate = \"sidecar\"\nsidecar_key = \"coverage-sidecar\"\nsidecar_root = {:?}\n\n[projects.live-policy]\nstate = \"sidecar\"\nsidecar_key = \"live-sidecar\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy(),
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let preview_output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "repair",
            "--stale-sidecars",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: JsonValue = serde_json::from_slice(&preview_output).expect("preview json");
    assert_eq!(preview["status"], "ok");
    assert_eq!(preview["result"]["kind"], "project.repair.preview");
    assert_eq!(preview["result"]["preview"], true);
    assert_eq!(
        preview["result"]["stale_sidecars"]
            .as_array()
            .expect("stale sidecars")
            .len(),
        1
    );
    assert_eq!(preview["result"]["stale_sidecars"][0]["id"], "stale-policy");
    let before_apply = std::fs::read_to_string(&policy_path).expect("read policy before apply");
    assert!(
        before_apply.contains("stale-policy"),
        "preview must not mutate policy"
    );

    let apply_output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "repair-apply",
            "--stale-sidecars",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let applied: JsonValue = serde_json::from_slice(&apply_output).expect("apply json");
    assert_eq!(applied["status"], "ok");
    assert_eq!(applied["result"]["kind"], "project.repair.apply");
    assert_eq!(
        applied["result"]["removed"]
            .as_array()
            .expect("removed")
            .len(),
        1
    );

    let policy_after = std::fs::read_to_string(&policy_path).expect("read policy after apply");
    assert!(!policy_after.contains("stale-policy"));
    assert!(policy_after.contains("live-policy"));

    let legacy_apply_output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "repair",
            "--stale-sidecars",
            "--apply",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let legacy_applied: JsonValue =
        serde_json::from_slice(&legacy_apply_output).expect("legacy apply json");
    assert_eq!(legacy_applied["status"], "ok");
    assert_eq!(legacy_applied["result"]["kind"], "project.repair.apply");
}

#[test]
fn cli_project_repair_previews_current_stale_sidecar_without_materializing_it() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    let project_id = Project::resolve(&repo)
        .expect("resolve default project")
        .id
        .as_str()
        .to_string();
    let stale_state_root = sidecar_root.join("projects").join("current-stale");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{project_id}]\nstate = \"sidecar\"\nsidecar_key = \"current-stale\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write project policy");

    let preview_output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "repair",
            "--stale-sidecars",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: JsonValue = serde_json::from_slice(&preview_output).expect("preview json");

    assert_eq!(preview["status"], "ok");
    assert_eq!(preview["result"]["kind"], "project.repair.preview");
    assert_eq!(preview["result"]["stale_sidecars"][0]["id"], project_id);
    assert!(
        !stale_state_root.exists(),
        "repair preview should not materialize stale sidecar state"
    );

    let apply_output = assert_cmd::cargo::cargo_bin_cmd!("exo")
        .current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "repair-apply",
            "--stale-sidecars",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let applied: JsonValue = serde_json::from_slice(&apply_output).expect("apply json");
    assert_eq!(applied["status"], "ok");
    assert_eq!(applied["result"]["kind"], "project.repair.apply");
    assert_eq!(applied["result"]["removed"][0]["id"], project_id);
    assert!(
        !stale_state_root.exists(),
        "repair apply should not materialize stale sidecar state through post-write dumps"
    );
}

#[test]
fn machine_channel_project_resolve_returns_stable_schema() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "project-resolve-machine".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["project".to_string(), "resolve".to_string()],
            },
            input: json!({}),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, Status::Ok);
    let result = response.result.as_ref().expect("expected result");
    assert_project_resolve_shape(result, root);
}

#[test]
fn machine_channel_project_list_uses_context_project_policy_path() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        "[projects.fixture-project]\nstate_root = \"shadow\"\n",
    )
    .expect("write fixture policy");
    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&repo)
        .expect("resolve fixture project");

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "project-list-context-policy".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["project".to_string(), "list".to_string()],
            },
            input: json!({}),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = exo::api::handler::handle_request_with_project(&repo, Some(&project), request);
    assert_eq!(response.status, Status::Ok, "{response:#?}");
    let result = response.result.as_ref().expect("expected result");
    let projects = result["projects"].as_array().expect("projects array");
    assert!(
        projects
            .iter()
            .any(|project| project["id"] == "fixture-project"),
        "{projects:?}"
    );
}

#[test]
fn machine_channel_project_snapshot_uses_context_project_policy_path() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let workspace = temp.path().join("fixture-workspace");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let project_dir = sidecar_root.join("projects").join("fixture");
    let db_path = project_dir.join("cache").join("exo.db");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create sidecar cache");
    git_init(&repo);
    git_init(&workspace);
    std::fs::write(
        project_dir.join("sidecar.toml"),
        "[sidecar]\nkey = \"fixture\"\nproject_id = \"fixture-project\"\n",
    )
    .expect("write sidecar manifest");
    let writer = SqliteWriter::open(&db_path).expect("open sidecar db");
    let epoch_id = writer
        .add_epoch("Fixture Epoch", None, &[])
        .expect("add fixture epoch");
    let phase_id = writer
        .add_phase(&epoch_id, "Fixture Phase", "regular", None, &[])
        .expect("add fixture phase");
    writer
        .update_phase_status(&phase_id, "in-progress")
        .expect("activate fixture phase");
    writer
        .set_workspace_active_phase(&workspace.to_string_lossy(), &phase_id)
        .expect("set fixture workspace active phase");

    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy parent");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.fixture-project]\nstate = \"sidecar\"\nsidecar_key = \"fixture\"\nsidecar_root = {:?}\n",
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write fixture policy");
    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&repo)
        .expect("resolve fixture project");

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "project-snapshot-context-policy".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["project".to_string(), "snapshot".to_string()],
            },
            input: json!({ "id": "fixture-project" }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = exo::api::handler::handle_request_with_project(&repo, Some(&project), request);
    assert_eq!(response.status, Status::Ok, "{response:#?}");
    let result = response.result.as_ref().expect("expected result");
    assert_eq!(result["project"]["id"], "fixture-project");
    assert_eq!(result["roots"]["status"]["phase_title"], "Fixture Phase");
}

#[test]
fn cli_json_project_resolve_returns_stable_schema_in_initialized_repo() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    let output = exo_cmd(root)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");

    assert_eq!(envelope["status"], "ok");
    assert_project_resolve_shape(&envelope["result"], root);
}

#[test]
fn cli_json_project_resolve_bootstraps_without_exo_context() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);

    let output = exo_cmd(root)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");

    assert_eq!(envelope["status"], "ok");
    assert_project_resolve_shape(&envelope["result"], root);
    assert!(!root.join(".exo").exists());
    assert!(!root.join("docs").exists());
    assert!(!root.join("exosuit.toml").exists());
}

#[test]
fn cli_json_project_resolve_primary_and_linked_worktree_share_project_paths() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let primary = temp.path().join("primary");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&primary).expect("create primary");
    git_init(&primary);
    exo_init(&primary);
    git_commit_all(&primary);
    git_worktree_add(&primary, &linked);

    let primary_result = cli_project_resolve(&primary);
    let linked_result = cli_project_resolve(&linked);

    assert_eq!(
        primary_result["project"]["id"],
        linked_result["project"]["id"]
    );
    assert_eq!(
        primary_result["project"]["git_common_dir"],
        linked_result["project"]["git_common_dir"]
    );

    let primary_workspace = primary.canonicalize().expect("canonical primary");
    let linked_workspace = linked.canonicalize().expect("canonical linked");
    assert_ne!(primary_workspace, linked_workspace);
    assert_eq!(
        json_path(&primary_result, "project", "workspace_root"),
        primary_workspace
    );
    assert_eq!(
        json_path(&linked_result, "project", "workspace_root"),
        linked_workspace
    );

    for field in [
        "state_root",
        "db_path",
        "runtime_dir",
        "socket_path",
        "endpoint",
        "pid_path",
    ] {
        assert_eq!(
            primary_result["paths"][field], linked_result["paths"][field],
            "{field} should be project-scoped"
        );
    }

    let primary_state_root = primary_workspace.join(".exo");
    assert_eq!(
        json_path(&primary_result, "paths", "state_root"),
        primary_state_root
    );
    assert_eq!(
        json_path(&linked_result, "paths", "state_root"),
        primary_state_root
    );
    assert_ne!(
        json_path(&linked_result, "paths", "state_root"),
        linked_workspace.join(".exo")
    );

    assert!(
        !legacy_root_db_path(&primary).exists(),
        "project resolve must not create primary legacy root DB"
    );
    assert!(
        !legacy_root_db_path(&linked).exists(),
        "project resolve must not create linked legacy root DB"
    );
}

#[test]
fn cli_json_project_resolve_non_git_error_is_friendly() {
    let temp = tempfile::tempdir().expect("create tempdir");

    let output = exo_cmd(temp.path())
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");

    assert_eq!(envelope["status"], "error");
    let error = envelope["error"].as_object().expect("error body");
    assert_eq!(
        error.get("code"),
        Some(&json!(ErrorCode::PreconditionFailed))
    );
    let message = error
        .get("message")
        .and_then(JsonValue::as_str)
        .expect("error message");
    assert!(message.contains("requires a git repository"));
    assert!(message.contains("git init"));
    assert!(!message.contains("fatal:"));
}

#[test]
fn cli_init_writes_project_db_path() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);

    exo_init(root);

    assert!(project_db_path(root).exists(), "project DB should exist");
    assert!(
        !legacy_root_db_path(root).exists(),
        "legacy root DB should not be created by init"
    );
}

#[test]
fn direct_cli_reads_project_db_not_legacy_root_db() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    exo_cmd(root)
        .args(["epoch", "add", "--title", "Project DB Epoch"])
        .assert()
        .success();

    let legacy_db = legacy_root_db_path(root);
    std::fs::create_dir_all(legacy_db.parent().expect("legacy DB parent"))
        .expect("create legacy root cache dir");
    SqliteWriter::open(&legacy_db)
        .expect("open legacy root DB")
        .add_epoch("Legacy Root DB Epoch", None, &[])
        .expect("write legacy root DB epoch");

    let output = exo_cmd(root)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    let rendered = serde_json::to_string(&envelope).expect("serialize envelope");

    assert!(rendered.contains("Project DB Epoch"));
    assert!(!rendered.contains("Legacy Root DB Epoch"));
    assert_db_has_epoch(&project_db_path(root), "Project DB Epoch");
    assert_db_has_epoch(&legacy_db, "Legacy Root DB Epoch");
    assert_db_lacks_epoch(&project_db_path(root), "Legacy Root DB Epoch");
}

#[test]
fn machine_channel_writes_project_db_and_dumps_from_project_db() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "machine-project-db-write".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["epoch".to_string(), "add".to_string()],
            },
            input: json!({ "title": "Machine Project DB Epoch" }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, Status::Ok);

    let project_db = project_db_path(root);
    assert_db_has_epoch(&project_db, "Machine Project DB Epoch");
    assert!(
        !legacy_root_db_path(root).exists(),
        "machine-channel write should not create legacy root DB"
    );

    let epochs_dump = std::fs::read_to_string(root.join("docs/agent-context/epochs.sql"))
        .expect("read epochs dump");
    assert!(epochs_dump.contains("Machine Project DB Epoch"));
}

#[test]
fn machine_channel_pure_command_does_not_rewrite_sql_dumps() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    let dump_path = root.join("docs/agent-context/epochs.sql");
    let before = std::fs::read_to_string(&dump_path).expect("read initial epochs dump");
    std::fs::write(&dump_path, "sentinel dump\n").expect("write sentinel dump");

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "machine-pure-no-dump".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["epoch".to_string(), "list".to_string()],
            },
            input: json!({}),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, Status::Ok);
    assert_eq!(response.effect, Some(exo::api::protocol::Effect::Pure));
    assert_eq!(
        std::fs::read_to_string(&dump_path).expect("read sentinel epochs dump"),
        "sentinel dump\n"
    );

    std::fs::write(&dump_path, before).expect("restore initial dump");
}

#[test]
fn machine_channel_sidecar_repo_status_is_pure_and_does_not_rewrite_sql_dumps() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    git_init(root);
    exo_init(root);
    let default_project = Project::resolve(root).expect("resolve default project");
    let sidecar_root = temp.path().join("sidecars");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy dir");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{}]\nstate = \"sidecar\"\nsidecar_key = \"locald\"\nsidecar_root = {:?}\n",
            default_project.id.as_str(),
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write sidecar policy");
    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(root)
        .expect("resolve sidecar project");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    run_git_ok(&sidecar_root, &["init"]);

    let dump_path = root.join("docs/agent-context/epochs.sql");
    let before = std::fs::read_to_string(&dump_path).expect("read initial epochs dump");
    std::fs::write(&dump_path, "sentinel dump\n").expect("write sentinel dump");

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "machine-sidecar-repo-status-pure".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["sidecar".to_string(), "repo".to_string()],
            },
            input: json!({ "action": "status" }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = exo::api::handler::handle_request_with_project(root, Some(&project), request);
    assert_eq!(response.status, Status::Ok, "{response:#?}");
    assert_eq!(response.effect, Some(exo::api::protocol::Effect::Pure));
    assert_eq!(
        std::fs::read_to_string(&dump_path).expect("read sentinel epochs dump"),
        "sentinel dump\n"
    );

    std::fs::write(&dump_path, before).expect("restore initial dump");
}

#[test]
fn machine_channel_exec_command_rewrites_sql_dumps() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    let dump_path = root.join("docs/agent-context/goals.sql");
    let before = std::fs::read_to_string(&dump_path).expect("read initial goals dump");
    std::fs::write(&dump_path, "sentinel dump\n").expect("write sentinel dump");

    let request = test_support::confirmed_machine_channel_request(RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "machine-exec-rewrite-dump".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["strike".to_string(), "start".to_string()],
            },
            input: json!({
                "name": "dump-rewrite-regression",
                "goal": "prove exec commands rewrite SQL dumps"
            }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    });

    let response = run_machine_channel_in_process(root, &request);
    assert_eq!(response.status, Status::Ok);
    assert_eq!(response.effect, Some(exo::api::protocol::Effect::Exec));
    let rewritten = std::fs::read_to_string(&dump_path).expect("read rewritten goals dump");
    assert_ne!(rewritten, "sentinel dump\n");
    assert!(rewritten.contains("dump-rewrite-regression"));

    std::fs::write(&dump_path, before).expect("restore initial dump");
}

#[test]
fn direct_pure_command_does_not_rewrite_sql_dumps() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    let dump_path = root.join("docs/agent-context/epochs.sql");
    let before = std::fs::read_to_string(&dump_path).expect("read initial epochs dump");
    std::fs::write(&dump_path, "sentinel dump\n").expect("write sentinel dump");

    exo_cmd(root)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(&dump_path).expect("read sentinel epochs dump"),
        "sentinel dump\n"
    );

    std::fs::write(&dump_path, before).expect("restore initial dump");
}

#[test]
fn machine_channel_write_mutation_auto_commits_sidecar_repo() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    run_git_ok(&sidecar_root, &["init"]);
    run_git_ok(
        &sidecar_root,
        &["config", "user.email", "exo-tests@example.invalid"],
    );
    run_git_ok(&sidecar_root, &["config", "user.name", "Exo Tests"]);
    run_git_ok(&sidecar_root, &["branch", "-M", "main"]);

    let default_project = Project::resolve(&repo).expect("resolve default project");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy dir");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{}]\nstate = \"sidecar\"\nsidecar_key = \"machine-sidecar\"\nsidecar_root = {:?}\n",
            default_project.id.as_str(),
            sidecar_root.to_string_lossy()
        ),
    )
    .expect("write sidecar policy");

    let mut init = assert_cmd::cargo::cargo_bin_cmd!("exo");
    init.current_dir(&repo)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args(["--direct", "init", "--defaults"])
        .assert()
        .success();
    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&repo)
        .expect("resolve sidecar project after init");
    assert_eq!(project.sidecar_auto_commit, true);
    assert_eq!(project.sidecar_auto_push, SidecarAutoPushPolicy::IfRemote);
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "machine-sidecar-auto-persist".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["epoch".to_string(), "add".to_string()],
            },
            input: json!({ "title": "Machine Sidecar Auto Commit" }),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = exo::api::handler::handle_request_with_project(&repo, Some(&project), request);
    assert_eq!(response.status, Status::Ok, "{response:#?}");

    let status = run_git_output(
        &sidecar_root,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert_eq!(status, "", "{response:#?}");
    let log = run_git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    let committed = run_git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/machine-sidecar/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Machine Sidecar Auto Commit"));
}

#[test]
fn fresh_clone_imports_sql_dumps_into_project_db() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    git_init(root);
    exo_init(root);

    exo_cmd(root)
        .args(["epoch", "add", "--title", "Dump Only Epoch"])
        .assert()
        .success();
    exo::context::write_sql_dump(root);

    let project_db = project_db_path(root);
    std::fs::remove_file(&project_db).expect("delete project DB");
    assert!(!project_db.exists());
    assert!(root.join("exosuit.toml").exists());
    assert!(root.join("docs/agent-context/epochs.sql").exists());

    let context = AgentContext::load(root.to_path_buf()).expect("load from SQL dumps");
    assert!(
        context
            .plan
            .epochs
            .iter()
            .any(|epoch| epoch.title == "Dump Only Epoch")
    );
    assert!(project_db.exists(), "project DB should be recreated");
    assert!(
        !legacy_root_db_path(root).exists(),
        "fresh-clone import should not recreate legacy root DB"
    );
}

#[test]
fn project_snapshot_imports_repo_sql_projection_into_missing_db() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    git_init(root);
    exo_init(root);

    exo_cmd(root)
        .args(["epoch", "add", "--title", "Snapshot Import Epoch"])
        .assert()
        .success();
    exo::context::write_sql_dump(root);

    let project_id = Project::resolve(root)
        .expect("resolve project")
        .id
        .as_str()
        .to_string();
    let project_db = project_db_path(root);
    std::fs::remove_file(&project_db).expect("delete project DB");
    assert!(!project_db.exists());
    assert!(root.join("docs/agent-context/epochs.sql").exists());

    let snapshot = cli_project_snapshot_with_env(root, &home, &config_home, &project_id);

    assert_eq!(snapshot["kind"], "project.snapshot");
    assert_eq!(snapshot["ok"], true);
    assert!(
        project_db.exists(),
        "project snapshot should recreate repo DB"
    );
    let snapshot_text = serde_json::to_string(&snapshot).expect("serialize snapshot");
    assert!(
        snapshot_text.contains("Snapshot Import Epoch"),
        "{snapshot_text}"
    );
}

#[test]
fn project_snapshot_reconciles_rfc_markdown_before_loading_pipeline() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    git_init(root);
    exo_init(root);
    exo_rfc_create(
        root,
        "Initial Snapshot RFC",
        "0001",
        "0",
        "General",
        Some("Body."),
    );
    exo_rfc_create(
        root,
        "Unattached Snapshot RFC",
        "0002",
        "0",
        "General",
        Some("Body."),
    );
    let phase_id = exo_active_phase_id(root);
    exo_cmd(root)
        .args(["phase", "update", &phase_id, "--rfcs", "0001"])
        .assert()
        .success();

    let project_id = Project::resolve(root)
        .expect("resolve project")
        .id
        .as_str()
        .to_string();
    let stage_dir = root.join("docs/rfcs/stage-0");
    let rfc_path = std::fs::read_dir(&stage_dir)
        .expect("read stage dir")
        .map(|entry| entry.expect("read entry").path())
        .find(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("md")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("0001-"))
        })
        .expect("created RFC markdown");
    let content = std::fs::read_to_string(&rfc_path).expect("read RFC markdown");
    std::fs::write(
        &rfc_path,
        content.replace("Initial Snapshot RFC", "Updated Snapshot RFC"),
    )
    .expect("update RFC title on disk");

    let snapshot = cli_project_snapshot_with_env(root, &home, &config_home, &project_id);
    let pipeline =
        serde_json::to_string(&snapshot["roots"]["rfc-pipeline"]).expect("serialize rfc pipeline");
    assert!(
        pipeline.contains("Updated Snapshot RFC"),
        "snapshot should reconcile RFC markdown before loading pipeline: {pipeline}"
    );
    assert!(
        !pipeline.contains("Unattached Snapshot RFC"),
        "snapshot RFC pipeline should stay scoped to active phase RFC attachments: {pipeline}"
    );
}

#[test]
fn shadow_policy_uses_shadow_project_db_path() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");
    let root = repo.as_path();
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    git_init(root);

    let default_project = Project::resolve(root).expect("resolve default project");
    let policy_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(policy_path.parent().expect("policy parent"))
        .expect("create policy dir");
    std::fs::write(
        &policy_path,
        format!(
            "[projects.{}]\nstate = \"shadow\"\n",
            default_project.id.as_str()
        ),
    )
    .expect("write shadow policy");

    let shadow_project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(root)
        .expect("resolve shadow project");

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .args(["--direct", "init", "--defaults"])
        .assert()
        .success();

    assert!(shadow_project.db_path().exists(), "shadow DB should exist");
    assert_eq!(
        shadow_project.runtime_dir(),
        home.join(".exo")
            .join("projects")
            .join(shadow_project.id.as_str())
            .join("runtime"),
        "shadow daemon runtime should live under HOME-scoped project state"
    );
    let expected_default_socket_path = shadow_project.runtime_dir().join("daemon.sock");
    if expected_default_socket_path.to_string_lossy().len()
        < exo::project::MAX_PORTABLE_UNIX_SOCKET_PATH_LEN
    {
        assert_eq!(shadow_project.socket_path(), expected_default_socket_path);
    } else {
        assert!(
            shadow_project
                .socket_path()
                .starts_with("/tmp/exo-daemon-sockets"),
            "long shadow daemon socket paths should use a stable short temp socket: {}",
            shadow_project.socket_path().display()
        );
    }
    assert_eq!(
        shadow_project.pid_path(),
        shadow_project.runtime_dir().join("daemon.pid")
    );
    assert!(
        !default_project.db_path().exists(),
        "default project DB should not be created when shadow policy is active"
    );
    assert!(
        !legacy_root_db_path(root).exists(),
        "shadow init should not create legacy root DB"
    );
}

#[test]
fn machine_channel_project_resolve_non_git_error_is_precondition_failed() {
    let temp = tempfile::tempdir().expect("create tempdir");

    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        id: "project-resolve-non-git".to_string(),
        op: Op::Call(CallParams {
            address: Address::Operation {
                path: vec!["project".to_string(), "resolve".to_string()],
            },
            input: json!({}),
        }),
        auth: None,
        workflow_confirmation: None,
        agent_id: None,
    };

    let response = run_machine_channel_in_process(temp.path(), &request);
    assert_eq!(response.status, Status::Error);
    let error = response.error.as_ref().expect("error body");
    assert_eq!(error.code, ErrorCode::PreconditionFailed);
    assert!(error.message.contains("requires a git repository"));
    assert!(error.message.contains("git init"));
    assert!(!error.message.contains("fatal:"));
}

#[test]
fn project_namespace_commands_are_registered_in_spec_and_help() {
    let spec = CommandSpec::from_registry(&default_registry());
    let resolve = spec
        .operation("project", "resolve")
        .expect("project resolve operation");

    assert_eq!(
        resolve.description,
        "Resolve project identity and state/runtime paths"
    );
    assert_eq!(resolve.effect, exo::api::protocol::Effect::Pure);
    assert!(resolve.args.is_empty());

    let list = spec
        .operation("project", "list")
        .expect("project list operation");
    assert_eq!(
        list.description,
        "List locally known Exo projects from project policy and sidecars"
    );
    assert_eq!(list.effect, exo::api::protocol::Effect::Pure);

    let repair = spec
        .operation("project", "repair")
        .expect("project repair operation");
    assert_eq!(repair.description, "Preview project policy repairs");
    assert_eq!(repair.effect, exo::api::protocol::Effect::Pure);
    assert!(
        repair
            .args
            .iter()
            .any(|arg| arg.name == "stale_sidecars" || arg.name == "stale-sidecars"),
        "{:?}",
        repair.args
    );
    assert!(
        repair
            .args
            .iter()
            .all(|arg| arg.name != "apply" && arg.id != "apply"),
        "{:?}",
        repair.args
    );

    let repair_apply = spec
        .operation("project", "repair-apply")
        .expect("project repair-apply operation");
    assert_eq!(
        repair_apply.description,
        "Apply project policy repairs after reviewing the preview"
    );
    assert_eq!(repair_apply.effect, exo::api::protocol::Effect::Exec);
    assert!(
        repair_apply
            .args
            .iter()
            .any(|arg| arg.name == "stale_sidecars" || arg.name == "stale-sidecars"),
        "{:?}",
        repair_apply.args
    );

    let snapshot = spec
        .operation("project", "snapshot")
        .expect("project snapshot operation");
    assert_eq!(
        snapshot.description,
        "Read project-scoped cockpit roots by project id"
    );
    assert_eq!(snapshot.effect, exo::api::protocol::Effect::Pure);
    assert!(
        snapshot.args.iter().any(|arg| arg.name == "id"),
        "{:?}",
        snapshot.args
    );

    let help = help_for_address(
        &spec,
        &Address::Namespace {
            path: vec!["project".to_string()],
        },
    )
    .expect("project namespace help");
    let help_text = serde_json::to_string(&help).expect("serialize help");
    assert!(help_text.contains("project resolve"));
    assert!(help_text.contains("project list"));
    assert!(help_text.contains("project snapshot"));
    assert!(help_text.contains("project repair"));
    assert!(help_text.contains("project repair-apply"));
}
